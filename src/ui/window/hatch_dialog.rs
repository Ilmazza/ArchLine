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
use crate::scene::model::hatch_model::{GradientKind, HatchPattern};
use crate::t;
use codec::types::Color as AcadColor;
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

/// The message that sets `slot` to the colour "Select Color" returned. A
/// gradient's colours are true colours: ByLayer, ByBlock and None are refused.
pub fn color_pick_message(slot: HatchColorSlot, color: codec::types::Color) -> Option<Message> {
    use codec::types::Color;
    let logical = matches!(color, Color::ByLayer | Color::ByBlock | Color::None);
    if slot != HatchColorSlot::Fill && logical {
        return None;
    }
    Some(Message::HatchDialogField(slot.field(color)))
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
            // The list's field may be hidden by the switch: close it, or it
            // would reopen when that field comes back.
            Field::Tab(tab) => {
                self.settings.tab = tab;
                self.color_list = None;
            }
            Field::Color(color) => {
                self.settings.color = color;
                self.color_list = None;
            }
            Field::GradientShape(index) => {
                self.settings.gradient.shape = index.min(GradientKind::CHOICES.len() - 1)
            }
            Field::GradientOneColor(on) => {
                self.settings.gradient.one_color = on;
                self.color_list = None;
            }
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

    /// The gradient the swatch draws and its first colour. An unusable angle
    /// draws as 0 degrees: the swatch is never empty.
    pub fn gradient_swatch(&self) -> (HatchPattern, [f32; 4]) {
        let mut gradient = self.settings.gradient.clone();
        if gradient.angle_error() {
            gradient.angle = "0".into();
        }
        gradient.spec().expect("the angle is usable").model_pattern()
    }
}

/// Largest size the window opens at (the app's `sized_flow` caps). The
/// Gradient tab is lower than the Hatch tab and is measured as the Hatch tab.
pub const MAX_WIDTH: u16 = 940;
pub const MAX_HEIGHT: u16 = 760;

/// Shape names in the order of `GradientKind::CHOICES`.
pub fn gradient_shape_labels() -> Vec<String> {
    GradientKind::CHOICES
        .iter()
        .map(|&(kind, invert)| t!(kind.choice_label(invert)).into_owned())
        .collect()
}

pub fn gradient_shape_index(label: &str) -> Option<usize> {
    gradient_shape_labels().iter().position(|candidate| candidate == label)
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

/// What a colour control shows: the colour of its swatch and, for the fill
/// colour on "Use Current", those words instead of the colour's name (the
/// swatch is then the current colour).
fn color_row_shown(
    state: &State,
    slot: HatchColorSlot,
    current: AcadColor,
) -> (AcadColor, Option<String>) {
    let settings = &state.settings;
    match slot {
        HatchColorSlot::Fill => match settings.color {
            HatchColor::UseCurrent => (current, Some(t!("Use Current").into_owned())),
            HatchColor::Color(color) => (color, None),
        },
        HatchColorSlot::Gradient1 => (settings.gradient.color1, None),
        HatchColorSlot::Gradient2 => (settings.gradient.color2, None),
    }
}

/// One colour control: the shared selector, with "Select Color..." wired to
/// the colour window of this slot. Only the fill colour offers ByLayer and
/// ByBlock: a gradient's colours are true colours.
fn color_row<'a>(state: &State, slot: HatchColorSlot, current: AcadColor) -> Element<'a, Message> {
    use crate::ui::color_select::{color_selector_labelled, ColorExtras};
    let (shown, label) = color_row_shown(state, slot, current);
    let open = state.color_list == Some(slot);
    let logical = slot == HatchColorSlot::Fill;
    color_selector_labelled(
        shown,
        label,
        open,
        ColorExtras {
            by_layer: logical,
            by_block: logical,
            ..Default::default()
        },
        move |color| Message::HatchDialogField(slot.field(color)),
        Message::HatchDialogField(Field::ColorList((!open).then_some(slot))),
        Message::HatchDialogField(Field::SelectColor(slot)),
    )
}

/// The small "Use Current" button beside the fill colour: off while already
/// chosen, absent while editing (a hatch always has a colour of its own).
fn use_current_button<'a>(state: &State) -> Option<Element<'a, Message>> {
    if state.edit.is_some() {
        return None;
    }
    let back = (state.settings.color != HatchColor::UseCurrent)
        .then_some(Message::HatchDialogField(Field::Color(HatchColor::UseCurrent)));
    Some(
        button(text(t!("Use Current")).size(11))
            .padding([3, 8])
            .style(button::secondary)
            .on_press_maybe(back)
            .into(),
    )
}

/// The Hatch tab's colour line: the selector and, creating, "Use Current".
fn fill_color_line<'a>(state: &State, current: AcadColor) -> Element<'a, Message> {
    let mut line = row![color_row(state, HatchColorSlot::Fill, current)]
        .spacing(6)
        .align_y(iced::Center);
    if let Some(button) = use_current_button(state) {
        line = line.push(button);
    }
    line.into()
}

/// The colour a pattern or solid swatch draws in. `None` keeps the theme's
/// colour: ByLayer, ByBlock and None have no colour of their own here, and
/// colour 7 is white or black after the background, as the theme's text is.
fn pattern_swatch_color(color: HatchColor, current: AcadColor) -> Option<iced::Color> {
    let color = match color {
        HatchColor::UseCurrent => current,
        HatchColor::Color(color) => color,
    };
    if color == AcadColor::Index(7) {
        return None;
    }
    color.rgb().map(|(r, g, b)| iced::Color::from_rgb8(r, g, b))
}

/// The Hatch tab's swatch: the pattern at the typed angle and scale, in the
/// fill colour; a plain fill when the pattern is unknown so the box is never
/// empty.
fn pattern_preview(state: &State, current: AcadColor) -> crate::ui::properties::HatchPatternPreview {
    use crate::modules::draw::draw::hatch_settings::{parse_angle_deg, parse_scale};
    let angle = parse_angle_deg(&state.settings.angle).unwrap_or(0.0).to_radians();
    let scale = parse_scale(&state.settings.scale).unwrap_or(1.0);
    let gpu = crate::scene::model::hatch_patterns::find(&state.settings.pattern)
        .map(|entry| entry.gpu.clone())
        .unwrap_or(HatchPattern::Solid);
    let preview =
        crate::ui::properties::HatchPatternPreview::new(gpu).with_angle_scale(angle, scale);
    match pattern_swatch_color(state.settings.color, current) {
        Some(color) => preview.with_color(color),
        None => preview,
    }
}

/// The Gradient tab's swatch: the chosen shape in the colours it will have.
fn gradient_preview(state: &State) -> crate::ui::properties::HatchPatternPreview {
    let (pattern, first) = state.gradient_swatch();
    crate::ui::properties::HatchPatternPreview::new(pattern)
        .with_color(iced::Color::from_rgba(first[0], first[1], first[2], first[3]))
}

fn type_and_pattern<'a>(state: &State, current: AcadColor) -> Element<'a, Message> {
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

    let swatch = canvas(pattern_preview(state, current))
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
            labelled(t!("Color").into_owned(), fill_color_line(state, current)),
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

/// The Gradient tab's left column: colours, shape (with its swatch) and
/// orientation. No pattern and no origin: a gradient has neither.
fn gradient_left<'a>(state: &State, current: AcadColor) -> Element<'a, Message> {
    let g = &state.settings.gradient;
    let modes = column![
        form_radio(t!("One color").into_owned(), true, Some(g.one_color), |on| {
            Message::HatchDialogField(Field::GradientOneColor(on))
        }),
        form_radio(t!("Two colors").into_owned(), false, Some(g.one_color), |on| {
            Message::HatchDialogField(Field::GradientOneColor(on))
        }),
    ]
    .spacing(6);
    let second: Element<'a, Message> = if g.one_color {
        labelled(
            t!("Tint/Shade").into_owned(),
            row![
                iced::widget::slider(0.0..=1.0, g.tint, |v| {
                    Message::HatchDialogField(Field::GradientTint(v))
                })
                .step(0.01),
                // Fixed width: the slider keeps its length while the value
                // under it changes.
                text(format!("{:.0}%", g.tint * 100.0)).size(11).width(34),
            ]
            .spacing(6)
            .align_y(iced::Center)
            .into(),
        )
    } else {
        labelled(
            t!("Color 2").into_owned(),
            color_row(state, HatchColorSlot::Gradient2, current),
        )
    };
    let labels = gradient_shape_labels();
    let selected = labels.get(g.shape.min(labels.len() - 1)).cloned();
    let shapes = pick_list(selected, labels, |label: &String| label.clone())
        .on_select(|label: String| {
            Message::HatchDialogField(Field::GradientShape(
                gradient_shape_index(&label).unwrap_or(0),
            ))
        })
        .text_size(12)
        .padding([3, 6])
        .width(Fill);
    let swatch = canvas(gradient_preview(state)).width(Fill).height(Length::Fixed(44.0));
    column![
        group(
            t!("Color").into_owned(),
            column![
                modes,
                labelled(
                    t!("Color 1").into_owned(),
                    color_row(state, HatchColorSlot::Gradient1, current)
                ),
                second,
            ]
            .spacing(6)
            .into(),
        ),
        group(
            t!("Gradient pattern").into_owned(),
            column![shapes, swatch].spacing(6).into(),
        ),
        group(
            t!("Orientation").into_owned(),
            column![
                row![
                    checkbox(g.centered)
                        .on_toggle(|on| Message::HatchDialogField(Field::GradientCentered(on)))
                        .size(14),
                    text(t!("Centered")).size(11),
                ]
                .spacing(6)
                .align_y(iced::Center),
                labelled(
                    t!("Angle").into_owned(),
                    field(&g.angle, state.gradient_angle_message(), Field::GradientAngle),
                ),
            ]
            .spacing(6)
            .into(),
        ),
    ]
    .spacing(8)
    .width(Fill)
    .into()
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

fn tab_button<'a>(label: String, tab: FillTab, active: bool, enabled: bool) -> Element<'a, Message> {
    button(text(label).size(12))
        .padding([4, 14])
        .style(if active { button::primary } else { button::secondary })
        .on_press_maybe(enabled.then_some(Message::HatchDialogField(Field::Tab(tab))))
        .into()
}

/// The left column of `tab`: pattern, angle/scale and origin on the Hatch
/// tab; colours, shape and orientation on the Gradient tab.
fn left_column<'a>(state: &State, current: AcadColor, tab: FillTab) -> Element<'a, Message> {
    match tab {
        FillTab::Hatch => column![
            type_and_pattern(state, current),
            angle_and_scale(state),
            origin_group(state)
        ]
        .spacing(8)
        .width(Fill)
        .into(),
        FillTab::Gradient => gradient_left(state, current),
    }
}

/// The window. `current` is the colour "Use Current" stands for: the owner
/// drawing's current colour.
pub fn view_window<'a>(
    state: &State,
    current: AcadColor,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let active = state.settings.tab;
    // Under the pattern palette (a page of the Hatch tab) the tabs only say
    // where it is.
    let switchable = state.palette.is_none();
    let tabs = row![
        tab_button(t!("Hatch").into_owned(), FillTab::Hatch, active == FillTab::Hatch, switchable),
        tab_button(
            t!("Gradient").into_owned(),
            FillTab::Gradient,
            active == FillTab::Gradient,
            switchable
        ),
    ]
    .spacing(8);

    if let Some(palette) = &state.palette {
        return super::hatch_palette::page(tabs.into(), palette, sizing);
    }

    // A tabbed window keeps its size when the tab changes: the copy that
    // measures the window always lays out the Hatch tab's left column (the
    // taller one), and the shown copy fills that frame with the buttons at
    // the bottom.
    let measuring = !matches!(sizing.height, Length::Fill);
    let left = left_column(state, current, if measuring { FillTab::Hatch } else { active });
    let height = if measuring { Length::Shrink } else { Length::Fill };
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

    column![tabs, row![left, middle, right].spacing(10).height(height), actions]
        .spacing(10)
        .padding(10)
        .width(sizing.width)
        .height(height)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::draw::draw::hatch_settings::{
        FillTab, HatchColor, HatchRegion, RegionOrigin,
    };
    use crate::command::WorkingPlane;
    use crate::ui::modal::ModalSizing;

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
        let mut ui = iced_test::simulator(view_window(&state, codec::types::Color::ByLayer, crate::ui::modal::ModalSizing::FILL));
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
        let mut ui = iced_test::simulator(view_window(&create, codec::types::Color::ByLayer, crate::ui::modal::ModalSizing::FILL));
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
        // Switching mode or tab closes it too: it would reopen when the
        // field comes back.
        state.apply(Field::ColorList(Some(HatchColorSlot::Gradient2)));
        state.apply(Field::GradientOneColor(true));
        assert_eq!(state.color_list, None, "One color closes the list");
        assert!(state.settings.gradient.one_color);
        state.apply(Field::ColorList(Some(HatchColorSlot::Fill)));
        state.apply(Field::Tab(FillTab::Gradient));
        assert_eq!(state.color_list, None, "a tab change closes the list");
        assert_eq!(state.settings.tab, FillTab::Gradient);
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
    fn choosing_a_fill_colour_closes_the_list() {
        use codec::types::Color;
        let mut state = state();
        state.apply(Field::ColorList(Some(HatchColorSlot::Fill)));
        state.apply(Field::Color(HatchColor::Color(Color::Index(5))));
        assert_eq!(state.color_list, None);
        assert_eq!(state.settings.color, HatchColor::Color(Color::Index(5)));
    }

    #[test]
    fn the_pick_message_refuses_the_logical_colours_for_a_gradient_only() {
        use codec::types::Color;
        let field = |message: Option<Message>| match message {
            Some(Message::HatchDialogField(field)) => Some(field),
            Some(other) => panic!("a field message, got {other:?}"),
            None => None,
        };
        for logical in [Color::ByLayer, Color::ByBlock, Color::None] {
            assert!(field(color_pick_message(HatchColorSlot::Gradient1, logical)).is_none());
            assert!(field(color_pick_message(HatchColorSlot::Gradient2, logical)).is_none());
            assert!(matches!(
                field(color_pick_message(HatchColorSlot::Fill, logical)),
                Some(Field::Color(HatchColor::Color(c))) if c == logical
            ));
        }
        let rgb = Color::Rgb { r: 1, g: 2, b: 3 };
        assert!(matches!(
            field(color_pick_message(HatchColorSlot::Gradient1, rgb)),
            Some(Field::GradientColor1(c)) if c == rgb
        ));
        assert!(matches!(
            field(color_pick_message(HatchColorSlot::Gradient2, Color::Index(4))),
            Some(Field::GradientColor2(c)) if c == Color::Index(4)
        ));
        assert!(matches!(
            field(color_pick_message(HatchColorSlot::Fill, Color::Index(4))),
            Some(Field::Color(HatchColor::Color(c))) if c == Color::Index(4)
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

    // ── Gradient tab: pure data the view draws ─────────────────────────────

    #[test]
    fn the_shape_list_has_the_nine_choices_in_order_and_maps_back() {
        let labels = gradient_shape_labels();
        assert_eq!(labels.len(), 9);
        for (index, label) in labels.iter().enumerate() {
            assert_eq!(gradient_shape_index(label), Some(index), "{label}");
        }
        assert_eq!(gradient_shape_index("nonsense"), None);
        assert_eq!(labels[0], "Linear");
        assert_eq!(labels[2], "Inverted cylindrical");
    }

    #[test]
    fn the_gradient_swatch_shows_the_effective_colours() {
        use crate::scene::model::hatch_model::HatchPattern;
        let mut state = state();
        state.apply(Field::Tab(FillTab::Gradient));
        state.apply(Field::GradientOneColor(true));
        state.apply(Field::GradientTint(0.5));
        state.apply(Field::GradientColor1(codec::types::Color::Index(1)));
        state.apply(Field::GradientColor2(codec::types::Color::Index(3))); // hidden
        let (pattern, first) = state.gradient_swatch();
        assert_eq!(first, [1.0, 0.0, 0.0, 1.0]);
        let HatchPattern::Gradient { color2, one_color, .. } = pattern else {
            panic!("a gradient swatch")
        };
        assert!(one_color);
        assert_ne!(color2, [0.0, 1.0, 0.0, 1.0], "never the hidden Color 2");
    }

    #[test]
    fn a_bad_gradient_angle_still_gives_a_swatch() {
        let mut state = state();
        state.apply(Field::GradientAngle("x".into()));
        let (_, first) = state.gradient_swatch();
        assert_eq!(first[3], 1.0);
    }

    #[test]
    fn the_colour_control_shows_its_slot_and_use_current_the_current_colour() {
        use codec::types::Color;
        let mut state = state();
        let current = Color::Index(6);
        assert_eq!(
            color_row_shown(&state, HatchColorSlot::Fill, current),
            (current, Some("Use Current".to_string())),
            "the swatch of the current colour, under the words Use Current"
        );
        state.apply(Field::Color(HatchColor::Color(Color::Index(2))));
        state.apply(Field::GradientColor1(Color::Index(3)));
        state.apply(Field::GradientColor2(Color::Index(4)));
        assert_eq!(color_row_shown(&state, HatchColorSlot::Fill, current), (Color::Index(2), None));
        assert_eq!(
            color_row_shown(&state, HatchColorSlot::Gradient1, current),
            (Color::Index(3), None)
        );
        assert_eq!(
            color_row_shown(&state, HatchColorSlot::Gradient2, current),
            (Color::Index(4), None)
        );
    }

    #[test]
    fn the_pattern_swatch_takes_the_fill_colour() {
        use codec::types::Color;
        let red = Some(iced::Color::from_rgb8(255, 0, 0));
        let green = Some(iced::Color::from_rgb8(0, 255, 0));
        assert_eq!(pattern_swatch_color(HatchColor::Color(Color::Index(1)), Color::Index(3)), red);
        assert_eq!(
            pattern_swatch_color(HatchColor::Color(Color::Rgb { r: 0, g: 255, b: 0 }), Color::Index(1)),
            green
        );
        assert_eq!(pattern_swatch_color(HatchColor::UseCurrent, Color::Index(3)), green);
        // Colours with no RGB of their own, and colour 7 (white or black
        // after the background), keep the theme's colour.
        for theme in [Color::ByLayer, Color::ByBlock, Color::None, Color::Index(7)] {
            assert_eq!(pattern_swatch_color(HatchColor::Color(theme), Color::Index(1)), None, "{theme:?}");
            assert_eq!(pattern_swatch_color(HatchColor::UseCurrent, theme), None, "{theme:?}");
        }
    }

    #[test]
    fn the_swatches_are_built_with_their_colours() {
        use crate::scene::model::hatch_model::HatchPattern;
        use codec::types::Color;
        let red = Some(iced::Color::from_rgb8(255, 0, 0));
        let green = Some(iced::Color::from_rgb8(0, 255, 0));
        let mut state = state();
        // "Use Current": the current colour the window was given.
        let preview = pattern_preview(&state, Color::Index(3));
        assert!(matches!(preview.parts(), (HatchPattern::Pattern(_), c) if c == green));
        assert_eq!(pattern_preview(&state, Color::ByLayer).parts().1, None, "theme colour");
        // A chosen colour, for a pattern and for SOLID.
        state.apply(Field::Color(HatchColor::Color(Color::Index(1))));
        assert_eq!(pattern_preview(&state, Color::Index(3)).parts().1, red);
        state.apply(Field::Pattern("SOLID".into()));
        let preview = pattern_preview(&state, Color::Index(3));
        assert!(matches!(preview.parts(), (HatchPattern::Solid, c) if c == red));
        // The gradient swatch: the swatch's gradient, in its first colour.
        state.apply(Field::GradientColor1(Color::Index(3)));
        let (pattern, first) = state.gradient_swatch();
        let preview = gradient_preview(&state);
        assert!(matches!(preview.parts().0, HatchPattern::Gradient { .. }));
        // `HatchPattern` has no `PartialEq`: its debug form says every field.
        assert_eq!(format!("{:?}", preview.parts().0), format!("{pattern:?}"));
        assert_eq!(first, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(preview.parts().1, green);
    }

    // ── The window's widgets per tab and mode, through the simulator ───────

    /// Every text the window shows, in layout order, the open colour list
    /// included.
    fn texts_shown(state: &State, current: codec::types::Color) -> Vec<String> {
        let mut ui = iced_test::simulator(view_window(state, current, ModalSizing::FILL));
        let mut seen = Vec::new();
        let _ = ui.find(|candidate: iced_test::selector::Candidate<'_>| -> Option<()> {
            if let iced_test::selector::Candidate::Text { content, .. } = candidate {
                seen.push(content.to_string());
            }
            None
        });
        seen
    }

    fn texts(state: &State) -> Vec<String> {
        texts_shown(state, codec::types::Color::ByLayer)
    }

    fn count(texts: &[String], wanted: &str) -> usize {
        texts.iter().filter(|text| text.as_str() == wanted).count()
    }

    fn fields(messages: Vec<Message>) -> Vec<Field> {
        messages
            .into_iter()
            .filter_map(|message| match message {
                Message::HatchDialogField(field) => Some(field),
                _ => None,
            })
            .collect()
    }

    /// The fields the window publishes when the `nth` text `label` (in
    /// layout order) is clicked, or the point `dx` px to its left/right.
    fn click_nth(state: &State, label: &str, nth: usize, dx: f32) -> Vec<Field> {
        let mut ui = iced_test::simulator(view_window(
            state,
            codec::types::Color::ByLayer,
            ModalSizing::FILL,
        ));
        let mut found = Vec::new();
        let _ = ui.find(|candidate: iced_test::selector::Candidate<'_>| -> Option<()> {
            if let iced_test::selector::Candidate::Text { content, bounds, .. } = candidate {
                if content == label {
                    found.push(bounds);
                }
            }
            None
        });
        let bounds = found.get(nth).unwrap_or_else(|| panic!("{label} #{nth} is on screen"));
        let center = bounds.center();
        let x = if dx < 0.0 { bounds.x + dx } else { center.x + dx };
        ui.point_at(iced::Point::new(x, center.y));
        let _ = ui.simulate(iced_test::simulator::click());
        fields(ui.into_messages().collect())
    }

    fn gradient_state() -> State {
        let mut state = state();
        state.apply(Field::Tab(FillTab::Gradient));
        state
    }

    #[test]
    fn the_tabs_are_buttons_that_switch_the_fill() {
        let picked = fields(click_all(&state(), &["Gradient"]));
        assert!(
            matches!(picked.as_slice(), [Field::Tab(FillTab::Gradient)]),
            "{picked:?}"
        );
        let picked = fields(click_all(&gradient_state(), &["Hatch"]));
        assert!(matches!(picked.as_slice(), [Field::Tab(FillTab::Hatch)]), "{picked:?}");
        // The pattern palette belongs to the Hatch tab: there the tabs only
        // say where it is.
        let palette = with_palette("", Some("ANSI31"));
        let picked = fields(click_all(&palette, &["Gradient", "Hatch"]));
        assert!(picked.is_empty(), "{picked:?}");
    }

    #[test]
    fn each_tab_shows_its_own_left_column_and_the_same_others() {
        let hatch = texts(&state());
        let gradient = texts(&gradient_state());
        let hatch_only = [
            "Type and pattern",
            "Pattern",
            "Swatch",
            "Angle and scale",
            "Scale",
            "Hatch origin",
            "Click to set new origin",
        ];
        let gradient_only = [
            "Color 1",
            "Color 2",
            "Gradient pattern",
            "Orientation",
            "Centered",
        ];
        for label in hatch_only {
            assert_eq!(count(&hatch, label), 1, "{label} on the Hatch tab");
            assert_eq!(count(&gradient, label), 0, "{label} not on the Gradient tab");
        }
        for label in gradient_only {
            assert_eq!(count(&hatch, label), 0, "{label} not on the Hatch tab");
        }
        // Radio labels are not texts of their own (the mode radios are tested
        // by clicking them): the Gradient tab's own texts are checked here.
        for label in ["Color 1", "Color 2", "Gradient pattern", "Orientation", "Centered"] {
            assert_eq!(count(&gradient, label), 1, "{label} on the Gradient tab");
        }
        assert_eq!(count(&gradient, "Angle"), 1, "the gradient's own angle");
        assert_eq!(count(&gradient, "Color"), 1, "the gradient's Color group");
        // Middle and right columns and the buttons: the same texts, in the
        // same order, on both tabs.
        let from = |texts: &[String]| {
            let start = texts.iter().position(|t| t == "Boundaries").expect("Boundaries");
            texts[start..].to_vec()
        };
        assert_eq!(from(&hatch), from(&gradient));
        for label in ["Islands", "Boundary retention", "Inherit options", "OK", "Cancel"] {
            assert_eq!(count(&gradient, label), 1, "{label}");
        }
    }

    /// The two mode radios sit above "Color 1": clicking down that strip from
    /// the top meets One color first, then Two colors, and nothing else.
    #[test]
    fn the_mode_radios_publish_one_or_two_colours() {
        let state = gradient_state();
        assert!(!state.settings.gradient.one_color, "starts with two colours");
        let bounds = {
            let mut ui = iced_test::simulator(view_window(
                &state,
                codec::types::Color::ByLayer,
                ModalSizing::FILL,
            ));
            let mut found = Vec::new();
            let _ = ui.find(|candidate: iced_test::selector::Candidate<'_>| -> Option<()> {
                if let iced_test::selector::Candidate::Text { content, bounds, .. } = candidate {
                    if content == "Color 1" {
                        found.push(bounds);
                    }
                }
                None
            });
            assert_eq!(found.len(), 1, "Color 1 is on screen once");
            found[0]
        };
        // (y, field) for every click that published something, top first.
        let mut hits: Vec<(f32, Field)> = Vec::new();
        let mut y = bounds.y - 60.0;
        while y <= bounds.y - 4.0 {
            let mut ui = iced_test::simulator(view_window(
                &state,
                codec::types::Color::ByLayer,
                ModalSizing::FILL,
            ));
            ui.point_at(iced::Point::new(bounds.x + 8.0, y));
            let _ = ui.simulate(iced_test::simulator::click());
            hits.extend(fields(ui.into_messages().collect()).into_iter().map(|f| (y, f)));
            y += 4.0;
        }
        let modes: Vec<bool> = hits
            .iter()
            .map(|(y, field)| match field {
                Field::GradientOneColor(on) => *on,
                other => panic!("only the mode radios publish here, got {other:?} at y {y}"),
            })
            .collect();
        assert!(modes.contains(&true) && modes.contains(&false), "{hits:?}");
        assert_eq!(modes.first(), Some(&true), "One color is the upper radio: {hits:?}");
        assert_eq!(modes.last(), Some(&false), "Two colors is the lower one: {hits:?}");
    }

    #[test]
    fn one_colour_shows_the_tint_and_two_colours_the_second_colour() {
        use codec::types::Color;
        let mut state = gradient_state();
        state.apply(Field::GradientColor1(Color::Index(1)));
        state.apply(Field::GradientColor2(Color::Index(3)));
        let two = texts(&state);
        assert_eq!(count(&two, "Color 2"), 1);
        assert_eq!(count(&two, "Green"), 1, "Color 2's name");
        assert_eq!(count(&two, "Red"), 1, "Color 1's name");
        assert_eq!(count(&two, "Tint/Shade"), 0);
        state.apply(Field::GradientOneColor(true));
        state.apply(Field::GradientTint(0.5));
        let one = texts(&state);
        assert_eq!(count(&one, "Tint/Shade"), 1);
        assert_eq!(count(&one, "50%"), 1, "the tint's value beside the slider");
        assert_eq!(count(&one, "Color 2"), 0);
        assert_eq!(count(&one, "Green"), 0, "the hidden Color 2 is not shown");
        assert_eq!(count(&one, "Red"), 1);
    }

    #[test]
    fn the_error_messages_belong_to_their_tab() {
        let mut state = state();
        state.apply(Field::Pattern("NO_SUCH_PATTERN".into()));
        state.apply(Field::Angle("a".into()));
        state.apply(Field::Scale("0".into()));
        let not_a_number = "Not a valid number";
        let not_found = "Pattern not found: choose another";
        let hatch = texts(&state);
        assert_eq!(count(&hatch, not_found), 1);
        assert_eq!(count(&hatch, not_a_number), 2, "angle and scale");
        state.apply(Field::Tab(FillTab::Gradient));
        let gradient = texts(&state);
        assert_eq!(count(&gradient, not_found), 0, "the pattern is not on this tab");
        assert_eq!(count(&gradient, not_a_number), 0, "nor its angle and scale");
        state.apply(Field::GradientAngle("x".into()));
        assert_eq!(count(&texts(&state), not_a_number), 1, "the gradient's angle");
        state.apply(Field::Tab(FillTab::Hatch));
        state.apply(Field::Angle("0".into()));
        state.apply(Field::Scale("1".into()));
        state.apply(Field::Pattern("ANSI31".into()));
        assert_eq!(count(&texts(&state), not_a_number), 0, "the gradient's angle is not here");
    }

    #[test]
    fn add_preview_and_ok_follow_the_active_tab() {
        let wanted = |fields: &[Message]| {
            (
                fields.iter().any(|m| matches!(m, Message::HatchDialogAdd(AddKind::Points))),
                fields.iter().any(|m| matches!(m, Message::HatchDialogPreview)),
                fields.iter().any(|m| matches!(m, Message::HatchDialogOk)),
            )
        };
        let buttons = ["Add: Pick points", "Preview", "OK"];
        let mut state = state();
        state.regions.push(one_region());
        state.apply(Field::Scale("0".into())); // unusable on the Hatch tab only
        assert_eq!(wanted(&click_all(&state, &buttons)), (false, false, false));
        state.apply(Field::Tab(FillTab::Gradient));
        assert_eq!(wanted(&click_all(&state, &buttons)), (true, true, true));
        state.apply(Field::GradientAngle("x".into()));
        assert_eq!(wanted(&click_all(&state, &buttons)), (false, false, false));
        state.apply(Field::Tab(FillTab::Hatch));
        state.apply(Field::Scale("2".into()));
        assert_eq!(wanted(&click_all(&state, &buttons)), (true, true, true));
    }

    #[test]
    fn the_colour_lists_open_and_publish_into_their_slot() {
        use codec::types::Color;
        // Closed: the head opens its own list.
        let picked = click_nth(&state(), "Use Current", 0, 0.0);
        assert!(
            matches!(picked.as_slice(), [Field::ColorList(Some(HatchColorSlot::Fill))]),
            "{picked:?}"
        );
        // Open on the fill colour: ByLayer/ByBlock, the nine colours and
        // Select Color..., each publishing for the fill colour.
        let mut fill = state();
        fill.apply(Field::ColorList(Some(HatchColorSlot::Fill)));
        let shown = texts(&fill);
        for label in ["ByLayer", "ByBlock", "Red", "Blue", "Select Color..."] {
            assert_eq!(count(&shown, label), 1, "{label} in the fill list");
        }
        let picked = fields(click_all(&fill, &["Red"]));
        assert!(
            matches!(picked.as_slice(), [Field::Color(HatchColor::Color(c))] if *c == Color::Index(1)),
            "{picked:?}"
        );
        let picked = fields(click_all(&fill, &["Select Color..."]));
        assert!(
            matches!(picked.as_slice(), [Field::SelectColor(HatchColorSlot::Fill)]),
            "{picked:?}"
        );
        let picked = fields(click_all(&fill, &["ByLayer"]));
        assert!(
            matches!(picked.as_slice(), [Field::Color(HatchColor::Color(Color::ByLayer))]),
            "{picked:?}"
        );
        // The gradient's colours: true colours only.
        for (slot, ctor) in [
            (HatchColorSlot::Gradient1, (|f: &Field| matches!(f, Field::GradientColor1(c) if *c == Color::Index(5)))
                as fn(&Field) -> bool),
            (HatchColorSlot::Gradient2, |f: &Field| {
                matches!(f, Field::GradientColor2(c) if *c == Color::Index(5))
            }),
        ] {
            let mut gradient = gradient_state();
            gradient.apply(Field::ColorList(Some(slot)));
            let shown = texts(&gradient);
            assert_eq!(count(&shown, "ByLayer"), 0, "{slot:?}");
            assert_eq!(count(&shown, "ByBlock"), 0, "{slot:?}");
            assert_eq!(count(&shown, "Select Color..."), 1, "{slot:?}: one list open");
            let picked = fields(click_all(&gradient, &["Blue"]));
            assert!(picked.len() == 1 && ctor(&picked[0]), "{slot:?}: {picked:?}");
            let picked = fields(click_all(&gradient, &["Select Color..."]));
            assert!(
                matches!(picked.as_slice(), [Field::SelectColor(s)] if *s == slot),
                "{slot:?}: {picked:?}"
            );
        }
    }

    #[test]
    fn use_current_is_a_button_while_creating_only() {
        use codec::types::Color;
        let layer_and_transparency = 2; // the greyed "Use Current" of Options
        // On "Use Current": the head says so, the button is off.
        let state = state();
        assert_eq!(count(&texts(&state), "Use Current"), 2 + layer_and_transparency);
        assert!(click_nth(&state, "Use Current", 1, 0.0).is_empty(), "the button is off");
        // On a chosen colour: the head names it, the button goes back.
        let mut chosen = self::state();
        chosen.apply(Field::Color(HatchColor::Color(Color::Index(1))));
        let shown = texts(&chosen);
        assert_eq!(count(&shown, "Red"), 1);
        assert_eq!(count(&shown, "Use Current"), 1 + layer_and_transparency);
        let picked = click_nth(&chosen, "Use Current", 0, 0.0);
        assert!(
            matches!(picked.as_slice(), [Field::Color(HatchColor::UseCurrent)]),
            "{picked:?}"
        );
        // Editing: the hatch's own colour and no button.
        let edit = edit_state(true, "ANSI31");
        let shown = texts(&edit);
        assert_eq!(count(&shown, "Use Current"), layer_and_transparency);
        assert_eq!(count(&shown, "ByLayer"), 1, "the hatch's own colour");
    }

    #[test]
    fn the_gradient_controls_publish_their_fields() {
        // The checkbox sits 6 px left of its label and is 14 px wide.
        let picked = click_nth(&gradient_state(), "Centered", 0, -13.0);
        assert!(
            matches!(picked.as_slice(), [Field::GradientCentered(false)]),
            "{picked:?}"
        );
        // The gradient's angle field holds its own text.
        let mut state = gradient_state();
        state.apply(Field::Angle("45".into()));
        state.apply(Field::GradientAngle("30".into()));
        let mut ui = iced_test::simulator(view_window(
            &state,
            codec::types::Color::ByLayer,
            ModalSizing::FILL,
        ));
        assert!(ui.find("30").is_ok(), "the gradient's angle");
        assert!(ui.find("45").is_err(), "not the pattern's");
        ui.click("30").expect("the field");
        let _ = ui.typewrite("5");
        let picked = fields(ui.into_messages().collect());
        assert!(
            picked.iter().any(|f| matches!(f, Field::GradientAngle(text) if text.contains('5'))),
            "{picked:?}"
        );
        assert!(!picked.iter().any(|f| matches!(f, Field::Angle(_))), "{picked:?}");
    }

    /// The bottom of OK and of the window's content as the app sizes the
    /// window (`sized_flow` with the window's caps), on the headless renderer.
    fn laid_out(state: &State) -> (f32, f32) {
        let element = crate::ui::modal::intrinsic(
            view_window(state, codec::types::Color::ByLayer, ModalSizing::INTRINSIC),
            view_window(state, codec::types::Color::ByLayer, ModalSizing::FILL),
            iced::Size::new(f32::from(MAX_WIDTH), f32::from(MAX_HEIGHT)),
            iced::Vector::ZERO,
        );
        let mut ui = iced_test::simulator(element);
        let ok = ui.find("OK").expect("OK").bounds();
        let cancel = ui.find("Cancel").expect("Cancel").bounds();
        (ok.y + ok.height, cancel.y + cancel.height)
    }

    #[test]
    fn the_window_fits_and_keeps_its_size_on_both_tabs() {
        use codec::types::Color;
        // The tallest Hatch tab: every message shown.
        let mut hatch = state();
        hatch.apply(Field::Pattern("NO_SUCH_PATTERN".into()));
        hatch.apply(Field::Angle("a".into()));
        hatch.apply(Field::Scale("0".into()));
        hatch.apply(Field::Color(HatchColor::Color(Color::Index(1))));
        let (hatch_ok, _) = laid_out(&hatch);
        // Text bottom + the button's 6 px + the window's 10 px padding.
        assert!(hatch_ok + 16.0 <= f32::from(MAX_HEIGHT), "Hatch tab: OK at {hatch_ok}");
        for one_color in [false, true] {
            let mut gradient = state();
            gradient.apply(Field::Tab(FillTab::Gradient));
            gradient.apply(Field::GradientOneColor(one_color));
            gradient.apply(Field::GradientAngle("x".into()));
            let (ok, _) = laid_out(&gradient);
            assert!(ok + 16.0 <= f32::from(MAX_HEIGHT), "Gradient tab: OK at {ok}");
            // Same state on the other tab: the window does not move.
            let mut back = gradient;
            back.apply(Field::Tab(FillTab::Hatch));
            let (hatch_ok, _) = laid_out(&back);
            assert!(
                (ok - hatch_ok).abs() < 0.5,
                "one colour {one_color}: OK at {ok} on Gradient, {hatch_ok} on Hatch"
            );
        }
    }

    // ── The real widgets, through the iced simulator ───────────────────────

    use super::super::hatch_palette::{Palette, PaletteAction, PatternCategory};

    /// Messages the real window publishes for the given interactions.
    fn click_all(state: &State, labels: &[&str]) -> Vec<Message> {
        let element = view_window(state, codec::types::Color::ByLayer, crate::ui::modal::ModalSizing::FILL);
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
            codec::types::Color::ByLayer,
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
            codec::types::Color::ByLayer,
            crate::ui::modal::ModalSizing::FILL,
        ));
        assert!(ui.find("No patterns found").is_ok());
        let state = with_palette("brick", None);
        let mut ui = iced_test::simulator(view_window(
            &state,
            codec::types::Color::ByLayer,
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
            codec::types::Color::ByLayer,
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
