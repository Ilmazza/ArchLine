//! Quantità delle aperture e detrazioni (spec parte 3 §4.3). Logica pura: legge
//! solo le aperture del documento, le righe delle zone e dei locali già
//! calcolate. Grandezze in m, m², m³; `None` = `n.d.` con il motivo, mai un
//! valore di ripiego.
//!
//! Regole:
//! - una riga per apertura, `P1`, `F1`, `PF1` per tipo e nell'ordine degli
//!   handle. Larghezza e spessore sono `dal disegno`; altezza e davanzale
//!   `inserita` o `n.d.` (un'apertura non eredita: l'altezza di una porta non
//!   si deduce da quella del locale). Il vano è larghezza × altezza (m²), il
//!   volume nel muro vano × spessore (m³). Soglia = larghezza (m), solo con
//!   `soglia = sì` su porte e portefinestre. Architrave = larghezza + 2 ×
//!   appoggio (m), solo con `architrave = sì`, `n.d.` senza appoggio. La riga
//!   è completa se ha il vano;
//! - detrazioni (righe a parte, mai sottratte in silenzio): il volume del vano
//!   di un'apertura esistente o da demolire il cui asse cade in una zona
//!   demolita (R105: qualunque zona demolita, non solo quelle con l'asse: il
//!   volume della parte 2 è area × H anche senza asse); la superficie del vano
//!   di un'apertura nuova il cui asse cade in una zona nuova. Un varco non
//!   ha detrazioni: è già il vano;
//! - pareti dei locali (solo se il disegno ha almeno un'apertura, R106): perimetro dell'anello esterno × altezza del locale −
//!   superficie dei vani delle aperture sul perimetro (il punto medio del
//!   vano a non più di metà spessore dall'anello). Le isole non si contano
//!   (R104: `RoomRow` ha solo l'anello esterno). Un'altezza che manca, del
//!   locale o di un vano incontrato, dà `n.d.` con il motivo: mai una parete
//!   parziale. Nel fatto contano le aperture esistenti e da demolire; nel
//!   progetto le esistenti e le nuove.

use codec::{Document, Handle};
use kernel::geom2d::{contains, distance_to, Tolerance};

use super::aperture::{apertures_in, Aperture, ApertureGeom, ApertureKind, ApertureState};
use super::abaco_render::height_state;
use super::height::{Height, HeightState};
use super::quantities::{ZoneKind, ZoneRow};
use super::quantities_rooms::{ProjectRoomRow, RoomRow};
use super::report::Notice;
use super::rooms::{ring_curves, Phase};
use super::units::DrawingUnit;
use super::zone::ZoneGeom;

const NOTE_NO_HEIGHT: &str = "altezza dell'apertura da inserire";
const NOTE_NO_BEARING: &str = "appoggio non inserito";

/// La riga di un'apertura.
#[derive(Clone, Debug, PartialEq)]
pub struct ApertureRow {
    pub handle: Handle,
    /// `P1`, `F1`, `PF1`… progressivo per tipo, in ordine di handle.
    pub code: String,
    pub kind: ApertureKind,
    pub state: ApertureState,
    /// Larghezza e spessore del muro, `dal disegno`.
    pub width_m: f64,
    pub thickness_m: f64,
    /// `Inserita`, oppure `NonDisponibile` con il motivo.
    pub height: Height,
    pub sill_m: Option<f64>,
    /// Larghezza × altezza.
    pub vano_m2: Option<f64>,
    /// Vano × spessore del muro.
    pub vano_m3: Option<f64>,
    /// Lunghezza della soglia (la larghezza), solo se ne ha una.
    pub threshold_m: Option<f64>,
    /// L'architrave è attivo (si computa).
    pub lintel_requested: bool,
    /// Larghezza + 2 × appoggio, solo con l'architrave attivo.
    pub lintel_m: Option<f64>,
    /// Note separate da «; » (il motivo di un architrave `n.d.`).
    pub note: Option<String>,
}

impl ApertureRow {
    /// La quantità principale è il vano: solo con quello la riga entra nei
    /// totali e nel CSV.
    pub fn is_complete(&self) -> bool {
        self.vano_m2.is_some()
    }

    /// Perché la riga è incompleta (`None` se ha il vano).
    pub fn incomplete_reason(&self) -> Option<String> {
        (!self.is_complete()).then(|| {
            self.height
                .note
                .clone()
                .unwrap_or_else(|| NOTE_NO_HEIGHT.to_string())
        })
    }
}

/// La detrazione di una zona: il vano delle aperture che cadono nella zona.
#[derive(Clone, Debug, PartialEq)]
pub struct Deduction {
    pub zone_handle: Handle,
    /// Il codice della zona: `D1`, `N1`…
    pub zone_code: String,
    /// I codici delle aperture, `P1`…
    pub apertures: Vec<String>,
    /// m³ per una demolizione, m² per una nuova muratura; `n.d.` se manca
    /// l'altezza di un vano.
    pub value: Option<f64>,
    /// Il motivo di un `n.d.`.
    pub reason: Option<String>,
}

/// Le pareti di un locale al netto dei vani.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomWalls {
    /// Il codice della riga del locale: `LF1`, `LP1`…
    pub room_code: String,
    pub phase: Phase,
    pub perimeter_m: f64,
    pub height_m: Option<f64>,
    /// Lo stato dell'altezza del locale: `inserita`, `ereditata` o `n.d.`.
    pub height_state: &'static str,
    /// Perimetro × altezza.
    pub gross_m2: Option<f64>,
    /// I vani sul perimetro.
    pub apertures: Vec<String>,
    pub vani_m2: Option<f64>,
    pub net_m2: Option<f64>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ApertureQuantities {
    pub rows: Vec<ApertureRow>,
    pub demolition_deductions: Vec<Deduction>,
    pub new_wall_deductions: Vec<Deduction>,
    pub room_walls: Vec<RoomWalls>,
    pub notices: Vec<Notice>,
}

/// Le quantità delle aperture di `doc` con le detrazioni sulle zone e sulle
/// pareti dei locali. `zones` e `zone_rows` sono quelli di
/// `quantities::zone_rows` (stessi handle), `fatto` e `progetto` le righe dei
/// locali con il loro contorno. Misure in m dalle unità del disegno `unit`,
/// geometria alla tolleranza `tol`.
pub fn aperture_quantities(
    doc: &Document,
    unit: DrawingUnit,
    tol: f64,
    zones: &[ZoneGeom],
    zone_rows: &[ZoneRow],
    fatto: &[RoomRow],
    progetto: &[ProjectRoomRow],
) -> ApertureQuantities {
    let mut found = apertures_in(doc);
    found.sort_by_key(|geom| (kind_order(geom.aperture.kind), geom.handle.value()));
    let rows = rows_of(&found, unit);
    // R106: senza nessuna apertura nel disegno le pareti dei locali non si
    // calcolano: un netto «senza vani» direbbe che non ce ne sono, mentre il
    // disegno può non averli disegnati (come la detrazione delle nuove murature,
    // «non calcolata»).
    if rows.is_empty() {
        return ApertureQuantities::default();
    }
    let mut out = ApertureQuantities {
        notices: rows
            .iter()
            .filter_map(|row| {
                Some(Notice::RigaIncompleta {
                    code: row.code.clone(),
                    reason: row.incomplete_reason()?,
                })
            })
            .collect(),
        ..ApertureQuantities::default()
    };
    out.demolition_deductions = deductions(
        &found,
        &rows,
        zones,
        zone_rows,
        ZoneKind::Demolizione,
        tol,
    );
    out.new_wall_deductions = deductions(&found, &rows, zones, zone_rows, ZoneKind::Nuova, tol);
    let progetto_rows: Vec<&RoomRow> = progetto.iter().map(|project| &project.row).collect();
    for (phase, rooms) in [
        (Phase::Fatto, fatto.iter().collect::<Vec<_>>()),
        (Phase::Progetto, progetto_rows),
    ] {
        for room in rooms {
            out.room_walls
                .extend(room_walls(room, phase, &found, &rows, tol));
        }
    }
    out.rows = rows;
    out
}

fn kind_order(kind: ApertureKind) -> u8 {
    match kind {
        ApertureKind::Porta => 0,
        ApertureKind::Finestra => 1,
        ApertureKind::Portafinestra => 2,
    }
}

fn rows_of(found: &[ApertureGeom], unit: DrawingUnit) -> Vec<ApertureRow> {
    let per_metre = unit.per_metre();
    let mut counts = [0_usize; 3];
    found
        .iter()
        .map(|geom| {
            let a = &geom.aperture;
            let count = &mut counts[usize::from(kind_order(a.kind))];
            *count += 1;
            let width_m = a.width() / per_metre;
            let thickness_m = a.thickness / per_metre;
            let height = match a.height {
                Some(h) => Height {
                    value_m: Some(h / per_metre),
                    state: HeightState::Inserita,
                    note: None,
                },
                None => Height::missing(Some(NOTE_NO_HEIGHT)),
            };
            let vano_m2 = height.value_m.map(|h| width_m * h);
            let mut notes: Vec<&str> = Vec::new();
            let lintel_m = if a.lintel {
                match a.bearing {
                    Some(b) => Some(width_m + 2.0 * b / per_metre),
                    None => {
                        notes.push(NOTE_NO_BEARING);
                        None
                    }
                }
            } else {
                None
            };
            ApertureRow {
                handle: geom.handle,
                code: format!("{}{count}", a.kind.code_prefix()),
                kind: a.kind,
                state: a.state,
                width_m,
                thickness_m,
                vano_m3: vano_m2.map(|area| area * thickness_m),
                vano_m2,
                sill_m: a.sill.map(|s| s / per_metre),
                threshold_m: (a.threshold && a.kind.is_door()).then_some(width_m),
                lintel_requested: a.lintel,
                lintel_m,
                height,
                note: (!notes.is_empty()).then(|| notes.join("; ")),
            }
        })
        .collect()
}

/// Il punto medio dell'asse del vano, in unità del disegno.
fn middle(a: &Aperture) -> [f64; 2] {
    [
        (a.axis.0[0] + a.axis.1[0]) / 2.0,
        (a.axis.0[1] + a.axis.1[1]) / 2.0,
    ]
}

/// Le detrazioni delle zone di tipo `kind`: demolizioni (aperture esistenti o
/// da demolire, volume) o nuove murature (aperture nuove, superficie).
fn deductions(
    found: &[ApertureGeom],
    rows: &[ApertureRow],
    zones: &[ZoneGeom],
    zone_rows: &[ZoneRow],
    kind: ZoneKind,
    tol: f64,
) -> Vec<Deduction> {
    let tolerance = Tolerance::new(tol);
    let counts = |state: ApertureState| match kind {
        ZoneKind::Demolizione => matches!(state, ApertureState::Esistente | ApertureState::Demolita),
        ZoneKind::Nuova => state == ApertureState::Nuova,
        ZoneKind::Varco => false,
    };
    zone_rows
        .iter()
        .filter(|row| row.kind == kind)
        .filter_map(|zone_row| {
            let geom = zones.iter().find(|geom| geom.handle == zone_row.handle)?;
            let outer = geom.rings.first()?;
            let curves = ring_curves(outer);
            let inside: Vec<&ApertureRow> = found
                .iter()
                .zip(rows)
                .filter(|(geom, _)| counts(geom.aperture.state))
                .filter(|(geom, _)| contains(&curves, middle(&geom.aperture), tolerance))
                .map(|(_, row)| row)
                .collect();
            if inside.is_empty() {
                return None;
            }
            let value = inside
                .iter()
                .map(|row| match kind {
                    ZoneKind::Nuova => row.vano_m2,
                    _ => row.vano_m3,
                })
                .sum::<Option<f64>>();
            let missing: Vec<&str> = inside
                .iter()
                .filter(|row| row.vano_m2.is_none())
                .map(|row| row.code.as_str())
                .collect();
            Some(Deduction {
                zone_handle: zone_row.handle,
                zone_code: zone_row.code.clone(),
                apertures: inside.iter().map(|row| row.code.clone()).collect(),
                value,
                reason: (!missing.is_empty()).then(|| {
                    format!("vano senza altezza: {} non detratto", missing.join(", "))
                }),
            })
        })
        .collect()
}

/// Le pareti di un locale misurato, o `None` se non ha un contorno.
fn room_walls(
    room: &RoomRow,
    phase: Phase,
    found: &[ApertureGeom],
    rows: &[ApertureRow],
    tol: f64,
) -> Option<RoomWalls> {
    let perimeter_m = room.perimeter_m?;
    if room.outline.len() < 3 {
        return None;
    }
    let curves = ring_curves(&room.outline);
    let on_ring: Vec<&ApertureRow> = found
        .iter()
        .zip(rows)
        .filter(|(geom, _)| match (phase, geom.aperture.state) {
            (_, ApertureState::Esistente) => true,
            (Phase::Fatto, ApertureState::Demolita) => true,
            (Phase::Progetto, ApertureState::Nuova) => true,
            _ => false,
        })
        .filter(|(geom, _)| {
            let mid = middle(&geom.aperture);
            let reach = geom.aperture.thickness / 2.0 + tol;
            curves.iter().any(|curve| distance_to(curve, mid) <= reach)
        })
        .map(|(_, row)| row)
        .collect();
    let height_m = room.height.value_m;
    let gross_m2 = height_m.map(|h| perimeter_m * h);
    let vani_m2 = on_ring.iter().map(|row| row.vano_m2).sum::<Option<f64>>();
    let mut reasons: Vec<String> = Vec::new();
    if height_m.is_none() {
        reasons.push("altezza del locale n.d.".to_string());
    }
    let missing: Vec<&str> = on_ring
        .iter()
        .filter(|row| row.vano_m2.is_none())
        .map(|row| row.code.as_str())
        .collect();
    if !missing.is_empty() {
        reasons.push(format!("vano senza altezza: {}", missing.join(", ")));
    }
    let net_m2 = gross_m2.zip(vani_m2).map(|(gross, vani)| gross - vani);
    Some(RoomWalls {
        room_code: room.code.clone(),
        phase,
        perimeter_m,
        height_m,
        height_state: height_state(&room.height),
        gross_m2,
        apertures: on_ring.iter().map(|row| row.code.clone()).collect(),
        vani_m2,
        net_m2,
        reason: (!reasons.is_empty()).then(|| reasons.join("; ")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bim::aperture::{aperture_polyline, ensure_layer};
    use crate::bim::fixtures::{case_a_with_door, scene_with, Form};
    use crate::bim::quantities::zone_rows;
    use crate::bim::quantities_rooms::{fatto_room_rows, project_room_rows};
    use crate::bim::rooms::{measure_at, room_entities, Computed};
    use crate::bim::walls::{faces as wall_faces, rect_from_axis, wall_set_without_rooms};
    use crate::bim::zone::{ensure_layers, zone_hatch, zones_in, Zone, ZoneOrigin, ZoneState};
    use crate::bim::MAX_WALL_THICKNESS_M;
    use crate::scene::Scene;

    const M: DrawingUnit = DrawingUnit::Metri;
    const TOL: f64 = 1e-6;
    const FORMS: [Form; 2] = [Form::Continua, Form::ChiusaPerMuro];

    fn door(height: Option<f64>) -> Aperture {
        Aperture {
            kind: ApertureKind::Porta,
            state: ApertureState::Esistente,
            axis: ([4.05, 1.0], [4.05, 1.9]),
            thickness: 0.10,
            height,
            sill: None,
            threshold: true,
            lintel: false,
            bearing: None,
        }
    }

    fn scene_with_door(form: Form, aperture: Aperture) -> Scene {
        let mut scene = scene_with(case_a_with_door(form, M), M.insunits());
        ensure_layers(&mut scene.document);
        ensure_layer(&mut scene.document);
        scene.add_entity(aperture_polyline(&aperture).unwrap());
        scene
    }

    /// Il locale di fatto attorno a `seed`, con le soglie delle aperture (come
    /// `AL_LOCALE`).
    fn write_room(scene: &mut Scene, name: &str, seed: [f64; 2]) {
        let walls = wall_set_without_rooms(&scene.document, TOL);
        let mut sources = walls.sources.clone();
        crate::bim::aperture::add_fatto_thresholds(&scene.document, &mut sources, TOL);
        let faces = wall_faces(&sources, TOL);
        let found = measure_at(&faces, None, &sources, seed, M, TOL).expect("locale chiuso");
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
        for entity in room_entities(&computed, Phase::Fatto, &sources, 0.25, TOL) {
            scene.add_entity(entity);
        }
    }

    fn demolish_tramezzo(scene: &mut Scene) {
        let walls = wall_set_without_rooms(&scene.document, TOL);
        let rect = rect_from_axis(
            &walls.sources,
            [4.05, 0.0],
            [4.05, 3.0],
            MAX_WALL_THICKNESS_M,
            TOL,
        )
        .expect("tramezzo");
        let zone = Zone {
            state: ZoneState::Demolito,
            origin: ZoneOrigin::MuroDuePunti,
            axis: Some(rect.axis),
            thickness: Some(rect.thickness),
            height: Some(2.7),
            varco: false,
        };
        scene.add_entity(zone_hatch(&zone, &[rect.ring()]));
    }

    fn compute(scene: &Scene) -> ApertureQuantities {
        let doc = &scene.document;
        let zones = zones_in(doc);
        let (zrows, _) = zone_rows(&zones, Some(2.7), M, TOL);
        let (fatto, _) = fatto_room_rows(doc, M, TOL, Some(2.7));
        let (progetto, _) = project_room_rows(doc, M, TOL, TOL, Some(2.7), &fatto);
        aperture_quantities(doc, M, TOL, &zones, &zrows, &fatto, &progetto)
    }

    fn near(value: Option<f64>, want: f64, what: &str) {
        assert!(
            value.is_some_and(|v| (v - want).abs() < 1e-9),
            "{what}: {value:?} invece di {want}"
        );
    }

    /// Spec §7 «A+Pd»: porta 0,90 × 2,10 nel muro da 0,10: vano 1,89 m²,
    /// volume 0,189 m³, soglia 0,90 m; codice P1; riga completa. Larghezza e
    /// spessore dal disegno, altezza inserita.
    #[test]
    fn door_row_matches_the_hand_numbers() {
        for form in FORMS {
            let scene = scene_with_door(form, door(Some(2.1)));
            let q = compute(&scene);
            let [row] = &q.rows[..] else {
                panic!("{form:?}: {:?}", q.rows)
            };
            assert_eq!(row.code, "P1", "{form:?}");
            near(Some(row.width_m), 0.9, "larghezza");
            near(Some(row.thickness_m), 0.1, "spessore");
            near(row.vano_m2, 1.89, "vano");
            near(row.vano_m3, 0.189, "volume del vano");
            near(row.threshold_m, 0.9, "soglia");
            assert_eq!(row.lintel_m, None, "architrave spento");
            assert_eq!(row.height.state, HeightState::Inserita);
            assert!(row.is_complete());
            assert!(q.notices.is_empty(), "{:?}", q.notices);
        }
    }

    /// Senza altezza: il vano e le detrazioni sono `n.d.` con il motivo, la
    /// riga è incompleta, e c'è un avviso per riga. Nessun ripiego.
    #[test]
    fn a_door_without_height_has_no_numbers_only_reasons() {
        let mut scene = scene_with_door(Form::Continua, door(None));
        demolish_tramezzo(&mut scene);
        write_room(&mut scene, "A", [2.0, 1.5]);
        let q = compute(&scene);
        let row = &q.rows[0];
        assert_eq!((row.vano_m2, row.vano_m3), (None, None));
        assert!(!row.is_complete());
        assert_eq!(row.height.state, HeightState::NonDisponibile);
        assert!(
            matches!(&q.notices[..], [Notice::RigaIncompleta { code, .. }] if code == "P1"),
            "{:?}",
            q.notices
        );
        let [d] = &q.demolition_deductions[..] else {
            panic!("{:?}", q.demolition_deductions)
        };
        assert_eq!(d.value, None);
        assert!(d.reason.as_deref().unwrap().contains("P1"), "{:?}", d.reason);
        let walls = q.room_walls.iter().find(|w| w.room_code == "LF1").unwrap();
        assert_eq!(walls.net_m2, None, "mai una parete parziale");
        assert!(walls.reason.as_deref().unwrap().contains("vano senza altezza"));
        assert!(walls.gross_m2.is_some(), "il lordo c'è");
    }

    /// Demolizione del tramezzo con la porta dentro: volume 3,00 × 0,10 × 2,70
    /// = 0,81 m³ (parte 2), detrazione 0,189 m³ in una riga a parte.
    #[test]
    fn the_demolition_of_the_wall_with_the_door_deducts_the_vano() {
        for form in FORMS {
            let mut scene = scene_with_door(form, door(Some(2.1)));
            demolish_tramezzo(&mut scene);
            let q = compute(&scene);
            let [d] = &q.demolition_deductions[..] else {
                panic!("{form:?}: {:?}", q.demolition_deductions)
            };
            assert_eq!(d.zone_code, "D1", "{form:?}");
            assert_eq!(d.apertures, ["P1"], "{form:?}");
            near(d.value, 0.189, "detrazione");
            assert_eq!(d.reason, None);
            assert!(q.new_wall_deductions.is_empty());
        }
    }

    /// Un'apertura nuova in un muro nuovo: superficie del vano 1,20 × 1,40 =
    /// 1,68 m² da detrarre; una finestra ha il davanzale e nessuna soglia.
    #[test]
    fn a_new_window_in_a_new_wall_deducts_its_surface() {
        let mut scene = scene_with(Vec::new(), M.insunits());
        ensure_layers(&mut scene.document);
        ensure_layer(&mut scene.document);
        let wall = crate::bim::walls::rect_from_axis_thickness([0.0, 0.0], [3.0, 0.0], 0.30).unwrap();
        let zone = Zone {
            state: ZoneState::Nuovo,
            origin: ZoneOrigin::LineaSpessore,
            axis: Some(wall.axis),
            thickness: Some(wall.thickness),
            height: Some(2.7),
            varco: false,
        };
        scene.add_entity(zone_hatch(&zone, &[wall.ring()]));
        let window = Aperture {
            kind: ApertureKind::Finestra,
            state: ApertureState::Nuova,
            axis: ([0.9, 0.0], [2.1, 0.0]),
            thickness: 0.30,
            height: Some(1.4),
            sill: Some(0.9),
            threshold: false,
            lintel: true,
            bearing: Some(0.2),
        };
        scene.add_entity(aperture_polyline(&window).unwrap());
        let q = compute(&scene);
        let row = &q.rows[0];
        assert_eq!(row.code, "F1");
        near(row.vano_m2, 1.68, "vano");
        near(row.sill_m, 0.9, "davanzale");
        assert_eq!(row.threshold_m, None);
        near(row.lintel_m, 1.6, "architrave 1,20 + 2 × 0,20");
        let [d] = &q.new_wall_deductions[..] else {
            panic!("{:?}", q.new_wall_deductions)
        };
        assert_eq!(d.zone_code, "N1");
        near(d.value, 1.68, "detrazione");
        assert!(q.demolition_deductions.is_empty());
    }

    /// L'architrave senza appoggio è `n.d.` con il motivo.
    #[test]
    fn a_lintel_without_bearing_is_missing_with_the_reason() {
        let scene = scene_with_door(
            Form::Continua,
            Aperture {
                lintel: true,
                ..door(Some(2.1))
            },
        );
        let row = &compute(&scene).rows[0];
        assert_eq!(row.lintel_m, None);
        assert_eq!(row.note.as_deref(), Some("appoggio non inserito"));
        assert!(row.is_complete(), "il vano c'è");
    }

    /// Pareti di A (fatto): perimetro 14,00 × 2,70 = 37,80 − vano 1,89 =
    /// 35,91 m²; anche B (perimetro 13,80 × 2,70 = 37,26 − 1,89 = 35,37). Nel
    /// progetto lo stesso: la porta esiste in entrambi.
    #[test]
    fn room_walls_are_the_perimeter_times_the_height_minus_the_vani() {
        for form in FORMS {
            let mut scene = scene_with_door(form, door(Some(2.1)));
            write_room(&mut scene, "A", [2.0, 1.5]);
            write_room(&mut scene, "B", [6.0, 1.5]);
            let q = compute(&scene);
            let fatto: Vec<&RoomWalls> = q.room_walls.iter().filter(|w| w.phase == Phase::Fatto).collect();
            let [a, b] = &fatto[..] else {
                panic!("{form:?}: {:?}", q.room_walls)
            };
            assert_eq!((a.room_code.as_str(), b.room_code.as_str()), ("LF1", "LF2"));
            assert_eq!(a.apertures, ["P1"], "{form:?}");
            near(a.gross_m2, 37.8, "lordo A");
            near(a.vani_m2, 1.89, "vani A");
            near(a.net_m2, 35.91, "netto A");
            near(b.net_m2, 35.37, "netto B");
            let progetto: Vec<&RoomWalls> =
                q.room_walls.iter().filter(|w| w.phase == Phase::Progetto).collect();
            assert_eq!(progetto.len(), 2, "{form:?}");
            near(progetto[0].net_m2, 35.91, "netto progetto A");
        }
    }

    /// Un'apertura demolita conta nel fatto e non nel progetto.
    #[test]
    fn rooms_count_the_apertures_of_their_phase() {
        let mut scene = scene_with(case_a_with_door(Form::Continua, M), M.insunits());
        ensure_layers(&mut scene.document);
        ensure_layer(&mut scene.document);
        scene.add_entity(aperture_polyline(&Aperture { state: ApertureState::Demolita, ..door(Some(2.1)) }).unwrap());
        write_room(&mut scene, "A", [2.0, 1.5]);
        let q = compute(&scene);
        let fatto = q.room_walls.iter().find(|w| w.phase == Phase::Fatto).unwrap();
        assert_eq!(fatto.apertures, ["P1"], "demolita: c'è nel fatto");
        assert!(
            q.room_walls.iter().all(|w| w.phase != Phase::Progetto || w.apertures.is_empty()),
            "demolita: non c'è nel progetto"
        );
    }
}
