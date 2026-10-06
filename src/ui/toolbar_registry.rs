//! ArchLine: registry of every toolbar the user can show.
//!
//! The six historic bars keep their names (Draw and Modify now carry every
//! command of their menu). Every other menu with commands of its own, and every
//! submenu with at least two, becomes a bar keyed `menu:Title`.

use std::sync::OnceLock;

use super::classic_menu::{menus, toolbar_icon, Action, Entry};
use super::classic_toolbar::{ClassicButton, ClassicItem};
use super::toolbar_layout::ToolbarId;
use crate::modules::{IconKind, ModuleEvent, ToolDef};

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
    // No separator first, last or twice in a row (rows dropped above can leave them).
    let mut tidy: Vec<ClassicItem> = Vec::new();
    for item in out {
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
        let g = groups().into_iter().find(|id| id.title() == "File").unwrap();
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
            "File", "Edit", "View", "Insert", "Format", "Tools", "Help", "Dimension", "Zoom",
            "Visual Styles", "Inquiry", "Geometric", "Dimensional", "Manage",
        ] {
            bar_named(name);
        }
    }

    #[test]
    fn no_bar_comes_from_a_ribbon_group_any_more() {
        for id in groups() {
            assert!(id.key().starts_with("menu:"), "{} is not a menu bar", id.key());
        }
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
        let edit = bar_named("Edit");
        let undo = items_for(edit)
            .iter()
            .find_map(|i| match i {
                ClassicItem::Button(b) if b.main.id == "UNDO" => Some(b),
                _ => None,
            })
            .expect("Edit has UNDO");
        match undo.main.icon {
            IconKind::Glyph(g) => assert!(!g.trim().is_empty()),
            _ => panic!("UNDO has no icon in the app: expected a text glyph"),
        }
    }
}
