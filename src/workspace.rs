//! ArchLine workspace selection.
//!
//! `classic` (default) hides the ribbon and shows the menu bar, plus AutoCAD-style
//! docked toolbars on drawing tabs; the Start page has the menu bar only.
//! `ARCHLINE_WORKSPACE=ribbon` restores the upstream ribbon everywhere.

/// The interface in use: 0 = not read yet, 1 = classic, 2 = ribbon.
///
/// Set from `ARCHLINE_WORKSPACE` on first use, then by the current workspace
/// (`set_classic`). Under `cfg(test)` it is per thread, so tests that build an
/// app (`apply_config` sets it) do not disturb one another.
#[cfg(not(test))]
static INTERFACE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

#[cfg(test)]
thread_local! {
    static INTERFACE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

fn load() -> u8 {
    #[cfg(not(test))]
    {
        INTERFACE.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(test)]
    {
        INTERFACE.with(|c| c.get())
    }
}

fn store(value: u8) {
    #[cfg(not(test))]
    {
        INTERFACE.store(value, std::sync::atomic::Ordering::Relaxed);
    }
    #[cfg(test)]
    {
        INTERFACE.with(|c| c.set(value));
    }
}

/// `true` when the classic toolbars replace the ribbon on this tab.
pub fn classic_active(is_start_tab: bool, clean_screen: bool) -> bool {
    is_classic() && !is_start_tab && !clean_screen
}

/// `true` for the AutoCAD-classic interface (default).
pub fn is_classic() -> bool {
    match load() {
        1 => true,
        2 => false,
        _ => {
            let classic = !matches!(
                std::env::var("ARCHLINE_WORKSPACE").as_deref(),
                Ok("ribbon") | Ok("Ribbon")
            );
            store(if classic { 1 } else { 2 });
            classic
        }
    }
}

/// Switch the interface at run time (the current workspace decides it).
pub fn set_classic(classic: bool) {
    store(if classic { 1 } else { 2 });
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

/// `true` when the command line and the status bar are drawn on this tab. The
/// classic Start page has neither, as in AutoCAD; every other tab keeps both.
pub fn bottom_chrome_shown(is_start_tab: bool) -> bool {
    bottom_chrome_shown_in(is_classic(), is_start_tab)
}

fn bottom_chrome_shown_in(classic: bool, is_start_tab: bool) -> bool {
    !(classic && is_start_tab)
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
    fn the_classic_start_page_has_no_command_line_or_status_bar() {
        assert!(!bottom_chrome_shown_in(true, true));
        assert!(bottom_chrome_shown_in(true, false), "drawing tabs keep them");
        assert!(bottom_chrome_shown_in(false, true), "the ribbon workspace is unchanged");
    }

    #[test]
    fn set_classic_flips_every_predicate() {
        let initial = is_classic();
        set_classic(false);
        assert!(!is_classic());
        assert!(ribbon_shown(false, false));
        assert!(!menu_bar_shown(false));
        assert!(!classic_active(false, false));
        set_classic(true);
        assert!(is_classic());
        assert!(!ribbon_shown(false, false));
        assert!(menu_bar_shown(false));
        assert!(classic_active(false, false));
        set_classic(initial);
    }

    #[test]
    fn the_menu_bar_is_on_every_classic_tab_including_start() {
        assert!(menu_bar_shown_in(true, false));
        assert!(!menu_bar_shown_in(true, true), "clean screen hides it");
        assert!(!menu_bar_shown_in(false, false), "the ribbon workspace has none");
    }
}
