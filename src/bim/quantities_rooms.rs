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
//!   dell'anello esterno della regione, isole escluse, misurato sulle corde
//!   del contorno (con un arco l'errore resta sotto 0,1 mm per corda: lo si
//!   dichiara solo quando anche la superficie non è esatta); volume =
//!   superficie × altezza, `n.d.` se manca l'altezza;
//! - superficie e perimetro non esatti (contorno curvo, o chiuso solo da
//!   `HPGAPTOL`, R47) → `approximate` e la nota;
//! - un locale di fatto non chiuso resta con superficie, perimetro e volume
//!   `n.d.`, la nota e [`Notice::LocaleNonChiuso`]; due locali di fatto fusi
//!   da un vuoto (R54, R55) lo stesso, con la nota «fuso con …» e un
//!   [`Notice::LocaleFuso`] per gruppo: l'area unita non è di nessuno dei due;
//! - altezza di un locale di fatto: la sua, poi la predefinita
//!   (`height::resolve`); di un locale di progetto: quella del suo seme, poi
//!   quella dei locali di fatto i cui semi cadono nella sua regione, se
//!   concordi, poi la predefinita (`height::resolve_project_room`).

use codec::Document;

use super::extract::{computed_project_rooms, rooms_tolerance};
use super::height::{resolve, resolve_project_room, Height};
use super::project::project_geometry;
use super::refresh::{measure_fatto, FattoMeasures};
use super::report::Notice;
use super::rooms::{quality_notices, region_holds, rooms_in, Phase, RoomPart};
use super::units::DrawingUnit;
use super::walls::{wall_set_without_rooms, Region};
use super::zone::zones_in;

/// Il layer dei muri nuovi per la geometria di progetto: non cambia la forma
/// dei locali, solo il layer delle linee nuove, che qui non si disegnano.
const NEW_WALLS_LAYER: &str = "0";

const NOTE_OPEN: &str = "non chiuso";
const NOTE_CONFIRM: &str = "nome da confermare";
const NOTE_BY_TOLERANCE: &str = "chiuso solo da HPGAPTOL: superficie e perimetro approssimati";
const NOTE_CURVED: &str = "contorno curvo: superficie e perimetro approssimati";
const NOTE_APPROXIMATE: &str =
    "superficie e perimetro approssimati (contorno curvo o chiuso solo da HPGAPTOL)";
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
    /// Superficie e perimetro non esatti (contorno curvo, o chiuso solo da
    /// `HPGAPTOL`): stato «approssimata».
    pub approximate: bool,
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
            note: None,
        }
    }

    /// Superficie, perimetro e volume dalla misura (superficie in m², regione
    /// in unità del disegno).
    fn measured(&mut self, area: f64, region: &Region, exact: bool, unit: DrawingUnit) {
        self.area_m2 = Some(area);
        self.perimeter_m = Some(ring_length(&region.outer) / unit.per_metre());
        self.volume_m3 = self.height.value_m.map(|height| area * height);
        self.approximate = !exact;
    }

    fn add_note(&mut self, text: &str) {
        self.note = Some(match self.note.take() {
            Some(note) => format!("{note}; {text}"),
            None => text.to_string(),
        });
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
    let FattoMeasures {
        rooms, mut notices, ..
    } = measure_fatto(doc, unit, room_tol);
    let mut rows = Vec::with_capacity(rooms.len());
    let mut reported: Vec<usize> = Vec::new();
    for (index, fatto) in rooms.iter().enumerate() {
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
            if !reported.contains(&fatto.group) {
                reported.push(fatto.group);
                let members: Vec<_> = rooms
                    .iter()
                    .filter(|other| other.group == fatto.group)
                    .collect();
                if let Some(area_m2) = members
                    .iter()
                    .find_map(|member| member.measured.as_ref().map(|m| m.area_m2))
                {
                    notices.push(Notice::LocaleFuso {
                        names: members.iter().map(|m| m.room.name.clone()).collect(),
                        area_m2,
                    });
                }
            }
        } else if let Some(measured) = &fatto.measured {
            row.measured(measured.area_m2, &measured.region, measured.exact, unit);
            if measured.by_tolerance {
                row.add_note(NOTE_BY_TOLERANCE);
            } else if !measured.exact {
                row.add_note(NOTE_CURVED);
            }
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
/// composto da semi di fatto ha la nota «nome da confermare». Gli avvisi:
/// [`Notice::FuoriPiano`] (lo stesso di [`fatto_room_rows`] sugli stessi
/// muri: chi li unisce toglie i doppioni), quelli dei locali di progetto, poi
/// [`Notice::AltezzeDiscordi`] per riga. Senza locali di fatto né semi di
/// progetto nel disegno, niente.
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
    if !has_seeds {
        return (Vec::new(), Vec::new());
    }
    let room_tol = rooms_tolerance(tol, room_tol);
    let walls = wall_set_without_rooms(doc, tol);
    let zones = zones_in(doc);
    let pg = project_geometry(doc, &walls, &zones, NEW_WALLS_LAYER, tol);
    let (computed, room_notices, _, seeds) =
        computed_project_rooms(doc, &walls, &pg.room_entities, &zones, unit, tol, room_tol);
    let mut notices = Vec::new();
    if walls.off_plane > 0 {
        notices.push(Notice::FuoriPiano {
            count: walls.off_plane,
        });
    }
    notices.extend(room_notices);
    let mut rows = Vec::with_capacity(computed.len());
    for (index, room) in computed.iter().enumerate() {
        let fatto_members: Vec<usize> = fatto
            .iter()
            .enumerate()
            .filter(|(_, row)| region_holds(&room.region, row.seed, tol))
            .map(|(member, _)| member)
            .collect();
        let own = seeds
            .iter()
            .find(|seed| region_holds(&room.region, seed.seed, tol))
            .and_then(|seed| seed.height);
        let members: Vec<(String, Height)> = fatto_members
            .iter()
            .map(|&member| (fatto[member].name.clone(), fatto[member].height.clone()))
            .collect();
        let (height, notice) = resolve_project_room(own, &members, default_height, unit);
        notices.extend(notice);
        let mut row = RoomRow::unmeasured(
            Phase::Progetto,
            format!("LP{}", index + 1),
            &room.name,
            &room.number,
            room.seed,
            height,
        );
        row.measured(room.area_m2, &room.region, room.exact, unit);
        if room.confirm {
            row.add_note(NOTE_CONFIRM);
        }
        if !room.exact {
            row.add_note(NOTE_APPROXIMATE);
        }
        rows.push(ProjectRoomRow { row, fatto_members });
    }
    (rows, notices)
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
    use codec::xdata::ExtendedDataRecord;
    use codec::{EntityType, Handle, Vector3};

    use crate::bim::fixtures::{case_a, disturbi, pilastro, scene_with, Form};
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
        let walls = wall_set_without_rooms(&scene.document, TOL);
        let rect = rect_from_axis(
            &walls.sources,
            [4.05, 0.0],
            [4.05, 3.0],
            MAX_WALL_THICKNESS_M,
            TOL,
        )
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
    /// (`HPGAPTOL` 5 mm) dà superficie e perimetro «approssimati», nel fatto e
    /// nel progetto, con la nota; alla tolleranza di base A non è chiuso. Solo
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
