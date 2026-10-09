//! "Hatch Edit": the Hatch and Gradient window opened on an existing hatch.
//! What the window starts from (the hatch's own values) and which of them the
//! user changed, so that OK touches those and nothing else.
//!
//! Pure data, like `hatch_settings`: no app, scene or GPU.

use codec::entities::{Hatch, HatchStyleType};
use codec::Handle;

use super::hatch_settings::{
    parse_angle_deg, parse_scale, FillTab, GradientSettings, HatchColor, HatchSettings, OriginMode,
};
use crate::command::{CmdResult, HatchEditOperation};
use crate::entities::hatch_fill::{
    read_gradient, FillEdit, FillKind, GradientPatch, GradientSpec, HatchWindowEdit,
};
use crate::scene::model::hatch_model::GradientKind;

/// What OK would change on the hatch: the window's one edit. Empty when
/// nothing changed.
pub type HatchEditChanges = HatchWindowEdit;

/// The Gradient tab's fields for a gradient read from a hatch. A two-colour
/// gradient shows tint 1.0: that is what switching it to One color writes. A
/// tint that is not a number shows as that same white end.
pub fn gradient_settings_from(spec: &GradientSpec) -> GradientSettings {
    let shape = GradientKind::CHOICES
        .iter()
        .position(|&(kind, invert)| kind == spec.kind && invert == spec.invert)
        .unwrap_or(0);
    GradientSettings {
        shape,
        one_color: spec.one_color,
        color1: spec.color1,
        color2: spec.color2,
        tint: if spec.one_color && spec.tint.is_finite() {
            spec.tint.clamp(0.0, 1.0) as f32
        } else {
            1.0
        },
        centered: spec.centered,
        angle: format_number(spec.angle_rad.to_degrees()),
    }
}

/// Whether the window can edit `hatch`: pattern fills only. Solid and gradient
/// fills stay with the Properties panel and -HATCHEDIT.
pub fn is_pattern_hatch(hatch: &Hatch) -> bool {
    !hatch.is_solid
        && !hatch.gradient_color.enabled
        && !hatch.pattern.name.eq_ignore_ascii_case("SOLID")
}

/// A number for a text field: seven significant digits, no trailing zeros.
/// Seven digits are what f32 holds, so a value that went through f32 (every
/// hatch the app makes stores its angle that way) reads as typed, "30" and
/// not "30.000001". A positive value too small to show keeps its full form,
/// so a tiny scale never reads as an unusable zero.
pub fn format_number(value: f64) -> String {
    if value == 0.0 || !value.is_finite() {
        return if value == 0.0 { "0".to_string() } else { value.to_string() };
    }
    let magnitude = value.abs().log10().floor() as i32;
    let decimals = (6 - magnitude).clamp(0, 20) as usize;
    let fixed = format!("{value:.decimals$}");
    let short = if fixed.contains('.') {
        fixed.trim_end_matches('0').trim_end_matches('.')
    } else {
        fixed.as_str()
    };
    match short {
        "0" | "-0" if value > 0.0 => value.to_string(),
        "-0" => "0".to_string(),
        _ => short.to_string(),
    }
}

/// The hatch the window edits and what it held when the window opened.
#[derive(Clone, Debug, PartialEq)]
pub struct EditTarget {
    pub handle: Handle,
    /// The kind of fill the hatch had: with the active tab it decides
    /// whether OK edits the fill in place or converts it.
    pub kind: FillKind,
    /// The window's fields as they opened: the reference for "changed".
    pub initial: HatchSettings,
    /// The pattern name exactly as the hatch stores it.
    pub pattern: String,
    /// Scale and angle (degrees) as HATCHEDIT reads them; handed back as they
    /// are for a field the user left alone.
    pub scale: f32,
    pub angle_deg: f32,
}

impl EditTarget {
    /// The window on `hatch`: the tab of its kind filled from it, the other
    /// tab at its defaults.
    pub fn from_hatch(handle: Handle, hatch: &Hatch) -> Self {
        let kind = FillKind::of(hatch);
        let defaults = HatchSettings::default();
        let (tab, pattern, angle, scale, gradient) = match kind {
            FillKind::Gradient => (
                FillTab::Gradient,
                defaults.pattern.clone(),
                defaults.angle.clone(),
                defaults.scale.clone(),
                gradient_settings_from(&read_gradient(hatch)),
            ),
            FillKind::Pattern | FillKind::Solid => (
                FillTab::Hatch,
                // The catalog's spelling, so the drop-down shows it selected; a
                // pattern the catalog does not have keeps its own name.
                crate::scene::model::hatch_patterns::find(&hatch.pattern.name)
                    .map(|entry| entry.name.clone())
                    .unwrap_or_else(|| hatch.pattern.name.clone()),
                format_number(hatch.pattern_angle.to_degrees()),
                format_number(hatch.pattern_scale),
                GradientSettings::default(),
            ),
        };
        let initial = HatchSettings {
            tab,
            // An existing hatch has a colour of its own, never "use current".
            color: HatchColor::Color(hatch.common.color),
            gradient,
            pattern,
            angle,
            scale,
            associative: hatch.is_associative,
            separate: false,
            retain: false,
            island_detection: hatch.style != HatchStyleType::Ignore,
            island_style: hatch.style,
            origin_mode: OriginMode::Current,
        };
        Self {
            handle,
            kind,
            initial,
            pattern: hatch.pattern.name.clone(),
            // Read exactly as HATCHEDIT reads them.
            scale: hatch.pattern_scale as f32,
            angle_deg: hatch.pattern_angle.to_degrees() as f32,
        }
    }

    /// Whether the pattern field holds another pattern than the hatch's.
    pub fn pattern_changed(&self, settings: &HatchSettings) -> bool {
        !settings.pattern.eq_ignore_ascii_case(&self.initial.pattern)
    }

    /// The active tab's fields are usable; the other tab's never block. On
    /// the Hatch tab the pattern must be in the catalog unless it is still
    /// the hatch's own (a hatch read from a file can carry a pattern the
    /// catalog does not have).
    pub fn fields_valid(&self, settings: &HatchSettings) -> bool {
        match settings.tab {
            FillTab::Gradient => !settings.gradient.angle_error(),
            FillTab::Hatch => {
                parse_angle_deg(&settings.angle).is_some()
                    && parse_scale(&settings.scale).is_some()
                    && (!self.pattern_changed(settings)
                        || crate::scene::model::hatch_patterns::find(&settings.pattern).is_some())
            }
        }
    }

    /// What OK changes, or `None` while a field of the active tab is
    /// unusable. The active tab against the hatch's kind decides: the same
    /// kind edits only the fields that changed, the other converts. Fields of
    /// the other tab are ignored. `specified_origin` is the point picked with
    /// "Click to set new origin", in the hatch's plane coordinates; it counts
    /// only in "Specified origin" and only from the Hatch tab.
    pub fn changes(
        &self,
        settings: &HatchSettings,
        specified_origin: Option<[f64; 2]>,
    ) -> Option<HatchEditChanges> {
        if !self.fields_valid(settings) {
            return None;
        }
        let initial = &self.initial;
        let style = settings.effective_island_style();
        let mut out = HatchWindowEdit {
            style: (style != initial.effective_island_style()).then_some(style),
            disassociate: initial.associative && !settings.associative,
            ..Default::default()
        };
        match (settings.tab, self.kind) {
            (FillTab::Hatch, FillKind::Pattern | FillKind::Solid) => {
                let pattern = if self.pattern_changed(settings) {
                    Some(crate::scene::model::hatch_patterns::find(&settings.pattern)?.name.clone())
                } else {
                    None
                };
                let scale = parse_scale(&settings.scale)?;
                let angle = parse_angle_deg(&settings.angle)?;
                let scale = (Some(scale) != parse_scale(&initial.scale)).then_some(scale);
                let angle_deg = (Some(angle) != parse_angle_deg(&initial.angle)).then_some(angle);
                if pattern.is_some() || scale.is_some() || angle_deg.is_some() {
                    out.fill = Some(FillEdit::Pattern {
                        pattern,
                        scale,
                        angle_deg,
                    });
                }
                out.color = changed_colour(initial, settings);
                out.origin = origin_of(settings, specified_origin);
            }
            (FillTab::Hatch, FillKind::Gradient) => {
                let entry = crate::scene::model::hatch_patterns::find(&settings.pattern)?;
                out.fill = Some(FillEdit::ToPattern {
                    name: entry.name.clone(),
                    scale: parse_scale(&settings.scale)?,
                    angle_deg: parse_angle_deg(&settings.angle)?,
                });
                out.color = changed_colour(initial, settings);
                out.origin = origin_of(settings, specified_origin);
            }
            (FillTab::Gradient, FillKind::Gradient) => {
                let patch = gradient_patch(&initial.gradient, &settings.gradient)?;
                if !patch.is_empty() {
                    out.fill = Some(FillEdit::Gradient(patch));
                }
            }
            (FillTab::Gradient, FillKind::Pattern | FillKind::Solid) => {
                out.fill = Some(FillEdit::ToGradient(settings.gradient.spec()?));
            }
        }
        Some(out)
    }

    /// The HATCHEDIT result that applies `changes` as one operation. Name,
    /// scale and angle around it are the hatch's own (the window's operation
    /// does not read them).
    pub fn apply_result(&self, changes: &HatchEditChanges) -> CmdResult {
        CmdResult::HatcheditApply {
            handle: self.handle,
            name: self.pattern.clone(),
            scale: self.scale,
            angle: self.angle_deg,
            operation: HatchEditOperation::Window(Box::new(changes.clone())),
        }
    }
}

/// The colour picked on the Hatch tab, when it is not the one it opened with.
/// Compared as stored values: the same ACI or true colour picked again is
/// no change.
fn changed_colour(initial: &HatchSettings, now: &HatchSettings) -> Option<codec::types::Color> {
    match (initial.color, now.color) {
        (HatchColor::Color(before), HatchColor::Color(after)) if before != after => Some(after),
        _ => None,
    }
}

fn origin_of(settings: &HatchSettings, picked: Option<[f64; 2]>) -> Option<[f64; 2]> {
    match settings.origin_mode {
        OriginMode::Specified => picked,
        OriginMode::Current => None,
    }
}

/// The fields of the gradient that changed and are visible in the mode the
/// window ends in: with one colour Color 2 is hidden, with two the tint is.
/// `None` while the angle is unusable.
fn gradient_patch(before: &GradientSettings, now: &GradientSettings) -> Option<GradientPatch> {
    let angle = parse_angle_deg(&now.angle)?;
    Some(GradientPatch {
        kind: (now.shape != before.shape).then(|| now.kind_invert()),
        one_color: (now.one_color != before.one_color).then_some(now.one_color),
        color1: (now.color1 != before.color1).then_some(now.color1),
        color2: (!now.one_color && now.color2 != before.color2).then_some(now.color2),
        tint: (now.one_color && (now.tint - before.tint).abs() > 1.0e-6)
            .then_some(now.tint.clamp(0.0, 1.0) as f64),
        angle_rad: (Some(angle) != parse_angle_deg(&before.angle))
            .then(|| (angle as f64).to_radians()),
        centered: (now.centered != before.centered).then_some(now.centered),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::hatch_fill::{
        apply_gradient, apply_window_edit, FillEdit, GradientPatch, GradientSpec, HatchWindowEdit,
    };
    use crate::modules::draw::draw::hatch_settings::{FillTab, GradientSettings, HatchColor};
    use crate::scene::model::hatch_model::GradientKind;
    use codec::entities::hatch::HatchPattern;
    use codec::types::Color;

    /// The edit of a pattern or solid hatch's own fields on the Hatch tab.
    fn pattern_edit(pattern: Option<&str>, scale: Option<f32>, angle_deg: Option<f32>) -> HatchEditChanges {
        HatchEditChanges {
            fill: Some(FillEdit::Pattern {
                pattern: pattern.map(str::to_string),
                scale,
                angle_deg,
            }),
            ..HatchEditChanges::default()
        }
    }

    fn hatch(name: &str, scale: f64, angle_rad: f64, style: HatchStyleType) -> Hatch {
        let mut hatch = Hatch::with_pattern(HatchPattern::new(name));
        hatch.pattern_scale = scale;
        hatch.pattern_angle = angle_rad;
        hatch.style = style;
        hatch.is_associative = true;
        hatch
    }

    fn target() -> EditTarget {
        EditTarget::from_hatch(
            Handle::new(42),
            &hatch("ANSI31", 2.5, 30f64.to_radians(), HatchStyleType::Outer),
        )
    }

    #[test]
    fn the_settings_start_from_the_hatch() {
        let target = target();
        let initial = &target.initial;
        assert_eq!(target.handle, Handle::new(42));
        assert_eq!(initial.pattern, "ANSI31");
        assert_eq!(initial.angle, "30", "degrees, without the radian noise");
        assert_eq!(initial.scale, "2.5");
        assert!(initial.island_detection);
        assert_eq!(initial.island_style, HatchStyleType::Outer);
        assert!(initial.associative);
        assert!(!initial.separate && !initial.retain);
        assert_eq!(initial.origin_mode, OriginMode::Current);
        assert_eq!(target.pattern, "ANSI31");
        assert_eq!(target.scale, 2.5);
        assert_eq!(target.angle_deg, 30.0);
    }

    #[test]
    fn the_ignore_style_starts_with_island_detection_off() {
        let target = EditTarget::from_hatch(
            Handle::new(1),
            &hatch("ANSI31", 1.0, 0.0, HatchStyleType::Ignore),
        );
        assert!(!target.initial.island_detection);
        assert_eq!(target.initial.island_style, HatchStyleType::Ignore);
        assert_eq!(target.initial.effective_island_style(), HatchStyleType::Ignore);
        let normal = EditTarget::from_hatch(
            Handle::new(1),
            &hatch("ANSI31", 1.0, 0.0, HatchStyleType::Normal),
        );
        assert!(normal.initial.island_detection);
        assert_eq!(normal.initial.island_style, HatchStyleType::Normal);
    }

    #[test]
    fn a_non_associative_hatch_starts_with_associative_off() {
        let mut source = hatch("ANSI31", 1.0, 0.0, HatchStyleType::Normal);
        source.is_associative = false;
        let target = EditTarget::from_hatch(Handle::new(1), &source);
        assert!(!target.initial.associative);
    }

    #[test]
    fn the_pattern_shows_its_catalog_name_but_the_stored_name_is_kept() {
        let target = EditTarget::from_hatch(
            Handle::new(1),
            &hatch("ansi31", 1.0, 0.0, HatchStyleType::Normal),
        );
        assert_eq!(target.initial.pattern, "ANSI31", "the drop-down shows it selected");
        assert_eq!(target.pattern, "ansi31", "handed back as stored");
        let custom = EditTarget::from_hatch(
            Handle::new(1),
            &hatch("MY_OWN", 1.0, 0.0, HatchStyleType::Normal),
        );
        assert_eq!(custom.initial.pattern, "MY_OWN");
    }

    #[test]
    fn numbers_are_written_short() {
        assert_eq!(format_number(1.0), "1");
        assert_eq!(format_number(2.5), "2.5");
        assert_eq!(format_number(0.1), "0.1");
        assert_eq!(format_number(30f64.to_radians().to_degrees()), "30");
        // An angle stored through f32, as every hatch the app makes.
        assert_eq!(format_number((30f32.to_radians() as f64).to_degrees()), "30");
        assert_eq!(format_number(0.1f32 as f64), "0.1");
        assert_eq!(format_number(-12.5), "-12.5");
        assert_eq!(format_number(-0.0), "0", "no negative zero");
        assert_eq!(format_number(1.0 / 3.0), "0.3333333");
        assert_eq!(format_number(1234.5678), "1234.568");
        assert_eq!(format_number(100.0), "100");
        assert_eq!(format_number(1.0e10), "10000000000");
        // Too small for six decimals: still a usable, non-zero scale.
        let tiny = format_number(1.0e-7);
        assert_eq!(parse_scale(&tiny), Some(1.0e-7));
    }

    #[test]
    fn only_pattern_fills_are_edited_in_the_window() {
        assert!(is_pattern_hatch(&hatch("ANSI31", 1.0, 0.0, HatchStyleType::Normal)));
        assert!(!is_pattern_hatch(&Hatch::solid()));
        let mut named_solid = hatch("SOLID", 1.0, 0.0, HatchStyleType::Normal);
        named_solid.is_solid = false;
        assert!(!is_pattern_hatch(&named_solid), "a SOLID pattern is a solid fill");
        let mut gradient = hatch("ANSI31", 1.0, 0.0, HatchStyleType::Normal);
        gradient.gradient_color.enabled = true;
        assert!(!is_pattern_hatch(&gradient));
    }

    fn changes(target: &EditTarget, edit: impl FnOnce(&mut HatchSettings)) -> HatchEditChanges {
        let mut settings = target.initial.clone();
        edit(&mut settings);
        target.changes(&settings, None).expect("valid fields")
    }

    #[test]
    fn nothing_changed_is_empty() {
        let target = target();
        let none = changes(&target, |_| {});
        assert!(none.is_empty());
        assert_eq!(none, HatchEditChanges::default());
        // The same values written differently are not a change.
        let same = changes(&target, |s| {
            s.scale = "2,50".into();
            s.angle = "30.0".into();
            s.pattern = "ansi31".into();
        });
        assert!(same.is_empty(), "{same:?}");
    }

    #[test]
    fn a_scale_change_is_only_a_scale_change() {
        let target = target();
        let got = changes(&target, |s| s.scale = "4".into());
        assert_eq!(got, pattern_edit(None, Some(4.0), None));
        assert!(!got.is_empty());
    }

    #[test]
    fn a_pattern_change_is_only_a_pattern_change() {
        let target = target();
        let got = changes(&target, |s| s.pattern = "brick".into());
        assert_eq!(got, pattern_edit(Some("BRICK"), None, None));
    }

    #[test]
    fn an_angle_change_is_only_an_angle_change() {
        let target = target();
        let got = changes(&target, |s| s.angle = "-15".into());
        assert_eq!(got, pattern_edit(None, None, Some(-15.0)));
    }

    #[test]
    fn island_style_changes_follow_the_effective_style() {
        let target = EditTarget::from_hatch(
            Handle::new(1),
            &hatch("ANSI31", 1.0, 0.0, HatchStyleType::Normal),
        );
        let off = changes(&target, |s| s.island_detection = false);
        assert_eq!(off.style, Some(HatchStyleType::Ignore), "Normal -> Ignore");
        assert!(off.fill.is_none() && off.color.is_none() && !off.disassociate);
        let radio = changes(&target, |s| s.island_style = HatchStyleType::Ignore);
        assert_eq!(radio.style, Some(HatchStyleType::Ignore));
        let outer = changes(&target, |s| s.island_style = HatchStyleType::Outer);
        assert_eq!(outer.style, Some(HatchStyleType::Outer));
        // The radio alone does not count while detection is off and was off.
        let ignore = EditTarget::from_hatch(
            Handle::new(1),
            &hatch("ANSI31", 1.0, 0.0, HatchStyleType::Ignore),
        );
        assert!(changes(&ignore, |s| s.island_style = HatchStyleType::Outer).is_empty());
        let back_on = changes(&ignore, |s| {
            s.island_detection = true;
            s.island_style = HatchStyleType::Normal;
        });
        assert_eq!(back_on.style, Some(HatchStyleType::Normal));
    }

    #[test]
    fn switching_associative_off_disassociates() {
        let target = target();
        let got = changes(&target, |s| s.associative = false);
        assert_eq!(
            got,
            HatchEditChanges {
                disassociate: true,
                ..HatchEditChanges::default()
            }
        );
        // A hatch that was not associative has nothing to disassociate.
        let mut source = hatch("ANSI31", 1.0, 0.0, HatchStyleType::Normal);
        source.is_associative = false;
        let loose = EditTarget::from_hatch(Handle::new(1), &source);
        assert!(changes(&loose, |s| s.associative = false).is_empty());
        assert!(changes(&loose, |s| s.associative = true).is_empty());
    }

    #[test]
    fn an_origin_counts_only_when_specified_and_picked() {
        let target = target();
        let mut settings = target.initial.clone();
        assert!(target.changes(&settings, Some([3.0, 4.0])).unwrap().is_empty(), "current mode");
        settings.origin_mode = OriginMode::Specified;
        assert!(target.changes(&settings, None).unwrap().is_empty(), "no point yet");
        let got = target.changes(&settings, Some([3.0, 4.0])).unwrap();
        assert_eq!(
            got,
            HatchEditChanges {
                origin: Some([3.0, 4.0]),
                ..HatchEditChanges::default()
            }
        );
    }

    #[test]
    fn unusable_fields_give_no_changes() {
        let target = target();
        for edit in [
            |s: &mut HatchSettings| s.scale = "0".into(),
            |s: &mut HatchSettings| s.angle = "x".into(),
            |s: &mut HatchSettings| s.pattern = "NO_SUCH_PATTERN".into(),
        ] {
            let mut settings = target.initial.clone();
            edit(&mut settings);
            assert!(!target.fields_valid(&settings), "{settings:?}");
            assert!(target.changes(&settings, None).is_none(), "{settings:?}");
        }
    }

    #[test]
    fn a_pattern_missing_from_the_catalog_is_fine_while_it_is_kept() {
        let target = EditTarget::from_hatch(
            Handle::new(1),
            &hatch("MY_OWN", 1.0, 0.0, HatchStyleType::Normal),
        );
        let settings = target.initial.clone();
        assert!(target.fields_valid(&settings), "the hatch's own pattern");
        assert!(target.changes(&settings, None).unwrap().is_empty(), "but nothing changed");
        let scaled = changes(&target, |s| s.scale = "3".into());
        assert_eq!(scaled, pattern_edit(None, Some(3.0), None), "the pattern is not rebuilt");
        // Another unknown pattern is not.
        let mut other = settings.clone();
        other.pattern = "ALSO_UNKNOWN".into();
        assert!(!target.fields_valid(&other));
        // A catalog pattern replaces it.
        let mut known = settings;
        known.pattern = "ANSI37".into();
        assert!(target.pattern_changed(&known));
        assert_eq!(target.changes(&known, None).unwrap(), pattern_edit(Some("ANSI37"), None, None));
    }

    /// The window's one operation and the name/scale/angle it is wrapped in.
    fn update_of(result: CmdResult) -> (Handle, String, f32, f32, HatchWindowEdit) {
        match result {
            CmdResult::HatcheditApply {
                handle,
                name,
                scale,
                angle,
                operation: HatchEditOperation::Window(edit),
            } => (handle, name, scale, angle, *edit),
            _ => panic!("expected HatcheditApply with the window operation"),
        }
    }

    #[test]
    fn the_update_hands_back_the_hatch_values_for_the_untouched_fields() {
        let target = EditTarget::from_hatch(
            Handle::new(9),
            &hatch("ansi31", 0.1, 0.5, HatchStyleType::Normal),
        );
        let changes = HatchEditChanges {
            style: Some(HatchStyleType::Outer),
            ..HatchEditChanges::default()
        };
        let (handle, name, scale, angle, edit) = update_of(target.apply_result(&changes));
        assert_eq!(handle, Handle::new(9));
        assert_eq!(name, "ansi31", "stored name, not the catalog spelling");
        assert_eq!(scale, 0.1f64 as f32);
        assert_eq!(angle, 0.5f64.to_degrees() as f32);
        assert_eq!(edit, changes);
    }

    #[test]
    fn the_update_carries_the_changed_fields() {
        let target = target();
        let changes = HatchEditChanges {
            fill: Some(FillEdit::Pattern {
                pattern: Some("BRICK".into()),
                scale: Some(4.0),
                angle_deg: Some(-15.0),
            }),
            color: Some(Color::Index(1)),
            style: None,
            disassociate: true,
            origin: Some([3.0, 4.0]),
        };
        let (_, name, scale, angle, edit) = update_of(target.apply_result(&changes));
        // The wrapping values are the hatch's own; the edit carries the rest.
        assert_eq!(name, "ANSI31");
        assert_eq!(scale, 2.5);
        assert_eq!(angle, 30.0);
        assert_eq!(edit, changes);
    }

    // ── Solid and gradient hatches, and the two tabs ───────────────────────

    fn gradient_spec() -> GradientSpec {
        GradientSpec {
            kind: GradientKind::Cylinder,
            invert: false,
            one_color: false,
            color1: Color::Index(1),
            color2: Color::Index(5),
            tint: 0.0, // what a two-colour file written elsewhere often leaves
            angle_rad: 0.5,
            centered: true,
        }
    }

    fn gradient_hatch(spec: &GradientSpec) -> Hatch {
        let mut hatch = Hatch::solid();
        apply_gradient(&mut hatch, spec);
        hatch
    }

    fn gradient_target() -> EditTarget {
        EditTarget::from_hatch(Handle::new(7), &gradient_hatch(&gradient_spec()))
    }

    fn edit(target: &EditTarget, change: impl FnOnce(&mut HatchSettings)) -> Option<HatchWindowEdit> {
        let mut settings = target.initial.clone();
        change(&mut settings);
        target.changes(&settings, None)
    }

    #[test]
    fn a_gradient_opens_on_the_gradient_tab_with_its_own_values() {
        let target = gradient_target();
        assert_eq!(target.kind, FillKind::Gradient);
        let s = &target.initial;
        assert_eq!(s.tab, FillTab::Gradient);
        assert_eq!(s.gradient.shape, 1, "CHOICES[1] = cylindrical");
        assert_eq!((s.gradient.color1, s.gradient.color2), (Color::Index(1), Color::Index(5)));
        assert_eq!(s.gradient.angle, "28.64789", "0.5 rad, seven digits");
        assert!(s.gradient.centered && !s.gradient.one_color);
        assert_eq!(s.color, HatchColor::Color(Color::ByLayer), "the entity's own colour");
    }

    #[test]
    fn a_two_colour_gradient_shows_the_tint_it_will_write() {
        // The file says tint 0; switching to One color writes 1.0 (the panel's
        // rule), so that is what the window must show.
        let target = gradient_target();
        assert_eq!(target.initial.gradient.tint, 1.0);
        let edit = edit(&target, |s| s.gradient.one_color = true).unwrap();
        assert_eq!(
            edit.fill,
            Some(FillEdit::Gradient(GradientPatch {
                one_color: Some(true),
                ..Default::default()
            }))
        );
        // And it is what OK writes.
        let mut hatch = gradient_hatch(&gradient_spec());
        assert_eq!(hatch.gradient_color.color_tint, 0.0);
        apply_window_edit(&mut hatch, &edit);
        assert!(hatch.gradient_color.is_single_color);
        assert_eq!(hatch.gradient_color.color_tint, target.initial.gradient.tint as f64);
    }

    #[test]
    fn a_gradient_read_with_missing_data_opens_without_panicking() {
        let mut hatch = Hatch::solid();
        hatch.gradient_color.enabled = true; // no stops, no name
        let target = EditTarget::from_hatch(Handle::new(1), &hatch);
        assert_eq!(target.initial.tab, FillTab::Gradient);
        assert_eq!(target.initial.gradient.shape, 0);
        assert!(target.changes(&target.initial, None).unwrap().is_empty());
    }

    #[test]
    fn a_gradient_with_one_stop_or_a_tint_that_is_not_a_number_opens_usable() {
        // One stop: Color 2 is the default, and a new Color 2 is one patch field.
        let mut hatch = Hatch::solid();
        hatch.gradient_color.enabled = true;
        hatch.gradient_color.name = "SPHERICAL".into();
        hatch.gradient_color.colors.push(codec::entities::hatch::GradientColorEntry {
            value: 0.0,
            color: Color::Index(3),
        });
        let target = EditTarget::from_hatch(Handle::new(1), &hatch);
        assert_eq!(target.initial.gradient.color1, Color::Index(3));
        assert_eq!(
            target.initial.gradient.color2,
            crate::entities::hatch_fill::DEFAULT_GRADIENT_COLOR2
        );
        assert!(target.changes(&target.initial, None).unwrap().is_empty());
        let new_two = edit(&target, |s| s.gradient.color2 = Color::Index(4)).unwrap();
        assert_eq!(
            new_two.fill,
            Some(FillEdit::Gradient(GradientPatch {
                color2: Some(Color::Index(4)),
                ..Default::default()
            }))
        );
        // A one-colour tint that is not a number shows as the white end and
        // can be changed like any other.
        hatch.gradient_color.is_single_color = true;
        hatch.gradient_color.color_tint = f64::NAN;
        let target = EditTarget::from_hatch(Handle::new(1), &hatch);
        assert_eq!(target.initial.gradient.tint, 1.0);
        let darker = edit(&target, |s| s.gradient.tint = 0.25).unwrap();
        assert_eq!(
            darker.fill,
            Some(FillEdit::Gradient(GradientPatch {
                tint: Some(0.25),
                ..Default::default()
            }))
        );
    }

    #[test]
    fn a_linear_gradient_stored_with_swapped_stops_is_not_rewritten() {
        // One colour, inverted Linear: written with the stops swapped. And a
        // two-colour Linear whose stops another program stored the other way
        // round (the swap is not recognisable there). ACI and true colours.
        let one = gradient_hatch(&GradientSpec {
            kind: GradientKind::Linear,
            invert: true,
            one_color: true,
            color1: Color::Rgb { r: 200, g: 100, b: 50 },
            tint: 0.3,
            ..gradient_spec()
        });
        let mut two = gradient_hatch(&GradientSpec {
            kind: GradientKind::Linear,
            color1: Color::Rgb { r: 10, g: 20, b: 30 },
            color2: Color::Index(5),
            ..gradient_spec()
        });
        two.gradient_color.colors.reverse();
        for hatch in [one, two] {
            let target = EditTarget::from_hatch(Handle::new(1), &hatch);
            assert!(edit(&target, |_| {}).unwrap().is_empty(), "{:?}", target.initial.gradient);
            // Only the centring: only the shift is written, stops untouched.
            let centring = edit(&target, |s| s.gradient.centered = false).unwrap();
            assert_eq!(
                centring.fill,
                Some(FillEdit::Gradient(GradientPatch {
                    centered: Some(false),
                    ..Default::default()
                }))
            );
            let mut after = hatch.clone();
            apply_window_edit(&mut after, &centring);
            let mut expected = hatch.clone();
            expected.gradient_color.shift = 1.0;
            assert_eq!(after, expected);
        }
    }

    #[test]
    fn the_hatch_tab_of_a_gradient_starts_from_the_defaults_not_from_the_gradient() {
        let s = gradient_target().initial;
        let defaults = HatchSettings::default();
        assert_eq!(
            (s.pattern.as_str(), s.angle.as_str(), s.scale.as_str()),
            (defaults.pattern.as_str(), "0", "1")
        );
    }

    #[test]
    fn the_gradient_tab_of_a_pattern_starts_from_the_defaults() {
        let target = target();
        assert_eq!(target.kind, FillKind::Pattern);
        assert_eq!(target.initial.tab, FillTab::Hatch);
        assert_eq!(target.initial.gradient, GradientSettings::default());
    }

    #[test]
    fn nothing_changed_on_a_gradient_is_empty() {
        let target = gradient_target();
        assert!(edit(&target, |_| {}).unwrap().is_empty());
        // The same angle written differently is not a change.
        assert!(edit(&target, |s| s.gradient.angle = "28,64789".into()).unwrap().is_empty());
        // Nor are the Hatch tab's fields while the Gradient tab is active.
        assert!(edit(&target, |s| {
            s.pattern = "BRICK".into();
            s.scale = "3".into();
            s.color = HatchColor::Color(Color::Index(2));
        })
        .unwrap()
        .is_empty());
    }

    #[test]
    fn one_gradient_field_is_one_patch_field() {
        let target = gradient_target();
        let patch = |change: fn(&mut GradientSettings)| match edit(&target, |s| change(&mut s.gradient))
            .unwrap()
            .fill
        {
            Some(FillEdit::Gradient(patch)) => patch,
            other => panic!("expected a gradient patch, got {other:?}"),
        };
        assert_eq!(
            patch(|g| g.shape = 5),
            GradientPatch {
                kind: Some(GradientKind::CHOICES[5]),
                ..Default::default()
            }
        );
        assert_eq!(
            patch(|g| g.color1 = Color::Index(2)),
            GradientPatch {
                color1: Some(Color::Index(2)),
                ..Default::default()
            }
        );
        assert_eq!(
            patch(|g| g.color2 = Color::Index(3)),
            GradientPatch {
                color2: Some(Color::Index(3)),
                ..Default::default()
            }
        );
        assert_eq!(
            patch(|g| g.centered = false),
            GradientPatch {
                centered: Some(false),
                ..Default::default()
            }
        );
        let angle = patch(|g| g.angle = "30".into());
        assert!((angle.angle_rad.unwrap() - 30f64.to_radians()).abs() < 1e-9);
        assert_eq!(GradientPatch { angle_rad: None, ..angle }, GradientPatch::default());
    }

    #[test]
    fn hidden_gradient_fields_never_enable_ok() {
        let target = gradient_target(); // two colours: the tint is hidden
        assert!(edit(&target, |s| s.gradient.tint = 0.2).unwrap().is_empty());
        // One colour: Color 2 is hidden.
        let one = EditTarget::from_hatch(
            Handle::new(2),
            &gradient_hatch(&GradientSpec {
                one_color: true,
                tint: 0.5,
                ..gradient_spec()
            }),
        );
        assert!(edit(&one, |s| s.gradient.color2 = Color::Index(2)).unwrap().is_empty());
        assert!(
            edit(&one, |s| s.gradient.tint = 0.9).unwrap().fill.is_some(),
            "the tint is visible there"
        );
        // Switching the mode is a change of its own, and brings along the
        // field that becomes visible only if it differs from the start.
        let to_two = edit(&one, |s| s.gradient.one_color = false).unwrap();
        assert_eq!(
            to_two.fill,
            Some(FillEdit::Gradient(GradientPatch {
                one_color: Some(false),
                ..Default::default()
            }))
        );
        let to_two_red = edit(&one, |s| {
            s.gradient.one_color = false;
            s.gradient.color2 = Color::Index(1);
        })
        .unwrap();
        assert_eq!(
            to_two_red.fill,
            Some(FillEdit::Gradient(GradientPatch {
                one_color: Some(false),
                color2: Some(Color::Index(1)),
                ..Default::default()
            }))
        );
    }

    #[test]
    fn the_tab_decides_the_conversion() {
        // Pattern -> Gradient tab: a whole gradient, even with nothing touched.
        let pattern = target();
        let to_gradient = edit(&pattern, |s| s.tab = FillTab::Gradient).unwrap();
        assert_eq!(
            to_gradient.fill,
            Some(FillEdit::ToGradient(GradientSettings::default().spec().unwrap()))
        );
        // And back on the Hatch tab: nothing to do.
        assert!(edit(&pattern, |s| s.tab = FillTab::Hatch).unwrap().is_empty());
        // Gradient -> Hatch tab: the default pattern; SOLID if chosen.
        let gradient = gradient_target();
        let to_pattern = edit(&gradient, |s| s.tab = FillTab::Hatch).unwrap();
        assert_eq!(
            to_pattern.fill,
            Some(FillEdit::ToPattern {
                name: "ANSI31".into(),
                scale: 1.0,
                angle_deg: 0.0
            })
        );
        let to_solid = edit(&gradient, |s| {
            s.tab = FillTab::Hatch;
            s.pattern = "SOLID".into();
        })
        .unwrap();
        assert!(matches!(to_solid.fill, Some(FillEdit::ToPattern { ref name, .. }) if name == "SOLID"));
        // A solid is a Hatch-tab hatch: changing its pattern is an ordinary edit.
        let mut solid = Hatch::solid();
        solid.pattern_scale = 1.0;
        let solid = EditTarget::from_hatch(Handle::new(3), &solid);
        assert_eq!(solid.kind, FillKind::Solid);
        assert_eq!(solid.initial.tab, FillTab::Hatch);
        assert_eq!(solid.initial.pattern, "SOLID");
        let to_ansi = edit(&solid, |s| s.pattern = "ANSI31".into()).unwrap();
        assert!(matches!(to_ansi.fill, Some(FillEdit::Pattern { pattern: Some(ref p), .. }) if p == "ANSI31"));
        // A solid on the Gradient tab becomes a gradient too.
        let solid_to_gradient = edit(&solid, |s| s.tab = FillTab::Gradient).unwrap();
        assert!(matches!(solid_to_gradient.fill, Some(FillEdit::ToGradient(_))));
    }

    #[test]
    fn the_entity_colour_changes_only_from_the_hatch_tab() {
        let target = target();
        assert_eq!(target.initial.color, HatchColor::Color(Color::ByLayer));
        let red = edit(&target, |s| s.color = HatchColor::Color(Color::Index(1))).unwrap();
        assert_eq!(red.color, Some(Color::Index(1)));
        assert!(red.fill.is_none());
        let on_gradient_tab = edit(&target, |s| {
            s.color = HatchColor::Color(Color::Index(1));
            s.tab = FillTab::Gradient;
        })
        .unwrap();
        assert_eq!(on_gradient_tab.color, None, "the Gradient tab has no colour of its own");
        // The same colour picked again is no change.
        assert!(edit(&target, |s| s.color = HatchColor::Color(Color::ByLayer)).unwrap().is_empty());
    }

    #[test]
    fn validity_follows_the_tab() {
        let target = target();
        let mut s = target.initial.clone();
        s.scale = "0".into();
        assert!(!target.fields_valid(&s));
        s.tab = FillTab::Gradient;
        assert!(target.fields_valid(&s), "a bad pattern scale does not block the Gradient tab");
        assert!(target.changes(&s, None).is_some());
        s.gradient.angle = "x".into();
        assert!(!target.fields_valid(&s));
        assert!(target.changes(&s, None).is_none());
        // A gradient: a bad gradient angle does not block its Hatch tab.
        let gradient = gradient_target();
        let mut s = gradient.initial.clone();
        s.gradient.angle = "x".into();
        assert!(!gradient.fields_valid(&s));
        s.tab = FillTab::Hatch;
        assert!(gradient.fields_valid(&s));
        assert!(gradient.changes(&s, None).is_some_and(|edit| edit.fill.is_some()));
    }

    #[test]
    fn apply_result_carries_the_whole_edit_in_one_operation() {
        let target = gradient_target();
        let changes = edit(&target, |s| s.gradient.shape = 5).unwrap();
        match target.apply_result(&changes) {
            CmdResult::HatcheditApply {
                handle,
                operation: HatchEditOperation::Window(window),
                ..
            } => {
                assert_eq!(handle, Handle::new(7));
                assert_eq!(*window, changes);
            }
            _ => panic!("expected the window operation"),
        }
    }
}
