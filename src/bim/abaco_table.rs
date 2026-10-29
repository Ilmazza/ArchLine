//! Le tabelle d'abaco come entità `TABLE` (spec parte 2, §5.2): la scrittura
//! di un [`TableText`] di `abaco::render_tables` ([`build`], [`stack`],
//! [`rebuilt`]) e il riconoscimento delle tabelle già nel disegno
//! ([`tables_in`]). Logica pura: quali tabelle riscrivere, dove mettere
//! quelle mancanti ([`plan`]) e il passo di annullamento li usa `AL_ABACO`
//! (`app/commands/bim_abaco.rs`).
//!
//! Regole:
//! - una tabella d'abaco è una `TABLE` dello spazio modello con il record
//!   XDATA `ARCHLINE_ABACO`: `Integer16(1)` (versione), `"tabella"`,
//!   `"demolizioni"|"nuove"|"locali"`; la lettura va per chiave;
//! - spike del Task 7 riuscito (`table_survives_dwg_roundtrip`): testo delle
//!   celle (anche «m³»), celle unite, punto d'inserimento e XDATA tornano da
//!   un DWG 2004, 2007, 2010, 2013 e 2018, quindi niente ripiego a linee e
//!   `MTEXT`. Come AutoCAD mostra queste tabelle lo vede Mauro;
//! - righe: titolo (una cella unita su tutte le colonne), intestazioni delle
//!   colonne, righe di dati, totali, note. `TableText` non dice quali righe
//!   sono etichette: qui la prima cella si unisce alle celle vuote che la
//!   seguono, così l'intestazione dei varchi e quella di un gruppo di spessore
//!   (tutte vuote dopo l'etichetta) prendono la riga intera e l'etichetta di
//!   un totale prende le colonne vuote alla sua destra. Le note vanno a capo
//!   sulla larghezza della tabella, una riga unita per riga di testo;
//! - dimensioni in unità del disegno: testo alto 0,25 m (come il ripiego dei
//!   testi dei locali, `rooms::DEFAULT_ROOM_TEXT_M`), override di ogni cella;
//!   righe alte il doppio; colonne larghe quanto il testo più lungo, stimato a
//!   0,75 volte l'altezza per carattere (il font dello stile non si conosce:
//!   meglio larga che col testo fuori). Font, colori e bordi seguono lo stile
//!   tabella corrente del disegno; solo un `n.d.` ha il colore del contenuto
//!   rosso (ACI 1): l'API delle celle lo permette e il DWG lo conserva, anche
//!   nel 2004 (vedi `put`), quindi niente `N.D.` maiuscolo;
//! - una tabella riscritta sul posto ([`rebuilt`]) tiene punto
//!   d'inserimento, direzione, proprietà comuni (handle, layer, colore, XDATA
//!   di altre applicazioni), stile tabella e blocco anonimo; un testo uguale
//!   dà una tabella uguale, così un lancio che non cambia nulla non lascia
//!   passi di annullamento.

use codec::entities::table::{legacy_override_properties, CellRange, CellStyle, Table};
use codec::entities::{Entity, TableBuilder};
use codec::objects::ObjectType;
use codec::tables::{AppId, Layer};
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{Color, Document, EntityType, Handle, Vector3};

use super::abaco::{Cell, TableKind, TableText};
use super::rooms::DEFAULT_ROOM_TEXT_M;
use super::units::DrawingUnit;
use super::{APP_ABACO, LAYER_ABACO};

/// Versione del record XDATA.
const VERSION: i16 = 1;
const KEY_TABLE: &str = "tabella";
/// Colore ACI del layer `AL-ABACO` (bianco/nero, come i layer dei locali).
const ACI_BIANCO: i16 = 7;
/// Colore ACI del testo di un `n.d.`.
const ACI_ND: i16 = 1;
/// Larghezza stimata di un carattere, in altezze del testo.
const CHAR_WIDTH: f64 = 0.75;
/// Margine orizzontale di una cella, per lato, in altezze del testo.
const MARGIN: f64 = 0.5;
/// Altezza di una riga, in altezze del testo.
const ROW_HEIGHT: f64 = 2.0;
/// Larghezza minima di una colonna, in altezze del testo.
const MIN_COLUMN: f64 = 3.0;
/// Distanza tra due tabelle impilate, in righe.
const GAP_ROWS: f64 = 2.0;
/// Bit dell'override del colore del contenuto di una cella, formato 2004–2007.
const LEGACY_CONTENT_COLOR: i32 = 0x08;
/// Bit dell'override dell'altezza del testo di una cella, formato 2004–2007.
const LEGACY_TEXT_HEIGHT: i32 = 0x20;

fn kind_key(kind: TableKind) -> &'static str {
    match kind {
        TableKind::Demolizioni => "demolizioni",
        TableKind::Nuove => "nuove",
        TableKind::Locali => "locali",
    }
}

fn kind_from_key(text: &str) -> Option<TableKind> {
    match text {
        "demolizioni" => Some(TableKind::Demolizioni),
        "nuove" => Some(TableKind::Nuove),
        "locali" => Some(TableKind::Locali),
        _ => None,
    }
}

/// Il nome della tabella in riga di comando.
pub fn kind_name(kind: TableKind) -> &'static str {
    match kind {
        TableKind::Demolizioni => "Demolizioni",
        TableKind::Nuove => "Nuove murature",
        TableKind::Locali => "Locali",
    }
}

/// La sequenza dei valori del record `ARCHLINE_ABACO` di una tabella `kind`.
pub fn encode(kind: TableKind) -> Vec<XDataValue> {
    vec![
        XDataValue::Integer16(VERSION),
        XDataValue::String(KEY_TABLE.to_string()),
        XDataValue::String(kind_key(kind).to_string()),
    ]
}

/// Il tipo di tabella del record, letto per chiave; `None` senza la chiave o
/// con un tipo sconosciuto.
pub fn decode(rec: &ExtendedDataRecord) -> Option<TableKind> {
    rec.values.windows(2).find_map(|pair| match pair {
        [XDataValue::String(key), XDataValue::String(value)] if key == KEY_TABLE => {
            kind_from_key(value)
        }
        _ => None,
    })
}

fn abaco_record(kind: TableKind) -> ExtendedDataRecord {
    let mut rec = ExtendedDataRecord::new(APP_ABACO);
    for value in encode(kind) {
        rec.add_value(value);
    }
    rec
}

/// Le tabelle d'abaco dello spazio modello (non quelle in carta o nei
/// blocchi), nell'ordine del disegno, con il loro tipo.
pub fn tables_in(doc: &Document) -> Vec<(Handle, TableKind)> {
    doc.block_records
        .iter()
        .filter(|record| record.is_model_space())
        .flat_map(|record| record.entity_handles.iter())
        .filter_map(|&handle| {
            let EntityType::Table(table) = doc.get_entity(handle)? else {
                return None;
            };
            Some((
                handle,
                decode(table.common.extended_data.get_record(APP_ABACO)?)?,
            ))
        })
        .collect()
}

/// Manca il layer `AL-ABACO` o l'APPID `ARCHLINE_ABACO`?
pub fn missing_layer(doc: &Document) -> bool {
    !doc.layers.contains(LAYER_ABACO) || !doc.app_ids.contains(APP_ABACO)
}

/// Crea il layer `AL-ABACO` (ACI 7) e l'APPID `ARCHLINE_ABACO` se mancano,
/// ognuno con il suo handle (senza, si perderebbero al salvataggio DWG), come
/// `zone::ensure_layers`. Niente undo: lo apre il chiamante.
pub fn ensure_layer(doc: &mut Document) {
    if !doc.layers.contains(LAYER_ABACO) {
        let mut layer = Layer::new(LAYER_ABACO);
        layer.handle = doc.allocate_handle();
        layer.color = Color::from_index(ACI_BIANCO);
        let _ = doc.layers.add(layer);
    }
    if !doc.app_ids.contains(APP_ABACO) {
        let mut app_id = AppId::new(APP_ABACO);
        app_id.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(app_id);
    }
}

/// L'handle dello stile tabella corrente del disegno (`CTABLESTYLE`,
/// `header.current_table_style_name`), cercato per nome tra gli oggetti
/// stile tabella senza badare alle maiuscole, come i comandi tabella di
/// upstream; `None` se il nome manca o lo stile non c'è.
pub fn current_table_style(doc: &Document) -> Option<Handle> {
    let name = doc.header.current_table_style_name.trim();
    if name.is_empty() {
        return None;
    }
    doc.objects
        .iter()
        .find_map(|(handle, object)| match object {
            ObjectType::TableStyle(style) if style.name.eq_ignore_ascii_case(name) => Some(*handle),
            _ => None,
        })
}

/// L'altezza del testo delle tabelle, in unità del disegno.
fn text_height(unit: DrawingUnit) -> f64 {
    unit.metres(DEFAULT_ROOM_TEXT_M)
}

/// La distanza tra due tabelle impilate, in unità del disegno.
pub fn gap(unit: DrawingUnit) -> f64 {
    text_height(unit) * ROW_HEIGHT * GAP_ROWS
}

/// La tabella `text` come `TABLE` sul layer `AL-ABACO`, con l'angolo in alto a
/// sinistra in `at` e il record `ARCHLINE_ABACO`. Lo stile tabella resta da
/// assegnare: chi crea la tabella nel disegno ci mette
/// [`current_table_style`] (Ruling Q, `AL_ABACO`).
pub fn build(text: &TableText, at: [f64; 2], unit: DrawingUnit) -> EntityType {
    EntityType::Table(Box::new(build_table(text, at, unit)))
}

fn build_table(text: &TableText, at: [f64; 2], unit: DrawingUnit) -> Table {
    let h = text_height(unit);
    let cols = text.columns.len().max(1);
    let body: Vec<&Vec<Cell>> = text.rows.iter().chain(&text.totals).collect();
    let spans: Vec<usize> = body.iter().map(|row| label_span(row, cols)).collect();
    let widths = column_widths(text, &body, &spans, cols, h);
    let total: f64 = widths.iter().sum();
    let per_line = ((total / h - 2.0 * MARGIN) / CHAR_WIDTH).floor().max(1.0) as usize;
    let notes: Vec<String> = text
        .notes
        .iter()
        .flat_map(|note| wrap(note, per_line))
        .collect();

    let rows = 2 + body.len() + notes.len();
    let mut table = TableBuilder::new(rows, cols)
        .at(Vector3::new(at[0], at[1], 0.0))
        .row_height(ROW_HEIGHT * h)
        .layer(LAYER_ABACO)
        .build();
    for (col, width) in widths.iter().enumerate() {
        table.set_column_width(col, *width);
    }
    put(&mut table, 0, 0, &text.title, false, h);
    merge(&mut table, 0, cols);
    for (col, name) in text.columns.iter().enumerate().take(cols) {
        put(&mut table, 1, col, name, false, h);
    }
    for (k, (cells, &span)) in body.iter().zip(&spans).enumerate() {
        let row = 2 + k;
        for (col, cell) in cells.iter().enumerate().take(cols) {
            if !cell.text.is_empty() {
                put(&mut table, row, col, &cell.text, cell.missing, h);
            }
        }
        merge(&mut table, row, span);
    }
    for (k, line) in notes.iter().enumerate() {
        let row = 2 + body.len() + k;
        put(&mut table, row, 0, line, false, h);
        merge(&mut table, row, cols);
    }
    table
        .common
        .extended_data
        .upsert_record(abaco_record(text.kind));
    table
}

/// Quante colonne prende la prima cella di una riga: lei e le celle vuote che
/// la seguono (1 se la prima è vuota o se la seconda ha un testo).
fn label_span(row: &[Cell], cols: usize) -> usize {
    let row = &row[..row.len().min(cols)];
    match row.split_first() {
        Some((first, rest)) if !first.text.is_empty() => {
            1 + rest.iter().take_while(|cell| cell.text.is_empty()).count()
        }
        _ => 1,
    }
}

/// La larghezza che serve a `text`, in unità del disegno.
fn needed(text: &str, h: f64) -> f64 {
    (text.chars().count() as f64 * CHAR_WIDTH + 2.0 * MARGIN) * h
}

/// Le larghezze delle colonne: il testo più lungo delle celle non unite, poi
/// le celle unite (titolo, etichette), il cui di più va all'ultima colonna
/// che prendono.
fn column_widths(
    text: &TableText,
    body: &[&Vec<Cell>],
    spans: &[usize],
    cols: usize,
    h: f64,
) -> Vec<f64> {
    let mut widths = vec![MIN_COLUMN * h; cols];
    for (col, name) in text.columns.iter().enumerate().take(cols) {
        widths[col] = widths[col].max(needed(name, h));
    }
    for (cells, &span) in body.iter().zip(spans) {
        for (col, cell) in cells
            .iter()
            .enumerate()
            .take(cols)
            .skip(usize::from(span > 1))
        {
            widths[col] = widths[col].max(needed(&cell.text, h));
        }
    }
    for (cells, &span) in body.iter().zip(spans) {
        if let (true, Some(first)) = (span > 1, cells.first()) {
            widen(&mut widths, span, needed(&first.text, h));
        }
    }
    widen(&mut widths, cols, needed(&text.title, h));
    widths
}

/// Allarga le prime `span` colonne finché insieme bastano a `need`.
fn widen(widths: &mut [f64], span: usize, need: f64) {
    let span = span.clamp(1, widths.len());
    let sum: f64 = widths[..span].iter().sum();
    if sum < need {
        widths[span - 1] += need - sum;
    }
}

/// Unisce le prime `span` celle della riga `row` (niente con `span` 1).
fn merge(table: &mut Table, row: usize, span: usize) {
    if span > 1 {
        table.merge_cells(CellRange::new(row, 0, row, span - 1));
    }
}

/// Il testo di una cella, alto `h`, rosso se è un dato che manca. Altezza e
/// colore sono override della cella nella forma che il DWG conserva in ogni
/// versione: lo stile della cella con i bit del formato 2004–2007
/// ([`LEGACY_TEXT_HEIGHT`], [`LEGACY_CONTENT_COLOR`]), come li legge il
/// codec, che dal 2010 li scrive come formato del contenuto
/// (`TableCell::binary_layout`). Scritti solo nel contenuto si perderebbero
/// in un DWG 2004 o 2007.
fn put(table: &mut Table, row: usize, col: usize, text: &str, missing: bool, h: f64) {
    table.set_cell_text(row, col, text);
    let Some(cell) = table.cell_mut(row, col) else {
        return;
    };
    let mut flags = LEGACY_TEXT_HEIGHT;
    let mut style = CellStyle::new();
    style.text_height = h;
    if missing {
        flags |= LEGACY_CONTENT_COLOR;
        style.content_color = Color::from_index(ACI_ND);
    }
    style.override_flags = flags;
    style.legacy_override_bits = true;
    style.property_flags = legacy_override_properties(flags);
    cell.style = Some(style);
}

/// `text` a capo sugli spazi, al più `per_line` caratteri per riga; una
/// parola più lunga resta intera su una riga sua.
fn wrap(text: &str, per_line: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > per_line {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Le tabelle una sotto l'altra, la prima con l'angolo in alto a sinistra in
/// `at`, staccate di [`gap`] e allineate a sinistra.
pub fn stack(texts: &[&TableText], at: [f64; 2], unit: DrawingUnit) -> Vec<EntityType> {
    let mut y = at[1];
    texts
        .iter()
        .map(|text| {
            let table = build_table(text, [at[0], y], unit);
            y -= table.total_height() + gap(unit);
            EntityType::Table(Box::new(table))
        })
        .collect()
}

/// La tabella `old` con il testo `text`, riscritta dove si trova (vedi le
/// regole del modulo). Il record `ARCHLINE_ABACO` si riscrive; il blocco
/// grezzo di un DWG lo scarta il chiamante con `set_entity_xdata`.
pub fn rebuilt(old: &Table, text: &TableText, unit: DrawingUnit) -> Table {
    let at = [old.insertion_point.x, old.insertion_point.y];
    let mut table = build_table(text, at, unit);
    table.insertion_point = old.insertion_point;
    table.horizontal_direction = old.horizontal_direction;
    table.normal = old.normal;
    table.table_style_handle = old.table_style_handle;
    table.block_record_handle = old.block_record_handle;
    table.block_name = old.block_name.clone();
    table.common = old.common.clone();
    table
        .common
        .extended_data
        .upsert_record(abaco_record(text.kind));
    table
}

/// Ciò che `AL_ABACO` fa sulle tabelle del disegno ([`plan`]).
#[derive(Debug, Default)]
pub struct AbacoPlan {
    /// Le tabelle che cambiano, riscritte sul posto.
    pub replace: Vec<(Handle, TableKind, Table)>,
    /// Le tabelle già uguali al testo nuovo.
    pub unchanged: usize,
    /// Le tabelle su layer bloccati: non si toccano e si contano.
    pub locked: usize,
    /// Le tabelle il cui tipo non ha più righe (un numero vecchio non resta
    /// nel disegno).
    pub remove: Vec<(Handle, TableKind)>,
    /// I tipi da creare (nessuna tabella di quel tipo nel disegno), in ordine.
    pub missing: Vec<TableKind>,
    /// Dove vanno le mancanti se resta almeno una tabella d'abaco: sotto la
    /// più bassa com'è dopo il comando (riscritta o lasciata com'era),
    /// allineate a sinistra con lei. `None`: serve un punto.
    pub below: Option<[f64; 2]>,
}

/// Il piano di `AL_ABACO` per le tabelle `texts` (`abaco::render_tables`):
/// ogni tabella già presente di un tipo da scrivere si riscrive dove si trova
/// (anche le copie), se cambia e se il suo layer non è bloccato
/// (`is_locked`); quelle di un tipo senza righe si tolgono; i tipi assenti
/// mancano e vanno sotto l'ultima tabella che resta.
pub fn plan(
    doc: &Document,
    texts: &[TableText],
    unit: DrawingUnit,
    is_locked: impl Fn(Handle) -> bool,
) -> AbacoPlan {
    let mut plan = AbacoPlan::default();
    let mut lowest: Option<(f64, f64)> = None;
    for (handle, kind) in tables_in(doc) {
        let Some(EntityType::Table(old)) = doc.get_entity(handle) else {
            continue;
        };
        // Il bordo sinistro e quello inferiore della tabella com'è dopo il
        // comando (Ruling R): una riscritta può crescere, e la mancante va
        // sotto le sue ultime righe nuove, non sotto quelle di prima.
        let place = |table: &Table| (table.insertion_point.x, table.bounding_box().min.y);
        let stays = if is_locked(handle) {
            plan.locked += 1;
            place(old)
        } else if let Some(text) = texts.iter().find(|text| text.kind == kind) {
            let new = rebuilt(old, text, unit);
            let stays = place(&new);
            if new == **old {
                plan.unchanged += 1;
            } else {
                plan.replace.push((handle, kind, new));
            }
            stays
        } else {
            plan.remove.push((handle, kind));
            continue;
        };
        if lowest.map_or(true, |(_, y)| stays.1 < y) {
            lowest = Some(stays);
        }
    }
    let present: Vec<TableKind> = tables_in(doc).into_iter().map(|(_, kind)| kind).collect();
    plan.missing = texts
        .iter()
        .map(|text| text.kind)
        .filter(|kind| !present.contains(kind))
        .collect();
    plan.below = lowest.map(|(x, bottom)| [x, bottom - gap(unit)]);
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    use codec::entities::table::CellStylePropertyFlags;
    use codec::entities::{Line, TableBuilder};
    use codec::tables::AppId;
    use codec::xdata::{ExtendedDataRecord, XDataValue};
    use codec::{Color, Document, DxfVersion, EntityType, Vector3};

    use crate::bim::abaco::{Cell, TableKind, TableText};
    use crate::bim::units::DrawingUnit;
    use crate::bim::{APP_ABACO, LAYER_ABACO};

    const LONG_NOTE: &str = "Valori arrotondati a due decimali; i totali sono calcolati sui \
        valori non arrotondati e possono non coincidere con la somma delle righe.";

    fn cell(text: &str) -> Cell {
        Cell {
            text: text.to_string(),
            missing: false,
        }
    }

    fn nd() -> Cell {
        Cell {
            text: "n.d.".to_string(),
            missing: true,
        }
    }

    /// Una tabella piccola con tutto ciò che `build` deve saper scrivere: un
    /// `n.d.`, l'intestazione dei varchi (celle vuote dopo l'etichetta), un
    /// totale con due celle vuote dopo l'etichetta, una nota lunga.
    fn sample(kind: TableKind) -> TableText {
        TableText {
            kind,
            title: "Demolizioni (m³)".into(),
            columns: vec![
                "Codice".into(),
                "Lunghezza (m)".into(),
                "Volume (m³)".into(),
                "Note".into(),
            ],
            rows: vec![
                vec![cell("D1"), cell("3,00"), cell("0,81"), cell("")],
                vec![
                    cell("D2"),
                    cell("3,00"),
                    nd(),
                    cell("da completare: altezza n.d."),
                ],
                vec![
                    cell("Varchi (fuori dal totale delle demolizioni)"),
                    cell(""),
                    cell(""),
                    cell(""),
                ],
                vec![cell("V1"), cell("0,90"), cell("0,19"), cell("")],
            ],
            totals: vec![vec![
                cell("Totale demolizioni (righe complete)"),
                cell(""),
                cell("0,81"),
                cell("1 riga esclusa"),
            ]],
            notes: vec![LONG_NOTE.to_string()],
        }
    }

    fn table_of(entity: &EntityType) -> &Table {
        match entity {
            EntityType::Table(table) => table,
            other => panic!("non è una TABLE: {other:?}"),
        }
    }

    fn span(table: &Table, row: usize, col: usize) -> i32 {
        table.cell(row, col).map_or(0, |cell| cell.merge_width)
    }

    /// L'override della cella per `property`: quello del contenuto (DWG dal
    /// 2010) o, se no, quello dello stile della cella (in memoria e DWG 2004–
    /// 2007), come lo legge il disegno delle tabelle.
    fn cell_override<T>(
        table: &Table,
        row: usize,
        col: usize,
        property: CellStylePropertyFlags,
        from_content: impl Fn(&codec::entities::table::CellContent) -> T,
        from_style: impl Fn(&CellStyle) -> T,
    ) -> Option<T> {
        let cell = table.cell(row, col)?;
        if let Some(content) = cell.contents.first().filter(|c| c.sets(property)) {
            return Some(from_content(content));
        }
        cell.style
            .as_ref()
            .filter(|style| style.sets(property))
            .map(from_style)
    }

    fn content_height(table: &Table, row: usize, col: usize) -> f64 {
        cell_override(
            table,
            row,
            col,
            CellStylePropertyFlags::TEXT_HEIGHT,
            |content| content.text_height,
            |style| style.text_height,
        )
        .unwrap_or(0.0)
    }

    fn content_color(table: &Table, row: usize, col: usize) -> Option<Color> {
        cell_override(
            table,
            row,
            col,
            CellStylePropertyFlags::CONTENT_COLOR,
            |content| content.color,
            |style| style.content_color,
        )
    }

    /// Titolo unito su tutte le colonne, intestazioni, righe, totali e note;
    /// l'etichetta dei varchi unita a tutta la riga, quella del totale alle
    /// due celle vuote che la seguono; la nota lunga va a capo in più righe
    /// unite; `n.d.` in rosso; testo alto 0,25 m; layer `AL-ABACO` e record
    /// `ARCHLINE_ABACO`.
    #[test]
    fn built_table_has_title_header_merged_labels_and_notes() {
        let text = sample(TableKind::Demolizioni);
        let entity = build(&text, [20.0, 5.0], DrawingUnit::Metri);
        let table = table_of(&entity);
        assert_eq!(table.common.layer, LAYER_ABACO);
        assert_eq!(table.insertion_point, Vector3::new(20.0, 5.0, 0.0));
        assert_eq!(table.column_count(), 4);
        let notes_from = 2 + text.rows.len() + text.totals.len();
        assert!(
            table.row_count() > notes_from + 1,
            "la nota lunga va a capo: {} righe",
            table.row_count()
        );

        assert_eq!(table.cell_text(0, 0), Some("Demolizioni (m³)"));
        assert_eq!(span(table, 0, 0), 4, "titolo");
        for (col, name) in text.columns.iter().enumerate() {
            assert_eq!(table.cell_text(1, col), Some(name.as_str()));
            assert_eq!(span(table, 1, col), 1);
        }
        assert_eq!(table.cell_text(2, 0), Some("D1"));
        assert_eq!(table.cell_text(2, 2), Some("0,81"));
        assert_eq!(span(table, 2, 0), 1);
        assert_eq!(table.cell_text(3, 2), Some("n.d."));
        assert_eq!(content_color(table, 3, 2), Some(Color::from_index(1)));
        assert_ne!(content_color(table, 3, 1), Some(Color::from_index(1)));
        assert_eq!(span(table, 4, 0), 4, "intestazione dei varchi");
        assert_eq!(span(table, 5, 0), 1);
        assert_eq!(
            table.cell_text(6, 0),
            Some("Totale demolizioni (righe complete)")
        );
        assert_eq!(span(table, 6, 0), 2, "etichetta del totale");
        assert_eq!(table.cell_text(6, 2), Some("0,81"));

        let lines: Vec<&str> = (notes_from..table.row_count())
            .map(|row| {
                assert_eq!(span(table, row, 0), 4, "riga di nota {row}");
                table.cell_text(row, 0).unwrap_or("")
            })
            .collect();
        assert_eq!(
            lines.join(" "),
            LONG_NOTE.split_whitespace().collect::<Vec<_>>().join(" ")
        );

        for row in 0..table.row_count() {
            for col in 0..table.column_count() {
                if table.cell_text(row, col).is_some_and(|t| !t.is_empty()) {
                    assert!(
                        (content_height(table, row, col) - 0.25).abs() < 1e-12,
                        "altezza del testo ({row}, {col})"
                    );
                }
            }
        }
        assert!(table.columns.iter().all(|column| column.width > 0.0));
        let rec = table.common.extended_data.get_record(APP_ABACO).unwrap();
        assert_eq!(decode(rec), Some(TableKind::Demolizioni));
        assert_eq!(rec.values, encode(TableKind::Demolizioni));
    }

    /// Dimensioni in unità del disegno: in millimetri tutto è mille volte più
    /// grande che in metri.
    #[test]
    fn built_table_scales_with_the_unit() {
        let text = sample(TableKind::Locali);
        let metres = build(&text, [0.0, 0.0], DrawingUnit::Metri);
        let millimetres = build(&text, [0.0, 0.0], DrawingUnit::Millimetri);
        let (m, mm) = (table_of(&metres), table_of(&millimetres));
        assert_eq!(m.row_count(), mm.row_count());
        assert!((mm.total_width() - 1000.0 * m.total_width()).abs() < 1e-6);
        assert!((mm.total_height() - 1000.0 * m.total_height()).abs() < 1e-6);
        assert!((content_height(mm, 2, 0) - 250.0).abs() < 1e-9);
        assert!((gap(DrawingUnit::Centimetri) - 100.0 * gap(DrawingUnit::Metri)).abs() < 1e-9);
    }

    /// Solo le `TABLE` dello spazio modello con un record `ARCHLINE_ABACO`
    /// leggibile: non una tabella qualunque, non una in un layout, non un tipo
    /// sconosciuto, non un'altra entità con il nostro XDATA.
    #[test]
    fn tables_in_reads_only_our_model_space_tables() {
        let mut doc = Document::new();
        ensure_layer(&mut doc);
        assert!(!missing_layer(&doc));
        let ours = doc
            .add_entity(build(
                &sample(TableKind::Locali),
                [0.0, 0.0],
                DrawingUnit::Metri,
            ))
            .unwrap();
        doc.add_entity(EntityType::Table(Box::new(TableBuilder::new(2, 2).build())))
            .unwrap();
        doc.add_entity_to_layout(
            build(&sample(TableKind::Nuove), [0.0, 0.0], DrawingUnit::Metri),
            "Layout1",
        )
        .unwrap();
        let mut unknown = table_of(&build(
            &sample(TableKind::Nuove),
            [0.0, 0.0],
            DrawingUnit::Metri,
        ))
        .clone();
        let mut rec = ExtendedDataRecord::new(APP_ABACO);
        for value in [
            XDataValue::Integer16(1),
            XDataValue::String("tabella".into()),
            XDataValue::String("porte".into()),
        ] {
            rec.add_value(value);
        }
        unknown.common.extended_data.upsert_record(rec);
        doc.add_entity(EntityType::Table(Box::new(unknown)))
            .unwrap();
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        let mut rec = ExtendedDataRecord::new(APP_ABACO);
        for value in encode(TableKind::Demolizioni) {
            rec.add_value(value);
        }
        line.common.extended_data.upsert_record(rec);
        doc.add_entity(EntityType::Line(line)).unwrap();

        assert_eq!(tables_in(&doc), [(ours, TableKind::Locali)]);
    }

    /// Una tabella costruita da `build` torna da un DWG con il suo tipo, le
    /// celle unite, l'altezza del testo e il rosso del `n.d.`.
    #[test]
    fn built_table_survives_dwg_roundtrip() {
        let tmp = TempDir::new("built");
        for version in [DxfVersion::AC1018, DxfVersion::AC1032] {
            let mut doc = Document::new();
            doc.version = version;
            ensure_layer(&mut doc);
            let handle = doc
                .add_entity(build(
                    &sample(TableKind::Demolizioni),
                    [20.0, 0.0],
                    DrawingUnit::Metri,
                ))
                .unwrap();
            let path = tmp.0.join(format!("{version:?}.dwg"));
            crate::io::save_as_version(&doc, &path, version)
                .unwrap_or_else(|e| panic!("{version:?}: {e}"));
            let loaded = crate::io::load_file(&path).unwrap_or_else(|e| panic!("{version:?}: {e}"));
            assert_eq!(
                tables_in(&loaded),
                [(handle, TableKind::Demolizioni)],
                "{version:?}"
            );
            assert!(!missing_layer(&loaded), "{version:?}: layer e APPID");
            let table = table_of(loaded.get_entity(handle).unwrap());
            assert_eq!(table.common.layer, LAYER_ABACO, "{version:?}");
            assert_eq!(table.cell_text(3, 2), Some("n.d."), "{version:?}");
            assert_eq!(
                content_color(table, 3, 2),
                Some(Color::from_index(1)),
                "{version:?}: n.d. evidenziato"
            );
            assert!(
                (content_height(table, 2, 0) - 0.25).abs() < 1e-9,
                "{version:?}: altezza del testo"
            );
            assert_eq!(span(table, 4, 0), 4, "{version:?}");
            assert_eq!(span(table, 6, 0), 2, "{version:?}");
        }
    }

    /// Le tabelle impilate dall'alto: la seconda sotto la prima, staccata di
    /// `gap`, allineata a sinistra.
    #[test]
    fn stacked_tables_go_one_below_the_other() {
        let first = sample(TableKind::Demolizioni);
        let second = sample(TableKind::Locali);
        let unit = DrawingUnit::Metri;
        let stack = stack(&[&first, &second], [10.0, 4.0], unit);
        let [a, b] = &stack[..] else {
            panic!("{stack:?}")
        };
        let (a, b) = (table_of(a), table_of(b));
        assert_eq!(a.insertion_point, Vector3::new(10.0, 4.0, 0.0));
        assert!((b.insertion_point.x - 10.0).abs() < 1e-12);
        let want = 4.0 - a.total_height() - gap(unit);
        assert!((b.insertion_point.y - want).abs() < 1e-9, "{b:?}");
    }

    /// Riscrivere sul posto: stesso punto, handle, layer, XDATA di altre
    /// applicazioni e blocco anonimo; il testo nuovo.
    #[test]
    fn rebuilt_table_keeps_its_place_and_properties() {
        let unit = DrawingUnit::Metri;
        let mut old = table_of(&build(&sample(TableKind::Locali), [0.0, 0.0], unit)).clone();
        old.common.handle = Handle::new(0x2A);
        old.common.layer = "TABELLE".into();
        old.insertion_point = Vector3::new(30.0, -7.0, 0.0);
        old.block_name = "*T3".into();
        let mut other = ExtendedDataRecord::new("ALTRA_APP");
        other.add_value(XDataValue::String("x".into()));
        old.common.extended_data.upsert_record(other);

        let mut text = sample(TableKind::Locali);
        text.rows[0][2] = cell("0,90");
        let new = rebuilt(&old, &text, unit);
        assert_eq!(new.common.handle, old.common.handle);
        assert_eq!(new.common.layer, "TABELLE");
        assert_eq!(new.insertion_point, old.insertion_point);
        assert_eq!(new.block_name, "*T3");
        assert!(new.common.extended_data.get_record("ALTRA_APP").is_some());
        assert_eq!(
            decode(new.common.extended_data.get_record(APP_ABACO).unwrap()),
            Some(TableKind::Locali)
        );
        assert_eq!(new.cell_text(2, 2), Some("0,90"));
        assert_ne!(new, old);
        assert_eq!(
            rebuilt(&old, &sample(TableKind::Locali), unit),
            old,
            "stesso testo"
        );
    }

    /// Il piano: una tabella che cambia si riscrive, una uguale no, una su un
    /// layer bloccato si conta, una di un tipo senza righe si toglie; il tipo
    /// assente manca e va sotto la tabella più bassa tra quelle che restano.
    #[test]
    fn plan_replaces_counts_removes_and_stacks_below_the_lowest() {
        let unit = DrawingUnit::Metri;
        let mut doc = Document::new();
        ensure_layer(&mut doc);
        let demolizioni = sample(TableKind::Demolizioni);
        let locali = sample(TableKind::Locali);
        let changed = doc
            .add_entity(build(&demolizioni, [0.0, 0.0], unit))
            .unwrap();
        let same = doc.add_entity(build(&locali, [0.0, -20.0], unit)).unwrap();
        let locked = doc.add_entity(build(&locali, [50.0, -60.0], unit)).unwrap();
        // Una tabella Nuove, ma i testi nuovi non hanno righe di zone nuove.
        let stale = doc
            .add_entity(build(&sample(TableKind::Nuove), [0.0, -100.0], unit))
            .unwrap();
        let mut demolizioni_now = demolizioni.clone();
        demolizioni_now.rows[0][2] = cell("0,90");
        let texts = [demolizioni_now, locali];
        let plan = plan(&doc, &texts, unit, |handle| handle == locked);

        let replaced: Vec<(Handle, TableKind)> = plan
            .replace
            .iter()
            .map(|(handle, kind, _)| (*handle, *kind))
            .collect();
        assert_eq!(replaced, [(changed, TableKind::Demolizioni)]);
        assert_eq!(plan.replace[0].2.cell_text(2, 2), Some("0,90"));
        assert_eq!(plan.unchanged, 1, "{same:?}");
        assert_eq!(plan.locked, 1);
        assert_eq!(plan.remove, [(stale, TableKind::Nuove)]);
        assert!(plan.missing.is_empty());
        let EntityType::Table(lowest) = doc.get_entity(locked).unwrap() else {
            panic!()
        };
        let want = [50.0, -60.0 - lowest.total_height() - gap(unit)];
        let below = plan.below.expect("c'è una tabella");
        assert!(
            (below[0] - want[0]).abs() < 1e-9 && (below[1] - want[1]).abs() < 1e-9,
            "{below:?} invece di {want:?}"
        );

        // Senza tabelle nel disegno mancano tutte e serve un punto.
        let empty = Document::new();
        let all = [sample(TableKind::Demolizioni), sample(TableKind::Nuove)];
        let plan = super::plan(&empty, &all, unit, |_| false);
        assert_eq!(plan.missing, [TableKind::Demolizioni, TableKind::Nuove]);
        assert_eq!(plan.below, None);
    }

    /// Ruling Q: lo stile tabella corrente si trova per nome (`CTABLESTYLE`),
    /// senza badare alle maiuscole; senza nome o senza l'oggetto stile,
    /// nessuno.
    #[test]
    fn current_table_style_is_found_by_name() {
        let mut doc = Document::new();
        assert_eq!(doc.header.current_table_style_name, "Standard");
        let standard = doc
            .objects
            .iter()
            .find_map(|(handle, object)| {
                matches!(object, ObjectType::TableStyle(s) if s.name == "Standard")
                    .then_some(*handle)
            })
            .expect("stile Standard di un disegno nuovo");
        assert_eq!(current_table_style(&doc), Some(standard));

        let mine = doc.allocate_handle();
        let mut style = codec::objects::TableStyle::standard();
        style.name = "Abaco".into();
        style.handle = mine;
        doc.objects.insert(mine, ObjectType::TableStyle(style));
        doc.header.current_table_style_name = "ABACO".into();
        assert_eq!(current_table_style(&doc), Some(mine));

        for missing in ["Nessuno", "", "  "] {
            doc.header.current_table_style_name = missing.into();
            assert_eq!(current_table_style(&doc), None, "{missing:?}");
        }
        doc.header.current_table_style_name = "Standard".into();
        doc.objects.remove(&standard);
        assert_eq!(current_table_style(&doc), None, "oggetto stile assente");
    }

    /// Ruling R: una tabella riscritta che cresce di parecchie righe non fa
    /// finire la tabella mancante sopra le sue ultime righe: «sotto l'ultima»
    /// si misura sulla tabella nuova. Una tabella invariata dà il punto di
    /// prima, sotto la tabella com'è.
    #[test]
    fn missing_table_goes_below_the_grown_replacement() {
        let unit = DrawingUnit::Metri;
        let mut doc = Document::new();
        ensure_layer(&mut doc);
        let old_text = sample(TableKind::Demolizioni);
        let handle = doc.add_entity(build(&old_text, [0.0, 0.0], unit)).unwrap();
        let EntityType::Table(old) = doc.get_entity(handle).unwrap().clone() else {
            panic!()
        };
        let mut grown = old_text.clone();
        for k in 0..8 {
            grown.rows.push(vec![
                cell(&format!("D{}", k + 3)),
                cell("3,00"),
                cell("0,81"),
                cell(""),
            ]);
        }
        let missing = sample(TableKind::Locali);

        let plan = plan(&doc, &[grown, missing.clone()], unit, |_| false);
        let [(replaced, _, new)] = &plan.replace[..] else {
            panic!("{:?}", plan.replace)
        };
        assert_eq!(*replaced, handle);
        assert!(new.row_count() >= old.row_count() + 8);
        assert_eq!(plan.missing, [TableKind::Locali]);
        let below = plan.below.expect("c'è una tabella");
        let created = stack(&[&missing], below, unit);
        let (created_box, new_box) = (table_of(&created[0]).bounding_box(), new.bounding_box());
        assert!(
            created_box.max.y < new_box.min.y,
            "la tabella creata ({created_box:?}) si sovrappone alla riscritta ({new_box:?})"
        );
        assert!((below[1] - (new_box.min.y - gap(unit))).abs() < 1e-9);

        let plan = super::plan(&doc, &[old_text, missing], unit, |_| false);
        assert_eq!(plan.unchanged, 1);
        let below = plan.below.expect("c'è una tabella");
        assert!(below[0].abs() < 1e-12);
        assert!((below[1] - (old.bounding_box().min.y - gap(unit))).abs() < 1e-9);
    }

    #[test]
    fn notes_wrap_on_spaces() {
        assert_eq!(
            wrap("uno due tre quattro", 8),
            ["uno due", "tre", "quattro"]
        );
        assert_eq!(wrap("lunghissimaparola x", 5), ["lunghissimaparola", "x"]);
        assert_eq!(wrap("", 5), Vec::<String>::new());
    }

    /// Una cartella nuova sotto la cartella temporanea, tolta a fine test.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir().join(format!(
                "archline_bim_abaco_{tag}_{}_{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Spike di fattibilità (spec §5.2, passo 1 del Task 7): una `TABLE` con
    /// celle (anche unite) e l'XDATA `ARCHLINE_ABACO`, salvata in DWG e riletta
    /// come fanno i test di `extract.rs` (`io::save_as_version`, poi
    /// `io::load_file`): il testo delle celle e l'XDATA tornano, nelle versioni
    /// DWG che hanno le tabelle (2004 in poi).
    #[test]
    fn table_survives_dwg_roundtrip() {
        let tmp = TempDir::new("roundtrip");
        for version in [
            DxfVersion::AC1018,
            DxfVersion::AC1021,
            DxfVersion::AC1024,
            DxfVersion::AC1027,
            DxfVersion::AC1032,
        ] {
            let mut doc = Document::new();
            doc.version = version;
            let mut app = AppId::new(APP_ABACO);
            app.handle = doc.allocate_handle();
            doc.app_ids.add(app).unwrap();
            let mut table = TableBuilder::new(3, 3)
                .at(Vector3::new(20.0, 0.0, 0.0))
                .build();
            table.set_cell_text(0, 0, "Demolizioni (m³)");
            table.set_cell_text(1, 0, "D1");
            table.set_cell_text(1, 2, "0,81");
            table.set_cell_text(2, 0, "Varchi (fuori dal totale delle demolizioni)");
            table.merge_cells(codec::entities::table::CellRange::new(2, 0, 2, 2));
            let mut rec = ExtendedDataRecord::new(APP_ABACO);
            for value in [
                XDataValue::Integer16(1),
                XDataValue::String("tabella".into()),
                XDataValue::String("demolizioni".into()),
            ] {
                rec.add_value(value);
            }
            table.common.extended_data.upsert_record(rec);
            let handle = doc.add_entity(EntityType::Table(Box::new(table))).unwrap();

            let path = tmp.0.join(format!("{version:?}.dwg"));
            crate::io::save_as_version(&doc, &path, version)
                .unwrap_or_else(|e| panic!("{version:?}: {e}"));
            let loaded = crate::io::load_file(&path).unwrap_or_else(|e| panic!("{version:?}: {e}"));

            let tables: Vec<_> = loaded
                .entities()
                .filter_map(|entity| match entity {
                    EntityType::Table(table) => Some(table.as_ref()),
                    _ => None,
                })
                .collect();
            let [table] = tables[..] else {
                panic!("{version:?}: {} tabelle", tables.len())
            };
            assert_eq!(table.common.handle, handle, "{version:?}");
            assert_eq!(
                table.cell_text(0, 0),
                Some("Demolizioni (m³)"),
                "{version:?}"
            );
            assert_eq!(table.cell_text(1, 0), Some("D1"), "{version:?}");
            assert_eq!(table.cell_text(1, 2), Some("0,81"), "{version:?}");
            assert_eq!(
                table.cell_text(2, 0),
                Some("Varchi (fuori dal totale delle demolizioni)"),
                "{version:?}"
            );
            assert!(
                table.cell(2, 0).is_some_and(|cell| cell.merge_width == 3),
                "{version:?}: cella unita"
            );
            assert!(
                (table.insertion_point.x - 20.0).abs() < 1e-9
                    && table.insertion_point.y.abs() < 1e-9,
                "{version:?}: {:?}",
                table.insertion_point
            );
            let rec = table
                .common
                .extended_data
                .get_record(APP_ABACO)
                .unwrap_or_else(|| panic!("{version:?}: XDATA perso"));
            assert_eq!(
                rec.values,
                [
                    XDataValue::Integer16(1),
                    XDataValue::String("tabella".into()),
                    XDataValue::String("demolizioni".into()),
                ],
                "{version:?}"
            );
        }
    }
}
