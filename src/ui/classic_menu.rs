//! Menu bar of the AutoCAD-classic workspace (File, Edit, View, …, Help).
//!
//! The tree below is data. Every leaf names a command (the string the command
//! line takes) or a direct message, and a test checks that each command
//! exists, so a renamed command fails the build instead of leaving a dead row.
//! Rows publish `Message::Command`, the path the shortcuts and the command
//! line already use.

use std::sync::OnceLock;

use crate::app::Message;
use crate::modules::{registry, ModuleEvent, RibbonItem, ToolDef};

/// What a menu row does when clicked.
#[derive(Clone)]
pub enum Action {
    /// `Message::Command(..)`: the same as typing the command.
    Cmd(String),
    /// A message that is not a typeable command (F2 history, Close All).
    Msg(fn() -> Message),
}

/// One row of a menu.
pub enum Entry {
    Item {
        /// English label; also the translation key.
        label: &'static str,
        action: Action,
        /// Shortcut-table action whose key is shown on the right, when it is
        /// not the command itself.
        accel: Option<&'static str>,
    },
    Sub {
        label: &'static str,
        entries: Vec<Entry>,
    },
    Sep,
    /// The open document tabs (Window menu), built from app state at draw time.
    Tabs,
}

pub struct MenuDef {
    pub title: &'static str,
    pub entries: Vec<Entry>,
}

fn cmd(label: &'static str, command: &str) -> Entry {
    Entry::Item {
        label,
        action: Action::Cmd(command.to_string()),
        accel: None,
    }
}

fn msg(label: &'static str, make: fn() -> Message, accel: &'static str) -> Entry {
    Entry::Item {
        label,
        action: Action::Msg(make),
        accel: Some(accel),
    }
}

fn sub(label: &'static str, entries: Vec<Entry>) -> Entry {
    Entry::Sub { label, entries }
}

fn push_tool(out: &mut Vec<Entry>, t: &ToolDef) {
    if let ModuleEvent::Command(command) = &t.event {
        out.push(cmd(t.label, command));
    }
}

/// Rows taken from a ribbon group, in ribbon order. Dropdowns are spread into
/// their items; only tools that run a command are kept.
fn group(module: &str, group: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    for m in registry::all_modules() {
        if m.id() != module {
            continue;
        }
        for g in m.ribbon_groups() {
            if g.title != group {
                continue;
            }
            for item in &g.tools {
                match item {
                    RibbonItem::Tool(t) | RibbonItem::LabeledTool(t) | RibbonItem::LargeTool(t) => {
                        push_tool(&mut out, t)
                    }
                    RibbonItem::Dropdown { items, .. }
                    | RibbonItem::LabeledDropdown { items, .. }
                    | RibbonItem::LargeDropdown { items, .. } => {
                        for (command, label, _) in items {
                            out.push(cmd(label, command));
                        }
                    }
                    RibbonItem::ToolGrid { columns } => {
                        for t in columns.iter().flatten() {
                            push_tool(&mut out, t);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

fn build() -> Vec<MenuDef> {
    use Entry::Sep;
    vec![
        MenuDef {
            title: "File",
            entries: vec![
                cmd("New", "NEW"),
                cmd("Open", "OPEN"),
                cmd("Save", "SAVE"),
                cmd("Save As", "SAVEAS"),
                cmd("Plot", "PLOT"),
                cmd("Close", "CLOSE"),
                cmd("Exit", "QUIT"),
            ],
        },
        MenuDef {
            title: "Edit",
            entries: vec![
                cmd("Undo", "UNDO"),
                cmd("Redo", "REDO"),
                Sep,
                cmd("Cut", "CUTCLIP"),
                cmd("Copy", "COPYCLIP"),
                cmd("Copy with Base Point", "COPYBASE"),
                cmd("Paste", "PASTECLIP"),
                cmd("Paste as Block", "PASTEBLOCK"),
                Sep,
                cmd("Select All", "SELECTALL"),
                cmd("Find", "FIND"),
            ],
        },
        MenuDef {
            title: "View",
            entries: vec![
                cmd("Regen", "REGEN"),
                cmd("Regen All", "REGENALL"),
                Sep,
                sub(
                    "Zoom",
                    vec![
                        cmd("Window", "ZOOM WINDOW"),
                        cmd("Previous", "ZOOM PREVIOUS"),
                        cmd("Extents", "ZOOM EXTENTS"),
                        cmd("All", "ZOOM ALL"),
                    ],
                ),
                cmd("Pan", "PAN"),
                cmd("3D Orbit", "3DORBIT"),
                sub("Visual Styles", group("view", "Visual Style")),
                Sep,
                cmd("Clean Screen", "CLEANSCREEN"),
                cmd("Properties", "PROPERTIES"),
                msg("Command History", || Message::CommandHistoryToggle, "COMMANDHISTORY"),
            ],
        },
        MenuDef {
            title: "Insert",
            entries: vec![
                cmd("Block", "BLOCK"),
                cmd("Insert", "INSERT"),
                Sep,
                cmd("Xref", "XATTACH"),
                cmd("Image", "IMAGEATTACH"),
                cmd("PDF", "PDFATTACH"),
                Sep,
                cmd("Field", "FIELD"),
            ],
        },
        MenuDef {
            title: "Format",
            entries: vec![
                cmd("Layer", "LAYERS"),
                cmd("Color", "COLOR"),
                cmd("Linetype", "LINETYPE"),
                Sep,
                cmd("Text Style", "STYLE"),
                cmd("Dimension Style", "DIMSTYLE"),
                Sep,
                cmd("Units", "UNITS"),
                cmd("Limits", "LIMITS"),
            ],
        },
        MenuDef {
            title: "Tools",
            entries: vec![
                sub(
                    "Inquiry",
                    vec![
                        cmd("Distance", "DIST"),
                        cmd("Area", "AREA"),
                        cmd("ID", "ID"),
                        cmd("List", "LIST"),
                    ],
                ),
                Sep,
                cmd("UCS", "UCS"),
                cmd("Drafting Settings", "DSETTINGS"),
                Sep,
                cmd("Aliases", "ALIASEDIT"),
                cmd("Shortcuts", "SHORTCUTS"),
                cmd("Options", "OPTIONS"),
                cmd("Plugins", "PLUGINS"),
            ],
        },
        MenuDef {
            title: "Draw",
            entries: vec![
                cmd("Line", "LINE"),
                cmd("Construction Line", "XLINE"),
                cmd("Polyline", "PLINE"),
                cmd("Polygon", "POLYGON"),
                cmd("Rectangle", "RECTANG"),
                cmd("Arc", "ARC"),
                cmd("Circle", "CIRCLE"),
                cmd("Donut", "DONUT"),
                cmd("Spline", "SPLINE"),
                cmd("Ellipse", "ELLIPSE"),
                Sep,
                cmd("Point", "POINT"),
                cmd("Hatch", "HATCH"),
                cmd("Region", "REGION"),
                Sep,
                sub(
                    "Text",
                    vec![cmd("Single Line Text", "TEXT"), cmd("Multiline Text", "MTEXT")],
                ),
            ],
        },
        MenuDef {
            title: "Dimension",
            entries: vec![
                cmd("Linear", "DIMLINEAR"),
                cmd("Aligned", "DIMALIGNED"),
                cmd("Arc Length", "DIMARC"),
                cmd("Ordinate", "DIMORDINATE"),
                cmd("Radius", "DIMRADIUS"),
                cmd("Diameter", "DIMDIAMETER"),
                cmd("Angular", "DIMANGULAR"),
                Sep,
                cmd("Baseline", "DIMBASELINE"),
                cmd("Continue", "DIMCONTINUE"),
                cmd("Quick Dimension", "QDIM"),
                Sep,
                cmd("Leader", "LEADER"),
                cmd("Multileader", "MLEADER"),
                cmd("Tolerance", "TOLERANCE"),
                Sep,
                cmd("Dimension Style", "DIMSTYLE"),
            ],
        },
        MenuDef {
            title: "Modify",
            entries: vec![
                cmd("Erase", "ERASE"),
                cmd("Copy", "COPY"),
                cmd("Mirror", "MIRROR"),
                cmd("Offset", "OFFSET"),
                cmd("Array", "ARRAY"),
                Sep,
                cmd("Move", "MOVE"),
                cmd("Rotate", "ROTATE"),
                cmd("Scale", "SCALE"),
                cmd("Stretch", "STRETCH"),
                Sep,
                cmd("Trim", "TRIM"),
                cmd("Extend", "EXTEND"),
                cmd("Break", "BREAK"),
                cmd("Chamfer", "CHAMFER"),
                cmd("Fillet", "FILLET"),
                Sep,
                cmd("Explode", "EXPLODE"),
                cmd("Polyline Edit", "PEDIT"),
            ],
        },
        MenuDef {
            title: "Parametric",
            entries: vec![
                sub("Geometric", group("parametric", "Geometric")),
                sub("Dimensional", group("parametric", "Dimensional")),
                sub("Manage", group("parametric", "Manage")),
            ],
        },
        MenuDef {
            title: "Window",
            entries: vec![
                Entry::Tabs,
                Sep,
                msg("Close All", || Message::DocTabCloseAll, "DOCTABCLOSEALL"),
            ],
        },
        MenuDef {
            title: "Help",
            entries: vec![cmd("Help", "HELP"), cmd("About", "ABOUT")],
        },
    ]
}

/// The menus, built once.
pub fn menus() -> &'static [MenuDef] {
    static MENUS: OnceLock<Vec<MenuDef>> = OnceLock::new();
    MENUS.get_or_init(build)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn leaves<'a>(entries: &'a [Entry], out: &mut Vec<&'a Action>) {
        for e in entries {
            match e {
                Entry::Item { action, .. } => out.push(action),
                Entry::Sub { entries, .. } => leaves(entries, out),
                Entry::Sep | Entry::Tabs => {}
            }
        }
    }

    fn commands(menu: &MenuDef) -> Vec<String> {
        let mut actions = Vec::new();
        leaves(&menu.entries, &mut actions);
        actions
            .into_iter()
            .filter_map(|a| match a {
                Action::Cmd(c) => Some(c.clone()),
                Action::Msg(_) => None,
            })
            .collect()
    }

    /// First word of a command line, upper case: `ZOOM WINDOW` → `ZOOM`.
    fn head(command: &str) -> String {
        command
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_uppercase()
    }

    fn check_shape(entries: &[Entry], at: &str) {
        assert!(!entries.is_empty(), "{at} is empty");
        assert!(
            !matches!(entries.first(), Some(Entry::Sep)),
            "{at} starts with a separator"
        );
        assert!(
            !matches!(entries.last(), Some(Entry::Sep)),
            "{at} ends with a separator"
        );
        for pair in entries.windows(2) {
            assert!(
                !(matches!(pair[0], Entry::Sep) && matches!(pair[1], Entry::Sep)),
                "{at} has two separators in a row"
            );
        }
        for e in entries {
            match e {
                Entry::Item { label, .. } => assert!(!label.is_empty(), "{at} has an empty label"),
                Entry::Sub { label, entries } => {
                    assert!(!label.is_empty(), "{at} has an empty submenu label");
                    check_shape(entries, label);
                }
                Entry::Sep | Entry::Tabs => {}
            }
        }
    }

    #[test]
    fn titles_follow_the_autocad_order() {
        let titles: Vec<_> = menus().iter().map(|m| m.title).collect();
        assert_eq!(
            titles,
            [
                "File", "Edit", "View", "Insert", "Format", "Tools", "Draw", "Dimension",
                "Modify", "Parametric", "Window", "Help"
            ]
        );
    }

    #[test]
    fn no_menu_is_empty_and_separators_only_sit_between_rows() {
        for m in menus() {
            check_shape(&m.entries, m.title);
        }
    }

    #[test]
    fn no_command_repeats_inside_one_menu() {
        for m in menus() {
            let cmds = commands(m);
            let unique: HashSet<_> = cmds.iter().collect();
            assert_eq!(unique.len(), cmds.len(), "{} repeats a command: {cmds:?}", m.title);
        }
    }

    #[test]
    fn every_command_exists() {
        let registered: HashSet<String> = crate::command::all_registered_command_names()
            .into_iter()
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let ribbon = registry::ribbon_commands();
        let ribbon_heads: HashSet<String> = ribbon.keys().map(|k| head(k)).collect();
        let mut missing = Vec::new();
        for m in menus() {
            for c in commands(m) {
                let known = ribbon.contains_key(&c.to_ascii_uppercase())
                    || registered.contains(&head(&c))
                    || ribbon_heads.contains(&head(&c));
                if !known {
                    missing.push(format!("{}: {c}", m.title));
                }
            }
        }
        assert!(missing.is_empty(), "commands with no source: {missing:#?}");
    }

    #[test]
    fn derived_groups_resolve_to_commands() {
        for (module, name) in [
            ("view", "Visual Style"),
            ("parametric", "Geometric"),
            ("parametric", "Dimensional"),
            ("parametric", "Manage"),
        ] {
            assert!(!group(module, name).is_empty(), "{module}/{name} is empty");
        }
        let styles = group("view", "Visual Style");
        assert!(styles.iter().all(|e| matches!(
            e,
            Entry::Item { action: Action::Cmd(c), .. } if c.starts_with("VISUALSTYLES ")
        )));
    }
}
