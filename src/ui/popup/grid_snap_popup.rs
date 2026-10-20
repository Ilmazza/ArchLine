//! ArchLine: status menu of the Snap to Grid pill, after AutoCAD's snap menu:
//! the grid snap switch, the snap spacing, isometric snap and the way into the
//! full Drafting Settings window. The spacing is handled by
//! `app/snap_spacing.rs`.

use iced::widget::{button, container, row, text, text_input};
use iced::{Background, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::t;
use crate::ui::statusbar::status_menu::Entry;

/// Edits of the spacing fields, behind the one `Message::SnapSpacing`.
#[derive(Debug, Clone)]
pub enum SnapSpacingMsg {
    X(String),
    Y(String),
    /// Enter in either field: apply both.
    Submit,
    /// The menu was opened afresh: drop whatever was half-typed.
    Discard,
}

pub fn menu_entries<'a>(
    grid_snap: bool,
    isometric: bool,
    spacing_x: &str,
    spacing_y: &str,
) -> Vec<Entry<'a>> {
    vec![
        Entry::stay(check_row(
            t!("Grid Snap"),
            grid_snap,
            Message::ToggleGridSnap,
        )),
        Entry::stay(spacing_row(spacing_x, spacing_y)),
        Entry::stay(check_row(
            t!("Isometric Snap"),
            isometric,
            Message::ToggleIsometricDrafting,
        )),
        Entry::stay(divider()),
        Entry::close(plain_row(t!("Snap Settings…"), Message::ToggleSnapPopup)),
    ]
}

fn spacing_row<'a>(x: &str, y: &str) -> Element<'a, Message> {
    let field = |value: &str, on_input: fn(String) -> Message| {
        text_input("10", value)
            .on_input(on_input)
            .on_submit(Message::SnapSpacing(SnapSpacingMsg::Submit))
            .size(11)
            .padding([2, 6])
            .width(Length::Fixed(58.0))
    };
    container(
        row![
            text(t!("Spacing")).size(11),
            text("X").size(11),
            field(x, |s| Message::SnapSpacing(SnapSpacingMsg::X(s))),
            text("Y").size(11),
            field(y, |s| Message::SnapSpacing(SnapSpacingMsg::Y(s))),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .padding([5, 10])
    .into()
}

fn check_row(
    label: std::borrow::Cow<'static, str>,
    checked: bool,
    msg: Message,
) -> Element<'static, Message> {
    let content = row![
        crate::ui::icons::themed_check_cell(checked),
        text(label).size(11)
    ]
    .spacing(6)
    .align_y(iced::Center);
    button(content)
        .on_press(msg)
        .style(button::subtle)
        .width(Fill)
        .padding([4, 10])
        .into()
}

fn plain_row(label: std::borrow::Cow<'static, str>, msg: Message) -> Element<'static, Message> {
    button(text(label).size(11))
        .on_press(msg)
        .style(button::subtle)
        .width(Fill)
        .padding([4, 10])
        .into()
}

fn divider<'a>() -> Element<'a, Message> {
    container(iced::widget::Space::new().height(1))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            ..Default::default()
        })
        .width(Fill)
        .padding([0, 4])
        .into()
}
