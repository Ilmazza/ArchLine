//! ArchLine: registry of every toolbar the user can show.
//!
//! The six historic bars keep their names; every other ribbon group with at
//! least one button becomes a bar keyed `module:Title`.

use std::sync::OnceLock;

use super::classic_toolbar::{buttons_of, ClassicItem};
use super::toolbar_layout::ToolbarId;
use crate::modules::registry;

/// Ribbon groups of the `draw` module that are already the built-in bars.
const BUILTIN_GROUPS: [&str; 6] = ["Draw", "Modify", "Annotation", "Layers", "Block", "Measure"];

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

fn build() -> Registry {
    // (key id, group title, module title, items)
    let mut raw: Vec<(ToolbarId, &'static str, &'static str, Vec<ClassicItem>)> = Vec::new();
    for module in registry::all_modules() {
        for group in module.ribbon_groups() {
            if module.id() == "draw" && BUILTIN_GROUPS.contains(&group.title) {
                continue;
            }
            let items = buttons_of(&group.tools);
            if !items.iter().any(|i| matches!(i, ClassicItem::Button(_))) {
                continue;
            }
            let key = leak(format!("{}:{}", module.id(), group.title));
            raw.push((ToolbarId::Group(key), group.title, module.title(), items));
        }
    }
    // A title shared by two bars is told apart by its module.
    let shared = |title: &str| {
        let builtin = ToolbarId::BUILTIN
            .iter()
            .filter(|b| b.key().eq_ignore_ascii_case(title))
            .count();
        let groups = raw.iter().filter(|r| r.1.eq_ignore_ascii_case(title)).count();
        builtin + groups > 1
    };
    let mut names: Vec<(ToolbarId, &'static str)> =
        ToolbarId::BUILTIN.iter().map(|&id| (id, id.key())).collect();
    for (id, title, module_title, _) in &raw {
        let name = if shared(title) {
            leak(format!("{title} ({module_title})"))
        } else {
            *title
        };
        names.push((*id, name));
    }
    names.sort_by_key(|(_, n)| n.to_lowercase());
    let ids = ToolbarId::BUILTIN
        .iter()
        .copied()
        .chain(raw.iter().map(|r| r.0))
        .collect();
    let groups = raw
        .into_iter()
        .map(|(id, _, _, items)| Group { id, items })
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

/// Buttons of a group bar (empty for a built-in).
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
        for key in ["Draw", "Modify", "Annotation", "Block", "Measure", "Layers"] {
            let id = ToolbarId::from_key(key).unwrap_or_else(|| panic!("{key} missing"));
            assert_eq!(id.key(), key);
            assert!(id.is_builtin());
        }
        assert_eq!(ToolbarId::BUILTIN.len(), 6);
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
        assert_eq!(l.bars.len(), 6, "only the built-ins are materialised");
        let g = groups();
        assert!(!l.is_visible(g[0]));
        let l = l.sanitized();
        assert_eq!(l.bars.len(), 6, "sanitize must not materialise ~30 hidden bars");
    }

    #[test]
    fn sanitize_keeps_known_group_keys_and_drops_unknown_ones() {
        // Review focus 1.
        let g = groups()[0];
        let json = format!(
            r#"{{"bars":{{"{}":{{"Floating":{{"x":10.0,"y":20.0,"home":null}}}},"gone:Removed":{{"Hidden":{{"home":null}}}}}}}}"#,
            g.key()
        );
        let l: ToolbarLayout = serde_json::from_str(&json).unwrap();
        let l = l.sanitized();
        assert!(l.is_visible(g), "a saved group bar keeps its position");
        assert!(!l.bars.contains_key("gone:Removed"));
    }
}
