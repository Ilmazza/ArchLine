//! ArchLine: rendering of the dockable classic toolbars.
//!
//! Window layout: menu, tabs, **Top edge**, `[Left edge | centre | Right edge]`,
//! **Bottom edge**, status bar. Each edge is a stack of *lanes*; a lane is a
//! row (or column) of bars side by side. Lane 0 hugs the window edge.
//!
//! The model (`toolbar_layout`) decides where bars are; this module only draws.
//! Kept out of `view_main` on purpose: that function is so large that extra
//! nesting there can overflow rustc's stack on Windows release builds.

use std::sync::Arc;

use iced::widget::{column, container, mouse_area, opaque, pin, row, scrollable, text, Space, Stack};
use iced::{Background, Border, Color, Element, Length, Point, Theme};

use super::classic_layers::layer_row;
use super::classic_toolbar::{
    item_el, items_for, panel_style, strip_style, ClassicItem, BTN_GAP, BTN_SIZE, SEP_SPAN,
};
use super::ribbon::Ribbon;
use super::toolbar_layout::{
    band_rect, clamp_floating, insertion_rect, Edge, Target, ToolbarId, ToolbarLayout,
};
use crate::app::Message;

/// Offset from the pointer to a dragged bar's top-left: the user holds the
/// grip, which sits in the bar's corner.
pub const GRIP_ANCHOR: f32 = 10.0;
const GRIP_W: f32 = 8.0;
/// Distance (px) the pointer must travel from the grab before it is a drag.
pub const DRAG_THRESHOLD: f32 = 4.0;
/// How long a press must last to open the variants flyout (ms).
pub const HOLD_MS: u64 = 400;
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
    /// Tick / untick a bar in the right-click list.
    Toggle(ToolbarId),
    /// A button with variants was pressed: start timing the hold.
    HoldStart(ToolbarId, &'static str),
    /// The pointer left the button before the hold fired.
    HoldCancel,
    /// Time passed while a hold is pending.
    HoldTick(iced::time::Instant),
    /// The button was released: run it, unless the hold already opened the
    /// variants flyout.
    HoldEnd {
        tool_id: String,
        event: crate::modules::ModuleEvent,
    },
}

/// Transient state of a toolbar drag.
#[derive(Clone, Debug)]
pub struct ToolbarDrag {
    pub id: ToolbarId,
    /// Where the pointer was first seen after the grab. The drag layer is
    /// mounted fresh and reports a move at the press position, so a drag only
    /// starts once the pointer travels `DRAG_THRESHOLD` from here.
    pub origin: Option<Point>,
    /// Last pointer position, in the toolbar frame; `None` until the drag starts.
    pub cursor: Option<Point>,
    /// Where releasing now would put the bar; `None` until it moves.
    pub target: Option<Target>,
}

/// One row of the right-click bar list.
#[derive(Clone, Debug)]
pub struct BarEntry {
    pub id: ToolbarId,
    pub name: &'static str,
    pub visible: bool,
}

/// What every bar needs to open the list: the entries (built once per view and
/// shared) and the height the list may take.
#[derive(Clone)]
struct BarsMenu {
    entries: Arc<[BarEntry]>,
    max_h: f32,
}

/// Every bar, alphabetical, with whether it is currently shown.
pub fn menu_entries(layout: &ToolbarLayout) -> Arc<[BarEntry]> {
    super::toolbar_registry::entries()
        .iter()
        .map(|&(id, name)| BarEntry {
            id,
            name,
            visible: layout.is_visible(id),
        })
        .collect::<Vec<_>>()
        .into()
}

/// Lower bound of a bar-list row's height (text 12 + padding), in px.
const BAR_ROW_H: f32 = 22.0;

fn bars_menu_for(layout: &ToolbarLayout, win_h: f32) -> BarsMenu {
    BarsMenu {
        entries: menu_entries(layout),
        max_h: (win_h - 80.0).clamp(160.0, 600.0),
    }
}

/// Rows are `mouse_area`s publishing on press, never buttons: `ContextMenu`
/// rebuilds its overlay every view (trap 2 in CLAUDE.md).
fn bars_menu_panel(menu: &BarsMenu) -> Element<'static, Message> {
    let rows: Vec<Element<'static, Message>> = menu
        .entries
        .iter()
        .map(|e| {
            mouse_area(
                container(
                    row![
                        super::icons::themed_check_cell::<Message>(e.visible),
                        text(crate::t!(e.name).into_owned()).size(12)
                    ]
                    .spacing(8)
                    .align_y(iced::Center),
                )
                .padding([3, 10])
                .width(Length::Fill),
            )
            .on_press(Message::Toolbar(ToolbarMsg::Toggle(e.id)))
            .interaction(iced::mouse::Interaction::Pointer)
            .into()
        })
        .collect();
    // `container` has no max height in this iced: cap the scrollable instead,
    // from a (slightly low) row-height estimate; shorter lists shrink to fit.
    let est = menu.entries.len() as f32 * BAR_ROW_H;
    let height = if est > menu.max_h {
        Length::Fixed(menu.max_h)
    } else {
        Length::Shrink
    };
    container(scrollable(column(rows)).height(height))
        .padding(2)
        .width(Length::Fixed(260.0))
        .style(panel_style)
        .into()
}

fn with_bars_menu(el: Element<'static, Message>, menu: &BarsMenu) -> Element<'static, Message> {
    let menu = menu.clone();
    iced_aw::ContextMenu::new(el, move || bars_menu_panel(&menu)).into()
}

/// Estimated length of a bar along its own axis (px), grip included.
pub fn bar_length(id: ToolbarId) -> f32 {
    if id == ToolbarId::Layers {
        return LAYERS_LENGTH;
    }
    let items: f32 = items_for(id)
        .iter()
        .map(|i| match i {
            ClassicItem::Button(_) => BTN_SIZE + BTN_GAP,
            ClassicItem::Separator => SEP_SPAN,
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
        column(items.iter().map(|i| item_el(i, true, id)))
            .spacing(BTN_GAP)
            .into()
    } else {
        row(items.iter().map(|i| item_el(i, false, id)))
            .spacing(BTN_GAP)
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
    menu: &BarsMenu,
) -> Element<'static, Message> {
    let body = bar_body(id, vertical, ribbon);
    let inner: Element<'static, Message> = if vertical {
        column![grip(id, true), body].spacing(2).into()
    } else {
        row![grip(id, false), body]
            .spacing(2)
            .align_y(iced::Center)
            .into()
    };
    let bar = container(inner)
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
        });
    with_bars_menu(bar.into(), menu)
}

fn lane_el(
    bars: &[ToolbarId],
    vertical: bool,
    ribbon: &Ribbon,
    dragging: Option<ToolbarId>,
    menu: &BarsMenu,
) -> Element<'static, Message> {
    let els: Vec<Element<'static, Message>> = bars
        .iter()
        .map(|&id| bar_el(id, vertical, ribbon, dragging == Some(id), menu))
        .collect();
    if vertical {
        container(scrollable(column(els).spacing(2)))
            .padding(2)
            .height(Length::Fill)
            .style(strip_style)
            .into()
    } else {
        container(
            scrollable(row(els).spacing(2).align_y(iced::Center)).direction(
                scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::new().width(4).scroller_width(4),
                ),
            ),
        )
        .padding(2)
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
    menu: &BarsMenu,
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
        .map(|l| lane_el(l, vertical, ribbon, dragging, menu))
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
    win_h: f32,
    center: Element<'a, Message>,
) -> Element<'a, Message> {
    if !classic {
        return center;
    }
    let menu = bars_menu_for(layout, win_h);
    let middle = row![
        edge_el(layout, Edge::Left, ribbon, dragging, &menu),
        container(center).width(Length::Fill).height(Length::Fill),
        edge_el(layout, Edge::Right, ribbon, dragging, &menu),
    ]
    .height(Length::Fill);
    column![
        edge_el(layout, Edge::Top, ribbon, dragging, &menu),
        middle,
        edge_el(layout, Edge::Bottom, ribbon, dragging, &menu),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Height of a floating bar's title strip plus its padding.
const FLOAT_TITLE_H: f32 = 18.0;

/// `(width, height)` of a floating bar: a title strip over a horizontal body.
pub fn floating_size(id: ToolbarId) -> (f32, f32) {
    let (w, h) = bar_size(id, false);
    (w, h + FLOAT_TITLE_H)
}

/// Where each floating bar is drawn: its saved position, pulled back inside
/// the window so a bar saved on a bigger screen never ends up unreachable.
pub fn floating_positions(layout: &ToolbarLayout, win: (f32, f32)) -> Vec<(ToolbarId, Point)> {
    layout
        .floating()
        .into_iter()
        .map(|(id, x, y)| {
            let (cx, cy) = clamp_floating((x, y), floating_size(id), win);
            (id, Point::new(cx, cy))
        })
        .collect()
}

/// Widget id of a floating bar's frame (lets tests measure its real layout).
pub fn floating_id(id: ToolbarId) -> iced::widget::Id {
    iced::widget::Id::new(match id {
        ToolbarId::Draw => "toolbar-float-Draw",
        ToolbarId::Modify => "toolbar-float-Modify",
        ToolbarId::Layers => "toolbar-float-Layers",
        ToolbarId::Group(key) => key,
    })
}

fn floating_el(
    id: ToolbarId,
    ribbon: &Ribbon,
    being_dragged: bool,
    menu: &BarsMenu,
) -> Element<'static, Message> {
    // Every part has its natural width: the title band is just the frame's
    // own background showing above the buttons (a `Fill` strip would stretch
    // the whole bar across the window).
    let title = container(text(crate::t!(id.title()).into_owned()).size(11)).padding([2, 6]);
    let body = container(bar_body(id, false, ribbon))
        .padding(2)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            ..Default::default()
        });
    let frame = container(column![title, body])
        .id(floating_id(id))
        .padding(1)
        .style(move |theme: &Theme| {
            let p = theme.palette();
            container::Style {
                background: Some(Background::Color(p.background.strong.color)),
                text_color: Some(p.background.strong.text),
                border: Border {
                    color: if being_dragged {
                        p.primary.base.color
                    } else {
                        p.background.neutral.color
                    },
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        });
    // Buttons capture their own presses, so only the band and the padding
    // around them start a drag or a redock.
    let grab = mouse_area(frame)
        .on_press(Message::Toolbar(ToolbarMsg::Grab(id)))
        .on_double_click(Message::Toolbar(ToolbarMsg::Redock(id)))
        .interaction(iced::mouse::Interaction::Grab);
    with_bars_menu(grab.into(), menu)
}

fn floating_layer(
    layout: &ToolbarLayout,
    ribbon: &Ribbon,
    win: (f32, f32),
    dragging: Option<ToolbarId>,
    menu: &BarsMenu,
) -> Option<Element<'static, Message>> {
    let bars = floating_positions(layout, win);
    if bars.is_empty() {
        return None;
    }
    let layers: Vec<Element<'static, Message>> = bars
        .into_iter()
        .map(|(id, at)| {
            pin(opaque(floating_el(id, ribbon, dragging == Some(id), menu)))
                .position(at)
                .into()
        })
        .collect();
    Some(
        Stack::with_children(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    )
}

fn block_el(x: f32, y: f32, w: f32, h: f32, fill_alpha: f32) -> Element<'static, Message> {
    pin(container(Space::new())
        .width(Length::Fixed(w))
        .height(Length::Fixed(h))
        .style(move |theme: &Theme| {
            let c = theme.palette().primary.base.color;
            container::Style {
                background: Some(Background::Color(c.scale_alpha(fill_alpha))),
                border: Border {
                    color: c,
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            }
        }))
    .position(Point::new(x.max(0.0), y.max(0.0)))
    .into()
}

/// The highlighted lane the bar would dock into, plus a ghost outline
/// following the pointer.
fn drag_visuals(d: &ToolbarDrag, win: (f32, f32)) -> Vec<Element<'static, Message>> {
    let Some(cursor) = d.cursor else {
        return Vec::new();
    };
    let mut v = Vec::new();
    match d.target {
        Some(Target::Dock(slot)) => {
            let (x, y, w, h) = band_rect(slot, win);
            v.push(block_el(x, y, w, h, 0.18));
        }
        Some(Target::NewLane { edge, lane }) => {
            let (x, y, w, h) = insertion_rect(edge, lane, win);
            v.push(block_el(x, y, w, h, 0.7));
        }
        _ => {}
    }
    let vertical = match d.target {
        Some(Target::Dock(s)) => s.edge.is_vertical(),
        Some(Target::NewLane { edge, .. }) => edge.is_vertical(),
        _ => false,
    };
    let (w, h) = bar_size(d.id, vertical);
    v.push(block_el(cursor.x - GRIP_ANCHOR, cursor.y - GRIP_ANCHOR, w, h, 0.10));
    v
}

/// Wrap the main window content with the floating bars and, while a bar is
/// being dragged, the full-size pointer tracker + previews.
#[inline(never)]
pub fn decorate<'a>(
    base: Element<'a, Message>,
    layout: &ToolbarLayout,
    ribbon: &Ribbon,
    drag: Option<&ToolbarDrag>,
    win: (f32, f32),
    classic: bool,
) -> Element<'a, Message> {
    if !classic {
        return base;
    }
    let mut layers: Vec<Element<'a, Message>> = vec![base];
    let menu = bars_menu_for(layout, win.1);
    if let Some(floating) = floating_layer(layout, ribbon, win, drag.map(|d| d.id), &menu) {
        layers.push(floating);
    }
    if let Some(d) = drag {
        layers.extend(drag_visuals(d, win));
    }
    let stacked: Element<'a, Message> = Stack::with_children(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    if drag.is_some() {
        mouse_area(stacked)
            .on_move(|p| Message::Toolbar(ToolbarMsg::DragMove(p)))
            .on_release(Message::Toolbar(ToolbarMsg::DragRelease))
            // A release outside the window never reaches the layer: the pointer
            // leaving ends the drag instead of leaving it stuck.
            .on_exit(Message::Toolbar(ToolbarMsg::DragRelease))
            .interaction(iced::mouse::Interaction::Grabbing)
            .into()
    } else {
        stacked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::toolbar_layout::Target;

    #[test]
    fn floating_positions_are_clamped_into_the_window() {
        // Review focus 4: a bar saved far away must stay reachable in a
        // smaller window.
        let mut layout = ToolbarLayout::default();
        layout.move_to(ToolbarId::Layers, Target::Float { x: 5000.0, y: 4000.0 });
        layout.move_to(ToolbarId::Draw, Target::Float { x: 30.0, y: 40.0 });
        let win = (800.0, 600.0);
        let got = floating_positions(&layout, win);
        assert_eq!(got.len(), 2);
        for (id, p) in &got {
            let (w, h) = floating_size(*id);
            assert!(p.x >= 0.0 && p.y >= 0.0, "{id:?} at {p:?}");
            assert!(p.x + w <= win.0.max(w) && p.y + h <= win.1, "{id:?} at {p:?}");
        }
        let draw = got.iter().find(|(id, _)| *id == ToolbarId::Draw).unwrap().1;
        assert_eq!((draw.x, draw.y), (30.0, 40.0), "in-window bars are untouched");
    }
}
