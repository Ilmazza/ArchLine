//! Hatch and Gradient — the dialog HATCH opens to choose how a hatch looks and
//! which areas it fills, as in AutoCAD.
//!
//! Only what the engine already does is live; the rest is shown greyed so the
//! window reads like the original. The dialog never writes into the drawing
//! itself: OK builds a `HatchCommand` from this state and lets that commit.

use codec::entities::HatchStyleType;
use codec::Handle;
use iced::widget::{button, canvas, checkbox, column, container, pick_list, row, text, text_input, Space};
use iced::{Border, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::command::WorkingPlane;
use crate::modules::draw::draw::hatch_settings::{
    HatchRegion, HatchSettings, OriginMode, RegionOrigin,
};
use crate::t;
use crate::ui::style::form::{dialog_button_styled_opt, form_radio};

/// Which hidden step the dialog is waiting on. `None` while it is visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    None,
    Pick,
    Select,
    Preview,
    Origin,
}

/// Which "Add" button was pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddKind {
    Points,
    Objects,
}

/// One field of the dialog changed.
#[derive(Clone, Debug)]
pub enum Field {
    Pattern(String),
    Angle(String),
    Scale(String),
    Associative(bool),
    Separate(bool),
    Retain(bool),
    IslandDetection(bool),
    IslandStyle(HatchStyleType),
    OriginMode(OriginMode),
}

/// The dialog's working copy. Nothing reaches the drawing until OK.
pub struct State {
    /// Stable id of the tab that opened the dialog (indices shift on close).
    pub owner_tab_id: u64,
    pub plane: WorkingPlane,
    pub outlines: Vec<Vec<[f64; 2]>>,
    pub boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    pub regions: Vec<(HatchRegion, RegionOrigin)>,
    /// Boundary objects already used; informational, never a geometry filter.
    pub taken_objects: Vec<Handle>,
    /// The drawing's selection while a "Select objects" round clears it.
    pub saved_selection: Option<Vec<Handle>>,
    pub settings: HatchSettings,
    /// Local (working-plane) coordinates of the origin picked with
    /// "Click to set new origin".
    pub specified_origin: Option<[f64; 2]>,
    pub flow: Flow,
}

impl State {
    pub fn new(
        owner_tab_id: u64,
        plane: WorkingPlane,
        outlines: Vec<Vec<[f64; 2]>>,
        boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
        settings: HatchSettings,
    ) -> Self {
        Self {
            owner_tab_id,
            plane,
            outlines,
            boundary_sources,
            regions: Vec::new(),
            taken_objects: Vec::new(),
            saved_selection: None,
            settings,
            specified_origin: None,
            flow: Flow::None,
        }
    }

    pub fn apply(&mut self, field: Field) {
        match field {
            Field::Pattern(name) => self.settings.pattern = name,
            Field::Angle(text) => self.settings.angle = text,
            Field::Scale(text) => self.settings.scale = text,
            Field::Associative(on) => self.settings.associative = on,
            Field::Separate(on) => self.settings.set_separate(on),
            Field::Retain(on) => self.settings.set_retain(on),
            Field::IslandDetection(on) => self.settings.island_detection = on,
            Field::IslandStyle(style) => self.settings.island_style = style,
            Field::OriginMode(mode) => self.settings.origin_mode = mode,
        }
    }

    /// Add, Preview and OK need every field usable.
    pub fn fields_valid(&self) -> bool {
        self.settings.resolve().is_some()
    }

    pub fn can_ok(&self) -> bool {
        self.fields_valid() && !self.regions.is_empty()
    }

    /// Message shown under Angle while it is not a number.
    pub fn angle_message(&self) -> Option<String> {
        self.settings
            .angle_error()
            .then(|| t!("Not a valid number").into_owned())
    }

    /// Message shown under Scale while it is not a number above zero.
    pub fn scale_message(&self) -> Option<String> {
        self.settings
            .scale_error()
            .then(|| t!("Not a valid number").into_owned())
    }

    /// Message shown under the pattern list when the remembered pattern is no
    /// longer in the catalog (Add, Preview and OK are off until another is chosen).
    pub fn pattern_message(&self) -> Option<String> {
        crate::scene::model::hatch_patterns::find(&self.settings.pattern)
            .is_none()
            .then(|| t!("Pattern not found: choose another").into_owned())
    }
}

// ── View ───────────────────────────────────────────────────────────────────

fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.6)),
    }
}

fn danger(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().danger.base.color),
    }
}

fn group<'a>(title: String, body: Element<'a, Message>) -> Element<'a, Message> {
    container(column![text(title).size(11).style(muted), body].spacing(6))
        .padding(8)
        .width(Fill)
        .style(|theme: &Theme| container::Style {
            border: Border {
                width: 1.0,
                radius: 4.0.into(),
                color: theme.palette().background.strong.color,
            },
            ..Default::default()
        })
        .into()
}

/// A control the engine does not support yet: shown, never active.
fn grey<'a>(label: String) -> Element<'a, Message> {
    text(label).size(11).style(muted).into()
}

fn grey_check<'a>(label: String) -> Element<'a, Message> {
    row![checkbox(false).size(14), text(label).size(11).style(muted)]
        .spacing(6)
        .align_y(iced::Center)
        .into()
}

fn labelled<'a>(label: String, control: Element<'a, Message>) -> Element<'a, Message> {
    row![text(label).size(11).style(muted).width(82), control]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

/// Border of a field holding an unusable value.
fn invalid_field_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = crate::ui::style::form::field_style(theme, status);
    style.border.color = theme.palette().danger.base.color;
    style
}

/// A text field; with a `message` it gets the error border and the message
/// sits on its own line below it.
fn field<'a>(
    value: &str,
    message: Option<String>,
    ctor: fn(String) -> Field,
) -> Element<'a, Message> {
    let input = text_input("", value)
        .on_input(move |text| Message::HatchDialogField(ctor(text)))
        .size(12)
        .padding([3, 6])
        .width(Length::Fixed(90.0));
    match message {
        Some(message) => column![
            input.style(invalid_field_style),
            text(message).size(10).style(danger)
        ]
        .spacing(2)
        .into(),
        None => input.into(),
    }
}

fn add_button<'a>(label: String, kind: AddKind, enabled: bool) -> Element<'a, Message> {
    dialog_button_styled_opt(
        label,
        enabled.then_some(Message::HatchDialogAdd(kind)),
        button::secondary,
    )
    .width(Fill)
    .into()
}

fn type_and_pattern<'a>(state: &State) -> Element<'a, Message> {
    use crate::scene::model::hatch_model::HatchPattern;
    use crate::scene::model::hatch_patterns;

    let names: Vec<String> = hatch_patterns::catalog()
        .iter()
        .map(|entry| entry.name.clone())
        .collect();
    let known = hatch_patterns::find(&state.settings.pattern).is_some();
    let selected = known.then(|| state.settings.pattern.clone());
    let picker = pick_list(selected, names, |name: &String| name.clone())
        .on_select(|name: String| Message::HatchDialogField(Field::Pattern(name)))
        .text_size(12)
        .padding([3, 6])
        .width(Fill);

    // Swatch: the pattern at the typed angle and scale; a plain fill when the
    // pattern is unknown so the box is never empty.
    let angle = crate::modules::draw::draw::hatch_settings::parse_angle_deg(&state.settings.angle)
        .unwrap_or(0.0)
        .to_radians();
    let scale = crate::modules::draw::draw::hatch_settings::parse_scale(&state.settings.scale)
        .unwrap_or(1.0);
    let gpu = hatch_patterns::find(&state.settings.pattern)
        .map(|entry| entry.gpu.clone())
        .unwrap_or(HatchPattern::Solid);
    let swatch = canvas(
        crate::ui::properties::HatchPatternPreview::new(gpu).with_angle_scale(angle, scale),
    )
    .width(Fill)
    .height(Length::Fixed(44.0));

    let pattern_note: Element<'a, Message> = match state.pattern_message() {
        Some(message) => text(message).size(10).style(danger).into(),
        None => Space::new().into(),
    };
    group(
        t!("Type and pattern").into_owned(),
        column![
            labelled(t!("Type").into_owned(), grey(t!("Predefined").into_owned())),
            labelled(
                t!("Pattern").into_owned(),
                row![
                    picker,
                    // Pattern palette: not available yet, shown inert.
                    button(text("...").size(12)).padding([3, 8]).style(button::secondary),
                ]
                .spacing(6)
                .into()
            ),
            pattern_note,
            labelled(t!("Color").into_owned(), grey(t!("Use Current").into_owned())),
            labelled(t!("Swatch").into_owned(), swatch.into()),
            labelled(
                t!("Custom pattern").into_owned(),
                grey(String::new())
            ),
        ]
        .spacing(6)
        .into(),
    )
}

fn angle_and_scale<'a>(state: &State) -> Element<'a, Message> {
    group(
        t!("Angle and scale").into_owned(),
        column![
            labelled(
                t!("Angle").into_owned(),
                field(&state.settings.angle, state.angle_message(), Field::Angle)
            ),
            labelled(
                t!("Scale").into_owned(),
                field(&state.settings.scale, state.scale_message(), Field::Scale)
            ),
            grey_check(t!("Double").into_owned()),
            grey_check(t!("Relative to paper space").into_owned()),
            labelled(t!("Spacing").into_owned(), grey(String::new())),
            labelled(t!("ISO pen width").into_owned(), grey(String::new())),
        ]
        .spacing(6)
        .into(),
    )
}

fn origin_group<'a>(state: &State) -> Element<'a, Message> {
    let mode = Some(state.settings.origin_mode);
    let set_origin = dialog_button_styled_opt(
        t!("Click to set new origin").into_owned(),
        (state.settings.origin_mode == OriginMode::Specified && state.fields_valid())
            .then_some(Message::HatchDialogPickOrigin),
        button::secondary,
    );
    group(
        t!("Hatch origin").into_owned(),
        column![
            form_radio(
                t!("Use current origin").into_owned(),
                OriginMode::Current,
                mode,
                |mode| Message::HatchDialogField(Field::OriginMode(mode)),
            ),
            form_radio(
                t!("Specified origin").into_owned(),
                OriginMode::Specified,
                mode,
                |mode| Message::HatchDialogField(Field::OriginMode(mode)),
            ),
            set_origin,
            grey_check(t!("Default to boundary extents").into_owned()),
            grey(t!("Bottom left").into_owned()),
            grey_check(t!("Store as default origin").into_owned()),
        ]
        .spacing(6)
        .into(),
    )
}

fn boundaries_group<'a>(state: &State) -> Element<'a, Message> {
    let enabled = state.fields_valid();
    group(
        t!("Boundaries").into_owned(),
        column![
            add_button(t!("Add: Pick points").into_owned(), AddKind::Points, enabled),
            add_button(t!("Add: Select objects").into_owned(), AddKind::Objects, enabled),
            grey(t!("Remove boundaries").into_owned()),
            grey(t!("Recreate boundary").into_owned()),
            grey(t!("View Selections").into_owned()),
            text(crate::tf!("{} region(s) selected", state.regions.len())).size(11),
        ]
        .spacing(6)
        .into(),
    )
}

fn options_group<'a>(state: &State) -> Element<'a, Message> {
    let settings = &state.settings;
    let check = |on: bool, label: String, ctor: fn(bool) -> Field| -> Element<'a, Message> {
        row![
            checkbox(on)
                .on_toggle(move |value| Message::HatchDialogField(ctor(value)))
                .size(14),
            text(label).size(11),
        ]
        .spacing(6)
        .align_y(iced::Center)
        .into()
    };
    group(
        t!("Options").into_owned(),
        column![
            grey_check(t!("Annotative").into_owned()),
            check(settings.associative, t!("Associative").into_owned(), Field::Associative),
            check(
                settings.separate,
                t!("Create separate hatches").into_owned(),
                Field::Separate
            ),
            labelled(t!("Draw order").into_owned(), grey(t!("Send Behind Boundary").into_owned())),
            labelled(t!("Layer").into_owned(), grey(t!("Use Current").into_owned())),
            labelled(t!("Transparency").into_owned(), grey(t!("Use Current").into_owned())),
        ]
        .spacing(6)
        .into(),
    )
}

fn islands_group<'a>(state: &State) -> Element<'a, Message> {
    let settings = &state.settings;
    let detection = row![
        checkbox(settings.island_detection)
            .on_toggle(|on| Message::HatchDialogField(Field::IslandDetection(on)))
            .size(14),
        text(t!("Island detection")).size(11),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let styles: Element<'a, Message> = if settings.island_detection {
        let selected = Some(settings.island_style);
        row![
            form_radio(t!("Normal").into_owned(), HatchStyleType::Normal, selected, |style| {
                Message::HatchDialogField(Field::IslandStyle(style))
            }),
            form_radio(t!("Outer").into_owned(), HatchStyleType::Outer, selected, |style| {
                Message::HatchDialogField(Field::IslandStyle(style))
            }),
            form_radio(t!("Ignore").into_owned(), HatchStyleType::Ignore, selected, |style| {
                Message::HatchDialogField(Field::IslandStyle(style))
            }),
        ]
        .spacing(10)
        .into()
    } else {
        row![
            grey(t!("Normal").into_owned()),
            grey(t!("Outer").into_owned()),
            grey(t!("Ignore").into_owned()),
        ]
        .spacing(10)
        .into()
    };
    group(
        t!("Islands").into_owned(),
        column![detection, text(t!("Island display style:")).size(11).style(muted), styles]
            .spacing(6)
            .into(),
    )
}

fn retention_group<'a>(state: &State) -> Element<'a, Message> {
    group(
        t!("Boundary retention").into_owned(),
        column![
            row![
                checkbox(state.settings.retain)
                    .on_toggle(|on| Message::HatchDialogField(Field::Retain(on)))
                    .size(14),
                text(t!("Retain boundaries")).size(11),
            ]
            .spacing(6)
            .align_y(iced::Center),
            labelled(t!("Object type").into_owned(), grey(t!("Polyline").into_owned())),
        ]
        .spacing(6)
        .into(),
    )
}

pub fn view_window<'a>(
    state: &State,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let tabs = row![
        text(t!("Hatch")).size(12),
        text(t!("Gradient")).size(12).style(muted),
    ]
    .spacing(16);

    let left = column![type_and_pattern(state), angle_and_scale(state), origin_group(state)]
        .spacing(8)
        .width(Fill);
    let middle = column![boundaries_group(state), options_group(state)]
        .spacing(8)
        .width(Fill);
    let right = column![
        islands_group(state),
        retention_group(state),
        group(t!("Boundary set").into_owned(), grey(t!("Current viewport").into_owned())),
        group(t!("Gap tolerance").into_owned(), grey(t!("0 units").into_owned())),
        group(
            t!("Inherit options").into_owned(),
            column![
                grey(t!("Use current origin").into_owned()),
                grey(t!("Use source hatch origin").into_owned()),
                grey(t!("Inherit Properties").into_owned()),
            ]
            .spacing(6)
            .into()
        ),
    ]
    .spacing(8)
    .width(Fill);

    let ok = state.can_ok();
    let fields = state.fields_valid();
    let actions = row![
        Space::new().width(Fill),
        dialog_button_styled_opt(
            t!("Preview").into_owned(),
            (fields && !state.regions.is_empty()).then_some(Message::HatchDialogPreview),
            button::secondary,
        ),
        dialog_button_styled_opt(
            t!("OK").into_owned(),
            ok.then_some(Message::HatchDialogOk),
            button::primary,
        ),
        dialog_button_styled_opt(
            t!("Cancel").into_owned(),
            Some(Message::CloseModal),
            button::secondary,
        ),
    ]
    .spacing(8)
    .align_y(iced::Center);

    column![tabs, row![left, middle, right].spacing(10), actions]
        .spacing(10)
        .padding(10)
        .width(sizing.width)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::draw::draw::hatch_settings::{HatchRegion, RegionOrigin};
    use crate::command::WorkingPlane;

    fn state() -> State {
        State::new(
            7,
            WorkingPlane::default(),
            Vec::new(),
            Default::default(),
            HatchSettings::default(),
        )
    }

    fn one_region() -> (HatchRegion, RegionOrigin) {
        (
            HatchRegion {
                rings: vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0]]],
            },
            RegionOrigin::Points,
        )
    }

    #[test]
    fn a_new_state_has_no_regions_and_no_flow() {
        let state = state();
        assert_eq!(state.owner_tab_id, 7);
        assert!(state.regions.is_empty());
        assert!(state.taken_objects.is_empty());
        assert!(state.saved_selection.is_none());
        assert_eq!(state.flow, Flow::None);
        assert!(state.specified_origin.is_none());
    }

    #[test]
    fn ok_needs_valid_fields_and_a_region() {
        let mut state = state();
        assert!(state.fields_valid());
        assert!(!state.can_ok(), "no region yet");
        state.regions.push(one_region());
        assert!(state.can_ok());
        state.apply(Field::Scale("0".into()));
        assert!(!state.fields_valid());
        assert!(!state.can_ok());
        state.apply(Field::Scale("2,5".into()));
        assert!(state.can_ok());
        state.apply(Field::Angle("abc".into()));
        assert!(!state.can_ok());
    }

    #[test]
    fn a_pattern_missing_from_the_catalog_blocks_everything() {
        let mut state = state();
        state.regions.push(one_region());
        state.apply(Field::Pattern("NO_SUCH_PATTERN".into()));
        assert!(!state.fields_valid());
        assert!(!state.can_ok());
    }

    #[test]
    fn fields_update_the_settings() {
        let mut state = state();
        state.apply(Field::Associative(false));
        state.apply(Field::IslandDetection(false));
        state.apply(Field::IslandStyle(HatchStyleType::Outer));
        state.apply(Field::OriginMode(OriginMode::Specified));
        assert!(!state.settings.associative);
        assert!(!state.settings.island_detection);
        assert_eq!(state.settings.island_style, HatchStyleType::Outer);
        assert_eq!(state.settings.origin_mode, OriginMode::Specified);
    }

    #[test]
    fn field_messages_follow_the_settings() {
        let mut state = state();
        assert!(state.angle_message().is_none());
        assert!(state.scale_message().is_none());
        assert!(state.pattern_message().is_none());
        state.apply(Field::Angle("abc".into()));
        state.apply(Field::Scale("0".into()));
        state.apply(Field::Pattern("NO_SUCH_PATTERN".into()));
        assert!(state.angle_message().is_some());
        assert!(state.scale_message().is_some());
        assert!(state.pattern_message().is_some());
        state.apply(Field::Angle("-12,5".into()));
        state.apply(Field::Scale("0,5".into()));
        state.apply(Field::Pattern("ANSI31".into()));
        assert!(state.angle_message().is_none());
        assert!(state.scale_message().is_none());
        assert!(state.pattern_message().is_none());
    }

    #[test]
    fn retain_and_separate_toggle_each_other_off() {
        let mut state = state();
        state.apply(Field::Separate(true));
        assert!(state.settings.separate && !state.settings.retain);
        state.apply(Field::Retain(true));
        assert!(state.settings.retain && !state.settings.separate);
    }
}
