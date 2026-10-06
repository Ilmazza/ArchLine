//! AutoCAD-classic toolbars (dockable on any edge or floating: see `toolbar_layout`).
//!
//! Built from the same `RibbonGroup` definitions the ribbon uses, so tool ids,
//! icons and commands stay in sync with upstream. Each ribbon dropdown becomes
//! ONE button that runs its default entry; the other entries open in a flyout
//! with a right click (`iced_aw::ContextMenu`).

use std::sync::OnceLock;

use iced::widget::{button, column, container, mouse_area, row, text, tooltip, Space};
use iced::{Background, Border, Element, Length, Theme};

use crate::app::Message;
use crate::modules::{registry, IconKind, ModuleEvent, RibbonItem, ToolDef};

pub(super) const BTN_SIZE: f32 = 36.0;
const ICON_SIZE: f32 = 24.0;
const FLYOUT_ICON_SIZE: f32 = 18.0;
const FLYOUT_WIDTH: f32 = 230.0;

/// One toolbar button: runs `main` on click; `variants` (if any) open on right click.
pub struct ClassicButton {
    pub main: ToolDef,
    pub variants: Vec<ToolDef>,
}

/// A toolbar entry.
pub enum ClassicItem {
    Button(ClassicButton),
    Separator,
}

/// Item lists for the three classic toolbars.
pub struct ClassicTools {
    pub draw: Vec<ClassicItem>,
    pub modify: Vec<ClassicItem>,
    pub annotation: Vec<ClassicItem>,
    pub block: Vec<ClassicItem>,
    pub measure: Vec<ClassicItem>,
}

pub(super) fn plain(t: &ToolDef) -> ClassicItem {
    ClassicItem::Button(ClassicButton {
        main: t.clone(),
        variants: Vec::new(),
    })
}

fn dropdown(items: &[(&'static str, &'static str, IconKind)], default: &str) -> Option<ClassicItem> {
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
    Some(ClassicItem::Button(ClassicButton { main, variants }))
}

fn flatten(item: &RibbonItem, out: &mut Vec<ClassicItem>) {
    match item {
        RibbonItem::Tool(t) | RibbonItem::LabeledTool(t) | RibbonItem::LargeTool(t) => {
            out.push(plain(t))
        }
        RibbonItem::Dropdown { items, default, .. }
        | RibbonItem::LabeledDropdown { items, default, .. }
        | RibbonItem::LargeDropdown { items, default, .. } => {
            out.extend(dropdown(items, default));
        }
        RibbonItem::ToolGrid { columns } => {
            for t in columns.iter().flatten() {
                out.push(plain(t));
            }
        }
        _ => {}
    }
}

/// Buttons of one ribbon group's items (dropdowns collapse to one button).
pub(super) fn buttons_of(tools: &[RibbonItem]) -> Vec<ClassicItem> {
    let mut out = Vec::new();
    for item in tools {
        flatten(item, &mut out);
    }
    out
}

/// Buttons of the draw-module groups named in `titles`, with a separator
/// between groups.
fn group_items(titles: &[&str]) -> Vec<ClassicItem> {
    let mut out: Vec<ClassicItem> = Vec::new();
    for module in registry::all_modules() {
        if module.id() != "draw" {
            continue;
        }
        for group in module.ribbon_groups() {
            if !titles.contains(&group.title) {
                continue;
            }
            if !out.is_empty() {
                out.push(ClassicItem::Separator);
            }
            for item in &group.tools {
                flatten(item, &mut out);
            }
        }
    }
    out
}

/// Item lists, computed once.
pub fn tools() -> &'static ClassicTools {
    static TOOLS: OnceLock<ClassicTools> = OnceLock::new();
    TOOLS.get_or_init(|| ClassicTools {
        draw: group_items(&["Draw"]),
        modify: group_items(&["Modify"]),
        annotation: group_items(&["Annotation"]),
        block: group_items(&["Block"]),
        measure: group_items(&["Measure"]),
    })
}

/// Item list of one toolbar. `Layers` is built by `classic_layers`, so its
/// list is empty.
pub fn items_for(id: crate::ui::toolbar_layout::ToolbarId) -> &'static [ClassicItem] {
    use crate::ui::toolbar_layout::ToolbarId::*;
    let t = tools();
    match id {
        Draw => &t.draw,
        Modify => &t.modify,
        Annotation => &t.annotation,
        Block => &t.block,
        Measure => &t.measure,
        Layers => &[],
        Group(_) => super::toolbar_registry::group_items(id),
    }
}

fn icon_el(icon: &IconKind, size: f32) -> Element<'static, Message> {
    match icon {
        IconKind::Glyph(s) => text(*s).size(size * 0.85).into(),
        IconKind::Svg(bytes) => crate::ui::icons::semantic(bytes, size),
    }
}

fn click(t: &ToolDef) -> Message {
    Message::RibbonToolClick {
        tool_id: t.id.to_string(),
        event: t.event.clone(),
    }
}

fn tip(label: &'static str, has_variants: bool) -> Element<'static, Message> {
    let label = crate::t!(label).replace('\n', " ");
    let mut col = column![text(label).size(11)].spacing(2);
    if has_variants {
        col = col.push(text(crate::t!("Right-click: more options").into_owned()).size(10));
    }
    container(col)
        .padding([2, 6])
        .style(|theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: Some(Background::Color(palette.background.strong.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                text_color: Some(palette.background.strong.text),
                ..Default::default()
            }
        })
        .into()
}

/// Flyout rows publish on PRESS through a single `mouse_area`: `iced_aw`'s
/// `ContextMenu` rebuilds its overlay every view, so a nested `button` would
/// lose its pressed state (see the note in `window/xref_manager.rs`).
#[allow(dead_code)] // used again by the long-press flyout
fn flyout_row(t: &ToolDef) -> Element<'static, Message> {
    mouse_area(
        container(
            row![
                icon_el(&t.icon, FLYOUT_ICON_SIZE),
                text(crate::t!(t.label).replace('\n', " ")).size(12)
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .padding([4, 10])
        .width(Length::Fill),
    )
    .on_press(click(t))
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}

#[allow(dead_code)] // used again by the long-press flyout
fn flyout(variants: &[ToolDef]) -> Element<'static, Message> {
    container(column(variants.iter().map(flyout_row)))
        .padding(2)
        .width(Length::Fixed(FLYOUT_WIDTH))
        .style(panel_style)
        .into()
}

pub(super) fn panel_style(theme: &Theme) -> container::Style {
    let palette = theme.palette();
    container::Style {
        background: Some(Background::Color(palette.background.weak.color)),
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    }
}

fn tool_button(b: &ClassicButton) -> Element<'static, Message> {
    let btn = button(icon_el(&b.main.icon, ICON_SIZE))
        .on_press(click(&b.main))
        .width(Length::Fixed(BTN_SIZE))
        .height(Length::Fixed(BTN_SIZE))
        .style(|theme: &Theme, status| {
            let palette = theme.palette();
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: hovered.then_some(Background::Color(palette.background.strong.color)),
                border: Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                text_color: palette.background.base.text,
                ..Default::default()
            }
        });
    let with_tip: Element<'static, Message> =
        tooltip(btn, tip(b.main.label, !b.variants.is_empty()), tooltip::Position::Bottom)
            .gap(4)
            .into();
    // Variants open on a long press (see the hold flyout); right click now
    // opens the bar list on the whole bar.
    with_tip
}

pub(super) fn separator(vertical: bool) -> Element<'static, Message> {
    let line = container(Space::new())
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.strong.color)),
            ..Default::default()
        });
    if vertical {
        line.width(Length::Fill).height(Length::Fixed(1.0)).into()
    } else {
        line.width(Length::Fixed(1.0)).height(Length::Fixed(BTN_SIZE - 8.0)).into()
    }
}

pub(super) fn item_el(item: &ClassicItem, vertical_bar: bool) -> Element<'static, Message> {
    match item {
        ClassicItem::Button(b) => tool_button(b),
        ClassicItem::Separator => separator(vertical_bar),
    }
}

pub(super) fn strip_style(theme: &Theme) -> container::Style {
    let palette = theme.palette();
    container::Style {
        background: Some(Background::Color(palette.background.weak.color)),
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buttons(v: &[ClassicItem]) -> Vec<&ClassicButton> {
        v.iter()
            .filter_map(|i| match i {
                ClassicItem::Button(b) => Some(b),
                ClassicItem::Separator => None,
            })
            .collect()
    }

    #[test]
    fn dropdowns_collapse_into_one_button_with_variants() {
        let draw = buttons(&tools().draw);
        let ids: Vec<_> = draw.iter().map(|b| b.main.id).collect();
        assert!(ids.contains(&"LINE"), "{ids:?}");
        let circle = draw
            .iter()
            .find(|b| b.variants.iter().any(|v| v.id == "CIRCLE_3P"))
            .expect("circle dropdown keeps its variants");
        assert_eq!(circle.main.id, "CIRCLE", "default entry is the main button");
        assert!(circle.variants.len() > 1);
        // The 3-point variant is no longer a toolbar button of its own.
        assert!(!ids.contains(&"CIRCLE_3P"), "{ids:?}");
    }

    #[test]
    fn draw_toolbar_is_compact_and_other_bars_are_populated() {
        let t = tools();
        assert!(buttons(&t.draw).len() <= 12, "draw has {} buttons", buttons(&t.draw).len());
        assert!(!buttons(&t.modify).is_empty());
        assert!(!buttons(&t.annotation).is_empty());
    }

    #[test]
    fn annotation_block_and_measure_are_separate_bars_with_buttons() {
        use crate::ui::toolbar_layout::ToolbarId::*;
        for id in [Annotation, Block, Measure] {
            assert!(!buttons(items_for(id)).is_empty(), "{id:?} has no buttons");
        }
        // Layers is built by classic_layers, not from an item list.
        assert!(items_for(Layers).is_empty());
    }

    #[test]
    fn no_main_button_is_duplicated() {
        let t = tools();
        for list in [&t.draw, &t.modify, &t.annotation, &t.block, &t.measure] {
            let mut ids: Vec<_> = buttons(list).iter().map(|b| b.main.id).collect();
            let n = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), n, "duplicate main buttons");
        }
    }
}
