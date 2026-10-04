//! ArchLine workspace selection.
//!
//! `classic` (default) hides the ribbon on drawing tabs and shows AutoCAD-style
//! docked toolbars instead; `ARCHLINE_WORKSPACE=ribbon` restores the upstream
//! ribbon. The Start page keeps the ribbon in both modes.

use std::sync::OnceLock;

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
