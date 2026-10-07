//! ArchLine workspace selection.
//!
//! `classic` (default) hides the ribbon and shows the menu bar, plus AutoCAD-style
//! docked toolbars on drawing tabs; the Start page has the menu bar only.
//! `ARCHLINE_WORKSPACE=ribbon` restores the upstream ribbon everywhere.

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

/// `true` when the ribbon is drawn: never in clean-screen mode, and in the
/// classic workspace never at all (the Start page included, as in AutoCAD).
pub fn ribbon_shown(is_start_tab: bool, clean_screen: bool) -> bool {
    ribbon_shown_in(is_classic(), is_start_tab, clean_screen)
}

/// `true` when the menu bar (File, Edit, ...) is drawn: classic workspace,
/// including the Start page, unless clean-screen mode is on.
pub fn menu_bar_shown(clean_screen: bool) -> bool {
    menu_bar_shown_in(is_classic(), clean_screen)
}

fn ribbon_shown_in(classic: bool, _is_start_tab: bool, clean_screen: bool) -> bool {
    !classic && !clean_screen
}

fn menu_bar_shown_in(classic: bool, clean_screen: bool) -> bool {
    classic && !clean_screen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_classic_workspace_has_no_ribbon_even_on_the_start_page() {
        assert!(!ribbon_shown_in(true, true, false));
        assert!(!ribbon_shown_in(true, false, false));
    }

    #[test]
    fn the_ribbon_workspace_keeps_its_ribbon_except_in_clean_screen() {
        assert!(ribbon_shown_in(false, true, false), "Start page keeps the ribbon");
        assert!(ribbon_shown_in(false, false, false));
        assert!(!ribbon_shown_in(false, false, true));
    }

    #[test]
    fn the_menu_bar_is_on_every_classic_tab_including_start() {
        assert!(menu_bar_shown_in(true, false));
        assert!(!menu_bar_shown_in(true, true), "clean screen hides it");
        assert!(!menu_bar_shown_in(false, false), "the ribbon workspace has none");
    }
}
