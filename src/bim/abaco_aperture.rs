//! Le tabelle e le righe del CSV delle aperture (spec parte 3 §5.3): la tabella
//! **Aperture** (una riga per porta o finestra, con la soglia, l'architrave e
//! le zone da cui il vano si detrae), la tabella **Pareti dei locali** (perimetro
//! × altezza − vani) e le misure del libretto, con le detrazioni come righe a
//! detrarre (`detrai` = sì). Il calcolo sta in [`super::aperture_quantities`];
//! qui solo il testo. Le stesse regole delle altre tabelle: `n.d.` con il
//! motivo, solo le righe complete nei totali e nel CSV.

use super::abaco::{format_fixed, CsvRecord, UNIT_M2, UNIT_M3};
use super::abaco_render::{
    excluded, height_cell, label_row, table, Cell, TableKind, TableText, NOT_APPLICABLE, SEP,
};
use super::aperture::{ApertureKind, ApertureState};
use super::aperture_quantities::{ApertureQuantities, ApertureRow, RoomWalls};
use super::rooms::Phase;

const APERTURE_COLUMNS: [&str; 12] = [
    "Codice",
    "Tipo",
    "Stato",
    "Larghezza (m)",
    "Altezza (m)",
    "Davanzale (m)",
    "Vano (m²)",
    "Vano nel muro (m³)",
    "Soglia (m)",
    "Architrave (m)",
    "Detrae da",
    "Note",
];
const WALLS_COLUMNS: [&str; 9] = [
    "Codice",
    "Stato",
    "Perimetro (m)",
    "Altezza (m)",
    "Lordo (m²)",
    "Vani (m²)",
    "Netto (m²)",
    "Vani sul perimetro",
    "Note",
];
const TOTAL_APERTURE: &str = "Totale aperture (righe complete)";
const NOTE_APERTURE: &str = "Larghezza e spessore del muro dal disegno; altezza, davanzale e \
    appoggio dell'architrave inseriti da chi disegna: senza altezza il vano e le detrazioni sono \
    n.d., mai un valore di ripiego. Le detrazioni stanno in questa tabella e nel CSV come righe a \
    detrarre (demolizioni in m³, nuove murature in m²): non sono sottratte dalle altre tabelle.";
const NOTE_WALLS: &str = "Pareti = perimetro dell'anello esterno × altezza del locale − vani \
    delle aperture sul perimetro; pilastri e cavedi (isole) non si contano. Senza altezza del \
    locale o di un vano, il netto è n.d. e non si somma una parete parziale.";

fn kind_name(kind: ApertureKind) -> &'static str {
    match kind {
        ApertureKind::Porta => "Porta",
        ApertureKind::Finestra => "Finestra",
        ApertureKind::Portafinestra => "Portafinestra",
    }
}

fn fixed(value: f64) -> String {
    format_fixed(value, 2)
}

/// «D1 (0,19 m³)», una per zona che contiene il vano.
fn detracts_from(row: &ApertureRow, q: &ApertureQuantities) -> String {
    let mut parts = Vec::new();
    for (deductions, value, unit) in [
        (&q.demolition_deductions, row.vano_m3, "m³"),
        (&q.new_wall_deductions, row.vano_m2, "m²"),
    ] {
        for deduction in deductions.iter().filter(|d| d.apertures.contains(&row.code)) {
            parts.push(match value {
                Some(v) => format!("{} ({} {unit})", deduction.zone_code, fixed(v)),
                None => format!("{} (n.d.)", deduction.zone_code),
            });
        }
    }
    parts.join(SEP)
}

fn aperture_cells(row: &ApertureRow, q: &ApertureQuantities) -> Vec<Cell> {
    let sill = if row.kind == ApertureKind::Finestra {
        Cell::number(row.sill_m)
    } else {
        Cell::plain(NOT_APPLICABLE)
    };
    let threshold = match row.threshold_m {
        Some(_) => Cell::number(row.threshold_m),
        None => Cell::plain(NOT_APPLICABLE),
    };
    let lintel = if row.lintel_requested {
        Cell::number(row.lintel_m)
    } else {
        Cell::plain(NOT_APPLICABLE)
    };
    let mut notes: Vec<String> = Vec::new();
    notes.extend(row.note.clone());
    if let Some(reason) = row.incomplete_reason() {
        notes.push(format!("da completare: {reason}"));
    }
    vec![
        Cell::plain(&row.code),
        Cell::plain(kind_name(row.kind)),
        Cell::plain(row.state.key()),
        Cell::number(Some(row.width_m)),
        height_cell(&row.height),
        sill,
        Cell::number(row.vano_m2),
        Cell::number(row.vano_m3),
        threshold,
        lintel,
        Cell::plain(detracts_from(row, q)),
        Cell::notes(notes),
    ]
}

/// La tabella delle aperture: una riga per apertura e il totale delle righe
/// complete.
pub(super) fn aperture_table(q: &ApertureQuantities) -> TableText {
    let rows: Vec<Vec<Cell>> = q.rows.iter().map(|row| aperture_cells(row, q)).collect();
    let complete: Vec<&ApertureRow> = q.rows.iter().filter(|row| row.is_complete()).collect();
    let sum = |value: fn(&ApertureRow) -> Option<f64>| -> Cell {
        let values: Vec<f64> = complete.iter().filter_map(|row| value(row)).collect();
        if values.is_empty() {
            Cell::nd()
        } else {
            Cell::number(Some(values.iter().sum()))
        }
    };
    let mut total = label_row(TOTAL_APERTURE.to_string(), false, APERTURE_COLUMNS.len());
    total[6] = sum(|row| row.vano_m2);
    total[7] = sum(|row| row.vano_m3);
    total[8] = sum(|row| row.threshold_m);
    total[11] = Cell::plain(excluded(q.rows.len() - complete.len()));
    table(
        TableKind::Aperture,
        "Aperture (m²)",
        &APERTURE_COLUMNS,
        rows,
        vec![total],
        vec![NOTE_APERTURE.to_string()],
    )
}

fn walls_cells(walls: &RoomWalls) -> Vec<Cell> {
    vec![
        Cell::plain(&walls.room_code),
        Cell::plain(match walls.phase {
            Phase::Fatto => "fatto",
            Phase::Progetto => "progetto",
        }),
        Cell::number(Some(walls.perimeter_m)),
        Cell::number(walls.height_m),
        Cell::number(walls.gross_m2),
        Cell::number(walls.vani_m2),
        Cell::number(walls.net_m2),
        Cell::plain(walls.apertures.join(SEP)),
        Cell::plain(walls.reason.clone().unwrap_or_default()),
    ]
}

/// La tabella delle pareti dei locali, con il totale del netto per stato.
pub(super) fn walls_table(q: &ApertureQuantities) -> TableText {
    let rows: Vec<Vec<Cell>> = q.room_walls.iter().map(walls_cells).collect();
    let mut totals = Vec::new();
    for (phase, label) in [
        (Phase::Fatto, "Totale pareti stato di fatto (righe complete)"),
        (Phase::Progetto, "Totale pareti stato di progetto (righe complete)"),
    ] {
        let of_phase: Vec<&RoomWalls> = q.room_walls.iter().filter(|w| w.phase == phase).collect();
        if of_phase.is_empty() {
            continue;
        }
        let nets: Vec<f64> = of_phase.iter().filter_map(|w| w.net_m2).collect();
        let mut total = label_row(label.to_string(), false, WALLS_COLUMNS.len());
        total[6] = if nets.is_empty() {
            Cell::nd()
        } else {
            Cell::number(Some(nets.iter().sum()))
        };
        total[8] = Cell::plain(excluded(of_phase.len() - nets.len()));
        totals.push(total);
    }
    table(
        TableKind::Pareti,
        "Pareti dei locali (m²)",
        &WALLS_COLUMNS,
        rows,
        totals,
        vec![NOTE_WALLS.to_string()],
    )
}

/// Le misure del libretto: le aperture complete (vano = larghezza × altezza,
/// m²), le detrazioni sulle demolizioni (m³) e sulle nuove murature (m²), le
/// pareti dei locali e i loro vani, tutte con `detrai` = sì dove si tolgono.
/// Il nome dello stato dell'apertura va nelle note.
pub(super) fn csv_records(q: &ApertureQuantities) -> Vec<CsvRecord> {
    let find = |code: &str| q.rows.iter().find(|row| row.code == code && row.is_complete());
    let mut out = Vec::new();
    for row in q.rows.iter().filter(|row| row.is_complete()) {
        let Some(height) = row.height.value_m else {
            continue;
        };
        let name = kind_name(row.kind);
        out.push(CsvRecord {
            tipo: match row.kind {
                ApertureKind::Porta => "porta",
                ApertureKind::Finestra => "finestra",
                ApertureKind::Portafinestra => "portafinestra",
            },
            code: row.code.clone(),
            description: format!("{name} {} × {} m", fixed(row.width_m), fixed(height)),
            length: row.width_m,
            width: None,
            height: Some(height),
            unit: UNIT_M2,
            height_state: "inserita",
            source: format!("apertura {:X}", row.handle),
            notes: std::iter::once(match row.state {
                ApertureState::Esistente => "esistente".to_string(),
                ApertureState::Demolita => "demolita".to_string(),
                ApertureState::Nuova => "nuova".to_string(),
            })
            .chain(row.note.clone())
            .collect(),
            deduct: false,
        });
    }
    for (deductions, tipo, unit, with_thickness) in [
        (&q.demolition_deductions, "detrazione demolizione", UNIT_M3, true),
        (&q.new_wall_deductions, "detrazione nuova muratura", UNIT_M2, false),
    ] {
        for deduction in deductions {
            for code in &deduction.apertures {
                let Some(row) = find(code) else { continue };
                let Some(height) = row.height.value_m else {
                    continue;
                };
                out.push(CsvRecord {
                    tipo,
                    code: format!("{}-{code}", deduction.zone_code),
                    description: format!("Detrazione vano {code} da {}", deduction.zone_code),
                    length: row.width_m,
                    width: with_thickness.then_some(row.thickness_m),
                    height: Some(height),
                    unit,
                    height_state: "inserita",
                    source: format!("zona {:X}; apertura {:X}", deduction.zone_handle, row.handle),
                    notes: Vec::new(),
                    deduct: true,
                });
            }
        }
    }
    for walls in &q.room_walls {
        let Some(height) = walls.height_m else {
            continue;
        };
        let (tipo, label) = match walls.phase {
            Phase::Fatto => ("pareti locale di fatto", "fatto"),
            Phase::Progetto => ("pareti locale di progetto", "progetto"),
        };
        out.push(CsvRecord {
            tipo,
            code: format!("{}-PA", walls.room_code),
            description: format!("Pareti del locale {} ({label})", walls.room_code),
            length: walls.perimeter_m,
            width: None,
            height: Some(height),
            unit: UNIT_M2,
            height_state: walls.height_state,
            source: format!("locale {}", walls.room_code),
            notes: Vec::new(),
            deduct: false,
        });
        for code in &walls.apertures {
            let Some(row) = find(code) else { continue };
            let Some(row_height) = row.height.value_m else {
                continue;
            };
            out.push(CsvRecord {
                tipo: "detrazione pareti",
                code: format!("{}-PA-{code}", walls.room_code),
                description: format!("Detrazione vano {code} dalle pareti"),
                length: row.width_m,
                width: None,
                height: Some(row_height),
                unit: UNIT_M2,
                height_state: "inserita",
                source: format!("locale {}; apertura {:X}", walls.room_code, row.handle),
                notes: Vec::new(),
                deduct: true,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bim::abaco::{compute_abaco, csv_row_count, render_tables, to_csv, AbacoOptions};
    use crate::bim::aperture::{aperture_polyline, ensure_layer, Aperture};
    use crate::bim::fixtures::{case_a_with_door, scene_with, Form};
    use crate::bim::rooms::{measure_at, room_entities, Computed};
    use crate::bim::units::DrawingUnit;
    use crate::bim::walls::{faces as wall_faces, rect_from_axis, wall_set_without_rooms};
    use crate::bim::zone::{ensure_layers, zone_hatch, Zone, ZoneOrigin, ZoneState};
    use crate::bim::MAX_WALL_THICKNESS_M;
    use crate::scene::Scene;

    const M: DrawingUnit = DrawingUnit::Metri;
    const TOL: f64 = 1e-6;
    const OPTS: AbacoOptions = AbacoOptions {
        unit: M,
        tol: TOL,
        room_tol: TOL,
    };

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

    /// Caso A con la porta, i locali A e B di fatto, l'altezza predefinita
    /// 2,70 e il tramezzo demolito.
    fn scene(height: Option<f64>) -> Scene {
        let mut scene = scene_with(case_a_with_door(Form::Continua, M), M.insunits());
        ensure_layers(&mut scene.document);
        ensure_layer(&mut scene.document);
        scene.add_entity(aperture_polyline(&door(height)).unwrap());
        let rec = crate::bim::record::ProjectRecord {
            default_height: Some(2.7),
            ..crate::bim::record::read(&scene.document)
        };
        crate::bim::record::write(&mut scene.document, &rec);
        for (name, seed) in [("A", [2.0, 1.5]), ("B", [6.0, 1.5])] {
            let walls = wall_set_without_rooms(&scene.document, TOL);
            let mut sources = walls.sources.clone();
            crate::bim::aperture::add_fatto_thresholds(&scene.document, &mut sources, TOL);
            let faces = wall_faces(&sources, TOL);
            let found = measure_at(&faces, None, &sources, seed, M, TOL).expect("locale");
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
            height: None,
            varco: false,
        };
        scene.add_entity(zone_hatch(&zone, &[rect.ring()]));
        scene
    }

    fn col(t: &TableText, header: &str) -> usize {
        t.columns
            .iter()
            .position(|c| c == header)
            .unwrap_or_else(|| panic!("manca la colonna {header}: {:?}", t.columns))
    }

    fn row_of<'a>(t: &'a TableText, code: &str) -> &'a Vec<Cell> {
        t.rows
            .iter()
            .find(|row| row[0].text == code)
            .unwrap_or_else(|| panic!("manca la riga {code}"))
    }

    /// La tabella Aperture e quella delle pareti con i numeri a mano della
    /// spec: vano 1,89 m², 0,19 m³ (0,189), soglia 0,90 m, detrae da D1;
    /// pareti di A 35,91 m².
    #[test]
    fn tables_show_the_hand_numbers() {
        let abaco = compute_abaco(&scene(Some(2.1)).document, &OPTS);
        let tables = render_tables(&abaco);
        let kinds: Vec<TableKind> = tables.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            [
                TableKind::Demolizioni,
                TableKind::Locali,
                TableKind::Aperture,
                TableKind::Pareti
            ]
        );
        let t = &tables[2];
        let p1 = row_of(t, "P1");
        for (header, want) in [
            ("Tipo", "Porta"),
            ("Stato", "esistente"),
            ("Larghezza (m)", "0,90"),
            ("Altezza (m)", "2,10 (inserita)"),
            ("Davanzale (m)", "-"),
            ("Vano (m²)", "1,89"),
            ("Vano nel muro (m³)", "0,19"),
            ("Soglia (m)", "0,90"),
            ("Architrave (m)", "-"),
            ("Detrae da", "D1 (0,19 m³)"),
        ] {
            assert_eq!(p1[col(t, header)].text, want, "{header}");
        }
        assert!(p1.iter().all(|c| !c.missing));
        assert_eq!(t.totals[0][col(t, "Vano (m²)")].text, "1,89");
        assert!(t.totals[0][col(t, "Note")].text.contains("nessuna riga esclusa"));

        let w = &tables[3];
        let a = row_of(w, "LF1");
        assert_eq!(a[col(w, "Lordo (m²)")].text, "37,80");
        assert_eq!(a[col(w, "Vani (m²)")].text, "1,89");
        assert_eq!(a[col(w, "Netto (m²)")].text, "35,91");
        assert_eq!(a[col(w, "Vani sul perimetro")].text, "P1");
        assert_eq!(row_of(w, "LF2")[col(w, "Netto (m²)")].text, "35,37");
        assert_eq!(w.totals[0][col(w, "Netto (m²)")].text, "71,28");
    }

    /// Senza altezza: `n.d.` in rosso, il motivo nelle note, nessun numero di
    /// ripiego, e fuori dal CSV.
    #[test]
    fn a_door_without_height_is_missing_and_out_of_the_csv() {
        let abaco = compute_abaco(&scene(None).document, &OPTS);
        let tables = render_tables(&abaco);
        let t = tables
            .iter()
            .find(|t| t.kind == TableKind::Aperture)
            .unwrap();
        let p1 = row_of(t, "P1");
        assert_eq!(p1[col(t, "Vano (m²)")].text, "n.d.");
        assert!(p1[col(t, "Vano (m²)")].missing);
        assert!(p1[col(t, "Note")]
            .text
            .contains("altezza dell'apertura da inserire"));
        assert_eq!(p1[col(t, "Detrae da")].text, "D1 (n.d.)");
        assert_eq!(t.totals[0][col(t, "Vano (m²)")].text, "n.d.");
        assert!(t.totals[0][col(t, "Note")].text.contains("1 riga esclusa"));
        let csv = to_csv(&abaco);
        assert!(!csv.contains(";P1;"), "{csv}");
        assert!(!csv.contains("D1-P1"));
        assert!(!csv.contains("-PA-P1"));
        assert!(
            abaco.notices.iter().any(|n| matches!(
                n,
                crate::bim::report::Notice::RigaIncompleta { code, .. } if code == "P1"
            )),
            "{:?}",
            abaco.notices
        );
    }

    /// Il CSV: l'apertura (m²), la sua detrazione dalla demolizione (m³, a
    /// detrarre), le pareti dei locali e i loro vani a detrarre; il prodotto dei
    /// fattori è la quantità.
    #[test]
    fn csv_has_the_aperture_the_deductions_and_the_walls() {
        let abaco = compute_abaco(&scene(Some(2.1)).document, &OPTS);
        let csv = to_csv(&abaco);
        let rows: Vec<Vec<&str>> = csv
            .trim_start_matches('\u{FEFF}')
            .split("\r\n")
            .filter(|line| !line.is_empty())
            .map(|line| line.split(';').collect())
            .collect();
        assert_eq!(rows.len() - 1, csv_row_count(&abaco));
        let find = |code: &str| {
            rows.iter()
                .find(|r| r[1] == code)
                .unwrap_or_else(|| panic!("manca {code}: {rows:?}"))
        };
        let p1 = find("P1");
        assert_eq!(p1[0], "porta");
        assert_eq!(&p1[4..9], &["0,9000", "", "2,1000", "no", "1,8900"]);
        assert_eq!((p1[9], p1[10]), ("m²", "inserita"));
        assert!(p1[11].starts_with("apertura "), "{}", p1[11]);
        assert_eq!(p1[12], "esistente");

        let d = find("D1-P1");
        assert_eq!(d[0], "detrazione demolizione");
        assert_eq!(&d[4..9], &["0,9000", "0,1000", "2,1000", "sì", "0,1890"]);
        assert_eq!(d[9], "m³");

        let pa = find("LF1-PA");
        assert_eq!(&pa[4..9], &["14,0000", "", "2,7000", "no", "37,8000"]);
        let dp = find("LF1-PA-P1");
        assert_eq!(&dp[4..9], &["0,9000", "", "2,1000", "sì", "1,8900"]);
        assert_eq!(dp[0], "detrazione pareti");

        // Il prodotto dei fattori scritti è sempre la quantità (parte 2, §7).
        for record in &rows[1..] {
            let number = |text: &str| text.replace(',', ".").parse::<f64>().unwrap();
            let product: f64 = record[3..7]
                .iter()
                .filter(|f| !f.is_empty())
                .map(|f| number(f))
                .product();
            assert!((product - number(record[8])).abs() <= 5e-5, "{record:?}");
        }
    }

    /// Le nuove murature rimandano alla tabella Aperture invece di dire «non
    /// calcolata» quando il disegno ha aperture.
    #[test]
    fn new_walls_point_to_the_aperture_table() {
        let mut scene = scene_with(Vec::new(), M.insunits());
        ensure_layers(&mut scene.document);
        ensure_layer(&mut scene.document);
        let wall =
            crate::bim::walls::rect_from_axis_thickness([0.0, 0.0], [3.0, 0.0], 0.30).unwrap();
        let zone = Zone {
            state: ZoneState::Nuovo,
            origin: ZoneOrigin::LineaSpessore,
            axis: Some(wall.axis),
            thickness: Some(wall.thickness),
            height: Some(2.7),
            varco: false,
        };
        scene.add_entity(zone_hatch(&zone, &[wall.ring()]));
        let tables = render_tables(&compute_abaco(&scene.document, &OPTS));
        let nuove = &tables[0];
        assert!(
            nuove.rows.iter().flatten().any(|c| c.text == "non calcolata"),
            "senza aperture"
        );

        let window = Aperture {
            kind: ApertureKind::Finestra,
            state: ApertureState::Nuova,
            axis: ([0.9, 0.0], [2.1, 0.0]),
            thickness: 0.30,
            height: Some(1.4),
            sill: Some(0.9),
            threshold: false,
            lintel: false,
            bearing: None,
        };
        scene.add_entity(aperture_polyline(&window).unwrap());
        let tables = render_tables(&compute_abaco(&scene.document, &OPTS));
        let nuove = tables.iter().find(|t| t.kind == TableKind::Nuove).unwrap();
        assert!(!nuove
            .rows
            .iter()
            .chain(&nuove.totals)
            .flatten()
            .any(|c| c.text == "non calcolata"));
        assert!(nuove.rows.iter().flatten().any(|c| c.text == "vedi Aperture"));
        assert!(
            nuove.notes.iter().any(|n| n.contains("tabella Aperture")),
            "{:?}",
            nuove.notes
        );
        let t = tables
            .iter()
            .find(|t| t.kind == TableKind::Aperture)
            .unwrap();
        assert_eq!(row_of(t, "F1")[col(t, "Detrae da")].text, "N1 (1,68 m²)");
        assert_eq!(row_of(t, "F1")[col(t, "Davanzale (m)")].text, "0,90");
    }
}
