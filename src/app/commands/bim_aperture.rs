//! I comandi BIM `AL_PORTA`, `AL_FINESTRA` e `AL_PORTAFINESTRA` della parte 3
//! (spec `2026-11-05-bim-porte-finestre-design.md`, §5.1), lato app.
//!
//! Qui sta solo l'avvio: spazio modello, unità del disegno (chiesta come negli
//! altri comandi `AL_*`), layer `AL-APERTURE` e APPID in un passo di
//! annullamento a sé la prima volta, istantanea dei muri e
//! [`ApertureCommand`]. La logica sta in `crate::bim::aperture_command`; il
//! commit dell'entità lo fa l'host (`CommitEntitiesAndExit`).

use super::bim::{height_echo, parse_positive};
use crate::app::{Message, OpenCADStudio};
use crate::bim::aperture::{
    self, read_aperture, Aperture, ApertureChange, ApertureKind, ApertureState,
};
use crate::bim::aperture_command::{
    ApertureCommand, ApertureSnapshot, HEIGHT_RANGE_M,
};
use crate::bim::APP_APERTURA;
use crate::scene::view::dispatch::set_entity_xdata;
use crate::bim::report::Notice;
use crate::bim::units::DrawingUnit;
use crate::bim::walls;
use crate::command::{CadCommand, KeywordCommand};
use codec::{Document, Handle};
use iced::Task;
use std::collections::HashMap;

impl OpenCADStudio {
    /// `AL_PORTA` / `AL_FINESTRA` / `AL_PORTAFINESTRA`: spazio modello, unità,
    /// layer (la prima volta), istantanea, poi [`ApertureCommand`].
    pub(super) fn bim_aperture(&mut self, i: usize, cmd: &str, kind: ApertureKind) -> Task<Message> {
        let name = cmd.split_whitespace().next().unwrap_or(cmd).to_ascii_uppercase();
        if !self.bim_in_model_space(i, &name) {
            return Task::none();
        }
        let Some(unit) = self.bim_unit_or_ask(i, cmd) else {
            return self.finish_dispatch(&name);
        };
        // Un rilancio lasciato a metà (AL_UNITA annullato) non vale più.
        self.bim_pending = None;
        self.bim_ensure_aperture_layer(i);
        let (snap, off_plane) = aperture_snapshot(&self.tabs[i].scene.document, unit);
        if off_plane > 0 {
            self.command_line
                .push_info(&Notice::FuoriPiano { count: off_plane }.text());
        }
        let command = ApertureCommand::new(kind, snap);
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.finish_dispatch(&name)
    }

    /// Il layer `AL-APERTURE` e l'APPID `ARCHLINE_APERTURA`, se mancano, in un
    /// passo di annullamento a sé (come `bim_ensure_layers`, R98).
    fn bim_ensure_aperture_layer(&mut self, i: usize) {
        if aperture::missing_layer(&self.tabs[i].scene.document) {
            self.push_undo_snapshot(i, "AL_LAYER");
            aperture::ensure_layer(&mut self.tabs[i].scene.document);
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
            self.refresh_layer_panel();
        }
    }
}

/// L'istantanea dei muri per le aperture: le parti dei locali non sono muri
/// (R67). Dice anche quante entità fuori piano sono rimaste escluse.
fn aperture_snapshot(doc: &Document, unit: DrawingUnit) -> (ApertureSnapshot, usize) {
    let tol = unit.tolerance();
    let set = walls::wall_set_without_rooms(doc, tol);
    let snap = ApertureSnapshot {
        walls: set.sources,
        unit,
        tol,
    };
    (snap, set.off_plane)
}

const DATI: &str = "AL_APERTURA_DATI";

const DATI_PROMPT: &str =
    "AL_APERTURA_DATI  [Stato/Altezza/Davanzale/sOglia/aRchitrave/aPpoggio/riMuovi]:";

const NO_SELECTION: &str = "AL_APERTURA_DATI: nessun oggetto selezionato.";

impl OpenCADStudio {
    /// `AL_APERTURA_DATI <opzione> [valore]` sulle aperture selezionate (spec
    /// parte 3 §5.2): `Stato E|D|N`, `Altezza <v>`, `Davanzale <v>`,
    /// `Appoggio <v>`, `Soglia S|N`, `Architrave S|N`, `Rimuovi
    /// Altezza|Davanzale|Appoggio`. Come `AL_ALTEZZA S`: un solo passo di
    /// annullamento, nessuno se nulla cambia, layer bloccati saltati e contati.
    /// I valori si digitano in unità del disegno; l'unità si chiede solo per
    /// quelli (e la riga di comando li ripete in metri).
    pub(super) fn bim_aperture_dati(&mut self, i: usize, cmd: &str) -> Task<Message> {
        if !self.bim_in_model_space(i, DATI) {
            return Task::none();
        }
        let mut words = cmd.split_whitespace();
        let _name = words.next();
        let Some(keyword) = words.next() else {
            return self.bim_start_dati(i);
        };
        let args: Vec<&str> = words.collect();
        let change = match parse_change(keyword, &args) {
            Ok(change) => change,
            Err(message) => {
                self.command_line.push_error(&message);
                return Task::none();
            }
        };
        if self.tabs[i].scene.selected_entities().is_empty() {
            self.command_line.push_error(NO_SELECTION);
            return Task::none();
        }
        // L'unità serve solo a ripetere i valori in metri.
        let unit = if takes_a_length(change) {
            let Some(unit) = self.bim_unit_or_ask(i, cmd.trim()) else {
                return self.finish_dispatch(DATI);
            };
            Some(unit)
        } else {
            None
        };
        self.bim_pending = None;
        let plan = dati_plan(&self.tabs[i].scene, change);
        if !plan.changes.is_empty() {
            let mut handles: Vec<Handle> = plan.changes.keys().copied().collect();
            handles.sort_by_key(|handle| handle.value());
            let changes = &plan.changes;
            self.apply_property_op(i, DATI, &handles, |app, handle| {
                let Some(new) = changes.get(&handle) else {
                    return;
                };
                let doc = &mut app.tabs[i].scene.document;
                if let Some(entity) = doc.get_entity_mut(handle) {
                    aperture::restyle(entity, new);
                }
                // Un'apertura letta da un DWG tiene l'XDATA anche nel blocco
                // grezzo, che al salvataggio prevale sul record: lo scarta.
                set_entity_xdata(doc, handle, APP_APERTURA, Some(aperture::encode(new)));
            });
            self.finish_pending_history(i);
        }
        let what = describe(change, unit);
        self.command_line.push_info(&format!(
            "{DATI}: {what}; {} aperture modificate.",
            plan.changes.len()
        ));
        if let (ApertureChange::Altezza(Some(value)), Some(unit)) = (change, unit) {
            let metres = value / unit.per_metre();
            if metres < HEIGHT_RANGE_M.0 || metres > HEIGHT_RANGE_M.1 {
                self.command_line.push_info(&format!(
                    "Altezza fuori da {}–{} m: controlla l'unità.",
                    crate::bim::units::format_decimal(HEIGHT_RANGE_M.0),
                    crate::bim::units::format_decimal(HEIGHT_RANGE_M.1)
                ));
            }
        }
        if plan.not_ours > 0 {
            self.command_line.push_info(&format!(
                "{} oggetti non sono aperture e sono stati ignorati.",
                plan.not_ours
            ));
        }
        if plan.locked > 0 {
            self.command_line.push_info(&format!(
                "Aperture su layer bloccati ignorate: {}.",
                plan.locked
            ));
        }
        if plan.not_applicable > 0 {
            self.command_line.push_info(&format!(
                "Non si applica a {} aperture: {}.",
                plan.not_applicable,
                plan.reason.unwrap_or("tipo non adatto")
            ));
        }
        Task::none()
    }

    /// `AL_APERTURA_DATI` da solo (anche dal pulsante): la parola chiave e il
    /// suo valore tornano come riga intera. Senza selezione lo dice subito.
    fn bim_start_dati(&mut self, i: usize) -> Task<Message> {
        if self.tabs[i].scene.selected_entities().is_empty() {
            self.command_line.push_error(NO_SELECTION);
            return Task::none();
        }
        let ask = KeywordCommand::new(
            DATI,
            DATI_PROMPT,
            vec![
                ("Stato", "S", Some("Stato [Esistente/Demolita/Nuova]:")),
                ("Altezza", "A", Some("Altezza del vano (unità del disegno):")),
                ("Davanzale", "D", Some("Altezza del davanzale (unità del disegno):")),
                ("sOglia", "O", Some("Soglia [Sì/No]:")),
                ("aRchitrave", "R", Some("Architrave [Sì/No]:")),
                ("aPpoggio", "P", Some("Appoggio dell'architrave (unità del disegno):")),
                ("riMuovi", "M", Some("Togliere [Altezza/Davanzale/Appoggio]:")),
            ],
        );
        self.command_line.push_info(&ask.prompt());
        self.tabs[i].active_cmd = Some(Box::new(ask));
        self.finish_dispatch(DATI)
    }
}

/// Il cambio dalla parola chiave (breve o intera, senza badare alle
/// maiuscole) e dal suo valore. Ciò che non si capisce è un errore, mai un
/// valore inventato.
fn parse_change(keyword: &str, args: &[&str]) -> Result<ApertureChange, String> {
    let one = |what: &str| -> Result<&str, String> {
        match args {
            [] => Err(format!("{DATI}: manca {what}.")),
            [value] => Ok(*value),
            [_, extra, ..] => Err(format!("{DATI}: argomento in più \"{extra}\".")),
        }
    };
    let length = |what: &str, allow_zero: bool| -> Result<f64, String> {
        let text = one(what)?;
        let ok = if allow_zero && text.trim().replace(',', ".").parse::<f64>().ok() == Some(0.0) {
            Some(0.0)
        } else {
            parse_positive(text)
        };
        ok.ok_or_else(|| {
            format!("{DATI}: valore non valido \"{text}\": serve un numero maggiore di zero, in unità del disegno.")
        })
    };
    let yes_no = |what: &str| -> Result<bool, String> {
        match one(what)?.to_uppercase().as_str() {
            "S" | "SI" | "SÌ" => Ok(true),
            "N" | "NO" => Ok(false),
            other => Err(format!("{DATI}: \"{other}\" non è Sì né No.")),
        }
    };
    match keyword.to_uppercase().as_str() {
        "S" | "STATO" => match one("lo stato")?.to_uppercase().as_str() {
            "E" | "ESISTENTE" => Ok(ApertureChange::Stato(ApertureState::Esistente)),
            "D" | "DEMOLITA" => Ok(ApertureChange::Stato(ApertureState::Demolita)),
            "N" | "NUOVA" => Ok(ApertureChange::Stato(ApertureState::Nuova)),
            other => Err(format!("{DATI}: stato \"{other}\" sconosciuto. Usa Esistente, Demolita o Nuova.")),
        },
        "A" | "ALTEZZA" => length("l'altezza", false).map(|v| ApertureChange::Altezza(Some(v))),
        "D" | "DAVANZALE" => length("il davanzale", true).map(|v| ApertureChange::Davanzale(Some(v))),
        "P" | "APPOGGIO" => length("l'appoggio", false).map(|v| ApertureChange::Appoggio(Some(v))),
        "O" | "SOGLIA" => yes_no("Sì o No").map(ApertureChange::Soglia),
        "R" | "ARCHITRAVE" => yes_no("Sì o No").map(ApertureChange::Architrave),
        "M" | "RIMUOVI" => match one("che cosa togliere")?.to_uppercase().as_str() {
            "A" | "ALTEZZA" => Ok(ApertureChange::Altezza(None)),
            "D" | "DAVANZALE" => Ok(ApertureChange::Davanzale(None)),
            "P" | "APPOGGIO" => Ok(ApertureChange::Appoggio(None)),
            other => Err(format!("{DATI}: \"{other}\" non si può togliere. Usa Altezza, Davanzale o Appoggio.")),
        },
        _ => Err(format!(
            "{DATI}: opzione \"{keyword}\" sconosciuta. Usa Stato, Altezza, Davanzale, Soglia, Architrave, Appoggio o Rimuovi."
        )),
    }
}

/// Il cambio porta una lunghezza (e quindi chiede l'unità e ripete il valore
/// in metri).
fn takes_a_length(change: ApertureChange) -> bool {
    matches!(
        change,
        ApertureChange::Altezza(Some(_))
            | ApertureChange::Davanzale(Some(_))
            | ApertureChange::Appoggio(Some(_))
    )
}

/// Il cambio a parole, con l'eco in metri dei valori digitati.
fn describe(change: ApertureChange, unit: Option<crate::bim::units::DrawingUnit>) -> String {
    let echo = |name: &str, value: f64| match unit {
        Some(unit) => height_echo(value, unit).replacen("altezza", name, 1),
        None => format!("{name} {value}"),
    };
    match change {
        ApertureChange::Stato(state) => format!("stato {}", state.key()),
        ApertureChange::Altezza(Some(v)) => echo("altezza", v),
        ApertureChange::Altezza(None) => "altezza tolta (n.d.)".into(),
        ApertureChange::Davanzale(Some(v)) => echo("davanzale", v),
        ApertureChange::Davanzale(None) => "davanzale tolto (n.d.)".into(),
        ApertureChange::Appoggio(Some(v)) => echo("appoggio", v),
        ApertureChange::Appoggio(None) => "appoggio tolto (n.d.)".into(),
        ApertureChange::Soglia(on) => format!("soglia {}", if on { "sì" } else { "no" }),
        ApertureChange::Architrave(on) => format!("architrave {}", if on { "sì" } else { "no" }),
    }
}

/// Ciò che `AL_APERTURA_DATI` farebbe sulla selezione, senza ancora toccare
/// nulla.
#[derive(Default)]
struct DatiPlan {
    /// Le aperture che cambiano davvero, con il loro record nuovo.
    changes: HashMap<Handle, Aperture>,
    not_ours: usize,
    locked: usize,
    not_applicable: usize,
    reason: Option<&'static str>,
}

fn dati_plan(scene: &crate::scene::Scene, change: ApertureChange) -> DatiPlan {
    let doc = &scene.document;
    let mut plan = DatiPlan::default();
    for (handle, _) in scene.selected_entities() {
        let Some(found) = doc.get_entity(handle).and_then(|e| read_aperture(handle, e)) else {
            plan.not_ours += 1;
            continue;
        };
        if scene.is_layer_locked(handle) {
            plan.locked += 1;
            continue;
        }
        match change.apply(&found.aperture) {
            Ok(new) if new != found.aperture => {
                plan.changes.insert(handle, new);
            }
            Ok(_) => {}
            Err(reason) => {
                plan.not_applicable += 1;
                plan.reason = Some(reason);
            }
        }
    }
    plan
}
