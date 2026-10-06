//! ArchLine: rendering of the dockable classic toolbars.
//!
//! Window layout: menu, tabs, **Top edge**, `[Left edge | centre | Right edge]`,
//! **Bottom edge**, status bar. Each edge is a stack of *lanes*; a lane is a
//! row (or column) of bars side by side. Lane 0 hugs the window edge.
//!
//! The model (`toolbar_layout`) decides where bars are; this module only draws.
//! Kept out of `view_main` on purpose: that function is so large that extra
//! nesting there can overflow rustc's stack on Windows release builds.

use iced::widget::{column, container, mouse_area, row, scrollable, Space};
use iced::{Background, Border, Color, Element, Length, Point, Theme};

use super::classic_layers::layer_row;
use super::classic_toolbar::{item_el, items_for, strip_style, ClassicItem, BTN_SIZE};
use super::ribbon::Ribbon;
use super::toolbar_layout::{Edge, ToolbarId, ToolbarLayout};
use crate::app::Message;

/// Offset from the pointer to a dragged bar's top-left: the user holds the
/// grip, which sits in the bar's corner.
pub const GRIP_ANCHOR: f32 = 10.0;
const GRIP_W: f32 = 8.0;
/// Estimated length of the layer / properties bar (combo 220, 3×130 combos,
/// 11 buttons, separators, spacing).
const LAYERS_LENGTH: f32 = 1050.0;

#[derive(Clone, Debug)]
pub enum ToolbarMsg {
    /// The grip (or a floating bar's title) was pressed.
    Grab(ToolbarId),
    /// Pointer moved while dragging (position in the toolbar frame).
    DragMove(Point),
    DragRelease,
    /// Double click on a floating bar's title.
    Redock(ToolbarId),
    /// Put every bar back where it was at first launch.
    Reset,
}

/// Estimated length of a bar along its own axis (px), grip included.
pub fn bar_length(id: ToolbarId) -> f32 {
    if id == ToolbarId::Layers {
        return LAYERS_LENGTH;
    }
    let items: f32 = items_for(id)
        .iter()
        .map(|i| match i {
            ClassicItem::Button(_) => BTN_SIZE + 3.0,
            ClassicItem::Separator => 4.0,
        })
        .sum();
    items + GRIP_W + 12.0
}

/// `(width, height)` of a bar.
pub fn bar_size(id: ToolbarId, vertical: bool) -> (f32, f32) {
    let l = bar_length(id);
    let t = super::toolbar_layout::LANE_THICKNESS;
    if vertical {
        (t, l)
    } else {
        (l, t)
    }
}

fn grip(id: ToolbarId, vertical: bool) -> Element<'static, Message> {
    let line = || {
        container(Space::new()).style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.neutral.color)),
            ..Default::default()
        })
    };
    let span = Length::Fixed(BTN_SIZE - 8.0);
    let lines: Element<'static, Message> = if vertical {
        column![line().width(span).height(2), line().width(span).height(2)]
            .spacing(2)
            .into()
    } else {
        row![line().width(2).height(span), line().width(2).height(span)]
            .spacing(2)
            .into()
    };
    mouse_area(container(lines).padding(2))
        .on_press(Message::Toolbar(ToolbarMsg::Grab(id)))
        .interaction(iced::mouse::Interaction::Grab)
        .into()
}

fn bar_body(id: ToolbarId, vertical: bool, ribbon: &Ribbon) -> Element<'static, Message> {
    if id == ToolbarId::Layers {
        // Combo boxes: horizontal only (the model never docks it on a side).
        return layer_row(ribbon).into();
    }
    let items = items_for(id);
    if vertical {
        column(items.iter().map(|i| item_el(i, true))).spacing(3).into()
    } else {
        row(items.iter().map(|i| item_el(i, false)))
            .spacing(3)
            .align_y(iced::Center)
            .into()
    }
}

/// One docked bar: grip + buttons. `being_dragged` tints it while its ghost
/// follows the pointer.
fn bar_el(
    id: ToolbarId,
    vertical: bool,
    ribbon: &Ribbon,
    being_dragged: bool,
) -> Element<'static, Message> {
    let body = bar_body(id, vertical, ribbon);
    let inner: Element<'static, Message> = if vertical {
        column![grip(id, true), body].spacing(3).into()
    } else {
        row![grip(id, false), body]
            .spacing(3)
            .align_y(iced::Center)
            .into()
    };
    container(inner)
        .padding(2)
        .style(move |theme: &Theme| {
            let p = theme.palette();
            container::Style {
                background: being_dragged
                    .then(|| Background::Color(p.background.weakest.color.scale_alpha(0.5))),
                border: Border {
                    color: if being_dragged {
                        p.primary.base.color
                    } else {
                        Color::TRANSPARENT
                    },
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            }
        })
        .into()
}

fn lane_el(
    bars: &[ToolbarId],
    vertical: bool,
    ribbon: &Ribbon,
    dragging: Option<ToolbarId>,
) -> Element<'static, Message> {
    let els: Vec<Element<'static, Message>> = bars
        .iter()
        .map(|&id| bar_el(id, vertical, ribbon, dragging == Some(id)))
        .collect();
    if vertical {
        container(scrollable(column(els).spacing(3)))
            .padding(3)
            .height(Length::Fill)
            .style(strip_style)
            .into()
    } else {
        container(
            scrollable(row(els).spacing(3).align_y(iced::Center)).direction(
                scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::new().width(4).scroller_width(4),
                ),
            ),
        )
        .padding(3)
        .width(Length::Fill)
        .style(strip_style)
        .into()
    }
}

fn edge_el(
    layout: &ToolbarLayout,
    edge: Edge,
    ribbon: &Ribbon,
    dragging: Option<ToolbarId>,
) -> Element<'static, Message> {
    let mut lanes = layout.lanes(edge);
    if lanes.is_empty() {
        return Space::new().width(0).height(0).into();
    }
    // Lane 0 hugs the window edge: it is last on the bottom and right edges.
    if matches!(edge, Edge::Bottom | Edge::Right) {
        lanes.reverse();
    }
    let vertical = edge.is_vertical();
    let els: Vec<Element<'static, Message>> = lanes
        .iter()
        .map(|l| lane_el(l, vertical, ribbon, dragging))
        .collect();
    if vertical {
        row(els).height(Length::Fill).into()
    } else {
        column(els).into()
    }
}

/// Surround `center` with the four toolbar edges when `classic` is set;
/// otherwise return it untouched.
#[inline(never)]
pub fn frame<'a>(
    classic: bool,
    layout: &ToolbarLayout,
    ribbon: &Ribbon,
    dragging: Option<ToolbarId>,
    center: Element<'a, Message>,
) -> Element<'a, Message> {
    if !classic {
        return center;
    }
    let middle = row![
        edge_el(layout, Edge::Left, ribbon, dragging),
        container(center).width(Length::Fill).height(Length::Fill),
        edge_el(layout, Edge::Right, ribbon, dragging),
    ]
    .height(Length::Fill);
    column![
        edge_el(layout, Edge::Top, ribbon, dragging),
        middle,
        edge_el(layout, Edge::Bottom, ribbon, dragging),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
