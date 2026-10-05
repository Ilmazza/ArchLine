//! ArchLine: app-state glue for the classic menu bar.
//!
//! Kept out of `view_main` (see `ui::classic_toolbar::top_bar`): that function
//! is so large that extra nesting there can overflow rustc's stack.

use crate::app::{Message, OpenCADStudio};
use crate::ui::classic_menu::{menu_bar, MenuCtx, TabEntry};
use iced::Element;

impl OpenCADStudio {
    /// One entry per open document tab, the active one flagged.
    pub(super) fn classic_menu_tabs(&self) -> Vec<TabEntry> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| TabEntry {
                index,
                name: tab.tab_display_name(),
                active: index == self.active_tab,
            })
            .collect()
    }

    /// The classic menu bar for the current state.
    #[inline(never)]
    pub(super) fn classic_menu_bar(&self) -> Element<'static, Message> {
        menu_bar(&MenuCtx {
            bindings: &self.shortcut_bindings,
            tabs: self.classic_menu_tabs(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::app::OpenCADStudio;

    #[test]
    fn the_window_menu_lists_every_tab_and_flags_the_active_one() {
        let app = OpenCADStudio::new_for_test();
        let tabs = app.classic_menu_tabs();
        assert_eq!(tabs.len(), app.tabs.len());
        assert_eq!(tabs.iter().filter(|t| t.active).count(), 1);
        assert!(tabs[app.active_tab].active);
        assert!(tabs.iter().all(|t| !t.name.is_empty()));
    }

    #[test]
    fn the_bar_builds_from_app_state() {
        let app = OpenCADStudio::new_for_test();
        let bar = app.classic_menu_bar();
        assert_eq!(bar.as_widget().size().width, iced::Length::Fill);
    }

    #[test]
    fn the_bar_is_hidden_on_start_and_clean_screen() {
        assert!(!crate::workspace::classic_active(true, false));
        assert!(!crate::workspace::classic_active(false, true));
    }
}
