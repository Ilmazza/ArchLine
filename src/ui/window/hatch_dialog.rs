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
use crate::modules::draw::draw::hatch_edit_settings::EditTarget;
use crate::modules::draw::draw::hatch_settings::{
    FillTab, HatchColor, HatchRegion, HatchSettings, OriginMode, RegionOrigin,
};
use crate::scene::model::hatch_model::GradientKind;
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

/// Which colour control: the fill colour of a pattern or solid, or one of the
/// gradient's two colours. Tells "Select Color" where its answer goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HatchColorSlot {
    Fill,
    Gradient1,
    Gradient2,
}

impl HatchColorSlot {
    /// The field that sets this slot to `color`.
    pub fn field(self, color: codec::types::Color) -> Field {
        match self {
            Self::Fill => Field::Color(HatchColor::Color(color)),
            Self::Gradient1 => Field::GradientColor1(color),
            Self::Gradient2 => Field::GradientColor2(color),
        }
    }
}

/// One field of the dialog changed.
#[derive(Clone, Debug)]
pub enum Field {
    Tab(FillTab),
    Color(HatchColor),
    GradientShape(usize),
    GradientOneColor(bool),
    GradientColor1(codec::types::Color),
    GradientColor2(codec::types::Color),
    GradientTint(f32),
    GradientCentered(bool),
    GradientAngle(String),
    /// Opens (`Some`) or closes (`None`) a colour list; view state only.
    ColorList(Option<HatchColorSlot>),
    /// "Select Color..." was chosen in a list. Opening the colour window is the
    /// app's job; here it only closes the list.
    SelectColor(HatchColorSlot),
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
    /// The pattern browser opened by "..."; `None` while closed.
    pub palette: Option<super::hatch_palette::Palette>,
    /// "Hatch Edit": the hatch the window edits. `None` while HATCH creates one.
    pub edit: Option<EditTarget>,
    /// The colour list that is open, if any. View state: never part of the
    /// settings and never remembered.
    pub color_list: Option<HatchColorSlot>,
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
            palette: None,
            edit: None,
            color_list: None,
        }
    }

    /// The window on an existing hatch, its fields filled from it. `plane` is
    /// the hatch's own plane, where a new origin is measured.
    pub fn for_edit(owner_tab_id: u64, plane: WorkingPlane, target: EditTarget) -> Self {
        let mut state = Self::new(
            owner_tab_id,
            plane,
            Vec::new(),
            Default::default(),
            target.initial.clone(),
        );
        state.edit = Some(target);
        state
    }

    pub fn apply(&mut self, field: Field) {
        if let Some(edit) = &self.edit {
            match field {
                // One existing hatch: these only shape a hatch being created.
                Field::Separate(_) | Field::Retain(_) => return,
                // A hatch can be disassociated here, never associated.
                Field::Associative(_) if !edit.initial.associative => return,
                // An existing hatch always has a colour of its own.
                Field::Color(HatchColor::UseCurrent) => return,
                _ => {}
            }
        }
        match field {
            Field::Tab(tab) => self.settings.tab = tab,
            Field::Color(color) => {
                self.settings.color = color;
                self.color_list = None;
            }
            Field::GradientShape(index) => {
                self.settings.gradient.shape = index.min(GradientKind::CHOICES.len() - 1)
            }
            Field::GradientOneColor(on) => self.settings.gradient.one_color = on,
            Field::GradientColor1(color) => {
                self.settings.gradient.color1 = color;
                self.color_list = None;
            }
            Field::GradientColor2(color) => {
                self.settings.gradient.color2 = color;
                self.color_list = None;
            }
            // A tint that is not a number is refused: the previous one stays.
            Field::GradientTint(tint) => {
                if tint.is_finite() {
                    self.settings.gradient.tint = tint.clamp(0.0, 1.0);
                }
            }
            Field::GradientCentered(on) => self.settings.gradient.centered = on,
            Field::GradientAngle(text) => self.settings.gradient.angle = text,
            Field::ColorList(slot) => self.color_list = slot,
            // Opening "Select Color" is the app's job (it needs the colour
            // window); the state only closes the list.
            Field::SelectColor(_) => self.color_list = None,
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

    /// Add, Preview and OK need every field usable. Editing a hatch, its own
    /// pattern counts as usable even when the catalog does not have it.
    pub fn fields_valid(&self) -> bool {
        match &self.edit {
            Some(edit) => edit.fields_valid(&self.settings),
            None => self.settings.resolve().is_some(),
        }
    }

    /// Creating: usable fields and at least one area. Editing: usable fields
    /// and at least one of them changed.
    pub fn can_ok(&self) -> bool {
        match &self.edit {
            Some(edit) => edit
                .changes(&self.settings, self.specified_origin)
                .is_some_and(|changes| !changes.is_empty()),
            None => self.fields_valid() && !self.regions.is_empty(),
        }
    }

    /// Message shown under Angle while it is not a number.
    pub fn angle_message(&self) -> Option<String> {
        self.settings
            .angle_error()
            .then(|| t!("Not a valid number").into_owned())
    }

    /// Message shown under the Gradient tab's Angle while it is not a number.
    pub fn gradient_angle_message(&self) -> Option<String> {
        self.settings
            .gradient_angle_error()
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
                    button(text("...").size(12))
                        .padding([3, 8])
                        .style(button::secondary)
                        .on_press(Message::HatchDialogPalette(
                            super::hatch_palette::PaletteAction::Open
                        )),
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
    // Editing keeps the hatch's own boundaries: no Add, and no area count.
    let enabled = state.fields_valid() && state.edit.is_none();
    let count: Element<'a, Message> = match state.edit {
        Some(_) => Space::new().into(),
        None => text(crate::tf!("{} region(s) selected", state.regions.len()))
            .size(11)
            .into(),
    };
    group(
        t!("Boundaries").into_owned(),
        column![
            add_button(t!("Add: Pick points").into_owned(), AddKind::Points, enabled),
            add_button(t!("Add: Select objects").into_owned(), AddKind::Objects, enabled),
            grey(t!("Remove boundaries").into_owned()),
            grey(t!("Recreate boundary").into_owned()),
            grey(t!("View Selections").into_owned()),
            count,
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
    // Editing: "Associative" can only switch an associative hatch off, and
    // separate hatches are a creation choice.
    let editing = state.edit.as_ref();
    let associative = match editing {
        Some(edit) if !edit.initial.associative => grey_check(t!("Associative").into_owned()),
        _ => check(settings.associative, t!("Associative").into_owned(), Field::Associative),
    };
    let separate = match editing {
        Some(_) => grey_check(t!("Create separate hatches").into_owned()),
        None => check(
            settings.separate,
            t!("Create separate hatches").into_owned(),
            Field::Separate,
        ),
    };
    group(
        t!("Options").into_owned(),
        column![
            grey_check(t!("Annotative").into_owned()),
            associative,
            separate,
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
    // Editing keeps the hatch's boundaries as they are.
    let retain: Element<'a, Message> = match state.edit {
        Some(_) => grey_check(t!("Retain boundaries").into_owned()),
        None => row![
            checkbox(state.settings.retain)
                .on_toggle(|on| Message::HatchDialogField(Field::Retain(on)))
                .size(14),
            text(t!("Retain boundaries")).size(11),
        ]
        .spacing(6)
        .align_y(iced::Center)
        .into(),
    };
    group(
        t!("Boundary retention").into_owned(),
        column![
            retain,
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

    if let Some(palette) = &state.palette {
        return super::hatch_palette::page(tabs.into(), palette, sizing);
    }

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
    // Editing has no Preview: the hatch on screen is the preview.
    let preview = state.edit.is_none() && state.fields_valid() && !state.regions.is_empty();
    let actions = row![
        Space::new().width(Fill),
        dialog_button_styled_opt(
            t!("Preview").into_owned(),
            preview.then_some(Message::HatchDialogPreview),
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
    use crate::modules::draw::draw::hatch_settings::{
        FillTab, HatchColor, HatchRegion, RegionOrigin,
    };
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

    // ── Hatch Edit ─────────────────────────────────────────────────────────

    fn edit_state(associative: bool, pattern: &str) -> State {
        let mut hatch = codec::entities::Hatch::with_pattern(
            codec::entities::hatch::HatchPattern::new(pattern),
        );
        hatch.is_associative = associative;
        hatch.pattern_scale = 1.0;
        State::for_edit(
            7,
            WorkingPlane::default(),
            EditTarget::from_hatch(Handle::new(5), &hatch),
        )
    }

    #[test]
    fn a_creation_state_is_not_an_edit() {
        assert!(state().edit.is_none());
    }

    #[test]
    fn an_edit_starts_from_the_hatch_and_ok_waits_for_a_change() {
        let mut state = edit_state(true, "ANSI31");
        let target = state.edit.clone().expect("an edit");
        assert_eq!(target.handle, Handle::new(5));
        assert_eq!(state.settings, target.initial);
        assert!(state.regions.is_empty());
        assert!(state.fields_valid());
        assert!(!state.can_ok(), "nothing changed yet");
        state.apply(Field::Scale("2".into()));
        assert!(state.can_ok(), "a change and no region needed");
        state.apply(Field::Scale("1,0".into()));
        assert!(!state.can_ok(), "back to the hatch's own value");
        state.apply(Field::Scale("0".into()));
        assert!(!state.fields_valid());
        assert!(!state.can_ok(), "an unusable field");
    }

    #[test]
    fn an_edit_ignores_the_options_that_only_shape_a_new_hatch() {
        let mut state = edit_state(true, "ANSI31");
        state.apply(Field::Separate(true));
        state.apply(Field::Retain(true));
        assert!(!state.settings.separate && !state.settings.retain);
        assert!(!state.can_ok());
    }

    #[test]
    fn associative_can_only_be_switched_off_on_an_associative_hatch() {
        let mut state = edit_state(true, "ANSI31");
        state.apply(Field::Associative(false));
        assert!(!state.settings.associative);
        assert!(state.can_ok(), "switching it off disassociates");
        let mut loose = edit_state(false, "ANSI31");
        loose.apply(Field::Associative(true));
        assert!(!loose.settings.associative, "a hatch is not re-associated here");
        assert!(!loose.can_ok());
    }

    #[test]
    fn the_hatch_own_unknown_pattern_warns_but_does_not_block_ok() {
        let mut state = edit_state(true, "MY_OWN");
        assert!(state.pattern_message().is_some(), "the warning is shown");
        assert!(state.fields_valid());
        assert!(!state.can_ok(), "nothing changed");
        state.apply(Field::Scale("3".into()));
        assert!(state.can_ok());
        state.apply(Field::Pattern("ALSO_UNKNOWN".into()));
        assert!(!state.can_ok(), "another unknown pattern is not usable");
    }

    #[test]
    fn a_creation_state_still_needs_a_region() {
        let mut state = state();
        state.apply(Field::Scale("2".into()));
        assert!(!state.can_ok());
    }

    fn has_message(messages: &[Message], wanted: fn(&Message) -> bool) -> bool {
        messages.iter().any(wanted)
    }

    #[test]
    fn the_edit_window_offers_no_add_no_preview_and_no_region_count() {
        let mut state = edit_state(true, "ANSI31");
        state.apply(Field::Scale("2".into()));
        let messages = click_all(&state, &["Add: Pick points", "Add: Select objects", "Preview"]);
        assert!(
            !has_message(&messages, |m| matches!(
                m,
                Message::HatchDialogAdd(_) | Message::HatchDialogPreview
            )),
            "Add and Preview are grey"
        );
        let mut ui = iced_test::simulator(view_window(&state, crate::ui::modal::ModalSizing::FILL));
        assert!(ui.find("0 region(s) selected").is_err());
        assert!(ui.find("Create separate hatches").is_ok(), "shown, greyed");
        assert!(ui.find("Retain boundaries").is_ok(), "shown, greyed");
        let messages = click_all(&state, &["OK"]);
        assert!(has_message(&messages, |m| matches!(m, Message::HatchDialogOk)));
    }

    #[test]
    fn the_edit_window_ok_is_off_until_something_changes() {
        let state = edit_state(true, "ANSI31");
        let messages = click_all(&state, &["OK"]);
        assert!(!has_message(&messages, |m| matches!(m, Message::HatchDialogOk)));
        // The creation window keeps its buttons.
        let mut create = self::state();
        create.regions.push(one_region());
        let messages = click_all(&create, &["Preview", "OK"]);
        assert!(has_message(&messages, |m| matches!(m, Message::HatchDialogPreview)));
        assert!(has_message(&messages, |m| matches!(m, Message::HatchDialogOk)));
        let mut ui = iced_test::simulator(view_window(&create, crate::ui::modal::ModalSizing::FILL));
        assert!(ui.find("1 region(s) selected").is_ok());
    }

    #[test]
    fn the_new_fields_change_the_settings() {
        use codec::types::Color;
        let mut state = state();
        state.apply(Field::Tab(FillTab::Gradient));
        state.apply(Field::Color(HatchColor::Color(Color::Index(1))));
        state.apply(Field::GradientShape(4));
        state.apply(Field::GradientOneColor(true));
        state.apply(Field::GradientColor1(Color::Index(2)));
        state.apply(Field::GradientColor2(Color::Index(3)));
        state.apply(Field::GradientTint(0.5));
        state.apply(Field::GradientCentered(false));
        state.apply(Field::GradientAngle("45".into()));
        let s = &state.settings;
        assert_eq!(s.tab, FillTab::Gradient);
        assert_eq!(s.color, HatchColor::Color(Color::Index(1)));
        let g = &s.gradient;
        assert_eq!((g.shape, g.one_color, g.tint, g.centered), (4, true, 0.5, false));
        assert_eq!((g.color1, g.color2), (Color::Index(2), Color::Index(3)));
        assert_eq!(g.angle, "45");
    }

    #[test]
    fn a_shape_beyond_the_list_is_clamped() {
        let mut state = state();
        state.apply(Field::GradientShape(500));
        assert_eq!(state.settings.gradient.shape, 8);
    }

    #[test]
    fn a_tint_that_is_not_a_number_never_gets_stored() {
        let mut state = state();
        state.apply(Field::GradientTint(0.3));
        state.apply(Field::GradientTint(f32::NAN));
        assert_eq!(state.settings.gradient.tint, 0.3, "NaN is refused");
        state.apply(Field::GradientTint(f32::INFINITY));
        assert_eq!(state.settings.gradient.tint, 0.3, "infinity is refused");
        state.apply(Field::GradientTint(9.0));
        assert_eq!(state.settings.gradient.tint, 1.0);
        state.apply(Field::GradientTint(-9.0));
        assert_eq!(state.settings.gradient.tint, 0.0);
    }

    #[test]
    fn the_colour_list_opens_and_closes_and_choosing_a_colour_closes_it() {
        use codec::types::Color;
        let mut state = state();
        state.apply(Field::ColorList(Some(HatchColorSlot::Gradient1)));
        assert_eq!(state.color_list, Some(HatchColorSlot::Gradient1));
        state.apply(Field::GradientColor1(Color::Index(2)));
        assert_eq!(state.color_list, None);
        state.apply(Field::ColorList(Some(HatchColorSlot::Fill)));
        state.apply(Field::ColorList(None));
        assert_eq!(state.color_list, None);
        // Every way out of the list closes it, and the choice lands in its slot.
        state.apply(Field::ColorList(Some(HatchColorSlot::Fill)));
        state.apply(Field::Color(HatchColor::UseCurrent));
        assert_eq!(state.color_list, None);
        state.apply(Field::ColorList(Some(HatchColorSlot::Gradient2)));
        state.apply(Field::GradientColor2(Color::Index(4)));
        assert_eq!(state.color_list, None);
        assert_eq!(state.settings.gradient.color2, Color::Index(4));
        state.apply(Field::ColorList(Some(HatchColorSlot::Gradient2)));
        state.apply(Field::SelectColor(HatchColorSlot::Gradient2));
        assert_eq!(state.color_list, None, "Select Color closes the list");
        assert_eq!(state.settings.gradient.color2, Color::Index(4), "and changes nothing itself");
    }

    #[test]
    fn the_slot_makes_the_matching_field() {
        use codec::types::Color;
        assert!(matches!(
            HatchColorSlot::Fill.field(Color::Index(3)),
            Field::Color(HatchColor::Color(c)) if c == Color::Index(3)
        ));
        assert!(matches!(
            HatchColorSlot::Gradient1.field(Color::Index(3)),
            Field::GradientColor1(c) if c == Color::Index(3)
        ));
        assert!(matches!(
            HatchColorSlot::Gradient2.field(Color::Index(3)),
            Field::GradientColor2(c) if c == Color::Index(3)
        ));
    }

    #[test]
    fn can_ok_follows_the_active_tab() {
        let mut state = state();
        state.regions.push(one_region());
        state.apply(Field::Scale("0".into())); // bad on the Hatch tab only
        assert!(!state.can_ok());
        state.apply(Field::Tab(FillTab::Gradient));
        assert!(state.can_ok());
        assert!(state.gradient_angle_message().is_none());
        state.apply(Field::GradientAngle("x".into()));
        assert!(!state.can_ok());
        assert!(state.gradient_angle_message().is_some());
    }

    #[test]
    fn editing_never_goes_back_to_use_current() {
        let mut state = edit_state(true, "ANSI31");
        state.apply(Field::Color(HatchColor::UseCurrent));
        assert!(matches!(state.settings.color, HatchColor::Color(_)));
    }

    #[test]
    fn editing_can_still_change_tab_and_colour() {
        use codec::types::Color;
        let mut state = edit_state(true, "ANSI31");
        state.apply(Field::Color(HatchColor::Color(Color::Index(5))));
        assert_eq!(state.settings.color, HatchColor::Color(Color::Index(5)));
        state.apply(Field::Tab(FillTab::Gradient));
        assert_eq!(state.settings.tab, FillTab::Gradient);
    }

    #[test]
    fn retain_and_separate_toggle_each_other_off() {
        let mut state = state();
        state.apply(Field::Separate(true));
        assert!(state.settings.separate && !state.settings.retain);
        state.apply(Field::Retain(true));
        assert!(state.settings.retain && !state.settings.separate);
    }

    // ── The real widgets, through the iced simulator ───────────────────────

    use super::super::hatch_palette::{Palette, PaletteAction, PatternCategory};

    /// Messages the real window publishes for the given interactions.
    fn click_all(state: &State, labels: &[&str]) -> Vec<Message> {
        let element = view_window(state, crate::ui::modal::ModalSizing::FILL);
        let mut ui = iced_test::simulator(element);
        for label in labels {
            ui.click(*label).unwrap_or_else(|_| panic!("{label} is on screen"));
        }
        ui.into_messages().collect()
    }

    fn with_palette(search: &str, selected: Option<&str>) -> State {
        let mut state = state();
        let mut palette = Palette::open("ANSI31");
        palette.search = search.into();
        palette.selected = selected.map(str::to_string);
        state.palette = Some(palette);
        state
    }

    fn palette_actions(messages: &[Message]) -> Vec<PaletteAction> {
        messages
            .iter()
            .filter_map(|message| match message {
                Message::HatchDialogPalette(action) => Some(action.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_dots_button_opens_the_palette() {
        let messages = click_all(&state(), &["..."]);
        assert_eq!(palette_actions(&messages), vec![PaletteAction::Open]);
    }

    #[test]
    fn a_card_click_picks_and_a_double_click_applies() {
        let state = with_palette("brick", None);
        let messages = click_all(&state, &["BRICK"]);
        assert_eq!(
            palette_actions(&messages),
            vec![PaletteAction::Pick("BRICK".into())]
        );
        let messages = click_all(&state, &["BRICK", "BRICK"]);
        let actions = palette_actions(&messages);
        assert!(
            actions.contains(&PaletteAction::Apply),
            "the second quick click is a double click: {actions:?}"
        );
        assert_eq!(actions[0], PaletteAction::Pick("BRICK".into()));
    }

    #[test]
    fn the_tabs_and_cancel_publish_palette_actions_and_the_window_controls_are_gone() {
        let state = with_palette("", Some("ANSI31"));
        let messages = click_all(&state, &["ISO"]);
        assert_eq!(
            palette_actions(&messages),
            vec![PaletteAction::Tab(PatternCategory::Iso)]
        );
        let messages = click_all(&state, &["Cancel"]);
        assert_eq!(palette_actions(&messages), vec![PaletteAction::Close]);
        assert!(
            !messages.iter().any(|m| matches!(m, Message::CloseModal)),
            "Cancel in the palette is not the window's Cancel"
        );
        let mut ui = iced_test::simulator(view_window(
            &state,
            crate::ui::modal::ModalSizing::FILL,
        ));
        for hidden in ["Preview", "Add: Pick points", "Click to set new origin", "Islands"] {
            assert!(ui.find(hidden).is_err(), "{hidden} is not on the palette page");
        }
        assert!(ui.find("Other Predefined").is_ok());
    }

    #[test]
    fn ok_in_the_palette_needs_a_selection() {
        let none = with_palette("brick", None);
        assert!(palette_actions(&click_all(&none, &["OK"])).is_empty(), "OK is off");
        let some = with_palette("brick", Some("BRICK"));
        assert_eq!(
            palette_actions(&click_all(&some, &["OK"])),
            vec![PaletteAction::Apply]
        );
    }

    #[test]
    fn a_search_without_results_says_so() {
        let state = with_palette("zzzz-no-such-pattern", None);
        let mut ui = iced_test::simulator(view_window(
            &state,
            crate::ui::modal::ModalSizing::FILL,
        ));
        assert!(ui.find("No patterns found").is_ok());
        let state = with_palette("brick", None);
        let mut ui = iced_test::simulator(view_window(
            &state,
            crate::ui::modal::ModalSizing::FILL,
        ));
        assert!(ui.find("No patterns found").is_err());
        assert!(ui.find("BRICK").is_ok());
    }

    #[test]
    fn every_pattern_of_the_tab_has_a_card() {
        // Every pattern of the tab is a card in the grid, the ones far below
        // the visible part of the scrollable included, and no other is.
        let mut state = with_palette("", Some("ANSI31"));
        state.palette.as_mut().unwrap().category = PatternCategory::Other;
        let entries = super::super::hatch_palette::in_category(PatternCategory::Other);
        assert!(entries.len() > 40, "more cards than one screen of the grid shows");
        let mut ui = iced_test::simulator(view_window(
            &state,
            crate::ui::modal::ModalSizing::FILL,
        ));
        for entry in &entries {
            let shown = crate::ui::text_util::elide(&entry.name, 18);
            assert!(ui.find(shown.as_str()).is_ok(), "{} has a card", entry.name);
        }
        let ansi = super::super::hatch_palette::in_category(PatternCategory::Ansi);
        let foreign = crate::ui::text_util::elide(&ansi[0].name, 18);
        assert!(ui.find(foreign.as_str()).is_err(), "{foreign} is on another tab");
    }
}
