//! "Hatch Edit": the Hatch and Gradient window opened on an existing hatch.
//! What the window starts from (the hatch's own values) and which of them the
//! user changed, so that OK touches those and nothing else.
//!
//! Pure data, like `hatch_settings`: no app, scene or GPU.

use codec::entities::{Hatch, HatchStyleType};
use codec::Handle;

use super::hatch_settings::{parse_angle_deg, parse_scale, HatchColor, HatchSettings, OriginMode};
use crate::command::{CmdResult, HatchEditOperation};

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
    /// The window's fields as they opened: the reference for "changed".
    pub initial: HatchSettings,
    /// The pattern name exactly as the hatch stores it.
    pub pattern: String,
    /// Scale and angle (degrees) as HATCHEDIT reads them; handed back as they
    /// are for a field the user left alone.
    pub scale: f32,
    pub angle_deg: f32,
}

/// What OK would change on the hatch. Empty when nothing changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HatchEditChanges {
    /// Catalog name of the new pattern.
    pub pattern: Option<String>,
    pub scale: Option<f32>,
    pub angle_deg: Option<f32>,
    pub style: Option<HatchStyleType>,
    /// The hatch was associative and "Associative" was switched off.
    pub disassociate: bool,
    /// New pattern origin, in the hatch's plane coordinates.
    pub origin: Option<[f64; 2]>,
}

impl HatchEditChanges {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

impl EditTarget {
    pub fn from_hatch(handle: Handle, hatch: &Hatch) -> Self {
        // The catalog's spelling, so the drop-down shows the pattern selected;
        // a pattern the catalog does not have keeps its own name.
        let pattern = crate::scene::model::hatch_patterns::find(&hatch.pattern.name)
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| hatch.pattern.name.clone());
        let initial = HatchSettings {
            // An existing hatch has a colour of its own, never "use current".
            color: HatchColor::Color(hatch.common.color),
            pattern,
            angle: format_number(hatch.pattern_angle.to_degrees()),
            scale: format_number(hatch.pattern_scale),
            associative: hatch.is_associative,
            separate: false,
            retain: false,
            island_detection: hatch.style != HatchStyleType::Ignore,
            island_style: hatch.style,
            origin_mode: OriginMode::Current,
            ..HatchSettings::default()
        };
        Self {
            handle,
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

    /// Angle and scale usable, and the pattern in the catalog unless it is
    /// still the hatch's own (a hatch read from a file can carry a pattern
    /// the catalog does not have).
    pub fn fields_valid(&self, settings: &HatchSettings) -> bool {
        parse_angle_deg(&settings.angle).is_some()
            && parse_scale(&settings.scale).is_some()
            && (!self.pattern_changed(settings)
                || crate::scene::model::hatch_patterns::find(&settings.pattern).is_some())
    }

    /// The fields `settings` changes, or `None` while one is unusable.
    /// `specified_origin` is the point picked with "Click to set new origin",
    /// in the hatch's plane coordinates; it counts only in "Specified origin".
    pub fn changes(
        &self,
        settings: &HatchSettings,
        specified_origin: Option<[f64; 2]>,
    ) -> Option<HatchEditChanges> {
        if !self.fields_valid(settings) {
            return None;
        }
        let initial = &self.initial;
        let pattern = if self.pattern_changed(settings) {
            Some(crate::scene::model::hatch_patterns::find(&settings.pattern)?.name.clone())
        } else {
            None
        };
        let scale = parse_scale(&settings.scale)?;
        let angle = parse_angle_deg(&settings.angle)?;
        let style = settings.effective_island_style();
        Some(HatchEditChanges {
            pattern,
            scale: (Some(scale) != parse_scale(&initial.scale)).then_some(scale),
            angle_deg: (Some(angle) != parse_angle_deg(&initial.angle)).then_some(angle),
            style: (style != initial.effective_island_style()).then_some(style),
            disassociate: initial.associative && !settings.associative,
            origin: match settings.origin_mode {
                OriginMode::Specified => specified_origin,
                OriginMode::Current => None,
            },
        })
    }

    /// The HATCHEDIT update that applies `changes` and nothing else: name,
    /// scale and angle of an untouched field are the hatch's own.
    pub fn apply_result(&self, changes: &HatchEditChanges) -> CmdResult {
        CmdResult::HatcheditApply {
            handle: self.handle,
            name: changes.pattern.clone().unwrap_or_else(|| self.pattern.clone()),
            scale: changes.scale.unwrap_or(self.scale),
            angle: changes.angle_deg.unwrap_or(self.angle_deg),
            operation: HatchEditOperation::Update {
                origin: changes.origin.map(|point| (point[0], point[1])),
                store_origin: false,
                disassociate: changes.disassociate,
                style: changes.style,
                annotative: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::hatch::HatchPattern;

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
        assert_eq!(
            got,
            HatchEditChanges {
                scale: Some(4.0),
                ..HatchEditChanges::default()
            }
        );
        assert!(!got.is_empty());
    }

    #[test]
    fn a_pattern_change_is_only_a_pattern_change() {
        let target = target();
        let got = changes(&target, |s| s.pattern = "brick".into());
        assert_eq!(
            got,
            HatchEditChanges {
                pattern: Some("BRICK".into()),
                ..HatchEditChanges::default()
            }
        );
    }

    #[test]
    fn an_angle_change_is_only_an_angle_change() {
        let target = target();
        let got = changes(&target, |s| s.angle = "-15".into());
        assert_eq!(
            got,
            HatchEditChanges {
                angle_deg: Some(-15.0),
                ..HatchEditChanges::default()
            }
        );
    }

    #[test]
    fn island_style_changes_follow_the_effective_style() {
        let target = EditTarget::from_hatch(
            Handle::new(1),
            &hatch("ANSI31", 1.0, 0.0, HatchStyleType::Normal),
        );
        let off = changes(&target, |s| s.island_detection = false);
        assert_eq!(off.style, Some(HatchStyleType::Ignore), "Normal -> Ignore");
        assert!(off.pattern.is_none() && off.scale.is_none() && !off.disassociate);
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
        assert_eq!(scaled.scale, Some(3.0));
        assert!(scaled.pattern.is_none(), "the pattern is not rebuilt");
        // Another unknown pattern is not.
        let mut other = settings.clone();
        other.pattern = "ALSO_UNKNOWN".into();
        assert!(!target.fields_valid(&other));
        // A catalog pattern replaces it.
        let mut known = settings;
        known.pattern = "ANSI37".into();
        assert!(target.pattern_changed(&known));
        assert_eq!(target.changes(&known, None).unwrap().pattern.as_deref(), Some("ANSI37"));
    }

    fn update_of(result: CmdResult) -> (Handle, String, f32, f32, HatchEditOperation) {
        match result {
            CmdResult::HatcheditApply {
                handle,
                name,
                scale,
                angle,
                operation,
            } => (handle, name, scale, angle, operation),
            _ => panic!("expected HatcheditApply"),
        }
    }

    #[test]
    fn the_update_hands_back_the_hatch_values_for_the_untouched_fields() {
        let target = EditTarget::from_hatch(
            Handle::new(9),
            &hatch("ansi31", 0.1, 0.5, HatchStyleType::Normal),
        );
        let (handle, name, scale, angle, operation) =
            update_of(target.apply_result(&HatchEditChanges {
                style: Some(HatchStyleType::Outer),
                ..HatchEditChanges::default()
            }));
        assert_eq!(handle, Handle::new(9));
        assert_eq!(name, "ansi31", "stored name, not the catalog spelling");
        assert_eq!(scale, 0.1f64 as f32);
        assert_eq!(angle, 0.5f64.to_degrees() as f32);
        assert!(matches!(
            operation,
            HatchEditOperation::Update {
                origin: None,
                store_origin: false,
                disassociate: false,
                style: Some(HatchStyleType::Outer),
                annotative: None,
            }
        ));
    }

    #[test]
    fn the_update_carries_the_changed_fields() {
        let target = target();
        let (_, name, scale, angle, operation) = update_of(target.apply_result(&HatchEditChanges {
            pattern: Some("BRICK".into()),
            scale: Some(4.0),
            angle_deg: Some(-15.0),
            style: None,
            disassociate: true,
            origin: Some([3.0, 4.0]),
        }));
        assert_eq!(name, "BRICK");
        assert_eq!(scale, 4.0);
        assert_eq!(angle, -15.0);
        assert!(matches!(
            operation,
            HatchEditOperation::Update {
                origin: Some((3.0, 4.0)),
                store_origin: false,
                disassociate: true,
                style: None,
                annotative: None,
            }
        ));
    }
}
