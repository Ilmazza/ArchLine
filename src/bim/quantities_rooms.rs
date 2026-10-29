//! Quantità dei locali (spec parte 2, §4.1, §4.3): una riga per locale di
//! fatto e una per locale di progetto, con superficie netta, perimetro,
//! altezza risolta con il suo stato e volume. Logica pura.
//!
//! Le misure sono quelle della parte 1, mai le etichette scritte (R61): i
//! locali di fatto come `AL_AGGIORNA` (`refresh::measure_fatto`), i locali di
//! progetto come lo stato di progetto di `AL_ESTRAI`
//! (`extract::computed_project_rooms` su `project::project_geometry`).
//!
//! Regole:
//! - superficie netta della parte 1 (isole sottratte); perimetro = lunghezza
//!   dell'anello esterno della regione, isole escluse; volume = superficie ×
//!   altezza, `n.d.` se manca l'altezza;
//! - ogni grandezza ha il suo stato (spec §4.2). La superficie non è esatta
//!   con un contorno curvo che la parte 1 non sa misurare sulle curve vere, o
//!   con un locale chiuso solo da `HPGAPTOL` (R47): `approximate` e la nota.
//!   Il perimetro si misura sempre sull'anello delle facce, dove gli archi
//!   sono corde: con un arco nell'anello esterno è più corto del vero (circa
//!   7·10⁻⁴ della lunghezza dell'arco; 9 mm su un cerchio di raggio 2 m), e
//!   allora `perimeter_approximate` e la nota, anche se la superficie è
//!   esatta (Ruling L);
//! - un locale di fatto non chiuso resta con superficie, perimetro e volume
//!   `n.d.`, la nota e [`Notice::LocaleNonChiuso`]; due locali di fatto fusi
//!   da un vuoto (R54, R55) lo stesso, con la nota «fuso con …» e un
//!   [`Notice::LocaleFuso`] per gruppo: l'area unita non è di nessuno dei due;
//!   un seme (di fatto o di progetto) senza locale chiuso nella geometria di
//!   progetto dà una riga di progetto «non chiuso», non sparisce (Ruling M);
//! - altezza di un locale di fatto: la sua, poi la predefinita
//!   (`height::resolve`); di un locale di progetto: quella del suo seme, poi
//!   quella dei locali di fatto i cui semi cadono nella sua regione, se
//!   concordi, poi la predefinita (`height::resolve_project_room`);
//! - gli avvisi della geometria di progetto (`project::project_geometry`)
//!   escono con le righe di progetto, come in `AL_ESTRAI`.

use codec::{Document, Handle};
use kernel::geom2d::{distance_to, Curve};
use rustc_hash::FxHashMap;

use crate::scene::BoundarySource;

use super::extract::{computed_project_rooms, push_unique, rooms_tolerance};
use super::height::{resolve, resolve_project_room, Height};
use super::project::project_geometry;
use super::refresh::measure_fatto;
use super::report::Notice;
use super::rooms::{
    closed_region, distinct, in_new_zone, quality_notices, region_holds, rooms_in, Phase, RoomPart,
};
use super::units::DrawingUnit;
use super::walls::{wall_set_without_rooms, Region};
use super::zone::zones_in;

/// Il layer dei muri nuovi per la geometria di progetto: non cambia la forma
/// dei locali, solo il layer delle linee nuove, che qui non si disegnano.
const NEW_WALLS_LAYER: &str = "0";

/// Quante tolleranze può distare dal suo muro dritto un punto di un lato
/// dell'anello esterno (la saldatura delle facce, come in `walls`).
const ON_WALL_SLACK: f64 = 4.0;

const NOTE_OPEN: &str = "non chiuso";
const NOTE_CONFIRM: &str = "nome da confermare";
const NOTE_BY_TOLERANCE: &str = "chiuso solo da HPGAPTOL: superficie da verificare";
const NOTE_CURVED: &str = "contorno curvo: superficie approssimata";
const NOTE_APPROXIMATE: &str = "superficie approssimata (contorno curvo o chiuso solo da HPGAPTOL)";
const NOTE_CHORDS: &str = "perimetro misurato sulle corde degli archi";
/// Il motivo di una riga senza superficie che non ne ha uno suo.
const REASON_AREA_ND: &str = "superficie n.d.";
/// Il motivo di un'altezza che manca senza un motivo suo.
const REASON_HEIGHT_ND: &str = "altezza n.d.";
/// Tra le parti del motivo di una riga incompleta.
const REASON_SEP: &str = "; ";
/// Tra i nomi dei locali fusi, come nei nomi composti dei locali di progetto.
const JOIN: &str = " + ";

/// Una riga di quantità di un locale. Grandezze in m, m², m³; `None` = `n.d.`.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomRow {
    pub phase: Phase,
    /// `LF1`, `LF2`… per i locali di fatto, `LP1`… per quelli di progetto,
    /// nell'ordine in cui escono.
    pub code: String,
    pub name: String,
    /// Vuoto se Mauro non l'ha dato.
    pub number: String,
    /// Il seme, in unità del disegno: accoppia fatto e progetto (Ruling B).
    pub seed: [f64; 2],
    /// Superficie netta (isole sottratte).
    pub area_m2: Option<f64>,
    /// Lunghezza dell'anello esterno, isole escluse.
    pub perimeter_m: Option<f64>,
    pub height: Height,
    /// Superficie netta × altezza.
    pub volume_m3: Option<f64>,
    /// Superficie non esatta (contorno curvo, o chiuso solo da `HPGAPTOL`):
    /// stato «approssimata».
    pub approximate: bool,
    /// Perimetro misurato sulle corde degli archi (stato «approssimata»),
    /// separato dalla superficie, che può essere esatta anche con gli archi.
    pub perimeter_approximate: bool,
    /// Note separate da «; ».
    pub note: Option<String>,
}

/// Una riga di un locale di progetto e i locali di fatto i cui semi cadono
/// nella sua regione.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRoomRow {
    pub row: RoomRow,
    /// Indici nelle righe di fatto date a [`project_room_rows`].
    pub fatto_members: Vec<usize>,
}

impl RoomRow {
    /// La quantità principale di un locale è la superficie netta (spec §4.2):
    /// solo con quella la riga entra nei totali e nel CSV. Perimetro e volume
    /// sono informazione, quindi un locale senza altezza resta completo
    /// (Ruling O).
    pub fn is_complete(&self) -> bool {
        self.area_m2.is_some()
    }

    /// Perché la riga è incompleta (`None` se ha la superficie): la nota della
    /// riga (o «superficie n.d.»), poi il motivo dell'altezza o, se manca senza
    /// un motivo suo, «altezza n.d.», senza vuoti né doppioni, come
    /// `ZoneRow::incomplete_reason`.
    pub fn incomplete_reason(&self) -> Option<String> {
        if self.is_complete() {
            return None;
        }
        let mut parts = Vec::new();
        push_reason(&mut parts, self.note.as_deref());
        if parts.is_empty() {
            parts.push(REASON_AREA_ND);
        }
        let height_note = self.height.note.as_deref();
        push_reason(&mut parts, height_note);
        if self.height.value_m.is_none() && height_note.map_or(true, |n| n.trim().is_empty()) {
            push_reason(&mut parts, Some(REASON_HEIGHT_ND));
        }
        Some(parts.join(REASON_SEP))
    }

    /// La riga senza grandezze misurate.
    fn unmeasured(
        phase: Phase,
        code: String,
        name: &str,
        number: &str,
        seed: [f64; 2],
        height: Height,
    ) -> Self {
        RoomRow {
            phase,
            code,
            name: name.to_string(),
            number: number.to_string(),
            seed,
            area_m2: None,
            perimeter_m: None,
            height,
            volume_m3: None,
            approximate: false,
            perimeter_approximate: false,
            note: None,
        }
    }

    /// Superficie, perimetro e volume dalla misura (superficie in m², regione
    /// in unità del disegno), ognuno col suo stato: la superficie da `exact`,
    /// il perimetro da [`outer_on_chords`] sulle sorgenti della misura, alla
    /// sua tolleranza `tol`. La nota del perimetro la aggiunge
    /// [`Self::note_quality`].
    fn measured(
        &mut self,
        area: f64,
        region: &Region,
        exact: bool,
        sources: &FxHashMap<Handle, BoundarySource>,
        tol: f64,
        unit: DrawingUnit,
    ) {
        self.area_m2 = Some(area);
        self.perimeter_m = Some(ring_length(&region.outer) / unit.per_metre());
        self.volume_m3 = self.height.value_m.map(|height| area * height);
        self.approximate = !exact;
        self.perimeter_approximate = outer_on_chords(region, sources, tol);
    }

    /// Le note degli stati: quella della superficie (`area_note`, se non è
    /// esatta), poi quella del perimetro sulle corde.
    fn note_quality(&mut self, area_note: Option<&str>) {
        if let Some(text) = area_note {
            self.add_note(text);
        }
        if self.perimeter_approximate {
            self.add_note(NOTE_CHORDS);
        }
    }

    fn add_note(&mut self, text: &str) {
        self.note = Some(match self.note.take() {
            Some(note) => format!("{note}; {text}"),
            None => text.to_string(),
        });
    }
}

/// Aggiunge a `parts` un testo non vuoto (senza spazi attorno) che non c'è
/// ancora.
fn push_reason<'a>(parts: &mut Vec<&'a str>, text: Option<&'a str>) {
    if let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) {
        if !parts.contains(&text) {
            parts.push(text);
        }
    }
}

/// Le righe dei locali di fatto, una per locale nell'ordine di disegno
/// (`LF1`, `LF2`…), misurati come in `AL_AGGIORNA` (`refresh::measure_fatto`)
/// alla tolleranza dei locali `room_tol` (mai sotto la tolleranza di base);
/// `default_height` in unità del disegno. Gli avvisi: [`Notice::FuoriPiano`]
/// in testa, poi locale per locale il gruppo dei fusi (una volta),
/// [`Notice::LocaleNonChiuso`] o gli avvisi di qualità
/// (`rooms::quality_notices`). Senza locali di fatto, niente.
pub fn fatto_room_rows(
    doc: &Document,
    unit: DrawingUnit,
    room_tol: f64,
    default_height: Option<f64>,
) -> (Vec<RoomRow>, Vec<Notice>) {
    let room_tol = rooms_tolerance(unit.tolerance(), room_tol);
    let mut measures = measure_fatto(doc, unit, room_tol);
    let mut notices = std::mem::take(&mut measures.notices);
    let mut rows = Vec::with_capacity(measures.rooms.len());
    for (index, fatto) in measures.rooms.iter().enumerate() {
        let room = &fatto.room;
        let mut row = RoomRow::unmeasured(
            Phase::Fatto,
            format!("LF{}", index + 1),
            &room.name,
            &room.number,
            room.seed,
            resolve(room.height, default_height, unit),
        );
        if !fatto.fused_with.is_empty() {
            // R55: la regione unita non è di nessuno dei due.
            row.add_note(&format!("fuso con {}", fatto.fused_with.join(JOIN)));
            notices.extend(measures.fused_notice(index));
        } else if let Some(measured) = &fatto.measured {
            row.measured(
                measured.area_m2,
                &measured.region,
                measured.exact,
                &measures.walls.sources,
                room_tol,
                unit,
            );
            row.note_quality(if measured.by_tolerance {
                Some(NOTE_BY_TOLERANCE)
            } else if !measured.exact {
                Some(NOTE_CURVED)
            } else {
                None
            });
            notices.extend(quality_notices(
                &room.name,
                measured.exact,
                measured.by_tolerance,
                measured.islands,
            ));
        } else {
            row.add_note(NOTE_OPEN);
            notices.push(Notice::LocaleNonChiuso {
                name: room.name.clone(),
            });
        }
        rows.push(row);
    }
    (rows, notices)
}

/// Le righe dei locali di progetto (`LP1`, `LP2`…), calcolati come nello
/// stato di progetto di `AL_ESTRAI`: `project::project_geometry` con i muri
/// senza le parti dei locali (R67) e le zone del disegno, poi
/// `extract::computed_project_rooms` (sorgenti a `tol`, locali a `room_tol`,
/// mai sotto `tol`). Ogni riga porta i locali di fatto di `fatto` (le righe di
/// [`fatto_room_rows`]) il cui seme cade nella sua regione, e la sua altezza
/// (`height::resolve_project_room`): il primo seme di progetto nella
/// regione, se ne ha una propria, altrimenti quei locali di fatto. Un nome
/// composto da semi di fatto ha la nota «nome da confermare».
///
/// Dopo i locali chiusi, una riga «non chiuso» (superficie, perimetro e
/// volume `n.d.`, nessun locale di fatto, altezza propria o predefinita) per
/// ogni seme che `rooms::project_rooms` scarta perché non ha un locale chiuso
/// attorno ([`Notice::LocaleNonChiuso`]): prima i locali di fatto, poi i semi
/// di progetto, senza doppioni, con la stessa condizione (`rooms::distinct`,
/// `rooms::in_new_zone` per i semi di progetto, `rooms::closed_region` sulle
/// stesse facce di progetto). Le facce senza seme non chiuse restano solo
/// nell'avviso «(senza nome)».
///
/// Gli avvisi, senza doppioni come in `AL_ESTRAI`: quelli della geometria di
/// progetto, [`Notice::FuoriPiano`] (lo stesso di [`fatto_room_rows`] sugli
/// stessi muri: chi li unisce toglie i doppioni), quelli dei locali di
/// progetto, poi [`Notice::AltezzeDiscordi`] per riga. Senza zone, senza
/// locali di fatto e senza semi di progetto nel disegno, niente; con le sole
/// zone, nessuna riga e gli avvisi della geometria.
pub fn project_room_rows(
    doc: &Document,
    unit: DrawingUnit,
    tol: f64,
    room_tol: f64,
    default_height: Option<f64>,
    fatto: &[RoomRow],
) -> (Vec<ProjectRoomRow>, Vec<Notice>) {
    let has_seeds = rooms_in(doc)
        .iter()
        .any(|(_, room, part)| match room.phase {
            Phase::Fatto => *part != RoomPart::Seme,
            Phase::Progetto => *part == RoomPart::Seme,
        });
    let zones = zones_in(doc);
    if !has_seeds && zones.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let room_tol = rooms_tolerance(tol, room_tol);
    let walls = wall_set_without_rooms(doc, tol);
    let pg = project_geometry(doc, &walls, &zones, NEW_WALLS_LAYER, tol);
    let rooms = computed_project_rooms(doc, &walls, &pg.room_entities, &zones, unit, tol, room_tol);
    let mut notices = Vec::new();
    push_unique(&mut notices, pg.notices);
    if walls.off_plane > 0 {
        push_unique(
            &mut notices,
            vec![Notice::FuoriPiano {
                count: walls.off_plane,
            }],
        );
    }
    push_unique(&mut notices, rooms.notices);
    let mut rows = Vec::with_capacity(rooms.computed.len());
    for room in &rooms.computed {
        let fatto_members: Vec<usize> = fatto
            .iter()
            .enumerate()
            .filter(|(_, row)| region_holds(&room.region, row.seed, tol))
            .map(|(member, _)| member)
            .collect();
        let own = rooms
            .progetto
            .iter()
            .find(|seed| region_holds(&room.region, seed.seed, tol))
            .and_then(|seed| seed.height);
        let members: Vec<(String, Height)> = fatto_members
            .iter()
            .map(|&member| (fatto[member].name.clone(), fatto[member].height.clone()))
            .collect();
        let (height, notice) = resolve_project_room(own, &members, default_height, unit);
        push_unique(&mut notices, notice.into_iter().collect());
        let mut row = RoomRow::unmeasured(
            Phase::Progetto,
            format!("LP{}", rows.len() + 1),
            &room.name,
            &room.number,
            room.seed,
            height,
        );
        row.measured(
            room.area_m2,
            &room.region,
            room.exact,
            &rooms.sources,
            room_tol,
            unit,
        );
        if room.confirm {
            row.add_note(NOTE_CONFIRM);
        }
        row.note_quality((!room.exact).then_some(NOTE_APPROXIMATE));
        rows.push(ProjectRoomRow { row, fatto_members });
    }
    // Ruling M: i semi senza locale chiuso restano tra le righe.
    let seeds = distinct(&rooms.fatto, room_tol)
        .into_iter()
        .map(|room| (room, false))
        .chain(
            distinct(&rooms.progetto, room_tol)
                .into_iter()
                .map(|room| (room, true)),
        );
    for (room, of_project) in seeds {
        if of_project && in_new_zone(&zones, room.seed, room_tol) {
            continue;
        }
        if closed_region(&rooms.faces, &rooms.sources, room.seed, unit, room_tol).is_some() {
            continue;
        }
        let mut row = RoomRow::unmeasured(
            Phase::Progetto,
            format!("LP{}", rows.len() + 1),
            &room.name,
            &room.number,
            room.seed,
            resolve(room.height, default_height, unit),
        );
        row.add_note(NOTE_OPEN);
        rows.push(ProjectRoomRow {
            row,
            fatto_members: Vec::new(),
        });
    }
    (rows, notices)
}

/// L'anello esterno della regione ha un lato che non sta su un tratto dritto
/// dei muri: una corda di un arco (o di un'ellisse, di una spline), più corta
/// della curva vera. Un lato sta su un tratto dritto se i suoi estremi e il
/// suo punto medio stanno, ognuno, entro [`ON_WALL_SLACK`] tolleranze da una
/// `Curve::Line` delle sorgenti (anche due rette in fila per un lato solo);
/// il punto medio di una corda dista dall'arco la sua freccia e dalle rette
/// dei muri molto di più. Le rette lontane dal riquadro dell'anello non si
/// guardano.
fn outer_on_chords(region: &Region, sources: &FxHashMap<Handle, BoundarySource>, tol: f64) -> bool {
    let reach = ON_WALL_SLACK * tol;
    let ring = &region.outer;
    let (lo, hi) = ring.iter().fold(
        ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]),
        |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        },
    );
    let lines: Vec<&Curve> = sources
        .values()
        .flat_map(|source| source.curves.iter())
        .filter(|curve| match curve {
            Curve::Line(line) => (0..2).all(|axis| {
                line.start[axis].min(line.end[axis]) <= hi[axis] + reach
                    && line.start[axis].max(line.end[axis]) >= lo[axis] - reach
            }),
            _ => false,
        })
        .collect();
    let on_line = |p: [f64; 2]| lines.iter().any(|line| distance_to(line, p) <= reach);
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .any(|(&a, &b)| {
            let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            !(on_line(a) && on_line(mid) && on_line(b))
        })
}

/// La lunghezza dell'anello chiuso, in unità del disegno.
fn ring_length(ring: &[[f64; 2]]) -> f64 {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(a, b)| (b[0] - a[0]).hypot(b[1] - a[1]))
        .sum()
}

#[cfg(test)]
mod tests {
    //! I casi della spec parte 2, §7 (A, D, I, L), metri, altezza predefinita
    //! 2,70, nelle due forme del rilievo.
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI};

    use codec::entities::{Arc, Line, LwPolyline};
    use codec::xdata::ExtendedDataRecord;
    use codec::{EntityType, Handle, Vector2, Vector3};

    use crate::bim::fixtures::{case_a, disturbi, pilastro, scene_with, Form, LAYER_MURI};
    use crate::bim::height::HeightState;
    use crate::bim::room_command::test_support::{is_tramezzo, open_tramezzo};
    use crate::bim::rooms::{
        encode, measure_at, room_entities, room_height_edits, seed_entity, Computed, Room,
    };
    use crate::bim::walls::{faces as wall_faces, rect_from_axis, wall_set_without_rooms};
    use crate::bim::zone::{ensure_layers, zone_hatch, Zone, ZoneOrigin, ZoneState};
    use crate::bim::{APP_LOCALE, MAX_WALL_THICKNESS_M};
    use crate::scene::Scene;

    const M: DrawingUnit = DrawingUnit::Metri;
    const TOL: f64 = 1e-6;
    /// `HPGAPTOL` dei casi con il vuoto d'angolo: 5 mm.
    const GAP: f64 = 0.005;
    const DEFAULT: Option<f64> = Some(2.7);
    const FORMS: [Form; 2] = [Form::Continua, Form::ChiusaPerMuro];

    /// Il caso A nella forma e nell'unità date (con il suo INSUNITS), più
    /// `extra`, con i layer `AL-*`.
    fn scene_in(form: Form, unit: DrawingUnit, extra: Vec<EntityType>) -> Scene {
        let mut entities = case_a(form, unit);
        entities.extend(extra);
        let mut scene = scene_with(entities, unit.insunits());
        ensure_layers(&mut scene.document);
        scene
    }

    fn scene_of(form: Form, extra: Vec<EntityType>) -> Scene {
        scene_in(form, M, extra)
    }

    /// Il locale di fatto attorno a `seed` scritto come lo scrive
    /// `AL_LOCALE`: contorno e testo misurati sul rilievo alla tolleranza
    /// `tol`. Gli handle del contorno e del testo.
    fn write_room_in(
        scene: &mut Scene,
        unit: DrawingUnit,
        name: &str,
        seed: [f64; 2],
        tol: f64,
    ) -> (Handle, Handle) {
        let walls = wall_set_without_rooms(&scene.document, unit.tolerance());
        let faces = wall_faces(&walls.sources, tol);
        let found =
            measure_at(&faces, None, &walls.sources, seed, unit, tol).expect("locale chiuso");
        let computed = Computed {
            name: name.into(),
            number: String::new(),
            area_m2: found.area_m2,
            region: found.region,
            confirm: false,
            exact: found.exact,
            islands: found.islands,
            seed,
        };
        let handles: Vec<Handle> =
            room_entities(&computed, Phase::Fatto, &walls.sources, 0.25, tol)
                .into_iter()
                .map(|entity| scene.add_entity(entity))
                .collect();
        (handles[0], handles[1])
    }

    fn write_room(scene: &mut Scene, name: &str, seed: [f64; 2]) -> (Handle, Handle) {
        write_room_in(scene, M, name, seed, TOL)
    }

    /// Il seme di un locale di progetto, come lo scrive `AL_LOCALE`.
    fn write_seed(scene: &mut Scene, name: &str, seed: [f64; 2]) -> Handle {
        let room = Room {
            phase: Phase::Progetto,
            name: name.into(),
            number: String::new(),
            seed,
            height: None,
        };
        scene.add_entity(seed_entity(&room, 0.25))
    }

    /// L'altezza `height` su tutte le parti del locale di `handle`, come la
    /// scrive `AL_ALTEZZA S`.
    fn set_height(scene: &mut Scene, handle: Handle, height: f64) {
        for (part_handle, room, part) in
            room_height_edits(&scene.document, &[handle], Some(height), TOL)
        {
            let mut record = ExtendedDataRecord::new(APP_LOCALE);
            for value in encode(&room, part) {
                record.add_value(value);
            }
            scene
                .document
                .get_entity_mut(part_handle)
                .expect("parte del locale")
                .common_mut()
                .extended_data
                .upsert_record(record);
        }
    }

    /// Il tramezzo del caso A demolito per intero (asse (4,05; 0)-(4,05; 3)),
    /// zona «muro tra due punti».
    fn demolish_tramezzo(scene: &mut Scene) {
        demolish(scene, [4.05, 0.0], [4.05, 3.0]);
    }

    /// Una zona demolita «muro tra due punti» sull'asse `p1`-`p2`.
    fn demolish(scene: &mut Scene, p1: [f64; 2], p2: [f64; 2]) {
        let walls = wall_set_without_rooms(&scene.document, TOL);
        let rect = rect_from_axis(&walls.sources, p1, p2, MAX_WALL_THICKNESS_M, TOL)
            .expect("muro sull'asse");
        let zone = Zone {
            state: ZoneState::Demolito,
            origin: ZoneOrigin::MuroDuePunti,
            axis: Some(rect.axis),
            thickness: Some(rect.thickness),
            height: None,
            varco: false,
        };
        scene.add_entity(zone_hatch(&zone, &[rect.ring()]));
    }

    /// Una zona demolita senza asse (rettangolo) sull'anello `ring`.
    fn demolish_without_axis(scene: &mut Scene, ring: Vec<[f64; 2]>) {
        let zone = Zone {
            state: ZoneState::Demolito,
            origin: ZoneOrigin::Rettangolo,
            axis: None,
            thickness: None,
            height: None,
            varco: false,
        };
        scene.add_entity(zone_hatch(&zone, &[ring]));
    }

    /// Un locale 4,00 × 3,00 con un'abside semicircolare di raggio 1,50 sul
    /// lato est (centro (4; 1,5)), muri a una linea sul layer `MURI`: tre
    /// `LINE` e un `ARC`, oppure una `LWPOLYLINE` chiusa con il lato est a
    /// bulge 1. Superficie 12 + π · 1,5² / 2, perimetro vero 11 + π · 1,5.
    fn apse(polyline: bool) -> Vec<EntityType> {
        let wall = |mut entity: EntityType| {
            entity.common_mut().layer = LAYER_MURI.to_string();
            entity
        };
        if polyline {
            let mut outline = LwPolyline::from_points(vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(4.0, 0.0),
                Vector2::new(4.0, 3.0),
                Vector2::new(0.0, 3.0),
            ]);
            outline.vertices[1].bulge = 1.0;
            outline.is_closed = true;
            return vec![wall(EntityType::LwPolyline(outline))];
        }
        let line = |a: [f64; 2], b: [f64; 2]| {
            wall(EntityType::Line(Line::from_points(
                Vector3::new(a[0], a[1], 0.0),
                Vector3::new(b[0], b[1], 0.0),
            )))
        };
        vec![
            line([0.0, 0.0], [4.0, 0.0]),
            line([4.0, 3.0], [0.0, 3.0]),
            line([0.0, 3.0], [0.0, 0.0]),
            wall(EntityType::Arc(Arc::from_center_radius_angles(
                Vector3::new(4.0, 1.5, 0.0),
                1.5,
                3.0 * FRAC_PI_2,
                FRAC_PI_2,
            ))),
        ]
    }

    fn near(p: Vector3, q: [f64; 2]) -> bool {
        (p.x - q[0]).abs() < 1e-9 && (p.y - q[1]).abs() < 1e-9
    }

    /// La faccia ovest di A: la `LINE` (0; 3)-(0; 0) del caso a linee, o la
    /// polilinea del muro ovest.
    fn west_wall_of_a(scene: &Scene) -> Handle {
        scene
            .document
            .entities()
            .find(|entity| match entity {
                EntityType::Line(l) => near(l.start, [0.0, 3.0]) && near(l.end, [0.0, 0.0]),
                EntityType::LwPolyline(p) => {
                    p.vertices.iter().any(|v| {
                        (v.location.x + 0.3).abs() < 1e-9 && (v.location.y - 3.0).abs() < 1e-9
                    }) && p.vertices.iter().all(|v| v.location.x <= 1e-9)
                }
                _ => false,
            })
            .map(|entity| entity.common().handle)
            .expect("faccia ovest di A")
    }

    /// L'angolo (0; 0) di A aperto di 2 mm nel caso a linee: la faccia ovest si
    /// ferma a y = 0,002 (Review Focus 3 della parte 1, come
    /// `case_a_with_corner_gap` dei test dell'estrazione).
    fn open_corner_of_a(scene: &mut Scene) {
        let west = west_wall_of_a(scene);
        match scene.document.get_entity_mut(west) {
            Some(EntityType::Line(line)) => line.end = Vector3::new(0.0, 0.002, 0.0),
            other => panic!("faccia ovest del caso a linee: {other:?}"),
        }
    }

    #[track_caller]
    fn approx(value: Option<f64>, want: f64, what: &str) {
        assert!(
            value.is_some_and(|v| (v - want).abs() < 1e-9),
            "{what}: {value:?} invece di {want}"
        );
    }

    #[track_caller]
    fn note_has(row: &RoomRow, text: &str, what: &str) {
        assert!(
            row.note.as_deref().is_some_and(|note| note.contains(text)),
            "{what}: nota {:?} senza «{text}»",
            row.note
        );
    }

    /// Spec §7, caso A: A 12,00 m², perimetro 14,00, volume 32,40; B 11,70,
    /// 13,80, 31,59; altezza ereditata 2,70. Il disturbo del caso F (testi,
    /// campitura, polilinea su `AL-LOCALI` senza XDATA) non cambia nulla.
    #[test]
    fn fatto_rows_case_a() {
        for form in FORMS {
            for (extra, what) in [
                (Vec::new(), "senza disturbi"),
                (disturbi(M), "con disturbi"),
            ] {
                let what = format!("{form:?} {what}");
                let mut scene = scene_of(form, extra);
                write_room(&mut scene, "A", [2.0, 1.5]);
                write_room(&mut scene, "B", [6.0, 1.5]);
                let (rows, notices) = fatto_room_rows(&scene.document, M, TOL, DEFAULT);
                assert!(notices.is_empty(), "{what}: {notices:?}");
                let [a, b] = &rows[..] else {
                    panic!("{what}: {rows:?}")
                };
                for (row, code, name, seed, area, perimeter, volume) in [
                    (a, "LF1", "A", [2.0, 1.5], 12.0, 14.0, 32.4),
                    (b, "LF2", "B", [6.0, 1.5], 11.7, 13.8, 31.59),
                ] {
                    let what = format!("{what}, {name}");
                    assert_eq!(row.phase, Phase::Fatto, "{what}");
                    assert_eq!(
                        (row.code.as_str(), row.name.as_str()),
                        (code, name),
                        "{what}"
                    );
                    assert_eq!(row.number, "", "{what}");
                    assert_eq!(row.seed, seed, "{what}");
                    approx(row.area_m2, area, &format!("{what}: superficie"));
                    approx(row.perimeter_m, perimeter, &format!("{what}: perimetro"));
                    approx(row.volume_m3, volume, &format!("{what}: volume"));
                    approx(row.height.value_m, 2.7, &format!("{what}: altezza"));
                    assert_eq!(row.height.state, HeightState::Ereditata, "{what}");
                    assert!(!row.approximate, "{what}");
                    assert!(!row.perimeter_approximate, "{what}: lati dritti");
                    assert_eq!(row.note, None, "{what}");
                }
            }
        }
    }

    /// Caso J in piccolo: lo stesso caso A in centimetri, con la predefinita
    /// 270 (unità del disegno), dà le stesse quantità in metri.
    #[test]
    fn fatto_rows_in_centimetres() {
        let cm = DrawingUnit::Centimetri;
        for form in FORMS {
            let mut scene = scene_in(form, cm, Vec::new());
            write_room_in(&mut scene, cm, "A", [200.0, 150.0], cm.tolerance());
            write_room_in(&mut scene, cm, "B", [600.0, 150.0], cm.tolerance());
            let (rows, notices) = fatto_room_rows(&scene.document, cm, cm.tolerance(), Some(270.0));
            assert!(notices.is_empty(), "{form:?}: {notices:?}");
            let [a, b] = &rows[..] else {
                panic!("{form:?}: {rows:?}")
            };
            for (row, area, perimeter, volume) in [(a, 12.0, 14.0, 32.4), (b, 11.7, 13.8, 31.59)] {
                let what = format!("{form:?} {}", row.name);
                approx(row.area_m2, area, &what);
                approx(row.perimeter_m, perimeter, &what);
                approx(row.volume_m3, volume, &what);
                approx(row.height.value_m, 2.7, &what);
            }
        }
    }

    /// Caso I: l'altezza propria del locale di fatto vince sulla predefinita
    /// (A 3,00 inserita → 36,00 m³); senza predefinita B resta senza altezza
    /// e senza volume, con la superficie.
    #[test]
    fn fatto_row_own_height() {
        for form in FORMS {
            let mut scene = scene_of(form, Vec::new());
            let (outline, _) = write_room(&mut scene, "A", [2.0, 1.5]);
            write_room(&mut scene, "B", [6.0, 1.5]);
            set_height(&mut scene, outline, 3.0);
            let (rows, _) = fatto_room_rows(&scene.document, M, TOL, DEFAULT);
            let [a, b] = &rows[..] else {
                panic!("{form:?}: {rows:?}")
            };
            approx(a.height.value_m, 3.0, &format!("{form:?}"));
            assert_eq!(a.height.state, HeightState::Inserita, "{form:?}");
            approx(a.volume_m3, 36.0, &format!("{form:?}"));
            assert_eq!(b.height.state, HeightState::Ereditata, "{form:?}");

            let (rows, _) = fatto_room_rows(&scene.document, M, TOL, None);
            let [a, b] = &rows[..] else {
                panic!("{form:?}: {rows:?}")
            };
            approx(a.volume_m3, 36.0, &format!("{form:?}: senza predefinita"));
            assert_eq!(b.height.state, HeightState::NonDisponibile, "{form:?}");
            assert_eq!(b.volume_m3, None, "{form:?}");
            approx(b.area_m2, 11.7, &format!("{form:?}: la superficie resta"));
        }
    }

    /// Caso I: un locale non chiuso (la faccia ovest di A tolta) resta nelle
    /// righe con superficie, perimetro e volume `n.d.`, la nota e
    /// [`Notice::LocaleNonChiuso`]; B non cambia.
    #[test]
    fn open_room_has_no_area() {
        for form in FORMS {
            let mut scene = scene_of(form, Vec::new());
            write_room(&mut scene, "A", [2.0, 1.5]);
            write_room(&mut scene, "B", [6.0, 1.5]);
            let west = west_wall_of_a(&scene);
            scene.erase_entities(&[west]);
            let (rows, notices) = fatto_room_rows(&scene.document, M, TOL, DEFAULT);
            assert_eq!(
                notices,
                [Notice::LocaleNonChiuso { name: "A".into() }],
                "{form:?}"
            );
            let [a, b] = &rows[..] else {
                panic!("{form:?}: {rows:?}")
            };
            assert_eq!(
                (a.area_m2, a.perimeter_m, a.volume_m3),
                (None, None, None),
                "{form:?}"
            );
            note_has(a, "non chiuso", &format!("{form:?}"));
            assert!(!a.approximate, "{form:?}");
            approx(a.height.value_m, 2.7, &format!("{form:?}: l'altezza resta"));
            approx(b.area_m2, 11.7, &format!("{form:?}"));
            assert_eq!(b.note, None, "{form:?}");
        }
    }

    /// R55: due semi di fatto nella stessa regione (un secondo locale scritto
    /// dentro A, o il tramezzo a polilinea aperto di 2 mm) non danno nessuna
    /// superficie: nota «fuso con …» su ognuno, un [`Notice::LocaleFuso`] per
    /// gruppo, nessuna somma.
    #[test]
    fn fused_rooms_have_no_area() {
        for form in FORMS {
            let mut scene = scene_of(form, Vec::new());
            write_room(&mut scene, "A", [2.0, 1.5]);
            write_room(&mut scene, "Cucina", [3.0, 1.0]);
            write_room(&mut scene, "B", [6.0, 1.5]);
            let (rows, notices) = fatto_room_rows(&scene.document, M, TOL, DEFAULT);
            let [Notice::LocaleFuso { names, area_m2 }] = &notices[..] else {
                panic!("{form:?}: {notices:?}")
            };
            assert_eq!(names, &["A", "Cucina"], "{form:?}");
            assert!((area_m2 - 12.0).abs() < 1e-9, "{form:?}: {area_m2}");
            let [a, cucina, b] = &rows[..] else {
                panic!("{form:?}: {rows:?}")
            };
            for (row, other) in [(a, "Cucina"), (cucina, "A")] {
                let what = format!("{form:?} {}", row.name);
                assert_eq!(
                    (row.area_m2, row.perimeter_m, row.volume_m3),
                    (None, None, None),
                    "{what}"
                );
                assert_eq!(
                    row.note.as_deref(),
                    Some(&*format!("fuso con {other}")),
                    "{what}"
                );
            }
            assert_eq!(cucina.code, "LF2", "{form:?}");
            approx(b.area_m2, 11.7, &format!("{form:?}"));
        }

        // Il tramezzo a polilinea si ferma 2 mm prima del muro nord.
        let mut scene = scene_of(Form::ChiusaPerMuro, Vec::new());
        write_room(&mut scene, "A", [2.0, 1.5]);
        write_room(&mut scene, "B", [6.0, 1.5]);
        let tramezzo = scene
            .document
            .entities()
            .find(|entity| is_tramezzo(entity))
            .map(|entity| entity.common().handle)
            .expect("tramezzo");
        if let Some(EntityType::LwPolyline(polyline)) = scene.document.get_entity_mut(tramezzo) {
            open_tramezzo(polyline);
        }
        let (rows, notices) = fatto_room_rows(&scene.document, M, TOL, DEFAULT);
        let [Notice::LocaleFuso { names, area_m2 }] = &notices[..] else {
            panic!("{notices:?}")
        };
        assert_eq!(names, &["A", "B"]);
        assert!((area_m2 - 23.7002).abs() < 1e-9, "{area_m2}");
        assert!(rows.iter().all(|row| row.area_m2.is_none()), "{rows:?}");
        assert_eq!(rows[0].note.as_deref(), Some("fuso con B"));
        assert_eq!(rows[1].note.as_deref(), Some("fuso con A"));
    }

    /// Caso D: il pilastro 0,30 × 0,30 dentro A si sottrae dalla superficie
    /// (11,91 m²) ma non entra nel perimetro (14,00).
    #[test]
    fn island_is_subtracted_not_in_perimeter() {
        for form in FORMS {
            let mut scene = scene_of(form, vec![pilastro(M)]);
            write_room(&mut scene, "A", [2.0, 1.5]);
            let (rows, notices) = fatto_room_rows(&scene.document, M, TOL, DEFAULT);
            assert_eq!(
                notices,
                [Notice::Isole {
                    name: "A".into(),
                    count: 1
                }],
                "{form:?}"
            );
            let [a] = &rows[..] else {
                panic!("{form:?}: {rows:?}")
            };
            approx(a.area_m2, 11.91, &format!("{form:?}: superficie"));
            approx(a.perimeter_m, 14.0, &format!("{form:?}: perimetro"));
            approx(a.volume_m3, 11.91 * 2.7, &format!("{form:?}: volume"));
            assert!(!a.approximate, "{form:?}");
        }
    }

    /// Spec §7, caso A demolito, e caso I: A e B diventano un locale di
    /// progetto da 24,00 m² (perimetro 22,00), «A + B» con il nome da
    /// confermare, accoppiato ai due locali di fatto. Altezze di fatto 2,70 e
    /// 3,00: altezza e volume `n.d.` con [`Notice::AltezzeDiscordi`]; 3,00 e
    /// 3,00: 3,00 inserita «dal locale di fatto», 72,00 m³.
    #[test]
    fn project_rows_merge_with_fatto_heights() {
        for form in FORMS {
            let mut scene = scene_of(form, Vec::new());
            let (a_outline, _) = write_room(&mut scene, "A", [2.0, 1.5]);
            let (_, b_text) = write_room(&mut scene, "B", [6.0, 1.5]);
            demolish_tramezzo(&mut scene);
            set_height(&mut scene, b_text, 3.0);

            let (fatto, _) = fatto_room_rows(&scene.document, M, TOL, DEFAULT);
            let (rows, notices) = project_room_rows(&scene.document, M, TOL, TOL, DEFAULT, &fatto);
            let [merged] = &rows[..] else {
                panic!("{form:?}: {rows:?}")
            };
            let row = &merged.row;
            assert_eq!(row.phase, Phase::Progetto, "{form:?}");
            assert_eq!(
                (row.code.as_str(), row.name.as_str()),
                ("LP1", "A + B"),
                "{form:?}"
            );
            note_has(row, "da confermare", &format!("{form:?}"));
            approx(row.area_m2, 24.0, &format!("{form:?}: superficie"));
            approx(row.perimeter_m, 22.0, &format!("{form:?}: perimetro"));
            assert_eq!(merged.fatto_members, [0, 1], "{form:?}");
            assert_eq!(row.height.state, HeightState::NonDisponibile, "{form:?}");
            assert_eq!(row.volume_m3, None, "{form:?}");
            assert!(!row.approximate, "{form:?}");
            assert!(!row.perimeter_approximate, "{form:?}");
            assert!(
                notices.contains(&Notice::AltezzeDiscordi {
                    names: vec!["A".into(), "B".into()],
                    values_m: vec![Some(2.7), Some(3.0)],
                }),
                "{form:?}: {notices:?}"
            );
            assert!(
                notices
                    .iter()
                    .any(|n| matches!(n, Notice::LocaleFuso { names, .. } if names == &["A", "B"])),
                "{form:?}: {notices:?}"
            );

            set_height(&mut scene, a_outline, 3.0);
            let (fatto, _) = fatto_room_rows(&scene.document, M, TOL, DEFAULT);
            let (rows, notices) = project_room_rows(&scene.document, M, TOL, TOL, DEFAULT, &fatto);
            let [merged] = &rows[..] else {
                panic!("{form:?}: {rows:?}")
            };
            let row = &merged.row;
            approx(row.height.value_m, 3.0, &format!("{form:?}"));
            assert_eq!(row.height.state, HeightState::Inserita, "{form:?}");
            assert_eq!(
                row.height.note.as_deref(),
                Some("dal locale di fatto"),
                "{form:?}"
            );
            approx(row.volume_m3, 72.0, &format!("{form:?}: volume"));
            assert!(
                !notices
                    .iter()
                    .any(|n| matches!(n, Notice::AltezzeDiscordi { .. })),
                "{form:?}: {notices:?}"
            );
        }
    }

    /// Caso L: un locale di progetto senza seme proprio prende l'altezza del
    /// locale di fatto accoppiato (A 3,00 inserita: 36,00 m³, non 32,40), con
    /// la nota; un seme di progetto senza altezza dà il nome ma non cambia
    /// l'altezza; con un'altezza sua (3,20) vince il seme.
    #[test]
    fn project_row_without_own_seed_takes_paired_fatto_height() {
        for form in FORMS {
            let mut scene = scene_of(form, Vec::new());
            let (a_outline, _) = write_room(&mut scene, "A", [2.0, 1.5]);
            write_room(&mut scene, "B", [6.0, 1.5]);
            set_height(&mut scene, a_outline, 3.0);
            let rows_now = |scene: &Scene| {
                let (fatto, _) = fatto_room_rows(&scene.document, M, TOL, DEFAULT);
                project_room_rows(&scene.document, M, TOL, TOL, DEFAULT, &fatto)
            };

            let (rows, notices) = rows_now(&scene);
            assert!(notices.is_empty(), "{form:?}: {notices:?}");
            let [a, b] = &rows[..] else {
                panic!("{form:?}: {rows:?}")
            };
            assert_eq!(
                (a.row.code.as_str(), a.row.name.as_str()),
                ("LP1", "A"),
                "{form:?}"
            );
            assert_eq!(a.fatto_members, [0], "{form:?}");
            approx(a.row.area_m2, 12.0, &format!("{form:?}"));
            approx(a.row.height.value_m, 3.0, &format!("{form:?}"));
            assert_eq!(a.row.height.state, HeightState::Inserita, "{form:?}");
            assert_eq!(
                a.row.height.note.as_deref(),
                Some("dal locale di fatto"),
                "{form:?}"
            );
            approx(a.row.volume_m3, 36.0, &format!("{form:?}: volume di A"));
            assert_eq!(
                (b.row.code.as_str(), b.row.name.as_str()),
                ("LP2", "B"),
                "{form:?}"
            );
            assert_eq!(b.fatto_members, [1], "{form:?}");
            assert_eq!(b.row.height.state, HeightState::Ereditata, "{form:?}");
            assert_eq!(
                b.row.height.note.as_deref(),
                Some("dal locale di fatto"),
                "{form:?}"
            );
            approx(b.row.volume_m3, 31.59, &format!("{form:?}: volume di B"));

            let seed = write_seed(&mut scene, "Soggiorno", [3.0, 1.0]);
            let (rows, _) = rows_now(&scene);
            let a = &rows[0];
            assert_eq!(a.row.name, "Soggiorno", "{form:?}");
            assert_eq!(a.fatto_members, [0], "{form:?}");
            approx(
                a.row.height.value_m,
                3.0,
                &format!("{form:?}: seme senza altezza"),
            );
            assert_eq!(
                a.row.height.note.as_deref(),
                Some("dal locale di fatto"),
                "{form:?}"
            );

            set_height(&mut scene, seed, 3.2);
            let (rows, _) = rows_now(&scene);
            let a = &rows[0];
            approx(
                a.row.height.value_m,
                3.2,
                &format!("{form:?}: seme con altezza"),
            );
            assert_eq!(a.row.height.state, HeightState::Inserita, "{form:?}");
            assert_eq!(a.row.height.note, None, "{form:?}");
            approx(a.row.volume_m3, 12.0 * 3.2, &format!("{form:?}"));
            approx(
                rows[1].row.volume_m3,
                31.59,
                &format!("{form:?}: B non cambia"),
            );
        }
    }

    /// Caso I: un vuoto d'angolo di 2 mm chiuso dalla tolleranza dei locali
    /// (`HPGAPTOL` 5 mm) dà la superficie «approssimata», nel fatto e nel
    /// progetto, con la nota (il perimetro, senza archi, no: Ruling L); alla
    /// tolleranza di base A non è chiuso. Solo
    /// nel caso a linee, come nella parte 1: nel caso a polilinee un vuoto di
    /// 2 mm tra lo spigolo di un muro e il lato di un altro (tramezzo fermato
    /// prima del muro nord) resta aperto anche a 5 mm, e A e B escono fusi,
    /// senza superficie.
    #[test]
    fn hpgaptol_room_is_approximate() {
        let mut scene = scene_of(Form::Continua, Vec::new());
        open_corner_of_a(&mut scene);
        write_room_in(&mut scene, M, "A", [2.0, 1.5], GAP);
        write_room_in(&mut scene, M, "B", [6.0, 1.5], GAP);

        let (rows, notices) = fatto_room_rows(&scene.document, M, GAP, DEFAULT);
        assert_eq!(
            notices,
            [Notice::ChiusoPerTolleranza { name: "A".into() }],
            "B è chiuso anche alla tolleranza di base"
        );
        let [a, b] = &rows[..] else {
            panic!("{rows:?}")
        };
        assert!(a.approximate);
        assert!(
            a.area_m2.is_some_and(|area| (area - 12.0).abs() < 0.01),
            "{:?}",
            a.area_m2
        );
        assert!(
            a.perimeter_m.is_some_and(|p| (p - 14.0).abs() < 0.01),
            "{:?}",
            a.perimeter_m
        );
        note_has(a, "HPGAPTOL", "fatto");
        assert!(!a.perimeter_approximate, "nessun arco");
        assert!(!b.approximate);
        assert_eq!(b.note, None);

        let (rows, notices) = fatto_room_rows(&scene.document, M, TOL, DEFAULT);
        assert_eq!(rows[0].area_m2, None, "alla tolleranza di base");
        assert_eq!(notices, [Notice::LocaleNonChiuso { name: "A".into() }]);

        let (fatto, _) = fatto_room_rows(&scene.document, M, GAP, DEFAULT);
        let (rows, _) = project_room_rows(&scene.document, M, TOL, GAP, DEFAULT, &fatto);
        let [a, b] = &rows[..] else {
            panic!("{rows:?}")
        };
        assert_eq!(a.row.name, "A");
        assert!(a.row.approximate, "progetto");
        note_has(&a.row, "approssimat", "progetto");
        assert_eq!(a.fatto_members, [0]);
        assert!(!b.row.approximate, "progetto");
    }

    /// Ruling L: in un locale con un arco il perimetro si misura sulle corde:
    /// `perimeter_approximate`, la nota, un poco meno del vero (11 + 1,5 π);
    /// la superficie resta esatta (curve vere, parte 1), con il suo stato.
    /// Fatto e progetto, arco come `ARC` o come lato a bulge.
    #[test]
    fn curved_room_perimeter_is_on_chords() {
        let area = 12.0 + PI * 1.5 * 1.5 / 2.0;
        let perimeter = 11.0 + PI * 1.5;
        #[track_caller]
        fn check(row: &RoomRow, area: f64, perimeter: f64, what: &str) {
            assert!(row.perimeter_approximate, "{what}");
            note_has(row, "perimetro misurato sulle corde degli archi", what);
            assert!(!row.approximate, "{what}: superficie esatta");
            assert!(
                row.area_m2.is_some_and(|a| (a - area).abs() < 1e-6),
                "{what}: {:?} invece di {area}",
                row.area_m2
            );
            assert!(
                row.perimeter_m
                    .is_some_and(|p| p < perimeter && p > perimeter - 0.01),
                "{what}: {:?} contro {perimeter}",
                row.perimeter_m
            );
        }
        for polyline in [false, true] {
            let what = if polyline { "bulge" } else { "ARC" };
            let mut scene = scene_with(apse(polyline), 6);
            ensure_layers(&mut scene.document);
            write_room(&mut scene, "Abside", [2.0, 1.5]);
            let (fatto, notices) = fatto_room_rows(&scene.document, M, TOL, DEFAULT);
            assert!(notices.is_empty(), "{what}: {notices:?}");
            let [row] = &fatto[..] else {
                panic!("{what}: {fatto:?}")
            };
            check(row, area, perimeter, &format!("{what}: fatto"));
            let (rows, _) = project_room_rows(&scene.document, M, TOL, TOL, DEFAULT, &fatto);
            let [row] = &rows[..] else {
                panic!("{what}: {rows:?}")
            };
            check(&row.row, area, perimeter, &format!("{what}: progetto"));
        }
    }

    /// Ruling M (spec §4.3): un locale di progetto non chiuso resta tra le
    /// righe con superficie, perimetro e volume `n.d.` e la nota, non sparisce.
    /// Un tratto del muro esterno di A demolito (asse (1; −0,15)-(2; −0,15))
    /// apre A verso l'esterno; un seme di progetto fuori dall'edificio non ha
    /// locale attorno. B resta com'era.
    #[test]
    fn unclosed_project_rooms_are_rows() {
        for form in FORMS {
            let mut scene = scene_of(form, Vec::new());
            write_room(&mut scene, "A", [2.0, 1.5]);
            write_room(&mut scene, "B", [6.0, 1.5]);
            demolish(&mut scene, [1.0, -0.15], [2.0, -0.15]);
            write_seed(&mut scene, "Giardino", [10.0, 1.5]);
            let (fatto, _) = fatto_room_rows(&scene.document, M, TOL, DEFAULT);
            approx(fatto[0].area_m2, 12.0, &format!("{form:?}: A di fatto"));

            let (rows, notices) = project_room_rows(&scene.document, M, TOL, TOL, DEFAULT, &fatto);
            let [b, a, garden] = &rows[..] else {
                panic!("{form:?}: {rows:?}")
            };
            assert_eq!(
                (b.row.code.as_str(), b.row.name.as_str()),
                ("LP1", "B"),
                "{form:?}"
            );
            approx(b.row.area_m2, 11.7, &format!("{form:?}: B"));
            assert_eq!(b.fatto_members, [1], "{form:?}");
            assert_eq!(b.row.note, None, "{form:?}");
            for (row, code, name, seed) in [
                (a, "LP2", "A", [2.0, 1.5]),
                (garden, "LP3", "Giardino", [10.0, 1.5]),
            ] {
                let what = format!("{form:?} {name}");
                assert_eq!(row.row.phase, Phase::Progetto, "{what}");
                assert_eq!(
                    (row.row.code.as_str(), row.row.name.as_str()),
                    (code, name),
                    "{what}"
                );
                assert_eq!(row.row.seed, seed, "{what}");
                assert_eq!(
                    (row.row.area_m2, row.row.perimeter_m, row.row.volume_m3),
                    (None, None, None),
                    "{what}"
                );
                assert_eq!(row.row.note.as_deref(), Some("non chiuso"), "{what}");
                assert!(row.fatto_members.is_empty(), "{what}");
                assert!(
                    !row.row.approximate && !row.row.perimeter_approximate,
                    "{what}"
                );
                approx(row.row.height.value_m, 2.7, &what);
            }
            for name in ["A", "Giardino"] {
                assert!(
                    notices.contains(&Notice::LocaleNonChiuso { name: name.into() }),
                    "{form:?}: {notices:?}"
                );
            }
        }
    }

    /// I problemi della geometria di progetto (qui una zona demolita senza
    /// asse sul tramezzo: `ConfiniNonRimossi`) escono tra gli avvisi delle
    /// righe di progetto, una volta sola, come in `AL_ESTRAI`; anche senza
    /// locali. Con il tramezzo che resta, A e B restano due locali.
    #[test]
    fn project_geometry_notices_are_returned() {
        let borders = |notices: &[Notice]| {
            notices
                .iter()
                .filter(|n| {
                    matches!(n, Notice::ConfiniNonRimossi { at }
                        if (at[0] - 4.05).abs() < 1e-9 && (at[1] - 1.5).abs() < 1e-9)
                })
                .count()
        };
        for form in FORMS {
            let mut scene = scene_of(form, Vec::new());
            demolish_without_axis(
                &mut scene,
                vec![[4.0, 0.0], [4.1, 0.0], [4.1, 3.0], [4.0, 3.0]],
            );
            let (rows, notices) = project_room_rows(&scene.document, M, TOL, TOL, DEFAULT, &[]);
            assert!(rows.is_empty(), "{form:?}: {rows:?}");
            assert_eq!(borders(&notices), 1, "{form:?}: {notices:?}");

            write_room(&mut scene, "A", [2.0, 1.5]);
            write_room(&mut scene, "B", [6.0, 1.5]);
            let (fatto, _) = fatto_room_rows(&scene.document, M, TOL, DEFAULT);
            let (rows, notices) = project_room_rows(&scene.document, M, TOL, TOL, DEFAULT, &fatto);
            assert_eq!(borders(&notices), 1, "{form:?}: {notices:?}");
            let [a, b] = &rows[..] else {
                panic!("{form:?}: {rows:?}")
            };
            approx(a.row.area_m2, 12.0, &format!("{form:?}: A"));
            approx(b.row.area_m2, 11.7, &format!("{form:?}: B"));
        }
    }

    /// Ruling N: senza predefinita, un locale di progetto i cui locali di
    /// fatto non hanno altezza è `n.d.` con la nota «altezza mancante nei
    /// locali di fatto», senza l'avviso delle altezze discordi.
    #[test]
    fn project_rows_without_any_fatto_height_have_no_discord_notice() {
        for form in FORMS {
            let mut scene = scene_of(form, Vec::new());
            write_room(&mut scene, "A", [2.0, 1.5]);
            write_room(&mut scene, "B", [6.0, 1.5]);
            let (fatto, _) = fatto_room_rows(&scene.document, M, TOL, None);
            let (rows, notices) = project_room_rows(&scene.document, M, TOL, TOL, None, &fatto);
            assert!(notices.is_empty(), "{form:?}: {notices:?}");
            assert_eq!(rows.len(), 2, "{form:?}");
            for row in &rows {
                let what = format!("{form:?} {}", row.row.name);
                assert_eq!(row.row.height.state, HeightState::NonDisponibile, "{what}");
                assert_eq!(
                    row.row.height.note.as_deref(),
                    Some("altezza mancante nei locali di fatto"),
                    "{what}"
                );
                assert_eq!(row.row.volume_m3, None, "{what}");
                assert!(row.row.area_m2.is_some(), "{what}");
            }
        }
    }

    /// Ruling O: la quantità principale di un locale è la superficie, quindi
    /// la riga è completa anche senza altezza (niente motivo); senza
    /// superficie il motivo è la nota della riga (o «superficie n.d.»), poi
    /// quello dell'altezza o, se manca senza un motivo suo, «altezza n.d.»,
    /// senza vuoti né doppioni.
    #[test]
    fn room_row_is_complete_with_the_area() {
        let mut row = RoomRow::unmeasured(
            Phase::Fatto,
            "LF1".into(),
            "A",
            "",
            [2.0, 1.5],
            Height::missing(None),
        );
        assert!(!row.is_complete());
        assert_eq!(
            row.incomplete_reason().as_deref(),
            Some("superficie n.d.; altezza n.d.")
        );
        row.note = Some(" non chiuso ".into());
        assert_eq!(
            row.incomplete_reason().as_deref(),
            Some("non chiuso; altezza n.d.")
        );
        row.height = Height::missing(Some("altezze dei locali di fatto discordi"));
        assert_eq!(
            row.incomplete_reason().as_deref(),
            Some("non chiuso; altezze dei locali di fatto discordi")
        );
        row.note = Some("altezze dei locali di fatto discordi".into());
        assert_eq!(
            row.incomplete_reason().as_deref(),
            Some("altezze dei locali di fatto discordi"),
            "nessun doppione"
        );
        row.note = None;
        row.height = Height {
            value_m: Some(2.7),
            state: HeightState::Ereditata,
            note: None,
        };
        assert_eq!(row.incomplete_reason().as_deref(), Some("superficie n.d."));

        // Con la superficie: completa, con o senza altezza.
        row.area_m2 = Some(12.0);
        assert!(row.is_complete());
        assert_eq!(row.incomplete_reason(), None);
        row.height = Height::missing(None);
        row.volume_m3 = None;
        row.note = Some("nome da confermare".into());
        assert!(row.is_complete());
        assert_eq!(row.incomplete_reason(), None);
    }

    #[test]
    fn no_rooms_no_rows() {
        let scene = scene_of(Form::Continua, Vec::new());
        assert_eq!(
            fatto_room_rows(&scene.document, M, TOL, DEFAULT),
            (Vec::new(), Vec::new())
        );
        assert_eq!(
            project_room_rows(&scene.document, M, TOL, TOL, DEFAULT, &[]),
            (Vec::new(), Vec::new())
        );
    }
}
