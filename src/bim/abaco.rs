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
//!   altezza), così un dato che manca non sembra mai uno zero; la parete e le
//!   grandezze dei locali a cui nessuna riga completa dà un valore hanno il
//!   totale `n.d.` (revisione finale). Per ogni riga
//!   incompleta un [`Notice::RigaIncompleta`] con il motivo, in testa agli
//!   avvisi (Ruling C);
//! - degli avvisi della geometria di progetto restano solo quelli che
//!   spiegano i locali: quelli del solo disegno (sovrapposizioni ai lati,
//!   giunzioni, entità escluse o lasciate intere) li dice `AL_ESTRAI`
//!   ([`drawing_only`], revisione finale);
//! - locali (spec §4.3): un locale di progetto con un solo locale di fatto
//!   nella sua regione, che non sta in nessun altro locale di progetto, fa una
//!   riga accoppiata con la variazione (progetto meno fatto); gli altri
//!   restano righe separate con la nota («da A + B», «unito in C»), senza
//!   somme. Nelle righe accoppiate codice, nome, perimetro, volume e altezza
//!   sono «fatto / progetto» quando differiscono;
//! - la resa nelle tabelle: vedi le regole di [`super::abaco_render`];
//! - il CSV ([`to_csv`], spec §5.3): solo le righe complete, una per misura,
//!   nella forma della `Misura` di ArchCme (`parti × lunghezza × larghezza ×
//!   altezza`, un fattore vuoto non si applica ed è saltato nel prodotto), con
//!   le note delle tabelle. Un locale senza altezza è completo (la superficie
//!   è un fattore solo) ed entra con lo stato dell'altezza `n.d.` (Ruling O);
//!   per demolizioni, varchi e nuove murature l'altezza è un fattore, quindi
//!   senza altezza la riga è incompleta e non entra: un'altezza che manca non
//!   diventa mai un campo vuoto.

use codec::Document;

use super::abaco_render::{height_state, room_notes, zone_notes, ND};
use super::aperture_quantities::{aperture_quantities, ApertureQuantities};
use super::extract::push_unique;
use super::quantities::{zone_rows, ZoneKind, ZoneRow};
use super::quantities_rooms::{fatto_room_rows, project_room_rows, ProjectRoomRow, RoomRow};
use super::record;
use super::report::Notice;
use super::rooms::Phase;
use super::units::DrawingUnit;
use super::zone::zones_in;

pub use super::abaco_render::{render_tables, Cell, TableKind, TableText};

/// Tra i nomi dei locali uniti, come nei nomi composti dei locali di progetto.
const JOIN: &str = " + ";
/// Decimali al massimo in [`format_fixed`]: con di più le unità dell'ultima
/// cifra di un numero di qualche miliardo uscirebbero da un `i64`.
const MAX_DECIMALS: usize = 9;

/// L'intestazione del CSV (spec §5.3): le colonne della `Misura` del libretto
/// di ArchCme (`parti`, `lunghezza`, `larghezza`, `altezza`, `detrai`) con
/// tipo, codice, descrizione, quantità, unità, stato dell'altezza, fonte e
/// note.
pub const CSV_HEADER: &str = "tipo;codice;descrizione;parti;lunghezza;larghezza;altezza;detrai;\
    quantita;um;stato_altezza;fonte;note";
/// BOM UTF-8: Excel italiano riconosce la codifica e legge «m³».
const CSV_BOM: char = '\u{FEFF}';
const CSV_SEPARATOR: char = ';';
const CSV_EOL: &str = "\r\n";
/// Decimali nel CSV (spec §3).
const CSV_DECIMALS: usize = 4;
/// Tra le note di un campo, come nelle tabelle.
const CSV_NOTE_SEP: &str = "; ";
/// La colonna `detrai` (spec parte 3 §5.3): le detrazioni dei vani sono le
/// sole righe «a detrarre».
const CSV_NO_DEDUCTION: &str = "no";
const CSV_DEDUCTION: &str = "sì";
pub(super) const UNIT_M3: &str = "m³";
pub(super) const UNIT_M2: &str = "m²";
const DEMOLITION_WITHOUT_AXIS: &str = "Demolizione area in pianta (zona senza asse)";

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
    /// Le aperture con le detrazioni e le pareti dei locali (parte 3).
    pub aperture: ApertureQuantities,
    /// Gli strati delle stratigrafie assegnate (parte 6).
    pub strati: super::strata_quantities::StrataQuantities,
    /// Un [`Notice::RigaIncompleta`] per riga incompleta (zone, poi locali di
    /// fatto, poi di progetto), poi gli avvisi delle zone, dei locali di fatto
    /// e di quelli di progetto (senza quelli del solo disegno,
    /// [`drawing_only`]), senza doppioni.
    pub notices: Vec<Notice>,
}

/// L'abaco del disegno (spec §4.3): le righe delle zone
/// (`quantities::zone_rows`) divise per tipo, quelle dei locali di fatto e
/// di progetto (`quantities_rooms`) accoppiate, con l'altezza predefinita
/// del record di progetto. Non scrive nulla nel disegno.
pub fn compute_abaco(doc: &Document, opts: &AbacoOptions) -> Abaco {
    let default_height = record::read(doc).default_height;
    let zone_geoms = zones_in(doc);
    let (zones, zone_notices) = zone_rows(&zone_geoms, default_height, opts.unit, opts.tol);
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
        .filter_map(|row| Some((row.code.clone(), row.incomplete_reason()?)));
    let incomplete = zone_reasons
        .chain(room_reasons)
        .map(|(code, reason)| Notice::RigaIncompleta { code, reason })
        .collect();
    let project_notices = project_notices
        .into_iter()
        .filter(|notice| !drawing_only(notice))
        .collect();
    // `FuoriPiano` lo trovano sia i locali di fatto sia quelli di progetto.
    let mut notices = Vec::new();
    for more in [incomplete, zone_notices, fatto_notices, project_notices] {
        push_unique(&mut notices, more);
    }

    // Parte 3: aperture, detrazioni e pareti dei locali, sulle stesse righe.
    let aperture = aperture_quantities(
        doc,
        opts.unit,
        opts.tol,
        &zone_geoms,
        &zones,
        &fatto,
        &progetto,
    );
    push_unique(&mut notices, aperture.notices.clone());

    // Parte 6: gli strati, sulle righe delle nuove murature e dei locali di
    // progetto.
    let nuove_rows: Vec<ZoneRow> = zones
        .iter()
        .filter(|row| row.kind == ZoneKind::Nuova)
        .cloned()
        .collect();
    let strati =
        super::strata_quantities::strata_quantities(doc, &nuove_rows, &progetto, opts.unit);
    push_unique(&mut notices, strati.notices.clone());

    let mut abaco = Abaco {
        demolizioni: Vec::new(),
        varchi: Vec::new(),
        nuove: Vec::new(),
        locali: pair_rooms(fatto, progetto),
        aperture,
        strati,
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

/// Un avviso della geometria di progetto che riguarda solo il DWG dello stato
/// di progetto, non i numeri dell'abaco (revisione finale): lo elenca
/// `AL_ESTRAI`, l'abaco non lo ripete a ogni lancio.
/// - `NuoveSovrapposte` vede ogni angolo asse su asse di `AL_NUOVO`, dove la
///   spec (§4.3, §7 caso K) non vuole avvisi; la sovrapposizione vera la dice
///   già `ZoneSovrapposteSostanzialmente` delle quantità, una volta per coppia;
/// - `GiunzioneDaVerificare` è una linea da pulire a mano nel disegno;
/// - `Esclusa` e `NonTagliata` sono entità non muro lasciate fuori o intere
///   nell'estratto (anche una tabella d'abaco posata in una zona demolita).
///
/// Restano gli avvisi che cambiano i locali di progetto (`TaglioNonChiuso`,
/// `ConfiniNonRimossi`, `FacciaNonTolta`, `VarcoNonSupportato`) e tutti gli
/// altri.
fn drawing_only(notice: &Notice) -> bool {
    matches!(
        notice,
        Notice::NuoveSovrapposte { .. }
            | Notice::GiunzioneDaVerificare { .. }
            | Notice::Esclusa { .. }
            | Notice::NonTagliata { .. }
    )
}

/// Le righe dei locali (spec §4.3). Un locale di fatto e uno di progetto si
/// accoppiano se il progetto ha come solo membro quel locale di fatto e
/// nessun altro locale di progetto ne contiene il seme né sta nel suo
/// contorno: una riga con la variazione. Gli altri restano righe separate: un
/// locale di fatto con «unito in C» (o «diviso tra C + D» se il suo seme sta
/// in più locali di progetto, o se una parte del suo contorno è un altro
/// locale di progetto: R93), un locale di progetto con «da A + B» (anche «da
/// A» per la parte di A senza il suo seme); nessuna somma.
fn pair_rooms(fatto: Vec<RoomRow>, progetto: Vec<ProjectRoomRow>) -> Vec<LocaleLine> {
    // I locali di progetto che contengono il seme di ciascun locale di fatto
    // o che stanno nel suo contorno (la parte di un locale diviso senza il
    // suo seme, R93): con più di un locale di progetto il locale di fatto è
    // diviso e non si accoppia.
    let mut owners: Vec<Vec<usize>> = vec![Vec::new(); fatto.len()];
    for (p, project) in progetto.iter().enumerate() {
        for &f in project.fatto_members.iter().chain(&project.inside_fatto) {
            if let Some(list) = owners.get_mut(f) {
                list.push(p);
            }
        }
    }
    let partners: Vec<Option<usize>> = owners
        .iter()
        .enumerate()
        .map(|(f, list)| match list[..] {
            [p] if progetto[p].fatto_members == [f] && progetto[p].inside_fatto.is_empty() => {
                Some(p)
            }
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
                .chain(&project.inside_fatto)
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
        .chain(&project.inside_fatto)
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
/// sull'ultima cifra, mezzo via dallo zero, e uno zero arrotondato non ha
/// segno. Un valore non finito non ha testo sensato: chi chiama mette `n.d.`.
/// È l'unico arrotondamento dei numeri scritti: `units::format_decimal` (le
/// etichette dei locali) la usa con due decimali, così etichette e celle
/// dell'abaco danno lo stesso testo (R61).
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

/// L'abaco come CSV per Excel italiano (spec §5.3): BOM UTF-8, [`CSV_HEADER`],
/// poi una riga per misura delle sole righe complete, ognuna con il codice
/// delle tabelle: demolizioni, varchi, nuove murature, locali di fatto,
/// locali di progetto. `;` tra i campi, quattro decimali con la virgola, CRLF
/// in fondo a ogni riga; un campo con `;`, virgolette o un a capo va tra
/// virgolette, con le virgolette raddoppiate (RFC 4180).
///
/// Le misure, sempre `parti` = 1 e mai a detrarre:
/// - demolizione con asse: lunghezza × spessore effettivo × altezza (m³),
///   «Demolizione muro spessore 0,100»;
/// - demolizione senza asse: area in pianta × altezza (m³), con l'area nella
///   colonna `lunghezza` e la larghezza vuota, detto nella descrizione;
/// - varco: lunghezza × spessore effettivo × altezza (m³), «Varco»;
/// - nuova muratura: lunghezza × altezza (m², larghezza vuota), «Nuova
///   muratura spessore 0,080»;
/// - locale: la superficie netta come `lunghezza`, un fattore solo (m²), con
///   il nome come descrizione.
///
/// `quantita` è il prodotto dei fattori come sono scritti nella riga (già a
/// quattro decimali, `parti` = 1,0000, i vuoti saltati come nella `Misura` di
/// ArchCme), arrotondato a quattro decimali (Ruling S): chi rifà il prodotto
/// dalle colonne trova `quantita` a meno di 5·10⁻⁵, sempre. Dalla quantità
/// delle tabelle, calcolata sui valori non arrotondati, può differire per
/// l'arrotondamento dei fattori: al più mezzo decimillesimo per ogni fattore
/// moltiplicato per il prodotto degli altri, più 5·10⁻⁵ (per una nuova
/// muratura di 7,12 × 3,05 m circa 4·10⁻⁴ m², per un locale al più 5·10⁻⁵).
pub fn to_csv(abaco: &Abaco) -> String {
    let mut out = String::new();
    out.push(CSV_BOM);
    out.push_str(CSV_HEADER);
    out.push_str(CSV_EOL);
    for record in csv_records(abaco) {
        for (n, field) in record.fields().iter().enumerate() {
            if n > 0 {
                out.push(CSV_SEPARATOR);
            }
            push_csv_field(&mut out, field);
        }
        out.push_str(CSV_EOL);
    }
    out
}

/// Quante righe di misura ha il CSV di [`to_csv`].
pub fn csv_row_count(abaco: &Abaco) -> usize {
    csv_records(abaco).len()
}

/// Una riga del CSV: una misura del libretto di ArchCme con `parti` = 1, mai
/// a detrarre. I fattori sono quelli non arrotondati; `None` = un fattore che
/// non si applica: campo vuoto, saltato nel prodotto. La quantità non si
/// tiene: è il prodotto dei fattori scritti ([`CsvRecord::fields`]).
pub(super) struct CsvRecord {
    pub(super) tipo: &'static str,
    pub(super) code: String,
    pub(super) description: String,
    pub(super) length: f64,
    pub(super) width: Option<f64>,
    pub(super) height: Option<f64>,
    pub(super) unit: &'static str,
    pub(super) height_state: &'static str,
    pub(super) source: String,
    pub(super) notes: Vec<String>,
    /// Una detrazione (`detrai` = sì): la quantità si toglie.
    pub(super) deduct: bool,
}

impl CsvRecord {
    /// I campi nell'ordine di [`CSV_HEADER`], senza virgolette. I fattori si
    /// scrivono arrotondati a quattro decimali e `quantita` è il loro prodotto
    /// così come sono scritti (i vuoti saltati), arrotondato a sua volta
    /// (Ruling S).
    fn fields(&self) -> [String; 13] {
        let [parti, length, width, height] =
            [Some(1.0), Some(self.length), self.width, self.height].map(|f| f.map(csv_round));
        let quantity: f64 = [parti, length, width, height].iter().flatten().product();
        let factor = |value: Option<f64>| value.map_or_else(String::new, csv_number);
        [
            self.tipo.to_string(),
            self.code.clone(),
            self.description.clone(),
            factor(parti),
            factor(length),
            factor(width),
            factor(height),
            if self.deduct { CSV_DEDUCTION } else { CSV_NO_DEDUCTION }.to_string(),
            csv_number(quantity),
            self.unit.to_string(),
            self.height_state.to_string(),
            self.source.clone(),
            self.notes.join(CSV_NOTE_SEP),
        ]
    }
}

/// `value` come lo scrive [`csv_number`], letto di nuovo come numero: lo
/// stesso arrotondamento di `format_fixed` a quattro decimali.
fn csv_round(value: f64) -> f64 {
    let scale = 10_f64.powi(CSV_DECIMALS as i32);
    (value * scale).round() / scale
}

fn csv_number(value: f64) -> String {
    format_fixed(value, CSV_DECIMALS)
}

/// Le misure delle righe complete, nell'ordine di [`to_csv`]. Le note di
/// accoppiamento di una riga di locali («unito in C», «da A + B») vanno al
/// locale che la riga ha da solo.
fn csv_records(abaco: &Abaco) -> Vec<CsvRecord> {
    let zones = abaco
        .demolizioni
        .iter()
        .chain(&abaco.varchi)
        .chain(&abaco.nuove)
        .filter_map(zone_record);
    let rooms = |phase: Phase| {
        abaco.locali.iter().filter_map(move |line| {
            let row = match phase {
                Phase::Fatto => line.fatto.as_ref(),
                Phase::Progetto => line.progetto.as_ref(),
            }?;
            room_record(row, line.note.as_deref())
        })
    };
    zones
        .chain(rooms(Phase::Fatto))
        .chain(rooms(Phase::Progetto))
        .chain(super::abaco_aperture::csv_records(&abaco.aperture))
        .chain(super::abaco_strata::csv_records(abaco))
        .collect()
}

/// La misura di una zona completa (`None` se incompleta: la quantità
/// principale o un suo fattore manca o non è finito). Con l'asse, lo spessore
/// effettivo (area ÷ lunghezza) fa sì che lunghezza × spessore × altezza sia
/// il volume anche con buchi o con l'asse accorciato, a meno
/// dell'arrotondamento dei fattori scritti.
pub(super) fn zone_record(row: &ZoneRow) -> Option<CsvRecord> {
    let finite = |value: Option<f64>| value.filter(|v| v.is_finite());
    // Solo le righe complete; la quantità si riscrive dai fattori (Ruling S).
    finite(row.main())?;
    let height = finite(row.height.value_m)?;
    let axis = finite(row.length_m).zip(finite(row.thickness_m));
    let thickness =
        |value: Option<f64>| finite(value).map_or_else(|| ND.to_string(), |t| format_fixed(t, 3));
    let (tipo, description, length, width, unit) = match row.kind {
        ZoneKind::Demolizione => match axis {
            Some((length, width)) => (
                "demolizione",
                format!("Demolizione muro spessore {}", thickness(Some(width))),
                length,
                Some(width),
                UNIT_M3,
            ),
            None => (
                "demolizione",
                DEMOLITION_WITHOUT_AXIS.to_string(),
                finite(Some(row.plan_area_m2))?,
                None,
                UNIT_M3,
            ),
        },
        ZoneKind::Varco => {
            let (length, width) = axis?;
            ("varco", "Varco".to_string(), length, Some(width), UNIT_M3)
        }
        ZoneKind::Nuova => (
            "nuova muratura",
            format!("Nuova muratura spessore {}", thickness(row.thickness_m)),
            finite(row.length_m)?,
            None,
            UNIT_M2,
        ),
    };
    Some(CsvRecord {
        tipo,
        code: row.code.clone(),
        description,
        length,
        width,
        height: Some(height),
        unit,
        height_state: height_state(&row.height),
        source: format!("zona {:X}", row.handle),
        notes: zone_notes(row),
        deduct: false,
    })
}

/// La misura di un locale con la superficie (`None` senza): la superficie
/// netta, un fattore solo. L'altezza non è un fattore, ma il suo stato resta:
/// `n.d.` per un locale senza altezza, che è completo (Ruling O), con il
/// motivo nelle note.
pub(super) fn room_record(row: &RoomRow, line_note: Option<&str>) -> Option<CsvRecord> {
    if !row.is_complete() {
        return None;
    }
    let area = row.area_m2.filter(|area| area.is_finite())?;
    let tipo = match row.phase {
        Phase::Fatto => "locale di fatto",
        Phase::Progetto => "locale di progetto",
    };
    let mut notes: Vec<String> = line_note.map(str::to_string).into_iter().collect();
    notes.extend(room_notes(row));
    Some(CsvRecord {
        tipo,
        code: row.code.clone(),
        description: row.name.clone(),
        length: area,
        width: None,
        height: None,
        unit: UNIT_M2,
        height_state: height_state(&row.height),
        source: format!("locale {}", row.name),
        notes,
        deduct: false,
    })
}

/// Un campo del CSV: tra virgolette, con le virgolette raddoppiate, se ha
/// `;`, virgolette o un a capo (RFC 4180); altrimenti com'è.
fn push_csv_field(out: &mut String, field: &str) {
    if field.contains(|c: char| c == CSV_SEPARATOR || matches!(c, '"' | '\n' | '\r')) {
        out.push('"');
        out.push_str(&field.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(field);
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
    //! Il calcolo dell'abaco contro `AL_AGGIORNA` e `AL_ESTRAI` (R61), il CSV
    //! (spec §5.3) e il formato dei numeri; la resa delle tabelle si prova in
    //! `abaco_render`.
    use super::test_support::*;
    use super::*;

    use codec::EntityType;

    use crate::bim::extract::{extract, Elaborato, ExtractOptions};
    use crate::bim::fixtures::{shift_tramezzo, Form, LAYER_MURI};
    use crate::bim::refresh::refresh_rooms;
    use crate::bim::room_command::test_support::write_room;
    use crate::bim::rooms::label;
    use crate::bim::walls::rect_from_axis_thickness;
    use crate::bim::zone::{zone_hatch, Zone, ZoneOrigin, ZoneState};
    use crate::bim::LAYER_LOCALI;
    use crate::scene::Scene;

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

    const CSV_HEADER_LINE: &str = "\u{FEFF}tipo;codice;descrizione;parti;lunghezza;larghezza;\
        altezza;detrai;quantita;um;stato_altezza;fonte;note\r\n";

    /// Le righe del CSV, campo per campo, come le legge un foglio di calcolo
    /// (RFC 4180 con `;`): un campo tra virgolette può avere `;`, virgolette
    /// raddoppiate e a capo. Ogni riga finisce con CRLF; un a capo senza CR
    /// fuori dalle virgolette è un errore.
    fn parse_csv(text: &str) -> Vec<Vec<String>> {
        let text = text.strip_prefix('\u{FEFF}').expect("BOM in testa");
        let mut records = Vec::new();
        let mut record = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match (quoted, c) {
                (true, '"') if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                (true, '"') => quoted = false,
                (true, _) => field.push(c),
                (false, '"') if field.is_empty() => quoted = true,
                (false, ';') => record.push(std::mem::take(&mut field)),
                (false, '\r') => {
                    assert_eq!(chars.next(), Some('\n'), "CR senza LF");
                    record.push(std::mem::take(&mut field));
                    records.push(std::mem::take(&mut record));
                }
                (false, '\n') => panic!("a capo senza CR fuori dalle virgolette: {text:?}"),
                (false, _) => field.push(c),
            }
        }
        assert!(
            !quoted && field.is_empty() && record.is_empty(),
            "ultima riga senza CRLF: {text:?}"
        );
        records
    }

    /// Un numero del CSV: vuoto = fattore che non si applica; altrimenti
    /// quattro decimali con la virgola.
    #[track_caller]
    fn csv_number(field: &str) -> Option<f64> {
        if field.is_empty() {
            return None;
        }
        let (_, decimals) = field.split_once(',').expect("virgola decimale");
        assert_eq!(decimals.len(), 4, "quattro decimali: {field}");
        Some(field.replace(',', ".").parse().expect("numero"))
    }

    /// I campi della riga con il codice `code`.
    #[track_caller]
    fn record_of<'a>(records: &'a [Vec<String>], code: &str) -> &'a [String] {
        records
            .iter()
            .find(|record| record[1] == code)
            .unwrap_or_else(|| panic!("nessuna riga {code}: {records:?}"))
    }

    /// Spec §5.3: BOM UTF-8, intestazione, `;`, virgola decimale, quattro
    /// decimali, CRLF; la demolizione del caso A (0,10 × 3,00, altezza
    /// ereditata 2,70) è 1 × 3,0000 × 0,1000 × 2,7000 = 0,8100 m³.
    #[test]
    fn csv_has_bom_crlf_and_semicolons() {
        let mut scene = scene_of(Form::Continua);
        set_default_height(&mut scene, 2.7);
        demolish(&mut scene, [4.05, 0.0], [4.05, 3.0]);
        let abaco = compute_abaco(&scene.document, &OPTS);
        let handle = abaco.demolizioni[0].handle;
        let csv = to_csv(&abaco);
        assert_eq!(
            csv,
            format!(
                "{CSV_HEADER_LINE}demolizione;D1;Demolizione muro spessore 0,100;1,0000;3,0000;\
                 0,1000;2,7000;no;0,8100;m³;ereditata;zona {handle:X};\r\n"
            )
        );
        let records = parse_csv(&csv);
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| record.len() == 13), "{records:?}");
        for field in &records[1][3..7] {
            csv_number(field);
        }
        assert_eq!(csv_row_count(&abaco), 1);
    }

    /// Spec §7, caso H: senza altezza la demolizione è incompleta e il CSV ha
    /// solo l'intestazione; lo stesso per un varco senza altezza propria e una
    /// zona nuova senza asse, anche con la predefinita. Due locali di fatto
    /// fusi (nessuna superficie) restano fuori, il terzo entra.
    #[test]
    fn csv_omits_incomplete_rows() {
        for form in FORMS {
            let mut scene = scene_of(form);
            demolish(&mut scene, [4.05, 0.0], [4.05, 3.0]);
            let abaco = compute_abaco(&scene.document, &OPTS);
            assert_eq!(incomplete(&abaco).len(), 1, "{form:?}");
            assert_eq!(to_csv(&abaco), CSV_HEADER_LINE, "{form:?}");
            assert_eq!(csv_row_count(&abaco), 0, "{form:?}");
        }

        let mut scene = empty_scene();
        set_default_height(&mut scene, 2.7);
        let varco = Some(([20.05, 1.0], [20.05, 1.9]));
        let ring = rect(20.0, 1.0, 20.1, 1.9);
        add_zone(&mut scene, ZoneState::Demolito, varco, None, true, ring);
        let ring = rect(0.0, 5.0, 2.0, 5.08);
        add_zone(&mut scene, ZoneState::Nuovo, None, None, false, ring);
        let abaco = compute_abaco(&scene.document, &OPTS);
        assert_eq!(incomplete(&abaco).len(), 2, "{:?}", abaco.notices);
        assert_eq!(to_csv(&abaco), CSV_HEADER_LINE);

        let mut scene = scene_of(Form::Continua);
        set_default_height(&mut scene, 2.7);
        write_room(&mut scene, "A", [2.0, 1.5]);
        write_room(&mut scene, "Cucina", [3.0, 1.0]);
        write_room(&mut scene, "B", [6.0, 1.5]);
        let abaco = compute_abaco(&scene.document, &OPTS);
        let reasons = incomplete(&abaco);
        let codes: Vec<&str> = reasons.iter().map(|(code, _)| code.as_str()).collect();
        assert_eq!(codes, ["LF1", "LF2"], "{reasons:?}");
        let records = parse_csv(&to_csv(&abaco));
        let codes: Vec<&str> = records[1..].iter().map(|r| r[1].as_str()).collect();
        assert!(codes.contains(&"LF3"), "{codes:?}");
        assert!(!codes.contains(&"LF1") && !codes.contains(&"LF2"), "{codes:?}");
        assert_eq!(csv_row_count(&abaco), records.len() - 1);
    }

    /// Arrotondato a quattro decimali come `format_fixed(value, 4)`.
    fn round4(value: f64) -> f64 {
        (value * 10_000.0).round() / 10_000.0
    }

    /// Una zona demolita con l'asse e un buco: l'anello esterno `outer` meno
    /// `hole`.
    fn add_demolition_with_hole(
        scene: &mut Scene,
        axis: ([f64; 2], [f64; 2]),
        outer: Vec<[f64; 2]>,
        hole: Vec<[f64; 2]>,
    ) {
        let zone = Zone {
            state: ZoneState::Demolito,
            origin: ZoneOrigin::MuroDuePunti,
            axis: Some(axis),
            thickness: None,
            height: None,
            varco: false,
        };
        scene.add_entity(zone_hatch(&zone, &[outer, hole]));
    }

    /// Spec §7 e Ruling S: per ogni riga `quantita` è il prodotto
    /// `parti × lunghezza × larghezza × altezza` dei fattori come sono scritti
    /// (i vuoti saltati come nel libretto di ArchCme), arrotondato a quattro
    /// decimali: ricalcolato dai campi letti torna identico (entro 1e-9) e
    /// dista dal prodotto non arrotondato al più 5e-5, anche su misure non
    /// tonde; dalla quantità delle tabelle (valori non arrotondati) meno di
    /// 1e-3 in questi casi. Casi A (0,81 m³), A' (altezza inserita 3,00: 0,90
    /// m³), demolizione senza asse (0,81 m³), una zona con un buco (L 3,00,
    /// area 0,29: 3,0000 × 0,0967 × 2,7000 = 0,7833 contro 0,783 della
    /// tabella), un muro da rilievo non tondo (L 4,56789, spessore
    /// 0,12345678), B (varco con 2,10: 0,189 m³), G (nuova 2,00 × 2,70: 5,40
    /// m²), due nuove lunghe 7,12345 (altezza inserita 3,04567, ed ereditata
    /// 2,70) e i locali (A 12,00, B 11,70, progetto A + B 24,00 m²).
    #[test]
    fn csv_product_equals_quantity() {
        let mut scene = scene_of(Form::Continua);
        set_default_height(&mut scene, 2.7);
        write_room(&mut scene, "A", [2.0, 1.5]);
        write_room(&mut scene, "B", [6.0, 1.5]);
        let demolito = ZoneState::Demolito;
        demolish(&mut scene, [4.05, 0.0], [4.05, 3.0]);
        let axis = Some(([12.05, 0.0], [12.05, 3.0]));
        add_zone(&mut scene, demolito, axis, Some(3.0), false, rect(12.0, 0.0, 12.1, 3.0));
        add_zone(&mut scene, demolito, None, None, false, rect(14.0, 0.0, 14.1, 3.0));
        add_demolition_with_hole(
            &mut scene,
            ([30.05, 0.0], [30.05, 3.0]),
            rect(30.0, 0.0, 30.1, 3.0),
            rect(30.025, 1.0, 30.075, 1.2),
        );
        let (length, width) = (4.56789, 0.12345678);
        let axis = Some(([40.0, 5.0], [40.0 + length, 5.0]));
        let ring = rect(40.0, 5.0 - width / 2.0, 40.0 + length, 5.0 + width / 2.0);
        add_zone(&mut scene, demolito, axis, None, false, ring);
        let axis = Some(([20.05, 1.0], [20.05, 1.9]));
        add_zone(&mut scene, demolito, axis, Some(2.1), true, rect(20.0, 1.0, 20.1, 1.9));
        let axis = Some(([0.0, 10.04], [2.0, 10.04]));
        let ring = rect(0.0, 10.0, 2.0, 10.08);
        add_zone(&mut scene, ZoneState::Nuovo, axis, None, false, ring);
        for (y, height) in [(20.0, Some(3.04567)), (22.0, None)] {
            let axis = Some(([0.0, y + 0.04], [7.12345, y + 0.04]));
            let ring = rect(0.0, y, 7.12345, y + 0.08);
            add_zone(&mut scene, ZoneState::Nuovo, axis, height, false, ring);
        }
        let abaco = compute_abaco(&scene.document, &OPTS);
        assert!(incomplete(&abaco).is_empty(), "{:?}", abaco.notices);
        // La quantità principale non arrotondata di ogni riga, per codice.
        let mut unrounded: Vec<(String, f64)> = abaco
            .demolizioni
            .iter()
            .chain(&abaco.varchi)
            .chain(&abaco.nuove)
            .map(|row| (row.code.clone(), row.main().expect("completa")))
            .collect();
        for line in &abaco.locali {
            for row in [&line.fatto, &line.progetto].into_iter().flatten() {
                unrounded.push((row.code.clone(), row.area_m2.expect("superficie")));
            }
        }

        let records = parse_csv(&to_csv(&abaco));
        assert_eq!(records.len() - 1, unrounded.len(), "{records:?}");
        for record in &records[1..] {
            let code = &record[1];
            let factors: f64 = record[3..7].iter().filter_map(|f| csv_number(f)).product();
            let quantity = csv_number(&record[8]).expect("quantità");
            assert!(
                (round4(factors) - quantity).abs() <= 1e-9,
                "{code}: prodotto dei fattori scritti {factors}, quantita {quantity}"
            );
            assert!((factors - quantity).abs() <= 5e-5 + 1e-12, "{code}");
            let (_, table) = unrounded
                .iter()
                .find(|(c, _)| c == code)
                .unwrap_or_else(|| panic!("{code} non nell'abaco"));
            assert!(
                (quantity - table).abs() < 1e-3,
                "{code}: {quantity} contro {table} delle tabelle"
            );
        }
        let rows: Vec<(&str, &str, &str, &str)> = records[1..]
            .iter()
            .map(|r| (r[0].as_str(), r[1].as_str(), r[8].as_str(), r[9].as_str()))
            .collect();
        assert_eq!(rows.len(), 12, "{rows:?}");
        assert_eq!(
            rows[..4],
            [
                ("demolizione", "D1", "0,8100", "m³"),
                ("demolizione", "D2", "0,9000", "m³"),
                ("demolizione", "D3", "0,8100", "m³"),
                ("demolizione", "D4", "0,7833", "m³"),
            ]
        );
        assert_eq!(
            rows[5..],
            [
                ("varco", "V1", "0,1890", "m³"),
                ("nuova muratura", "N1", "5,4000", "m²"),
                ("nuova muratura", "N2", "21,6960", "m²"),
                ("nuova muratura", "N3", rows[8].2, "m²"),
                ("locale di fatto", "LF1", "12,0000", "m²"),
                ("locale di fatto", "LF2", "11,7000", "m²"),
                ("locale di progetto", "LP1", "24,0000", "m²"),
            ]
        );
        let d4 = record_of(&records, "D4");
        assert_eq!(&d4[3..9], ["1,0000", "3,0000", "0,0967", "2,7000", "no", "0,7833"]);
        let d5 = record_of(&records, "D5");
        assert_eq!(&d5[3..9], ["1,0000", "4,5679", "0,1235", "2,7000", "no", "1,5232"]);
        let n2 = record_of(&records, "N2");
        assert_eq!(&n2[3..9], ["1,0000", "7,1235", "", "3,0457", "no", "21,6960"]);
        assert_eq!(&record_of(&records, "N3")[3..7], ["1,0000", "7,1235", "", "2,7000"]);
        let d2 = record_of(&records, "D2");
        assert_eq!((d2[6].as_str(), d2[10].as_str()), ("3,0000", "inserita"));
        let v1 = record_of(&records, "V1");
        assert_eq!(&v1[2..7], ["Varco", "1,0000", "0,9000", "0,1000", "2,1000"]);
        let n1 = record_of(&records, "N1");
        assert_eq!(
            &n1[2..7],
            ["Nuova muratura spessore 0,080", "1,0000", "2,0000", "", "2,7000"]
        );
        let lf1 = record_of(&records, "LF1");
        assert_eq!(&lf1[2..7], ["A", "1,0000", "12,0000", "", ""]);
        assert_eq!((lf1[10].as_str(), lf1[11].as_str()), ("ereditata", "locale A"));
        assert!(lf1[12].contains("unito in A + B"), "{lf1:?}");
        let lp1 = record_of(&records, "LP1");
        assert_eq!(lp1[2], "A + B");
        assert!(lp1[12].contains("da A + B"), "{lp1:?}");
        assert!(lp1[12].contains("da confermare"), "{lp1:?}");
        assert!(records[1..].iter().all(|r| r[7] == "no"), "nessuna detrazione");
    }

    /// Un nome di locale con `;`, virgolette e un a capo resta un campo solo,
    /// tra virgolette con le virgolette raddoppiate (RFC 4180): il file ha
    /// ancora una riga logica per misura.
    #[test]
    fn csv_quotes_special_characters() {
        let mut scene = scene_of(Form::Continua);
        set_default_height(&mut scene, 2.7);
        write_room(&mut scene, "A", [2.0, 1.5]);
        write_room(&mut scene, "B", [6.0, 1.5]);
        let mut abaco = compute_abaco(&scene.document, &OPTS);
        let name = "Bagno; \"WC\"\nsecondo";
        abaco.locali[0].fatto.as_mut().expect("A di fatto").name = name.into();
        let csv = to_csv(&abaco);
        assert!(
            csv.contains("LF1;\"Bagno; \"\"WC\"\"\nsecondo\";1,0000;"),
            "{csv:?}"
        );
        let records = parse_csv(&csv);
        let codes: Vec<&str> = records.iter().map(|r| r[1].as_str()).collect();
        assert_eq!(codes, ["codice", "LF1", "LF2", "LP1", "LP2"]);
        assert!(records.iter().all(|record| record.len() == 13), "{records:?}");
        let lf1 = record_of(&records, "LF1");
        assert_eq!(lf1[2], name);
        assert_eq!(lf1[11], format!("locale {name}"));
    }

    /// Spec §5.3: una demolizione senza asse ha l'area in pianta come
    /// lunghezza, la larghezza vuota e la descrizione che lo dice; il prodotto
    /// resta area × altezza.
    #[test]
    fn csv_demolition_without_axis_uses_area_as_length() {
        let mut scene = empty_scene();
        set_default_height(&mut scene, 2.7);
        let ring = rect(10.0, 0.0, 10.1, 3.0);
        add_zone(&mut scene, ZoneState::Demolito, None, None, false, ring);
        let abaco = compute_abaco(&scene.document, &OPTS);
        let handle = abaco.demolizioni[0].handle;
        let records = parse_csv(&to_csv(&abaco));
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[1],
            [
                "demolizione",
                "D1",
                "Demolizione area in pianta (zona senza asse)",
                "1,0000",
                "0,3000",
                "",
                "2,7000",
                "no",
                "0,8100",
                "m³",
                "ereditata",
                &format!("zona {handle:X}"),
                "zona senza asse",
            ]
        );
    }

    /// Ruling O: un locale con la superficie e senza altezza (nessuna
    /// predefinita, nessuna sul locale) entra nel CSV: 1 × superficie in m²,
    /// `altezza` vuota (non è un fattore del locale), `stato_altezza` n.d. e il
    /// motivo nelle note. Un locale non chiuso (la faccia ovest di A tolta),
    /// che non ha superficie, non entra, né di fatto né di progetto.
    #[test]
    fn csv_room_without_height_is_exported_with_nd() {
        let mut scene = scene_of(Form::Continua);
        write_room(&mut scene, "A", [2.0, 1.5]);
        write_room(&mut scene, "B", [6.0, 1.5]);
        let abaco = compute_abaco(&scene.document, &OPTS);
        assert!(incomplete(&abaco).is_empty(), "{:?}", abaco.notices);
        let records = parse_csv(&to_csv(&abaco));
        let codes: Vec<&str> = records[1..].iter().map(|r| r[1].as_str()).collect();
        assert_eq!(codes, ["LF1", "LF2", "LP1", "LP2"]);
        let lf1 = record_of(&records, "LF1");
        assert_eq!(
            lf1,
            [
                "locale di fatto",
                "LF1",
                "A",
                "1,0000",
                "12,0000",
                "",
                "",
                "no",
                "12,0000",
                "m²",
                "n.d.",
                "locale A",
                "altezza n.d.",
            ]
        );
        let lp1 = record_of(&records, "LP1");
        assert_eq!(
            &lp1[3..11],
            ["1,0000", "12,0000", "", "", "no", "12,0000", "m²", "n.d."]
        );
        assert!(
            lp1[12].contains("altezza mancante nei locali di fatto"),
            "{lp1:?}"
        );
        assert_eq!(csv_row_count(&abaco), 4);

        // A non chiuso: nessuna superficie, nessuna riga di A.
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
        let abaco = compute_abaco(&scene.document, &OPTS);
        let reasons = incomplete(&abaco);
        assert!(
            reasons
                .iter()
                .any(|(code, reason)| code == "LF1" && reason.contains("non chiuso")),
            "{reasons:?}"
        );
        let records = parse_csv(&to_csv(&abaco));
        let codes: Vec<&str> = records[1..].iter().map(|r| r[1].as_str()).collect();
        assert!(codes.contains(&"LF2"), "{codes:?}");
        assert!(!codes.contains(&"LF1"), "{codes:?}");
        assert!(records[1..].iter().all(|r| r[2] != "A"), "{records:?}");
        assert!(records[1..].iter().all(|r| r[10] == "n.d."), "{records:?}");
    }

    /// Un muro nuovo come `AL_NUOVO`: il rettangolo di spessore `t` centrato
    /// sull'asse `p1`-`p2` (`rect_from_axis_thickness`), asse e spessore nel
    /// record.
    fn add_new_wall(scene: &mut Scene, p1: [f64; 2], p2: [f64; 2], t: f64) {
        let rect = rect_from_axis_thickness(p1, p2, t).expect("asse");
        let zone = Zone {
            state: ZoneState::Nuovo,
            origin: ZoneOrigin::LineaSpessore,
            axis: Some(rect.axis),
            thickness: Some(rect.thickness),
            height: None,
            varco: false,
        };
        scene.add_entity(zone_hatch(&zone, &[rect.ring()]));
    }

    /// Gli avvisi di sovrapposizione dell'abaco.
    fn overlaps(abaco: &Abaco) -> Vec<Notice> {
        abaco
            .notices
            .iter()
            .filter(|notice| {
                matches!(
                    notice,
                    Notice::NuoveSovrapposte { .. }
                        | Notice::ZoneSovrapposteSostanzialmente { .. }
                        | Notice::StessoStatoSovrapposte
                )
            })
            .cloned()
            .collect()
    }

    /// Revisione finale (spec §4.3, §7 caso K): due muri nuovi di `AL_NUOVO`
    /// ad angolo, asse su asse, si coprono per t/2 × t/2 e due loro lati si
    /// incrociano in (t/2; t/2): la geometria di progetto lo segnala
    /// (`NuoveSovrapposte`), l'abaco no. Una sovrapposizione vera (metà del
    /// muro più piccolo) dà un avviso solo per la coppia, quello delle
    /// quantità (`ZoneSovrapposteSostanzialmente`).
    #[test]
    fn overlap_notice_only_for_a_real_overlap_and_once() {
        let mut scene = empty_scene();
        set_default_height(&mut scene, 2.7);
        add_new_wall(&mut scene, [0.0, 0.0], [2.0, 0.0], 0.10);
        add_new_wall(&mut scene, [0.0, 0.0], [0.0, 2.0], 0.10);
        let (_, geometry) = project_room_rows(&scene.document, M, TOL, TOL, Some(2.7), &[]);
        assert!(
            geometry
                .iter()
                .any(|n| matches!(n, Notice::NuoveSovrapposte { .. })),
            "la geometria lo vede: {geometry:?}"
        );
        let abaco = compute_abaco(&scene.document, &OPTS);
        assert_eq!(abaco.nuove.len(), 2);
        assert!(overlaps(&abaco).is_empty(), "angolo: {:?}", abaco.notices);

        let mut scene = empty_scene();
        set_default_height(&mut scene, 2.7);
        add_new_wall(&mut scene, [0.0, 0.0], [2.0, 0.0], 0.10);
        add_new_wall(&mut scene, [1.0, 0.0], [3.0, 0.0], 0.10);
        let abaco = compute_abaco(&scene.document, &OPTS);
        assert_eq!(
            overlaps(&abaco),
            [Notice::ZoneSovrapposteSostanzialmente {
                codes: vec!["N1".into(), "N2".into()],
            }],
            "{:?}",
            abaco.notices
        );
    }

    /// Revisione finale: gli avvisi del solo disegno di progetto, che non
    /// dicono nulla dei numeri dell'abaco (giunzioni da verificare, entità
    /// escluse dall'estratto o lasciate intere), restano di `AL_ESTRAI` e
    /// l'abaco non li ripete a ogni lancio. Restano quelli che spiegano i
    /// locali di progetto, come i confini di una zona senza asse non tolti.
    #[test]
    fn abaco_leaves_drawing_only_notices_to_the_extraction() {
        let drawing_only = |notice: &Notice| {
            matches!(
                notice,
                Notice::GiunzioneDaVerificare { .. }
                    | Notice::Esclusa { .. }
                    | Notice::NonTagliata { .. }
                    | Notice::NuoveSovrapposte { .. }
            )
        };
        let mut scene = scene_of(Form::ChiusaPerMuro);
        set_default_height(&mut scene, 2.7);
        demolish(&mut scene, [4.05, 0.0], [4.05, 3.0]);
        let mut text = codec::entities::MText::with_value("x", codec::Vector3::new(4.05, 1.5, 0.0));
        text.common.layer = "TESTI".into();
        scene.add_entity(EntityType::MText(Box::new(text)));
        let (_, geometry) = project_room_rows(&scene.document, M, TOL, TOL, Some(2.7), &[]);
        for (what, found) in [
            (
                "giunzione",
                geometry
                    .iter()
                    .any(|n| matches!(n, Notice::GiunzioneDaVerificare { .. })),
            ),
            (
                "esclusa",
                geometry.iter().any(|n| matches!(n, Notice::Esclusa { .. })),
            ),
        ] {
            assert!(found, "{what}: {geometry:?}");
        }
        let abaco = compute_abaco(&scene.document, &OPTS);
        assert!(
            !abaco.notices.iter().any(drawing_only),
            "{:?}",
            abaco.notices
        );

        let mut scene = scene_of(Form::Continua);
        set_default_height(&mut scene, 2.7);
        add_zone(
            &mut scene,
            ZoneState::Demolito,
            None,
            None,
            false,
            rect(4.0, 0.0, 4.1, 3.0),
        );
        let abaco = compute_abaco(&scene.document, &OPTS);
        assert!(
            abaco
                .notices
                .iter()
                .any(|n| matches!(n, Notice::ConfiniNonRimossi { .. })),
            "{:?}",
            abaco.notices
        );
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
