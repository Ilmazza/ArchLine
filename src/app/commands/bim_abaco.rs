//! I comandi BIM `AL_ABACO` e `AL_ABACO_CSV` della parte 2 (spec
//! `2026-10-28-bim-abachi-quantita-design.md`, §5.2 e §5.3), lato app.
//!
//! `AL_ABACO [x,y]` calcola l'abaco (`abaco::compute_abaco`, con le
//! tolleranze di `AL_AGGIORNA` e `AL_ESTRAI`) e lo scrive come tabelle `TABLE`
//! sul layer `AL-ABACO` (`abaco_table`). Regole:
//! - spazio modello, poi l'eventuale punto (`x,y` col punto decimale, in
//!   coordinate del mondo), poi zone e locali: senza né le une né gli altri
//!   (e senza tabelle d'abaco) un messaggio e basta, senza chiedere l'unità;
//!   poi l'unità del disegno (`AL_UNITA`, che rilancia la riga intera);
//! - le tabelle già presenti si riscrivono dove si trovano, copie comprese,
//!   solo se cambiano; quelle su layer bloccati non si toccano e si contano;
//!   quelle di un tipo che non ha più righe si tolgono (un numero vecchio non
//!   resta nel disegno). Quelle mancanti vanno sotto l'ultima tabella che
//!   resta; se non ce n'è nessuna, nel punto dato, oppure al clic
//!   ([`AbacoPointCommand`], che rilancia `AL_ABACO <x>,<y>`), oppure, da uno
//!   script senza punto, a destra dell'estensione dello spazio modello con un
//!   margine pari alla larghezza della tabella più larga. Le tabelle create
//!   prendono lo stile tabella corrente del disegno
//!   (`abaco_table::current_table_style`, nessuno se non si trova); quelle
//!   riscritte tengono lo stile che hanno (Ruling Q);
//! - il layer `AL-ABACO` e l'APPID, se mancano, in un passo di annullamento a
//!   sé (`AL_LAYER`, come `bim_ensure_layers`); sostituzioni, creazioni e
//!   rimozioni in un solo passo `AL_ABACO`, e nessun passo se nulla cambia;
//! - in riga di comando: tabelle create e sostituite, righe e righe
//!   incomplete, poi gli avvisi dell'abaco (prima un `RigaIncompleta` per riga
//!   incompleta, con il motivo). Rilievo, zone, locali e record non cambiano.
//!
//! La riga `AL_ABACO <x,y>` va al comando intera (elenco del runner in
//! `command_driver/utilities.rs`): da solo, in uno script, il comando scrive
//! subito, e il runner non deve provarlo senza il punto.
//!
//! `AL_ABACO_CSV [percorso]` scrive lo stesso abaco come CSV
//! (`abaco::to_csv`: solo le righe complete). Regole:
//! - il percorso è il resto della riga, spazi compresi, anche tra virgolette
//!   (anche questa riga va intera al comando, elenco del runner); legge il
//!   disegno e basta, quindi vale anche da un layout, come `AL_ESTRAI`;
//! - con il percorso un file che c'è già non si riscrive mai: niente è
//!   scritto e lo si dice (così anche una cartella che non c'è); senza
//!   percorso, da uno script (`scripted_dispatch`) è un errore, da una persona
//!   si apre la finestra «Salva con nome» del sistema, che chiede lei se
//!   sovrascrivere, e la risposta torna come `BimMsg::AbacoCsvPath` con il
//!   testo già calcolato;
//! - senza zone né locali un messaggio e basta, senza chiedere l'unità; poi
//!   l'unità del disegno (`AL_UNITA`, che rilancia la riga intera);
//! - il file si scrive in un file temporaneo nella stessa cartella e poi si
//!   rinomina: un errore non lascia un file a metà né quello vecchio rovinato;
//! - in riga di comando il file scritto, le righe scritte e quelle incomplete
//!   rimaste fuori, poi gli avvisi dell'abaco (un `RigaIncompleta` per riga
//!   incompleta, con il motivo). Il disegno non cambia.

use crate::app::{Message, OpenCADStudio};
use crate::bim::abaco::{
    compute_abaco, csv_row_count, render_tables, to_csv, Abaco, AbacoOptions, TableText,
};
use crate::bim::abaco_table::{self, kind_name, AbacoPlan};
use crate::bim::report::Notice;
use crate::bim::rooms::rooms_in;
use crate::bim::units::DrawingUnit;
use crate::bim::zone::zones_in;
use crate::bim::APP_ABACO;
use crate::command::{CadCommand, CmdResult};
use crate::scene::view::dispatch::set_entity_xdata;
use crate::ui::window::bim_extract::BimMsg;
use codec::{EntityType, Handle};
use glam::DVec3;
use iced::Task;
use std::path::{Path, PathBuf};

use super::bim::{base_name, room_tolerance, unquote};

const CMD: &str = "AL_ABACO";
const CSV_CMD: &str = "AL_ABACO_CSV";

const POINT_PROMPT: &str = "AL_ABACO  Punto di inserimento delle tabelle:";

/// Attende il punto delle tabelle e rilancia `AL_ABACO <x>,<y>`, coordinate
/// del mondo col punto decimale; Invio annulla.
struct AbacoPointCommand;

impl CadCommand for AbacoPointCommand {
    fn name(&self) -> &'static str {
        CMD
    }
    fn prompt(&self) -> String {
        POINT_PROMPT.to_string()
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        CmdResult::Relaunch(format!("{CMD} {},{}", pt.x, pt.y), vec![])
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

impl OpenCADStudio {
    /// `AL_ABACO [x,y]` (spec parte 2, §5.2): vedi le regole del modulo.
    pub(super) fn bim_abaco(&mut self, i: usize, cmd: &str) -> Task<Message> {
        if !self.bim_in_model_space(i, CMD) {
            return Task::none();
        }
        let point = match point_argument(cmd) {
            Ok(point) => point,
            Err(message) => {
                self.command_line.push_error(&message);
                return Task::none();
            }
        };
        let doc = &self.tabs[i].scene.document;
        if zones_in(doc).is_empty()
            && rooms_in(doc).is_empty()
            && abaco_table::tables_in(doc).is_empty()
        {
            // Un rilancio lasciato a metà (AL_UNITA annullato) non vale più.
            self.bim_pending = None;
            self.command_line
                .push_info("AL_ABACO: nessuna zona né locale nel disegno: nessuna tabella.");
            return Task::none();
        }
        let Some(unit) = self.bim_unit_or_ask(i, cmd.trim()) else {
            return self.finish_dispatch(CMD);
        };
        self.bim_pending = None;
        let scene = &self.tabs[i].scene;
        let abaco = compute_abaco(&scene.document, &abaco_options(unit));
        let texts = render_tables(&abaco);
        let plan = abaco_table::plan(&scene.document, &texts, unit, |handle| {
            scene.is_layer_locked(handle)
        });
        let missing: Vec<&TableText> = plan
            .missing
            .iter()
            .filter_map(|kind| texts.iter().find(|text| text.kind == *kind))
            .collect();
        let at = match (missing.is_empty(), plan.below, point) {
            (true, _, _) => None,
            (false, Some(below), _) => Some(below),
            (false, None, Some(point)) => Some(point),
            (false, None, None) if self.scripted_dispatch => {
                let corner = scene
                    .model_space_extents()
                    .map(|(_, max)| [f64::from(max.x), f64::from(max.y)]);
                Some(right_of_extents(corner, &missing, unit))
            }
            (false, None, None) => {
                // Da una persona: il punto si clicca e torna qui come riga.
                let ask = AbacoPointCommand;
                self.command_line.push_info(&ask.prompt());
                self.tabs[i].active_cmd = Some(Box::new(ask));
                return self.finish_dispatch(CMD);
            }
        };
        let mut created = at.map_or_else(Vec::new, |at| abaco_table::stack(&missing, at, unit));
        // Ruling Q: le tabelle nuove con lo stile tabella corrente del
        // disegno (nessuno se non si trova); le riscritte tengono il loro.
        let style = abaco_table::current_table_style(&self.tabs[i].scene.document);
        for entity in &mut created {
            if let EntityType::Table(table) = entity {
                table.table_style_handle = style;
            }
        }
        self.bim_write_abaco(i, &plan, created);

        if point.is_some() && (missing.is_empty() || plan.below.is_some()) {
            self.command_line.push_info(if missing.is_empty() {
                "AL_ABACO: il punto indicato non serve: nessuna tabella da creare."
            } else {
                "AL_ABACO: il punto indicato non serve: le tabelle mancanti vanno sotto l'ultima esistente."
            });
        }
        let changes = missing.len() + plan.replace.len() + plan.remove.len();
        self.command_line
            .push_info(&if changes == 0 && plan.locked == 0 {
                "AL_ABACO: le tabelle sono già aggiornate.".to_string()
            } else {
                format!(
                    "AL_ABACO: tabelle create: {}; sostituite: {}.",
                    missing.len(),
                    plan.replace.len()
                )
            });
        for (_, kind) in &plan.remove {
            self.command_line.push_info(&format!(
                "AL_ABACO: tolta la tabella {}: non ha più righe.",
                kind_name(*kind)
            ));
        }
        if plan.locked > 0 {
            self.command_line.push_info(&format!(
                "Tabelle d'abaco su layer bloccati non aggiornate: {}.",
                plan.locked
            ));
        }
        let rows =
            abaco.demolizioni.len() + abaco.varchi.len() + abaco.nuove.len() + abaco.locali.len();
        self.command_line.push_info(&format!(
            "AL_ABACO: righe: {rows}; incomplete: {}.",
            incomplete_rows(&abaco)
        ));
        self.bim_push_notices(&abaco.notices);
        Task::none()
    }

    /// `AL_ABACO_CSV [percorso]` (spec parte 2, §5.3): vedi le regole del
    /// modulo.
    pub(super) fn bim_abaco_csv(&mut self, i: usize, cmd: &str) -> Task<Message> {
        let line = cmd.trim();
        let rest = line
            .split_once(char::is_whitespace)
            .map_or("", |(_, rest)| rest);
        let target = unquote(rest.trim());
        if target.is_empty() && self.scripted_dispatch {
            // Nessuno risponderebbe alla finestra.
            self.bim_pending = None;
            self.command_line.push_error(
                "AL_ABACO_CSV: serve il percorso del file (AL_ABACO_CSV <percorso>). \
                 Niente è stato scritto.",
            );
            return Task::none();
        }
        let path = (!target.is_empty()).then(|| PathBuf::from(target));
        if let Some(error) = path.as_deref().and_then(csv_path_error) {
            self.bim_pending = None;
            self.command_line.push_error(&format!("{CSV_CMD}: {error}"));
            return Task::none();
        }
        let doc = &self.tabs[i].scene.document;
        if zones_in(doc).is_empty() && rooms_in(doc).is_empty() {
            // Un rilancio lasciato a metà (AL_UNITA annullato) non vale più.
            self.bim_pending = None;
            self.command_line
                .push_info("AL_ABACO_CSV: nessuna zona né locale nel disegno: nessun file.");
            return Task::none();
        }
        let Some(unit) = self.bim_unit_or_ask(i, line) else {
            return self.finish_dispatch(CSV_CMD);
        };
        self.bim_pending = None;
        let abaco = compute_abaco(&self.tabs[i].scene.document, &abaco_options(unit));
        let text = to_csv(&abaco);
        let summary = format!(
            "{CSV_CMD}: righe: {}; incomplete escluse: {}.",
            csv_row_count(&abaco),
            incomplete_rows(&abaco)
        );
        let Some(path) = path else {
            // Da una persona: righe e avvisi subito, poi la finestra, che
            // chiede lei se sovrascrivere un file esistente.
            self.command_line.push_info(&summary);
            self.bim_push_notices(&abaco.notices);
            let tab = &self.tabs[i];
            let file_name = format!(
                "{}_abaco.csv",
                base_name(tab.current_path.as_deref(), &tab.tab_title)
            );
            let start = tab
                .current_path
                .as_deref()
                .and_then(Path::parent)
                .filter(|dir| dir.is_dir())
                .map(Path::to_path_buf);
            return Task::perform(
                async move {
                    let mut dialog = crate::sys::file_dialog()
                        .set_title("Salva l'abaco CSV")
                        .set_file_name(file_name)
                        .add_filter("CSV (separato da punto e virgola)", &["csv"]);
                    if let Some(dir) = start {
                        dialog = dialog.set_directory(dir);
                    }
                    dialog
                        .save_file()
                        .await
                        .map(|file| crate::sys::handle_path(&file))
                },
                move |path| Message::Bim(BimMsg::AbacoCsvPath(path, text)),
            );
        };
        match write_csv(&path, &text, false) {
            Ok(()) => {
                self.command_line
                    .push_info(&format!("{CSV_CMD}: scritto {}.", path.display()));
                self.command_line.push_info(&summary);
                self.bim_push_notices(&abaco.notices);
            }
            Err(error) => self.command_line.push_error(&error.text(&path)),
        }
        Task::none()
    }

    /// La risposta della finestra «Salva con nome» di `AL_ABACO_CSV`: scrive
    /// `text` nel file scelto, che la finestra del sistema ha già chiesto se
    /// sovrascrivere; annullata, niente è scritto.
    pub(super) fn bim_abaco_csv_answer(
        &mut self,
        path: Option<PathBuf>,
        text: String,
    ) -> Task<Message> {
        let Some(path) = path else {
            self.command_line
                .push_info("AL_ABACO_CSV: annullato. Niente è stato scritto.");
            return Task::none();
        };
        match write_csv(&path, &text, true) {
            Ok(()) => self
                .command_line
                .push_info(&format!("{CSV_CMD}: scritto {}.", path.display())),
            Err(error) => self.command_line.push_error(&error.text(&path)),
        }
        Task::none()
    }

    /// Scrive il piano: il layer `AL-ABACO` e l'APPID in un passo a sé se si
    /// crea una tabella e mancano, poi sostituzioni, creazioni e rimozioni in
    /// un solo passo `AL_ABACO`, nessuno se nulla cambia.
    fn bim_write_abaco(&mut self, i: usize, plan: &AbacoPlan, created: Vec<EntityType>) {
        if !created.is_empty() && abaco_table::missing_layer(&self.tabs[i].scene.document) {
            self.push_undo_snapshot(i, "AL_LAYER");
            abaco_table::ensure_layer(&mut self.tabs[i].scene.document);
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
            self.refresh_layer_panel();
        }
        if created.is_empty() && plan.replace.is_empty() && plan.remove.is_empty() {
            return;
        }
        self.push_undo_snapshot(i, CMD);
        let scene = &mut self.tabs[i].scene;
        for (handle, kind, table) in &plan.replace {
            scene.update_entity(EntityType::Table(Box::new(table.clone())));
            // Una tabella letta da un DWG tiene l'XDATA anche nel blocco
            // grezzo, che al salvataggio prevale sul record: lo scarta.
            set_entity_xdata(
                &mut scene.document,
                *handle,
                APP_ABACO,
                Some(abaco_table::encode(*kind)),
            );
        }
        for entity in created {
            scene.add_entity(entity);
        }
        let removed: Vec<Handle> = plan.remove.iter().map(|(handle, _)| *handle).collect();
        if !removed.is_empty() {
            scene.erase_entities(&removed);
        }
        self.tabs[i].dirty = true;
        self.finish_pending_history(i);
        self.refresh_properties();
    }
}

/// Le opzioni dell'abaco nell'unità del disegno, con le tolleranze di
/// `AL_ESTRAI`: i locali a quella di `AL_LOCALE` e `AL_AGGIORNA` (R61).
fn abaco_options(unit: DrawingUnit) -> AbacoOptions {
    AbacoOptions {
        unit,
        tol: unit.tolerance(),
        room_tol: room_tolerance(unit),
    }
}

/// Le righe incomplete dell'abaco: una per [`Notice::RigaIncompleta`].
fn incomplete_rows(abaco: &Abaco) -> usize {
    abaco
        .notices
        .iter()
        .filter(|notice| matches!(notice, Notice::RigaIncompleta { .. }))
        .count()
}

/// Perché il CSV non si è scritto.
#[derive(Debug, PartialEq)]
enum CsvWriteError {
    /// Il file c'è già e senza la conferma della finestra non si riscrive.
    Exists,
    Io(String),
}

impl CsvWriteError {
    /// Il messaggio per la riga di comando: niente è stato scritto.
    fn text(&self, path: &Path) -> String {
        match self {
            CsvWriteError::Exists => format!("{CSV_CMD}: {}", exists_text(path)),
            CsvWriteError::Io(message) => format!(
                "{CSV_CMD}: scrittura di {} non riuscita: {message}. Niente è stato scritto.",
                path.display()
            ),
        }
    }
}

fn exists_text(path: &Path) -> String {
    format!(
        "il file {} esiste già: non si sovrascrive. Niente è stato scritto.",
        path.display()
    )
}

/// Perché `path`, dato in riga di comando, non va bene per il CSV, prima di
/// calcolare: una cartella, un file che c'è già (senza la finestra non si
/// sovrascrive mai), una cartella di destinazione che non c'è (non si crea).
fn csv_path_error(path: &Path) -> Option<String> {
    if path.is_dir() {
        return Some(format!(
            "{} è una cartella. Niente è stato scritto.",
            path.display()
        ));
    }
    if path.exists() {
        return Some(exists_text(path));
    }
    let dir = path.parent().filter(|dir| !dir.as_os_str().is_empty())?;
    (!dir.is_dir()).then(|| {
        format!(
            "la cartella {} non esiste. Niente è stato scritto.",
            dir.display()
        )
    })
}

/// Scrive `text` in `path` senza lasciare un file a metà: prima in un file
/// temporaneo nella stessa cartella (la rinomina resta sullo stesso disco),
/// poi lo rinomina sopra `path`; se qualcosa non va il temporaneo si toglie e
/// `path` resta com'era. Con `overwrite` falso un file esistente non si
/// tocca, anche se compare mentre si scrive; vero solo dopo la conferma della
/// finestra «Salva con nome» del sistema.
fn write_csv(path: &Path, text: &str, overwrite: bool) -> Result<(), CsvWriteError> {
    if !overwrite && path.exists() {
        return Err(CsvWriteError::Exists);
    }
    let temp = temp_path(path);
    let fail = |error: CsvWriteError| {
        let _ = std::fs::remove_file(&temp);
        Err(error)
    };
    if let Err(error) = std::fs::write(&temp, text) {
        return fail(CsvWriteError::Io(error.to_string()));
    }
    if !overwrite && path.exists() {
        return fail(CsvWriteError::Exists);
    }
    match std::fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => fail(CsvWriteError::Io(error.to_string())),
    }
}

/// Il file temporaneo accanto a `path`: stessa cartella, nome che comincia
/// con un punto e porta il numero del processo.
fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map_or_else(|| "abaco.csv".into(), |name| name.to_string_lossy());
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

/// Il punto delle tabelle di uno script senza punto: a destra dell'angolo in
/// alto a destra dell'estensione dello spazio modello (`corner`), staccate
/// quanto la più larga delle tabelle da creare. Un modello senza estensione
/// le mette nell'origine.
fn right_of_extents(
    corner: Option<[f64; 2]>,
    missing: &[&TableText],
    unit: DrawingUnit,
) -> [f64; 2] {
    let Some([x, y]) = corner else {
        return [0.0, 0.0];
    };
    let width = abaco_table::stack(missing, [0.0, 0.0], unit)
        .iter()
        .filter_map(|entity| match entity {
            EntityType::Table(table) => Some(table.total_width()),
            _ => None,
        })
        .fold(0.0, f64::max);
    [x + width, y]
}

/// Il punto di `AL_ABACO <x,y>` (anche `x,y,z`: la quota non conta), col
/// punto decimale; `None` senza argomento. Ciò che non si capisce è un
/// errore, mai un punto inventato.
fn point_argument(cmd: &str) -> Result<Option<[f64; 2]>, String> {
    let mut words = cmd.split_whitespace();
    let _name = words.next();
    let Some(text) = words.next() else {
        return Ok(None);
    };
    if let Some(extra) = words.next() {
        return Err(format!(
            "AL_ABACO: argomento in più \"{extra}\" dopo il punto."
        ));
    }
    let numbers: Option<Vec<f64>> = text
        .split(',')
        .map(|part| part.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
        .collect();
    match numbers.as_deref() {
        Some(&[x, y]) | Some(&[x, y, _]) => Ok(Some([x, y])),
        _ => Err(format!(
            "AL_ABACO: punto non valido \"{text}\": serve x,y col punto decimale, per esempio 20.5,0."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{csv_path_error, point_argument, write_csv, CsvWriteError};
    use std::path::{Path, PathBuf};

    /// Una cartella nuova sotto la cartella temporanea, tolta a fine test.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir().join(format!(
                "archline_abaco_csv_{tag}_{}_{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Il CSV si scrive senza lasciare il file temporaneo; un file che c'è già
    /// resta com'è senza la conferma e si riscrive con quella; in una
    /// cartella che non c'è non si scrive nulla, né lì né altrove.
    #[test]
    fn write_csv_never_leaves_a_half_file() {
        let scratch = Scratch::new("scrittura");
        let path = scratch.0.join("abaco.csv");
        assert_eq!(write_csv(&path, "uno\r\n", false), Ok(()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "uno\r\n");
        assert_eq!(entries(&scratch.0), ["abaco.csv"]);

        assert_eq!(
            write_csv(&path, "due\r\n", false),
            Err(CsvWriteError::Exists)
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "uno\r\n");
        assert_eq!(write_csv(&path, "tre\r\n", true), Ok(()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "tre\r\n");
        assert_eq!(entries(&scratch.0), ["abaco.csv"]);

        let missing = scratch.0.join("manca").join("abaco.csv");
        assert!(matches!(
            write_csv(&missing, "quattro\r\n", true),
            Err(CsvWriteError::Io(_))
        ));
        assert_eq!(entries(&scratch.0), ["abaco.csv"]);
    }

    #[test]
    fn csv_path_error_refuses_folders_existing_files_and_missing_folders() {
        let scratch = Scratch::new("percorso");
        let file = scratch.0.join("abaco.csv");
        assert_eq!(csv_path_error(&file), None);
        assert_eq!(csv_path_error(Path::new("abaco.csv")), None, "relativo");
        std::fs::write(&file, "x").unwrap();
        assert!(csv_path_error(&file).is_some_and(|e| e.contains("esiste già")));
        assert!(csv_path_error(&scratch.0).is_some_and(|e| e.contains("è una cartella")));
        let missing = scratch.0.join("manca").join("abaco.csv");
        assert!(csv_path_error(&missing).is_some_and(|e| e.contains("non esiste")));
    }

    #[test]
    fn point_argument_takes_x_y_with_the_decimal_point() {
        assert_eq!(point_argument("AL_ABACO"), Ok(None));
        assert_eq!(point_argument("AL_ABACO  "), Ok(None));
        assert_eq!(point_argument("AL_ABACO 20,0"), Ok(Some([20.0, 0.0])));
        assert_eq!(
            point_argument("al_abaco 20.5,-3.25"),
            Ok(Some([20.5, -3.25]))
        );
        assert_eq!(point_argument("AL_ABACO 1,2,3"), Ok(Some([1.0, 2.0])));
        for bad in [
            "AL_ABACO abc",
            "AL_ABACO 20",
            "AL_ABACO 20,",
            "AL_ABACO 1,2,3,4",
            "AL_ABACO NaN,0",
            "AL_ABACO inf,0",
            "AL_ABACO 20,0 30,0",
        ] {
            let error = point_argument(bad).unwrap_err();
            assert!(error.starts_with("AL_ABACO:"), "{bad}: {error}");
        }
    }
}
