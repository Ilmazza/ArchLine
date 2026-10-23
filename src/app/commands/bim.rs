//! Famiglia dei comandi BIM di ArchLine (spec §3 e §5.1): `AL_DEMOLISCI`,
//! `AL_NUOVO` e l'interno `AL_UNITA`.
//!
//! Qui sta solo il lato app: l'unità del disegno (chiesta quando `INSUNITS`
//! vale 0, mai scritta in `INSUNITS`), i layer `AL-*` creati la prima volta in
//! un passo di annullamento a sé, l'istantanea della geometria e l'avvio di
//! [`ZoneCommand`]. La logica sta in `crate::bim`.

use crate::app::{Message, OpenCADStudio};
use crate::bim::commands::{ZoneCommand, ZoneSnapshot};
use crate::bim::record;
use crate::bim::report::Notice;
use crate::bim::units::{self, DrawingUnit, UnitLookup};
use crate::bim::walls;
use crate::bim::zone::{self, ZoneGeom, ZoneState};
use crate::command::{CadCommand, KeywordCommand};
use crate::scene::Scene;
use codec::CadDocument;
use iced::Task;

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

impl OpenCADStudio {
    /// I comandi `AL_*`, riconosciuti dal primo token in maiuscolo.
    pub(super) fn dispatch_bim(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let mut words = cmd.split_whitespace();
        let name = words.next()?.to_ascii_uppercase();
        match name.as_str() {
            "AL_DEMOLISCI" => Some(self.bim_start_zone(i, "AL_DEMOLISCI", ZoneState::Demolito)),
            "AL_NUOVO" => Some(self.bim_start_zone(i, "AL_NUOVO", ZoneState::Nuovo)),
            "AL_UNITA" => Some(self.bim_set_unit(i, words.next())),
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
    use super::commits_to_model_space;
    use crate::scene::Scene;
    use codec::Handle;

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
