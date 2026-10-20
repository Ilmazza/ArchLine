//! ArchLine: the "Missing fonts" dialog's choice of a stand-in per font. The
//! model and the saved table are `crate::font_alt`; the window is
//! `ui/window/missing_fonts.rs`.

use iced::Task;

use super::{Message, OpenCADStudio};
use crate::font_alt::{self, Substitute};

impl OpenCADStudio {
    pub(super) fn on_missing_fonts_pick(&mut self, font: String, pick: Substitute) {
        self.missing_font_picks.insert(font, pick);
    }

    /// OK: remember what each listed font is drawn with, then redraw the text
    /// of every open drawing so the choice shows at once.
    #[inline(never)]
    pub(super) fn on_missing_fonts_apply(&mut self) -> Task<Message> {
        if self.missing_fonts_downloading {
            return Task::none();
        }
        let fonts = self.missing_fonts.take().unwrap_or_default();
        let mut redraw = false;
        for name in &fonts {
            let pick = self
                .missing_font_picks
                .remove(name)
                .unwrap_or(Substitute::Default);
            match &pick {
                Substitute::Default => {}
                Substitute::Stroke(font) | Substitute::System(font) => {
                    redraw = true;
                    self.command_line
                        .push_output(crate::tf!("FONT  {name} → {font}").as_ref());
                }
            }
            font_alt::choose(name, pick);
        }
        self.missing_font_picks.clear();
        self.missing_fonts_path = None;
        self.close_active_modal();
        self.save_config();
        if redraw {
            self.invalidate_text_everywhere();
        }
        Task::none()
    }
}
