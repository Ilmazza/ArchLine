//! Pattern palette of the Hatch dialog: the "..." button next to Pattern
//! swaps the dialog's three columns for a browser of the catalog, sorted in
//! three tabs (ANSI, ISO, Other Predefined) and searchable.
//!
//! The palette is part of the dialog's [`State`](super::hatch_dialog::State),
//! so it disappears with it. Applying a choice goes through the dialog's own
//! `Field::Pattern`, so the usual rules apply.

use iced::widget::{
    button, canvas, column, container, mouse_area, row, scrollable, text, text_input, Space,
};
use iced::{Element, Fill, Length, Theme};

use crate::app::Message;
use crate::scene::model::hatch_patterns::{self, PatternEntry};
use crate::t;
use crate::ui::style::form::dialog_button_styled_opt;

/// Id of the search field, so opening the palette can focus it.
pub const PALETTE_SEARCH_ID: &str = "hatch-palette-search";

/// The tabs of the palette. Every catalog pattern belongs to exactly one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternCategory {
    Ansi,
    Iso,
    Other,
}

impl PatternCategory {
    pub const ALL: [PatternCategory; 3] = [Self::Ansi, Self::Iso, Self::Other];
}

/// What the palette can be told to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    Open,
    Close,
    Tab(PatternCategory),
    Search(String),
    Pick(String),
    Apply,
}

/// The palette's working state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    pub category: PatternCategory,
    pub search: String,
    /// Catalog name of the highlighted card; applied by OK / double click.
    pub selected: Option<String>,
}

/// Tab of a pattern, by the prefix of its name (case-insensitive).
pub fn category_of(name: &str) -> PatternCategory {
    let upper = name.to_ascii_uppercase();
    if upper.starts_with("ANSI") {
        PatternCategory::Ansi
    } else if upper.starts_with("ISO") {
        PatternCategory::Iso
    } else {
        PatternCategory::Other
    }
}

/// The catalog patterns of one tab, in catalog order.
pub fn in_category(category: PatternCategory) -> Vec<&'static PatternEntry> {
    hatch_patterns::catalog()
        .iter()
        .filter(|entry| category_of(&entry.name) == category)
        .collect()
}

/// Every catalog pattern whose name contains `search` (trimmed,
/// case-insensitive), whatever its tab.
pub fn matching(search: &str) -> Vec<&'static PatternEntry> {
    let query = search.trim().to_lowercase();
    hatch_patterns::catalog()
        .iter()
        .filter(|entry| entry.name.to_lowercase().contains(&query))
        .collect()
}

impl Palette {
    /// A palette opened on `current`: its tab and its card highlighted when it
    /// is in the catalog, otherwise ANSI and nothing highlighted.
    pub fn open(current: &str) -> Self {
        match hatch_patterns::find(current) {
            Some(entry) => Self {
                category: category_of(&entry.name),
                search: String::new(),
                selected: Some(entry.name.clone()),
            },
            None => Self {
                category: PatternCategory::Ansi,
                search: String::new(),
                selected: None,
            },
        }
    }

    /// What the grid shows: every match of a non-empty search, else the tab.
    pub fn visible(&self) -> Vec<&'static PatternEntry> {
        if self.search.trim().is_empty() {
            in_category(self.category)
        } else {
            matching(&self.search)
        }
    }
}

// ── View ───────────────────────────────────────────────────────────────────

/// Cards per row of the grid.
const COLUMNS: usize = 5;
/// The grid scrolls inside a fixed height, so the modal's measured height
/// does not depend on how many patterns match.
const GRID_HEIGHT: f32 = 480.0;

impl PatternCategory {
    fn label(self) -> String {
        match self {
            Self::Ansi => t!("ANSI").into_owned(),
            Self::Iso => t!("ISO").into_owned(),
            Self::Other => t!("Other Predefined").into_owned(),
        }
    }
}

/// One card: preview and name. A `mouse_area`, not a `button`, so that the
/// second click of a double click still reaches it (a button would capture
/// the press): click picks, double click applies.
fn card<'a>(entry: &PatternEntry, selected: bool) -> Element<'a, Message> {
    let preview = canvas(crate::ui::properties::HatchPatternPreview::new(entry.gpu.clone()))
        .width(Fill)
        .height(crate::ui::properties::PATTERN_PREVIEW_H);
    let body = container(
        column![
            preview,
            container(text(crate::ui::text_util::elide(&entry.name, 18)).size(11))
                .width(Fill)
                .align_x(iced::Center),
        ]
        .spacing(3),
    )
    .padding(5)
    .width(Fill)
    .style(move |theme: &Theme| {
        let look = crate::ui::properties::pattern_card_style(theme, selected, false, false);
        container::Style {
            background: look.background,
            text_color: Some(look.text_color),
            border: look.border,
            ..Default::default()
        }
    });
    mouse_area(body)
        .on_press(Message::HatchDialogPalette(PaletteAction::Pick(entry.name.clone())))
        .on_double_click(Message::HatchDialogPalette(PaletteAction::Apply))
        .into()
}

fn grid<'a>(palette: &Palette) -> Element<'a, Message> {
    let visible = palette.visible();
    if visible.is_empty() {
        return container(text(t!("No patterns found")).size(12))
            .width(Fill)
            .height(Length::Fixed(GRID_HEIGHT))
            .center_x(Fill)
            .padding(24)
            .into();
    }
    let mut rows = column![].spacing(6).width(Fill);
    for chunk in visible.chunks(COLUMNS) {
        let mut cards = row![].spacing(6);
        for entry in chunk {
            let selected = palette
                .selected
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(&entry.name));
            cards = cards.push(card(entry, selected));
        }
        // A short last row keeps the cards the same width as the others.
        for _ in chunk.len()..COLUMNS {
            cards = cards.push(Space::new().width(Fill));
        }
        rows = rows.push(cards);
    }
    scrollable(rows).width(Fill).height(Length::Fixed(GRID_HEIGHT)).into()
}

/// The dialog's body while the palette is open: `tabs` is the window's own
/// tab strip, kept so the window keeps its title row.
#[inline(never)]
pub fn page<'a>(
    tabs: Element<'a, Message>,
    palette: &Palette,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let searching = !palette.search.trim().is_empty();
    let mut header = row![].spacing(6).align_y(iced::Center);
    for category in PatternCategory::ALL {
        let active = !searching && palette.category == category;
        header = header.push(dialog_button_styled_opt(
            category.label(),
            Some(Message::HatchDialogPalette(PaletteAction::Tab(category))),
            if active { button::primary } else { button::secondary },
        ));
    }
    let search = text_input(t!("Search patterns…").as_ref(), &palette.search)
        .id(iced::widget::Id::new(PALETTE_SEARCH_ID))
        .on_input(|text| Message::HatchDialogPalette(PaletteAction::Search(text)))
        .size(12)
        .padding([5, 7])
        .width(Length::Fixed(260.0));
    header = header.push(Space::new().width(Fill)).push(search);

    let actions = row![
        Space::new().width(Fill),
        dialog_button_styled_opt(
            t!("OK").into_owned(),
            palette
                .selected
                .is_some()
                .then_some(Message::HatchDialogPalette(PaletteAction::Apply)),
            button::primary,
        ),
        dialog_button_styled_opt(
            t!("Cancel").into_owned(),
            Some(Message::HatchDialogPalette(PaletteAction::Close)),
            button::secondary,
        ),
    ]
    .spacing(8)
    .align_y(iced::Center);

    column![tabs, header, grid(palette), actions]
        .spacing(10)
        .padding(10)
        .width(sizing.width)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[&PatternEntry]) -> Vec<String> {
        entries.iter().map(|entry| entry.name.clone()).collect()
    }

    #[test]
    fn the_category_follows_the_name_prefix() {
        assert_eq!(category_of("ANSI31"), PatternCategory::Ansi);
        assert_eq!(category_of("ansi37"), PatternCategory::Ansi);
        assert_eq!(category_of("ISO02W100"), PatternCategory::Iso);
        assert_eq!(category_of("iso03w100"), PatternCategory::Iso);
        assert_eq!(category_of("AR-CONC"), PatternCategory::Other);
        assert_eq!(category_of("BRICK"), PatternCategory::Other);
        assert_eq!(category_of("JIS_WOOD"), PatternCategory::Other);
        // a prefix, not a substring
        assert_eq!(category_of("MYANSI"), PatternCategory::Other);
        assert_eq!(category_of(""), PatternCategory::Other);
    }

    #[test]
    fn the_three_tabs_partition_the_catalog() {
        let catalog = hatch_patterns::catalog();
        let mut seen: Vec<String> = Vec::new();
        for category in PatternCategory::ALL {
            let entries = in_category(category);
            assert!(!entries.is_empty(), "{category:?} is not empty");
            for entry in entries {
                assert_eq!(category_of(&entry.name), category, "{}", entry.name);
                seen.push(entry.name.clone());
            }
        }
        assert_eq!(seen.len(), catalog.len(), "no pattern lost, none twice");
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), seen.len(), "no duplicate across tabs");
        for entry in catalog {
            assert!(seen.contains(&entry.name), "{} is in a tab", entry.name);
        }
        // the known members land where the prefix rule puts them
        assert!(names(&in_category(PatternCategory::Ansi)).contains(&"ANSI31".to_string()));
        assert!(names(&in_category(PatternCategory::Iso)).contains(&"ISO02W100".to_string()));
        assert!(names(&in_category(PatternCategory::Other)).contains(&"BRICK".to_string()));
    }

    #[test]
    fn search_is_a_case_insensitive_substring_across_tabs() {
        let found = names(&matching("ansi3"));
        assert!(found.contains(&"ANSI31".to_string()));
        assert!(!found.is_empty());
        assert!(found.iter().all(|name| name.to_lowercase().contains("ansi3")));
        // a substring in the middle of a name, in another tab
        let brick = names(&matching("  Bric "));
        assert!(brick.contains(&"BRICK".to_string()), "trimmed and any case");
        // crosses tabs: "0" appears in ANSI, ISO and Other names
        let zero = matching("0");
        let tabs: std::collections::HashSet<_> =
            zero.iter().map(|entry| category_of(&entry.name) as u8).collect();
        assert!(tabs.len() > 1, "a search is not limited to one tab");
        assert!(matching("zzzz-no-such-pattern").is_empty());
    }

    #[test]
    fn an_empty_search_shows_the_current_tab_and_a_search_shows_every_match() {
        let mut palette = Palette::open("ANSI31");
        assert_eq!(
            names(&palette.visible()),
            names(&in_category(PatternCategory::Ansi))
        );
        palette.category = PatternCategory::Iso;
        assert_eq!(
            names(&palette.visible()),
            names(&in_category(PatternCategory::Iso))
        );
        palette.search = "   ".into();
        assert_eq!(
            names(&palette.visible()),
            names(&in_category(PatternCategory::Iso)),
            "blank counts as empty"
        );
        // whatever the tab, the search wins
        palette.search = "brick".into();
        assert!(names(&palette.visible()).contains(&"BRICK".to_string()));
        palette.category = PatternCategory::Ansi;
        assert!(names(&palette.visible()).contains(&"BRICK".to_string()));
        palette.search = "zzzz-no-such-pattern".into();
        assert!(palette.visible().is_empty(), "no result is an empty grid");
    }

    #[test]
    fn opening_starts_on_the_tab_of_the_current_pattern() {
        let ansi = Palette::open("ANSI31");
        assert_eq!(ansi.category, PatternCategory::Ansi);
        assert_eq!(ansi.selected.as_deref(), Some("ANSI31"));
        let iso = Palette::open("ISO02W100");
        assert_eq!(iso.category, PatternCategory::Iso);
        assert_eq!(iso.selected.as_deref(), Some("ISO02W100"));
        let other = Palette::open("brick");
        assert_eq!(other.category, PatternCategory::Other);
        assert_eq!(
            other.selected.as_deref(),
            Some("BRICK"),
            "the catalog's spelling is kept"
        );
        assert!(other.search.is_empty());
    }

    #[test]
    fn a_pattern_missing_from_the_catalog_opens_on_ansi_with_nothing_chosen() {
        let palette = Palette::open("NO_SUCH_PATTERN");
        assert_eq!(palette.category, PatternCategory::Ansi);
        assert!(palette.selected.is_none());
        // a name that starts like an ISO one but is not in the catalog
        let palette = Palette::open("ISO99X999");
        assert_eq!(palette.category, PatternCategory::Ansi, "not by prefix");
        assert!(palette.selected.is_none());
    }
}
