//! I comandi BIM `AL_PORTA`, `AL_FINESTRA` e `AL_PORTAFINESTRA` della parte 3
//! (spec `2026-11-05-bim-porte-finestre-design.md`, §5.1), lato app.
//!
//! Qui sta solo l'avvio: spazio modello, unità del disegno (chiesta come negli
//! altri comandi `AL_*`), layer `AL-APERTURE` e APPID in un passo di
//! annullamento a sé la prima volta, istantanea dei muri e
//! [`ApertureCommand`]. La logica sta in `crate::bim::aperture_command`; il
//! commit dell'entità lo fa l'host (`CommitEntitiesAndExit`).

use crate::app::{Message, OpenCADStudio};
use crate::bim::aperture::{self, ApertureKind};
use crate::bim::aperture_command::{ApertureCommand, ApertureSnapshot};
use crate::bim::report::Notice;
use crate::bim::units::DrawingUnit;
use crate::bim::walls;
use crate::command::CadCommand;
use codec::Document;
use iced::Task;

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
