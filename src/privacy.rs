//! ArchLine: privacy switch for outbound network traffic at startup.
//!
//! Upstream contacts GitHub (release check, Discussions feed), YouTube (tutorial
//! playlist) and Patreon (supporters list) on every boot. ArchLine is offline by
//! default; setting `ARCHLINE_ONLINE=1` restores the upstream behaviour.

/// Name shown in window titles, Start page and About dialog.
pub const APP_NAME: &str = "ArchLine";

/// `true` when the user opted in to background network requests.
pub fn online() -> bool {
    matches!(
        std::env::var("ARCHLINE_ONLINE").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}
