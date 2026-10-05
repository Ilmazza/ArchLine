//! Menu bar of the AutoCAD-classic workspace (File, Edit, View, …, Help).
//!
//! The tree below is data. Every leaf names a command (the string the command
//! line takes) or a direct message, and a test checks that each command
//! exists, so a renamed command fails the build instead of leaving a dead row.
//! Rows publish `Message::Command`, the path the shortcuts and the command
//! line already use.

use std::sync::OnceLock;

use iced::widget::{button, container, row, text, Space};
use iced::{Background, Border, Color, Element, Length, Shadow, Theme};
use iced_aw::menu::{DrawPath, Item, Menu, MenuBar};
use rustc_hash::FxHashMap;
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

/// `CTRL+SHIFT+S` → `Ctrl+Shift+S`. Single keys and function keys stay as
/// written (`S`, `F8`).
pub fn format_accel(key: &str) -> String {
    key.split('+')
        .map(|part| {
            let is_function_key = part.len() > 1
                && part.starts_with('F')
                && part[1..].chars().all(|c| c.is_ascii_digit());
            if part.chars().count() == 1 || is_function_key {
                return part.to_string();
            }
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// The shortcut shown for `action`: the shortest key bound to it, ties broken
/// alphabetically (so Redo shows `Ctrl+Y`, not `Ctrl+Shift+Z`).
pub fn accel_for(bindings: &FxHashMap<String, String>, action: &str) -> Option<String> {
    bindings
        .iter()
        .filter(|(_, bound)| bound.eq_ignore_ascii_case(action))
        .map(|(key, _)| key)
        .min_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)))
        .map(|key| format_accel(key))
}

type BarItem = Item<'static, Message, Theme, iced::Renderer>;
type BarMenu = Menu<'static, Message, Theme, iced::Renderer>;

const MENU_W: f32 = 270.0;
const ICON: f32 = 16.0;
const TAB_NAME_MAX: usize = 40;

/// One open document tab, for the Window menu.
pub struct TabEntry {
    pub index: usize,
    pub name: String,
    pub active: bool,
}

/// App state the menu needs, so this file does not depend on the app type.
pub struct MenuCtx<'a> {
    pub bindings: &'a FxHashMap<String, String>,
    pub tabs: Vec<TabEntry>,
}

/// Translated label; the English key itself when the catalog has no entry.
/// Line breaks of ribbon labels become spaces.
fn label_text(key: &str) -> String {
    crate::t!(key).replace('\n', " ")
}

fn tab_label(name: &str) -> String {
    crate::ui::text_util::elide(name, TAB_NAME_MAX).to_string()
}

fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.6)),
    }
}

fn lead(icon: Option<&'static [u8]>) -> Element<'static, Message> {
    match icon {
        Some(bytes) => crate::ui::icons::semantic(bytes, ICON),
        None => Space::new().width(ICON).height(ICON).into(),
    }
}

fn row_content(
    lead: Element<'static, Message>,
    label: String,
    accel: Option<String>,
    arrow: bool,
) -> Element<'static, Message> {
    let mut r = row![lead, text(label).size(12), Space::new().width(Length::Fill)]
        .spacing(8)
        .align_y(iced::Center);
    if let Some(accel) = accel {
        r = r.push(text(accel).size(11).style(muted));
    }
    if arrow {
        r = r.push(text("▸").size(12));
    }
    container(r).padding([4, 10]).width(Length::Fill).into()
}

fn row_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then_some(Background::Color(palette.primary.weak.color)),
        text_color: if hovered {
            palette.primary.weak.text
        } else {
            palette.background.base.text
        },
        border: Border {
            radius: 2.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn row_button(content: Element<'static, Message>, message: Message) -> Element<'static, Message> {
    button(content)
        .on_press(message)
        .width(Length::Fill)
        .padding(0)
        .style(row_style)
        .into()
}

fn separator_row() -> Element<'static, Message> {
    let line = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.strong.color)),
            ..Default::default()
        });
    container(line).padding([3, 6]).width(Length::Fill).into()
}

fn click(action: &Action) -> Message {
    match action {
        Action::Cmd(command) => Message::Command(command.clone()),
        Action::Msg(make) => make(),
    }
}

fn entry_items(entries: &[Entry], ctx: &MenuCtx<'_>) -> Vec<BarItem> {
    let mut out: Vec<BarItem> = Vec::new();
    for e in entries {
        match e {
            Entry::Item {
                label,
                action,
                accel,
            } => {
                let key: Option<&str> = (*accel).or(match action {
                    Action::Cmd(c) => Some(c.as_str()),
                    Action::Msg(_) => None,
                });
                let accel_text = key.and_then(|k| accel_for(ctx.bindings, k));
                let icon = match action {
                    Action::Cmd(c) => registry::command_icon(c),
                    Action::Msg(_) => None,
                };
                let content = row_content(lead(icon), label_text(label), accel_text, false);
                out.push(Item::new(row_button(content, click(action))).close_on_click(true));
            }
            Entry::Sub { label, entries } => {
                let header = row_button(
                    row_content(lead(None), label_text(label), None, true),
                    Message::Noop,
                );
                let menu: BarMenu = Menu::new(entry_items(entries, ctx))
                    .width(Length::Fixed(MENU_W))
                    .padding(2)
                    .spacing(0)
                    .offset(0.0);
                out.push(Item::with_menu(header, menu).close_on_click(false));
            }
            Entry::Sep => out.push(Item::new(separator_row())),
            Entry::Tabs => {
                for tab in &ctx.tabs {
                    let mark: Element<'static, Message> = if tab.active {
                        text("✓").size(12).width(ICON).into()
                    } else {
                        lead(None)
                    };
                    let content = row_content(mark, tab_label(&tab.name), None, false);
                    out.push(
                        Item::new(row_button(content, Message::TabSwitch(tab.index)))
                            .close_on_click(true),
                    );
                }
            }
        }
    }
    out
}

fn root_button(title: &'static str) -> Element<'static, Message> {
    button(text(label_text(title)).size(12))
        .on_press(Message::Noop)
        .padding([3, 10])
        .style(row_style)
        .into()
}

/// The menu bar, to sit above the document tabs.
///
/// Kept out of `view_main` on purpose (see `classic_toolbar::top_bar`).
#[inline(never)]
pub fn menu_bar(ctx: &MenuCtx<'_>) -> Element<'static, Message> {
    let roots: Vec<BarItem> = menus()
        .iter()
        .map(|m| {
            let menu: BarMenu = Menu::new(entry_items(&m.entries, ctx))
                .width(Length::Fixed(MENU_W))
                .padding(2)
                .spacing(0)
                .offset(1.0)
                .close_on_background_click(true);
            Item::with_menu(root_button(m.title), menu)
        })
        .collect();
    let bar = MenuBar::new(roots)
        .spacing(0)
        .draw_path(DrawPath::Backdrop)
        .close_on_background_click_global(true)
        .style(|theme: &Theme, _| {
            let palette = theme.palette();
            iced_aw::style::menu_bar::Style {
                bar_background: Background::Color(Color::TRANSPARENT),
                bar_border: Border::default(),
                bar_shadow: Shadow::default(),
                menu_background: Background::Color(palette.background.weakest.color),
                menu_border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                menu_shadow: Shadow {
                    color: palette.background.strongest.color.scale_alpha(0.35),
                    offset: iced::Vector::new(0.0, 2.0),
                    blur_radius: 6.0,
                },
                path: Background::Color(palette.primary.weak.color),
                path_border: Border {
                    color: palette.primary.base.color,
                    width: 1.0,
                    radius: 2.0.into(),
                },
            }
        });
    container(bar)
        .width(Length::Fill)
        .style(super::classic_toolbar::strip_style)
        .into()
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

    use rustc_hash::FxHashMap;

    fn bindings(pairs: &[(&str, &str)]) -> FxHashMap<String, String> {
        pairs
            .iter()
            .map(|(k, a)| (k.to_string(), a.to_string()))
            .collect()
    }

    #[test]
    fn accelerators_are_written_like_a_menu() {
        assert_eq!(format_accel("CTRL+SHIFT+S"), "Ctrl+Shift+S");
        assert_eq!(format_accel("CTRL+O"), "Ctrl+O");
        assert_eq!(format_accel("F8"), "F8");
        assert_eq!(format_accel("F12"), "F12");
        assert_eq!(format_accel("DELETE"), "Delete");
    }

    #[test]
    fn the_shortest_key_wins_and_ties_are_alphabetical() {
        let b = bindings(&[("CTRL+SHIFT+Z", "REDO"), ("CTRL+Y", "REDO"), ("CTRL+Z", "UNDO")]);
        assert_eq!(accel_for(&b, "REDO").as_deref(), Some("Ctrl+Y"));
        assert_eq!(accel_for(&b, "UNDO").as_deref(), Some("Ctrl+Z"));
        let tie = bindings(&[("CTRL+H", "FIND"), ("CTRL+F", "FIND")]);
        assert_eq!(accel_for(&tie, "FIND").as_deref(), Some("Ctrl+F"));
    }

    #[test]
    fn no_binding_means_no_accelerator() {
        assert_eq!(accel_for(&FxHashMap::default(), "SAVE"), None);
        let b = bindings(&[("CTRL+S", "SAVE")]);
        assert_eq!(accel_for(&b, "SAVEAS"), None);
        assert_eq!(accel_for(&b, "DOCTABCLOSEALL"), None);
    }

    #[test]
    fn the_action_name_is_matched_without_regard_to_case() {
        let b = bindings(&[("CTRL+S", "save")]);
        assert_eq!(accel_for(&b, "SAVE").as_deref(), Some("Ctrl+S"));
    }
}

#[cfg(test)]
mod build_tests {
    use super::*;

    fn tabs(n: usize, name: &str) -> Vec<TabEntry> {
        (0..n)
            .map(|index| TabEntry {
                index,
                name: format!("{name} {index}"),
                active: index == 0,
            })
            .collect()
    }

    /// The bar is a full-width strip. Reading its size asserts something real
    /// and proves the whole widget tree was built without panicking.
    fn spans_the_window(el: &Element<'static, Message>) -> bool {
        el.as_widget().size().width == Length::Fill
    }

    #[test]
    fn the_bar_builds_without_bindings_or_tabs() {
        let bindings = FxHashMap::default();
        let bar = menu_bar(&MenuCtx {
            bindings: &bindings,
            tabs: Vec::new(),
        });
        assert!(spans_the_window(&bar));
    }

    #[test]
    fn the_bar_builds_with_many_long_non_ascii_tabs() {
        let bindings = FxHashMap::default();
        let bar = menu_bar(&MenuCtx {
            bindings: &bindings,
            tabs: tabs(60, "Pianta piano terra — lungo nome àèìòù con molte parole"),
        });
        assert!(spans_the_window(&bar));
    }

    #[test]
    fn tab_names_are_elided() {
        let long = "x".repeat(200);
        assert!(tab_label(&long).chars().count() <= TAB_NAME_MAX + 1);
        assert_eq!(tab_label("Drawing1.dwg"), "Drawing1.dwg");
    }

    #[test]
    fn a_label_without_a_catalog_entry_stays_english() {
        let key: &str = "Parametric";
        assert_eq!(label_text(key), "Parametric");
        assert!(!label_text("Visual\nStyle").contains('\n'));
    }
}
