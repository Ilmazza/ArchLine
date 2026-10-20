//! ArchLine: the workspace button of the status bar and its menu, as in
//! AutoCAD: the workspaces (a check on the current one), a separator, Save
//! Current As, Workspace Settings, Customize, and Display Workspace Label.
//! Actions are `Message::Workspace`; see `app/workspaces.rs`.

use iced::widget::{container, rule, text};
use iced::{Element, Length};

use super::status_menu::Entry;
use super::statusbar_menu::menu_row;
use crate::app::Message;
use crate::t;
use crate::ui::window::workspaces::WorkspaceMsg;
use crate::workspaces::WorkspaceSet;

/// `icons::GEAR` exists only on the web build.
static GEAR: &[u8] = include_bytes!("../../../assets/icons/ui/gear.svg");

/// What the button needs from the app, copied per frame (a handful of names).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceMenu {
    /// Every workspace in menu order.
    pub names: Vec<String>,
    pub current: String,
    pub show_label: bool,
}

impl WorkspaceMenu {
    pub fn from_set(set: &WorkspaceSet) -> Self {
        Self {
            names: set.names(),
            current: set.resolved().name,
            show_label: set.show_label,
        }
    }
}

/// One row of the menu, before it is drawn (so the order can be tested).
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Workspace { name: String, current: bool },
    Separator,
    SaveAs,
    Settings,
    Customize,
    Label { on: bool },
}

pub fn rows(menu: &WorkspaceMenu) -> Vec<Row> {
    let mut rows: Vec<Row> = menu
        .names
        .iter()
        .map(|name| Row::Workspace {
            name: name.clone(),
            current: name.eq_ignore_ascii_case(&menu.current),
        })
        .collect();
    rows.extend([
        Row::Separator,
        Row::SaveAs,
        Row::Settings,
        Row::Customize,
        Row::Label { on: menu.show_label },
    ]);
    rows
}

pub fn entries(menu: &WorkspaceMenu) -> Vec<Entry<'static>> {
    let action = |label, checked, msg| {
        Entry::close(menu_row(label, checked, Message::Workspace(msg)))
    };
    rows(menu)
        .into_iter()
        .map(|row| match row {
            Row::Workspace { name, current } => {
                let msg = WorkspaceMsg::Switch(name.clone());
                action(std::borrow::Cow::Owned(name), current, msg)
            }
            Row::Separator => Entry::stay(
                container(rule::horizontal(1))
                    .padding([2, 6])
                    .width(Length::Fill),
            ),
            Row::SaveAs => action(t!("Save Current As…"), false, WorkspaceMsg::OpenSaveAs),
            Row::Settings => action(t!("Workspace Settings…"), false, WorkspaceMsg::OpenSettings),
            Row::Customize => action(t!("Customize…"), false, WorkspaceMsg::OpenCustomize),
            Row::Label { on } => action(t!("Display Workspace Label"), on, WorkspaceMsg::ToggleLabel),
        })
        .collect()
}

/// The gear that opens the menu (wrapped by the caller in `status_menu::menu_bar`).
pub fn button() -> Element<'static, Message> {
    iced::widget::button(crate::ui::icons::themed_secondary(GEAR, 16.0))
        .on_press(Message::StatusMenuTooltipHidden(true))
        .style(iced::widget::button::subtle)
        .padding([4, 8])
        .into()
}

/// The current workspace's name, when "Display Workspace Label" is on.
pub fn label(menu: &WorkspaceMenu) -> Option<Element<'static, Message>> {
    menu.show_label.then(|| {
        container(text(menu.current.clone()).size(11))
            .padding([0, 4])
            .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspaces::{Interface, AUTOCAD_CLASSIC, OPENCAD_STUDIO};

    fn menu(show_label: bool) -> WorkspaceMenu {
        let mut set = WorkspaceSet::default();
        set.save("Mio", Interface::Classic, crate::ui::toolbar_layout::ToolbarLayout::default());
        set.current = AUTOCAD_CLASSIC.to_string();
        set.show_label = show_label;
        WorkspaceMenu::from_set(&set)
    }

    #[test]
    fn rows_follow_the_autocad_menu_order() {
        let r = rows(&menu(false));
        let names = [AUTOCAD_CLASSIC, OPENCAD_STUDIO, "Mio"];
        for (i, name) in names.iter().enumerate() {
            assert_eq!(
                r[i],
                Row::Workspace { name: name.to_string(), current: i == 0 },
                "row {i}"
            );
        }
        assert_eq!(
            r[3..],
            [Row::Separator, Row::SaveAs, Row::Settings, Row::Customize, Row::Label { on: false }]
        );
        let separators = r.iter().filter(|x| **x == Row::Separator).count();
        assert_eq!(separators, 1, "one line under the list, as in the AutoCAD menu");
        assert_eq!(entries(&menu(false)).len(), r.len());
    }

    #[test]
    fn the_label_row_shows_its_state() {
        assert_eq!(rows(&menu(true)).last(), Some(&Row::Label { on: true }));
    }

    #[test]
    fn label_is_only_shown_when_asked() {
        assert!(label(&menu(false)).is_none());
        assert!(label(&menu(true)).is_some());
    }

    #[test]
    fn a_current_name_that_names_nothing_marks_autocad_classic() {
        let mut set = WorkspaceSet::default();
        set.current = "sparito".to_string();
        let m = WorkspaceMenu::from_set(&set);
        assert_eq!(m.current, AUTOCAD_CLASSIC);
        assert!(matches!(&rows(&m)[0], Row::Workspace { current: true, .. }));
    }

    /// Spike of the plan (Task 6): the menu opens in the headless simulator and
    /// a row publishes its message.
    #[test]
    fn clicking_a_row_of_the_open_menu_publishes_its_message() {
        let m = menu(false);
        let el = crate::ui::statusbar::status_menu::menu_bar(button(), entries(&m), 230.0);
        let mut ui = iced_test::simulator(el);
        ui.point_at(iced::Point::new(16.0, 16.0));
        ui.simulate([
            iced_core::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
            iced_core::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)),
        ]);
        ui.click(OPENCAD_STUDIO).expect("the menu shows OpenCAD Studio");
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(
            messages.iter().any(|m| matches!(
                m,
                Message::Workspace(WorkspaceMsg::Switch(n)) if n == OPENCAD_STUDIO
            )),
            "no Switch published: {messages:?}"
        );
        // Applied to an app, the click really changes the interface.
        let mut app = crate::app::OpenCADStudio::new_for_test();
        for message in messages {
            let _ = app.update(message);
        }
        assert!(!crate::workspace::is_classic());
        assert_eq!(app.workspaces.current, OPENCAD_STUDIO);
    }
}
