//! ArchLine: registry of every toolbar the user can show.
//!
//! Draw, Modify and Layers are built in (Draw and Modify carry every command of
//! their menu). Every other menu with commands of its own, and every submenu
//! with at least two, becomes a bar keyed `menu:Title`. A few AutoCAD toolbars
//! that no menu covers (Modeling, Viewports, Reference...) are gathered from
//! ribbon groups and keyed `ribbon:Title`.

use std::sync::OnceLock;

use super::classic_menu::{menus, toolbar_icon, Action, Entry};
use super::classic_toolbar::{ClassicButton, ClassicItem};
use super::toolbar_layout::ToolbarId;
use crate::modules::{registry, IconKind, ModuleEvent, RibbonItem, ToolDef};

/// Submenus with at least this many commands also get a bar of their own
/// (Zoom, Visual Styles, Inquiry, Text, the Parametric groups).
const MIN_SUBMENU_BAR: usize = 2;

struct Group {
    id: ToolbarId,
    items: Vec<ClassicItem>,
}

struct Registry {
    ids: Vec<ToolbarId>,
    groups: Vec<Group>,
    /// Every bar with its list name, alphabetical.
    names: Vec<(ToolbarId, &'static str)>,
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Two letters for a command with no icon: the initials of the first two words,
/// or the first two letters of a single word.
fn glyph_for(label: &str) -> &'static str {
    let words: Vec<&str> = label.split_whitespace().collect();
    let g: String = if words.len() >= 2 {
        words[..2]
            .iter()
            .filter_map(|w| w.chars().next())
            .flat_map(|c| c.to_uppercase())
            .collect()
    } else {
        let mut cs = label.chars();
        cs.next()
            .into_iter()
            .flat_map(|c| c.to_uppercase())
            .chain(cs.take(1).flat_map(|c| c.to_lowercase()))
            .collect()
    };
    leak(g)
}

/// The tool a menu row stands for: its command, label and icon.
fn tool_for(label: &'static str, command: &'static str) -> (ToolDef, bool) {
    let (icon, tinted) = match toolbar_icon(command) {
        Some((bytes, tinted)) => (IconKind::Svg(bytes), tinted),
        None => (IconKind::Glyph(glyph_for(label)), false),
    };
    let tool = ToolDef {
        id: command,
        label,
        icon,
        event: ModuleEvent::Command(command.to_string()),
    };
    (tool, tinted)
}

fn button(label: &'static str, command: &'static str) -> ClassicItem {
    let (main, tinted) = tool_for(label, command);
    ClassicItem::Button(ClassicButton {
        main,
        variants: Vec::new(),
        tinted,
    })
}

/// Commands run by the rows of `entries`, submenus flattened.
fn commands(entries: &'static [Entry], out: &mut Vec<(&'static str, &'static str)>) {
    for e in entries {
        match e {
            Entry::Item { label, action: Action::Cmd(c), .. } => out.push((label, c.as_str())),
            Entry::Sub { entries, .. } => commands(entries, out),
            _ => {}
        }
    }
}

/// Toolbar items for menu rows: one button per command, separators where the
/// menu has them, and a submenu as one button whose variants open on a long
/// press. Rows that are not commands (history toggle, window tabs) are left out.
fn entry_items(entries: &'static [Entry]) -> Vec<ClassicItem> {
    let mut out: Vec<ClassicItem> = Vec::new();
    for e in entries {
        match e {
            Entry::Item { label, action: Action::Cmd(c), .. } => out.push(button(label, c.as_str())),
            Entry::Sub { entries, .. } => {
                let mut cmds = Vec::new();
                commands(entries, &mut cmds);
                match cmds.as_slice() {
                    [] => {}
                    [(label, c)] => out.push(button(label, c)),
                    [(label, c), ..] => {
                        let (main, tinted) = tool_for(label, c);
                        let variants = cmds.iter().map(|(l, c)| tool_for(l, c).0).collect();
                        out.push(ClassicItem::Button(ClassicButton { main, variants, tinted }));
                    }
                }
            }
            Entry::Sep => out.push(ClassicItem::Separator),
            _ => {}
        }
    }
    tidy(out)
}

/// No separator first, last or twice in a row (rows dropped can leave them).
fn tidy(items: Vec<ClassicItem>) -> Vec<ClassicItem> {
    let mut tidy: Vec<ClassicItem> = Vec::new();
    for item in items {
        let sep = matches!(item, ClassicItem::Separator);
        if sep && matches!(tidy.last(), None | Some(ClassicItem::Separator)) {
            continue;
        }
        tidy.push(item);
    }
    while matches!(tidy.last(), Some(ClassicItem::Separator)) {
        tidy.pop();
    }
    tidy
}

/// AutoCAD toolbars no menu stands for, as (list name, ribbon groups they
/// gather as (module id, group title)); a separator goes between groups.
const RIBBON_BARS: &[(&str, &[(&str, &str)])] = &[
    ("Modeling", &[("model", "Create")]),
    ("Solid Editing", &[("model", "Boolean"), ("model", "Edges")]),
    ("Viewports", &[("view", "Model Viewports"), ("layout", "Viewport")]),
    ("Layouts", &[("layout", "Plot")]),
    ("Preset Views", &[("view", "Preset"), ("view", "Projection")]),
    ("Navigate", &[("view", "Navigate")]),
    ("Viewport Tools", &[("view", "Viewport Tools")]),
    ("Palettes", &[("view", "Palettes")]),
    ("Reference", &[("insert", "Reference")]),
    ("Block Definition", &[("insert", "Block Definition")]),
    ("Content", &[("insert", "Content")]),
    ("Group", &[("draw", "Groups")]),
    ("Centerlines", &[("annotate", "Centerlines")]),
    ("Markup", &[("annotate", "Markup"), ("annotate", "Tables")]),
    ("Annotation Scaling", &[("annotate", "Annotation Scaling")]),
    ("Cleanup", &[("manage", "Cleanup")]),
    ("Customization", &[("manage", "Customization")]),
];

/// A button or a separator of a bar defined here by command.
enum Cb {
    /// Label, command and an icon of its own (`None`: the menu icon, or a glyph).
    Btn(&'static str, &'static str, Option<&'static [u8]>),
    Sep,
}

use Cb::{Btn, Sep};

/// AutoCAD toolbars with neither a menu of their own nor a ribbon group (the
/// ribbon keeps Draw Order in a slide-out), or that gather commands from several
/// menus (Standard), as (list name, buttons).
const COMMAND_BARS: &[(&str, &[Cb])] = &[
    (
        "Standard",
        &[
            Btn("New", "NEW", None),
            Btn("Open", "OPEN", None),
            Btn("Save", "SAVE", None),
            Btn("Save As", "SAVEAS", None),
            Sep,
            Btn("Plot", "PLOT", None),
            Btn("Publish", "PUBLISH", None),
            Sep,
            Btn("Cut", "CUTCLIP", None),
            Btn("Copy", "COPYCLIP", None),
            Btn("Paste", "PASTECLIP", None),
            Btn("Match Properties", "MATCHPROP", None),
            Sep,
            Btn("Undo", "UNDO", None),
            Btn("Redo", "REDO", None),
            Sep,
            Btn("Pan", "PAN", None),
            Btn("Zoom Window", "ZOOM WINDOW", None),
            Btn("Zoom Previous", "ZOOM PREVIOUS", None),
            Btn("Zoom Extents", "ZOOM EXTENTS", None),
            Sep,
            Btn("Properties", "PROPERTIES", None),
            Btn("Design Center", "ADCENTER", None),
            Btn("Tool Palettes", "TOOLPALETTES", None),
            Btn("Sheet Set Manager", "SHEETSET", None),
            Btn("Calculator", "QUICKCALC", None),
            Sep,
            Btn("Help", "HELP", None),
            Sep,
            Btn("Close", "CLOSE", None),
            Btn("Exit", "QUIT", None),
        ],
    ),
    (
        // The combos are drawn by `classic_styles`; these are its manager buttons.
        "Styles",
        &[
            Btn("Text Style", "STYLE", None),
            Btn("Dimension Style", "DIMSTYLE", None),
            Btn("Table Style", "TABLESTYLE", None),
            Btn("Multileader Style", "MLEADERSTYLE", None),
        ],
    ),
    (
        "Draw Order",
        &[
            Btn(
                "Bring to Front",
                "DRAWORDER_FRONT",
                Some(include_bytes!("../../assets/icons/draworder_front.svg")),
            ),
            Btn(
                "Send to Back",
                "DRAWORDER_BACK",
                Some(include_bytes!("../../assets/icons/draworder_back.svg")),
            ),
            Btn(
                "Bring Above Objects",
                "DRAWORDER_ABOVE",
                Some(include_bytes!("../../assets/icons/draworder_above.svg")),
            ),
            Btn(
                "Send Under Objects",
                "DRAWORDER_UNDER",
                Some(include_bytes!("../../assets/icons/draworder_under.svg")),
            ),
        ],
    ),
];

/// Items of a [`COMMAND_BARS`] bar.
fn command_items(buttons: &'static [Cb]) -> Vec<ClassicItem> {
    let items = buttons
        .iter()
        .map(|b| match *b {
            Sep => ClassicItem::Separator,
            Btn(label, command, None) => button(label, command),
            Btn(label, command, Some(icon)) => {
                let main = ToolDef {
                    id: command,
                    label,
                    icon: IconKind::Svg(icon),
                    event: ModuleEvent::Command(command.to_string()),
                };
                ClassicItem::Button(ClassicButton { main, variants: Vec::new(), tinted: false })
            }
        })
        .collect();
    tidy(items)
}

fn ribbon_tool(t: &ToolDef) -> ClassicItem {
    ClassicItem::Button(ClassicButton {
        main: t.clone(),
        variants: Vec::new(),
        tinted: false,
    })
}

/// A ribbon dropdown as one button (its default variant) with the variants.
fn ribbon_dropdown(
    items: &[(&'static str, &'static str, IconKind)],
    default: &str,
) -> Option<ClassicItem> {
    let variants: Vec<ToolDef> = items
        .iter()
        .map(|(id, label, icon)| ToolDef {
            id,
            label,
            icon: icon.clone(),
            event: ModuleEvent::Command((*id).to_string()),
        })
        .collect();
    let main = variants
        .iter()
        .find(|t| t.id == default)
        .or_else(|| variants.first())?
        .clone();
    Some(ClassicItem::Button(ClassicButton { main, variants, tinted: false }))
}

fn ribbon_item(item: &RibbonItem, out: &mut Vec<ClassicItem>) {
    match item {
        RibbonItem::Tool(t) | RibbonItem::LabeledTool(t) | RibbonItem::LargeTool(t) => {
            out.push(ribbon_tool(t))
        }
        RibbonItem::Dropdown { items, default, .. }
        | RibbonItem::LabeledDropdown { items, default, .. }
        | RibbonItem::LargeDropdown { items, default, .. } => {
            out.extend(ribbon_dropdown(items, default));
        }
        RibbonItem::ToolGrid { columns } => {
            out.extend(columns.iter().flatten().map(ribbon_tool));
        }
        // Combo boxes and the like have no place on a button bar.
        _ => {}
    }
}

/// Buttons of the ribbon groups `groups` name, a separator between groups.
fn ribbon_items(groups: &[(&str, &str)]) -> Vec<ClassicItem> {
    let mut out: Vec<ClassicItem> = Vec::new();
    for (module_id, title) in groups {
        for module in registry::all_modules().iter().filter(|m| m.id() == *module_id) {
            for group in module.ribbon_groups().iter().filter(|g| g.title == *title) {
                out.push(ClassicItem::Separator);
                for item in &group.tools {
                    ribbon_item(item, &mut out);
                }
            }
        }
    }
    tidy(out)
}

/// Items of the menu called `title` (Draw and Modify bars use this).
pub fn menu_items(title: &str) -> Vec<ClassicItem> {
    menus()
        .iter()
        .find(|m| m.title == title)
        .map(|m| entry_items(&m.entries))
        .unwrap_or_default()
}

fn has_buttons(items: &[ClassicItem]) -> bool {
    items.iter().any(|i| matches!(i, ClassicItem::Button(_)))
}

fn build() -> Registry {
    // (id, list name, items)
    let mut raw: Vec<(ToolbarId, &'static str, Vec<ClassicItem>)> = Vec::new();
    let mut taken: Vec<String> = ToolbarId::BUILTIN
        .iter()
        .map(|b| b.key().to_lowercase())
        .collect();
    // The File menu's bar is the Standard bar of `COMMAND_BARS`.
    taken.push("file".to_string());
    let mut add = |title: &'static str, items: Vec<ClassicItem>| {
        if !has_buttons(&items) || taken.contains(&title.to_lowercase()) {
            return;
        }
        taken.push(title.to_lowercase());
        let key = leak(format!("menu:{title}"));
        raw.push((ToolbarId::Group(key), title, items));
    };
    for menu in menus() {
        // Draw and Modify are the built-in bars; the rest of a menu is a bar
        // when it has commands of its own (Parametric is only submenus).
        let own = menu
            .entries
            .iter()
            .any(|e| matches!(e, Entry::Item { action: Action::Cmd(_), .. }));
        if own {
            add(menu.title, entry_items(&menu.entries));
        }
        for e in &menu.entries {
            if let Entry::Sub { label, entries } = e {
                let mut cmds = Vec::new();
                commands(entries, &mut cmds);
                if cmds.len() >= MIN_SUBMENU_BAR {
                    add(label, entry_items(entries));
                }
            }
        }
    }
    for (title, groups) in RIBBON_BARS {
        let items = ribbon_items(groups);
        if !has_buttons(&items) || taken.contains(&title.to_lowercase()) {
            continue;
        }
        taken.push(title.to_lowercase());
        raw.push((ToolbarId::Group(leak(format!("ribbon:{title}"))), title, items));
    }
    for (title, buttons) in COMMAND_BARS {
        if taken.contains(&title.to_lowercase()) {
            continue;
        }
        taken.push(title.to_lowercase());
        raw.push((
            ToolbarId::Group(leak(format!("ribbon:{title}"))),
            title,
            command_items(buttons),
        ));
    }
    let mut names: Vec<(ToolbarId, &'static str)> =
        ToolbarId::BUILTIN.iter().map(|&id| (id, id.key())).collect();
    names.extend(raw.iter().map(|(id, title, _)| (*id, *title)));
    names.sort_by_key(|(_, n)| n.to_lowercase());
    let ids = ToolbarId::BUILTIN
        .iter()
        .copied()
        .chain(raw.iter().map(|r| r.0))
        .collect();
    let groups = raw
        .into_iter()
        .map(|(id, _, items)| Group { id, items })
        .collect();
    Registry { ids, groups, names }
}

fn registry_data() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(build)
}

/// Every bar, built-ins first.
pub fn all_ids() -> &'static [ToolbarId] {
    &registry_data().ids
}

/// Every bar with its list name, alphabetical (case-insensitive).
pub fn entries() -> &'static [(ToolbarId, &'static str)] {
    &registry_data().names
}

pub fn display_name(id: ToolbarId) -> &'static str {
    registry_data()
        .names
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, n)| *n)
        .unwrap_or_else(|| id.key())
}

/// Buttons of a menu bar (empty for a built-in).
pub fn group_items(id: ToolbarId) -> &'static [ClassicItem] {
    registry_data()
        .groups
        .iter()
        .find(|g| g.id == id)
        .map(|g| g.items.as_slice())
        .unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::classic_toolbar::{items_for, ClassicItem};
    use crate::ui::toolbar_layout::{ToolbarId, ToolbarLayout};

    fn groups() -> Vec<ToolbarId> {
        all_ids().iter().copied().filter(|id| !id.is_builtin()).collect()
    }

    #[test]
    fn the_historic_keys_still_resolve() {
        for key in ["Draw", "Modify", "Layers"] {
            let id = ToolbarId::from_key(key).unwrap_or_else(|| panic!("{key} missing"));
            assert_eq!(id.key(), key);
            assert!(id.is_builtin());
        }
        assert_eq!(ToolbarId::BUILTIN.len(), 3);
    }

    #[test]
    fn annotation_block_and_measure_are_retired_in_favour_of_the_menu_bars() {
        for key in ["Annotation", "Block", "Measure"] {
            assert_eq!(ToolbarId::from_key(key), None, "{key} should no longer exist");
        }
        let default_top: Vec<_> = ToolbarId::default_visible_groups()
            .iter()
            .map(|id| id.title())
            .collect();
        assert_eq!(default_top, ["Dimension", "Insert", "Inquiry"]);
        for id in ToolbarId::default_visible_groups() {
            assert!(!items_for(*id).is_empty(), "{} has no buttons", id.key());
        }
    }

    #[test]
    fn group_bars_exist_with_unique_prefixed_keys() {
        let g = groups();
        assert!(g.len() >= 5, "only {} group bars", g.len());
        let mut keys: Vec<_> = all_ids().iter().map(|id| id.key()).collect();
        let n = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), n, "duplicate bar keys");
        for id in g {
            assert!(id.key().contains(':'), "{} is not module:Title", id.key());
            assert_eq!(ToolbarId::from_key(id.key()), Some(id));
        }
    }

    #[test]
    fn every_offered_group_bar_has_buttons() {
        for id in groups() {
            assert!(
                items_for(id).iter().any(|i| matches!(i, ClassicItem::Button(_))),
                "{} has no buttons",
                id.key()
            );
        }
    }

    #[test]
    fn names_are_unique_sorted_and_builtins_stay_plain() {
        let e = entries();
        assert_eq!(e.len(), all_ids().len());
        let lower: Vec<String> = e.iter().map(|(_, n)| n.to_lowercase()).collect();
        let mut sorted = lower.clone();
        sorted.sort();
        assert_eq!(lower, sorted, "entries are alphabetical");
        let mut unique = lower.clone();
        unique.dedup();
        assert_eq!(unique.len(), lower.len(), "two bars share a name: {lower:?}");
        for b in ToolbarId::BUILTIN {
            assert_eq!(display_name(b), b.key(), "built-in names stay plain");
        }
    }

    #[test]
    fn group_bars_are_hidden_by_default_and_not_written_to_the_file() {
        // Review focus 2.
        let l = ToolbarLayout::default();
        assert_eq!(l.bars.len(), 6, "built-ins and the default top bars are materialised");
        let shown = ToolbarId::default_visible_groups();
        let hidden = groups().into_iter().find(|id| !shown.contains(id)).unwrap();
        assert!(!l.is_visible(hidden));
        let l = l.sanitized();
        assert_eq!(l.bars.len(), 6, "sanitize must not materialise ~30 hidden bars");
    }

    #[test]
    fn sanitize_keeps_known_group_keys_and_drops_unknown_ones() {
        // Review focus 1.
        let g = groups().into_iter().find(|id| id.title() == "Edit").unwrap();
        let json = format!(
            r#"{{"bars":{{"{}":{{"Floating":{{"x":10.0,"y":20.0,"home":null}}}},"gone:Removed":{{"Hidden":{{"home":null}}}}}}}}"#,
            g.key()
        );
        let l: ToolbarLayout = serde_json::from_str(&json).unwrap();
        let l = l.sanitized();
        assert!(l.is_visible(g), "a saved group bar keeps its position");
        assert!(!l.bars.contains_key("gone:Removed"));
    }

    use crate::ui::classic_menu::{menus, Action, Entry};

    /// Every command a menu (or one of its submenus) runs, in order.
    fn leaves(entries: &[Entry], out: &mut Vec<String>) {
        for e in entries {
            match e {
                Entry::Item { action: Action::Cmd(c), .. } => out.push(c.clone()),
                Entry::Sub { entries, .. } => leaves(entries, out),
                _ => {}
            }
        }
    }

    fn menu_commands(title: &str) -> Vec<String> {
        let m = menus().iter().find(|m| m.title == title).expect(title);
        let mut v = Vec::new();
        leaves(&m.entries, &mut v);
        v
    }

    fn bar_named(name: &str) -> ToolbarId {
        entries()
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(id, _)| *id)
            .unwrap_or_else(|| panic!("no bar named {name}"))
    }

    /// Commands a bar can run: main buttons and variants, in order, deduplicated
    /// only where a variant repeats its own button.
    fn bar_commands(id: ToolbarId) -> Vec<String> {
        let mut v = Vec::new();
        for i in items_for(id) {
            if let ClassicItem::Button(b) = i {
                if b.variants.is_empty() {
                    v.push(b.main.id.to_string());
                } else {
                    v.extend(b.variants.iter().map(|t| t.id.to_string()));
                }
            }
        }
        v
    }

    #[test]
    fn draw_and_modify_bars_carry_every_command_of_their_menu() {
        // The user's complaint: Draw lacked XLINE, SPLINE, DONUT, POINT, REGION,
        // TEXT/MTEXT; Modify lacked BREAK and PEDIT.
        for (id, menu) in [(ToolbarId::Draw, "Draw"), (ToolbarId::Modify, "Modify")] {
            assert_eq!(bar_commands(id), menu_commands(menu), "{menu} bar != {menu} menu");
        }
    }

    #[test]
    fn the_dimension_bar_mirrors_the_dimension_menu() {
        assert_eq!(bar_commands(bar_named("Dimension")), menu_commands("Dimension"));
    }

    #[test]
    fn the_visual_styles_bar_lists_every_style_of_the_menu() {
        let view = menus().iter().find(|m| m.title == "View").unwrap();
        let styles = view
            .entries
            .iter()
            .find_map(|e| match e {
                Entry::Sub { label: "Visual Styles", entries } => Some(entries),
                _ => None,
            })
            .expect("View > Visual Styles");
        let mut want = Vec::new();
        leaves(styles, &mut want);
        assert!(want.len() > 1);
        assert_eq!(bar_commands(bar_named("Visual Styles")), want);
    }

    #[test]
    fn there_is_a_bar_per_menu_and_per_command_submenu() {
        for name in [
            "Standard", "Edit", "View", "Insert", "Format", "Tools", "Help", "Dimension", "Zoom",
            "Visual Styles", "Inquiry", "Geometric", "Dimensional", "Manage",
        ] {
            bar_named(name);
        }
    }

    #[test]
    fn every_bar_comes_from_a_menu_or_from_a_named_ribbon_set() {
        for id in groups() {
            let k = id.key();
            assert!(k.starts_with("menu:") || k.starts_with("ribbon:"), "{k}");
        }
    }

    /// Commands of a bar in order, each dropdown contributing its main command.
    fn mains(id: ToolbarId) -> Vec<&'static str> {
        items_for(id)
            .iter()
            .filter_map(|i| match i {
                ClassicItem::Button(b) => Some(b.main.id),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_autocad_toolbars_without_a_menu_are_offered_and_hidden() {
        for name in [
            "Modeling", "Solid Editing", "Viewports", "Layouts", "Preset Views", "Navigate",
            "Viewport Tools", "Palettes", "Reference", "Block Definition", "Content", "Group",
            "Centerlines", "Markup", "Annotation Scaling", "Cleanup", "Customization",
            "Draw Order", "Standard", "Styles",
        ] {
            let id = bar_named(name);
            assert!(id.key().starts_with("ribbon:"), "{name}: {}", id.key());
            assert!(!ToolbarId::default_visible_groups().contains(&id), "{name} is on by default");
            assert!(!items_for(id).is_empty(), "{name} is empty");
        }
    }

    #[test]
    fn the_draw_order_bar_has_the_four_commands() {
        assert_eq!(
            bar_commands(bar_named("Draw Order")),
            ["DRAWORDER_FRONT", "DRAWORDER_BACK", "DRAWORDER_ABOVE", "DRAWORDER_UNDER"]
        );
    }

    #[test]
    fn autocad_classic_places_every_bar_of_the_screenshot() {
        use crate::ui::toolbar_layout::Edge;
        let l = ToolbarLayout::autocad_classic();
        let titles = |edge: Edge| -> Vec<Vec<&'static str>> {
            l.lanes(edge)
                .iter()
                .map(|lane| lane.iter().map(|id| id.title()).collect())
                .collect()
        };
        assert_eq!(
            titles(Edge::Top),
            vec![
                vec!["Standard", "Dimension", "Inquiry"],
                vec!["Layers", "Styles"],
                vec!["Insert", "Reference", "Ristrutturazione"],
            ]
        );
        assert_eq!(titles(Edge::Left), vec![vec!["Draw"]]);
        assert_eq!(titles(Edge::Right), vec![vec!["Modify", "Draw Order"]]);
        assert_eq!(
            titles(Edge::Bottom),
            vec![vec!["Visual Styles", "Preset Views", "Modeling", "Solid Editing"]]
        );
        // Every other bar is hidden.
        let shown = ToolbarId::all().iter().filter(|id| l.is_visible(**id)).count();
        assert_eq!(shown, 3 + 2 + 3 + 1 + 2 + 4);
    }

    #[test]
    fn ristrutturazione_bar_has_the_six_commands() {
        // ArchLine BIM: Tools > Ristrutturazione is a submenu with six commands,
        // so the registry makes the bar of the same name, one button each.
        let id = bar_named("Ristrutturazione");
        assert_eq!(id.key(), "menu:Ristrutturazione");
        assert_eq!(
            bar_commands(id),
            ["AL_DEMOLISCI", "AL_NUOVO", "AL_STATO", "AL_LOCALE", "AL_AGGIORNA", "AL_ESTRAI"]
        );
        // One plain button per command, no dropdown variants.
        for i in items_for(id) {
            if let ClassicItem::Button(b) = i {
                assert!(b.variants.is_empty(), "{} has variants", b.main.id);
            }
        }
        // Without an icon of their own the buttons fall back on a two-letter
        // glyph; no two of them may share it.
        let mut glyphs = Vec::new();
        for i in items_for(id) {
            if let ClassicItem::Button(b) = i {
                match b.main.icon {
                    IconKind::Glyph(g) => glyphs.push(g),
                    IconKind::Svg(_) => {}
                }
            }
        }
        let mut unique = glyphs.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), glyphs.len(), "two buttons share a glyph: {glyphs:?}");
    }

    #[test]
    fn autocad_classic_is_already_sanitized() {
        let l = ToolbarLayout::autocad_classic();
        assert_eq!(l, l.clone().sanitized());
    }

    #[test]
    fn from_title_is_case_insensitive() {
        assert_eq!(ToolbarId::from_title("styles"), Some(bar_named("Styles")));
        assert_eq!(ToolbarId::from_title("nope"), None);
    }

    #[test]
    fn the_standard_bar_replaces_file_and_runs_registered_commands() {
        assert!(entries().iter().all(|(_, n)| *n != "File"), "File is now Standard");
        let registered = crate::command::all_registered_command_names();
        let cmds = bar_commands(bar_named("Standard"));
        for c in ["NEW", "OPEN", "SAVE", "SAVEAS", "PLOT", "UNDO", "REDO", "PAN", "HELP", "CLOSE", "QUIT"] {
            assert!(cmds.iter().any(|x| x == c), "Standard lacks {c}");
        }
        for c in &cmds {
            let base = c.split(' ').next().unwrap();
            assert!(
                registered.contains(&c.as_str()) || registered.contains(&base),
                "{c} is not a registered command"
            );
        }
    }

    #[test]
    fn every_standard_and_styles_button_has_a_real_icon() {
        for name in ["Standard", "Styles"] {
            for i in items_for(bar_named(name)) {
                if let ClassicItem::Button(b) = i {
                    assert!(
                        matches!(b.main.icon, IconKind::Svg(_)),
                        "{name}: {} has only a glyph",
                        b.main.id
                    );
                }
            }
        }
    }

    #[test]
    fn modeling_and_solid_editing_carry_the_3d_commands() {
        let modeling = mains(bar_named("Modeling"));
        for c in ["EXTRUDE", "REVOLVE", "LOFT", "SWEEP", "PRESSPULL"] {
            assert!(modeling.contains(&c), "Modeling lacks {c}: {modeling:?}");
        }
        // The solid primitives are one button with the variants behind it.
        let boxes = items_for(bar_named("Modeling"))
            .iter()
            .find_map(|i| match i {
                ClassicItem::Button(b) if b.variants.len() > 1 && b.variants.iter().any(|v| v.id == "TORUS") => Some(b),
                _ => None,
            })
            .expect("a primitives button with variants");
        assert!(boxes.variants.iter().any(|v| v.id == "BOX"));
        let edit = items_for(bar_named("Solid Editing"));
        let ids = mains(bar_named("Solid Editing"));
        for c in ["UNION", "SUBTRACT", "INTERSECT", "FILLETEDGE", "CHAMFEREDGE", "SHELL"] {
            assert!(ids.contains(&c), "Solid Editing lacks {c}");
        }
        assert!(
            edit.iter().any(|i| matches!(i, ClassicItem::Separator)),
            "Boolean and edge commands are separated"
        );
    }

    #[test]
    fn viewports_and_layouts_gather_their_ribbon_groups() {
        let vp = mains(bar_named("Viewports"));
        for c in ["VPORTS", "VPORTS_NAMED", "VPJOIN", "VPORTS_RESTORE", "MVIEW"] {
            assert!(vp.contains(&c), "Viewports lacks {c}: {vp:?}");
        }
        let lay = mains(bar_named("Layouts"));
        assert!(lay.contains(&"PAGESETUP") && lay.contains(&"PRINTALL"), "{lay:?}");
    }

    #[test]
    fn bars_never_start_end_or_double_up_on_separators() {
        for &id in all_ids() {
            let items = items_for(id);
            if items.is_empty() {
                continue; // Layers is built by classic_layers
            }
            assert!(!matches!(items.first(), Some(ClassicItem::Separator)), "{}", id.key());
            assert!(!matches!(items.last(), Some(ClassicItem::Separator)), "{}", id.key());
            for w in items.windows(2) {
                assert!(
                    !(matches!(w[0], ClassicItem::Separator) && matches!(w[1], ClassicItem::Separator)),
                    "{} has two separators in a row",
                    id.key()
                );
            }
        }
    }

    #[test]
    fn a_command_with_no_icon_still_gets_a_visible_glyph() {
        use crate::modules::IconKind;
        let (tool, tinted) = tool_for("Some New Command", "NOSUCHCOMMAND");
        assert!(!tinted);
        match tool.icon {
            IconKind::Glyph(g) => assert_eq!(g, "SN"),
            _ => panic!("a command the app has no icon for must fall back to its initials"),
        }
    }

    #[test]
    fn the_formerly_lettered_buttons_now_carry_real_icons() {
        use crate::modules::IconKind;
        let edit = bar_named("Edit");
        for command in ["UNDO", "REDO"] {
            let b = items_for(edit)
                .iter()
                .find_map(|i| match i {
                    ClassicItem::Button(b) if b.main.id == command => Some(b),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Edit has {command}"));
            assert!(matches!(b.main.icon, IconKind::Svg(_)), "{command} still shows letters");
        }
    }
}
