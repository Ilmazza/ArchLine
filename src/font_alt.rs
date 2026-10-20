//! ArchLine: what to draw a missing `.shx` font with, and the user's choices.
//!
//! An SHX font is AutoCAD's own stroke format and is not a Windows font, so a
//! drawing that names `isoct.shx` on a machine without it is drawn with a
//! stand-in. The embedded stroke fonts (`lff.rs`) stand in for the common names;
//! three of them match the original's character widths exactly (measured on the
//! AutoCAD 2025 files), the rest only approximately, and a name nobody knows
//! falls to `standard`. AutoCAD asks the same question (FONTALT / `acad.fmp`):
//! this module keeps the answer, per user, in `settings.json`
//! (`font_substitutions`). The dialog is `ui/window/missing_fonts.rs`.

use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

use crate::scene::text::{lff, sysfont};

/// What a missing font is drawn with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Substitute {
    /// The stand-in the program picks by name (`lff.rs` aliases, else `standard`).
    /// Stored so that the question is not asked again.
    Default,
    /// An embedded stroke font, by its stem (`romans`).
    Stroke(String),
    /// A font installed in the system, by family name.
    System(String),
}

impl Substitute {
    fn encode(&self) -> String {
        match self {
            Substitute::Default => "default".into(),
            Substitute::Stroke(stem) => format!("stroke:{stem}"),
            Substitute::System(family) => format!("system:{family}"),
        }
    }

    fn decode(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.eq_ignore_ascii_case("default") {
            return Some(Substitute::Default);
        }
        let (kind, value) = raw.split_once(':')?;
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        match kind {
            "stroke" => Some(Substitute::Stroke(value.to_string())),
            "system" => Some(Substitute::System(value.to_string())),
            _ => None,
        }
    }

    /// Whether the choice can still be honoured: the stroke font is embedded and
    /// the system family is installed.
    fn usable(&self) -> bool {
        match self {
            Substitute::Default => true,
            Substitute::Stroke(stem) => lff::is_embedded_font(stem),
            Substitute::System(family) => sysfont::has_family(family),
        }
    }
}

type Table = BTreeMap<String, Substitute>;

// One table for the process; under `cfg(test)` one per test thread, because
// every test application loads the default settings (an empty table) and tests
// run in parallel.
#[cfg(not(test))]
static TABLE: RwLock<Table> = RwLock::new(BTreeMap::new());
#[cfg(test)]
thread_local! {
    static TABLE: std::cell::RefCell<Table> = const { std::cell::RefCell::new(BTreeMap::new()) };
}

#[cfg(not(test))]
fn read<R>(f: impl FnOnce(&Table) -> R) -> R {
    f(&TABLE.read().unwrap_or_else(|e| e.into_inner()))
}
#[cfg(not(test))]
fn write<R>(f: impl FnOnce(&mut Table) -> R) -> R {
    f(&mut TABLE.write().unwrap_or_else(|e| e.into_inner()))
}
#[cfg(test)]
fn read<R>(f: impl FnOnce(&Table) -> R) -> R {
    TABLE.with(|table| f(&table.borrow()))
}
#[cfg(test)]
fn write<R>(f: impl FnOnce(&mut Table) -> R) -> R {
    TABLE.with(|table| f(&mut table.borrow_mut()))
}

/// Stable key of a font reference: bare file name, lower case, without `.shx`
/// (`C:\Fonts\IsoCT.SHX` → `isoct`).
pub fn key(file: &str) -> String {
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file).trim();
    let name = name.to_ascii_lowercase();
    name.strip_suffix(".shx").unwrap_or(&name).to_string()
}

/// Load the saved table (settings). Entries that do not parse are dropped.
pub fn set_table(raw: &BTreeMap<String, String>) {
    let table = raw
        .iter()
        .filter_map(|(file, value)| Some((key(file), Substitute::decode(value)?)))
        .collect();
    write(|current| *current = table);
}

/// The table as saved in the settings.
pub fn snapshot() -> BTreeMap<String, String> {
    read(|table| {
        table
            .iter()
            .map(|(file, sub)| (file.clone(), sub.encode()))
            .collect()
    })
}

/// Remember what `file` is drawn with.
pub fn choose(file: &str, substitute: Substitute) {
    write(|table| {
        table.insert(key(file), substitute);
    });
}

fn chosen(file: &str) -> Option<Substitute> {
    read(|table| table.get(&key(file)).cloned())
}

/// The font name to draw `file` with, when the user chose one that still works.
/// `None` leaves the normal resolution (the stand-in by name) in charge.
pub fn replacement(file: &str) -> Option<String> {
    match chosen(file)? {
        Substitute::Stroke(stem) if lff::is_embedded_font(&stem) => Some(stem),
        Substitute::System(family) => sysfont::canonical_family_name(&family),
        _ => None,
    }
}

/// What the program draws a missing font with when nothing was chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StandIn {
    /// Same character widths as the original, measured.
    Exact(String),
    /// A look-alike: the widths differ.
    Approximate(String),
    /// Nothing known by that name: it is drawn with `standard`.
    None,
}

/// SHX fonts whose embedded stand-in matches the original's widths exactly
/// (measured against the AutoCAD 2025 files: 1.000 on real label strings).
const EXACT: &[&str] = &["romans", "simplex", "txt"];

pub fn stand_in(file: &str) -> StandIn {
    let key = key(file);
    let name = format!("{key}.shx");
    if !lff::is_builtin(&name) {
        return StandIn::None;
    }
    let label = lff::get_font(&name).name.clone();
    if EXACT.contains(&key.as_str()) {
        StandIn::Exact(label)
    } else {
        StandIn::Approximate(label)
    }
}

/// The missing fonts of a drawing, sorted by what must happen with each.
pub struct FontReview {
    /// Fonts to ask about: approximate or no stand-in, and not decided yet.
    pub ask: Vec<String>,
    /// One line each for the fonts with an exact stand-in: nothing to ask.
    pub notes: Vec<String>,
}

/// `missing` is `io::font_repo::missing_shx_fonts`.
pub fn review(missing: Vec<String>) -> FontReview {
    let mut review = FontReview {
        ask: Vec::new(),
        notes: Vec::new(),
    };
    for name in missing {
        if chosen(&name).is_some_and(|sub| sub.usable()) {
            continue;
        }
        match stand_in(&name) {
            StandIn::Exact(label) => review.notes.push(
                crate::tf!("FONT  {name} not found: drawn with {label} (same widths).")
                    .into_owned(),
            ),
            _ => review.ask.push(name),
        }
    }
    review
}

/// A line of the dialog's menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    pub value: Substitute,
}

/// Symbol and shape fonts say nothing as text: not offered as a stand-in.
const NOT_FOR_TEXT: &[&str] = &[
    "gdt", "ltypeshp", "symbol", "symap", "symath", "symeteo", "symusic", "syastro",
];

/// Installed families offered as a stand-in. A family that shares its name with
/// an embedded stroke font would resolve to that one, so it is left out.
fn system_families() -> &'static [String] {
    static FAMILIES: OnceLock<Vec<String>> = OnceLock::new();
    FAMILIES.get_or_init(|| {
        sysfont::picker_families()
            .into_iter()
            .filter(|family| !lff::is_builtin(family))
            .collect()
    })
}

/// The menu for `file`: the default stand-in first, then the embedded stroke
/// fonts, then the fonts installed in Windows.
pub fn choices(file: &str) -> Vec<Choice> {
    let default_label = match stand_in(file) {
        StandIn::Exact(label) | StandIn::Approximate(label) => {
            crate::tf!("Default ({label})").into_owned()
        }
        StandIn::None => crate::t!("Default (Standard)").into_owned(),
    };
    let mut out = vec![Choice {
        label: default_label,
        value: Substitute::Default,
    }];
    for stem in lff::embedded_font_stems() {
        if NOT_FOR_TEXT.contains(&stem) {
            continue;
        }
        // The stem first: several fonts share a display name (`romans`,
        // `simplex` and `scripts` are all "Roman Simplex").
        let label = format!("{stem} — {} (stroke)", lff::get_font(stem).name);
        out.push(Choice {
            label,
            value: Substitute::Stroke(stem.to_string()),
        });
    }
    for family in system_families() {
        out.push(Choice {
            label: crate::tf!("{family} (Windows)").into_owned(),
            value: Substitute::System(family.clone()),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_ignore_folder_case_and_extension() {
        assert_eq!(key(r"C:\Fonts\IsoCT.SHX"), "isoct");
        assert_eq!(key("folder/romans.shx"), "romans");
        assert_eq!(key("txt"), "txt");
    }

    #[test]
    fn the_table_round_trips_through_its_saved_form() {
        choose("zz_roundtrip_a.shx", Substitute::Stroke("romans".into()));
        choose("ZZ_Roundtrip_B.SHX", Substitute::System("Arial".into()));
        choose("zz_roundtrip_c.shx", Substitute::Default);
        let saved = snapshot();
        assert_eq!(saved["zz_roundtrip_a"], "stroke:romans");
        assert_eq!(saved["zz_roundtrip_b"], "system:Arial");
        assert_eq!(saved["zz_roundtrip_c"], "default");

        let mut raw = saved.clone();
        raw.insert("zz_roundtrip_bad".into(), "nonsense".into());
        raw.insert("zz_roundtrip_empty".into(), "stroke:".into());
        set_table(&raw);
        let again = snapshot();
        assert_eq!(again["zz_roundtrip_a"], "stroke:romans");
        assert!(!again.contains_key("zz_roundtrip_bad"));
        assert!(!again.contains_key("zz_roundtrip_empty"));
    }

    #[test]
    fn stand_ins_are_told_apart() {
        assert!(matches!(stand_in("ROMANS.SHX"), StandIn::Exact(_)));
        assert!(matches!(stand_in("simplex.shx"), StandIn::Exact(_)));
        assert!(matches!(stand_in("txt.shx"), StandIn::Exact(_)));
        assert!(matches!(stand_in("isoct.shx"), StandIn::Approximate(_)));
        assert!(matches!(stand_in("monotxt.shx"), StandIn::Approximate(_)));
        assert_eq!(stand_in("zz_arqui_studio.shx"), StandIn::None);
    }

    #[test]
    fn exact_stand_ins_only_leave_a_note() {
        let review = review(vec![
            "romans.shx".into(),
            "zz_review_isoct.shx".into(),
            "zz_review_nobody.shx".into(),
        ]);
        assert_eq!(review.notes.len(), 1);
        assert!(review.notes[0].contains("romans.shx"));
        assert_eq!(
            review.ask,
            vec!["zz_review_isoct.shx", "zz_review_nobody.shx"]
        );
    }

    #[test]
    fn a_choice_made_is_not_asked_again() {
        choose("zz_decided.shx", Substitute::Default);
        choose("zz_decided_stroke.shx", Substitute::Stroke("romans".into()));
        let review = review(vec![
            "zz_decided.shx".into(),
            "zz_decided_stroke.shx".into(),
        ]);
        assert!(review.ask.is_empty());
        assert!(review.notes.is_empty());
    }

    #[test]
    fn a_choice_that_no_longer_works_is_asked_again() {
        choose(
            "zz_gone.shx",
            Substitute::System("Zz No Such Family".into()),
        );
        assert_eq!(replacement("zz_gone.shx"), None);
        assert_eq!(review(vec!["zz_gone.shx".into()]).ask, vec!["zz_gone.shx"]);
    }

    #[test]
    fn a_stroke_choice_replaces_the_name() {
        choose("zz_replaced.shx", Substitute::Stroke("romand".into()));
        assert_eq!(replacement("ZZ_Replaced.SHX").as_deref(), Some("romand"));
        // Default and unknown fonts leave the normal resolution alone.
        choose("zz_default.shx", Substitute::Default);
        assert_eq!(replacement("zz_default.shx"), None);
        assert_eq!(replacement("zz_never_seen.shx"), None);
    }

    #[test]
    fn the_choice_changes_what_a_text_style_is_drawn_with() {
        use crate::entities::text_support::{resolve_text_style, text_local_bounds};
        let mut doc = codec::CadDocument::new();
        let mut style = codec::TextStyle::new("ZZ_Quota");
        style.font_file = "zz_resolve_quota.shx".into();
        let _ = doc.text_styles.add(style);

        let before = resolve_text_style("ZZ_Quota", &doc).font_name;
        assert_eq!(before, "zz_resolve_quota.shx");
        choose("zz_resolve_quota.shx", Substitute::Stroke("romand".into()));
        let after = resolve_text_style("ZZ_Quota", &doc).font_name;
        assert_eq!(after, "romand");

        // The text really changes: another face, another width.
        let text = "SOGGIORNO 16.15 mq";
        let width = |font: &str| {
            text_local_bounds(font, text, 10.0, 1.0, 0.0)
                .unwrap()
                .advance
        };
        assert_ne!(width(&before), width(&after));
    }

    #[test]
    fn the_menu_starts_with_the_default_and_hides_symbol_fonts() {
        let menu = choices("isoct.shx");
        assert_eq!(menu[0].value, Substitute::Default);
        assert!(menu[0].label.contains("ISO3098"), "{}", menu[0].label);
        assert!(menu
            .iter()
            .any(|c| c.value == Substitute::Stroke("romans".into())));
        assert!(!menu
            .iter()
            .any(|c| c.value == Substitute::Stroke("symbol".into())));
        // Fonts sharing a display name stay distinguishable.
        let labels: Vec<&str> = menu.iter().map(|c| c.label.as_str()).collect();
        let mut unique = labels.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(labels.len(), unique.len(), "duplicate menu labels");
        // An embedded name is never offered as a Windows family.
        assert!(!menu
            .iter()
            .any(|c| c.value == Substitute::System("RomanS".into())));
    }
}
