//! `AL_LOCALE` (spec §5.2): il comando del locale, [`RoomCommand`], e la sua
//! istantanea della geometria di fatto e di progetto ([`room_snapshot`]).
//! Punto interno, nome e numero; nella fase di fatto consegna contorno e
//! testo, in quella di progetto il solo seme (spec §4.2). Il ricalcolo dei
//! locali scritti (`AL_AGGIORNA`) sta in [`super::refresh`].

use std::iter;

use codec::{CadDocument, Handle};
use glam::DVec3;
use kernel::geom2d::{contains, distance_to, Tolerance};
use rustc_hash::FxHashMap;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::BoundarySource;

use super::project::project_geometry;
use super::report::Notice;
use super::rooms::{
    measure_at, quality_notices, region_holds, ring_curves, room_entities, rooms_in, seed_entity,
    Computed, Measured, Phase, Room,
};
use super::units::{format_m2, DrawingUnit};
use super::walls::{faces as wall_faces, sources_of, wall_set_without_rooms};
use super::zone::{zones_in, ZoneState};

/// L'altezza dei testi dei locali sta in `rooms`, perché la usa anche
/// l'estrazione (che non dipende dai comandi).
pub use super::rooms::{room_text_height, TextFallback, DEFAULT_ROOM_TEXT_M};

/// La geometria che [`RoomCommand`] legge, preparata all'avvio
/// ([`room_snapshot`]). In unità del disegno, sul piano XY del mondo.
#[derive(Clone)]
pub struct RoomSnapshot {
    /// Le facce (`walls::faces` a `tol`) e i muri del rilievo.
    pub fatto: (Vec<Vec<[f64; 2]>>, FxHashMap<Handle, BoundarySource>),
    /// Le stesse sulla geometria di progetto (`project::project_geometry`, con
    /// le soglie dei varchi).
    pub progetto: (Vec<Vec<[f64; 2]>>, FxHashMap<Handle, BoundarySource>),
    /// Gli anelli delle zone nuove, contorni e buchi insieme, senza doppioni:
    /// un punto sta in un muro nuovo se è sul bordo di un anello o dentro un
    /// numero dispari di anelli. Due zone nuove sovrapposte solo in parte (la
    /// convalida lo segnala) si annullano nella parte comune.
    pub new_zone_rings: Vec<Vec<[f64; 2]>>,
    pub unit: DrawingUnit,
    /// La tolleranza dei locali: `max(unit.tolerance(), HPGAPTOL)`.
    pub tol: f64,
    /// L'altezza dei testi nuovi ([`room_text_height`]).
    pub text_height: f64,
    /// I locali di fatto già nel disegno, uno per locale: un locale di fatto
    /// la cui regione contiene il seme di un altro li fonde (R54).
    pub fatto_rooms: Vec<Room>,
}

/// L'istantanea di `AL_LOCALE` per `doc` alla tolleranza dei locali `tol`, e
/// quante entità muro restano fuori dal piano XY (ignorate). La geometria di
/// progetto si taglia alla tolleranza dell'unità, come nell'estrazione; le
/// facce si saldano a `tol`.
pub fn room_snapshot(doc: &CadDocument, unit: DrawingUnit, tol: f64) -> (RoomSnapshot, usize) {
    let base = unit.tolerance();
    // R67: le parti dei locali non sono muri, né del rilievo né della
    // geometria di progetto.
    let walls = wall_set_without_rooms(doc, base);
    let zones = zones_in(doc);
    // Il layer dei muri nuovi non cambia la geometria dei locali.
    let pg = project_geometry(doc, &walls, &zones, "0", base);
    let (pg_sources, _) = sources_of(&pg.room_entities, base);
    let pg_faces = wall_faces(&pg_sources, tol);
    let fatto_faces = wall_faces(&walls.sources, tol);
    // Una zona nuova data due volte (lo stesso muro) conta una volta: per la
    // regola pari-dispari due anelli uguali si annullerebbero.
    let mut new_zone_rings: Vec<Vec<[f64; 2]>> = Vec::new();
    for ring in zones
        .iter()
        .filter(|zone| zone.zone.state == ZoneState::Nuovo)
        .flat_map(|zone| zone.rings.iter())
    {
        if !new_zone_rings.iter().any(|seen| same_ring(seen, ring, tol)) {
            new_zone_rings.push(ring.clone());
        }
    }
    let snap = RoomSnapshot {
        fatto: (fatto_faces, walls.sources),
        progetto: (pg_faces, pg_sources),
        new_zone_rings,
        unit,
        tol,
        text_height: room_text_height(doc, unit).0,
        fatto_rooms: fatto_rooms(doc, tol),
    };
    (snap, walls.off_plane)
}

/// Lo stesso anello entro `tol`, da qualunque vertice e in tutti e due i versi.
fn same_ring(a: &[[f64; 2]], b: &[[f64; 2]], tol: f64) -> bool {
    let n = a.len();
    let rotated = |b: &[[f64; 2]]| {
        (0..n).any(|shift| (0..n).all(|k| distance(a[(k + shift) % n], b[k]) <= tol))
    };
    let reversed: Vec<[f64; 2]> = b.iter().rev().copied().collect();
    n == b.len() && (n == 0 || rotated(b) || rotated(&reversed))
}

/// I locali di fatto del disegno, uno per locale: contorno e testo dello
/// stesso locale (stesso nome, seme entro `tol`) contano una volta.
fn fatto_rooms(doc: &CadDocument, tol: f64) -> Vec<Room> {
    let mut rooms: Vec<Room> = Vec::new();
    for (_, room, _) in rooms_in(doc) {
        let twin = rooms
            .iter()
            .any(|seen| seen.name == room.name && distance(seen.seed, room.seed) <= tol);
        if room.phase == Phase::Fatto && !twin {
            rooms.push(room);
        }
    }
    rooms
}

/// Perché il punto di prima non ha dato un locale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RoomMiss {
    /// Nessuna faccia chiusa attorno, o muri dentro (R50).
    NotClosed,
    /// Un seme di progetto dentro una zona nuova.
    InNewWall,
}

/// Il locale trovato attorno al punto.
struct FoundRoom {
    seed: [f64; 2],
    measured: Measured,
    /// I locali di fatto già scritti con il seme in questa regione (R54).
    others: Vec<String>,
}

/// I passi di [`RoomCommand`].
enum RoomStep {
    /// Punto interno, o la parola chiave della fase; `miss` se il punto di
    /// prima non ha dato un locale.
    Point {
        miss: Option<RoomMiss>,
    },
    /// Nome; `empty` dopo un Invio a vuoto.
    Name {
        found: FoundRoom,
        empty: bool,
    },
    Number {
        found: FoundRoom,
        name: String,
    },
    /// Locale consegnato.
    Done,
}

/// `AL_LOCALE` (spec §5.2): punto interno, nome e numero. Nella fase di fatto
/// consegna contorno e testo misurati sul rilievo; in quella di progetto
/// mostra la superficie sulla geometria di progetto e consegna il solo seme
/// (spec §4.2). Parte nella fase di fatto; `F` la cambia.
pub struct RoomCommand {
    snap: RoomSnapshot,
    phase: Phase,
    step: RoomStep,
}

impl RoomCommand {
    pub fn new(snap: RoomSnapshot) -> Self {
        Self {
            snap,
            phase: Phase::Fatto,
            step: RoomStep::Point { miss: None },
        }
    }

    fn point_prompt(&self) -> String {
        let phase = match self.phase {
            Phase::Fatto => "Fatto",
            Phase::Progetto => "Progetto",
        };
        format!("AL_LOCALE ({phase})  Punto interno del locale o [Fase]:")
    }

    /// Le facce e i muri della fase.
    fn geometry(&self) -> (&[Vec<[f64; 2]>], &FxHashMap<Handle, BoundarySource>) {
        let (faces, sources) = match self.phase {
            Phase::Fatto => &self.snap.fatto,
            Phase::Progetto => &self.snap.progetto,
        };
        (faces, sources)
    }

    /// Il locale attorno a `p`, con l'area intera (archi veri dove si può,
    /// avvisi della tolleranza: `rooms::measure_at`).
    fn pick(&mut self, p: [f64; 2]) -> CmdResult {
        let tol = self.snap.tol;
        if self.phase == Phase::Progetto && inside_new_walls(&self.snap.new_zone_rings, p, tol) {
            self.step = RoomStep::Point {
                miss: Some(RoomMiss::InNewWall),
            };
            return CmdResult::NeedPoint;
        }
        let (faces, sources) = self.geometry();
        let Some(measured) = measure_at(faces, None, sources, p, self.snap.unit, tol) else {
            self.step = RoomStep::Point {
                miss: Some(RoomMiss::NotClosed),
            };
            return CmdResult::NeedPoint;
        };
        // R54: un vuoto con i due lati attaccati altrove fonde due locali senza
        // che la regola dei muri dentro lo veda. Nel progetto i semi si
        // raggruppano all'estrazione (`rooms::project_rooms`).
        let others = match self.phase {
            Phase::Fatto => self
                .snap
                .fatto_rooms
                .iter()
                .filter(|room| region_holds(&measured.region, room.seed, tol))
                .map(|room| room.name.clone())
                .collect(),
            Phase::Progetto => Vec::new(),
        };
        self.step = RoomStep::Name {
            found: FoundRoom {
                seed: p,
                measured,
                others,
            },
            empty: false,
        };
        CmdResult::NeedPoint
    }

    /// Il nome, senza spazi attorno; vuoto, lo si richiede col motivo.
    fn name_given(&mut self, text: &str) -> CmdResult {
        let name = text.trim();
        match std::mem::replace(&mut self.step, RoomStep::Done) {
            RoomStep::Name { found, .. } if !name.is_empty() => {
                self.step = RoomStep::Number {
                    found,
                    name: name.to_string(),
                };
            }
            RoomStep::Name { found, .. } => self.step = RoomStep::Name { found, empty: true },
            other => self.step = other,
        }
        CmdResult::NeedPoint
    }

    /// Il numero (anche vuoto) chiude il comando: contorno e testo nel fatto,
    /// il seme nel progetto.
    fn finish(&mut self, number: &str) -> CmdResult {
        let RoomStep::Number { found, name } = std::mem::replace(&mut self.step, RoomStep::Done)
        else {
            return CmdResult::NeedPoint;
        };
        let number = number.trim().to_string();
        let FoundRoom {
            seed,
            measured,
            others,
        } = found;
        let entities = match self.phase {
            // R59: un locale che contiene il seme di un altro (R54) porta il
            // «da confermare» anche nell'etichetta del disegno.
            Phase::Fatto => room_entities(
                &Computed {
                    name,
                    number,
                    area_m2: measured.area_m2,
                    region: measured.region,
                    confirm: !others.is_empty(),
                    exact: measured.exact,
                    islands: measured.islands,
                    seed,
                },
                Phase::Fatto,
                &self.snap.fatto.1,
                self.snap.text_height,
                self.snap.tol,
            ),
            Phase::Progetto => vec![seed_entity(
                &Room {
                    phase: Phase::Progetto,
                    name,
                    number,
                    seed,
                },
                self.snap.text_height,
            )],
        };
        CmdResult::CommitEntitiesAndExit(entities)
    }
}

/// Gli avvisi del locale trovato, col nome dato: prima la fusione con altri
/// locali di fatto (R54), poi quelli di qualità (isole, tolleranza, contorno).
fn found_notices(found: &FoundRoom, name: &str) -> Vec<Notice> {
    let m = &found.measured;
    let mut notices = Vec::new();
    if !found.others.is_empty() {
        notices.push(Notice::LocaleFuso {
            names: iter::once(name.to_string())
                .chain(found.others.iter().cloned())
                .collect(),
            area_m2: m.area_m2,
        });
    }
    notices.extend(quality_notices(name, m.exact, m.by_tolerance, m.islands));
    notices
}

/// ` (1 isola)`, ` (3 isole)`; niente senza isole.
fn islands_text(islands: usize) -> String {
    match islands {
        0 => String::new(),
        1 => " (1 isola)".into(),
        n => format!(" ({n} isole)"),
    }
}

/// `p` sta in un muro nuovo: sul bordo di un anello delle zone nuove, o
/// dentro un numero dispari di anelli (contorni e buchi insieme, vedi
/// [`RoomSnapshot::new_zone_rings`]). È la regola di `rooms::project_rooms`
/// (bordo compreso, buchi esclusi) per zone che non si sovrappongono.
fn inside_new_walls(rings: &[Vec<[f64; 2]>], p: [f64; 2], tol: f64) -> bool {
    let tolerance = Tolerance::new(tol);
    let mut inside = false;
    for ring in rings.iter().filter(|ring| ring.len() >= 3) {
        let edges = ring_curves(ring);
        if edges.iter().any(|edge| distance_to(edge, p) <= tol) {
            return true;
        }
        if contains(&edges, p, tolerance) {
            inside = !inside;
        }
    }
    inside
}

impl CadCommand for RoomCommand {
    /// Contorno e testo nascono sui layer `AL-LOCALI*`, `ByLayer`.
    fn preserve_commit_layer(&self) -> bool {
        true
    }

    fn preserve_commit_style(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "AL_LOCALE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            RoomStep::Point { miss: None } => self.point_prompt(),
            RoomStep::Point {
                miss: Some(RoomMiss::NotClosed),
            } => format!(
                "Nessun locale chiuso attorno al punto (chiudi i vani con una linea di soglia o usa HPGAPTOL). {}",
                self.point_prompt()
            ),
            RoomStep::Point {
                miss: Some(RoomMiss::InNewWall),
            } => format!("Il punto cade dentro un muro nuovo. {}", self.point_prompt()),
            RoomStep::Name { found, empty } => {
                let ask = format!(
                    "Superficie netta {}{}. Nome del locale:",
                    format_m2(found.measured.area_m2),
                    islands_text(found.measured.islands)
                );
                if *empty {
                    format!("Il nome del locale non può essere vuoto. {ask}")
                } else {
                    ask
                }
            }
            // Gli avvisi dopo la riga della superficie, quando il nome c'è.
            RoomStep::Number { found, name } => {
                let texts: Vec<String> = found_notices(found, name).iter().map(Notice::text).collect();
                if texts.is_empty() {
                    "Numero <nessuno>:".into()
                } else {
                    format!("{} Numero <nessuno>:", texts.join(" "))
                }
            }
            RoomStep::Done => String::new(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            RoomStep::Point { .. } => vec![CmdOption::new("Fase", "F")],
            _ => Vec::new(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            RoomStep::Point { .. } => self.pick([pt.x, pt.y]),
            // Un clic non è un nome né un numero.
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            RoomStep::Point { .. } | RoomStep::Done => CmdResult::Cancel,
            RoomStep::Name { .. } => self.name_given(""),
            RoomStep::Number { .. } => self.finish(""),
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match self.step {
            RoomStep::Point { .. } => {
                if !matches!(text.trim().to_uppercase().as_str(), "F" | "FASE") {
                    return None;
                }
                self.phase = match self.phase {
                    Phase::Fatto => Phase::Progetto,
                    Phase::Progetto => Phase::Fatto,
                };
                self.step = RoomStep::Point { miss: None };
                Some(CmdResult::NeedPoint)
            }
            RoomStep::Name { .. } => Some(self.name_given(text)),
            RoomStep::Number { .. } => Some(self.finish(text)),
            RoomStep::Done => None,
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            RoomStep::Name { .. } | RoomStep::Number { .. } => InputKind::FreeText,
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, RoomStep::Point { .. })
    }
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// Aiuti dei test dei locali, comuni a questo modulo e a [`super::refresh`].
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::bim::fixtures::scene_with;
    use crate::bim::units::DrawingUnit;
    use crate::scene::Scene;
    use codec::entities::{LwPolyline, LwVertex};
    use codec::{EntityType, Vector3};
    use kernel::geom2d::signed_area;

    pub(crate) const TOL: f64 = 1e-6;
    pub(crate) const M: DrawingUnit = DrawingUnit::Metri;

    pub(crate) fn scene_of(entities: Vec<EntityType>) -> Scene {
        scene_with(entities, 6)
    }

    pub(crate) fn snap_of(scene: &Scene) -> RoomSnapshot {
        room_snapshot(&scene.document, M, TOL).0
    }

    pub(crate) fn pt(x: f64, y: f64) -> DVec3 {
        DVec3::new(x, y, 0.0)
    }

    pub(crate) fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    #[track_caller]
    pub(crate) fn need(result: CmdResult) {
        assert!(matches!(result, CmdResult::NeedPoint));
    }

    #[track_caller]
    pub(crate) fn need_text(result: Option<CmdResult>) {
        assert!(matches!(result, Some(CmdResult::NeedPoint)));
    }

    #[track_caller]
    pub(crate) fn committed(result: CmdResult) -> Vec<EntityType> {
        let CmdResult::CommitEntitiesAndExit(entities) = result else {
            panic!("nessun commit")
        };
        entities
    }

    pub(crate) fn near(p: Vector3, q: [f64; 2]) -> bool {
        (p.x - q[0]).abs() < 1e-9 && (p.y - q[1]).abs() < 1e-9
    }

    /// L'area dei vertici di una polilinea (lati dritti).
    pub(crate) fn polyline_area(vertices: &[LwVertex]) -> f64 {
        let ring: Vec<[f64; 2]> = vertices
            .iter()
            .map(|v| [v.location.x, v.location.y])
            .collect();
        signed_area(&ring).abs()
    }

    /// Le entità che `AL_LOCALE` (fase di fatto, numero vuoto) consegna per
    /// il locale attorno a `seed`.
    pub(crate) fn room_of(scene: &Scene, name: &str, seed: [f64; 2]) -> Vec<EntityType> {
        let mut c = RoomCommand::new(snap_of(scene));
        need(c.on_point(pt(seed[0], seed[1])));
        need_text(c.on_text_input(name));
        committed(c.on_enter())
    }

    /// Il locale scritto nella scena: gli handle di contorno e testo.
    pub(crate) fn write_room(scene: &mut Scene, name: &str, seed: [f64; 2]) -> (Handle, Handle) {
        let handles: Vec<Handle> = room_of(scene, name, seed)
            .into_iter()
            .map(|entity| scene.add_entity(entity))
            .collect();
        (handles[0], handles[1])
    }

    /// Il tramezzo del caso A a polilinee si ferma 2 mm prima del muro nord.
    pub(crate) fn open_tramezzo(polyline: &mut LwPolyline) {
        for vertex in &mut polyline.vertices {
            if (vertex.location.y - 3.0).abs() < 1e-9 {
                vertex.location.y = 2.998;
            }
        }
    }

    pub(crate) fn is_tramezzo(entity: &EntityType) -> bool {
        matches!(entity, EntityType::LwPolyline(p) if p.vertices.iter().any(|v| {
            (v.location.x - 4.1).abs() < 1e-9 && (v.location.y - 3.0).abs() < 1e-9
        }))
    }
}

#[cfg(test)]
mod tests {
    //! `AL_LOCALE` ([`RoomCommand`]) sui casi di prova, senza l'app.
    use super::test_support::*;
    use super::*;
    use crate::bim::fixtures::{case_a, move_to_layer, pilastro, shift_tramezzo, Form};
    use crate::bim::rooms::decode;
    use crate::bim::rooms::RoomPart;
    use crate::bim::walls::{rect_from_axis, rect_from_axis_thickness, Region};
    use crate::bim::zone::{zone_hatch, Zone, ZoneOrigin};
    use crate::bim::{APP_LOCALE, LAYER_LOCALI, LAYER_LOCALI_PROGETTO, MAX_WALL_THICKNESS_M};
    use crate::scene::Scene;
    use codec::{EntityType, Vector3};

    fn case_snap(form: Form) -> RoomSnapshot {
        snap_of(&scene_of(case_a(form, M)))
    }

    /// La faccia ovest di A, (0;3)-(0;0), si ferma a (0;0,002): un vuoto
    /// d'angolo di 2 mm (Review Focus 3).
    fn open_west_face(entities: &mut [EntityType]) {
        let west = entities
            .iter_mut()
            .find_map(|e| match e {
                EntityType::Line(l) if near(l.start, [0.0, 3.0]) && near(l.end, [0.0, 0.0]) => {
                    Some(l)
                }
                _ => None,
            })
            .expect("faccia ovest di A");
        west.end = Vector3::new(0.0, 0.002, 0.0);
    }

    /// Una zona demolita «muro tra due punti» sulle facce del caso A, o nuova
    /// «linea + spessore» (`thickness`), come campitura nel disegno.
    fn zone_entity(
        scene: &Scene,
        state: ZoneState,
        p1: [f64; 2],
        p2: [f64; 2],
        thickness: f64,
    ) -> EntityType {
        let rect = match state {
            ZoneState::Demolito => {
                let walls = wall_set_without_rooms(&scene.document, TOL);
                rect_from_axis(&walls.sources, p1, p2, MAX_WALL_THICKNESS_M, TOL).expect("muro")
            }
            ZoneState::Nuovo => rect_from_axis_thickness(p1, p2, thickness).expect("rettangolo"),
        };
        let zone = Zone {
            state,
            origin: match state {
                ZoneState::Demolito => ZoneOrigin::MuroDuePunti,
                ZoneState::Nuovo => ZoneOrigin::LineaSpessore,
            },
            axis: Some(rect.axis),
            thickness: Some(rect.thickness),
            height: None,
            varco: false,
        };
        zone_hatch(&zone, &[rect.ring()])
    }

    #[test]
    fn room_prompts_phase_and_input_kinds() {
        let mut c = RoomCommand::new(case_snap(Form::Continua));
        assert_eq!(c.name(), "AL_LOCALE");
        assert_eq!(
            c.prompt(),
            "AL_LOCALE (Fatto)  Punto interno del locale o [Fase]:"
        );
        assert_eq!(c.input_kind(), InputKind::Point);
        assert!(c.point_step_accepts_keywords());
        let keywords: Vec<String> = c.options().into_iter().map(|o| o.keyword).collect();
        assert_eq!(keywords, ["F"]);
        assert!(c.on_text_input("X").is_none());

        // `F` alterna la fase.
        need_text(c.on_text_input("f"));
        assert_eq!(
            c.prompt(),
            "AL_LOCALE (Progetto)  Punto interno del locale o [Fase]:"
        );
        need_text(c.on_text_input("Fase"));
        assert_eq!(
            c.prompt(),
            "AL_LOCALE (Fatto)  Punto interno del locale o [Fase]:"
        );

        // Fuori da ogni locale: si richiede il punto con il motivo.
        need(c.on_point(pt(20.0, 20.0)));
        assert_eq!(
            c.prompt(),
            "Nessun locale chiuso attorno al punto (chiudi i vani con una linea di soglia o usa HPGAPTOL). AL_LOCALE (Fatto)  Punto interno del locale o [Fase]:"
        );
        need_text(c.on_text_input("F"));
        need(c.on_point(pt(20.0, 20.0)));
        assert_eq!(
            c.prompt(),
            "Nessun locale chiuso attorno al punto (chiudi i vani con una linea di soglia o usa HPGAPTOL). AL_LOCALE (Progetto)  Punto interno del locale o [Fase]:"
        );
        // Cambiando fase il motivo cade.
        need_text(c.on_text_input("F"));
        assert_eq!(
            c.prompt(),
            "AL_LOCALE (Fatto)  Punto interno del locale o [Fase]:"
        );

        // Il locale A: la superficie, poi il nome, in testo libero.
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(c.prompt(), "Superficie netta 12,00 m². Nome del locale:");
        assert_eq!(c.input_kind(), InputKind::FreeText);
        assert!(!c.point_step_accepts_keywords());
        assert!(c.options().is_empty());
        // Un clic non è un nome.
        need(c.on_point(pt(6.0, 1.5)));
        assert_eq!(c.prompt(), "Superficie netta 12,00 m². Nome del locale:");
        // Il nome non può essere vuoto.
        need(c.on_enter());
        let empty =
            "Il nome del locale non può essere vuoto. Superficie netta 12,00 m². Nome del locale:";
        assert_eq!(c.prompt(), empty);
        need_text(c.on_text_input("   "));
        assert_eq!(c.prompt(), empty);
        // Spazi interni e maiuscole del nome restano; poi il numero.
        need_text(c.on_text_input("  Camera da letto "));
        assert_eq!(c.prompt(), "Numero <nessuno>:");
        assert_eq!(c.input_kind(), InputKind::FreeText);
        let entities = committed(c.on_enter());
        let rec = entities[1]
            .common()
            .extended_data
            .get_record(APP_LOCALE)
            .expect("XDATA");
        assert_eq!(decode(rec).unwrap().0.name, "Camera da letto");

        // Invio al primo passo chiude il comando.
        let mut c = RoomCommand::new(case_snap(Form::Continua));
        assert!(matches!(c.on_enter(), CmdResult::Cancel));
    }

    #[test]
    fn fatto_commits_outline_and_label() {
        let snap = case_snap(Form::Continua);
        // TEXTSIZE del documento nuovo è 2,5: in un disegno in metri non è
        // plausibile (fix finale, punto 6) e il testo è alto 0,25 m.
        assert_eq!(snap.text_height, 0.25, "ripiego di TEXTSIZE");
        let mut c = RoomCommand::new(snap);
        need(c.on_point(pt(2.0, 1.5)));
        need_text(c.on_text_input("A"));
        assert_eq!(c.prompt(), "Numero <nessuno>:");
        let entities = committed(c.on_enter());
        assert_eq!(entities.len(), 2, "contorno e testo");
        let EntityType::LwPolyline(outline) = &entities[0] else {
            panic!("contorno: {:?}", entities[0])
        };
        assert_eq!(outline.common.layer, LAYER_LOCALI);
        assert!(outline.is_closed);
        assert!((polyline_area(&outline.vertices) - 12.0).abs() < 1e-9);
        let EntityType::MText(text) = &entities[1] else {
            panic!("testo: {:?}", entities[1])
        };
        assert_eq!(text.value, "A\\PS = 12,00 m²");
        assert_eq!(text.common.layer, LAYER_LOCALI);
        assert_eq!(text.height, 0.25);
        assert!(near(text.insertion_point, [2.0, 1.5]));
        let room = Room {
            phase: Phase::Fatto,
            name: "A".into(),
            number: String::new(),
            seed: [2.0, 1.5],
        };
        for (entity, part) in entities.iter().zip([RoomPart::Contorno, RoomPart::Testo]) {
            let rec = entity
                .common()
                .extended_data
                .get_record(APP_LOCALE)
                .expect("XDATA");
            assert_eq!(decode(rec), Some((room.clone(), part)));
        }
        assert!(c.preserve_commit_layer() && c.preserve_commit_style());
    }

    #[test]
    fn progetto_commits_only_the_seed() {
        let mut c = RoomCommand::new(case_snap(Form::Continua));
        need_text(c.on_text_input("F"));
        need(c.on_point(pt(3.0, 1.0)));
        assert_eq!(c.prompt(), "Superficie netta 12,00 m². Nome del locale:");
        need_text(c.on_text_input("Soggiorno"));
        let entities = committed(c.on_text_input(" 12 ").expect("numero"));
        assert_eq!(entities.len(), 1, "solo il seme");
        let EntityType::MText(seed) = &entities[0] else {
            panic!("seme: {:?}", entities[0])
        };
        assert_eq!(seed.value, "Soggiorno");
        assert_eq!(seed.common.layer, LAYER_LOCALI_PROGETTO);
        let rec = seed
            .common
            .extended_data
            .get_record(APP_LOCALE)
            .expect("XDATA");
        let want = Room {
            phase: Phase::Progetto,
            name: "Soggiorno".into(),
            number: "12".into(),
            seed: [3.0, 1.0],
        };
        assert_eq!(decode(rec), Some((want, RoomPart::Seme)));
    }

    /// Nel progetto la superficie viene dalla geometria di progetto e un seme
    /// dentro un muro nuovo si rifiuta; nel fatto le zone non contano.
    #[test]
    fn progetto_measures_the_project_and_refuses_new_walls() {
        // Tramezzo demolito: nel progetto A e B sono un locale solo.
        let mut scene = scene_of(case_a(Form::Continua, M));
        let zone = zone_entity(&scene, ZoneState::Demolito, [4.05, 0.0], [4.05, 3.0], 0.0);
        scene.add_entity(zone);
        let snap = snap_of(&scene);
        let mut c = RoomCommand::new(snap.clone());
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(
            c.prompt(),
            "Superficie netta 12,00 m². Nome del locale:",
            "il fatto non vede le zone"
        );
        let mut c = RoomCommand::new(snap);
        need_text(c.on_text_input("F"));
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(c.prompt(), "Superficie netta 24,00 m². Nome del locale:");

        // Muro nuovo da 0,12 sull'asse (2;0)-(2;3), dato due volte: per la
        // regola pari-dispari conta una volta sola.
        let mut scene = scene_of(case_a(Form::Continua, M));
        let zone = zone_entity(&scene, ZoneState::Nuovo, [2.0, 0.0], [2.0, 3.0], 0.12);
        scene.add_entity(zone.clone());
        scene.add_entity(zone);
        let snap = snap_of(&scene);
        assert_eq!(snap.new_zone_rings.len(), 1, "doppione tolto");
        let mut c = RoomCommand::new(snap.clone());
        need_text(c.on_text_input("F"));
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(
            c.prompt(),
            "Il punto cade dentro un muro nuovo. AL_LOCALE (Progetto)  Punto interno del locale o [Fase]:"
        );
        assert_eq!(c.input_kind(), InputKind::Point);
        // Sul bordo del muro nuovo è lo stesso.
        need(c.on_point(pt(1.94, 1.5)));
        assert!(c
            .prompt()
            .starts_with("Il punto cade dentro un muro nuovo."));
        need(c.on_point(pt(1.0, 1.5)));
        assert_eq!(c.prompt(), "Superficie netta 5,82 m². Nome del locale:");
        need_text(c.on_text_input("Studio"));
        assert_eq!(committed(c.on_enter()).len(), 1);
        // Nel fatto il muro nuovo non c'è ancora.
        let mut c = RoomCommand::new(snap);
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(c.prompt(), "Superficie netta 12,00 m². Nome del locale:");
    }

    #[test]
    fn islands_and_tolerance_in_the_prompts() {
        // Il pilastro del caso D: l'isola nella superficie e nell'avviso.
        let mut entities = case_a(Form::Continua, M);
        entities.push(pilastro(M));
        let mut c = RoomCommand::new(snap_of(&scene_of(entities)));
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(
            c.prompt(),
            "Superficie netta 11,91 m² (1 isola). Nome del locale:"
        );
        need_text(c.on_text_input("A"));
        assert_eq!(c.prompt(), "Locale A: 1 isola sottratta. Numero <nessuno>:");

        // Review Focus 3: vuoto di 2 mm sulla faccia ovest di A.
        let mut entities = case_a(Form::Continua, M);
        open_west_face(&mut entities);
        let scene = scene_of(entities);
        let mut c = RoomCommand::new(room_snapshot(&scene.document, M, TOL).0);
        need(c.on_point(pt(2.0, 1.5)));
        assert!(
            c.prompt()
                .starts_with("Nessun locale chiuso attorno al punto"),
            "{}",
            c.prompt()
        );
        // Con HPGAPTOL a 5 mm si chiude: 12,00, ma solo per la tolleranza.
        let mut c = RoomCommand::new(room_snapshot(&scene.document, M, 0.005).0);
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(c.prompt(), "Superficie netta 12,00 m². Nome del locale:");
        need_text(c.on_text_input("A"));
        assert_eq!(
            c.prompt(),
            "Locale A: chiuso solo dalla tolleranza HPGAPTOL, superficie da verificare. Numero <nessuno>:"
        );
        let entities = committed(c.on_enter());
        let EntityType::LwPolyline(outline) = &entities[0] else {
            panic!("contorno")
        };
        assert!(
            outline.vertices.iter().all(|v| v.bulge == 0.0),
            "l'anello delle facce"
        );
        assert!((polyline_area(&outline.vertices) - 12.0).abs() < 1e-3);
        let EntityType::MText(text) = &entities[1] else {
            panic!("testo")
        };
        assert_eq!(text.value, "A\\PS = 12,00 m²");
    }

    /// R54: un vuoto con i due lati attaccati altrove fonde due locali senza
    /// che la regola dei muri dentro lo veda; il seme di un altro locale di
    /// fatto nella regione lo dice prima del numero.
    #[test]
    fn fused_room_is_noticed_before_the_number() {
        let mut entities = case_a(Form::ChiusaPerMuro, M);
        let Some(EntityType::LwPolyline(tramezzo)) =
            entities.iter_mut().find(|entity| is_tramezzo(entity))
        else {
            panic!("tramezzo")
        };
        open_tramezzo(tramezzo);
        let mut scene = scene_of(entities);
        // B scritto prima del vuoto: contorno e testo, un locale solo.
        let b = Computed {
            name: "B".into(),
            number: String::new(),
            area_m2: 11.7,
            region: Region {
                outer: rect(4.1, 0.0, 8.0, 3.0),
                holes: Vec::new(),
            },
            confirm: false,
            exact: false,
            islands: 0,
            seed: [6.0, 1.5],
        };
        for entity in room_entities(&b, Phase::Fatto, &FxHashMap::default(), 0.25, TOL) {
            scene.add_entity(entity);
        }
        let snap = snap_of(&scene);
        let b_room = Room {
            phase: Phase::Fatto,
            name: "B".into(),
            number: String::new(),
            seed: [6.0, 1.5],
        };
        assert_eq!(snap.fatto_rooms, [b_room], "uno per locale");

        let mut c = RoomCommand::new(snap.clone());
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(c.prompt(), "Superficie netta 23,70 m². Nome del locale:");
        need_text(c.on_text_input("A"));
        assert_eq!(
            c.prompt(),
            "Locali A + B fusi in uno (23,70 m²): nome da confermare. Numero <nessuno>:"
        );
        // R59: il «da confermare» va anche nell'etichetta del disegno; il
        // nome nel record resta quello dato.
        let entities = committed(c.on_enter());
        let EntityType::MText(text) = &entities[1] else {
            panic!("testo")
        };
        assert_eq!(text.value, "A (da confermare)\\PS = 23,70 m²");
        let rec = text
            .common
            .extended_data
            .get_record(APP_LOCALE)
            .expect("XDATA");
        assert_eq!(decode(rec).unwrap().0.name, "A");
        // Nel progetto i semi si raggruppano all'estrazione.
        let mut c = RoomCommand::new(snap);
        need_text(c.on_text_input("F"));
        need(c.on_point(pt(2.0, 1.5)));
        need_text(c.on_text_input("A"));
        assert_eq!(c.prompt(), "Numero <nessuno>:");
    }

    #[test]
    fn new_walls_rules() {
        // Una zona nuova con un buco: contorno e buco insieme, pari e dispari.
        let rings = vec![rect(0.0, 0.0, 10.0, 10.0), rect(2.0, 2.0, 8.0, 8.0)];
        assert!(inside_new_walls(&rings, [1.0, 5.0], TOL), "nel muro");
        assert!(!inside_new_walls(&rings, [5.0, 5.0], TOL), "nel buco");
        assert!(
            inside_new_walls(&rings, [2.0, 5.0], TOL),
            "sul bordo del buco"
        );
        assert!(
            inside_new_walls(&rings, [0.0, 5.0], TOL),
            "sul bordo esterno"
        );
        assert!(!inside_new_walls(&rings, [11.0, 5.0], TOL), "fuori");
        assert!(!inside_new_walls(&[], [1.0, 1.0], TOL));
        // I doppioni si riconoscono da qualunque vertice e in tutti e due i versi.
        let square = rect(0.0, 0.0, 1.0, 1.0);
        let mut turned: Vec<[f64; 2]> = square.iter().rev().copied().collect();
        turned.rotate_left(2);
        assert!(same_ring(&square, &turned, TOL));
        assert!(!same_ring(&square, &rect(0.0, 0.0, 1.0, 2.0), TOL));
        assert!(!same_ring(&square, &square[..3], TOL));
    }

    /// R67: contorno e testo di A spostati a mano su LOCALI, poi il rilievo
    /// corretto (A da 12,00 a 12,60 m²). `AL_LOCALE` offre l'area vera nelle
    /// due fasi: il contorno vecchio non è un muro né del rilievo né della
    /// geometria di progetto. Prima offriva 12,00.
    #[test]
    fn moved_room_outline_does_not_split_the_room() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let mut scene = scene_of(case_a(form, M));
            let (outline, text) = write_room(&mut scene, "A", [2.0, 1.5]);
            for handle in [outline, text] {
                move_to_layer(&mut scene.document, handle, "LOCALI");
            }
            shift_tramezzo(&mut scene.document, M, 0.2);

            let (snap, off_plane) = room_snapshot(&scene.document, M, TOL);
            assert_eq!(off_plane, 0, "{form:?}");
            assert!(!snap.fatto.1.contains_key(&outline), "{form:?}: fatto");
            assert!(
                !snap.progetto.1.contains_key(&outline),
                "{form:?}: progetto"
            );
            let mut c = RoomCommand::new(snap.clone());
            need(c.on_point(pt(2.0, 1.5)));
            assert_eq!(
                c.prompt(),
                "Superficie netta 12,60 m². Nome del locale:",
                "{form:?}: fatto"
            );
            let mut c = RoomCommand::new(snap);
            need_text(c.on_text_input("F"));
            need(c.on_point(pt(2.0, 1.5)));
            assert_eq!(
                c.prompt(),
                "Superficie netta 12,60 m². Nome del locale:",
                "{form:?}: progetto"
            );
        }
    }
}
