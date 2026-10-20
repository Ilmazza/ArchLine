//! ArchLine: messages and state of the workspace windows ("Save Workspace As"
//! and the two-tab "Workspaces" window). The model is `crate::workspaces`; the
//! actions are `app/workspaces.rs`.

use crate::ui::toolbar_layout::ToolbarId;
use crate::workspaces::NameError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorkspaceTab {
    #[default]
    Settings,
    Customize,
}

/// Everything the workspace UI can ask for; one `Message::Workspace` carries it.
#[derive(Clone, Debug)]
pub enum WorkspaceMsg {
    /// Make the workspace called so the current one (the menu rows).
    Switch(String),
    ToggleLabel,
    ToggleAutoSave,
    OpenSaveAs,
    OpenSettings,
    OpenCustomize,
    /// The name typed in "Save Workspace As".
    SaveAsName(String),
    SaveAsApply,
    SaveAsConfirmReplace,
    Delete(String),
    Rename(String, String),
    /// Start renaming the row, with its name in the field.
    RenameStart(String),
    /// The text typed in the rename field.
    RenameEdit(String),
    RenameCancel,
    MoveUp(String),
    MoveDown(String),
    /// "Set current" in the Settings tab.
    SetCurrent(String),
    /// A bar's box in the Customize tab.
    ToggleBar(ToolbarId),
    /// Reload the current workspace's saved arrangement.
    Restore,
    Tab(WorkspaceTab),
}

/// State of the open workspace window; `None` while none is open.
#[derive(Clone, Debug, Default)]
pub struct WorkspaceUi {
    pub tab: WorkspaceTab,
    /// The name being typed in "Save Workspace As".
    pub save_name: String,
    /// Why the typed name cannot be used; `Duplicate` is not shown here (it
    /// asks to replace instead).
    pub save_error: Option<NameError>,
    /// The typed name is a workspace of the user: waiting for the user to
    /// confirm replacing it.
    pub confirm_replace: bool,
    /// A row being renamed: (old name, text typed).
    pub rename: Option<(String, String)>,
}

// ── Views ─────────────────────────────────────────────────────────────────

use iced::widget::{button, checkbox, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Fill};

use crate::app::Message;
use crate::t;
use crate::ui::modal::ModalSizing;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{dialog_button, dialog_button_styled_opt, field_style, hdivider};
use crate::ui::toolbar_dock::BarEntry;
use crate::workspaces::WorkspaceSet;

fn msg(m: WorkspaceMsg) -> Message {
    Message::Workspace(m)
}

/// The sentence for a refused name.
pub fn name_error_text(e: NameError) -> String {
    match e {
        NameError::Empty => t!("Enter a name.").into_owned(),
        NameError::TooLong => {
            let max = crate::workspaces::MAX_NAME;
            crate::tf!("The name is too long ({max} characters at most).").to_string()
        }
        NameError::Reserved => t!("That name belongs to a built-in workspace.").into_owned(),
        NameError::Duplicate => t!("Another workspace already has that name.").into_owned(),
    }
}

/// "Save Workspace As": a name, OK and Cancel; asks before replacing.
pub(crate) fn view_save_as(ui: &WorkspaceUi, sizing: ModalSizing) -> Element<'_, Message> {
    let can_ok = !ui.save_name.trim().is_empty() && ui.save_error.is_none();
    let field = text_input(t!("Workspace name").as_ref(), &ui.save_name)
        .on_input(|n| msg(WorkspaceMsg::SaveAsName(n)))
        .on_submit_maybe(
            (can_ok && !ui.confirm_replace).then(|| msg(WorkspaceMsg::SaveAsApply)),
        )
        .style(field_style);
    let note: String = if ui.confirm_replace {
        t!("A workspace with this name exists. Replace it?").into_owned()
    } else {
        ui.save_error.map(name_error_text).unwrap_or_default()
    };
    let ok = if ui.confirm_replace {
        dialog_button(t!("Replace"), msg(WorkspaceMsg::SaveAsConfirmReplace), true)
    } else {
        dialog_button_styled_opt(
            t!("OK"),
            can_ok.then(|| msg(WorkspaceMsg::SaveAsApply)),
            button::primary,
        )
    };
    container(
        column![
            text(t!("Save the current arrangement of menus and toolbars as:").into_owned()).size(12),
            field,
            text(note).size(11).style(text::danger),
            Space::new().height(Fill),
            row![
                Space::new().width(Fill),
                ok,
                dialog_button(t!("Cancel"), Message::CloseModal, false),
            ]
            .spacing(8),
        ]
        .spacing(10),
    )
    .padding(16)
    .width(sizing.width)
    .height(sizing.height)
    .into()
}

fn tab_button(label: String, tab: WorkspaceTab, current: WorkspaceTab) -> Element<'static, Message> {
    button(text(label).size(12))
        .on_press(msg(WorkspaceMsg::Tab(tab)))
        .padding([5, 16])
        .style(if tab == current { button::primary } else { button::subtle })
        .into()
}

fn small_button(label: &'static str, m: WorkspaceMsg) -> Element<'static, Message> {
    button(text(label).size(11))
        .on_press(msg(m))
        .padding([3, 8])
        .style(button::secondary)
        .into()
}

fn settings_rows<'a>(ui: &'a WorkspaceUi, set: &WorkspaceSet) -> Vec<Element<'a, Message>> {
    let current = set.resolved().name;
    set.names()
        .into_iter()
        .map(|name| {
            let is_current = name.eq_ignore_ascii_case(&current);
            let builtin = WorkspaceSet::is_builtin(&name);
            let renaming = ui
                .rename
                .as_ref()
                .filter(|(old, _)| old.eq_ignore_ascii_case(&name));
            let label: Element<'a, Message> = match renaming {
                Some((old, typed)) => {
                    let old = old.clone();
                    let new = typed.clone();
                    row![
                        text_input("", typed)
                            .on_input(|n| msg(WorkspaceMsg::RenameEdit(n)))
                            .on_submit(msg(WorkspaceMsg::Rename(old.clone(), new.clone())))
                            .style(field_style)
                            .width(Fill),
                        small_button("OK", WorkspaceMsg::Rename(old, new)),
                        small_button("Cancel", WorkspaceMsg::RenameCancel),
                    ]
                    .spacing(4)
                    .into()
                }
                None => {
                    let mut label = name.clone();
                    if is_current {
                        label.push_str("  ✓");
                    }
                    let note = if builtin { t!("built-in").into_owned() } else { String::new() };
                    row![
                        text(label).size(12).width(Fill),
                        text(note).size(11).style(muted_style),
                    ]
                    .spacing(8)
                    .into()
                }
            };
            let mut actions = row![].spacing(4);
            if !is_current {
                actions = actions.push(small_button("Set current", WorkspaceMsg::SetCurrent(name.clone())));
            }
            if !builtin && renaming.is_none() {
                actions = actions
                    .push(small_button("Rename", WorkspaceMsg::RenameStart(name.clone())))
                    .push(small_button("Delete", WorkspaceMsg::Delete(name.clone())))
                    .push(small_button("▲", WorkspaceMsg::MoveUp(name.clone())))
                    .push(small_button("▼", WorkspaceMsg::MoveDown(name.clone())));
            }
            container(row![label, actions].spacing(8).align_y(iced::Center))
                .padding([3, 6])
                .into()
        })
        .collect()
}

fn customize_content<'a>(bars: &[BarEntry], classic: bool) -> Element<'a, Message> {
    if !classic {
        return text(
            t!("The OpenCAD Studio workspace shows the ribbon; toolbars are not displayed. Switch to a classic workspace to choose them.")
                .into_owned(),
        )
        .size(12)
        .style(muted_style)
        .into();
    }
    let rows: Vec<Element<'a, Message>> = bars
        .iter()
        .map(|b| {
            let id = b.id;
            checkbox(b.visible)
                .label(t!(b.name).into_owned())
                .on_toggle(move |_| msg(WorkspaceMsg::ToggleBar(id)))
                .size(14)
                .text_size(12)
                .into()
        })
        .collect();
    column![
        text(t!("Toolbars shown in this workspace:").into_owned()).size(12),
        scrollable(column(rows).spacing(4).padding(4)).height(Fill),
    ]
    .spacing(6)
    .height(Fill)
    .into()
}

/// The two-tab window: Settings (list and options) and Customize (bars).
pub(crate) fn view_window<'a>(
    ui: &'a WorkspaceUi,
    set: &WorkspaceSet,
    bars: &[BarEntry],
    classic: bool,
    sizing: ModalSizing,
) -> Element<'a, Message> {
    let tabs = row![
        tab_button(t!("Settings").into_owned(), WorkspaceTab::Settings, ui.tab),
        tab_button(t!("Customize").into_owned(), WorkspaceTab::Customize, ui.tab),
    ]
    .spacing(4);
    let body: Element<'a, Message> = match ui.tab {
        WorkspaceTab::Settings => {
            let error = ui.save_error.map(name_error_text).unwrap_or_default();
            column![
                text(t!("Workspaces").into_owned()).size(12),
                scrollable(column(settings_rows(ui, set)).spacing(2)).height(Fill),
                text(error).size(11).style(text::danger),
                checkbox(set.auto_save)
                    .label(t!("Save changes automatically when switching workspace").into_owned())
                    .on_toggle(|_| msg(WorkspaceMsg::ToggleAutoSave))
                    .size(14)
                    .text_size(12),
            ]
            .spacing(8)
            .height(Fill)
            .into()
        }
        WorkspaceTab::Customize => customize_content(bars, classic),
    };
    let mut footer = row![Space::new().width(Fill)].spacing(8);
    if ui.tab == WorkspaceTab::Customize && classic {
        footer = footer.push(dialog_button(t!("Restore workspace"), msg(WorkspaceMsg::Restore), false));
    }
    footer = footer.push(dialog_button(t!("Close"), Message::CloseModal, true));
    container(column![tabs, hdivider(Fill), body, footer].spacing(10))
        .padding(16)
        .width(sizing.width)
        .height(sizing.height)
        .into()
}

