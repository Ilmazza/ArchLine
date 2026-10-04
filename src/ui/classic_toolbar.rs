//! AutoCAD-classic docked toolbars (Draw left, Modify right, extras on top).
//!
//! Built from the same `RibbonGroup` definitions the ribbon uses, so tool ids,
//! icons and commands stay in sync with upstream. Dropdowns are expanded into
//! one button per entry.

use std::sync::OnceLock;

use iced::widget::{button, column, container, row, scrollable, tooltip};
use iced::{Background, Border, Element, Length, Theme};

use crate::app::Message;
use crate::modules::{
    registry, IconKind, ModuleEvent, RibbonItem, ToolDef,
};
use crate::ui::side_toolbar::{icon_el, tip_panel};

const BTN_SIZE: f32 = 32.0;

/// Tool lists for the three classic toolbars.
pub struct ClassicTools {
    pub draw: Vec<ToolDef>,
    pub modify: Vec<ToolDef>,
    pub extra: Vec<ToolDef>,
}

fn flatten(item: &RibbonItem, out: &mut Vec<ToolDef>) {
    match item {
        RibbonItem::Tool(t) | RibbonItem::LabeledTool(t) | RibbonItem::LargeTool(t) => {
            out.push(t.clone())
        }
        RibbonItem::Dropdown { items, .. }
        | RibbonItem::LabeledDropdown { items, .. }
        | RibbonItem::LargeDropdown { items, .. } => {
            for (id, label, icon) in items {
                let icon: IconKind = icon.clone();
                out.push(ToolDef {
                    id,
                    label,
                    icon,
                    event: ModuleEvent::Command((*id).to_string()),
                });
            }
        }
        RibbonItem::ToolGrid { columns } => {
            for c in columns {
                out.extend(c.iter().cloned());
            }
        }
        _ => {}
    }
}

fn group_tools(titles: &[&str]) -> Vec<ToolDef> {
    let mut out = Vec::new();
    for module in registry::all_modules() {
        if module.id() != "draw" {
            continue;
        }
        for group in module.ribbon_groups() {
            if titles.contains(&group.title) {
                for item in &group.tools {
                    flatten(item, &mut out);
                }
            }
        }
    }
    out
}

/// Tool lists, computed once.
pub fn tools() -> &'static ClassicTools {
    static TOOLS: OnceLock<ClassicTools> = OnceLock::new();
    TOOLS.get_or_init(|| ClassicTools {
        draw: group_tools(&["Draw"]),
        modify: group_tools(&["Modify"]),
        extra: group_tools(&["Annotation", "Block", "Measure"]),
    })
}

fn tool_button(t: &ToolDef) -> Element<'static, Message> {
    let btn = button(icon_el(t.icon.clone()))
        .on_press(Message::RibbonToolClick {
            tool_id: t.id.to_string(),
            event: t.event.clone(),
        })
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
    tooltip(btn, tip_panel(t.label), tooltip::Position::Bottom)
        .gap(4)
        .into()
}

fn strip_style(theme: &Theme) -> container::Style {
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
pub fn vertical(tools: &'static [ToolDef]) -> Element<'static, Message> {
    let col = column(tools.iter().map(tool_button)).spacing(2);
    container(scrollable(col))
        .padding(3)
        .height(Length::Fill)
        .style(strip_style)
        .into()
}

/// Horizontal toolbar for the top edge.
pub fn horizontal(tools: &'static [ToolDef]) -> Element<'static, Message> {
    let r = row(tools.iter().map(tool_button)).spacing(2);
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

    fn ids(v: &[ToolDef]) -> Vec<&'static str> {
        v.iter().map(|t| t.id).collect()
    }

    #[test]
    fn classic_toolbars_are_populated_from_ribbon_groups() {
        let t = tools();
        let draw = ids(&t.draw);
        assert!(draw.contains(&"LINE"), "{draw:?}");
        assert!(draw.contains(&"CIRCLE_3P"), "dropdown entries are expanded: {draw:?}");
        assert!(!ids(&t.modify).is_empty());
        assert!(!t.extra.is_empty());
    }
}
