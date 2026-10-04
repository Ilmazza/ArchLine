//! ArchLine workspace selection.
//!
//! `classic` (default) hides the ribbon on drawing tabs and shows AutoCAD-style
//! docked toolbars instead; `ARCHLINE_WORKSPACE=ribbon` restores the upstream
//! ribbon. The Start page keeps the ribbon in both modes.

use std::sync::OnceLock;

/// `true` when the classic toolbars replace the ribbon on this tab.
pub fn classic_active(is_start_tab: bool, clean_screen: bool) -> bool {
    is_classic() && !is_start_tab && !clean_screen
}

/// `true` for the AutoCAD-classic workspace (default).
pub fn is_classic() -> bool {
    static CLASSIC: OnceLock<bool> = OnceLock::new();
    *CLASSIC.get_or_init(|| {
        !matches!(
            std::env::var("ARCHLINE_WORKSPACE").as_deref(),
            Ok("ribbon") | Ok("Ribbon")
        )
    })
}
