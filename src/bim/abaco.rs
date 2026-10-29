//! Abaco (spec parte 2, §4.3, §5.2): le righe delle zone e dei locali messe
//! insieme ([`compute_abaco`]). La resa come testo delle tre tabelle
//! (Demolizioni, Nuove murature, Locali) sta in [`super::abaco_render`]
//! ([`render_tables`], [`TableText`]), riesportata da qui. Logica pura.
//!
//! Regole:
//! - le misure sono quelle delle quantità (`quantities::zone_rows`,
//!   `quantities_rooms::{fatto_room_rows, project_room_rows}`), cioè quelle di
//!   `AL_AGGIORNA` e `AL_ESTRAI` (R61); l'altezza predefinita viene dal
//!   record di progetto;
//! - una riga è completa se ha la quantità principale (spec §4.2): volume per
//!   demolizioni e varchi, superficie per le nuove murature, superficie netta
//!   per i locali (perimetro e volume dei locali sono informazione, spec §2).
//!   Solo le righe complete entrano nei totali, che dicono quante righe
//!   restano fuori e quante righe complete non hanno una grandezza secondaria
//!   (la parete di una demolizione senza asse, il volume di un locale senza
//!   altezza), così un dato che manca non sembra mai uno zero. Per ogni riga
//!   incompleta un [`Notice::RigaIncompleta`] con il motivo, in testa agli
//!   avvisi (Ruling C);
//! - locali (spec §4.3): un locale di progetto con un solo locale di fatto
//!   nella sua regione, che non sta in nessun altro locale di progetto, fa una
//!   riga accoppiata con la variazione (progetto meno fatto); gli altri
//!   restano righe separate con la nota («da A + B», «unito in C»), senza
//!   somme. Nelle righe accoppiate codice, nome, perimetro, volume e altezza
//!   sono «fatto / progetto» quando differiscono;
//! - la resa nelle tabelle: vedi le regole di [`super::abaco_render`].

use codec::Document;

use super::abaco_render::{height_unexplained, push_note, HEIGHT_ND, SEP};
use super::extract::push_unique;
use super::quantities::{zone_rows, ZoneKind, ZoneRow};
use super::quantities_rooms::{fatto_room_rows, project_room_rows, ProjectRoomRow, RoomRow};
use super::record;
use super::report::Notice;
use super::units::DrawingUnit;
use super::zone::zones_in;

pub use super::abaco_render::{render_tables, Cell, TableKind, TableText};

const AREA_ND: &str = "superficie n.d.";
/// Tra i nomi dei locali uniti, come nei nomi composti dei locali di progetto.
const JOIN: &str = " + ";
/// Decimali al massimo in [`format_fixed`]: con di più le unità dell'ultima
/// cifra di un numero di qualche miliardo uscirebbero da un `i64`.
const MAX_DECIMALS: usize = 9;

/// Unità e tolleranze del calcolo, come per `AL_ESTRAI` (`ExtractOptions`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AbacoOptions {
    pub unit: DrawingUnit,
    /// Tolleranza geometrica, in unità del disegno: zone, muri, geometria di
    /// progetto.
    pub tol: f64,
    /// Tolleranza dei locali, in unità del disegno: `max(tol, HPGAPTOL)`,
    /// quella di `AL_LOCALE`, `AL_AGGIORNA` e `AL_ESTRAI` (R57, R61).
    pub room_tol: f64,
}

/// Una riga della tabella dei locali: un locale di fatto, uno di progetto o
/// i due accoppiati.
#[derive(Clone, Debug, PartialEq)]
pub struct LocaleLine {
    pub fatto: Option<RoomRow>,
    pub progetto: Option<RoomRow>,
    /// Progetto meno fatto, in m²: solo in una riga accoppiata con le due
    /// superfici.
    pub variation_m2: Option<f64>,
    /// Perché la riga non è accoppiata: «da A + B» su un locale di progetto,
    /// «unito in C» (o «diviso tra C + D») su un locale di fatto.
    pub note: Option<String>,
}

/// Le righe dell'abaco di un disegno ([`compute_abaco`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Abaco {
    /// Le zone demolite non varco, poi i varchi e le nuove murature, ognuna in
    /// ordine di codice (D1, V1, N1…).
    pub demolizioni: Vec<ZoneRow>,
    pub varchi: Vec<ZoneRow>,
    pub nuove: Vec<ZoneRow>,
    /// I locali di fatto in ordine di disegno, ognuno seguito dai locali di
    /// progetto non accoppiati di cui è l'ultimo membro; in fondo i locali di
    /// progetto senza locali di fatto.
    pub locali: Vec<LocaleLine>,
    /// Un [`Notice::RigaIncompleta`] per riga incompleta (zone, poi locali di
    /// fatto, poi di progetto), poi gli avvisi delle zone, dei locali di fatto
    /// e di quelli di progetto, senza doppioni.
    pub notices: Vec<Notice>,
}

/// L'abaco del disegno (spec §4.3): le righe delle zone
/// (`quantities::zone_rows`) divise per tipo, quelle dei locali di fatto e
/// di progetto (`quantities_rooms`) accoppiate, con l'altezza predefinita
/// del record di progetto. Non scrive nulla nel disegno.
pub fn compute_abaco(doc: &Document, opts: &AbacoOptions) -> Abaco {
    let default_height = record::read(doc).default_height;
    let (zones, zone_notices) = zone_rows(&zones_in(doc), default_height, opts.unit, opts.tol);
    let (fatto, fatto_notices) = fatto_room_rows(doc, opts.unit, opts.room_tol, default_height);
    let (progetto, project_notices) = project_room_rows(
        doc,
        opts.unit,
        opts.tol,
        opts.room_tol,
        default_height,
        &fatto,
    );

    // Ruling C: un avviso per riga incompleta, nell'ordine delle tabelle.
    let zone_reasons = zones
        .iter()
        .filter_map(|row| Some((row.code.clone(), row.incomplete_reason()?)));
    let room_reasons = fatto
        .iter()
        .chain(progetto.iter().map(|project| &project.row))
        .filter_map(|row| Some((row.code.clone(), room_incomplete_reason(row)?)));
    let incomplete = zone_reasons
        .chain(room_reasons)
        .map(|(code, reason)| Notice::RigaIncompleta { code, reason })
        .collect();
    // `FuoriPiano` lo trovano sia i locali di fatto sia quelli di progetto.
    let mut notices = Vec::new();
    for more in [incomplete, zone_notices, fatto_notices, project_notices] {
        push_unique(&mut notices, more);
    }

    let mut abaco = Abaco {
        demolizioni: Vec::new(),
        varchi: Vec::new(),
        nuove: Vec::new(),
        locali: pair_rooms(fatto, progetto),
        notices,
    };
    for row in zones {
        match row.kind {
            ZoneKind::Demolizione => abaco.demolizioni.push(row),
            ZoneKind::Varco => abaco.varchi.push(row),
            ZoneKind::Nuova => abaco.nuove.push(row),
        }
    }
    abaco
}

/// Perché una riga di locale è incompleta (`None` se ha la superficie): la
/// nota (o «superficie n.d.»), poi il motivo dell'altezza o, se manca senza
/// un motivo suo, «altezza n.d.», come `ZoneRow::incomplete_reason`.
pub(super) fn room_incomplete_reason(row: &RoomRow) -> Option<String> {
    if row.area_m2.is_some() {
        return None;
    }
    let mut parts = Vec::new();
    push_note(&mut parts, row.note.as_deref());
    if parts.is_empty() {
        parts.push(AREA_ND.to_string());
    }
    push_note(&mut parts, row.height.note.as_deref());
    if height_unexplained(&row.height) {
        push_note(&mut parts, Some(HEIGHT_ND));
    }
    Some(parts.join(SEP))
}

/// Le righe dei locali (spec §4.3). Un locale di fatto e uno di progetto si
/// accoppiano se il progetto ha come solo membro quel locale di fatto e il
/// seme di questo non sta in nessun altro locale di progetto: una riga con la
/// variazione. Gli altri restano righe separate: un locale di fatto con
/// «unito in C» (o «diviso tra C + D» se il suo seme sta in più locali di
/// progetto), un locale di progetto con «da A + B»; nessuna somma.
fn pair_rooms(fatto: Vec<RoomRow>, progetto: Vec<ProjectRoomRow>) -> Vec<LocaleLine> {
    // I locali di progetto che contengono il seme di ciascun locale di fatto.
    let mut owners: Vec<Vec<usize>> = vec![Vec::new(); fatto.len()];
    for (p, project) in progetto.iter().enumerate() {
        for &f in &project.fatto_members {
            if let Some(list) = owners.get_mut(f) {
                list.push(p);
            }
        }
    }
    let partners: Vec<Option<usize>> = owners
        .iter()
        .enumerate()
        .map(|(f, list)| match list[..] {
            [p] if progetto[p].fatto_members == [f] => Some(p),
            _ => None,
        })
        .collect();
    // Un locale di progetto non accoppiato esce dopo l'ultimo dei suoi locali
    // di fatto; senza locali di fatto, in fondo.
    let after: Vec<Option<usize>> = progetto
        .iter()
        .map(|project| {
            project
                .fatto_members
                .iter()
                .copied()
                .filter(|&f| f < fatto.len())
                .max()
        })
        .collect();
    let fatto_names: Vec<String> = fatto.iter().map(|row| row.name.clone()).collect();
    let project_names: Vec<String> = progetto.iter().map(|p| p.row.name.clone()).collect();
    let mut progetto: Vec<Option<ProjectRoomRow>> = progetto.into_iter().map(Some).collect();
    let mut lines = Vec::new();
    for (f, row) in fatto.into_iter().enumerate() {
        match partners[f].and_then(|p| progetto[p].take()) {
            Some(project) => {
                let variation_m2 = row
                    .area_m2
                    .zip(project.row.area_m2)
                    .map(|(before, after)| after - before);
                lines.push(LocaleLine {
                    fatto: Some(row),
                    progetto: Some(project.row),
                    variation_m2,
                    note: None,
                });
            }
            None => {
                let names: Vec<&str> = owners[f]
                    .iter()
                    .map(|&p| project_names[p].as_str())
                    .collect();
                let note = match names[..] {
                    [] => None,
                    [one] => Some(format!("unito in {one}")),
                    _ => Some(format!("diviso tra {}", names.join(JOIN))),
                };
                lines.push(LocaleLine {
                    fatto: Some(row),
                    progetto: None,
                    variation_m2: None,
                    note,
                });
            }
        }
        for (p, slot) in progetto.iter_mut().enumerate() {
            if after[p] == Some(f) {
                if let Some(project) = slot.take() {
                    lines.push(project_line(project, &fatto_names));
                }
            }
        }
    }
    lines.extend(
        progetto
            .into_iter()
            .flatten()
            .map(|project| project_line(project, &fatto_names)),
    );
    lines
}

/// La riga di un locale di progetto non accoppiato, con «da A + B» se ha
/// locali di fatto.
fn project_line(project: ProjectRoomRow, fatto_names: &[String]) -> LocaleLine {
    let names: Vec<&str> = project
        .fatto_members
        .iter()
        .filter_map(|&f| fatto_names.get(f))
        .map(String::as_str)
        .collect();
    LocaleLine {
        fatto: None,
        progetto: Some(project.row),
        variation_m2: None,
        note: (!names.is_empty()).then(|| format!("da {}", names.join(JOIN))),
    }
}

/// `value` con `decimals` decimali (al massimo 9), virgola decimale, senza
/// separatore delle migliaia: `11,91`, `0,1890`, `1234567,89`. Si arrotonda
/// sull'ultima cifra, mezzo via dallo zero (come `units::format_decimal`), e
/// uno zero arrotondato non ha segno. Un valore non finito non ha testo
/// sensato: chi chiama mette `n.d.`.
pub fn format_fixed(value: f64, decimals: usize) -> String {
    let decimals = decimals.min(MAX_DECIMALS);
    let scale = 10_u64.pow(decimals as u32);
    // La conversione satura: niente panico oltre i limiti di `i64`.
    let units = (value * scale as f64).round() as i64;
    let sign = if units < 0 { "-" } else { "" };
    let abs = units.unsigned_abs();
    if decimals == 0 {
        format!("{sign}{abs}")
    } else {
        format!(
            "{sign}{},{:0width$}",
            abs / scale,
            abs % scale,
            width = decimals
        )
    }
}

/// Aiuti dei test dell'abaco, comuni a questo modulo e a
/// [`super::abaco_render`]: il caso A della spec parte 2, §7, in metri, e le
/// zone scritte a mano.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    use crate::bim::fixtures::{case_a, scene_with, Form};
    use crate::bim::record::{self, ProjectRecord};
    use crate::bim::walls::{rect_from_axis, wall_set_without_rooms};
    use crate::bim::zone::{ensure_layers, zone_hatch, Zone, ZoneOrigin, ZoneState};
    use crate::bim::MAX_WALL_THICKNESS_M;
    use crate::scene::Scene;

    pub(crate) const M: DrawingUnit = DrawingUnit::Metri;
    pub(crate) const TOL: f64 = 1e-6;
    pub(crate) const FORMS: [Form; 2] = [Form::Continua, Form::ChiusaPerMuro];
    pub(crate) const OPTS: AbacoOptions = AbacoOptions {
        unit: M,
        tol: TOL,
        room_tol: TOL,
    };

    /// Il caso A nella forma data, in metri, con i layer `AL-*`.
    pub(crate) fn scene_of(form: Form) -> Scene {
        let mut scene = scene_with(case_a(form, M), M.insunits());
        ensure_layers(&mut scene.document);
        scene
    }

    /// Un disegno in metri senza rilievo, con i layer `AL-*`.
    pub(crate) fn empty_scene() -> Scene {
        let mut scene = scene_with(Vec::new(), M.insunits());
        ensure_layers(&mut scene.document);
        scene
    }

    /// L'altezza predefinita nel record di progetto, come `AL_ALTEZZA L`.
    pub(crate) fn set_default_height(scene: &mut Scene, height: f64) {
        let rec = ProjectRecord {
            default_height: Some(height),
            ..record::read(&scene.document)
        };
        record::write(&mut scene.document, &rec);
    }

    /// Una zona demolita «muro tra due punti» sull'asse `p1`-`p2`.
    pub(crate) fn demolish(scene: &mut Scene, p1: [f64; 2], p2: [f64; 2]) {
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

    pub(crate) fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    /// Una zona sull'anello `ring`, con o senza asse.
    pub(crate) fn add_zone(
        scene: &mut Scene,
        state: ZoneState,
        axis: Option<([f64; 2], [f64; 2])>,
        height: Option<f64>,
        varco: bool,
        ring: Vec<[f64; 2]>,
    ) {
        let zone = Zone {
            state,
            origin: if axis.is_some() {
                ZoneOrigin::MuroDuePunti
            } else {
                ZoneOrigin::Rettangolo
            },
            axis,
            thickness: None,
            height,
            varco,
        };
        scene.add_entity(zone_hatch(&zone, &[ring]));
    }

    #[track_caller]
    pub(crate) fn approx(value: Option<f64>, want: f64, what: &str) {
        assert!(
            value.is_some_and(|v| (v - want).abs() < 1e-9),
            "{what}: {value:?} invece di {want}"
        );
    }

    pub(crate) fn incomplete(abaco: &Abaco) -> Vec<(String, String)> {
        abaco
            .notices
            .iter()
            .filter_map(|notice| match notice {
                Notice::RigaIncompleta { code, reason } => Some((code.clone(), reason.clone())),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    //! Il calcolo dell'abaco contro `AL_AGGIORNA` e `AL_ESTRAI` (R61) e il
    //! formato dei numeri; la resa si prova in `abaco_render`.
    use super::test_support::*;
    use super::*;

    use codec::EntityType;

    use crate::bim::extract::{extract, Elaborato, ExtractOptions};
    use crate::bim::fixtures::{shift_tramezzo, LAYER_MURI};
    use crate::bim::refresh::refresh_rooms;
    use crate::bim::room_command::test_support::write_room;
    use crate::bim::rooms::label;
    use crate::bim::LAYER_LOCALI;

    /// I testi (`MTEXT`) su `layer`, in ordine alfabetico.
    fn labels_on(doc: &Document, layer: &str) -> Vec<String> {
        let mut labels: Vec<String> = doc
            .entities()
            .filter(|entity| entity.common().layer.eq_ignore_ascii_case(layer))
            .filter_map(|entity| match entity {
                EntityType::MText(text) => Some(text.value.clone()),
                _ => None,
            })
            .collect();
        labels.sort();
        labels
    }

    /// R61: dopo una correzione del rilievo senza `AL_AGGIORNA` (A da 12,00 a
    /// 12,60, B da 11,70 a 11,10), area e nome dei locali di fatto dell'abaco
    /// sono quelli di `AL_AGGIORNA` (`refresh_rooms`) e dello stato di fatto
    /// di `AL_ESTRAI`; i locali di progetto quelli dello stato di progetto.
    #[test]
    fn computation_matches_refresh_and_extract() {
        for form in FORMS {
            let mut scene = scene_of(form);
            set_default_height(&mut scene, 2.7);
            write_room(&mut scene, "A", [2.0, 1.5]);
            write_room(&mut scene, "B", [6.0, 1.5]);
            shift_tramezzo(&mut scene.document, M, 0.2);
            let doc = &scene.document;
            let abaco = compute_abaco(doc, &OPTS);
            let fatto: Vec<&RoomRow> = abaco
                .locali
                .iter()
                .filter_map(|l| l.fatto.as_ref())
                .collect();
            let progetto: Vec<&RoomRow> = abaco
                .locali
                .iter()
                .filter_map(|l| l.progetto.as_ref())
                .collect();
            let names: Vec<&str> = fatto.iter().map(|r| r.name.as_str()).collect();
            assert_eq!(names, ["A", "B"], "{form:?}");
            approx(fatto[0].area_m2, 12.6, &format!("{form:?}: A"));
            approx(fatto[1].area_m2, 11.1, &format!("{form:?}: B"));

            let refresh = refresh_rooms(doc, M, TOL);
            for row in &fatto {
                let change = refresh
                    .changed
                    .iter()
                    .find(|change| change.name == row.name)
                    .unwrap_or_else(|| panic!("{form:?}: {} non aggiornato", row.name));
                assert_eq!(
                    Some(change.new_area_m2),
                    row.area_m2,
                    "{form:?}: {}",
                    row.name
                );
            }

            let ex = extract(
                doc,
                &ExtractOptions {
                    elaborati: vec![Elaborato::StatoDiFatto, Elaborato::StatoDiProgetto],
                    new_walls_layer: LAYER_MURI.into(),
                    unit: M,
                    tol: TOL,
                    room_tol: TOL,
                },
            );
            let expected = |rows: &[&RoomRow]| {
                let mut labels: Vec<String> = rows
                    .iter()
                    .map(|r| label(&r.name, r.area_m2.expect("misurato"), false))
                    .collect();
                labels.sort();
                labels
            };
            assert_eq!(
                labels_on(&ex.docs[0].1, LAYER_LOCALI),
                expected(&fatto),
                "{form:?}: stato di fatto"
            );
            assert_eq!(
                labels_on(&ex.docs[1].1, LAYER_LOCALI),
                expected(&progetto),
                "{form:?}: stato di progetto"
            );
        }
    }

    #[test]
    fn format_fixed_uses_comma() {
        assert_eq!(format_fixed(11.905, 2), "11,91");
        assert_eq!(format_fixed(0.189, 4), "0,1890");
        assert_eq!(format_fixed(0.08, 3), "0,080");
        assert_eq!(format_fixed(-0.3, 2), "-0,30");
        assert_eq!(format_fixed(-0.001, 2), "0,00");
        assert_eq!(format_fixed(1_234_567.891, 2), "1234567,89");
        assert_eq!(format_fixed(2.5, 0), "3");
    }
}
