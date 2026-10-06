//! ArchLine: icons for the menu commands the app had no icon for (they showed
//! two letters on the classic toolbars and nothing in the menus).
//!
//! Two kinds, as in `classic_menu`: coloured art drawn as it is (the ribbon
//! assets that no tool was using), and black-stroke glyphs that are tinted with
//! the theme text colour.

macro_rules! asset {
    ($path:literal) => {
        include_bytes!(concat!("../../assets/icons/", $path)) as &[u8]
    };
}

/// Coloured art that already ships with the app but no ribbon tool claims.
const RIBBON: &[(&str, &[u8])] = &[
    ("POINT", asset!("point.svg")),
    ("DIMSTYLE", asset!("dim_style.svg")),
    ("STYLE", asset!("text_style.svg")),
    ("COLOR", asset!("color_palette.svg")),
    ("UCS", asset!("ucs_icon.svg")),
];

/// Black-stroke glyphs: some from the quick-access strip, the rest drawn for
/// these commands in `assets/icons/menu`.
const MONO: &[(&str, &[u8])] = &[
    ("UNDO", asset!("ui/undo.svg")),
    ("REDO", asset!("ui/redo.svg")),
    ("HELP", asset!("ui/help.svg")),
    ("CLOSE", asset!("ui/close.svg")),
    ("DSETTINGS", asset!("ui/snap.svg")),
    ("CLEANSCREEN", asset!("status/cleanscreen.svg")),
    ("ID", asset!("menu/id.svg")),
    ("LIST", asset!("menu/list.svg")),
    ("REGEN", asset!("menu/regen.svg")),
    ("REGENALL", asset!("menu/regen_all.svg")),
    ("SELECTALL", asset!("menu/select_all.svg")),
    ("ZOOM ALL", asset!("menu/zoom_all.svg")),
    ("ZOOM PREVIOUS", asset!("menu/zoom_previous.svg")),
    ("LINETYPE", asset!("menu/linetype.svg")),
    ("UNITS", asset!("menu/units.svg")),
    ("LIMITS", asset!("menu/limits.svg")),
    ("FIELD", asset!("menu/field.svg")),
    ("IMAGEATTACH", asset!("menu/image_attach.svg")),
    ("PLUGINS", asset!("menu/plugins.svg")),
    ("SHORTCUTS", asset!("menu/shortcuts.svg")),
    ("QUIT", asset!("menu/quit.svg")),
    ("COPYBASE", asset!("menu/copy_base.svg")),
];

fn find(table: &'static [(&'static str, &'static [u8])], command: &str) -> Option<&'static [u8]> {
    table.iter().find(|(c, _)| *c == command).map(|(_, b)| *b)
}

/// Coloured art for `command`, if it has some here.
pub fn ribbon(command: &str) -> Option<&'static [u8]> {
    find(RIBBON, command)
}

/// Black-stroke glyph for `command` (to be tinted), if it has one here.
pub fn mono(command: &str) -> Option<&'static [u8]> {
    find(MONO, command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draws_something(bytes: &[u8]) -> bool {
        let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default())
            .expect("the icon parses");
        let size = tree.size();
        size.width() > 0.0 && size.height() > 0.0 && !tree.root().children().is_empty()
    }

    #[test]
    fn every_icon_parses_and_draws_something() {
        for (command, bytes) in RIBBON.iter().chain(MONO) {
            assert!(draws_something(bytes), "{command}: empty or invalid svg");
        }
    }

    #[test]
    fn a_command_is_listed_once() {
        let mut all: Vec<&str> = RIBBON.iter().chain(MONO).map(|(c, _)| *c).collect();
        let n = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), n, "a command has two icons");
    }

    #[test]
    fn a_glyph_uses_the_themed_stroke_so_it_can_be_tinted() {
        for (command, bytes) in MONO {
            let text = std::str::from_utf8(bytes).expect("utf-8 svg");
            assert!(text.contains("#B4B6B9"), "{command}: not a themable glyph");
        }
    }
}
