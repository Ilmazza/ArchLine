//! AutoCAD-classic docked toolbars (Draw left, Modify right, extras on top).
//!
//! Built from the same `RibbonGroup` definitions the ribbon uses, so tool ids,
//! icons and commands stay in sync with upstream. Each ribbon dropdown becomes
//! ONE button that runs its default entry; the other entries open in a flyout
//! with a right click (`iced_aw::ContextMenu`).

use std::sync::OnceLock;

use iced::widget::{button, column, container, mouse_area, row, scrollable, text, tooltip, Space};
use iced::{Background, Border, Element, Length, Theme};

use crate::app::Message;
use crate::modules::{registry, IconKind, ModuleEvent, RibbonItem, ToolDef};

const BTN_SIZE: f32 = 36.0;
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
    pub extra: Vec<ClassicItem>,
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
        extra: group_items(&["Annotation", "Block", "Measure"]),
    })
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

fn flyout(variants: &[ToolDef]) -> Element<'static, Message> {
    container(column(variants.iter().map(flyout_row)))
        .padding(2)
        .width(Length::Fixed(FLYOUT_WIDTH))
        .style(panel_style)
        .into()
}

fn panel_style(theme: &Theme) -> container::Style {
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
    if b.variants.is_empty() {
        return with_tip;
    }
    let variants = b.variants.clone();
    iced_aw::ContextMenu::new(with_tip, move || flyout(&variants)).into()
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

/// Top toolbar row (Annotation / Block / Measure).
///
/// Kept out of `view_main` on purpose: that function is so large that extra
/// nesting there can overflow rustc's stack on Windows release builds.
#[inline(never)]
pub fn top_bar() -> Element<'static, Message> {
    horizontal(&tools().extra)
}

/// Surround the canvas with the Draw (left) and Modify (right) toolbars when
/// `classic` is set; otherwise return it untouched.
#[inline(never)]
pub fn wrap_center<'a>(classic: bool, center: Element<'a, Message>) -> Element<'a, Message> {
    if !classic {
        return center;
    }
    row![
        vertical(&tools().draw),
        container(center).width(Length::Fill).height(Length::Fill),
        vertical(&tools().modify),
    ]
    .height(Length::Fill)
    .into()
}

/// Vertical docked toolbar, pinned to the full height of its side.
pub fn vertical(items: &'static [ClassicItem]) -> Element<'static, Message> {
    let col = column(items.iter().map(|i| item_el(i, true))).spacing(3);
    container(scrollable(col))
        .padding(3)
        .height(Length::Fill)
        .style(strip_style)
        .into()
}

/// Horizontal toolbar for the top edge.
pub fn horizontal(items: &'static [ClassicItem]) -> Element<'static, Message> {
    let r = row(items.iter().map(|i| item_el(i, false)))
        .spacing(3)
        .align_y(iced::Center);
    container(scrollable(r).direction(scrollable::Direction::Horizontal(
        scrollable::Scrollbar::new().width(4).scroller_width(4),
    )))
    .padding(3)
    .width(Length::Fill)
    .style(strip_style)
    .into()
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
        assert!(!buttons(&t.extra).is_empty());
    }

    #[test]
    fn no_main_button_is_duplicated() {
        let t = tools();
        for list in [&t.draw, &t.modify, &t.extra] {
            let mut ids: Vec<_> = buttons(list).iter().map(|b| b.main.id).collect();
            let n = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), n, "duplicate main buttons");
        }
    }
}
