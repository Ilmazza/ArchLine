//! ArchLine: workspace actions (switch, save, delete, ...) behind the one
//! `Message::Workspace`. The model is `crate::workspaces`; the windows are
//! `ui/window/workspaces.rs`. Spec:
//! `docs/superpowers/specs/2026-10-20-workspace-design.md`.

use iced::Task;

use super::{Message, ModalKind, OpenCADStudio};
use crate::ui::toolbar_dock::ToolbarMsg;
use crate::ui::window::workspaces::{WorkspaceMsg, WorkspaceTab, WorkspaceUi};
use crate::workspaces::{Interface, NameError, AUTOCAD_CLASSIC};

impl OpenCADStudio {
    #[inline(never)]
    pub(super) fn on_workspace(&mut self, m: WorkspaceMsg) -> Task<Message> {
        match m {
            WorkspaceMsg::Switch(name) | WorkspaceMsg::SetCurrent(name) => {
                return self.apply_workspace(&name);
            }
            WorkspaceMsg::ToggleLabel => {
                self.workspaces.show_label = !self.workspaces.show_label;
                self.save_config();
            }
            WorkspaceMsg::ToggleAutoSave => {
                self.workspaces.auto_save = !self.workspaces.auto_save;
                self.save_config();
            }
            WorkspaceMsg::OpenSaveAs => {
                self.open_workspace_window(ModalKind::WorkspaceSaveAs, WorkspaceTab::Settings);
                self.check_workspace_name();
            }
            WorkspaceMsg::OpenSettings => {
                self.open_workspace_window(ModalKind::Workspaces, WorkspaceTab::Settings);
            }
            WorkspaceMsg::OpenCustomize => {
                self.open_workspace_window(ModalKind::Workspaces, WorkspaceTab::Customize);
            }
            WorkspaceMsg::Tab(tab) => {
                if let Some(ui) = self.workspace_ui.as_mut() {
                    ui.tab = tab;
                }
            }
            WorkspaceMsg::SaveAsName(name) => {
                if let Some(ui) = self.workspace_ui.as_mut() {
                    ui.save_name = name;
                    ui.confirm_replace = false;
                }
                self.check_workspace_name();
            }
            WorkspaceMsg::SaveAsApply => {
                let Some(ui) = self.workspace_ui.as_ref() else {
                    return Task::none();
                };
                match self.workspaces.validate_name(&ui.save_name, None) {
                    Ok(name) => self.save_workspace_as(&name),
                    // The name is one of the user's: ask before replacing it.
                    Err(NameError::Duplicate) => {
                        if let Some(ui) = self.workspace_ui.as_mut() {
                            ui.confirm_replace = true;
                        }
                    }
                    Err(e) => {
                        if let Some(ui) = self.workspace_ui.as_mut() {
                            ui.save_error = Some(e);
                        }
                    }
                }
            }
            WorkspaceMsg::SaveAsConfirmReplace => {
                let confirmed = self.workspace_ui.as_ref().filter(|ui| ui.confirm_replace);
                if let Some(ui) = confirmed {
                    let name = ui.save_name.trim().to_string();
                    self.save_workspace_as(&name);
                }
            }
            WorkspaceMsg::Delete(name) => {
                let was_current = self.workspaces.current.eq_ignore_ascii_case(&name);
                if !self.workspaces.delete(&name) {
                    return Task::none();
                }
                if was_current {
                    // The model already fell back to AutoCAD Classic: show it.
                    return self.apply_workspace(AUTOCAD_CLASSIC);
                }
                self.save_config();
            }
            WorkspaceMsg::Rename(old, new) => match self.workspaces.rename(&old, &new) {
                Ok(()) => {
                    if let Some(ui) = self.workspace_ui.as_mut() {
                        ui.rename = None;
                        ui.save_error = None;
                    }
                    self.save_config();
                }
                Err(e) => {
                    if let Some(ui) = self.workspace_ui.as_mut() {
                        ui.save_error = Some(e);
                    }
                }
            },
            WorkspaceMsg::MoveUp(name) => {
                self.workspaces.move_up(&name);
                self.save_config();
            }
            WorkspaceMsg::MoveDown(name) => {
                self.workspaces.move_down(&name);
                self.save_config();
            }
            // Same rule as the right-click list: at least one bar stays visible.
            WorkspaceMsg::ToggleBar(id) => {
                return self.update(Message::Toolbar(ToolbarMsg::Toggle(id)));
            }
            WorkspaceMsg::Restore => self.restore_workspace(),
        }
        Task::none()
    }

    fn open_workspace_window(&mut self, kind: ModalKind, tab: WorkspaceTab) {
        self.workspace_ui = Some(WorkspaceUi { tab, ..Default::default() });
        self.active_modal = Some(kind);
        self.reset_modal_geometry();
    }

    /// Re-check the name typed in "Save Workspace As": the error to show, if any.
    /// A name that is already one of the user's is not an error here (the window
    /// asks to replace it).
    fn check_workspace_name(&mut self) {
        let Some(ui) = self.workspace_ui.as_ref() else {
            return;
        };
        let error = match self.workspaces.validate_name(&ui.save_name, None) {
            Ok(_) | Err(NameError::Duplicate) => None,
            Err(e) => Some(e),
        };
        if let Some(ui) = self.workspace_ui.as_mut() {
            ui.save_error = error;
        }
    }

    /// The interface the window is showing now, as a workspace would store it.
    fn interface_now() -> Interface {
        if crate::workspace::is_classic() {
            Interface::Classic
        } else {
            Interface::Ribbon
        }
    }

    fn save_workspace_as(&mut self, name: &str) {
        self.workspaces
            .save(name, Self::interface_now(), self.toolbars.clone());
        self.close_active_modal();
        self.command_line
            .push_info(crate::tf!("Workspace '{name}' saved.").as_ref());
        self.save_config();
    }

    /// Reload the current workspace's saved arrangement into the working layout
    /// (a built-in comes from code, a user's from the last save).
    fn restore_workspace(&mut self) {
        let ws = self.workspaces.resolved();
        if ws.interface == Interface::Classic {
            self.toolbars = ws.toolbars.sanitized();
        }
        self.toolbar_drag = None;
        self.save_config();
    }

    /// Make `name` the current workspace: its interface, and for a classic one
    /// its arrangement of bars. Does nothing for a name that is not a workspace.
    #[inline(never)]
    fn apply_workspace(&mut self, name: &str) -> Task<Message> {
        let Some(target) = self.workspaces.get(name) else {
            return Task::none();
        };
        // Leaving the current workspace: its arrangement is kept only with auto
        // save on and only for a workspace of the user. Otherwise say what is lost.
        let leaving = self.workspaces.resolved();
        let loses_changes = leaving.toolbars != self.toolbars
            && !(self.workspaces.auto_save && !crate::workspaces::WorkspaceSet::is_builtin(&leaving.name));
        self.workspaces.leave(&self.toolbars);

        // Transient state of the interface being left.
        self.toolbar_drag = None;
        self.tool_hold = None;
        self.ribbon.close_dropdown();
        self.dock_edge_menu = None;
        let escape = if self.tabs[self.active_tab].active_cmd.is_some() {
            self.on_command_escape()
        } else {
            Task::none()
        };

        crate::workspace::set_classic(target.interface == Interface::Classic);
        if target.interface == Interface::Classic {
            self.toolbars = target.toolbars.sanitized();
        }
        self.workspaces.current = target.name;
        if loses_changes {
            self.command_line.push_info(
                crate::t!("Toolbar changes were not saved. Use Save Current As… in the workspace menu to keep them.")
                    .as_ref(),
            );
        }
        self.save_config();
        escape
    }
}

#[cfg(test)]
mod tests {
    use crate::app::config::AppConfig;
    use crate::app::{Message, OpenCADStudio};
    use crate::ui::toolbar_dock::ToolbarDrag;
    use crate::ui::toolbar_layout::{Target, ToolbarId, ToolbarLayout};
    use crate::ui::window::workspaces::{WorkspaceMsg as W, WorkspaceTab};
    use crate::workspaces::{
        Interface, NameError, WorkspaceSet, AUTOCAD_CLASSIC, MY_WORKSPACE, OPENCAD_STUDIO,
    };

    fn app() -> OpenCADStudio {
        OpenCADStudio::new_for_test()
    }

    fn send(app: &mut OpenCADStudio, m: W) {
        let _ = app.update(Message::Workspace(m));
    }

    /// The working layout with Draw floated: different from every built-in.
    fn edited() -> ToolbarLayout {
        let mut l = ToolbarLayout::autocad_classic();
        l.move_to(ToolbarId::Draw, Target::Float { x: 40.0, y: 90.0 });
        l
    }

    #[test]
    fn apply_config_runs_first_run_and_sets_the_interface() {
        let mut app = app();
        // A custom layout and no workspaces yet: kept as "My Workspace".
        let mut cfg = AppConfig::default();
        cfg.toolbars
            .move_to(ToolbarId::Draw, Target::Float { x: 40.0, y: 90.0 });
        cfg.workspaces = WorkspaceSet::default();
        app.apply_config(cfg);
        assert_eq!(app.workspaces.current, MY_WORKSPACE);
        assert!(crate::workspace::is_classic());
        // A saved OpenCAD Studio turns the ribbon on.
        let mut cfg = AppConfig::default();
        cfg.workspaces.current = OPENCAD_STUDIO.to_string();
        app.apply_config(cfg);
        assert!(!crate::workspace::is_classic());
        // And back.
        let mut cfg = AppConfig::default();
        cfg.workspaces.current = AUTOCAD_CLASSIC.to_string();
        app.apply_config(cfg);
        assert!(crate::workspace::is_classic());
    }

    #[test]
    fn apply_config_keeps_the_working_toolbars() {
        let mut app = app();
        let mut cfg = AppConfig::default();
        cfg.workspaces.current = AUTOCAD_CLASSIC.to_string();
        // The working layout differs from the saved AutoCAD Classic one.
        cfg.toolbars = edited();
        app.apply_config(cfg.clone());
        assert_eq!(app.toolbars, cfg.toolbars.sanitized());
    }

    #[test]
    fn current_config_carries_the_workspaces() {
        let mut app = app();
        app.apply_config(AppConfig::default());
        let cfg = app.current_config();
        assert_eq!(cfg.workspaces.current, AUTOCAD_CLASSIC);
        assert_eq!(cfg.workspaces, app.workspaces);
        assert_eq!(app.workspaces.resolved().interface, Interface::Classic);
    }

    #[test]
    fn switch_to_opencad_studio_turns_the_ribbon_on() {
        let mut app = app();
        let before = app.toolbars.clone();
        send(&mut app, W::Switch(OPENCAD_STUDIO.to_string()));
        assert!(!crate::workspace::is_classic());
        assert_eq!(app.workspaces.current, OPENCAD_STUDIO);
        assert_eq!(app.toolbars, before, "the ribbon does not touch the bars");
        assert_eq!(app.current_config().workspaces.current, OPENCAD_STUDIO);
    }

    #[test]
    fn switch_back_restores_the_classic_layout() {
        let mut app = app();
        app.toolbars = edited();
        send(&mut app, W::Switch(OPENCAD_STUDIO.to_string()));
        send(&mut app, W::Switch(AUTOCAD_CLASSIC.to_string()));
        assert!(crate::workspace::is_classic());
        assert_eq!(app.toolbars, ToolbarLayout::autocad_classic());
    }

    #[test]
    fn switch_clears_a_toolbar_drag_in_progress() {
        let mut app = app();
        app.toolbar_drag = Some(ToolbarDrag {
            id: ToolbarId::Draw,
            origin: None,
            cursor: None,
            target: None,
        });
        send(&mut app, W::Switch(OPENCAD_STUDIO.to_string()));
        assert!(app.toolbar_drag.is_none());
    }

    #[test]
    fn switch_works_on_the_start_tab_and_with_clean_screen() {
        let mut app = app();
        app.clean_screen = true;
        send(&mut app, W::Switch(OPENCAD_STUDIO.to_string()));
        assert!(!crate::workspace::ribbon_shown(false, true), "clean screen still hides it");
        send(&mut app, W::Switch(AUTOCAD_CLASSIC.to_string()));
        app.clean_screen = false;
        // The bars of the classic interface still build.
        let _ = crate::ui::toolbar_dock::frame(
            true,
            &app.toolbars,
            &app.ribbon,
            None,
            900.0,
            iced::widget::Space::new().into(),
        );
        assert!(crate::workspace::classic_active(false, false));
        assert!(!crate::workspace::classic_active(true, false), "the Start tab has no bars");
    }

    #[test]
    fn switch_with_auto_save_on_writes_back_a_user_workspace_not_a_builtin() {
        let mut app = app();
        app.workspaces
            .save("Mio", Interface::Classic, ToolbarLayout::autocad_classic());
        app.workspaces.auto_save = true;
        app.toolbars = edited();
        send(&mut app, W::Switch(AUTOCAD_CLASSIC.to_string()));
        assert_eq!(app.workspaces.get("Mio").unwrap().toolbars, edited());
        // From a built-in nothing is written and nothing is created.
        app.toolbars = edited();
        send(&mut app, W::Switch(OPENCAD_STUDIO.to_string()));
        assert_eq!(app.workspaces.saved.len(), 1);
        assert_eq!(
            app.workspaces.get(AUTOCAD_CLASSIC).unwrap().toolbars,
            ToolbarLayout::autocad_classic()
        );
    }

    #[test]
    fn switch_to_a_missing_name_changes_nothing() {
        let mut app = app();
        let before = (app.workspaces.clone(), app.toolbars.clone());
        send(&mut app, W::Switch("non esiste".to_string()));
        assert_eq!((app.workspaces.clone(), app.toolbars.clone()), before);
        assert!(crate::workspace::is_classic());
    }

    #[test]
    fn switch_cancels_a_running_command_and_clears_menus() {
        let mut app = app();
        // The Start tab blocks drawing commands: open a drawing first.
        app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("LINE");
        assert!(app.tabs[app.active_tab].active_cmd.is_some(), "LINE is running");
        app.ribbon.open_dropdown = Some("LAYER_COMBO".to_string());
        app.dock_edge_menu = Some(crate::app::config::DockSide::Right);
        send(&mut app, W::Switch(OPENCAD_STUDIO.to_string()));
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        assert!(app.ribbon.open_dropdown.is_none());
        assert!(app.dock_edge_menu.is_none());
    }

    #[test]
    fn leaving_a_builtin_with_unsaved_bar_changes_prints_a_notice() {
        let mut app = app();
        let start = app.command_line.history.len();
        send(&mut app, W::Switch(OPENCAD_STUDIO.to_string()));
        assert_eq!(app.command_line.history.len(), start, "no changes, no notice");
        send(&mut app, W::Switch(AUTOCAD_CLASSIC.to_string()));
        app.toolbars = edited();
        let start = app.command_line.history.len();
        send(&mut app, W::Switch(OPENCAD_STUDIO.to_string()));
        let notice = &app.command_line.history[start..];
        assert!(
            notice.iter().any(|e| e.text.contains("Save Current As")),
            "no notice: {:?}",
            notice.iter().map(|e| e.text.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn save_as_with_a_valid_name_adds_and_selects_it() {
        let mut app = app();
        app.toolbars = edited();
        send(&mut app, W::OpenSaveAs);
        assert_eq!(app.active_modal, Some(crate::app::ModalKind::WorkspaceSaveAs));
        send(&mut app, W::SaveAsName("  Rilievi  ".to_string()));
        assert_eq!(app.workspace_ui.as_ref().unwrap().save_error, None);
        send(&mut app, W::SaveAsApply);
        assert!(app.workspace_ui.is_none() && app.active_modal.is_none(), "the window closes");
        assert_eq!(app.workspaces.current, "Rilievi");
        let saved = app.workspaces.get("Rilievi").unwrap();
        assert_eq!(saved.toolbars, edited());
        assert_eq!(saved.interface, Interface::Classic);
    }

    #[test]
    fn save_as_with_an_invalid_name_sets_the_error_and_saves_nothing() {
        let mut app = app();
        send(&mut app, W::OpenSaveAs);
        assert_eq!(app.workspace_ui.as_ref().unwrap().save_error, Some(NameError::Empty));
        send(&mut app, W::SaveAsApply);
        assert!(app.workspaces.saved.is_empty());
        assert!(app.workspace_ui.is_some(), "the window stays open");
        send(&mut app, W::SaveAsName("autocad classic".to_string()));
        assert_eq!(app.workspace_ui.as_ref().unwrap().save_error, Some(NameError::Reserved));
        send(&mut app, W::SaveAsApply);
        assert!(app.workspaces.saved.is_empty());
    }

    #[test]
    fn save_as_an_existing_name_asks_before_replacing() {
        let mut app = app();
        app.workspaces
            .save("Mio", Interface::Classic, ToolbarLayout::autocad_classic());
        app.toolbars = edited();
        send(&mut app, W::OpenSaveAs);
        send(&mut app, W::SaveAsName("MIO".to_string()));
        assert_eq!(app.workspace_ui.as_ref().unwrap().save_error, None);
        send(&mut app, W::SaveAsApply);
        assert!(app.workspace_ui.as_ref().unwrap().confirm_replace);
        assert_eq!(
            app.workspaces.get("Mio").unwrap().toolbars,
            ToolbarLayout::autocad_classic(),
            "nothing changes before the confirmation"
        );
        send(&mut app, W::SaveAsConfirmReplace);
        assert_eq!(app.workspaces.saved.len(), 1);
        assert_eq!(app.workspaces.get("Mio").unwrap().toolbars, edited());
        assert!(app.workspace_ui.is_none());
    }

    #[test]
    fn delete_the_current_applies_autocad_classic() {
        let mut app = app();
        app.workspaces.save("Mio", Interface::Ribbon, edited());
        crate::workspace::set_classic(false);
        send(&mut app, W::Delete("Mio".to_string()));
        assert!(app.workspaces.saved.is_empty());
        assert_eq!(app.workspaces.current, AUTOCAD_CLASSIC);
        assert!(crate::workspace::is_classic(), "the interface really changes");
        // A built-in cannot be deleted.
        send(&mut app, W::Delete(AUTOCAD_CLASSIC.to_string()));
        assert_eq!(app.workspaces.current, AUTOCAD_CLASSIC);
    }

    #[test]
    fn rename_and_move_keep_the_set_consistent() {
        let mut app = app();
        app.workspaces.save("Uno", Interface::Classic, edited());
        app.workspaces.save("Due", Interface::Classic, edited());
        send(&mut app, W::Rename("Due".to_string(), "Tre".to_string()));
        assert_eq!(app.workspaces.current, "Tre");
        send(&mut app, W::MoveUp("Tre".to_string()));
        assert_eq!(app.workspaces.names()[2], "Tre");
        send(&mut app, W::OpenSettings);
        send(&mut app, W::Rename("Tre".to_string(), AUTOCAD_CLASSIC.to_string()));
        assert_eq!(app.workspace_ui.as_ref().unwrap().save_error, Some(NameError::Reserved));
        assert_eq!(app.workspaces.current, "Tre");
    }

    #[test]
    fn restore_reloads_the_saved_layout() {
        let mut app = app();
        app.toolbars = edited();
        send(&mut app, W::Restore);
        assert_eq!(app.toolbars, ToolbarLayout::autocad_classic(), "a built-in reloads from code");
        app.workspaces.save("Mio", Interface::Classic, edited());
        app.toolbars = ToolbarLayout::autocad_classic();
        send(&mut app, W::Restore);
        assert_eq!(app.toolbars, edited().sanitized(), "a user workspace, from its last save");
    }

    #[test]
    fn toggle_bar_hides_and_shows_in_the_working_layout() {
        let mut app = app();
        let id = ToolbarId::from_title("Standard").unwrap();
        assert!(app.toolbars.is_visible(id));
        send(&mut app, W::ToggleBar(id));
        assert!(!app.toolbars.is_visible(id));
        send(&mut app, W::ToggleBar(id));
        assert!(app.toolbars.is_visible(id));
    }

    #[test]
    fn toggle_label_and_auto_save_flip_and_persist() {
        let mut app = app();
        assert!(!app.workspaces.show_label && !app.workspaces.auto_save);
        send(&mut app, W::ToggleLabel);
        send(&mut app, W::ToggleAutoSave);
        let cfg = app.current_config();
        assert!(cfg.workspaces.show_label && cfg.workspaces.auto_save);
        send(&mut app, W::ToggleLabel);
        assert!(!app.current_config().workspaces.show_label);
    }

    #[test]
    fn the_tab_message_changes_the_open_window() {
        let mut app = app();
        send(&mut app, W::OpenCustomize);
        assert_eq!(app.active_modal, Some(crate::app::ModalKind::Workspaces));
        assert_eq!(app.workspace_ui.as_ref().unwrap().tab, WorkspaceTab::Customize);
        send(&mut app, W::Tab(WorkspaceTab::Settings));
        assert_eq!(app.workspace_ui.as_ref().unwrap().tab, WorkspaceTab::Settings);
    }
}
