//! Pure data and rules behind the HATCH dialog: the settings it edits, the
//! regions it collects, the canonical key that makes two regions "the same",
//! and the parsing that decides whether a field is usable.
//!
//! Nothing here touches the app, the scene or the GPU, so every rule can be
//! tested with plain values.

use codec::entities::HatchStyleType;
use codec::types::Color as AcadColor;

use crate::entities::hatch_fill::{GradientSpec, DEFAULT_GRADIENT_COLOR1, DEFAULT_GRADIENT_COLOR2};
use crate::scene::model::hatch_model::GradientKind;

// ── Regions ────────────────────────────────────────────────────────────────

/// One closed ring in the working plane's local coordinates.
pub type HatchRing = Vec<[f64; 2]>;

/// What one pick fills: an outer ring and any holes inside it. Keeping the
/// rings of a region together is what lets "Create separate hatches" make one
/// hatch per region instead of one per ring.
#[derive(Clone, Debug, PartialEq)]
pub struct HatchRegion {
    /// `rings[0]` is the outer ring; the rest are holes.
    pub rings: Vec<HatchRing>,
}

/// Which "Add" produced a region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionOrigin {
    Points,
    Objects,
}

/// How finely two coordinates must agree to count as the same point.
const QUANTUM: f64 = 1.0e-6;

type QuantizedRing = Vec<(i64, i64)>;

/// Canonical, comparable form of a region: the outer ring plus the sorted set
/// of its holes.
pub type RegionKey = (QuantizedRing, Vec<QuantizedRing>);

fn quantize(value: f64) -> i64 {
    (value / QUANTUM).round() as i64
}

/// Same ring however it was traced: no repeated closing vertex, counter-
/// clockwise, starting at its smallest vertex.
fn canonical_ring(ring: &[[f64; 2]]) -> QuantizedRing {
    let mut points: QuantizedRing = ring
        .iter()
        .map(|point| (quantize(point[0]), quantize(point[1])))
        .collect();
    points.dedup();
    while points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    let twice_area: i128 = (0..points.len())
        .map(|index| {
            let (x0, y0) = points[index];
            let (x1, y1) = points[(index + 1) % points.len()];
            i128::from(x0) * i128::from(y1) - i128::from(x1) * i128::from(y0)
        })
        .sum();
    if twice_area < 0 {
        points.reverse();
    }
    // A pinched ring can visit its smallest vertex more than once; of those
    // starts, take the rotation that reads smallest, so the result depends only
    // on the cyclic sequence and not on where the tracing began.
    let smallest = points.iter().min().copied();
    if let Some(smallest) = smallest {
        let len = points.len();
        let rotation = |start: usize| (0..len).map(move |step| (start + step) % len);
        let start = (0..len)
            .filter(|&index| points[index] == smallest)
            .min_by(|&a, &b| {
                rotation(a)
                    .map(|i| points[i])
                    .cmp(rotation(b).map(|i| points[i]))
            });
        if let Some(start) = start {
            points.rotate_left(start);
        }
    }
    points
}

pub fn region_key(region: &HatchRegion) -> RegionKey {
    let mut rings = region.rings.iter();
    let outer = rings
        .next()
        .map(Vec::as_slice)
        .map(canonical_ring)
        .unwrap_or_default();
    let mut holes: Vec<QuantizedRing> = rings
        .map(Vec::as_slice)
        .map(canonical_ring)
        .collect();
    holes.sort();
    (outer, holes)
}

/// Add `region` unless an equivalent one is already collected, whichever "Add"
/// produced either. Returns whether it was added.
pub fn add_region(
    list: &mut Vec<(HatchRegion, RegionOrigin)>,
    region: HatchRegion,
    origin: RegionOrigin,
) -> bool {
    if region.rings.is_empty() {
        return false;
    }
    let key = region_key(&region);
    if list.iter().any(|(existing, _)| region_key(existing) == key) {
        return false;
    }
    list.push((region, origin));
    true
}

// ── Settings ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginMode {
    Current,
    Specified,
}

/// The two ways to fill: the window's tabs. Pattern and solid share the Hatch
/// tab (SOLID is a catalog entry); the gradient is the other tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillTab {
    Hatch,
    Gradient,
}

/// Colour of a pattern or solid hatch. `UseCurrent` follows the drawing's
/// current colour at the moment the hatch is made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HatchColor {
    UseCurrent,
    Color(AcadColor),
}

/// What the Gradient tab edits. Angle stays as typed text, like the pattern's.
#[derive(Clone, Debug, PartialEq)]
pub struct GradientSettings {
    /// Index into `GradientKind::CHOICES`.
    pub shape: usize,
    pub one_color: bool,
    pub color1: AcadColor,
    pub color2: AcadColor,
    /// 0 = black end, 1 = white end; only with `one_color`.
    pub tint: f32,
    pub centered: bool,
    pub angle: String,
}

impl Default for GradientSettings {
    fn default() -> Self {
        Self {
            shape: 0,
            one_color: false,
            color1: DEFAULT_GRADIENT_COLOR1,
            color2: DEFAULT_GRADIENT_COLOR2,
            tint: 1.0,
            centered: true,
            angle: "0".into(),
        }
    }
}

impl GradientSettings {
    pub fn angle_error(&self) -> bool {
        parse_angle_deg(&self.angle).is_none()
    }

    pub fn kind_invert(&self) -> (GradientKind, bool) {
        GradientKind::CHOICES[self.shape.min(GradientKind::CHOICES.len() - 1)]
    }

    /// `None` while the angle is unusable. The tint comes from a slider as an
    /// `f32`: a non-finite value never reaches the spec (it falls back to the
    /// white end) and a finite one is held to 0..=1.
    pub fn spec(&self) -> Option<GradientSpec> {
        let angle = parse_angle_deg(&self.angle)?;
        let (kind, invert) = self.kind_invert();
        let tint = if self.tint.is_finite() {
            self.tint.clamp(0.0, 1.0)
        } else {
            1.0
        };
        Some(GradientSpec {
            kind,
            invert,
            one_color: self.one_color,
            color1: self.color1,
            color2: self.color2,
            tint: tint as f64,
            angle_rad: (angle as f64).to_radians(),
            centered: self.centered,
        })
    }
}

/// What the dialog edits. Angle and scale stay as typed text so a half-typed
/// number does not snap back while it is being typed.
#[derive(Clone, Debug, PartialEq)]
pub struct HatchSettings {
    /// The active tab; decides which fields `resolve` checks.
    pub tab: FillTab,
    /// Colour of a pattern or solid hatch (the Hatch tab).
    pub color: HatchColor,
    pub gradient: GradientSettings,
    /// Catalog name of the pattern.
    pub pattern: String,
    /// Degrees, as typed.
    pub angle: String,
    /// Scale factor, as typed.
    pub scale: String,
    pub associative: bool,
    pub separate: bool,
    pub retain: bool,
    pub island_detection: bool,
    /// The radio choice; only in force while `island_detection` is on.
    pub island_style: HatchStyleType,
    pub origin_mode: OriginMode,
}

impl Default for HatchSettings {
    fn default() -> Self {
        Self {
            tab: FillTab::Hatch,
            color: HatchColor::UseCurrent,
            gradient: GradientSettings::default(),
            pattern: "ANSI31".into(),
            angle: "0".into(),
            scale: "1".into(),
            associative: true,
            separate: false,
            retain: false,
            island_detection: true,
            island_style: HatchStyleType::Normal,
            origin_mode: OriginMode::Current,
        }
    }
}

/// What the active tab fills with, once its fields are known to be usable.
#[derive(Clone, Debug, PartialEq)]
pub enum ResolvedFill {
    Hatch { pattern: String, angle_rad: f32, scale: f32, color: HatchColor },
    Gradient(GradientSpec),
}

/// Settings once every field of the active tab is known to be usable. Pure
/// data: the current colour, layers and transparency are the app's to resolve.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSettings {
    pub associative: bool,
    pub separate: bool,
    pub retain: bool,
    pub island_style: HatchStyleType,
    pub fill: ResolvedFill,
}

/// A finite number of degrees; comma or dot as the decimal mark.
pub fn parse_angle_deg(text: &str) -> Option<f32> {
    let value: f32 = text.trim().replace(',', ".").parse().ok()?;
    value.is_finite().then_some(value)
}

/// A finite scale above zero.
pub fn parse_scale(text: &str) -> Option<f32> {
    let value = parse_angle_deg(text)?;
    (value > 0.0).then_some(value)
}

impl HatchSettings {
    pub fn angle_error(&self) -> bool {
        parse_angle_deg(&self.angle).is_none()
    }

    pub fn scale_error(&self) -> bool {
        parse_scale(&self.scale).is_none()
    }

    pub fn gradient_angle_error(&self) -> bool {
        self.gradient.angle_error()
    }

    /// `None` while a field of the active tab is unusable (on the Hatch tab,
    /// also when the pattern is not in the catalog). The other tab's fields
    /// never block.
    pub fn resolve(&self) -> Option<ResolvedSettings> {
        let fill = match self.tab {
            FillTab::Hatch => {
                crate::scene::model::hatch_patterns::find(&self.pattern)?;
                ResolvedFill::Hatch {
                    pattern: self.pattern.clone(),
                    angle_rad: parse_angle_deg(&self.angle)?.to_radians(),
                    scale: parse_scale(&self.scale)?,
                    color: self.color,
                }
            }
            FillTab::Gradient => ResolvedFill::Gradient(self.gradient.spec()?),
        };
        Some(ResolvedSettings {
            associative: self.associative,
            separate: self.separate,
            retain: self.retain,
            island_style: self.effective_island_style(),
            fill,
        })
    }

    /// The engine keeps "retain boundaries" and "separate hatches" apart.
    pub fn set_retain(&mut self, on: bool) {
        self.retain = on;
        if on {
            self.separate = false;
        }
    }

    pub fn set_separate(&mut self, on: bool) {
        self.separate = on;
        if on {
            self.retain = false;
        }
    }

    /// Island detection off draws through the islands, which is the Ignore style.
    pub fn effective_island_style(&self) -> HatchStyleType {
        if self.island_detection {
            self.island_style
        } else {
            HatchStyleType::Ignore
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::types::Color as AcadColor;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> HatchRing {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    fn region(rings: Vec<HatchRing>) -> HatchRegion {
        HatchRegion { rings }
    }

    #[test]
    fn same_ring_with_other_start_order_and_orientation_is_one_region() {
        let a = region(vec![rect(0.0, 0.0, 10.0, 5.0)]);
        // Same square, starting at another vertex.
        let rotated = region(vec![vec![[10.0, 5.0], [0.0, 5.0], [0.0, 0.0], [10.0, 0.0]]]);
        // Same square, reversed (clockwise) and closed with a repeated vertex.
        let reversed = region(vec![vec![
            [0.0, 0.0],
            [0.0, 5.0],
            [10.0, 5.0],
            [10.0, 0.0],
            [0.0, 0.0],
        ]]);
        assert_eq!(region_key(&a), region_key(&rotated));
        assert_eq!(region_key(&a), region_key(&reversed));
    }

    #[test]
    fn pinched_ring_is_one_region_whichever_loop_is_traced_first() {
        // Two loops touching at A = (0, 0): the smallest vertex appears twice.
        // Both loops run counter-clockwise, the first one enclosing more area.
        let (a, b, c) = ([0.0, 0.0], [8.0, 0.0], [8.0, 8.0]);
        let (d, e) = ([2.0, -2.0], [2.0, 0.0]);
        let first = region(vec![vec![a, b, c, a, d, e]]);
        let second = region(vec![vec![a, d, e, a, b, c]]);
        assert_eq!(region_key(&first), region_key(&second));
    }

    #[test]
    fn genuinely_different_pinched_rings_are_different_regions() {
        let (a, b, c) = ([0.0, 0.0], [8.0, 0.0], [8.0, 8.0]);
        let (d, e) = ([2.0, -2.0], [2.0, 0.0]);
        let one = region(vec![vec![a, b, c, a, d, e]]);
        // Same vertices, but the second loop is walked the other way round.
        let other = region(vec![vec![a, b, c, a, e, d]]);
        assert_ne!(region_key(&one), region_key(&other));
    }

    #[test]
    fn holes_are_compared_as_a_set() {
        let hole_a = rect(2.0, 2.0, 3.0, 3.0);
        let hole_b = rect(6.0, 2.0, 7.0, 3.0);
        let outer = rect(0.0, 0.0, 10.0, 5.0);
        let one = region(vec![outer.clone(), hole_a.clone(), hole_b.clone()]);
        let swapped = region(vec![outer.clone(), hole_b, hole_a.clone()]);
        let fewer = region(vec![outer, hole_a]);
        assert_eq!(region_key(&one), region_key(&swapped));
        assert_ne!(region_key(&one), region_key(&fewer));
    }

    #[test]
    fn different_outer_rings_are_different_regions() {
        let a = region(vec![rect(0.0, 0.0, 10.0, 5.0)]);
        let b = region(vec![rect(0.0, 0.0, 10.0, 6.0)]);
        assert_ne!(region_key(&a), region_key(&b));
    }

    #[test]
    fn add_region_skips_equivalents_even_across_origins() {
        let mut list = Vec::new();
        assert!(add_region(
            &mut list,
            region(vec![rect(0.0, 0.0, 10.0, 5.0)]),
            RegionOrigin::Points
        ));
        // The same contour arrives from "Select objects", starting elsewhere.
        assert!(!add_region(
            &mut list,
            region(vec![vec![[10.0, 5.0], [0.0, 5.0], [0.0, 0.0], [10.0, 0.0]]]),
            RegionOrigin::Objects
        ));
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].1, RegionOrigin::Points);
    }

    #[test]
    fn add_region_ignores_empty_regions() {
        let mut list = Vec::new();
        assert!(!add_region(&mut list, region(Vec::new()), RegionOrigin::Points));
        assert!(list.is_empty());
    }

    #[test]
    fn utm_scale_coordinates_keep_equal_regions_equal() {
        let (x, y) = (5_000_000.123_456, 4_640_000.654_321);
        let a = region(vec![rect(x, y, x + 10.0, y + 5.0)]);
        let b = region(vec![vec![
            [x + 10.0, y + 5.0],
            [x, y + 5.0],
            [x, y],
            [x + 10.0, y],
        ]]);
        assert_eq!(region_key(&a), region_key(&b));
        let c = region(vec![rect(x + 0.001, y, x + 10.001, y + 5.0)]);
        assert_ne!(region_key(&a), region_key(&c), "1 mm apart is a different region");
    }

    #[test]
    fn angle_and_scale_accept_comma_and_dot() {
        assert_eq!(parse_angle_deg("45"), Some(45.0));
        assert_eq!(parse_angle_deg(" -12,5 "), Some(-12.5));
        assert_eq!(parse_scale("0,5"), Some(0.5));
        assert_eq!(parse_scale("2.25"), Some(2.25));
    }

    #[test]
    fn angle_and_scale_reject_unusable_text() {
        for bad in ["", " ", "abc", "1,2,3", "NaN", "inf", "1e40", "-"] {
            assert_eq!(parse_angle_deg(bad), None, "angle {bad:?}");
            assert_eq!(parse_scale(bad), None, "scale {bad:?}");
        }
        assert_eq!(parse_scale("0"), None);
        assert_eq!(parse_scale("-1"), None);
        // A negative angle is fine; a negative scale is not.
        assert_eq!(parse_angle_deg("-90"), Some(-90.0));
    }

    #[test]
    fn default_settings_resolve() {
        let resolved = HatchSettings::default().resolve().expect("defaults are valid");
        let ResolvedFill::Hatch { pattern, angle_rad, scale, color } = &resolved.fill else {
            panic!("the Hatch tab resolves to a hatch fill")
        };
        assert_eq!(pattern, "ANSI31");
        assert_eq!(*angle_rad, 0.0);
        assert_eq!(*scale, 1.0);
        assert_eq!(*color, HatchColor::UseCurrent);
        assert!(resolved.associative && !resolved.separate && !resolved.retain);
        assert_eq!(resolved.island_style, HatchStyleType::Normal);
    }

    #[test]
    fn a_new_settings_value_starts_on_the_hatch_tab_with_the_current_colour() {
        let s = HatchSettings::default();
        assert_eq!(s.tab, FillTab::Hatch);
        assert_eq!(s.color, HatchColor::UseCurrent);
        assert_eq!(s.gradient, GradientSettings::default());
        let g = GradientSettings::default();
        assert_eq!((g.shape, g.one_color, g.centered), (0, false, true));
        assert_eq!(g.angle, "0");
        assert_eq!(g.tint, 1.0);
        assert_eq!(g.color1, crate::entities::hatch_fill::DEFAULT_GRADIENT_COLOR1);
        assert_eq!(g.color2, crate::entities::hatch_fill::DEFAULT_GRADIENT_COLOR2);
    }

    #[test]
    fn resolve_follows_the_tab() {
        // A pattern that left the catalog blocks the Hatch tab only.
        let mut s = HatchSettings {
            pattern: "NO_SUCH_PATTERN".into(),
            ..HatchSettings::default()
        };
        assert!(s.resolve().is_none());
        s.tab = FillTab::Gradient;
        assert!(matches!(s.resolve().unwrap().fill, ResolvedFill::Gradient(_)));
        // A bad gradient angle blocks the Gradient tab and not the Hatch tab.
        s.gradient.angle = "x".into();
        assert!(s.resolve().is_none());
        assert!(s.gradient_angle_error());
        s.tab = FillTab::Hatch;
        s.pattern = "ANSI31".into();
        assert!(s.resolve().is_some());
        // A bad pattern angle does not block the Gradient tab.
        s.tab = FillTab::Gradient;
        s.gradient.angle = "15".into();
        s.angle = "x".into();
        assert!(s.resolve().is_some());
    }

    #[test]
    fn the_hatch_tab_resolves_to_pattern_angle_scale_and_colour() {
        let s = HatchSettings {
            angle: "30".into(),
            scale: "2".into(),
            color: HatchColor::Color(AcadColor::Index(1)),
            ..HatchSettings::default()
        };
        match s.resolve().unwrap().fill {
            ResolvedFill::Hatch { pattern, angle_rad, scale, color } => {
                assert_eq!(pattern, "ANSI31");
                assert!((angle_rad - 30f32.to_radians()).abs() < 1e-6);
                assert_eq!(scale, 2.0);
                assert_eq!(color, HatchColor::Color(AcadColor::Index(1)));
            }
            other => panic!("expected the hatch fill, got {other:?}"),
        }
    }

    #[test]
    fn the_gradient_tab_resolves_to_a_full_spec() {
        use crate::scene::model::hatch_model::GradientKind;
        let mut s = HatchSettings::default();
        s.tab = FillTab::Gradient;
        s.gradient.shape = 2; // CHOICES[2] = inverted cylindrical
        s.gradient.one_color = true;
        s.gradient.tint = 0.4;
        s.gradient.centered = false;
        s.gradient.angle = "90".into();
        s.gradient.color1 = AcadColor::Index(5);
        s.gradient.color2 = AcadColor::Index(6);
        let ResolvedFill::Gradient(spec) = s.resolve().unwrap().fill else {
            panic!("a gradient")
        };
        assert_eq!((spec.kind, spec.invert), (GradientKind::Cylinder, true));
        assert!(spec.one_color && !spec.centered);
        assert!((spec.tint - 0.4).abs() < 1e-6);
        assert!((spec.angle_rad - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        assert_eq!((spec.color1, spec.color2), (AcadColor::Index(5), AcadColor::Index(6)));
    }

    #[test]
    fn an_out_of_range_shape_is_held_to_the_last_choice() {
        let g = GradientSettings { shape: 99, ..GradientSettings::default() };
        assert_eq!(
            g.kind_invert(),
            *crate::scene::model::hatch_model::GradientKind::CHOICES.last().unwrap()
        );
    }

    #[test]
    fn the_tint_that_reaches_the_spec_is_always_finite_and_in_range() {
        for (tint, expected) in [
            (f32::NAN, 1.0),
            (f32::INFINITY, 1.0),
            (f32::NEG_INFINITY, 1.0),
            (-3.0, 0.0),
            (7.0, 1.0),
            (0.25, 0.25),
        ] {
            let g = GradientSettings { tint, ..GradientSettings::default() };
            assert_eq!(g.spec().unwrap().tint, expected, "tint {tint}");
        }
    }

    #[test]
    fn unknown_pattern_does_not_resolve() {
        let settings = HatchSettings {
            pattern: "NO_SUCH_PATTERN".into(),
            ..HatchSettings::default()
        };
        assert!(settings.resolve().is_none());
    }

    #[test]
    fn invalid_fields_do_not_resolve_and_are_flagged() {
        let bad_angle = HatchSettings {
            angle: "x".into(),
            ..HatchSettings::default()
        };
        assert!(bad_angle.angle_error() && !bad_angle.scale_error());
        assert!(bad_angle.resolve().is_none());
        let bad_scale = HatchSettings {
            scale: "0".into(),
            ..HatchSettings::default()
        };
        assert!(bad_scale.scale_error() && !bad_scale.angle_error());
        assert!(bad_scale.resolve().is_none());
    }

    #[test]
    fn retain_and_separate_exclude_each_other() {
        let mut settings = HatchSettings::default();
        settings.set_separate(true);
        assert!(settings.separate && !settings.retain);
        settings.set_retain(true);
        assert!(settings.retain && !settings.separate);
        settings.set_separate(true);
        assert!(settings.separate && !settings.retain);
        settings.set_separate(false);
        assert!(!settings.separate && !settings.retain);
    }

    #[test]
    fn island_detection_off_means_ignore() {
        let mut settings = HatchSettings {
            island_style: HatchStyleType::Outer,
            ..HatchSettings::default()
        };
        assert_eq!(settings.effective_island_style(), HatchStyleType::Outer);
        settings.island_detection = false;
        assert_eq!(settings.effective_island_style(), HatchStyleType::Ignore);
        // The radio choice survives the toggle.
        settings.island_detection = true;
        assert_eq!(settings.effective_island_style(), HatchStyleType::Outer);
    }
}
