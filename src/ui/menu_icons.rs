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

/// Coloured art that already ships with the app but no ribbon tool claims, and
/// the icons drawn for ArchLine's own commands.
const RIBBON: &[(&str, &[u8])] = &[
    ("POINT", asset!("point.svg")),
    ("DIMSTYLE", asset!("dim_style.svg")),
    ("STYLE", asset!("text_style.svg")),
    ("COLOR", asset!("color_palette.svg")),
    ("UCS", asset!("ucs_icon.svg")),
    ("SHEETSET", asset!("sheetset.svg")),
    ("TABLESTYLE", asset!("table_style.svg")),
    ("MLEADERSTYLE", asset!("mleader_style.svg")),
    // The six Tools > Ristrutturazione commands, drawn for ArchLine: yellow is
    // a demolished zone (ACI 2), red a new one (ACI 1). Both are written as
    // CSS keywords because the recolouring in `icons.rs` turns `#FF0000` into
    // the theme's danger colour.
    ("AL_DEMOLISCI", asset!("ristr_demolisci.svg")),
    ("AL_NUOVO", asset!("ristr_nuovo.svg")),
    ("AL_STATO", asset!("ristr_stato.svg")),
    ("AL_LOCALE", asset!("ristr_locale.svg")),
    ("AL_AGGIORNA", asset!("ristr_aggiorna.svg")),
    ("AL_ESTRAI", asset!("ristr_estrai.svg")),
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
    ("PUBLISH", asset!("menu/publish.svg")),
    ("QUICKCALC", asset!("menu/quickcalc.svg")),
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

    const RISTRUTTURAZIONE: [&str; 6] = [
        "AL_DEMOLISCI",
        "AL_NUOVO",
        "AL_STATO",
        "AL_LOCALE",
        "AL_AGGIORNA",
        "AL_ESTRAI",
    ];

    fn text(bytes: &[u8]) -> String {
        std::str::from_utf8(bytes).expect("utf-8 svg").to_ascii_lowercase()
    }

    #[test]
    fn each_ristrutturazione_command_has_its_own_coloured_icon() {
        let mut seen = Vec::new();
        for command in RISTRUTTURAZIONE {
            let bytes = ribbon(command).unwrap_or_else(|| panic!("{command}: no coloured icon"));
            assert!(mono(command).is_none(), "{command}: also listed as a glyph");
            assert!(!seen.contains(&bytes), "{command}: same drawing as another command");
            seen.push(bytes);
        }
    }

    #[test]
    fn ristrutturazione_icons_use_only_colours_the_theme_leaves_alone() {
        // `icons::recolor_semantic_svg` maps a fixed list of hex colours onto
        // the theme (#FF0000 → danger, #FFFFFF and `white` → text). The line
        // (#B4B6B9) and accent (#6DB7ED) are meant to follow the theme; the
        // zone yellow, red and the marks on them must not.
        for command in RISTRUTTURAZIONE {
            let svg = text(ribbon(command).unwrap());
            assert!(!svg.contains("white"), "{command}: `white` is remapped to the text colour");
            for hex in svg.split('#').skip(1) {
                let hex: String = hex.chars().take_while(char::is_ascii_hexdigit).collect();
                assert!(
                    ["b4b6b9", "6db7ed", "fefefe"].contains(&hex.as_str()),
                    "{command}: #{hex} may be remapped by the theme; use a keyword or #FEFEFE"
                );
            }
        }
    }

    /// RGBA of one pixel of `bytes` drawn on a 24x24 canvas.
    fn pixel(bytes: &[u8], x: u32, y: u32) -> [u8; 4] {
        let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default())
            .expect("the icon parses");
        let mut pixmap = resvg::tiny_skia::Pixmap::new(24, 24).expect("24x24 pixmap");
        resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());
        let p = pixmap.pixel(x, y).expect("pixel inside the canvas");
        [p.red(), p.green(), p.blue(), p.alpha()]
    }

    #[test]
    fn zones_are_drawn_in_the_cad_colours() {
        const YELLOW: [u8; 4] = [255, 255, 0, 255]; // ACI 2, AL-DEMOLIZIONI
        const RED: [u8; 4] = [255, 0, 0, 255]; // ACI 1, AL-NUOVE
        let at = |command: &str, x, y| pixel(ribbon(command).unwrap(), x, y);
        // Beside the X and the + that sit in the middle of the zone.
        assert_eq!(at("AL_DEMOLISCI", 8, 12), YELLOW);
        assert_eq!(at("AL_NUOVO", 8, 12), RED);
        // Stato: demolished square on the left, new one on the right.
        assert_eq!(at("AL_STATO", 5, 5), YELLOW);
        assert_eq!(at("AL_STATO", 17, 5), RED);
    }

    #[test]
    fn a_glyph_uses_the_themed_stroke_so_it_can_be_tinted() {
        for (command, bytes) in MONO {
            let text = std::str::from_utf8(bytes).expect("utf-8 svg");
            assert!(text.contains("#B4B6B9"), "{command}: not a themable glyph");
        }
    }
}
