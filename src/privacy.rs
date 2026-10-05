//! ArchLine: privacy switch for outbound network traffic at startup.
//!
//! Upstream contacts GitHub (release check, Discussions feed), YouTube (tutorial
//! playlist) and Patreon (supporters list) on every boot. ArchLine is offline by
//! default; setting `ARCHLINE_ONLINE=1` restores the upstream behaviour.

/// Name shown in window titles, Start page and About dialog.
pub const APP_NAME: &str = "ArchLine";

/// Page opened by Help (F1 and the Help menu): ArchLine's own repository, not
/// upstream's Discussions.
pub const HELP_URL: &str = "https://github.com/Ilmazza/ArchLine";

/// `true` when the user opted in to background network requests.
pub fn online() -> bool {
    matches!(
        std::env::var("ARCHLINE_ONLINE").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

/// `true` when the upstream promotional blocks of the Start page (Donate,
/// Sponsors, Reddit, Patreon supporters) are shown. Hidden by default;
/// `ARCHLINE_PROMO=1` restores them.
pub fn show_promo() -> bool {
    matches!(
        std::env::var("ARCHLINE_PROMO").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

#[cfg(test)]
mod tests {
    /// The `"HELP" => { .. }` arm of the command dispatcher, as source text.
    fn help_arm() -> &'static str {
        let source = include_str!("app/commands/view.rs");
        let start = source.find("\"HELP\" => {").expect("HELP arm exists");
        &source[start..(start + 500).min(source.len())]
    }

    #[test]
    fn help_opens_archline_not_the_upstream_discussions() {
        assert!(super::HELP_URL.starts_with("https://github.com/Ilmazza/ArchLine"));
        assert!(!super::HELP_URL.contains("HakanSeven12"));
        let arm = help_arm();
        assert!(arm.contains("crate::privacy::HELP_URL"), "HELP does not use HELP_URL:\n{arm}");
        assert!(!arm.contains("HakanSeven12"), "HELP still points at upstream:\n{arm}");
    }
}
