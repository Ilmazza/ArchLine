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
