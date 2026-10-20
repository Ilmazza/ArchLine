//! ArchLine: workspaces, the saved arrangements of the interface (menu and
//! toolbars) switched from the status bar. Pure model, no iced: the app glue is
//! `app/workspaces.rs`. Spec: `docs/superpowers/specs/2026-10-20-workspace-design.md`.
//!
//! The two built-in workspaces are built from code, never written to
//! `settings.json`, so an update can correct them. Only the user's own are
//! serialized, in the `workspaces` section of the app config.

use serde::{Deserialize, Serialize};

use crate::ui::toolbar_layout::ToolbarLayout;

pub const AUTOCAD_CLASSIC: &str = "AutoCAD Classic";
pub const OPENCAD_STUDIO: &str = "OpenCAD Studio";
/// Where the layout a user had before workspaces existed is kept.
pub const MY_WORKSPACE: &str = "My Workspace";
/// Longest workspace name, in characters.
pub const MAX_NAME: usize = 40;

/// The interface a workspace shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interface {
    /// Menu bar and toolbars.
    Classic,
    /// The upstream ribbon.
    Ribbon,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub interface: Interface,
    /// Kept with the `Ribbon` interface too, but not shown there.
    pub toolbars: ToolbarLayout,
}

/// The two workspaces every install has, in menu order.
pub fn builtin() -> [Workspace; 2] {
    [
        Workspace {
            name: AUTOCAD_CLASSIC.to_string(),
            interface: Interface::Classic,
            toolbars: ToolbarLayout::autocad_classic(),
        },
        Workspace {
            name: OPENCAD_STUDIO.to_string(),
            interface: Interface::Ribbon,
            toolbars: ToolbarLayout::autocad_classic(),
        },
    ]
}

/// Why a workspace name was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    /// The name of a built-in workspace.
    Reserved,
    /// Another workspace of the user already has it.
    Duplicate,
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// The user's workspaces and the choices about them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceSet {
    /// Name of the workspace in use; empty until the first run after workspaces
    /// were introduced (see [`WorkspaceSet::first_run`]).
    pub current: String,
    /// The user's own workspaces, in menu order. The built-ins are not here.
    pub saved: Vec<Workspace>,
    /// Write the working layout back into the workspace being left (user
    /// workspaces only). Off by default, as in AutoCAD.
    pub auto_save: bool,
    /// Show the current workspace's name next to the status-bar button.
    pub show_label: bool,
}

impl WorkspaceSet {
    /// Every name in menu order: the built-ins, then the user's.
    pub fn names(&self) -> Vec<String> {
        builtin()
            .into_iter()
            .map(|w| w.name)
            .chain(self.saved.iter().map(|w| w.name.clone()))
            .collect()
    }

    pub fn is_builtin(name: &str) -> bool {
        same(name, AUTOCAD_CLASSIC) || same(name, OPENCAD_STUDIO)
    }

    /// The workspace called `name` (case-insensitive), built-ins included.
    pub fn get(&self, name: &str) -> Option<Workspace> {
        builtin()
            .into_iter()
            .chain(self.saved.iter().cloned())
            .find(|w| same(&w.name, name))
    }

    /// The workspace in use; AutoCAD Classic when `current` names none.
    pub fn resolved(&self) -> Workspace {
        self.get(&self.current)
            .unwrap_or_else(|| builtin().into_iter().next().expect("two built-ins"))
    }

    /// The trimmed `name` if it can be given to a workspace of the user.
    /// `replacing` is the workspace being renamed: its own name is allowed.
    pub fn validate_name(&self, name: &str, replacing: Option<&str>) -> Result<String, NameError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if name.chars().count() > MAX_NAME {
            return Err(NameError::TooLong);
        }
        if Self::is_builtin(name) {
            return Err(NameError::Reserved);
        }
        let taken = self.saved.iter().any(|w| same(&w.name, name));
        if taken && !replacing.is_some_and(|r| same(r, name)) {
            return Err(NameError::Duplicate);
        }
        Ok(name.to_string())
    }

    /// Store the arrangement as `name` (already validated), replacing a
    /// workspace of that name in place, and make it the current one.
    pub fn save(&mut self, name: &str, interface: Interface, toolbars: ToolbarLayout) {
        let ws = Workspace { name: name.to_string(), interface, toolbars };
        match self.saved.iter().position(|w| same(&w.name, name)) {
            Some(i) => self.saved[i] = ws,
            None => self.saved.push(ws),
        }
        self.current = name.to_string();
    }

    /// Remove a workspace of the user. `false` for a built-in or an unknown
    /// name. Deleting the current one falls back to AutoCAD Classic.
    pub fn delete(&mut self, name: &str) -> bool {
        let Some(i) = self.saved.iter().position(|w| same(&w.name, name)) else {
            return false;
        };
        self.saved.remove(i);
        if same(&self.current, name) {
            self.current = AUTOCAD_CLASSIC.to_string();
        }
        true
    }

    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), NameError> {
        let new = self.validate_name(new, Some(old))?;
        if let Some(w) = self.saved.iter_mut().find(|w| same(&w.name, old)) {
            if same(&self.current, old) {
                self.current = new.clone();
            }
            w.name = new;
        }
        Ok(())
    }

    pub fn move_up(&mut self, name: &str) {
        if let Some(i) = self.saved.iter().position(|w| same(&w.name, name)) {
            if i > 0 {
                self.saved.swap(i, i - 1);
            }
        }
    }

    pub fn move_down(&mut self, name: &str) {
        if let Some(i) = self.saved.iter().position(|w| same(&w.name, name)) {
            if i + 1 < self.saved.len() {
                self.saved.swap(i, i + 1);
            }
        }
    }

    /// Called when leaving the current workspace: with `auto_save` on, a user
    /// workspace takes the working layout. A built-in is never written.
    pub fn leave(&mut self, working: &ToolbarLayout) {
        if !self.auto_save {
            return;
        }
        let current = self.current.clone();
        if let Some(w) = self.saved.iter_mut().find(|w| same(&w.name, &current)) {
            w.toolbars = working.clone();
        }
    }

    /// First run after workspaces were introduced (`current` still empty): keep
    /// what the user had. A layout that is not AutoCAD Classic's becomes the
    /// user workspace "My Workspace" (as the Ribbon interface when
    /// `ARCHLINE_WORKSPACE=ribbon` asked for it); otherwise the built-in that
    /// matches. Does nothing once `current` is set.
    pub fn first_run(&mut self, working: &ToolbarLayout, env_ribbon: bool) {
        if !self.current.is_empty() {
            return;
        }
        if *working == ToolbarLayout::autocad_classic() {
            self.current = if env_ribbon { OPENCAD_STUDIO } else { AUTOCAD_CLASSIC }.to_string();
            return;
        }
        let interface = if env_ribbon { Interface::Ribbon } else { Interface::Classic };
        self.save(MY_WORKSPACE, interface, working.clone());
    }

    /// For a set read from a file that may have been edited by hand: drop
    /// empty, reserved and repeated names, cut long ones, and send a `current`
    /// that names nothing back to AutoCAD Classic. An empty `current` stays
    /// empty (it means the first run).
    pub fn sanitized(mut self) -> Self {
        let mut kept: Vec<Workspace> = Vec::new();
        for mut w in std::mem::take(&mut self.saved) {
            let name: String = w.name.trim().chars().take(MAX_NAME).collect();
            let name = name.trim().to_string();
            if name.is_empty()
                || Self::is_builtin(&name)
                || kept.iter().any(|k| same(&k.name, &name))
            {
                continue;
            }
            w.name = name;
            kept.push(w);
        }
        self.saved = kept;
        if !self.current.is_empty() && self.get(&self.current).is_none() {
            self.current = AUTOCAD_CLASSIC.to_string();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::toolbar_layout::{Target, ToolbarId};

    fn custom() -> ToolbarLayout {
        let mut l = ToolbarLayout::autocad_classic();
        l.move_to(ToolbarId::Draw, Target::Float { x: 40.0, y: 90.0 });
        l
    }

    fn with_user(name: &str) -> WorkspaceSet {
        let mut set = WorkspaceSet::default();
        set.save(name, Interface::Classic, custom());
        set
    }

    #[test]
    fn builtin_classic_matches_autocad_classic_layout() {
        let [classic, _] = builtin();
        assert_eq!(classic.name, AUTOCAD_CLASSIC);
        assert_eq!(classic.interface, Interface::Classic);
        assert_eq!(classic.toolbars, ToolbarLayout::autocad_classic());
    }

    #[test]
    fn builtin_ribbon_is_ribbon_interface() {
        let [_, studio] = builtin();
        assert_eq!(studio.name, OPENCAD_STUDIO);
        assert_eq!(studio.interface, Interface::Ribbon);
    }

    #[test]
    fn validate_name_trims_and_rejects() {
        let set = with_user("Mio");
        assert_eq!(set.validate_name("", None), Err(NameError::Empty));
        assert_eq!(set.validate_name("   ", None), Err(NameError::Empty));
        assert_eq!(set.validate_name(&"x".repeat(41), None), Err(NameError::TooLong));
        assert_eq!(set.validate_name(&"x".repeat(40), None), Ok("x".repeat(40)));
        assert_eq!(set.validate_name("autocad classic", None), Err(NameError::Reserved));
        assert_eq!(set.validate_name("OPENCAD STUDIO", None), Err(NameError::Reserved));
        assert_eq!(set.validate_name("MIO", None), Err(NameError::Duplicate));
        assert_eq!(set.validate_name("MIO", Some("Mio")), Ok("MIO".to_string()));
        assert_eq!(set.validate_name("  Nuovo  ", None), Ok("Nuovo".to_string()));
    }

    #[test]
    fn save_replaces_same_name_and_sets_current() {
        let mut set = with_user("Mio");
        set.save("MIO", Interface::Ribbon, ToolbarLayout::default());
        assert_eq!(set.saved.len(), 1);
        assert_eq!(set.saved[0].name, "MIO");
        assert_eq!(set.saved[0].interface, Interface::Ribbon);
        assert_eq!(set.current, "MIO");
    }

    #[test]
    fn delete_current_falls_back_to_autocad_classic() {
        let mut set = with_user("Mio");
        assert_eq!(set.current, "Mio");
        assert!(set.delete("mio"));
        assert!(set.saved.is_empty());
        assert_eq!(set.current, AUTOCAD_CLASSIC);
    }

    #[test]
    fn delete_refuses_a_builtin() {
        let mut set = with_user("Mio");
        assert!(!set.delete(AUTOCAD_CLASSIC));
        assert!(!set.delete(OPENCAD_STUDIO));
        assert!(!set.delete("non esiste"));
        assert_eq!(set.saved.len(), 1);
    }

    #[test]
    fn missing_current_resolves_to_autocad_classic() {
        let mut set = with_user("Mio");
        set.current = "sparito".to_string();
        assert_eq!(set.resolved().name, AUTOCAD_CLASSIC);
        set.current = "mio".to_string();
        assert_eq!(set.resolved().name, "Mio");
    }

    #[test]
    fn leave_with_auto_save_updates_a_user_workspace_only() {
        let working = ToolbarLayout::default();
        // On, on a user workspace: it takes the working layout, none is added.
        let mut set = with_user("Mio");
        set.auto_save = true;
        set.leave(&working);
        assert_eq!(set.saved.len(), 1);
        assert_eq!(set.saved[0].toolbars, working);
        // On, on a built-in: nothing is created or overwritten.
        let mut set = with_user("Mio");
        set.auto_save = true;
        set.current = AUTOCAD_CLASSIC.to_string();
        let before = set.clone();
        set.leave(&working);
        assert_eq!(set, before);
        assert_eq!(set.resolved().toolbars, ToolbarLayout::autocad_classic());
        // Off: nothing changes.
        let mut set = with_user("Mio");
        let before = set.clone();
        set.leave(&working);
        assert_eq!(set, before);
    }

    #[test]
    fn old_settings_without_section_deserialises() {
        let set: WorkspaceSet = serde_json::from_str("{}").unwrap();
        assert_eq!(set, WorkspaceSet::default());
        assert!(set.current.is_empty());
        assert!(!set.auto_save && !set.show_label);
    }

    #[test]
    fn round_trip_serde() {
        let mut set = with_user("Mio");
        set.auto_save = true;
        set.show_label = true;
        let back: WorkspaceSet = serde_json::from_str(&serde_json::to_string(&set).unwrap()).unwrap();
        assert_eq!(back, set);
    }

    #[test]
    fn first_run_keeps_a_custom_layout_as_my_workspace() {
        let mut set = WorkspaceSet::default();
        set.first_run(&custom(), false);
        assert_eq!(set.current, MY_WORKSPACE);
        assert_eq!(set.saved.len(), 1);
        assert_eq!(set.saved[0].name, MY_WORKSPACE);
        assert_eq!(set.saved[0].interface, Interface::Classic);
        assert_eq!(set.saved[0].toolbars, custom());
    }

    #[test]
    fn first_run_on_the_classic_layout_picks_autocad_classic() {
        let mut set = WorkspaceSet::default();
        set.first_run(&ToolbarLayout::autocad_classic(), false);
        assert_eq!(set.current, AUTOCAD_CLASSIC);
        assert!(set.saved.is_empty());
    }

    #[test]
    fn first_run_with_env_ribbon_and_default_picks_opencad_studio() {
        let mut set = WorkspaceSet::default();
        set.first_run(&ToolbarLayout::autocad_classic(), true);
        assert_eq!(set.current, OPENCAD_STUDIO);
    }

    #[test]
    fn first_run_with_env_ribbon_and_a_custom_layout_picks_my_workspace() {
        let mut set = WorkspaceSet::default();
        set.first_run(&custom(), true);
        assert_eq!(set.current, MY_WORKSPACE);
        assert_eq!(set.saved[0].interface, Interface::Ribbon);
    }

    #[test]
    fn first_run_is_idempotent() {
        let mut set = WorkspaceSet::default();
        set.first_run(&custom(), false);
        let after = set.clone();
        set.first_run(&ToolbarLayout::default(), true);
        assert_eq!(set, after);
    }

    #[test]
    fn sanitized_drops_bad_names_and_a_missing_current() {
        let ws = |name: &str| Workspace {
            name: name.to_string(),
            interface: Interface::Classic,
            toolbars: ToolbarLayout::default(),
        };
        let set = WorkspaceSet {
            current: "sparito".to_string(),
            saved: vec![
                ws("Uno"),
                ws("uno"),
                ws(""),
                ws("   "),
                ws("autocad classic"),
                ws(&"y".repeat(60)),
                ws("  Due  "),
            ],
            ..Default::default()
        }
        .sanitized();
        let names: Vec<_> = set.saved.iter().map(|w| w.name.clone()).collect();
        assert_eq!(names, vec!["Uno".to_string(), "y".repeat(40), "Due".to_string()]);
        assert_eq!(set.current, AUTOCAD_CLASSIC);
        // An empty current is kept: it means the first run.
        assert!(WorkspaceSet::default().sanitized().current.is_empty());
    }

    #[test]
    fn rename_and_move_keep_the_set_consistent() {
        let mut set = with_user("Uno");
        set.save("Due", Interface::Classic, custom());
        assert_eq!(set.current, "Due");
        assert_eq!(set.rename("Due", "Tre"), Ok(()));
        assert_eq!(set.current, "Tre");
        assert_eq!(set.rename("Tre", "uno"), Err(NameError::Duplicate));
        assert_eq!(set.rename("Tre", AUTOCAD_CLASSIC), Err(NameError::Reserved));
        set.move_up("Tre");
        assert_eq!(set.names(), vec![AUTOCAD_CLASSIC, OPENCAD_STUDIO, "Tre", "Uno"]);
        set.move_down("Tre");
        assert_eq!(set.names(), vec![AUTOCAD_CLASSIC, OPENCAD_STUDIO, "Uno", "Tre"]);
    }
}
