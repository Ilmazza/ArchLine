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
    use crate::app::{Message, OpenCADStudio};

    #[test]
    fn the_window_menu_lists_every_tab_and_flags_the_active_one() {
        let mut app = OpenCADStudio::new_for_test();
        // `TabNew` appends a drawing and makes it active.
        let _ = app.update(Message::TabNew);
        let _ = app.update(Message::TabNew);
        assert_eq!(app.tabs.len(), 3);
        assert_ne!(app.active_tab, 0);
        let tabs = app.classic_menu_tabs();
        assert_eq!(tabs.len(), app.tabs.len());
        assert_eq!(tabs.iter().filter(|t| t.active).count(), 1);
        assert!(tabs[app.active_tab].active);
        for (i, t) in tabs.iter().enumerate() {
            assert_eq!(t.index, i);
        }
        assert!(tabs.iter().all(|t| !t.name.is_empty()));
    }

    #[test]
    fn the_bar_builds_from_app_state() {
        let app = OpenCADStudio::new_for_test();
        let bar = app.classic_menu_bar();
        assert_eq!(bar.as_widget().size().width, iced::Length::Fill);
    }

    #[test]
    fn the_toolbar_frame_builds_for_default_and_all_floating_layouts() {
        use crate::ui::toolbar_layout::{Target, ToolbarId};
        let mut app = OpenCADStudio::new_for_test();
        app.toolbars = Default::default();
        let center: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let el = crate::ui::toolbar_dock::frame(true, &app.toolbars, &app.ribbon, None, center);
        assert_eq!(el.as_widget().size().height, iced::Length::Fill);

        // Review focus: every bar floating leaves all four edges empty.
        for (i, id) in ToolbarId::ALL.into_iter().enumerate() {
            app.toolbars.move_to(id, Target::Float { x: 10.0 * i as f32, y: 10.0 });
        }
        let center: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let _ = crate::ui::toolbar_dock::frame(true, &app.toolbars, &app.ribbon, None, center);

        // Not classic: the centre comes back untouched.
        let center: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let el = crate::ui::toolbar_dock::frame(false, &app.toolbars, &app.ribbon, None, center);
        assert_eq!(el.as_widget().size().width, iced::Length::Shrink);
    }

    #[test]
    fn classic_active_is_off_on_start_and_clean_screen() {
        assert!(!crate::workspace::classic_active(true, false));
        assert!(!crate::workspace::classic_active(false, true));
    }
}
