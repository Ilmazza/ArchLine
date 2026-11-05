//! Il comando BIM `AL_TAGLIO` della parte 4 (spec
//! `2026-11-05-bim-sezioni-prospetti-design.md`, §5.1), lato app.
//!
//! Solo l'avvio: spazio modello, layer `AL-TAGLI` e APPID in un passo di
//! annullamento a sé la prima volta, i nomi già presi e
//! [`CutCommand`]. Nessuna unità da chiedere: il taglio è geometria. La logica
//! sta in `crate::bim::cut_command`; il commit lo fa l'host.

use crate::app::{Message, OpenCADStudio};
use crate::bim::cut::{self, cuts_in};
use crate::bim::cut_command::CutCommand;
use crate::command::CadCommand;
use iced::Task;

const CMD: &str = "AL_TAGLIO";

impl OpenCADStudio {
    /// `AL_TAGLIO`: spazio modello, layer (la prima volta), nomi presi, poi
    /// [`CutCommand`].
    pub(super) fn bim_taglio(&mut self, i: usize) -> Task<Message> {
        if !self.bim_in_model_space(i, CMD) {
            return Task::none();
        }
        self.bim_pending = None;
        if cut::missing_layer(&self.tabs[i].scene.document) {
            self.push_undo_snapshot(i, "AL_LAYER");
            cut::ensure_layer(&mut self.tabs[i].scene.document);
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
            self.refresh_layer_panel();
        }
        let taken = cuts_in(&self.tabs[i].scene.document)
            .into_iter()
            .map(|found| found.cut.name)
            .collect();
        let command = CutCommand::new(taken);
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.finish_dispatch(CMD)
    }
}
