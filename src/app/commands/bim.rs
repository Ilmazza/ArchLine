//! Famiglia dei comandi BIM di ArchLine (spec §3, §4.2, §5.1, §5.2 e §5.5):
//! `AL_DEMOLISCI`, `AL_NUOVO`, `AL_STATO`, `AL_LOCALE`, `AL_AGGIORNA`,
//! `AL_ESTRAI` e l'interno `AL_UNITA`.
//!
//! Qui sta solo il lato app: l'unità del disegno (chiesta quando `INSUNITS`
//! vale 0, mai scritta in `INSUNITS`), i layer `AL-*` creati la prima volta in
//! un passo di annullamento a sé, l'istantanea della geometria, l'avvio di
//! [`ZoneCommand`] e di [`RoomCommand`], il cambio di stato delle zone
//! esistenti (`AL_STATO`), il ricalcolo dei locali di fatto (`AL_AGGIORNA`) e
//! l'estrazione degli elaborati con la sua finestra (`AL_ESTRAI`). La logica
//! sta in `crate::bim`.

use crate::app::{Message, ModalKind, OpenCADStudio};
use crate::bim::commands::{ZoneCommand, ZoneSnapshot};
use crate::bim::extract::{self, Elaborato, ExtractOptions, WriteError};
use crate::bim::record;
use crate::bim::refresh::{has_fatto_rooms, refresh_rooms, RoomEdit};
use crate::bim::report::Notice;
use crate::bim::room_command::{
    room_snapshot, room_text_height, RoomCommand, TextFallback, DEFAULT_ROOM_TEXT_M,
};
use crate::bim::units::{self, format_decimal, DrawingUnit, UnitLookup};
use crate::bim::walls;
use crate::bim::zone::{self, Zone, ZoneGeom, ZoneState};
use crate::bim::APP_ZONA;
use crate::command::{
    CadCommand, FreeTextValuePromptCommand, KeywordCommand, SelectThenKeywordCommand,
};
use crate::scene::view::dispatch::set_entity_xdata;
use crate::scene::Scene;
use crate::ui::window::bim_extract::{BimMsg, ExtractDialog};
use codec::{Document, Handle};
use iced::Task;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "AL_DEMOLISCI",
        "AL_NUOVO",
        "AL_STATO",
        "AL_LOCALE",
        "AL_AGGIORNA",
        "AL_ESTRAI",
        "AL_UNITA",
    ]
});

const UNIT_PROMPT: &str =
    "Unità del disegno non specificata (INSUNITS = 0). [Millimetri/Centimetri/Metri]:";

const STATO_PROMPT: &str = "AL_STATO  [Demolito/Nuovo/Varco/Altezza/Spessore]:";

const ESTRAI_PROMPT: &str = "Cartella di destinazione:";

/// Gli elaborati di `AL_ESTRAI <cartella>`: tutti e tre.
const ALL_ELABORATI: [Elaborato; 3] = [
    Elaborato::StatoDiFatto,
    Elaborato::DemolizioniCostruzioni,
    Elaborato::StatoDiProgetto,
];

/// Il nome di base dei file quando il disegno non ne dà uno utilizzabile.
const FALLBACK_BASE: &str = "Disegno";

impl OpenCADStudio {
    /// I comandi `AL_*`, riconosciuti dal primo token in maiuscolo.
    pub(super) fn dispatch_bim(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let mut words = cmd.split_whitespace();
        let name = words.next()?.to_ascii_uppercase();
        match name.as_str() {
            "AL_DEMOLISCI" => Some(self.bim_start_zone(i, "AL_DEMOLISCI", ZoneState::Demolito)),
            "AL_NUOVO" => Some(self.bim_start_zone(i, "AL_NUOVO", ZoneState::Nuovo)),
            "AL_UNITA" => Some(self.bim_set_unit(i, words.next())),
            "AL_STATO" => Some(match words.next() {
                None => self.bim_start_stato(i),
                Some(keyword) => {
                    let args: Vec<&str> = words.collect();
                    self.bim_apply_stato(i, keyword, &args)
                }
            }),
            "AL_LOCALE" => Some(self.bim_start_room(i)),
            "AL_AGGIORNA" => Some(self.bim_update_rooms(i)),
            "AL_ESTRAI" => Some(self.bim_start_extract(i, cmd)),
            _ => None,
        }
    }

    /// I comandi `AL_*` lavorano nello spazio modello, dove stanno muri e zone.
    /// Altrove (spazio carta, editor di blocchi) lo dicono e non fanno nulla:
    /// né domanda dell'unità, né layer, né passi di annullamento. Va chiamato
    /// per primo.
    fn bim_in_model_space(&mut self, i: usize, cmd: &str) -> bool {
        if commits_to_model_space(&self.tabs[i].scene) {
            return true;
        }
        self.command_line.push_error(&format!(
            "{cmd}: i comandi di ristrutturazione lavorano solo nello spazio modello."
        ));
        false
    }

    /// L'unità del disegno di `i`. Se manca la chiede (`AL_UNITA`) e ricorda
    /// `cmd` da rilanciare dopo la risposta; se non è supportata lo dice. In
    /// entrambi i casi `None`: il comando non parte.
    fn bim_unit_or_ask(&mut self, i: usize, cmd: &str) -> Option<DrawingUnit> {
        match units::resolve(&self.tabs[i].scene.document) {
            UnitLookup::Known(unit) => Some(unit),
            UnitLookup::Unsupported(code) => {
                self.command_line.push_error(&format!(
                    "Unità del disegno non supportata (INSUNITS = {code}): usa millimetri, centimetri o metri."
                ));
                None
            }
            UnitLookup::Unspecified => {
                self.bim_pending = Some(cmd.to_string());
                self.bim_ask_unit(i);
                None
            }
        }
    }

    /// `AL_DEMOLISCI` / `AL_NUOVO`: unità, layer (la prima volta), istantanea,
    /// poi [`ZoneCommand`].
    fn bim_start_zone(&mut self, i: usize, cmd: &str, state: ZoneState) -> Task<Message> {
        if !self.bim_in_model_space(i, cmd) {
            return Task::none();
        }
        let Some(unit) = self.bim_unit_or_ask(i, cmd) else {
            return self.finish_dispatch(cmd);
        };
        // Un rilancio lasciato a metà (AL_UNITA annullato) non vale più.
        self.bim_pending = None;
        self.bim_ensure_layers(i);
        let (snap, off_plane) = zone_snapshot(&self.tabs[i].scene.document, unit);
        if off_plane > 0 {
            self.command_line
                .push_info(&Notice::FuoriPiano { count: off_plane }.text());
        }
        let command = ZoneCommand::new(state, snap);
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.finish_dispatch(cmd)
    }

    /// I layer `AL-*` e gli APPID `ARCHLINE_*`, se ne manca uno, in un passo di
    /// annullamento a sé: un annullamento dopo il primo comando toglie ciò che
    /// il comando ha creato, non i layer; e nessun layer nasce fuori da un
    /// passo. Va chiamato prima di consegnare entità con l'XDATA dei nostri
    /// APPID.
    fn bim_ensure_layers(&mut self, i: usize) {
        if zone::missing_layers(&self.tabs[i].scene.document) {
            self.push_undo_snapshot(i, "AL_LAYER");
            zone::ensure_layers(&mut self.tabs[i].scene.document);
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
            self.refresh_layer_panel();
        }
    }

    /// `AL_LOCALE` (spec §5.2): unità, layer (la prima volta), istantanea dei
    /// muri di fatto e di progetto alla tolleranza dei locali, poi
    /// [`RoomCommand`]. Senza `TEXTSIZE` i testi hanno un'altezza di ripiego, e
    /// lo si dice.
    fn bim_start_room(&mut self, i: usize) -> Task<Message> {
        const CMD: &str = "AL_LOCALE";
        if !self.bim_in_model_space(i, CMD) {
            return Task::none();
        }
        let Some(unit) = self.bim_unit_or_ask(i, CMD) else {
            return self.finish_dispatch(CMD);
        };
        self.bim_pending = None;
        // Il contorno e il testo portano l'XDATA `ARCHLINE_LOCALE`.
        self.bim_ensure_layers(i);
        let doc = &self.tabs[i].scene.document;
        let (snap, off_plane) = room_snapshot(doc, unit, room_tolerance(unit));
        let (_, fallback_height) = room_text_height(doc, unit);
        let textsize = doc.header.text_height;
        if off_plane > 0 {
            self.command_line
                .push_info(&Notice::FuoriPiano { count: off_plane }.text());
        }
        match fallback_height {
            Some(TextFallback::NotPositive) => self.command_line.push_info(&format!(
                "AL_LOCALE: TEXTSIZE del disegno non è positiva: i testi dei locali sono alti {} m.",
                format_decimal(DEFAULT_ROOM_TEXT_M)
            )),
            Some(TextFallback::Implausible) => self.command_line.push_info(&format!(
                "AL_LOCALE: TEXTSIZE del disegno ({}) è implausibile (oltre 1 m): i testi dei \
                 locali sono alti {} m.",
                format_decimal(textsize),
                format_decimal(DEFAULT_ROOM_TEXT_M)
            )),
            None => {}
        }
        let command = RoomCommand::new(snap);
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.finish_dispatch(CMD)
    }

    /// `AL_AGGIORNA` (spec §4.2): ogni locale di fatto si rimisura sul rilievo
    /// attuale ([`refresh_rooms`]); contorno e testo cambiano sul posto, con lo
    /// stesso handle, in un solo passo di annullamento e solo se qualcosa
    /// cambia. Un locale su un layer bloccato non si tocca e si conta. Una riga
    /// per ogni locale cambiato, con l'area di prima e quella nuova (R58), poi
    /// il riepilogo e gli avvisi. Senza locali di fatto lo dice e basta, senza
    /// chiedere l'unità.
    fn bim_update_rooms(&mut self, i: usize) -> Task<Message> {
        const CMD: &str = "AL_AGGIORNA";
        if !self.bim_in_model_space(i, CMD) {
            return Task::none();
        }
        if !has_fatto_rooms(&self.tabs[i].scene.document) {
            // Un rilancio lasciato a metà (AL_UNITA annullato) non vale più.
            self.bim_pending = None;
            self.command_line
                .push_info("AL_AGGIORNA: nessun locale di fatto nel disegno.");
            return Task::none();
        }
        let Some(unit) = self.bim_unit_or_ask(i, CMD) else {
            return self.finish_dispatch(CMD);
        };
        self.bim_pending = None;
        let refresh = refresh_rooms(&self.tabs[i].scene.document, unit, room_tolerance(unit));
        let scene = &self.tabs[i].scene;
        let (locked, free): (Vec<_>, Vec<_>) = refresh.changed.into_iter().partition(|change| {
            change
                .edits
                .iter()
                .any(|(handle, _)| scene.is_layer_locked(*handle))
        });
        let updated = free.len();
        let lines: Vec<Notice> = free.iter().map(|change| change.notice()).collect();
        let edits: HashMap<Handle, RoomEdit> =
            free.into_iter().flat_map(|change| change.edits).collect();
        if !edits.is_empty() {
            let mut handles: Vec<Handle> = edits.keys().copied().collect();
            handles.sort_by_key(|handle| handle.value());
            self.apply_property_op(i, CMD, &handles, |app, handle| {
                if let (Some(edit), Some(entity)) = (
                    edits.get(&handle),
                    app.tabs[i].scene.document.get_entity_mut(handle),
                ) {
                    edit.apply_to(entity);
                }
            });
            self.finish_pending_history(i);
        }
        self.bim_push_notices(&lines);
        self.command_line
            .push_info(&format!("{CMD}: {updated} locali aggiornati."));
        if !locked.is_empty() {
            self.command_line.push_info(&format!(
                "Locali su layer bloccati ignorati: {}.",
                locked.len()
            ));
        }
        self.bim_push_notices(&refresh.notices);
        Task::none()
    }

    /// `AL_UNITA MM|CM|M`: l'unità va nel record del disegno (`INSUNITS` resta
    /// com'è), poi riparte il comando che l'aveva chiesta. Senza una parola
    /// chiave valida si chiede.
    fn bim_set_unit(&mut self, i: usize, keyword: Option<&str>) -> Task<Message> {
        let Some(unit) = keyword.and_then(DrawingUnit::from_keyword) else {
            // Digitato a mano: si chiede l'unità e basta, senza rilanci.
            self.bim_pending = None;
            self.bim_ask_unit(i);
            return self.finish_dispatch("AL_UNITA");
        };
        self.push_undo_snapshot(i, "AL_UNITA");
        let doc = &mut self.tabs[i].scene.document;
        let mut rec = record::read(doc);
        rec.unit = Some(unit);
        record::write(doc, &rec);
        let saved = record::read(doc).unit == Some(unit);
        let pending = self.bim_pending.take();
        if !saved {
            // Senza dizionario radice il record non si scrive: nessun passo di
            // annullamento vuoto e il disegno non risulta modificato (come
            // `bim_remember_new_walls_layer`). Rilanciare chiederebbe di
            // nuovo, all'infinito.
            self.drop_unchanged_pending_history(i);
            self.command_line
                .push_error("AL_UNITA: impossibile salvare l'unità nel disegno.");
            return Task::none();
        }
        self.tabs[i].dirty = true;
        self.finish_pending_history(i);
        match pending {
            Some(cmd) => self.dispatch_command(&cmd),
            None => Task::none(),
        }
    }

    /// Avvia la domanda dell'unità (`AL_UNITA`), che risponde con `AL_UNITA <kw>`.
    fn bim_ask_unit(&mut self, i: usize) {
        let ask = KeywordCommand::new(
            "AL_UNITA",
            UNIT_PROMPT,
            vec![
                ("Millimetri", "MM", None),
                ("Centimetri", "CM", None),
                ("Metri", "M", None),
            ],
        );
        self.command_line.push_info(&ask.prompt());
        self.tabs[i].active_cmd = Some(Box::new(ask));
    }

    /// `AL_STATO` da solo: con una selezione chiede subito la parola chiave,
    /// senza selezione raccoglie prima gli oggetti. La risposta torna qui come
    /// `AL_STATO <parola> [valore]` ([`Self::bim_apply_stato`]).
    fn bim_start_stato(&mut self, i: usize) -> Task<Message> {
        if !self.bim_in_model_space(i, "AL_STATO") {
            return Task::none();
        }
        let has_selection = !self.tabs[i].scene.selected_entities().is_empty();
        let command = SelectThenKeywordCommand::new(
            "AL_STATO",
            STATO_PROMPT,
            vec![
                ("Demolito", "D", None),
                ("Nuovo", "N", None),
                ("Varco", "V", None),
                ("Altezza", "A", Some("Altezza:")),
                ("Spessore", "S", Some("Spessore:")),
            ],
            has_selection,
        );
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.finish_dispatch("AL_STATO")
    }

    /// `AL_STATO <parola> [valore]` sulle zone selezionate. Un solo passo di
    /// annullamento, e solo se almeno una zona cambia davvero; il rilievo e gli
    /// oggetti che non sono zone non si toccano mai.
    fn bim_apply_stato(&mut self, i: usize, keyword: &str, args: &[&str]) -> Task<Message> {
        if !self.bim_in_model_space(i, "AL_STATO") {
            return Task::none();
        }
        let change = match StatoChange::parse(keyword, args) {
            Ok(change) => change,
            Err(message) => {
                self.command_line.push_error(&message);
                return Task::none();
            }
        };
        let plan = stato_plan(&self.tabs[i].scene, change);
        if plan.selected == 0 {
            self.command_line
                .push_error("AL_STATO: nessun oggetto selezionato.");
            return Task::none();
        }
        let modified = plan.changes.len();
        if modified > 0 {
            let handles: Vec<Handle> = plan.changes.iter().map(|(handle, _)| *handle).collect();
            let new_zones: HashMap<Handle, Zone> = plan.changes.into_iter().collect();
            let mut layers_created = false;
            self.apply_property_op(i, "AL_STATO", &handles, |app, handle| {
                let Some(new) = new_zones.get(&handle) else {
                    return;
                };
                let doc = &mut app.tabs[i].scene.document;
                // Dopo un PURGE l'altro layer `AL-*` può non esserci più: si
                // ricrea qui, nello stesso passo di annullamento della zona.
                if zone::missing_layers(doc) {
                    zone::ensure_layers(doc);
                    layers_created = true;
                }
                if let Some(entity) = doc.get_entity_mut(handle) {
                    zone::restyle(entity, new);
                }
                // Una zona letta da un DWG tiene l'XDATA anche nel blocco
                // grezzo, che al salvataggio prevale sul record: lo scarta.
                set_entity_xdata(doc, handle, APP_ZONA, Some(zone::encode(new)));
            });
            if layers_created {
                self.refresh_layer_panel();
            }
        }
        if plan.varco_on_new {
            self.command_line
                .push_info("Il varco vale solo per le zone demolite.");
        }
        if plan.not_zones > 0 {
            self.command_line.push_info(&format!(
                "{} oggetti non sono zone e sono stati ignorati.",
                plan.not_zones
            ));
        }
        if plan.locked > 0 {
            self.command_line.push_info(&format!(
                "Zone su layer bloccati ignorate: {}.",
                plan.locked
            ));
        }
        self.command_line
            .push_info(&format!("AL_STATO: {modified} zone modificate."));
        Task::none()
    }

    /// `AL_ESTRAI [cartella]` (spec §5.5). La cartella è il resto della riga,
    /// spazi compresi, anche tra virgolette: si estraggono subito i tre
    /// elaborati e non si sovrascrive mai (con file esistenti non si scrive
    /// nulla). Senza cartella la si chiede: in riga di comando a uno script
    /// (`scripted_dispatch`), con la finestra a una persona. Prima serve
    /// l'unità del disegno, come per le zone. Estrarre non cambia il disegno.
    fn bim_start_extract(&mut self, i: usize, cmd: &str) -> Task<Message> {
        let line = cmd.trim_start();
        let (name, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let folder = unquote(rest.trim());
        if folder.is_empty() {
            if line.len() > name.len() {
                // `AL_ESTRAI ` con lo spazio e niente dopo: l'Invio a vuoto alla
                // domanda della cartella.
                self.command_line
                    .push_error("AL_ESTRAI: nessuna cartella indicata. Niente è stato scritto.");
                return Task::none();
            }
            return self.bim_ask_extract(i);
        }
        let dir = PathBuf::from(folder);
        if let Some(error) = folder_error(&dir) {
            self.command_line.push_error(&format!("AL_ESTRAI: {error}"));
            return Task::none();
        }
        if self.bim_unit_or_ask(i, line.trim_end()).is_none() {
            return self.finish_dispatch("AL_ESTRAI");
        }
        self.bim_pending = None;
        // Nessuno sceglie: vale il layer ricordato o proposto, che non si
        // ricorda (estrarre da script non cambia il disegno); senza nessuno dei
        // due, il layer 0, e lo si dice.
        let layer = proposed_new_walls_layer(&self.tabs[i].scene.document).unwrap_or_else(|| {
            self.command_line.push_info(
                "AL_ESTRAI: nessun layer dei muri nuovi ricordato e nessun muro nel disegno: \
                 i muri nuovi vanno sul layer 0.",
            );
            "0".to_string()
        });
        let result = self.bim_run_extract(i, &dir, ALL_ELABORATI.to_vec(), layer, false);
        if let Err(WriteError::Exists(paths)) = result {
            self.command_line.push_error(&exists_text(&paths));
        }
        Task::none()
    }

    /// `AL_ESTRAI` senza cartella: l'unità, poi la domanda (script) o la
    /// finestra, con la cartella del disegno, le tre caselle e il layer dei
    /// muri nuovi ricordato nel record.
    fn bim_ask_extract(&mut self, i: usize) -> Task<Message> {
        if self.bim_unit_or_ask(i, "AL_ESTRAI").is_none() {
            return self.finish_dispatch("AL_ESTRAI");
        }
        self.bim_pending = None;
        if self.scripted_dispatch {
            // Nessuno risponderebbe alla finestra: la cartella si chiede in
            // riga di comando e torna qui come `AL_ESTRAI <cartella>`. Testo
            // libero: gli spazi e le maiuscole di un percorso restano (in un
            // file .scr la riga intera è la cartella).
            let ask = FreeTextValuePromptCommand::new("AL_ESTRAI", ESTRAI_PROMPT);
            self.command_line.push_info(&ask.prompt());
            self.tabs[i].active_cmd = Some(Box::new(ask));
            return self.finish_dispatch("AL_ESTRAI");
        }
        let tab = &self.tabs[i];
        let doc = &tab.scene.document;
        let mut layers: Vec<String> = doc
            .layers
            .iter()
            .map(|layer| layer.name.clone())
            .filter(|name| !walls::is_bim_layer(name))
            .collect();
        layers.sort_by_key(|name| name.to_lowercase());
        let dialog = ExtractDialog {
            tab_id: tab.id,
            dir: tab
                .current_path
                .as_deref()
                .and_then(Path::parent)
                .filter(|dir| dir.is_dir())
                .map(Path::to_path_buf),
            fatto: true,
            comparativa: true,
            progetto: true,
            layer: proposed_new_walls_layer(doc).unwrap_or_default(),
            layers,
            files_exist: false,
            overwrite: false,
        };
        self.bim_extract = Some(dialog);
        self.active_modal = Some(ModalKind::BimExtract);
        self.reset_modal_geometry();
        Task::none()
    }

    /// I messaggi della finestra di `AL_ESTRAI`. A finestra chiusa (una
    /// cartella scelta che arriva dopo Annulla) non fanno nulla.
    pub(in crate::app) fn on_bim(&mut self, m: BimMsg) -> Task<Message> {
        let Some(dialog) = self.bim_extract.as_mut() else {
            return Task::none();
        };
        match m {
            #[cfg(not(target_arch = "wasm32"))]
            BimMsg::Browse => {
                let start = dialog.dir.clone();
                return Task::perform(
                    async move {
                        let mut picker =
                            crate::sys::file_dialog().set_title("Cartella di destinazione");
                        if let Some(dir) = start {
                            picker = picker.set_directory(dir);
                        }
                        picker
                            .pick_folder()
                            .await
                            .map(|handle| crate::sys::handle_path(&handle))
                    },
                    |dir| Message::Bim(BimMsg::Picked(dir)),
                );
            }
            // Il browser non sceglie cartelle.
            #[cfg(target_arch = "wasm32")]
            BimMsg::Browse => {}
            BimMsg::Picked(Some(dir)) => dialog.set_dir(dir),
            BimMsg::Picked(None) => {}
            BimMsg::Toggle(elaborato) => dialog.toggle(elaborato),
            BimMsg::Layer(name) => dialog.set_layer(name),
            BimMsg::Overwrite(on) => dialog.set_overwrite(on),
            BimMsg::Ok => return self.bim_extract_ok(),
            BimMsg::Cancel => self.bim_close_extract(),
        }
        Task::none()
    }

    /// OK (o «Sovrascrivi») della finestra. Se i file esistono e la casella
    /// «Sovrascrivi i file esistenti» non è accesa, la finestra lo dice, mostra
    /// la casella spenta e resta aperta: niente è stato scritto, e un altro OK
    /// (doppio clic) non fa nulla finché la casella è spenta. Lo stesso, con un
    /// errore in riga di comando, se manca il layer dei muri nuovi per lo stato
    /// di progetto. Altrimenti estrae (sovrascrivendo solo con la casella
    /// accesa), ricorda il layer dei muri nuovi se ha scritto lo stato di
    /// progetto, si chiude e il rapporto va in riga di comando.
    fn bim_extract_ok(&mut self) -> Task<Message> {
        let Some(dialog) = self.bim_extract.as_ref() else {
            return Task::none();
        };
        // `can_ok` è falso anche dopo l'avviso con la casella spenta.
        let Some(dir) = dialog.dir.clone().filter(|_| dialog.can_ok()) else {
            return Task::none();
        };
        let tab_id = dialog.tab_id;
        let elaborati = dialog.elaborati();
        let layer = dialog.layer.trim().to_string();
        let overwrite = dialog.overwrite_confirmed();
        let Some(i) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            self.bim_close_extract();
            self.command_line
                .push_error("AL_ESTRAI: il disegno non è più aperto. Niente è stato scritto.");
            return Task::none();
        };
        if let Some(error) = new_walls_layer_error(&self.tabs[i].scene.document, &elaborati, &layer)
        {
            // La finestra resta aperta: si corregge il nome e si riprova.
            self.command_line.push_error(&error);
            return Task::none();
        }
        let progetto = elaborati.contains(&Elaborato::StatoDiProgetto);
        match self.bim_run_extract(i, &dir, elaborati, layer.clone(), overwrite) {
            Ok(_) => {
                if progetto {
                    self.bim_remember_new_walls_layer(i, &layer);
                }
                self.bim_close_extract();
            }
            Err(WriteError::Exists(_)) if !overwrite => {
                if let Some(dialog) = self.bim_extract.as_mut() {
                    dialog.found_existing();
                }
            }
            // Con la conferma `write` non lo restituisce; se capitasse, lo si dice.
            Err(WriteError::Exists(paths)) => {
                self.bim_close_extract();
                self.command_line.push_error(&exists_text(&paths));
            }
            // Il resto è già nella riga di comando.
            _ => self.bim_close_extract(),
        }
        Task::none()
    }

    /// Chiude la finestra di `AL_ESTRAI` (Annulla, o dopo l'estrazione).
    fn bim_close_extract(&mut self) {
        if self.active_modal == Some(ModalKind::BimExtract) {
            self.close_active_modal();
        }
        self.bim_extract = None;
    }

    /// Estrae `elaborati` dal disegno `i` e li scrive in `dir` con il nome del
    /// disegno; i muri nuovi dello stato di progetto vanno su `layer` (senza
    /// spazi attorno; vuoto o non valido, se c'è il progetto, è un errore:
    /// [`new_walls_layer_error`]). Con `overwrite` falso un
    /// file esistente ferma tutto prima di scrivere (`Exists`). Dice tutto in
    /// riga di comando tranne `Exists`, che il chiamante tratta a modo suo
    /// (rifiuto o conferma):
    /// - riuscita: «scritti n file», il layer dei muri nuovi da creare se il
    ///   disegno non lo ha, e una riga per avviso;
    /// - errore prima del primo file: la frase, «Niente è stato scritto.»;
    /// - errore dopo (`Partial`): la frase con i file già scritti, poi gli
    ///   avvisi, che valgono per quei file rimasti sul disco.
    ///
    /// Il layer non si ricorda qui: lo fa la finestra, dove lo si sceglie.
    fn bim_run_extract(
        &mut self,
        i: usize,
        dir: &Path,
        elaborati: Vec<Elaborato>,
        layer: String,
        overwrite: bool,
    ) -> Result<Vec<PathBuf>, WriteError> {
        if let Some(error) = folder_error(dir) {
            self.command_line.push_error(&format!("AL_ESTRAI: {error}"));
            return Err(WriteError::Io(error));
        }
        let layer = layer.trim().to_string();
        if let Some(error) = new_walls_layer_error(&self.tabs[i].scene.document, &elaborati, &layer)
        {
            self.command_line.push_error(&error);
            return Err(WriteError::Io(error));
        }
        let unit = match units::resolve(&self.tabs[i].scene.document) {
            UnitLookup::Known(unit) => unit,
            // Chi chiama ha già chiesto l'unità: qui non si inventa.
            UnitLookup::Unsupported(_) | UnitLookup::Unspecified => {
                let error = "unità del disegno non disponibile. Niente è stato scritto.";
                self.command_line.push_error(&format!("AL_ESTRAI: {error}"));
                return Err(WriteError::Io(error.into()));
            }
        };
        let progetto = elaborati.contains(&Elaborato::StatoDiProgetto);
        let opts = ExtractOptions {
            elaborati,
            new_walls_layer: layer.clone(),
            unit,
            tol: unit.tolerance(),
            // R61: i locali dell'estratto alla tolleranza di `AL_LOCALE` e
            // `AL_AGGIORNA`, cioè con `HPGAPTOL`.
            room_tol: room_tolerance(unit),
        };
        let extracted = extract::extract(&self.tabs[i].scene.document, &opts);
        let tab = &self.tabs[i];
        let base = base_name(tab.current_path.as_deref(), &tab.tab_title);
        // Un layer dei muri nuovi che il disegno non ha: con che cosa nasce nel
        // progetto (quelli del layer più frequente dei muri, o i predefiniti).
        let doc = &tab.scene.document;
        let new_layer = (progetto && !doc.layers.contains(&layer)).then(|| {
            match extract::new_walls_layer_template(doc, &layer) {
                Some(walls) => format!("colore, tipo di linea e spessore del layer {}", walls.name),
                None => "colore e tipo di linea predefiniti".to_string(),
            }
        });
        let result = extract::write(&extracted, dir, &base, overwrite);
        match &result {
            Ok(written) => {
                self.command_line.push_info(&format!(
                    "AL_ESTRAI: scritti {} file in {}.",
                    written.len(),
                    dir.display()
                ));
                if let Some(settings) = new_layer {
                    self.command_line.push_info(&format!(
                        "AL_ESTRAI: il layer {layer} dei muri nuovi non c'è nel disegno: \
                         creato nello stato di progetto con {settings}."
                    ));
                }
                self.bim_push_notices(&extracted.notices);
            }
            Err(WriteError::Exists(_)) => {}
            Err(WriteError::Io(message)) => {
                self.command_line.push_error(&format!(
                    "AL_ESTRAI: scrittura non riuscita: {message}. Niente è stato scritto."
                ));
            }
            Err(WriteError::Partial { written, message }) => {
                self.command_line
                    .push_error(&partial_failure_text(written, message));
                self.bim_push_notices(&extracted.notices);
            }
        }
        result
    }

    /// Una riga per avviso (dell'estrazione, di `AL_AGGIORNA`).
    fn bim_push_notices(&mut self, notices: &[Notice]) {
        for notice in notices {
            self.command_line.push_info(&notice.text());
        }
    }

    /// Il layer dei muri nuovi va nel record del disegno, in un passo di
    /// annullamento a sé, solo se è cambiato. Vuoto = nessuno.
    fn bim_remember_new_walls_layer(&mut self, i: usize, layer: &str) {
        let mut rec = record::read(&self.tabs[i].scene.document);
        let chosen = Some(layer.to_string()).filter(|name| !name.is_empty());
        if rec.new_walls_layer == chosen {
            return;
        }
        self.push_undo_snapshot(i, "AL_ESTRAI");
        rec.new_walls_layer = chosen.clone();
        let doc = &mut self.tabs[i].scene.document;
        record::write(doc, &rec);
        if record::read(doc).new_walls_layer == chosen {
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
        } else {
            // Senza dizionario radice il record non si scrive: nessun passo vuoto.
            self.drop_unchanged_pending_history(i);
            self.command_line.push_error(
                "AL_ESTRAI: impossibile ricordare il layer dei muri nuovi nel disegno.",
            );
        }
    }
}

/// Il rifiuto di `AL_ESTRAI` quando i file esistono già.
fn exists_text(paths: &[PathBuf]) -> String {
    format!(
        "AL_ESTRAI: i file esistono già: {}. Niente è stato scritto.",
        file_names(paths)
    )
}

/// L'errore di una scrittura rimasta a metà: quale file non si è scritto e
/// perché (`message`), e quali sono già stati scritti e restano sul disco.
fn partial_failure_text(written: &[PathBuf], message: &str) -> String {
    format!(
        "AL_ESTRAI: scrittura non riuscita: {message} (già scritti prima dell'errore: {}).",
        file_names(written)
    )
}

/// Perché `dir` non va bene come cartella di destinazione, se non va bene.
/// Una cartella che non c'è non si crea: lo si dice e basta.
fn folder_error(dir: &Path) -> Option<String> {
    if dir.is_dir() {
        None
    } else if dir.exists() {
        Some(format!(
            "{} non è una cartella. Niente è stato scritto.",
            dir.display()
        ))
    } else {
        Some(format!(
            "la cartella {} non esiste. Niente è stato scritto.",
            dir.display()
        ))
    }
}

/// Il testo senza le virgolette doppie attorno, se ci sono: un percorso con
/// spazi scritto come in Windows, `"C:\Progetti\Casa Rossi"`.
fn unquote(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(text)
        .trim()
}

/// Il layer dei muri nuovi da proporre (spec §5.5): quello ricordato nel
/// record, altrimenti il più frequente tra i muri del disegno
/// ([`extract::propose_new_walls_layer`]); `None` se non c'è né l'uno né
/// l'altro.
fn proposed_new_walls_layer(doc: &Document) -> Option<String> {
    record::read(doc)
        .new_walls_layer
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .or_else(|| extract::propose_new_walls_layer(doc))
}

/// Perché `layer` (già senza spazi attorno) non va bene per i muri nuovi dello
/// stato di progetto, se lo si chiede: vuoto, oppure un nome che AutoCAD non
/// accetta per un layer nuovo (uno di `<>/\":;?*|=` o l'apice inverso, un
/// carattere di controllo, più di 255 caratteri). Un layer che il disegno ha
/// già va sempre bene.
fn new_walls_layer_error(
    doc: &Document,
    elaborati: &[Elaborato],
    layer: &str,
) -> Option<String> {
    if !elaborati.contains(&Elaborato::StatoDiProgetto) || doc.layers.contains(layer) {
        return None;
    }
    if layer.is_empty() {
        return Some(
            "AL_ESTRAI: indica il layer dei muri nuovi per lo stato di progetto. \
             Niente è stato scritto."
                .into(),
        );
    }
    let invalid = layer.chars().count() > 255
        || layer
            .chars()
            .any(|c| "<>/\\\":;?*|=`".contains(c) || c.is_control());
    invalid.then(|| {
        format!(
            "AL_ESTRAI: «{layer}» non è un nome di layer valido (niente <>/\\\":;?*|=` \
             né caratteri di controllo, al massimo 255 caratteri). Niente è stato scritto."
        )
    })
}

/// Il nome di base dei file estratti: il file del disegno senza estensione
/// oppure, per un disegno mai salvato, il titolo del tab; sempre un nome di
/// file valido per Windows.
fn base_name(path: Option<&Path>, title: &str) -> String {
    let raw = path.and_then(Path::file_stem).map_or_else(
        || title.to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    safe_file_stem(&raw)
}

/// `name` come parte di un nome di file di Windows: i caratteri vietati
/// (`<>:"/\|?*` e quelli di controllo) diventano `_`. Un nome vuoto diventa
/// «Disegno».
fn safe_file_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    match cleaned.trim() {
        "" => FALLBACK_BASE.to_string(),
        stem => stem.to_string(),
    }
}

/// I nomi dei file, separati da virgole (la cartella è già nota).
fn file_names(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| {
            path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Che cosa chiede `AL_STATO <parola> [valore]`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum StatoChange {
    Demolito,
    Nuovo,
    Varco,
    Altezza(f64),
    Spessore(f64),
}

impl StatoChange {
    /// La parola chiave (breve o intera, senza badare alle maiuscole) e il suo
    /// valore. Ciò che non si capisce è un errore, mai un valore inventato.
    fn parse(keyword: &str, args: &[&str]) -> Result<Self, String> {
        let no_value = |change: Self| match args {
            [] => Ok(change),
            [extra, ..] => Err(format!(
                "AL_STATO: argomento in più \"{extra}\" dopo \"{keyword}\"."
            )),
        };
        match keyword.to_ascii_uppercase().as_str() {
            "D" | "DEMOLITO" => no_value(Self::Demolito),
            "N" | "NUOVO" => no_value(Self::Nuovo),
            "V" | "VARCO" => no_value(Self::Varco),
            "A" | "ALTEZZA" => stato_value("altezza", args).map(Self::Altezza),
            "S" | "SPESSORE" => stato_value("spessore", args).map(Self::Spessore),
            _ => Err(format!(
                "AL_STATO: opzione \"{keyword}\" sconosciuta. \
                 Usa Demolito, Nuovo, Varco, Altezza o Spessore."
            )),
        }
    }

    /// La zona dopo il cambio; `None` se non cambia nulla o se il cambio non
    /// vale per quella zona (il varco di una zona nuova).
    fn apply(self, zone: &Zone) -> Option<Zone> {
        let new = match self {
            Self::Demolito => Zone {
                state: ZoneState::Demolito,
                ..zone.clone()
            },
            // Una zona nuova non è un varco.
            Self::Nuovo => Zone {
                state: ZoneState::Nuovo,
                varco: false,
                ..zone.clone()
            },
            Self::Varco if zone.state == ZoneState::Demolito => Zone {
                varco: !zone.varco,
                ..zone.clone()
            },
            Self::Varco => return None,
            Self::Altezza(height) => Zone {
                height: Some(height),
                ..zone.clone()
            },
            Self::Spessore(thickness) => Zone {
                thickness: Some(thickness),
                ..zone.clone()
            },
        };
        (new != *zone).then_some(new)
    }
}

/// Il valore di `Altezza` / `Spessore`: un solo argomento, un numero positivo.
fn stato_value(what: &str, args: &[&str]) -> Result<f64, String> {
    match args {
        [] => Err(format!("AL_STATO: manca il valore ({what}).")),
        [text] => parse_positive(text).ok_or_else(|| {
            format!(
                "AL_STATO: valore non valido \"{text}\" ({what}): \
                 serve un numero maggiore di zero, per esempio 3,20."
            )
        }),
        [_, extra, ..] => Err(format!(
            "AL_STATO: argomento in più \"{extra}\" dopo il valore ({what})."
        )),
    }
}

/// Un numero maggiore di zero, con la virgola o il punto come separatore
/// decimale («3,20», «3.20»). Niente separatore delle migliaia.
fn parse_positive(text: &str) -> Option<f64> {
    let value: f64 = text.trim().replace(',', ".").parse().ok()?;
    (value > 0.0 && crate::entities::common::typed_value_in_range(value)).then_some(value)
}

/// Ciò che `AL_STATO` farebbe sulla selezione, senza ancora toccare nulla.
struct StatoPlan {
    /// Oggetti selezionati.
    selected: usize,
    /// Zone che cambiano davvero, con il loro nuovo record.
    changes: Vec<(Handle, Zone)>,
    /// Oggetti selezionati che non sono zone.
    not_zones: usize,
    /// Zone su layer bloccati: non si toccano.
    locked: usize,
    /// `V` su almeno una zona nuova.
    varco_on_new: bool,
}

fn stato_plan(scene: &Scene, change: StatoChange) -> StatoPlan {
    let mut plan = StatoPlan {
        selected: 0,
        changes: Vec::new(),
        not_zones: 0,
        locked: 0,
        varco_on_new: false,
    };
    for (handle, entity) in scene.selected_entities() {
        plan.selected += 1;
        let Some(found) = zone::read_zone(handle, entity) else {
            plan.not_zones += 1;
            continue;
        };
        if scene.is_layer_locked(handle) {
            plan.locked += 1;
            continue;
        }
        match change.apply(&found.zone) {
            Some(new) => plan.changes.push((handle, new)),
            None => {
                plan.varco_on_new |=
                    change == StatoChange::Varco && found.zone.state == ZoneState::Nuovo;
            }
        }
    }
    plan
}

/// Ciò che un comando crea ora finisce nello spazio modello? È la regola con
/// cui `Scene::add_entity` sceglie il blocco proprietario: nell'editor di
/// blocchi il blocco, in un layout senza finestra attiva (PSPACE) il layout,
/// altrimenti (modello, o finestra attiva in un layout: MSPACE) il modello.
fn commits_to_model_space(scene: &Scene) -> bool {
    scene.block_edit_block.is_none()
        && (scene.current_layout == "Model" || scene.active_viewport.is_some())
}

/// L'istantanea della geometria per [`ZoneCommand`] e il numero di entità muro
/// rimaste fuori dal piano XY. Le facce si saldano anche sui vuoti entro
/// `HPGAPTOL`, come HATCH; il resto usa la tolleranza dell'unità.
fn zone_snapshot(doc: &Document, unit: DrawingUnit) -> (ZoneSnapshot, usize) {
    let tol = unit.tolerance();
    // R67: le parti dei locali non sono muri (punto interno, asse, convalida).
    let set = walls::wall_set_without_rooms(doc, tol);
    let faces = walls::faces(&set.sources, tol.max(super::hatchvars::gap_tolerance()));
    let zones = zone::zones_in(doc);
    let last_thickness = last_new_thickness(&zones);
    let snap = ZoneSnapshot {
        walls: set.sources,
        faces,
        zones,
        unit,
        tol,
        last_thickness,
    };
    (snap, set.off_plane)
}

/// La tolleranza dei locali (spec §3): quella dell'unità, o `HPGAPTOL` se è
/// più larga.
fn room_tolerance(unit: DrawingUnit) -> f64 {
    unit.tolerance().max(super::hatchvars::gap_tolerance())
}

/// Lo spessore della zona nuova più recente (handle più alto) che ne ha uno.
fn last_new_thickness(zones: &[ZoneGeom]) -> Option<f64> {
    zones
        .iter()
        .filter(|z| z.zone.state == ZoneState::Nuovo)
        .filter_map(|z| Some((z.handle.value(), z.zone.thickness?)))
        .max_by_key(|&(handle, _)| handle)
        .map(|(_, thickness)| thickness)
}

#[cfg(test)]
mod tests {
    use super::{
        base_name, commits_to_model_space, exists_text, file_names, folder_error,
        new_walls_layer_error, parse_positive, partial_failure_text, safe_file_stem, unquote,
        StatoChange,
    };
    use crate::bim::extract::Elaborato;
    use crate::bim::zone::{Zone, ZoneOrigin, ZoneState};
    use crate::scene::Scene;
    use codec::tables::Layer;
    use codec::{Document, Handle};
    use std::path::{Path, PathBuf};

    #[test]
    fn new_walls_layer_must_be_a_valid_name_for_the_project() {
        let mut doc = Document::new();
        let mut old = Layer::new("VECCHIO;LAYER");
        old.handle = doc.allocate_handle();
        doc.layers.add(old).unwrap();
        let progetto = [Elaborato::StatoDiFatto, Elaborato::StatoDiProgetto];
        for good in [
            "MURI NUOVI",
            "0",
            "Muri-nuovi_2",
            "VECCHIO;LAYER",
            "vecchio;layer",
        ] {
            assert_eq!(
                new_walls_layer_error(&doc, &progetto, good),
                None,
                "{good:?}"
            );
        }
        assert_eq!(
            new_walls_layer_error(&doc, &progetto, &"x".repeat(255)),
            None
        );
        let empty = new_walls_layer_error(&doc, &progetto, "").unwrap();
        assert!(empty.starts_with("AL_ESTRAI: indica il layer"), "{empty}");
        for bad in [
            "A<B", "A>B", "A/B", "A\\B", "A\"B", "A:B", "A;B", "A?B", "A*B", "A|B", "A=B", "A`B",
            "A\tB",
        ] {
            let error = new_walls_layer_error(&doc, &progetto, bad).unwrap();
            assert!(error.contains("non è un nome di layer valido"), "{bad:?}");
            assert!(error.ends_with("Niente è stato scritto."), "{error}");
        }
        assert!(new_walls_layer_error(&doc, &progetto, &"x".repeat(256)).is_some());
        // Senza stato di progetto il layer non serve.
        assert_eq!(
            new_walls_layer_error(&doc, &[Elaborato::StatoDiFatto], ""),
            None
        );
        assert_eq!(new_walls_layer_error(&doc, &[], "A/B"), None);
    }

    #[test]
    fn base_names_are_valid_file_names() {
        assert_eq!(base_name(None, "Drawing1"), "Drawing1");
        assert_eq!(
            base_name(Some(Path::new("C:/Progetti/Casa Rossi.dwg")), "Drawing1"),
            "Casa Rossi"
        );
        assert_eq!(
            base_name(Some(Path::new("C:/Progetti/rilievo.v2.dxf")), "x"),
            "rilievo.v2"
        );
        assert_eq!(
            safe_file_stem("a<b>c:d\"e/f\\g|h?i*j"),
            "a_b_c_d_e_f_g_h_i_j"
        );
        assert_eq!(safe_file_stem("tab\u{7}\t"), "tab__");
        assert_eq!(safe_file_stem("  casa  "), "casa");
        assert_eq!(safe_file_stem("   "), "Disegno");
        assert_eq!(base_name(None, ""), "Disegno");
    }

    #[test]
    fn folder_argument_drops_the_quotes() {
        assert_eq!(
            unquote(r#""C:\Progetti\Casa Rossi""#),
            r"C:\Progetti\Casa Rossi"
        );
        assert_eq!(
            unquote(r"C:\Progetti\Casa Rossi"),
            r"C:\Progetti\Casa Rossi"
        );
        assert_eq!(unquote(r#"" C:\x ""#), r"C:\x");
        assert_eq!(unquote(r#""""#), "");
        assert_eq!(unquote(r#"""#), r#"""#);
    }

    #[test]
    fn extract_error_texts() {
        let fatto = PathBuf::from("C:/x/casa_stato-di-fatto.dwg");
        let comparativa = PathBuf::from("C:/x/casa_demolizioni-costruzioni.dwg");
        assert_eq!(
            exists_text(&[fatto.clone(), comparativa]),
            concat!(
                "AL_ESTRAI: i file esistono già: casa_stato-di-fatto.dwg, ",
                "casa_demolizioni-costruzioni.dwg. Niente è stato scritto."
            )
        );
        assert_eq!(
            partial_failure_text(&[fatto], "C:/x/b.dwg: Accesso negato."),
            concat!(
                "AL_ESTRAI: scrittura non riuscita: C:/x/b.dwg: Accesso negato. ",
                "(già scritti prima dell'errore: casa_stato-di-fatto.dwg)."
            )
        );
    }

    #[test]
    fn folder_errors_and_file_lists() {
        let here = std::env::temp_dir();
        assert_eq!(folder_error(&here), None);
        let missing = here.join("archline-cartella-che-non-esiste-mai");
        assert_eq!(
            folder_error(&missing),
            Some(format!(
                "la cartella {} non esiste. Niente è stato scritto.",
                missing.display()
            ))
        );
        assert_eq!(
            file_names(&[
                PathBuf::from("C:/x/casa_stato-di-fatto.dwg"),
                PathBuf::from("C:/x/casa_demolizioni-costruzioni.dwg"),
            ]),
            "casa_stato-di-fatto.dwg, casa_demolizioni-costruzioni.dwg"
        );
    }

    fn zone(state: ZoneState) -> Zone {
        Zone {
            state,
            origin: ZoneOrigin::MuroDuePunti,
            axis: Some(([0.0, 0.0], [0.0, 3.0])),
            thickness: Some(0.10),
            height: None,
            varco: false,
        }
    }

    #[test]
    fn positive_numbers_take_comma_or_dot() {
        assert_eq!(parse_positive("3,20"), Some(3.2));
        assert_eq!(parse_positive("3.20"), Some(3.2));
        assert_eq!(parse_positive(" 0,10 "), Some(0.1));
        assert_eq!(parse_positive("+2"), Some(2.0));
        for bad in [
            "", "0", "0,0", "-1", "abc", "1.234,50", "3,2,1", "1e999", "NaN", "inf",
        ] {
            assert_eq!(parse_positive(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn stato_keywords_short_and_long_with_their_values() {
        for (keyword, args, want) in [
            ("D", &[][..], StatoChange::Demolito),
            ("demolito", &[][..], StatoChange::Demolito),
            ("N", &[][..], StatoChange::Nuovo),
            ("Nuovo", &[][..], StatoChange::Nuovo),
            ("v", &[][..], StatoChange::Varco),
            ("A", &["3,20"][..], StatoChange::Altezza(3.2)),
            ("ALTEZZA", &["3.20"][..], StatoChange::Altezza(3.2)),
            ("S", &["0,25"][..], StatoChange::Spessore(0.25)),
        ] {
            assert_eq!(StatoChange::parse(keyword, args), Ok(want), "{keyword}");
        }
        for (keyword, args) in [
            ("A", &[][..]),
            ("S", &["0"][..]),
            ("A", &["x"][..]),
            ("A", &["3", "20"][..]),
            ("N", &["3"][..]),
            ("X", &[][..]),
        ] {
            let error = StatoChange::parse(keyword, args).unwrap_err();
            assert!(
                error.starts_with("AL_STATO:"),
                "{keyword} {args:?}: {error}"
            );
        }
    }

    #[test]
    fn stato_changes_only_what_changes() {
        let demolita = zone(ZoneState::Demolito);
        let nuova = zone(ZoneState::Nuovo);

        assert_eq!(StatoChange::Demolito.apply(&demolita), None);
        assert_eq!(
            StatoChange::Demolito.apply(&nuova),
            Some(Zone {
                state: ZoneState::Demolito,
                ..nuova.clone()
            })
        );
        assert_eq!(StatoChange::Nuovo.apply(&nuova), None);

        // Una zona nuova non è un varco: passando a «nuovo» il varco cade.
        let varco = Zone {
            varco: true,
            ..demolita.clone()
        };
        let now_new = StatoChange::Nuovo.apply(&varco).unwrap();
        assert_eq!((now_new.state, now_new.varco), (ZoneState::Nuovo, false));

        // Il varco si inverte per le demolite e non vale per le nuove.
        assert!(StatoChange::Varco.apply(&demolita).unwrap().varco);
        assert!(!StatoChange::Varco.apply(&varco).unwrap().varco);
        assert_eq!(StatoChange::Varco.apply(&nuova), None);

        // Altezza e spessore: stesso valore, nessun cambio.
        let tall = StatoChange::Altezza(2.7).apply(&demolita).unwrap();
        assert_eq!(tall.height, Some(2.7));
        assert_eq!(StatoChange::Altezza(2.7).apply(&tall), None);
        assert_eq!(StatoChange::Spessore(0.10).apply(&demolita), None);
        let thick = StatoChange::Spessore(0.3).apply(&demolita).unwrap();
        assert_eq!((thick.thickness, thick.axis), (Some(0.3), demolita.axis));
    }

    #[test]
    fn model_space_or_entered_viewport_only() {
        let mut scene = Scene::new();
        assert!(commits_to_model_space(&scene), "modello");
        scene.current_layout = "Layout1".into();
        assert!(
            !commits_to_model_space(&scene),
            "layout senza finestra attiva (PSPACE)"
        );
        scene.active_viewport = Some(Handle::new(0x99));
        assert!(
            commits_to_model_space(&scene),
            "finestra attiva in un layout (MSPACE)"
        );
        scene.current_layout = "Model".into();
        scene.active_viewport = None;
        scene.block_edit_block = Some(Handle::new(0x42));
        assert!(!commits_to_model_space(&scene), "editor di blocchi");
    }
}
