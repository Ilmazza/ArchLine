//! "ArchLine Dark": the AutoCAD-classic dark chrome.
//!
//! Colours were sampled from AutoCAD 2025's dark theme. The chrome has two
//! flat greys-with-a-blue-cast (title/menu/status bars and docked toolbars)
//! around a canvas that is almost the darker of the two.
//!
//! iced derives every background shade from a single seed colour, which
//! cannot hit AutoCAD's exact toolbar grey, so the palette is generated and
//! then the background and accent roles the UI actually uses are pinned by
//! hand. Role map (what the existing widgets read):
//!
//! - `base`    bars: title, menu, status, tab strip      (#222933)
//! - `weak`    docked toolbar strips                     (#3B4453)
//! - `strong`  toolbar button hover, separators          (#4F5A6F)
//! - `neutral` 1px borders                               (#465063)
//! - `primary` selection / hover accent                  (#0078D7)
//!
//! Like Fusion, this is an `iced::Theme::Custom`: it is absent from
//! `iced::Theme::ALL`, so enumerate themes through
//! [`crate::app::config::all_themes`]. Its name is persisted in
//! `settings.json` as `theme.name`; do not rename it.

use iced::theme::palette::{Palette, Pair, Seed};
use iced::{Color, Theme};

/// Display name, persisted verbatim in `settings.json`.
pub const ARCHLINE_DARK: &str = "ArchLine Dark";

/// Model-space canvas (#212830). Same value as
/// `config::CLASSIC_CAD_DARK_BG`; kept here so the theme is self-contained
/// (a test pins them together).
pub const ARCHLINE_DARK_CANVAS: [u8; 3] = [33, 40, 48];

fn rgb(hex: u32) -> Color {
    Color::from_rgb8(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

fn generate(seed: Seed) -> Palette {
    let mut p = Palette::generate(seed);
    let text = seed.text;

    p.background.weakest = Pair::new(rgb(0x2A_323E), text);
    p.background.weaker = Pair::new(rgb(0x32_3B48), text);
    p.background.weak = Pair::new(rgb(0x3B_4453), text);
    p.background.neutral = Pair::new(rgb(0x46_5063), text);
    p.background.strong = Pair::new(rgb(0x4F_5A6F), text);
    p.background.stronger = Pair::new(rgb(0x5A_6580), text);
    p.background.strongest = Pair::new(rgb(0x67_728E), text);

    // `weak` is the active tab, MODEL pill, toggle fill and menu hover:
    // AutoCAD's #454F61 (hover grey-blue), lighter than the #3B4453 strips.
    p.primary.weak = Pair::new(rgb(0x45_4F61), text);
    p.primary.strong = Pair::new(rgb(0x15_65C0), text);
    p
}

/// AutoCAD-classic dark chrome with a blue accent.
#[must_use]
pub fn archline_dark() -> Theme {
    Theme::custom_with_fn(
        ARCHLINE_DARK,
        Seed {
            background: rgb(0x22_2933),
            text: rgb(0xF5_F5F5),
            primary: rgb(0x00_78D7),
            success: rgb(0x62_D987),
            warning: rgb(0xFF_D580),
            danger: rgb(0xE5_3935),
        },
        generate,
    )
}

/// The canvas colour this theme asks for, or `None` for any other theme.
/// Keyed by name because `Theme::Custom` carries no discriminant to match on.
#[must_use]
pub fn archline_canvas(theme: &Theme) -> Option<[u8; 3]> {
    (theme.to_string() == ARCHLINE_DARK).then_some(ARCHLINE_DARK_CANVAS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::style::common::wcag_contrast;

    #[test]
    fn pinned_roles_match_the_autocad_samples() {
        let p = archline_dark().palette().clone();
        let hex = |c: Color| {
            let [r, g, b, _] = c.into_rgba8();
            format!("#{r:02X}{g:02X}{b:02X}")
        };
        assert_eq!(hex(p.background.base.color), "#222933");
        assert_eq!(hex(p.background.weak.color), "#3B4453");
        assert_eq!(hex(p.primary.base.color), "#0078D7");
    }

    #[test]
    fn text_meets_wcag_aa_on_every_surface() {
        let p = *archline_dark().palette();
        for (name, pair) in [
            ("base", p.background.base),
            ("weakest", p.background.weakest),
            ("weaker", p.background.weaker),
            ("weak", p.background.weak),
            ("neutral", p.background.neutral),
            ("strong", p.background.strong),
            ("primary weak", p.primary.weak),
            ("primary strong", p.primary.strong),
        ] {
            let contrast = wcag_contrast(pair.text, pair.color);
            assert!(contrast >= 4.5, "{name}: {contrast:.2}:1, below AA");
        }
    }

    /// The accent doubles as the selection colour, so it must read on the canvas.
    #[test]
    fn accent_is_visible_on_the_canvas() {
        let canvas = Color::from_rgb8(33, 40, 48);
        let contrast = wcag_contrast(archline_dark().palette().primary.base.color, canvas);
        assert!(contrast >= 3.0, "accent is {contrast:.2}:1 on the canvas");
    }

    #[test]
    fn canvas_matches_classic_cad_dark_bg() {
        assert_eq!(ARCHLINE_DARK_CANVAS, crate::app::config::CLASSIC_CAD_DARK_BG);
        assert_eq!(archline_canvas(&archline_dark()), Some(ARCHLINE_DARK_CANVAS));
        assert_eq!(archline_canvas(&Theme::Dark), None);
    }
}
