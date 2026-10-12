//! One-shot commands the HATCH dialog starts while it is hidden: choosing the
//! hatch origin and showing a preview. Both hand control back to the dialog
//! through a `Dispatch` string, as the Write Block dialog's point picker does.

use crate::command::{CadCommand, CmdResult};
use crate::scene::model::hatch_model::HatchModel;
use glam::DVec3;

/// One-shot picker for the dialog's "Click to set new origin" button.
pub struct HatchOriginPickCommand;

impl CadCommand for HatchOriginPickCommand {
    fn name(&self) -> &'static str {
        "HATCH"
    }

    fn prompt(&self) -> String {
        crate::t!("HATCH  Specify hatch origin:").into_owned()
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        CmdResult::Dispatch(format!("HATCH_ORIGIN_PICKED {} {} {}", pt.x, pt.y, pt.z))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch("HATCH_PICK_CANCELLED".to_string())
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Dispatch("HATCH_PICK_CANCELLED".to_string())
    }
}

/// Shows the hatch the dialog would create until a click, Enter or Esc.
pub struct HatchPreviewCommand {
    models: Vec<HatchModel>,
}

impl HatchPreviewCommand {
    pub fn new(models: Vec<HatchModel>) -> Self {
        Self { models }
    }
}

impl CadCommand for HatchPreviewCommand {
    fn name(&self) -> &'static str {
        "HATCH"
    }

    fn prompt(&self) -> String {
        crate::t!("HATCH  Preview — click or press Enter to return to the dialog:").into_owned()
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::Dispatch("HATCH_PREVIEW_DONE".to_string())
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch("HATCH_PREVIEW_DONE".to_string())
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Dispatch("HATCH_PREVIEW_DONE".to_string())
    }

    fn hatch_preview_models(&self) -> Option<Vec<HatchModel>> {
        Some(self.models.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CadCommand, CmdResult};
    use glam::DVec3;

    #[test]
    fn origin_pick_reports_the_picked_point() {
        let mut command = HatchOriginPickCommand;
        match command.on_point(DVec3::new(1.5, -2.0, 0.0)) {
            CmdResult::Dispatch(text) => assert_eq!(text, "HATCH_ORIGIN_PICKED 1.5 -2 0"),
            _ => panic!("expected Dispatch"),
        }
    }

    #[test]
    fn origin_pick_enter_and_escape_cancel() {
        let mut command = HatchOriginPickCommand;
        for result in [command.on_enter(), command.on_escape()] {
            match result {
                CmdResult::Dispatch(text) => assert_eq!(text, "HATCH_PICK_CANCELLED"),
                _ => panic!("expected Dispatch"),
            }
        }
    }

    #[test]
    fn preview_ends_on_click_enter_and_escape() {
        let mut command = HatchPreviewCommand::new(Vec::new());
        let results = [
            command.on_point(DVec3::ZERO),
            command.on_enter(),
            command.on_escape(),
        ];
        for result in results {
            match result {
                CmdResult::Dispatch(text) => assert_eq!(text, "HATCH_PREVIEW_DONE"),
                _ => panic!("expected Dispatch"),
            }
        }
    }

    #[test]
    fn preview_shows_the_models_it_was_given() {
        let command = HatchPreviewCommand::new(Vec::new());
        assert_eq!(command.hatch_preview_models().map(|models| models.len()), Some(0));
    }
}
