//! La tabella **Stratigrafie** e le righe del CSV degli strati (spec parte 6
//! §5.3). Il calcolo sta in [`super::strata_quantities`]; qui solo il testo.
//! Stesse regole delle altre tabelle: `n.d.` con il motivo, solo le righe
//! complete nei totali e nel CSV. I totali sono per **materiale** (sommare
//! metri quadri di materiali diversi non dice nulla).

use super::abaco::{room_record, zone_record, Abaco, CsvRecord};
use super::abaco_render::{excluded, label_row, table, Cell, TableKind, TableText};
use super::strata_assign::Element;
use super::strata_quantities::{StrataQuantities, StrataRow};

const COLUMNS: [&str; 8] = [
    "Codice",
    "Elemento",
    "Stratigrafia",
    "Strato",
    "Spessore (cm)",
    "Superficie (m²)",
    "Volume (m³)",
    "Note",
];
const NOTE: &str = "Ogni strato ha la superficie dell'elemento (nuova muratura L × H, \
    superficie netta del locale di progetto), con il suo stato; il volume (superficie × spessore) \
    è solo informativo. Spessori inseriti da chi progetta: nessun valore di ripiego.";

fn cells(row: &StrataRow) -> Vec<Cell> {
    vec![
        Cell::plain(&row.code),
        Cell::plain(format!(
            "{} {}",
            match row.owner {
                Element::NewWall => "muratura",
                Element::ProjectRoom => "locale",
            },
            row.owner_code
        )),
        Cell::plain(&row.stratigraphy),
        Cell::plain(&row.material),
        Cell::plain(&row.thickness_cm),
        Cell::number(row.area_m2),
        Cell::number(row.volume_m3),
        Cell::plain(row.reason.clone().unwrap_or_default()),
    ]
}

/// La tabella degli strati, con un totale per materiale (nell'ordine in cui i
/// materiali compaiono).
pub(super) fn strata_table(q: &StrataQuantities) -> TableText {
    let rows: Vec<Vec<Cell>> = q.rows.iter().map(cells).collect();
    let mut materials: Vec<String> = Vec::new();
    for row in &q.rows {
        if !materials.iter().any(|m| m.eq_ignore_ascii_case(&row.material)) {
            materials.push(row.material.clone());
        }
    }
    let mut totals = Vec::new();
    for material in materials {
        let of: Vec<&StrataRow> = q
            .rows
            .iter()
            .filter(|r| r.material.eq_ignore_ascii_case(&material))
            .collect();
        let complete: Vec<&&StrataRow> = of.iter().filter(|r| r.is_complete()).collect();
        let mut total = label_row(
            format!("Totale {material} (righe complete)"),
            false,
            COLUMNS.len(),
        );
        if complete.is_empty() {
            total[5] = Cell::nd();
            total[6] = Cell::nd();
        } else {
            total[5] = Cell::number(Some(complete.iter().filter_map(|r| r.area_m2).sum()));
            total[6] = Cell::number(Some(complete.iter().filter_map(|r| r.volume_m3).sum()));
        }
        total[7] = Cell::plain(excluded(of.len() - complete.len()));
        totals.push(total);
    }
    table(
        TableKind::Strati,
        "Stratigrafie (m²)",
        &COLUMNS,
        rows,
        totals,
        vec![NOTE.to_string()],
    )
}

/// Le misure degli strati completi: i fattori, l'unità, lo stato e la fonte
/// della misura dell'elemento (`zone_record`, `room_record`), con il
/// materiale come descrizione e lo spessore nelle note.
pub(super) fn csv_records(abaco: &Abaco) -> Vec<CsvRecord> {
    let mut out = Vec::new();
    for row in abaco.strati.rows.iter().filter(|r| r.is_complete()) {
        let owner = match row.owner {
            Element::NewWall => abaco
                .nuove
                .iter()
                .find(|z| z.code == row.owner_code)
                .and_then(zone_record),
            Element::ProjectRoom => abaco
                .locali
                .iter()
                .filter_map(|line| line.progetto.as_ref())
                .find(|r| r.code == row.owner_code)
                .and_then(|r| room_record(r, None)),
        };
        let Some(owner) = owner else { continue };
        out.push(CsvRecord {
            tipo: match row.owner {
                Element::NewWall => "strato parete",
                Element::ProjectRoom => "strato solaio",
            },
            code: row.code.clone(),
            description: row.material.clone(),
            notes: vec![
                format!("sp. {} cm", row.thickness_cm),
                format!("stratigrafia {}", row.stratigraphy),
            ],
            ..owner
        });
    }
    out
}
