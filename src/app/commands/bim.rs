//! Famiglia dei comandi BIM di ArchLine (spec §3 e §5.1): `AL_DEMOLISCI`,
//! `AL_NUOVO`, `AL_STATO` e l'interno `AL_UNITA`.
//!
//! Qui sta solo il lato app: l'unità del disegno (chiesta quando `INSUNITS`
//! vale 0, mai scritta in `INSUNITS`), i layer `AL-*` creati la prima volta in
//! un passo di annullamento a sé, l'istantanea della geometria, l'avvio di
//! [`ZoneCommand`] e il cambio di stato delle zone esistenti (`AL_STATO`). La
//! logica sta in `crate::bim`.

use crate::app::{Message, OpenCADStudio};
use crate::bim::commands::{ZoneCommand, ZoneSnapshot};
use crate::bim::record;
use crate::bim::report::Notice;
use crate::bim::units::{self, DrawingUnit, UnitLookup};
use crate::bim::walls;
use crate::bim::zone::{self, Zone, ZoneGeom, ZoneState};
use crate::bim::APP_ZONA;
use crate::command::{CadCommand, KeywordCommand, SelectThenKeywordCommand};
use crate::scene::view::dispatch::set_entity_xdata;
use crate::scene::Scene;
use codec::{CadDocument, Handle};
use iced::Task;
use std::collections::HashMap;

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
        if zone::missing_layers(&self.tabs[i].scene.document) {
            // Passo a sé: un annullamento dopo il primo comando toglie la zona,
            // non i layer; e nessun layer nasce fuori da un passo.
            self.push_undo_snapshot(i, "AL_LAYER");
            zone::ensure_layers(&mut self.tabs[i].scene.document);
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
            self.refresh_layer_panel();
        }
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
        self.tabs[i].dirty = true;
        self.finish_pending_history(i);
        let pending = self.bim_pending.take();
        if !saved {
            // Senza dizionario radice il record non si scrive: rilanciare
            // chiederebbe di nuovo, all'infinito.
            self.command_line
                .push_error("AL_UNITA: impossibile salvare l'unità nel disegno.");
            return Task::none();
        }
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
fn zone_snapshot(doc: &CadDocument, unit: DrawingUnit) -> (ZoneSnapshot, usize) {
    let tol = unit.tolerance();
    let set = walls::wall_set(doc, tol);
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
    use super::{commits_to_model_space, parse_positive, StatoChange};
    use crate::bim::zone::{Zone, ZoneOrigin, ZoneState};
    use crate::scene::Scene;
    use codec::Handle;

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
