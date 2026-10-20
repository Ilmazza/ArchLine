//! ArchLine: the snap-spacing fields of the Snap to Grid status-bar menu
//! (`ui/popup/grid_snap_popup.rs`). The spacing itself is
//! `Snapper::snap_spacing_x/y` (SNAPUNIT), the same values the Drafting
//! Settings window edits; this file only holds what is half-typed in the menu.

use iced::Task;

use super::{Message, OpenCADStudio};
use crate::ui::popup::grid_snap_popup::SnapSpacingMsg;
use crate::ui::window::drafting_settings::{format_snap_spacing, parse_snap_spacing};

/// What the two fields hold while the user is typing. `None` on the app means
/// "show the live spacing", so a value changed elsewhere (Drafting Settings,
/// SNAPUNIT, a loaded file) is never hidden behind stale text.
pub(super) struct SnapSpacingEdit {
    x: String,
    y: String,
    /// X and Y were equal when typing began: the other field follows, as the
    /// "Equal X and Y spacing" checkbox of Drafting Settings does.
    equal: bool,
}

impl OpenCADStudio {
    /// Text of the X and Y fields: the half-typed values, else the live spacing.
    pub(crate) fn snap_spacing_texts(&self) -> (String, String) {
        match &self.snap_spacing_edit {
            Some(edit) => (edit.x.clone(), edit.y.clone()),
            None => (
                format_snap_spacing(self.snapper.snap_spacing_x),
                format_snap_spacing(self.snapper.snap_spacing_y),
            ),
        }
    }

    #[inline(never)]
    pub(super) fn on_snap_spacing(&mut self, msg: SnapSpacingMsg) -> Task<Message> {
        match msg {
            SnapSpacingMsg::X(value) => {
                let edit = self.snap_spacing_edit_mut();
                if edit.equal {
                    edit.y = value.clone();
                }
                edit.x = value;
            }
            SnapSpacingMsg::Y(value) => {
                let edit = self.snap_spacing_edit_mut();
                if edit.equal {
                    edit.x = value.clone();
                }
                edit.y = value;
            }
            SnapSpacingMsg::Submit => self.apply_snap_spacing(),
            SnapSpacingMsg::Discard => self.snap_spacing_edit = None,
        }
        Task::none()
    }

    fn snap_spacing_edit_mut(&mut self) -> &mut SnapSpacingEdit {
        let (x, y) = self.snap_spacing_texts();
        let equal = (self.snapper.snap_spacing_x - self.snapper.snap_spacing_y).abs() < 1e-6;
        self.snap_spacing_edit
            .get_or_insert(SnapSpacingEdit { x, y, equal })
    }

    fn apply_snap_spacing(&mut self) {
        let Some(edit) = &self.snap_spacing_edit else {
            return;
        };
        let (Some(x), Some(y)) = (parse_snap_spacing(&edit.x), parse_snap_spacing(&edit.y)) else {
            self.command_line
                .push_error(crate::t!("Snap X and Y spacings must be positive numbers.").as_ref());
            return;
        };
        self.snapper.snap_spacing_x = x;
        self.snapper.snap_spacing_y = y;
        self.snap_spacing_edit = None;
        self.persist_settings_if_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_x(app: &mut OpenCADStudio, value: &str) {
        let _ = app.update(Message::SnapSpacing(SnapSpacingMsg::X(value.into())));
    }

    #[test]
    fn typing_shows_the_text_and_enter_applies_it() {
        let mut app = OpenCADStudio::new_for_test();
        type_x(&mut app, "25");
        // Equal spacings stay equal while typing.
        assert_eq!(app.snap_spacing_texts(), ("25".into(), "25".into()));
        assert_ne!(app.snapper.snap_spacing_x, 25.0);

        let _ = app.update(Message::SnapSpacing(SnapSpacingMsg::Submit));
        assert_eq!(app.snapper.snap_spacing_x, 25.0);
        assert_eq!(app.snapper.snap_spacing_y, 25.0);
        assert!(app.snap_spacing_edit.is_none());
    }

    #[test]
    fn unequal_spacings_are_edited_one_at_a_time() {
        let mut app = OpenCADStudio::new_for_test();
        app.snapper.snap_spacing_x = 10.0;
        app.snapper.snap_spacing_y = 5.0;
        type_x(&mut app, "20");
        assert_eq!(app.snap_spacing_texts(), ("20".into(), "5".into()));
        let _ = app.update(Message::SnapSpacing(SnapSpacingMsg::Submit));
        assert_eq!(
            (app.snapper.snap_spacing_x, app.snapper.snap_spacing_y),
            (20.0, 5.0)
        );
    }

    #[test]
    fn a_bad_value_changes_nothing_and_keeps_the_text() {
        let mut app = OpenCADStudio::new_for_test();
        let before = app.snapper.snap_spacing_x;
        for bad in ["0", "-3", "abc", ""] {
            type_x(&mut app, bad);
            let _ = app.update(Message::SnapSpacing(SnapSpacingMsg::Submit));
            assert_eq!(app.snapper.snap_spacing_x, before, "{bad:?}");
            assert_eq!(app.snap_spacing_texts().0, bad);
        }
    }

    #[test]
    fn discarding_goes_back_to_the_live_spacing() {
        let mut app = OpenCADStudio::new_for_test();
        type_x(&mut app, "99");
        let _ = app.update(Message::SnapSpacing(SnapSpacingMsg::Discard));
        let live = format_snap_spacing(app.snapper.snap_spacing_x);
        assert_eq!(app.snap_spacing_texts().0, live);
    }
}
