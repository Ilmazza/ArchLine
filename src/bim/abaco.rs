//! Abaco (spec parte 2, §4.3, §5.2): le righe delle zone e dei locali messe
//! insieme ([`compute_abaco`]) e rese come testo delle tre tabelle
//! ([`render_tables`]): Demolizioni, Nuove murature, Locali. Logica pura: chi
//! scrive le tabelle nel DWG (`AL_ABACO`) prende i [`TableText`] come sono.
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
//! - nelle tabelle due decimali con la virgola, totali sui valori non
//!   arrotondati; `n.d.` (cella `missing`) per ogni dato che manca, «-» per
//!   ciò che non si applica; l'altezza con il suo stato; lo stato
//!   «approssimata» e il motivo di ogni `n.d.` nelle note. Nel testo delle
//!   tabelle niente frecce né lineette tipografiche: i font SHX dello stile
//!   tabella non le hanno.

use codec::Document;

use super::extract::push_unique;
use super::height::{Height, HeightState};
use super::quantities::{group_new_by_thickness, zone_rows, ZoneKind, ZoneRow};
use super::quantities_rooms::{fatto_room_rows, project_room_rows, ProjectRoomRow, RoomRow};
use super::record;
use super::report::Notice;
use super::rooms::Phase;
use super::units::DrawingUnit;
use super::zone::zones_in;

const ND: &str = "n.d.";
/// Ciò che in una cella non si applica: non è un dato che manca.
const NOT_APPLICABLE: &str = "-";
const NOT_COMPUTED: &str = "non calcolata";
const INCOMPLETE: &str = "da completare";
const APPROXIMATE: &str = "approssimata";
const HEIGHT_ND: &str = "altezza n.d.";
const AREA_ND: &str = "superficie n.d.";
/// Tra le note di una cella.
const SEP: &str = "; ";
/// Tra i nomi dei locali uniti, come nei nomi composti dei locali di progetto.
const JOIN: &str = " + ";
/// Tra il valore di fatto e quello di progetto di una riga accoppiata.
const SIDES: &str = " / ";
/// Decimali nelle tabelle (spec §3).
const DECIMALS: usize = 2;
/// Decimali al massimo in [`format_fixed`]: con di più le unità dell'ultima
/// cifra di un numero di qualche miliardo uscirebbero da un `i64`.
const MAX_DECIMALS: usize = 9;

const DEMOLITION_COLUMNS: [&str; 8] = [
    "Codice",
    "Lunghezza (m)",
    "Spessore (m)",
    "Area in pianta (m²)",
    "Altezza (m)",
    "Parete (m²)",
    "Volume (m³)",
    "Note",
];
const NEW_WALL_COLUMNS: [&str; 9] = [
    "Codice",
    "Lunghezza (m)",
    "Spessore (m)",
    "Area in pianta (m²)",
    "Altezza (m)",
    "Superficie (m²)",
    "Volume (m³)",
    "Detrazione vani",
    "Note",
];
const ROOM_COLUMNS: [&str; 9] = [
    "Codice",
    "Locale",
    "Fatto (m²)",
    "Progetto (m²)",
    "Variazione (m²)",
    "Perimetro (m)",
    "Volume (m³)",
    "Altezza (m)",
    "Note",
];

const VARCHI_HEADING: &str = "Varchi (fuori dal totale delle demolizioni)";
const TOTAL_DEMOLITIONS: &str = "Totale demolizioni (righe complete)";
const TOTAL_VARCHI: &str = "Totale varchi (righe complete)";
const TOTAL_NEW_WALLS: &str = "Totale nuove murature (righe complete)";
const TOTAL_FATTO: &str = "Totale stato di fatto (righe complete)";
const TOTAL_PROGETTO: &str = "Totale stato di progetto (righe complete)";

const NOTE_ROUNDING: &str = "Valori arrotondati a due decimali; i totali sono calcolati sui \
    valori non arrotondati e possono non coincidere con la somma delle righe.";
const NOTE_ND: &str = "n.d. = dato non disponibile, con il motivo nelle note; le righe da \
    completare restano fuori dai totali.";
const NOTE_CROSSINGS: &str = "Agli angoli e agli incroci le zone nuove si sovrappongono: \
    lunghezze, superfici e volumi sono la somma delle zone come disegnate, non corretti.";
const NOTE_OPENINGS: &str =
    "Detrazione vani non calcolata: porte e finestre non sono ancora nel disegno.";
const NOTE_VARIATION: &str = "Variazione = progetto meno fatto, solo per un locale di fatto e \
    uno di progetto che si corrispondono uno a uno; in quelle righe codice, nome, perimetro, \
    volume e altezza sono fatto / progetto quando differiscono.";
const NOTE_MERGED: &str = "Locali uniti o divisi: righe separate, senza somme e senza variazione.";

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

/// Le tre tabelle dell'abaco.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableKind {
    Demolizioni,
    Nuove,
    Locali,
}

/// Il testo di una cella; `missing` = un dato che manca (`n.d.`), da
/// evidenziare.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub text: String,
    pub missing: bool,
}

/// Una tabella d'abaco come testo: ogni riga e ogni totale ha una cella per
/// colonna.
#[derive(Clone, Debug, PartialEq)]
pub struct TableText {
    pub kind: TableKind,
    /// Tipo e unità della quantità principale.
    pub title: String,
    pub columns: Vec<String>,
    /// Righe di dati, con le righe di intestazione (sezione dei varchi,
    /// gruppo di spessore) e i totali dei gruppi al loro posto: queste hanno
    /// l'etichetta nella prima cella.
    pub rows: Vec<Vec<Cell>>,
    /// I totali in fondo: etichetta con «righe complete» nella prima cella e,
    /// nelle note, quante righe restano fuori.
    pub totals: Vec<Vec<Cell>>,
    pub notes: Vec<String>,
}

impl Cell {
    fn plain(text: impl Into<String>) -> Self {
        Cell {
            text: text.into(),
            missing: false,
        }
    }

    fn empty() -> Self {
        Cell::plain("")
    }

    fn nd() -> Self {
        Cell {
            text: ND.to_string(),
            missing: true,
        }
    }

    fn not_applicable() -> Self {
        Cell::plain(NOT_APPLICABLE)
    }

    /// Il numero con due decimali, `n.d.` se manca (o non è finito).
    fn number(value: Option<f64>) -> Self {
        match value.filter(|v| v.is_finite()) {
            Some(v) => Cell::plain(format_fixed(v, DECIMALS)),
            None => Cell::nd(),
        }
    }

    fn notes(notes: Vec<String>) -> Self {
        Cell::plain(notes.join(SEP))
    }
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
fn room_incomplete_reason(row: &RoomRow) -> Option<String> {
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

/// L'altezza manca senza un motivo suo.
fn height_unexplained(height: &Height) -> bool {
    height.value_m.is_none() && height.note.as_deref().map_or(true, |n| n.trim().is_empty())
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

/// Le tabelle dell'abaco, nell'ordine Demolizioni, Nuove murature, Locali;
/// una tabella senza righe non c'è.
///
/// - **Demolizioni** (m³): una riga per demolizione, poi l'intestazione dei
///   varchi e una riga per varco; in fondo il totale delle demolizioni
///   (volume e parete) e, a parte, quello dei varchi;
/// - **Nuove murature** (m²): per ogni spessore (al millimetro, «spessore
///   n.d.» in fondo) l'intestazione, le righe e il totale del gruppo; la
///   colonna «Detrazione vani» dice «non calcolata» (spec §4.4, mai zero); in
///   fondo il totale generale e la nota sugli incroci;
/// - **Locali** (m²): una riga per [`LocaleLine`]; in fondo il totale dello
///   stato di fatto e quello dello stato di progetto.
///
/// Ogni totale somma le sole righe complete e lo dice; ogni tabella ha la
/// nota sull'arrotondamento e, se ha un `n.d.`, quella che lo spiega.
pub fn render_tables(abaco: &Abaco) -> Vec<TableText> {
    let mut tables = Vec::new();
    if !abaco.demolizioni.is_empty() || !abaco.varchi.is_empty() {
        tables.push(demolition_table(&abaco.demolizioni, &abaco.varchi));
    }
    if !abaco.nuove.is_empty() {
        tables.push(new_walls_table(&abaco.nuove));
    }
    if !abaco.locali.is_empty() {
        tables.push(rooms_table(&abaco.locali));
    }
    tables
}

/// La tabella con le note comuni in fondo a quelle date.
fn table(
    kind: TableKind,
    title: &str,
    columns: &[&str],
    rows: Vec<Vec<Cell>>,
    totals: Vec<Vec<Cell>>,
    mut notes: Vec<String>,
) -> TableText {
    if rows
        .iter()
        .chain(&totals)
        .flatten()
        .any(|cell| cell.missing)
    {
        notes.push(NOTE_ND.to_string());
    }
    notes.push(NOTE_ROUNDING.to_string());
    TableText {
        kind,
        title: title.to_string(),
        columns: columns.iter().map(|column| column.to_string()).collect(),
        rows,
        totals,
        notes,
    }
}

/// Una riga di intestazione o di totale di gruppo: l'etichetta nella prima
/// cella, le altre vuote.
fn label_row(text: String, missing: bool, width: usize) -> Vec<Cell> {
    let mut row = vec![Cell::empty(); width];
    row[0] = Cell { text, missing };
    row
}

fn demolition_table(demolizioni: &[ZoneRow], varchi: &[ZoneRow]) -> TableText {
    let mut rows: Vec<Vec<Cell>> = demolizioni.iter().map(demolition_cells).collect();
    let mut totals = Vec::new();
    if !demolizioni.is_empty() {
        totals.push(demolition_total(TOTAL_DEMOLITIONS, demolizioni, true));
    }
    if !varchi.is_empty() {
        rows.push(label_row(
            VARCHI_HEADING.to_string(),
            false,
            DEMOLITION_COLUMNS.len(),
        ));
        rows.extend(varchi.iter().map(demolition_cells));
        totals.push(demolition_total(TOTAL_VARCHI, varchi, false));
    }
    table(
        TableKind::Demolizioni,
        "Demolizioni (m³)",
        &DEMOLITION_COLUMNS,
        rows,
        totals,
        Vec::new(),
    )
}

/// Le prime cinque colonne delle tabelle delle zone: codice, lunghezza,
/// spessore, area in pianta, altezza.
fn zone_cells(row: &ZoneRow) -> Vec<Cell> {
    vec![
        Cell::plain(&row.code),
        Cell::number(row.length_m),
        Cell::number(row.thickness_m),
        // 0 = area non calcolabile (zona degenere, contorno non semplice).
        Cell::number(Some(row.plan_area_m2).filter(|area| *area > 0.0)),
        height_cell(&row.height),
    ]
}

/// Una demolizione o un varco (la parete di un varco non si applica).
fn demolition_cells(row: &ZoneRow) -> Vec<Cell> {
    let wall = match row.kind {
        ZoneKind::Demolizione => Cell::number(row.wall_area_m2),
        ZoneKind::Varco | ZoneKind::Nuova => Cell::not_applicable(),
    };
    let mut cells = zone_cells(row);
    cells.extend([
        wall,
        Cell::number(row.volume_m3),
        Cell::notes(zone_notes(row)),
    ]);
    cells
}

/// Il totale di volume (e, con `with_wall`, di parete) delle righe complete:
/// quante righe restano fuori e quante righe complete non hanno la parete.
fn demolition_total(label: &str, rows: &[ZoneRow], with_wall: bool) -> Vec<Cell> {
    let complete: Vec<&ZoneRow> = rows.iter().filter(|row| row.is_complete()).collect();
    let mut notes = vec![excluded(rows.len() - complete.len())];
    let wall = if with_wall {
        let without = complete
            .iter()
            .filter(|row| row.wall_area_m2.is_none())
            .count();
        if without > 0 {
            notes.push(count(
                without,
                "riga completa senza parete",
                "righe complete senza parete",
            ));
        }
        Cell::number(Some(
            complete.iter().filter_map(|row| row.wall_area_m2).sum(),
        ))
    } else {
        Cell::not_applicable()
    };
    let volume: f64 = complete.iter().filter_map(|row| row.volume_m3).sum();
    vec![
        Cell::plain(label),
        Cell::empty(),
        Cell::empty(),
        Cell::empty(),
        Cell::empty(),
        wall,
        Cell::number(Some(volume)),
        Cell::plain(notes.join(SEP)),
    ]
}

fn new_walls_table(rows: &[ZoneRow]) -> TableText {
    let mut out = Vec::new();
    for group in group_new_by_thickness(rows) {
        let members: Vec<&ZoneRow> = group.rows.iter().map(|&index| &rows[index]).collect();
        let name = match group.thickness_mm {
            Some(mm) => format!("{} m", format_fixed(mm as f64 / 1000.0, 3)),
            None => ND.to_string(),
        };
        out.push(label_row(
            format!("Spessore {name}"),
            group.thickness_mm.is_none(),
            NEW_WALL_COLUMNS.len(),
        ));
        out.extend(members.iter().map(|row| new_wall_cells(row)));
        out.push(new_walls_total(
            &format!("Totale spessore {name} (righe complete)"),
            &members,
        ));
    }
    let all: Vec<&ZoneRow> = rows.iter().collect();
    table(
        TableKind::Nuove,
        "Nuove murature (m²)",
        &NEW_WALL_COLUMNS,
        out,
        vec![new_walls_total(TOTAL_NEW_WALLS, &all)],
        vec![NOTE_CROSSINGS.to_string(), NOTE_OPENINGS.to_string()],
    )
}

/// Una nuova muratura: senza asse ha area in pianta e volume, ma la
/// superficie (la quantità principale) è `n.d.`.
fn new_wall_cells(row: &ZoneRow) -> Vec<Cell> {
    let mut cells = zone_cells(row);
    cells.extend([
        Cell::number(row.surface_m2),
        Cell::number(row.volume_m3),
        Cell::plain(NOT_COMPUTED),
        Cell::notes(zone_notes(row)),
    ]);
    cells
}

/// Lunghezza, superficie e volume delle righe complete, e quante restano
/// fuori.
fn new_walls_total(label: &str, rows: &[&ZoneRow]) -> Vec<Cell> {
    let complete: Vec<&ZoneRow> = rows
        .iter()
        .copied()
        .filter(|row| row.is_complete())
        .collect();
    let sum = |value: fn(&ZoneRow) -> Option<f64>| -> f64 {
        complete.iter().filter_map(|row| value(row)).sum()
    };
    vec![
        Cell::plain(label),
        Cell::number(Some(sum(|row| row.length_m))),
        Cell::empty(),
        Cell::empty(),
        Cell::empty(),
        Cell::number(Some(sum(|row| row.surface_m2))),
        Cell::number(Some(sum(|row| row.volume_m3))),
        Cell::plain(NOT_COMPUTED),
        Cell::plain(excluded(rows.len() - complete.len())),
    ]
}

fn rooms_table(lines: &[LocaleLine]) -> TableText {
    let rows = lines.iter().map(locale_cells).collect();
    let fatto: Vec<&RoomRow> = lines.iter().filter_map(|l| l.fatto.as_ref()).collect();
    let progetto: Vec<&RoomRow> = lines.iter().filter_map(|l| l.progetto.as_ref()).collect();
    let mut totals = Vec::new();
    if !fatto.is_empty() {
        totals.push(rooms_total(TOTAL_FATTO, &fatto, Phase::Fatto));
    }
    if !progetto.is_empty() {
        totals.push(rooms_total(TOTAL_PROGETTO, &progetto, Phase::Progetto));
    }
    let mut notes = Vec::new();
    if lines
        .iter()
        .any(|line| line.fatto.is_some() && line.progetto.is_some())
    {
        notes.push(NOTE_VARIATION.to_string());
    }
    if lines.iter().any(|line| line.note.is_some()) {
        notes.push(NOTE_MERGED.to_string());
    }
    table(
        TableKind::Locali,
        "Locali (m²)",
        &ROOM_COLUMNS,
        rows,
        totals,
        notes,
    )
}

/// Una riga di locale: superfici di fatto e di progetto nelle loro colonne
/// («-» se la riga non ha quel lato), variazione solo nelle righe
/// accoppiate, le altre grandezze «fatto / progetto» se differiscono.
fn locale_cells(line: &LocaleLine) -> Vec<Cell> {
    let (fatto, progetto) = (line.fatto.as_ref(), line.progetto.as_ref());
    let both = |cell: fn(&RoomRow) -> Cell| sides(fatto.map(cell), progetto.map(cell));
    let area = |row: Option<&RoomRow>| {
        row.map_or_else(Cell::not_applicable, |row| Cell::number(row.area_m2))
    };
    let variation = if fatto.is_some() && progetto.is_some() {
        signed(line.variation_m2)
    } else {
        Cell::not_applicable()
    };
    vec![
        both(|row| Cell::plain(&row.code)),
        both(|row| Cell::plain(&row.name)),
        area(fatto),
        area(progetto),
        variation,
        both(|row| Cell::number(row.perimeter_m)),
        both(|row| Cell::number(row.volume_m3)),
        both(|row| height_cell(&row.height)),
        Cell::notes(locale_notes(line)),
    ]
}

/// La cella di una grandezza di fatto e di progetto: una sola se coincidono
/// (o se c'è un lato solo), altrimenti «fatto / progetto».
fn sides(fatto: Option<Cell>, progetto: Option<Cell>) -> Cell {
    match (fatto, progetto) {
        (Some(f), Some(p)) if f.text != p.text => Cell {
            text: format!("{}{SIDES}{}", f.text, p.text),
            missing: f.missing || p.missing,
        },
        (Some(cell), _) | (None, Some(cell)) => cell,
        (None, None) => Cell::not_applicable(),
    }
}

/// La variazione con il segno («+0,60», «-0,30», «0,00»).
fn signed(value: Option<f64>) -> Cell {
    match value.filter(|v| v.is_finite()) {
        Some(v) => {
            let text = format_fixed(v, DECIMALS);
            let zero = text.chars().all(|c| c == '0' || c == ',');
            Cell::plain(if zero || text.starts_with('-') {
                text
            } else {
                format!("+{text}")
            })
        }
        None => Cell::nd(),
    }
}

/// Il totale di uno stato (superficie nella sua colonna, perimetro e volume)
/// sulle righe con la superficie: quante restano fuori e quante non hanno il
/// volume.
fn rooms_total(label: &str, rows: &[&RoomRow], phase: Phase) -> Vec<Cell> {
    let complete: Vec<&RoomRow> = rows
        .iter()
        .copied()
        .filter(|row| row.area_m2.is_some())
        .collect();
    let sum = |value: fn(&RoomRow) -> Option<f64>| -> f64 {
        complete.iter().filter_map(|row| value(row)).sum()
    };
    let mut notes = vec![excluded(rows.len() - complete.len())];
    let without = complete
        .iter()
        .filter(|row| row.volume_m3.is_none())
        .count();
    if without > 0 {
        notes.push(count(
            without,
            "riga senza volume (altezza n.d.)",
            "righe senza volume (altezza n.d.)",
        ));
    }
    let area = Cell::number(Some(sum(|row| row.area_m2)));
    let (fatto, progetto) = match phase {
        Phase::Fatto => (area, Cell::empty()),
        Phase::Progetto => (Cell::empty(), area),
    };
    vec![
        Cell::plain(label),
        Cell::empty(),
        fatto,
        progetto,
        Cell::empty(),
        Cell::number(Some(sum(|row| row.perimeter_m))),
        Cell::number(Some(sum(|row| row.volume_m3))),
        Cell::empty(),
        Cell::plain(notes.join(SEP)),
    ]
}

/// «2,70 (ereditata)», «3,00 (inserita)» o `n.d.`.
fn height_cell(height: &Height) -> Cell {
    let state = match height.state {
        HeightState::Inserita => "inserita",
        HeightState::Ereditata => "ereditata",
        HeightState::NonDisponibile => return Cell::nd(),
    };
    match height.value_m.filter(|v| v.is_finite()) {
        Some(v) => Cell::plain(format!("{} ({state})", format_fixed(v, DECIMALS))),
        None => Cell::nd(),
    }
}

/// Aggiunge una nota non vuota che non c'è ancora.
fn push_note(notes: &mut Vec<String>, text: Option<&str>) {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return;
    };
    if !notes.iter().any(|note| note == text) {
        notes.push(text.to_string());
    }
}

/// Le note di una zona: lo stato «approssimata», poi il motivo di una riga
/// incompleta («da completare: …») o le note della riga.
fn zone_notes(row: &ZoneRow) -> Vec<String> {
    let mut notes = Vec::new();
    if row.approximate {
        push_note(&mut notes, Some(APPROXIMATE));
    }
    match row.incomplete_reason() {
        Some(reason) => push_note(&mut notes, Some(format!("{INCOMPLETE}: {reason}").as_str())),
        None => {
            push_note(&mut notes, row.note.as_deref());
            push_note(&mut notes, row.height.note.as_deref());
        }
    }
    notes
}

/// Le grandezze approssimate di un locale misurato (spec §4.2): con la
/// superficie anche perimetro e volume; il perimetro anche da solo, sulle
/// corde degli archi.
fn room_state(row: &RoomRow) -> Option<String> {
    row.area_m2?;
    let mut quantities = Vec::new();
    if row.approximate {
        quantities.push("superficie");
    }
    if row.approximate || row.perimeter_approximate {
        quantities.push("perimetro");
    }
    if row.approximate && row.volume_m3.is_some() {
        quantities.push("volume");
    }
    (!quantities.is_empty()).then(|| format!("{APPROXIMATE} ({})", quantities.join(", ")))
}

/// Le note di un locale: lo stato «approssimata», poi il motivo di una riga
/// incompleta («da completare: …») o le note della riga e dell'altezza
/// («altezza n.d.» se il volume manca senza un motivo).
fn room_notes(row: &RoomRow) -> Vec<String> {
    let mut notes = Vec::new();
    push_note(&mut notes, room_state(row).as_deref());
    match room_incomplete_reason(row) {
        Some(reason) => push_note(&mut notes, Some(format!("{INCOMPLETE}: {reason}").as_str())),
        None => {
            push_note(&mut notes, row.note.as_deref());
            push_note(&mut notes, row.height.note.as_deref());
            if height_unexplained(&row.height) {
                push_note(&mut notes, Some(HEIGHT_ND));
            }
        }
    }
    notes
}

/// Le note di una riga di locali: quella dell'accoppiamento, poi quelle dei
/// locali (con «fatto:» e «progetto:» davanti in una riga accoppiata).
fn locale_notes(line: &LocaleLine) -> Vec<String> {
    let mut notes: Vec<String> = line.note.iter().cloned().collect();
    match (&line.fatto, &line.progetto) {
        (Some(fatto), Some(progetto)) => {
            notes.extend(room_notes(fatto).into_iter().map(|n| format!("fatto: {n}")));
            notes.extend(
                room_notes(progetto)
                    .into_iter()
                    .map(|n| format!("progetto: {n}")),
            );
        }
        (Some(row), None) | (None, Some(row)) => notes.extend(room_notes(row)),
        (None, None) => {}
    }
    notes
}

/// «1 riga esclusa», «3 righe escluse», «nessuna riga esclusa».
fn excluded(n: usize) -> String {
    if n == 0 {
        "nessuna riga esclusa".to_string()
    } else {
        count(n, "riga esclusa", "righe escluse")
    }
}

fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
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

#[cfg(test)]
mod tests {
    //! I casi della spec parte 2, §7 (A, B, G, H, I), metri, altezza
    //! predefinita 2,70 dove serve, nelle due forme del rilievo.
    use super::*;

    use codec::{EntityType, Vector3};

    use crate::bim::extract::{extract, Elaborato, ExtractOptions};
    use crate::bim::fixtures::{case_a, scene_with, shift_tramezzo, Form, LAYER_MURI};
    use crate::bim::quantities::ZoneKind;
    use crate::bim::record::{self, ProjectRecord};
    use crate::bim::refresh::refresh_rooms;
    use crate::bim::room_command::test_support::write_room;
    use crate::bim::rooms::label;
    use crate::bim::walls::{rect_from_axis, wall_set_without_rooms};
    use crate::bim::zone::{ensure_layers, zone_hatch, Zone, ZoneOrigin, ZoneState};
    use crate::bim::{LAYER_LOCALI, MAX_WALL_THICKNESS_M};
    use crate::scene::Scene;

    const M: DrawingUnit = DrawingUnit::Metri;
    const TOL: f64 = 1e-6;
    const FORMS: [Form; 2] = [Form::Continua, Form::ChiusaPerMuro];
    const OPTS: AbacoOptions = AbacoOptions {
        unit: M,
        tol: TOL,
        room_tol: TOL,
    };

    /// Il caso A nella forma data, in metri, con i layer `AL-*`.
    fn scene_of(form: Form) -> Scene {
        let mut scene = scene_with(case_a(form, M), M.insunits());
        ensure_layers(&mut scene.document);
        scene
    }

    /// Un disegno in metri senza rilievo, con i layer `AL-*`.
    fn empty_scene() -> Scene {
        let mut scene = scene_with(Vec::new(), M.insunits());
        ensure_layers(&mut scene.document);
        scene
    }

    /// L'altezza predefinita nel record di progetto, come `AL_ALTEZZA L`.
    fn set_default_height(scene: &mut Scene, height: f64) {
        let rec = ProjectRecord {
            default_height: Some(height),
            ..record::read(&scene.document)
        };
        record::write(&mut scene.document, &rec);
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

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    /// Una zona sull'anello `ring`, con o senza asse.
    fn add_zone(
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

    /// Ogni riga e ogni totale ha una cella per colonna.
    #[track_caller]
    fn check_shape(table: &TableText) {
        for row in table.rows.iter().chain(&table.totals) {
            assert_eq!(row.len(), table.columns.len(), "{:?}: {row:?}", table.kind);
        }
    }

    fn table_of(tables: &[TableText], kind: TableKind) -> &TableText {
        tables
            .iter()
            .find(|table| table.kind == kind)
            .unwrap_or_else(|| panic!("nessuna tabella {kind:?}"))
    }

    /// L'indice della colonna la cui intestazione comincia con `header`.
    #[track_caller]
    fn col(table: &TableText, header: &str) -> usize {
        table
            .columns
            .iter()
            .position(|column| column.starts_with(header))
            .unwrap_or_else(|| panic!("nessuna colonna {header}: {:?}", table.columns))
    }

    /// La riga di dati con il codice `code` nella prima colonna.
    #[track_caller]
    fn row_of<'a>(table: &'a TableText, code: &str) -> &'a [Cell] {
        table
            .rows
            .iter()
            .find(|row| row[0].text == code)
            .unwrap_or_else(|| panic!("nessuna riga {code}: {:?}", table.rows))
    }

    /// La cella della riga `row` sotto la colonna `header`.
    #[track_caller]
    fn cell<'a>(table: &TableText, row: &'a [Cell], header: &str) -> &'a Cell {
        &row[col(table, header)]
    }

    #[track_caller]
    fn nd(cell: &Cell, what: &str) {
        assert_eq!(cell.text, "n.d.", "{what}");
        assert!(cell.missing, "{what}: n.d. evidenziato");
    }

    #[track_caller]
    fn approx(value: Option<f64>, want: f64, what: &str) {
        assert!(
            value.is_some_and(|v| (v - want).abs() < 1e-9),
            "{what}: {value:?} invece di {want}"
        );
    }

    fn incomplete(abaco: &Abaco) -> Vec<(String, String)> {
        abaco
            .notices
            .iter()
            .filter_map(|notice| match notice {
                Notice::RigaIncompleta { code, reason } => Some((code.clone(), reason.clone())),
                _ => None,
            })
            .collect()
    }

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

    /// Spec §7, caso A demolito (altezza ereditata 2,70): una tabella sola,
    /// Demolizioni, con la riga D1 (volume 0,81, parete 8,10) e il totale
    /// 0,81; nessuna tabella Nuove né Locali senza zone nuove e senza locali.
    #[test]
    fn case_a_demolition_tables() {
        for form in FORMS {
            let mut scene = scene_of(form);
            set_default_height(&mut scene, 2.7);
            demolish(&mut scene, [4.05, 0.0], [4.05, 3.0]);
            let abaco = compute_abaco(&scene.document, &OPTS);
            assert_eq!(abaco.demolizioni.len(), 1, "{form:?}");
            assert!(
                abaco.varchi.is_empty() && abaco.nuove.is_empty() && abaco.locali.is_empty(),
                "{form:?}: {abaco:?}"
            );
            assert!(incomplete(&abaco).is_empty(), "{form:?}");

            let tables = render_tables(&abaco);
            assert_eq!(tables.len(), 1, "{form:?}: {tables:?}");
            let t = &tables[0];
            check_shape(t);
            assert_eq!(t.kind, TableKind::Demolizioni, "{form:?}");
            assert_eq!(t.title, "Demolizioni (m³)", "{form:?}");
            let d1 = row_of(t, "D1");
            for (header, want) in [
                ("Lunghezza", "3,00"),
                ("Spessore", "0,10"),
                ("Area in pianta", "0,30"),
                ("Altezza", "2,70 (ereditata)"),
                ("Parete", "8,10"),
                ("Volume", "0,81"),
            ] {
                assert_eq!(cell(t, d1, header).text, want, "{form:?}: {header}");
            }
            assert!(d1.iter().all(|c| !c.missing), "{form:?}: {d1:?}");
            let [total] = &t.totals[..] else {
                panic!("{form:?}: {:?}", t.totals)
            };
            assert!(total[0].text.contains("righe complete"), "{form:?}");
            assert_eq!(cell(t, total, "Volume").text, "0,81", "{form:?}");
            assert_eq!(cell(t, total, "Parete").text, "8,10", "{form:?}");
            assert_eq!(
                cell(t, total, "Note").text,
                "nessuna riga esclusa",
                "{form:?}"
            );
            assert!(
                t.notes.iter().any(|n| n.contains("arrotondat")),
                "{form:?}: {:?}",
                t.notes
            );
            assert!(
                !t.notes.iter().any(|n| n.contains("incroci")),
                "{form:?}: niente zone nuove"
            );
        }
    }

    /// Spec §7, caso H: senza altezza la riga resta con `n.d.` evidenziato,
    /// la nota «da completare» e il suo avviso, fuori dal totale (0,00, «1
    /// riga esclusa»); la geometria resta.
    #[test]
    fn incomplete_rows_are_flagged_and_excluded_from_totals() {
        for form in FORMS {
            let mut scene = scene_of(form);
            demolish(&mut scene, [4.05, 0.0], [4.05, 3.0]);
            let abaco = compute_abaco(&scene.document, &OPTS);
            assert_eq!(
                incomplete(&abaco),
                [("D1".to_string(), "altezza n.d.".to_string())],
                "{form:?}"
            );
            let tables = render_tables(&abaco);
            let t = table_of(&tables, TableKind::Demolizioni);
            check_shape(t);
            let d1 = row_of(t, "D1");
            for header in ["Altezza", "Parete", "Volume"] {
                nd(cell(t, d1, header), &format!("{form:?}: {header}"));
            }
            assert_eq!(cell(t, d1, "Lunghezza").text, "3,00", "{form:?}");
            assert!(!cell(t, d1, "Lunghezza").missing, "{form:?}");
            assert_eq!(
                cell(t, d1, "Note").text,
                "da completare: altezza n.d.",
                "{form:?}"
            );
            let total = &t.totals[0];
            assert_eq!(cell(t, total, "Volume").text, "0,00", "{form:?}");
            assert_eq!(cell(t, total, "Parete").text, "0,00", "{form:?}");
            assert!(
                cell(t, total, "Note").text.contains("1 riga esclusa"),
                "{form:?}: {total:?}"
            );
            assert!(
                t.notes.iter().any(|n| n.starts_with("n.d.")),
                "{form:?}: {:?}",
                t.notes
            );
        }
    }

    /// Ruling del Task 6: una demolizione senza asse è completa ma senza
    /// parete (`n.d.`), il totale delle pareti conta solo le righe che la
    /// hanno e dice quante no; i varchi stanno in fondo, con il loro totale,
    /// fuori da quello delle demolizioni; un varco senza altezza propria è
    /// incompleto anche con la predefinita.
    #[test]
    fn demolition_without_axis_and_varchi_totals() {
        let mut scene = empty_scene();
        set_default_height(&mut scene, 2.7);
        let demolito = ZoneState::Demolito;
        add_zone(
            &mut scene,
            demolito,
            Some(([4.05, 0.0], [4.05, 3.0])),
            None,
            false,
            rect(4.0, 0.0, 4.1, 3.0),
        );
        add_zone(
            &mut scene,
            demolito,
            None,
            None,
            false,
            rect(10.0, 0.0, 10.1, 3.0),
        );
        add_zone(
            &mut scene,
            demolito,
            Some(([20.05, 1.0], [20.05, 1.9])),
            Some(2.1),
            true,
            rect(20.0, 1.0, 20.1, 1.9),
        );
        add_zone(
            &mut scene,
            demolito,
            Some(([30.05, 1.0], [30.05, 1.9])),
            None,
            true,
            rect(30.0, 1.0, 30.1, 1.9),
        );
        let abaco = compute_abaco(&scene.document, &OPTS);
        assert_eq!((abaco.demolizioni.len(), abaco.varchi.len()), (2, 2));
        let reasons = incomplete(&abaco);
        let [(code, reason)] = &reasons[..] else {
            panic!("{reasons:?}")
        };
        assert_eq!(code, "V2");
        assert!(reason.contains("altezza del varco da inserire"), "{reason}");

        let tables = render_tables(&abaco);
        assert_eq!(tables.len(), 1, "{tables:?}");
        let t = &tables[0];
        check_shape(t);
        let codes: Vec<&str> = t.rows.iter().map(|row| row[0].text.as_str()).collect();
        assert_eq!(codes.len(), 5, "{codes:?}");
        assert_eq!(codes[..2], ["D1", "D2"]);
        assert!(codes[2].starts_with("Varchi"), "{codes:?}");
        assert_eq!(codes[3..], ["V1", "V2"]);

        let d2 = row_of(t, "D2");
        assert_eq!(cell(t, d2, "Volume").text, "0,81");
        for header in ["Lunghezza", "Spessore", "Parete"] {
            nd(cell(t, d2, header), header);
        }
        assert_eq!(cell(t, d2, "Note").text, "zona senza asse");
        let v1 = row_of(t, "V1");
        assert_eq!(cell(t, v1, "Volume").text, "0,19");
        assert_eq!(cell(t, v1, "Altezza").text, "2,10 (inserita)");
        assert_eq!(cell(t, v1, "Parete").text, "-");
        assert!(!cell(t, v1, "Parete").missing, "non si applica");
        nd(cell(t, row_of(t, "V2"), "Volume"), "V2");

        let [demolitions, varchi] = &t.totals[..] else {
            panic!("{:?}", t.totals)
        };
        assert!(demolitions[0].text.starts_with("Totale demolizioni"));
        assert!(demolitions[0].text.contains("righe complete"));
        assert_eq!(cell(t, demolitions, "Volume").text, "1,62");
        assert_eq!(cell(t, demolitions, "Parete").text, "8,10");
        assert_eq!(
            cell(t, demolitions, "Note").text,
            "nessuna riga esclusa; 1 riga completa senza parete"
        );
        assert!(varchi[0].text.starts_with("Totale varchi"));
        assert_eq!(cell(t, varchi, "Volume").text, "0,19");
        assert_eq!(cell(t, varchi, "Parete").text, "-");
        assert_eq!(cell(t, varchi, "Note").text, "1 riga esclusa");
    }

    /// Spec §7, caso G: nuove murature per spessore, con intestazione e
    /// totale di gruppo, «Detrazione vani» = «non calcolata», il totale
    /// generale e la nota sugli incroci. Una zona nuova senza asse resta nel
    /// gruppo «spessore n.d.» con area in pianta e volume, ma la superficie è
    /// `n.d.` e la riga è fuori dai totali.
    #[test]
    fn new_wall_group_totals() {
        let mut scene = empty_scene();
        set_default_height(&mut scene, 2.7);
        let nuovo = ZoneState::Nuovo;
        for y in [0.0, 1.0] {
            let axis = Some(([0.0, y + 0.04], [2.0, y + 0.04]));
            add_zone(
                &mut scene,
                nuovo,
                axis,
                None,
                false,
                rect(0.0, y, 2.0, y + 0.08),
            );
        }
        let axis = Some(([0.0, 3.06], [2.0, 3.06]));
        add_zone(
            &mut scene,
            nuovo,
            axis,
            None,
            false,
            rect(0.0, 3.0, 2.0, 3.12),
        );
        add_zone(
            &mut scene,
            nuovo,
            None,
            None,
            false,
            rect(0.0, 5.0, 2.0, 5.08),
        );
        let abaco = compute_abaco(&scene.document, &OPTS);
        assert_eq!(abaco.nuove.len(), 4);
        assert!(abaco.nuove.iter().all(|row| row.kind == ZoneKind::Nuova));
        assert_eq!(
            incomplete(&abaco),
            [("N4".to_string(), "zona senza asse".to_string())]
        );

        let tables = render_tables(&abaco);
        assert_eq!(tables.len(), 1, "{tables:?}");
        let t = table_of(&tables, TableKind::Nuove);
        check_shape(t);
        assert_eq!(t.title, "Nuove murature (m²)");
        let first: Vec<&str> = t.rows.iter().map(|row| row[0].text.as_str()).collect();
        assert_eq!(
            first,
            [
                "Spessore 0,080 m",
                "N1",
                "N2",
                "Totale spessore 0,080 m (righe complete)",
                "Spessore 0,120 m",
                "N3",
                "Totale spessore 0,120 m (righe complete)",
                "Spessore n.d.",
                "N4",
                "Totale spessore n.d. (righe complete)",
            ]
        );
        assert!(t.rows[7][0].missing, "spessore n.d. evidenziato");
        let n1 = row_of(t, "N1");
        assert_eq!(cell(t, n1, "Superficie").text, "5,40");
        assert_eq!(cell(t, n1, "Spessore").text, "0,08");
        assert_eq!(cell(t, n1, "Area in pianta").text, "0,16");
        assert_eq!(cell(t, n1, "Volume").text, "0,43");
        assert_eq!(cell(t, n1, "Detrazione vani").text, "non calcolata");
        let group = &t.rows[3];
        assert_eq!(cell(t, group, "Superficie").text, "10,80");
        assert_eq!(cell(t, group, "Lunghezza").text, "4,00");
        assert_eq!(cell(t, group, "Detrazione vani").text, "non calcolata");
        assert_eq!(cell(t, group, "Note").text, "nessuna riga esclusa");
        assert_eq!(cell(t, row_of(t, "N3"), "Superficie").text, "5,40");
        assert_eq!(cell(t, row_of(t, "N3"), "Volume").text, "0,65");

        let n4 = row_of(t, "N4");
        nd(cell(t, n4, "Superficie"), "N4");
        nd(cell(t, n4, "Lunghezza"), "N4");
        assert_eq!(cell(t, n4, "Volume").text, "0,43", "il volume resta");
        assert!(!cell(t, n4, "Volume").missing);
        assert_eq!(cell(t, n4, "Area in pianta").text, "0,16");
        assert_eq!(cell(t, n4, "Note").text, "da completare: zona senza asse");
        let nd_group = &t.rows[9];
        assert_eq!(cell(t, nd_group, "Superficie").text, "0,00");
        assert_eq!(cell(t, nd_group, "Note").text, "1 riga esclusa");

        let [total] = &t.totals[..] else {
            panic!("{:?}", t.totals)
        };
        assert!(total[0].text.contains("righe complete"));
        assert_eq!(cell(t, total, "Superficie").text, "16,20");
        assert_eq!(cell(t, total, "Lunghezza").text, "6,00");
        assert_eq!(cell(t, total, "Volume").text, "1,51");
        assert_eq!(cell(t, total, "Detrazione vani").text, "non calcolata");
        assert_eq!(cell(t, total, "Note").text, "1 riga esclusa");
        assert!(
            t.notes.iter().any(|n| n.contains("incroci")),
            "{:?}",
            t.notes
        );
        assert!(t.notes.iter().any(|n| n.contains("arrotondat")));
    }

    /// Spec §4.3: una zona nuova 0,10 × 3,00 addossata alla faccia interna
    /// ovest di A (rettangolo senza asse) restringe A di progetto a 11,70:
    /// riga accoppiata A di fatto 12,00 / di progetto 11,70, variazione
    /// −0,30; B accoppiato, variazione 0,00.
    #[test]
    fn locali_pairing_one_to_one_has_variation() {
        for form in FORMS {
            let mut scene = scene_of(form);
            set_default_height(&mut scene, 2.7);
            write_room(&mut scene, "A", [2.0, 1.5]);
            write_room(&mut scene, "B", [6.0, 1.5]);
            add_zone(
                &mut scene,
                ZoneState::Nuovo,
                None,
                None,
                false,
                rect(0.0, 0.0, 0.1, 3.0),
            );
            let abaco = compute_abaco(&scene.document, &OPTS);
            let [a, b] = &abaco.locali[..] else {
                panic!("{form:?}: {:?}", abaco.locali)
            };
            for (line, name, fatto, progetto, variation) in
                [(a, "A", 12.0, 11.7, -0.3), (b, "B", 11.7, 11.7, 0.0)]
            {
                let what = format!("{form:?} {name}");
                let (f, p) = (
                    line.fatto.as_ref().expect("fatto"),
                    line.progetto.as_ref().expect("progetto"),
                );
                assert_eq!((f.name.as_str(), p.name.as_str()), (name, name), "{what}");
                approx(f.area_m2, fatto, &format!("{what}: fatto"));
                approx(p.area_m2, progetto, &format!("{what}: progetto"));
                approx(line.variation_m2, variation, &format!("{what}: variazione"));
                assert_eq!(line.note, None, "{what}");
            }

            let tables = render_tables(&abaco);
            let kinds: Vec<TableKind> = tables.iter().map(|t| t.kind).collect();
            assert_eq!(kinds, [TableKind::Nuove, TableKind::Locali], "{form:?}");
            let t = table_of(&tables, TableKind::Locali);
            check_shape(t);
            assert_eq!(t.title, "Locali (m²)", "{form:?}");
            let row = row_of(t, "LF1 / LP1");
            for (header, want) in [
                ("Locale", "A"),
                ("Fatto", "12,00"),
                ("Progetto", "11,70"),
                ("Variazione", "-0,30"),
                ("Perimetro", "14,00 / 13,80"),
                ("Volume", "32,40 / 31,59"),
                ("Altezza", "2,70 (ereditata)"),
            ] {
                assert_eq!(cell(t, row, header).text, want, "{form:?}: {header}");
            }
            assert!(row.iter().all(|c| !c.missing), "{form:?}: {row:?}");
            assert_eq!(
                cell(t, row_of(t, "LF2 / LP2"), "Variazione").text,
                "0,00",
                "{form:?}"
            );
            let [fatto, progetto] = &t.totals[..] else {
                panic!("{form:?}: {:?}", t.totals)
            };
            assert!(fatto[0].text.contains("fatto") && fatto[0].text.contains("righe complete"));
            assert_eq!(cell(t, fatto, "Fatto").text, "23,70", "{form:?}");
            assert_eq!(cell(t, fatto, "Progetto").text, "", "{form:?}");
            assert!(progetto[0].text.contains("progetto"));
            assert_eq!(cell(t, progetto, "Progetto").text, "23,40", "{form:?}");
            assert_eq!(cell(t, progetto, "Volume").text, "63,18", "{form:?}");
            assert!(
                t.notes.iter().any(|n| n.contains("Variazione")),
                "{form:?}: {:?}",
                t.notes
            );
        }
    }

    /// Spec §4.3: A e B uniti dalla demolizione del tramezzo non sono un
    /// accoppiamento: tre righe separate («unito in A + B» due volte, «da A +
    /// B»), nessuna variazione e nessuna somma in riga.
    #[test]
    fn locali_merge_has_no_numeric_sum() {
        for form in FORMS {
            let mut scene = scene_of(form);
            set_default_height(&mut scene, 2.7);
            write_room(&mut scene, "A", [2.0, 1.5]);
            write_room(&mut scene, "B", [6.0, 1.5]);
            demolish(&mut scene, [4.05, 0.0], [4.05, 3.0]);
            let abaco = compute_abaco(&scene.document, &OPTS);
            let [a, b, merged] = &abaco.locali[..] else {
                panic!("{form:?}: {:?}", abaco.locali)
            };
            for (line, name) in [(a, "A"), (b, "B")] {
                let what = format!("{form:?} {name}");
                assert_eq!(
                    line.fatto.as_ref().map(|r| r.name.as_str()),
                    Some(name),
                    "{what}"
                );
                assert_eq!(line.progetto, None, "{what}");
                assert_eq!(line.variation_m2, None, "{what}");
                assert_eq!(line.note.as_deref(), Some("unito in A + B"), "{what}");
            }
            assert_eq!(merged.fatto, None, "{form:?}");
            let p = merged.progetto.as_ref().expect("progetto");
            approx(p.area_m2, 24.0, &format!("{form:?}"));
            assert_eq!(merged.variation_m2, None, "{form:?}");
            assert_eq!(merged.note.as_deref(), Some("da A + B"), "{form:?}");

            let tables = render_tables(&abaco);
            let t = table_of(&tables, TableKind::Locali);
            check_shape(t);
            let lf1 = row_of(t, "LF1");
            assert_eq!(cell(t, lf1, "Fatto").text, "12,00", "{form:?}");
            assert_eq!(cell(t, lf1, "Progetto").text, "-", "{form:?}");
            assert_eq!(cell(t, lf1, "Variazione").text, "-", "{form:?}");
            assert!(cell(t, lf1, "Note").text.contains("unito in A + B"));
            let lp1 = row_of(t, "LP1");
            assert_eq!(cell(t, lp1, "Fatto").text, "-", "{form:?}");
            assert_eq!(cell(t, lp1, "Progetto").text, "24,00", "{form:?}");
            assert_eq!(cell(t, lp1, "Variazione").text, "-", "{form:?}");
            let note = &cell(t, lp1, "Note").text;
            assert!(note.contains("da A + B"), "{form:?}: {note}");
            assert!(note.contains("da confermare"), "{form:?}: {note}");
            assert!(
                !t.rows
                    .iter()
                    .any(|row| row.iter().any(|c| c.text.contains("23,70"))),
                "{form:?}: nessuna somma in riga"
            );
        }
    }

    /// Un locale di fatto aperto (la faccia ovest di A tolta) resta in tabella
    /// con la superficie `n.d.`, la nota e il suo avviso, mai un numero; un
    /// muro fuori dal piano XY dà un solo [`Notice::FuoriPiano`], anche se lo
    /// trovano sia i locali di fatto sia quelli di progetto.
    #[test]
    fn open_fatto_room_and_off_plane_notice_once() {
        let mut scene = scene_of(Form::Continua);
        set_default_height(&mut scene, 2.7);
        write_room(&mut scene, "A", [2.0, 1.5]);
        write_room(&mut scene, "B", [6.0, 1.5]);
        let west = scene
            .document
            .entities()
            .find(|entity| match entity {
                EntityType::Line(l) => {
                    l.start.x.abs() < 1e-9
                        && (l.start.y - 3.0).abs() < 1e-9
                        && l.end.x.abs() < 1e-9
                        && l.end.y.abs() < 1e-9
                }
                _ => false,
            })
            .map(|entity| entity.common().handle)
            .expect("faccia ovest di A");
        scene.erase_entities(&[west]);
        let mut raised = codec::entities::Line::from_points(
            Vector3::new(5.0, 1.0, 1.0),
            Vector3::new(7.0, 1.0, 1.0),
        );
        raised.common.layer = LAYER_MURI.to_string();
        scene.add_entity(EntityType::Line(raised));

        let abaco = compute_abaco(&scene.document, &OPTS);
        let off_plane = abaco
            .notices
            .iter()
            .filter(|n| matches!(n, Notice::FuoriPiano { .. }))
            .count();
        assert_eq!(off_plane, 1, "{:?}", abaco.notices);
        let open = abaco
            .notices
            .iter()
            .filter(|n| matches!(n, Notice::LocaleNonChiuso { name } if name == "A"))
            .count();
        assert_eq!(open, 1, "{:?}", abaco.notices);
        let reasons = incomplete(&abaco);
        assert!(
            reasons.contains(&("LF1".to_string(), "non chiuso".to_string())),
            "{reasons:?}"
        );
        assert!(
            abaco.locali[0]
                .fatto
                .as_ref()
                .is_some_and(|r| r.area_m2.is_none()),
            "{:?}",
            abaco.locali
        );
        let tables = render_tables(&abaco);
        let t = table_of(&tables, TableKind::Locali);
        check_shape(t);
        let lf1 = row_of(t, "LF1");
        nd(cell(t, lf1, "Fatto"), "LF1");
        nd(cell(t, lf1, "Perimetro"), "LF1");
        assert_eq!(cell(t, lf1, "Note").text, "da completare: non chiuso");
        let fatto_total = &t.totals[0];
        assert_eq!(cell(t, fatto_total, "Fatto").text, "11,70");
        assert!(cell(t, fatto_total, "Note").text.contains("1 riga esclusa"));
    }

    /// Spec §4.2: lo stato «approssimata» si vede nella riga: per una zona
    /// (contorno ad arco) e per un locale, dove la superficie approssimata
    /// porta con sé perimetro e volume, e il solo perimetro sulle corde dice
    /// solo il perimetro.
    #[test]
    fn approximate_state_is_shown() {
        let mut scene = scene_of(Form::Continua);
        set_default_height(&mut scene, 2.7);
        write_room(&mut scene, "A", [2.0, 1.5]);
        demolish(&mut scene, [4.05, 0.0], [4.05, 3.0]);
        let mut abaco = compute_abaco(&scene.document, &OPTS);
        abaco.demolizioni[0].approximate = true;
        let mut fatto_line = abaco.locali[0].clone();
        assert!(fatto_line.fatto.is_some(), "{fatto_line:?}");
        fatto_line.fatto.as_mut().unwrap().approximate = true;
        let mut chords_line = fatto_line.clone();
        let row = chords_line.fatto.as_mut().unwrap();
        row.approximate = false;
        row.perimeter_approximate = true;
        abaco.locali = vec![fatto_line, chords_line];

        let tables = render_tables(&abaco);
        let t = table_of(&tables, TableKind::Demolizioni);
        let note = &cell(t, row_of(t, "D1"), "Note").text;
        assert!(note.starts_with("approssimata"), "{note}");
        let t = table_of(&tables, TableKind::Locali);
        let notes: Vec<&str> = t
            .rows
            .iter()
            .map(|row| cell(t, row, "Note").text.as_str())
            .collect();
        assert!(
            notes[0].contains("approssimata (superficie, perimetro, volume)"),
            "{notes:?}"
        );
        assert!(notes[1].contains("approssimata (perimetro)"), "{notes:?}");
    }

    #[test]
    fn empty_drawing_gives_no_tables() {
        for scene in [
            empty_scene(),
            scene_of(Form::Continua),
            scene_of(Form::ChiusaPerMuro),
        ] {
            let abaco = compute_abaco(&scene.document, &OPTS);
            assert_eq!(
                abaco,
                Abaco {
                    demolizioni: Vec::new(),
                    varchi: Vec::new(),
                    nuove: Vec::new(),
                    locali: Vec::new(),
                    notices: Vec::new(),
                }
            );
            assert!(render_tables(&abaco).is_empty());
        }
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
