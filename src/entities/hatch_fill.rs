//! Shared fill logic for hatch entities: what kind of fill a hatch has and
//! the one place that writes a gradient into one. Pure: only `&mut Hatch`,
//! no app, scene or GPU. The HATCH window, the Properties panel and
//! `Scene::add_hatch` all write gradients through here, so a gradient can
//! never be persisted two different ways.

use codec::entities::hatch::GradientColorEntry;
use codec::entities::{Hatch, HatchPatternType, HatchStyleType};
use codec::types::Color as AcadColor;

use crate::scene::model::hatch_model::GradientKind;
use crate::scene::model::hatch_patterns::{self, PatternEntry};

/// What kind of fill a stored hatch has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillKind {
    Pattern,
    Solid,
    Gradient,
}

impl FillKind {
    pub fn of(hatch: &Hatch) -> Self {
        if hatch.gradient_color.enabled {
            Self::Gradient
        } else if hatch.is_solid || hatch.pattern.name.eq_ignore_ascii_case("SOLID") {
            Self::Solid
        } else {
            Self::Pattern
        }
    }
}

/// Starting colours of a new gradient: the ones the GRADIENT command has
/// always used, so a default gradient does not change look.
pub const DEFAULT_GRADIENT_COLOR1: AcadColor = AcadColor::Rgb { r: 77, g: 153, b: 242 };
pub const DEFAULT_GRADIENT_COLOR2: AcadColor = AcadColor::Rgb { r: 46, g: 46, b: 46 };

/// A whole gradient, as the dialog describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct GradientSpec {
    pub kind: GradientKind,
    pub invert: bool,
    pub one_color: bool,
    pub color1: AcadColor,
    /// Only meaningful with two colours; see [`GradientSpec::effective_color2`].
    pub color2: AcadColor,
    /// 0 = black end, 1 = white end; only meaningful with one colour.
    pub tint: f64,
    pub angle_rad: f64,
    pub centered: bool,
}

impl GradientSpec {
    /// The second colour the gradient really ends in: `color2`, or with one
    /// colour the tint of `color1`. The single place that decides it, so the
    /// preview, the stored stop, the swatch and the rebuilt model agree.
    pub fn effective_color2(&self) -> AcadColor {
        if !self.one_color {
            return self.color2;
        }
        let base = rgba_of(self.color1).unwrap_or([1.0; 4]);
        tinted_second_color(base, self.tint as f32)
    }

    /// The render pattern for this gradient and its first colour, as the HATCH
    /// command and the swatch build them. The second colour is the effective
    /// one, never the raw `color2`.
    pub fn model_pattern(&self) -> (crate::scene::model::hatch_model::HatchPattern, [f32; 4]) {
        use crate::scene::model::hatch_model::HatchPattern;
        let first = rgba_of(self.color1).unwrap_or([1.0; 4]);
        let second = rgba_of(self.effective_color2()).unwrap_or([1.0; 4]);
        (
            HatchPattern::Gradient {
                angle_deg: self.angle_rad.to_degrees() as f32,
                color2: second,
                kind: self.kind,
                invert: self.invert,
                shift: if self.centered { 0.0 } else { 1.0 },
                one_color: self.one_color,
                tint: self.tint as f32,
            },
            first,
        )
    }
}

/// Some fields of a gradient; `None` means "leave as it is".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradientPatch {
    pub kind: Option<(GradientKind, bool)>,
    pub one_color: Option<bool>,
    pub color1: Option<AcadColor>,
    pub color2: Option<AcadColor>,
    pub tint: Option<f64>,
    pub angle_rad: Option<f64>,
    pub centered: Option<bool>,
}

impl GradientPatch {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

pub fn rgba_of(color: AcadColor) -> Option<[f32; 4]> {
    color
        .rgb()
        .map(|(r, g, b)| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
}

/// Preserve the selected hue while moving its HSL lightness towards the
/// persisted one-colour tint/shade target (0 = black, 1 = white).
pub fn gradient_tint_color(base: [f32; 4], target: f32) -> [f32; 4] {
    let max = base[0].max(base[1]).max(base[2]);
    let min = base[0].min(base[1]).min(base[2]);
    let lightness = (max + min) * 0.5;
    let target = if target.is_nan() {
        lightness
    } else {
        target.clamp(0.0, 1.0)
    };
    let mut result = base;
    if target <= lightness {
        let factor = if lightness > 1.0e-6 {
            target / lightness
        } else {
            0.0
        };
        for channel in &mut result[..3] {
            *channel *= factor;
        }
    } else {
        let factor = if lightness < 1.0 - 1.0e-6 {
            (target - lightness) / (1.0 - lightness)
        } else {
            1.0
        };
        for channel in &mut result[..3] {
            *channel += (1.0 - *channel) * factor;
        }
    }
    result
}

/// [`gradient_tint_color`] rounded to the 8-bit channels a stop stores, so
/// every consumer sees exactly the same second colour.
pub fn tinted_second_color(base: [f32; 4], tint: f32) -> AcadColor {
    let tinted = gradient_tint_color(base, tint);
    let byte = |value: f32| (value * 255.0).round().clamp(0.0, 255.0) as u8;
    AcadColor::Rgb {
        r: byte(tinted[0]),
        g: byte(tinted[1]),
        b: byte(tinted[2]),
    }
}

/// The shader's shape curve (`hatch_texture.wgsl`): how far from colour 1 to
/// colour 2 a point at fraction `t` is. For the radial kinds `t` is the
/// distance from the centre over the radius. The GPU has its own copy; this
/// one serves the swatch and the tests.
pub fn gradient_profile(kind: GradientKind, invert: bool, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let shaped = match kind {
        GradientKind::Cylinder => 1.0 - (2.0 * t - 1.0).abs(),
        GradientKind::Curved => t * t,
        GradientKind::Hemispherical => t.sqrt(),
        GradientKind::Linear | GradientKind::Spherical => t,
    };
    if invert {
        1.0 - shaped
    } else {
        shaped
    }
}

/// The two stops of a stored gradient, in the order of colour 1 / colour 2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStops {
    pub color1: Option<AcadColor>,
    pub color2: Option<AcadColor>,
    /// The stops were written swapped for an inverted Linear one-colour
    /// gradient (see [`apply_gradient`]); `color1`/`color2` are already put
    /// back in order and the gradient is inverted.
    pub swapped: bool,
}

/// Read the stops of `hatch`. Only for a one-colour gradient named as a plain
/// Linear one, it recognises the swap [`apply_gradient`] writes for an inverted
/// Linear: stop 0 is the tint of stop 1. Anything else reads as stored (a
/// file from another program recomputes colour 2 from stop 0 as before).
pub fn read_stops(hatch: &Hatch) -> GradientStops {
    let g = &hatch.gradient_color;
    let stop = |index: usize| g.colors.get(index).map(|entry| entry.color);
    let (first, second) = (stop(0), stop(1));
    let plain_linear = GradientKind::from_name(&g.name) == (GradientKind::Linear, false);
    if g.is_single_color && plain_linear {
        if let (Some(a), Some(b)) = (first, second) {
            let tint = g.color_tint.clamp(0.0, 1.0) as f32;
            if a != b && rgba_of(b).is_some_and(|base| tinted_second_color(base, tint) == a) {
                return GradientStops {
                    color1: Some(b),
                    color2: Some(a),
                    swapped: true,
                };
            }
        }
    }
    GradientStops {
        color1: first,
        color2: second,
        swapped: false,
    }
}

/// The gradient a stored hatch holds, with defaults for whatever a file from
/// another program left out (stops, name, tint).
pub fn read_gradient(hatch: &Hatch) -> GradientSpec {
    let g = &hatch.gradient_color;
    let (kind, mut invert) = GradientKind::from_name(&g.name);
    let stops = read_stops(hatch);
    invert |= stops.swapped;
    GradientSpec {
        kind,
        invert,
        one_color: g.is_single_color,
        color1: stops.color1.unwrap_or(DEFAULT_GRADIENT_COLOR1),
        color2: stops.color2.unwrap_or(DEFAULT_GRADIENT_COLOR2),
        tint: g.color_tint.clamp(0.0, 1.0),
        angle_rad: g.angle,
        centered: g.shift < 0.5,
    }
}

fn stop_entry(value: f64, color: AcadColor) -> GradientColorEntry {
    GradientColorEntry { value, color }
}

/// Write `spec` as the hatch's whole fill: the canonical, complete form used
/// when a gradient is created or a hatch becomes one.
pub fn apply_gradient(hatch: &mut Hatch, spec: &GradientSpec) {
    if !hatch.gradient_color.enabled {
        hatch.pattern = codec::entities::hatch::HatchPattern::solid();
    }
    hatch.is_solid = true;
    hatch.pattern_angle = spec.angle_rad;
    let (first, second) = (spec.color1, spec.effective_color2());
    // Linear has no INV name in the standard set: an inverted linear is
    // persisted by swapping the stops instead.
    let (first, second) = if spec.invert && spec.kind == GradientKind::Linear {
        (second, first)
    } else {
        (first, second)
    };
    let g = &mut hatch.gradient_color;
    g.enabled = true;
    g.name = spec.kind.dxf_name(spec.invert).to_string();
    g.angle = spec.angle_rad;
    g.shift = if spec.centered { 0.0 } else { 1.0 };
    g.is_single_color = spec.one_color;
    g.color_tint = spec.tint.clamp(0.0, 1.0);
    g.colors = vec![stop_entry(0.0, first), stop_entry(1.0, second)];
}

/// Change only the fields `patch` names, and the physical fields that depend
/// on them. Never converts: a hatch that is not a gradient is left alone.
pub fn apply_gradient_patch(hatch: &mut Hatch, patch: &GradientPatch) {
    if !hatch.gradient_color.enabled {
        return;
    }
    if let Some(angle) = patch.angle_rad {
        hatch.pattern_angle = angle;
    }
    let g = &mut hatch.gradient_color;
    if let Some((kind, invert)) = patch.kind {
        g.name = kind.dxf_name(invert).to_string();
    }
    if let Some(one_color) = patch.one_color {
        if one_color && !g.is_single_color && patch.tint.is_none() {
            g.color_tint = 1.0;
        }
        g.is_single_color = one_color;
    }
    if let Some(tint) = patch.tint {
        g.color_tint = tint.clamp(0.0, 1.0);
    }
    for (index, color) in [(0usize, patch.color1), (1usize, patch.color2)] {
        let Some(color) = color else { continue };
        while g.colors.len() <= index {
            let value = if g.colors.is_empty() { 0.0 } else { 1.0 };
            g.colors.push(stop_entry(value, AcadColor::Index(7)));
        }
        g.colors[index].color = color;
    }
    if let Some(angle) = patch.angle_rad {
        g.angle = angle;
    }
    if let Some(centered) = patch.centered {
        g.shift = if centered { 0.0 } else { 1.0 };
    }
}

/// Put a catalog pattern (or SOLID) into the hatch as its fill: the stored
/// lines are final world geometry, so they are scaled, rotated and moved to
/// the current pattern origin. A gradient the hatch had is cleared.
/// `pattern_scale` and `pattern_angle` are the caller's to set.
pub fn set_catalog_pattern(h: &mut Hatch, entry: &PatternEntry, scale: f64, angle: f64) {
    let mut pattern = hatch_patterns::build_dxf_pattern(entry);
    crate::entities::hatch::scale_pattern_geometry(&mut pattern, scale);
    crate::entities::hatch::rotate_pattern_geometry(&mut pattern, angle);
    let origin = h.pattern_origin();
    crate::entities::hatch::translate_pattern_geometry(&mut pattern, origin.x, origin.y);
    h.pattern = pattern;
    h.is_solid = matches!(
        entry.gpu,
        crate::scene::model::hatch_model::HatchPattern::Solid
    );
    h.pattern_type = HatchPatternType::Predefined;
    h.gradient_color = codec::entities::hatch::HatchGradientPattern::new();
}

/// Origin, association and island style: the part of an update that is the
/// same whatever the fill is.
pub fn apply_common_update(
    h: &mut Hatch,
    origin: Option<(f64, f64)>,
    disassociate: bool,
    style: Option<HatchStyleType>,
) {
    if let Some((x, y)) = origin {
        h.set_pattern_origin(codec::types::Vector2::new(x, y));
    }
    if disassociate {
        for path in &mut h.paths {
            path.boundary_handles.clear();
            path.flags.set_external(false);
        }
        h.is_associative = false;
    }
    if let Some(style) = style {
        h.style = style;
    }
}

/// HATCHEDIT's update of one hatch. A scale or angle left alone comes back as
/// the stored value rounded to f32 (HATCHEDIT and the Hatch Edit window read
/// it that way): the stored value is kept then, so the pattern does not drift.
pub fn apply_pattern_update(
    h: &mut Hatch,
    name: &str,
    scale: f32,
    angle: f32,
    origin: Option<(f64, f64)>,
    disassociate: bool,
    style: Option<HatchStyleType>,
) {
    let keep_scale = h.pattern_scale >= 1.0e-6 && scale == h.pattern_scale as f32;
    let keep_angle = angle == h.pattern_angle.to_degrees() as f32;
    let requested_scale = if keep_scale {
        h.pattern_scale
    } else {
        scale.max(1.0e-6) as f64
    };
    let requested_angle = if keep_angle {
        h.pattern_angle
    } else {
        (angle as f64).to_radians()
    };
    if !name.is_empty() && name != h.pattern.name {
        if let Some(entry) = hatch_patterns::find(name) {
            set_catalog_pattern(h, entry, requested_scale, requested_angle);
        }
    } else {
        if !keep_scale && h.pattern_scale > 1.0e-12 {
            let factor = requested_scale / h.pattern_scale;
            h.scale_pattern_about_origin(factor);
        }
        if !keep_angle {
            let delta = requested_angle - h.pattern_angle;
            h.rotate_pattern_about_origin(delta);
        }
    }
    h.pattern_scale = requested_scale;
    h.pattern_angle = requested_angle;
    apply_common_update(h, origin, disassociate, style);
}

/// What the fill part of a Hatch Edit OK changes.
#[derive(Clone, Debug, PartialEq)]
pub enum FillEdit {
    /// A pattern or solid hatch, edited on the Hatch tab: only the fields
    /// that changed (`None` = keep the stored value).
    Pattern {
        pattern: Option<String>,
        scale: Option<f32>,
        angle_deg: Option<f32>,
    },
    /// A gradient turned into a pattern or a solid (`name == "SOLID"`).
    ToPattern { name: String, scale: f32, angle_deg: f32 },
    /// A gradient edited on the Gradient tab: only the changed fields.
    Gradient(GradientPatch),
    /// A pattern or solid turned into the gradient described.
    ToGradient(GradientSpec),
}

/// Everything one OK of the Hatch Edit window changes in a hatch, applied
/// together so it is one undo step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HatchWindowEdit {
    pub fill: Option<FillEdit>,
    /// The entity's own colour (Hatch tab only).
    pub color: Option<AcadColor>,
    pub style: Option<HatchStyleType>,
    /// The hatch was associative and "Associative" was switched off.
    pub disassociate: bool,
    /// New pattern origin in the hatch's plane (Hatch tab only).
    pub origin: Option<[f64; 2]>,
}

impl HatchWindowEdit {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Apply one Hatch Edit OK to `h`: the fill (edited in place or converted to
/// another kind), then the parts common to every kind. Boundaries, their
/// links and everything not named in `edit` are left as they are.
pub fn apply_window_edit(h: &mut Hatch, edit: &HatchWindowEdit) {
    let origin = edit.origin.map(|point| (point[0], point[1]));
    match &edit.fill {
        Some(FillEdit::Pattern {
            pattern,
            scale,
            angle_deg,
        }) => {
            // A field left alone is handed over as stored (read through f32,
            // as `apply_pattern_update` expects), so it is kept exactly.
            //
            // A stored scale that is zero, negative or not a number, and an
            // angle that is not a number, are what the window showed as 1 and
            // 0 (`EditTarget::from_hatch`). The update works from those
            // defaults (so no pattern line is scaled by a nonsense factor and
            // a new pattern gets the scale the user saw), and when the pattern
            // stays and the field was left alone the stored value is put back.
            let stored_scale = h.pattern_scale;
            let stored_angle = h.pattern_angle;
            let scale_unusable = !(stored_scale.is_finite() && stored_scale > 0.0);
            let angle_unusable = !stored_angle.is_finite();
            if scale_unusable {
                h.pattern_scale = 1.0;
            }
            if angle_unusable {
                h.pattern_angle = 0.0;
            }
            let name = pattern.clone().unwrap_or_else(|| h.pattern.name.clone());
            let scale_f32 = scale.unwrap_or(h.pattern_scale as f32);
            let angle = angle_deg.unwrap_or(h.pattern_angle.to_degrees() as f32);
            apply_pattern_update(h, &name, scale_f32, angle, origin, edit.disassociate, edit.style);
            if pattern.is_none() {
                if scale.is_none() && scale_unusable {
                    h.pattern_scale = stored_scale;
                }
                if angle_deg.is_none() && angle_unusable {
                    h.pattern_angle = stored_angle;
                }
            }
        }
        Some(FillEdit::ToPattern {
            name,
            scale,
            angle_deg,
        }) => {
            // Always through `set_catalog_pattern`: the gradient's stored name
            // is already "SOLID", so a by-name update would keep the gradient.
            if let Some(entry) = hatch_patterns::find(name) {
                let scale = (*scale as f64).max(1.0e-6);
                let angle = (*angle_deg as f64).to_radians();
                set_catalog_pattern(h, entry, scale, angle);
                h.pattern_scale = scale;
                h.pattern_angle = angle;
            }
            apply_common_update(h, origin, edit.disassociate, edit.style);
        }
        Some(FillEdit::Gradient(patch)) => {
            apply_gradient_patch(h, patch);
            apply_common_update(h, None, edit.disassociate, edit.style);
        }
        Some(FillEdit::ToGradient(spec)) => {
            apply_gradient(h, spec);
            apply_common_update(h, None, edit.disassociate, edit.style);
        }
        None => apply_common_update(h, origin, edit.disassociate, edit.style),
    }
    if let Some(color) = edit.color {
        h.common.color = color;
        h.common.color_name = None;
        h.common.color_book_handle = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::hatch::HatchPattern as CodecPattern;
    use codec::types::Vector2;

    fn spec() -> GradientSpec {
        GradientSpec {
            kind: GradientKind::Cylinder,
            invert: true,
            one_color: false,
            color1: AcadColor::Rgb { r: 10, g: 20, b: 30 },
            color2: AcadColor::Index(1),
            tint: 0.25,
            angle_rad: 0.5,
            centered: false,
        }
    }

    #[test]
    fn model_pattern_carries_the_effective_second_colour_and_the_flags() {
        use crate::scene::model::hatch_model::HatchPattern;
        let two = spec();
        let (pattern, first) = two.model_pattern();
        assert_eq!(first, rgba_of(two.color1).unwrap());
        let HatchPattern::Gradient { color2, one_color, shift, kind, invert, angle_deg, .. } = pattern
        else {
            panic!("a gradient")
        };
        assert_eq!(color2, rgba_of(two.color2).unwrap());
        assert!(!one_color && shift == 1.0 && invert && kind == GradientKind::Cylinder);
        assert!((angle_deg - 0.5_f64.to_degrees() as f32).abs() < 1e-4);

        let one = GradientSpec { one_color: true, centered: true, ..spec() };
        let (pattern, _) = one.model_pattern();
        let HatchPattern::Gradient { color2, one_color, tint, shift, .. } = pattern else {
            panic!("a gradient")
        };
        assert_eq!(color2, rgba_of(one.effective_color2()).unwrap(), "never the raw color2");
        assert_ne!(color2, rgba_of(one.color2).unwrap());
        assert!(one_color && tint == 0.25 && shift == 0.0);
    }

    fn gradient_hatch() -> Hatch {
        let mut hatch = Hatch::solid();
        apply_gradient(&mut hatch, &spec());
        hatch
    }

    fn pattern_hatch() -> Hatch {
        let mut hatch = Hatch::with_pattern(CodecPattern::new("ANSI31"));
        hatch.pattern.lines.push(codec::entities::HatchPatternLine {
            angle: 0.0,
            base_point: Vector2::new(1.0, 2.0),
            offset: Vector2::new(0.0, 3.0),
            dash_lengths: vec![],
        });
        hatch
    }

    #[test]
    fn the_four_kinds_of_fill_are_told_apart() {
        assert_eq!(FillKind::of(&pattern_hatch()), FillKind::Pattern);
        assert_eq!(FillKind::of(&Hatch::solid()), FillKind::Solid);
        // A pattern named SOLID is a solid fill whatever `is_solid` says.
        let mut named = Hatch::with_pattern(CodecPattern::new("solid"));
        named.is_solid = false;
        assert_eq!(FillKind::of(&named), FillKind::Solid);
        // A gradient also carries `is_solid`; the gradient flag wins.
        assert!(gradient_hatch().is_solid);
        assert_eq!(FillKind::of(&gradient_hatch()), FillKind::Gradient);
    }

    #[test]
    fn apply_gradient_writes_every_field() {
        let mut hatch = pattern_hatch();
        apply_gradient(&mut hatch, &spec());
        let g = &hatch.gradient_color;
        assert!(g.enabled);
        assert_eq!(g.name, "INVCYLINDER");
        assert_eq!(g.angle, 0.5);
        assert_eq!(hatch.pattern_angle, 0.5, "both angles stay aligned");
        assert_eq!(g.shift, 1.0, "not centred");
        assert!(!g.is_single_color);
        assert_eq!(g.color_tint, 0.25);
        assert_eq!(g.colors.len(), 2);
        assert_eq!((g.colors[0].value, g.colors[1].value), (0.0, 1.0));
        assert_eq!(g.colors[0].color, spec().color1);
        assert_eq!(g.colors[1].color, spec().color2);
        assert!(hatch.is_solid);
        assert!(hatch.pattern.lines.is_empty(), "the pattern lines are gone");
        assert_eq!(hatch.pattern.name, "SOLID");
    }

    #[test]
    fn an_inverted_linear_swaps_the_stops() {
        let mut hatch = Hatch::solid();
        let linear = GradientSpec {
            kind: GradientKind::Linear,
            invert: true,
            ..spec()
        };
        apply_gradient(&mut hatch, &linear);
        assert_eq!(hatch.gradient_color.name, "LINEAR");
        assert_eq!(hatch.gradient_color.colors[0].color, linear.color2);
        assert_eq!(hatch.gradient_color.colors[1].color, linear.color1);
    }

    #[test]
    fn one_color_writes_the_tinted_second_stop_and_the_tint() {
        let one = GradientSpec {
            one_color: true,
            color1: AcadColor::Rgb { r: 200, g: 100, b: 50 },
            color2: AcadColor::Rgb { r: 1, g: 2, b: 3 }, // hidden: must not be written
            tint: 0.5,
            ..spec()
        };
        let mut hatch = Hatch::solid();
        apply_gradient(&mut hatch, &one);
        let g = &hatch.gradient_color;
        assert!(g.is_single_color);
        assert_eq!(g.color_tint, 0.5);
        assert_eq!(g.colors[1].color, one.effective_color2());
        assert_ne!(g.colors[1].color, one.color2);
    }

    #[test]
    fn the_effective_second_color_follows_the_mode() {
        let two = spec();
        assert_eq!(two.effective_color2(), two.color2);
        let black = GradientSpec {
            one_color: true,
            color1: AcadColor::Rgb { r: 0, g: 0, b: 0 },
            tint: 1.0,
            ..spec()
        };
        assert_eq!(black.effective_color2(), AcadColor::Rgb { r: 255, g: 255, b: 255 });
        let white = GradientSpec {
            color1: AcadColor::Rgb { r: 255, g: 255, b: 255 },
            tint: 0.0,
            ..black.clone()
        };
        assert_eq!(white.effective_color2(), AcadColor::Rgb { r: 0, g: 0, b: 0 });
        // A tint equal to the colour's own lightness leaves it as it is.
        let red = GradientSpec {
            color1: AcadColor::Rgb { r: 255, g: 0, b: 0 },
            tint: 0.5,
            ..black.clone()
        };
        assert_eq!(red.effective_color2(), AcadColor::Rgb { r: 255, g: 0, b: 0 });
    }

    #[test]
    fn the_tint_is_safe_at_the_extremes_for_black_and_white() {
        for base in [[0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]] {
            for tint in [0.0, 1.0, -3.0, 7.0, f32::NAN] {
                let out = gradient_tint_color(base, tint);
                for channel in &out[..3] {
                    assert!(
                        channel.is_finite() && (0.0..=1.0).contains(channel),
                        "{base:?} tint {tint}: {out:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_profile_is_the_shaders_curve() {
        use GradientKind::*;
        assert_eq!(gradient_profile(Linear, false, 0.3), 0.3);
        assert_eq!(gradient_profile(Cylinder, false, 0.0), 0.0);
        assert_eq!(gradient_profile(Cylinder, false, 0.5), 1.0);
        assert_eq!(gradient_profile(Cylinder, false, 1.0), 0.0);
        assert_eq!(gradient_profile(Curved, false, 0.5), 0.25);
        assert_eq!(gradient_profile(Spherical, false, 0.25), 0.25);
        assert_eq!(gradient_profile(Hemispherical, false, 0.25), 0.5);
        assert_eq!(gradient_profile(Linear, true, 0.25), 0.75);
        assert_eq!(gradient_profile(Linear, false, 9.0), 1.0, "clamped");
    }

    #[test]
    fn reading_a_gradient_with_missing_data_gives_defaults_and_never_panics() {
        let mut hatch = Hatch::solid();
        hatch.gradient_color.enabled = true; // no stops, no name, tint 0
        let read = read_gradient(&hatch);
        assert_eq!((read.kind, read.invert), (GradientKind::Linear, false));
        assert_eq!(read.color1, DEFAULT_GRADIENT_COLOR1);
        assert_eq!(read.color2, DEFAULT_GRADIENT_COLOR2);
        assert!(read.centered, "shift 0 reads as centred");
        // One stop only: the second is the default.
        hatch.gradient_color.colors.push(GradientColorEntry {
            value: 0.0,
            color: AcadColor::Index(3),
        });
        let read = read_gradient(&hatch);
        assert_eq!(read.color1, AcadColor::Index(3));
        assert_eq!(read.color2, DEFAULT_GRADIENT_COLOR2);
    }

    #[test]
    fn a_written_gradient_reads_back_the_same() {
        let read = read_gradient(&gradient_hatch());
        assert_eq!(read, spec());
    }

    #[test]
    fn an_inverted_linear_one_colour_gradient_reads_back_with_its_swapped_stops() {
        let written = GradientSpec {
            kind: GradientKind::Linear,
            invert: true,
            one_color: true,
            color1: DEFAULT_GRADIENT_COLOR1,
            ..spec()
        };
        let mut hatch = Hatch::solid();
        apply_gradient(&mut hatch, &written);
        // The stops are written swapped: (tint, colour 1).
        assert_eq!(hatch.gradient_color.colors[1].color, written.color1);
        let read = read_gradient(&hatch);
        assert_eq!(read.color1, written.color1);
        assert!(read.one_color && read.invert);
        assert_eq!(read.kind, GradientKind::Linear);
        assert_eq!(read.effective_color2(), written.effective_color2());
    }

    #[test]
    fn a_one_colour_linear_gradient_from_another_program_is_still_recomputed() {
        let mut hatch = Hatch::solid();
        hatch.gradient_color.enabled = true;
        hatch.gradient_color.name = "LINEAR".into();
        hatch.gradient_color.is_single_color = true;
        hatch.gradient_color.color_tint = 0.25;
        let stops = [AcadColor::Rgb { r: 10, g: 20, b: 30 }, AcadColor::Rgb { r: 200, g: 0, b: 0 }];
        hatch.gradient_color.colors = vec![stop_entry(0.0, stops[0]), stop_entry(1.0, stops[1])];
        let read = read_gradient(&hatch);
        assert_eq!(read.color1, stops[0], "stop 0 is colour 1, as before");
        assert!(!read.invert);
        assert_eq!(
            read.effective_color2(),
            tinted_second_color(rgba_of(stops[0]).unwrap(), 0.25)
        );
    }

    // ── apply_gradient_patch: only the named fields, nothing else ──────────

    fn patched(patch: GradientPatch) -> (Hatch, Hatch) {
        let before = gradient_hatch();
        let mut after = before.clone();
        apply_gradient_patch(&mut after, &patch);
        (before, after)
    }

    #[test]
    fn a_patch_with_only_the_kind_touches_only_the_name() {
        let (mut expected, after) = patched(GradientPatch {
            kind: Some((GradientKind::Spherical, false)),
            ..Default::default()
        });
        expected.gradient_color.name = "SPHERICAL".into();
        assert_eq!(after, expected);
    }

    #[test]
    fn switching_to_one_color_without_a_tint_sets_the_tint_to_one() {
        let (mut expected, after) = patched(GradientPatch {
            one_color: Some(true),
            ..Default::default()
        });
        expected.gradient_color.is_single_color = true;
        expected.gradient_color.color_tint = 1.0;
        assert_eq!(after, expected);
    }

    #[test]
    fn switching_to_one_color_with_a_tint_keeps_that_tint() {
        let (mut expected, after) = patched(GradientPatch {
            one_color: Some(true),
            tint: Some(0.4),
            ..Default::default()
        });
        expected.gradient_color.is_single_color = true;
        expected.gradient_color.color_tint = 0.4;
        assert_eq!(after, expected);
    }

    #[test]
    fn a_tint_is_clamped_and_touches_only_the_tint() {
        let (mut expected, after) = patched(GradientPatch {
            tint: Some(5.0),
            ..Default::default()
        });
        expected.gradient_color.color_tint = 1.0;
        assert_eq!(after, expected);
    }

    #[test]
    fn a_color_patch_replaces_one_stop_and_creates_missing_ones() {
        let (mut expected, after) = patched(GradientPatch {
            color2: Some(AcadColor::Index(5)),
            ..Default::default()
        });
        expected.gradient_color.colors[1].color = AcadColor::Index(5);
        assert_eq!(after, expected);

        let mut bare = Hatch::solid();
        bare.gradient_color.enabled = true;
        apply_gradient_patch(
            &mut bare,
            &GradientPatch {
                color2: Some(AcadColor::Index(5)),
                ..Default::default()
            },
        );
        let colors = &bare.gradient_color.colors;
        assert_eq!(colors.len(), 2);
        assert_eq!((colors[0].value, colors[0].color), (0.0, AcadColor::Index(7)));
        assert_eq!((colors[1].value, colors[1].color), (1.0, AcadColor::Index(5)));
    }

    #[test]
    fn an_angle_patch_moves_both_angles_and_nothing_else() {
        let (mut expected, after) = patched(GradientPatch {
            angle_rad: Some(1.25),
            ..Default::default()
        });
        expected.gradient_color.angle = 1.25;
        expected.pattern_angle = 1.25;
        assert_eq!(after, expected);
    }

    #[test]
    fn a_centered_patch_touches_only_the_shift() {
        let (mut expected, after) = patched(GradientPatch {
            centered: Some(true),
            ..Default::default()
        });
        expected.gradient_color.shift = 0.0;
        assert_eq!(after, expected);
    }

    #[test]
    fn a_patch_never_converts_a_plain_hatch() {
        let mut solid = Hatch::solid();
        let before = solid.clone();
        apply_gradient_patch(
            &mut solid,
            &GradientPatch {
                kind: Some((GradientKind::Curved, false)),
                color1: Some(AcadColor::Index(1)),
                ..Default::default()
            },
        );
        assert_eq!(solid, before);
    }

    #[test]
    fn an_empty_patch_is_empty_and_changes_nothing() {
        assert!(GradientPatch::default().is_empty());
        let (before, after) = patched(GradientPatch::default());
        assert_eq!(before, after);
    }

    // ── set_catalog_pattern / apply_pattern_update ─────────────────────────

    use crate::scene::model::hatch_patterns;

    fn ansi31_hatch(scale: f64, angle: f64) -> Hatch {
        let entry = hatch_patterns::find("ANSI31").expect("catalog has ANSI31");
        let mut hatch = Hatch::solid();
        hatch.pattern_scale = scale;
        hatch.pattern_angle = angle;
        set_catalog_pattern(&mut hatch, entry, scale, angle);
        hatch
    }

    #[test]
    fn a_catalog_pattern_replaces_the_fill_and_clears_any_gradient() {
        let mut hatch = gradient_hatch();
        let entry = hatch_patterns::find("ANSI31").unwrap();
        set_catalog_pattern(&mut hatch, entry, 1.0, 0.0);
        assert!(!hatch.is_solid);
        assert!(!hatch.pattern.lines.is_empty());
        assert_eq!(hatch.gradient_color, codec::entities::hatch::HatchGradientPattern::new());
        assert_eq!(FillKind::of(&hatch), FillKind::Pattern);
    }

    #[test]
    fn the_solid_catalog_entry_makes_a_solid_fill() {
        let mut hatch = gradient_hatch();
        let entry = hatch_patterns::find("SOLID").expect("catalog has SOLID");
        set_catalog_pattern(&mut hatch, entry, 1.0, 0.0);
        assert!(hatch.is_solid);
        assert_eq!(FillKind::of(&hatch), FillKind::Solid);
        assert!(!hatch.gradient_color.enabled);
    }

    #[test]
    fn a_new_pattern_keeps_the_pattern_origin() {
        let mut hatch = ansi31_hatch(1.0, 0.0);
        hatch.set_pattern_origin(Vector2::new(3.0, 4.0));
        let before = hatch.pattern_origin();
        let entry = hatch_patterns::find("ANSI37").expect("catalog has ANSI37");
        set_catalog_pattern(&mut hatch, entry, 1.0, 0.0);
        assert_eq!(hatch.pattern_origin(), before);
    }

    #[test]
    fn an_unchanged_scale_and_angle_are_kept_to_the_last_bit() {
        // 0.1 is not an f32: the value comes back through f32 and must not drift.
        let mut hatch = ansi31_hatch(0.1, 0.3);
        let (scale, angle) = (hatch.pattern_scale as f32, hatch.pattern_angle.to_degrees() as f32);
        let before = hatch.clone();
        apply_pattern_update(&mut hatch, "ANSI31", scale, angle, None, false, None);
        assert_eq!(hatch, before);
    }

    #[test]
    fn a_scale_change_rescales_the_lines_by_the_ratio() {
        let mut hatch = ansi31_hatch(1.0, 0.0);
        let spacing = hatch.pattern.lines[0].offset.length();
        apply_pattern_update(&mut hatch, "ANSI31", 4.0, 0.0, None, false, None);
        assert_eq!(hatch.pattern_scale, 4.0);
        let now = hatch.pattern.lines[0].offset.length();
        assert!((now / spacing - 4.0).abs() < 1.0e-9, "{now} vs {spacing}");
    }

    #[test]
    fn a_name_change_converts_a_solid_to_a_pattern_and_back() {
        let mut hatch = Hatch::solid();
        apply_pattern_update(&mut hatch, "ANSI31", 1.0, 0.0, None, false, None);
        assert_eq!(FillKind::of(&hatch), FillKind::Pattern);
        apply_pattern_update(&mut hatch, "SOLID", 1.0, 0.0, None, false, None);
        assert_eq!(FillKind::of(&hatch), FillKind::Solid);
    }

    #[test]
    fn disassociating_clears_the_source_handles_and_the_flag() {
        let mut hatch = ansi31_hatch(1.0, 0.0);
        let mut path = codec::entities::BoundaryPath::new();
        path.add_boundary_handle(codec::Handle::new(9));
        path.flags.set_external(true);
        hatch.paths.push(path);
        hatch.is_associative = true;
        apply_pattern_update(&mut hatch, "ANSI31", 1.0, 0.0, None, true, None);
        assert!(!hatch.is_associative);
        assert!(hatch.paths[0].boundary_handles.is_empty());
        assert!(!hatch.paths[0].flags.is_external());
    }

    #[test]
    fn style_and_origin_are_the_common_part() {
        let mut hatch = ansi31_hatch(1.0, 0.0);
        apply_common_update(
            &mut hatch,
            Some((5.0, 6.0)),
            false,
            Some(codec::entities::HatchStyleType::Outer),
        );
        assert_eq!(hatch.style, codec::entities::HatchStyleType::Outer);
        assert_eq!(hatch.pattern_origin(), Vector2::new(5.0, 6.0));
    }

    // ── apply_window_edit: one OK of the Hatch Edit window ─────────────────

    /// An associative hatch with an outer path, a hole, links to two source
    /// objects and an island style: everything a conversion must leave alone.
    fn bounded_hatch() -> Hatch {
        let mut hatch = ansi31_hatch(1.0, 0.0);
        for (index, handle) in [(0u64, 11u64), (1, 12)] {
            let mut path = codec::entities::BoundaryPath::new();
            path.add_boundary_handle(codec::Handle::new(handle));
            path.flags.set_external(index == 0);
            hatch.paths.push(path);
        }
        hatch.is_associative = true;
        hatch.style = HatchStyleType::Outer;
        hatch.elevation = 2.5;
        hatch.common.color = AcadColor::Index(3);
        hatch
    }

    fn same_everything_but_the_fill(before: &Hatch, after: &Hatch) {
        assert_eq!(after.paths, before.paths, "boundary paths and their links");
        assert_eq!(after.is_associative, before.is_associative);
        assert_eq!(after.style, before.style);
        assert_eq!(after.elevation, before.elevation);
        assert_eq!(after.normal, before.normal);
        assert_eq!(after.common, before.common, "colour, layer, transparency, handle");
    }

    #[test]
    fn a_pattern_becomes_a_gradient_and_nothing_else_changes() {
        let before = bounded_hatch();
        let mut after = before.clone();
        apply_window_edit(
            &mut after,
            &HatchWindowEdit {
                fill: Some(FillEdit::ToGradient(spec())),
                ..Default::default()
            },
        );
        assert_eq!(FillKind::of(&after), FillKind::Gradient);
        assert_eq!(read_gradient(&after), spec());
        same_everything_but_the_fill(&before, &after);
    }

    #[test]
    fn a_gradient_becomes_a_pattern_a_solid_and_nothing_else_changes() {
        let mut before = bounded_hatch();
        apply_gradient(&mut before, &spec());
        let mut pattern = before.clone();
        apply_window_edit(
            &mut pattern,
            &HatchWindowEdit {
                fill: Some(FillEdit::ToPattern {
                    name: "ANSI31".into(),
                    scale: 2.0,
                    angle_deg: 45.0,
                }),
                ..Default::default()
            },
        );
        assert_eq!(FillKind::of(&pattern), FillKind::Pattern);
        assert_eq!(pattern.pattern_scale, 2.0);
        assert!((pattern.pattern_angle - 45f64.to_radians()).abs() < 1e-12);
        assert!(!pattern.gradient_color.enabled);
        same_everything_but_the_fill(&before, &pattern);

        // Same stored name "SOLID" as the gradient's: still a conversion.
        let mut solid = before.clone();
        apply_window_edit(
            &mut solid,
            &HatchWindowEdit {
                fill: Some(FillEdit::ToPattern {
                    name: "SOLID".into(),
                    scale: 1.0,
                    angle_deg: 0.0,
                }),
                ..Default::default()
            },
        );
        assert_eq!(FillKind::of(&solid), FillKind::Solid);
        assert_eq!(solid.gradient_color, codec::entities::hatch::HatchGradientPattern::new());
        same_everything_but_the_fill(&before, &solid);
    }

    #[test]
    fn a_solid_becomes_a_pattern_and_a_gradient() {
        let mut before = bounded_hatch();
        let entry = hatch_patterns::find("SOLID").unwrap();
        set_catalog_pattern(&mut before, entry, 1.0, 0.0);
        assert_eq!(FillKind::of(&before), FillKind::Solid);
        let mut pattern = before.clone();
        apply_window_edit(
            &mut pattern,
            &HatchWindowEdit {
                fill: Some(FillEdit::Pattern {
                    pattern: Some("ANSI31".into()),
                    scale: None,
                    angle_deg: None,
                }),
                ..Default::default()
            },
        );
        assert_eq!(FillKind::of(&pattern), FillKind::Pattern);
        assert_eq!(pattern.pattern.name, "ANSI31");
        assert_eq!(pattern.pattern_scale, before.pattern_scale, "scale kept");
        same_everything_but_the_fill(&before, &pattern);
        let mut gradient = before.clone();
        apply_window_edit(
            &mut gradient,
            &HatchWindowEdit {
                fill: Some(FillEdit::ToGradient(spec())),
                ..Default::default()
            },
        );
        assert_eq!(FillKind::of(&gradient), FillKind::Gradient);
        assert_eq!(read_gradient(&gradient), spec());
        same_everything_but_the_fill(&before, &gradient);
    }

    #[test]
    fn a_gradient_patch_edit_touches_only_the_patched_field() {
        let mut before = bounded_hatch();
        apply_gradient(&mut before, &spec());
        let mut after = before.clone();
        apply_window_edit(
            &mut after,
            &HatchWindowEdit {
                fill: Some(FillEdit::Gradient(GradientPatch {
                    kind: Some((GradientKind::Spherical, false)),
                    ..Default::default()
                })),
                ..Default::default()
            },
        );
        let mut expected = before.clone();
        expected.gradient_color.name = "SPHERICAL".into();
        assert_eq!(after, expected);
    }

    #[test]
    fn colour_style_and_disassociation_apply_with_or_without_a_fill_change() {
        let before = bounded_hatch();
        let mut after = before.clone();
        apply_window_edit(
            &mut after,
            &HatchWindowEdit {
                color: Some(AcadColor::Index(1)),
                style: Some(HatchStyleType::Ignore),
                disassociate: true,
                origin: Some([4.0, 5.0]),
                ..Default::default()
            },
        );
        assert_eq!(after.common.color, AcadColor::Index(1));
        assert_eq!(after.style, HatchStyleType::Ignore);
        assert!(!after.is_associative);
        assert!(after.paths.iter().all(|p| p.boundary_handles.is_empty()));
        assert_eq!(after.pattern_origin(), Vector2::new(4.0, 5.0));
        // Untouched: layer, linetype, the rest of the pattern.
        assert_eq!(after.common.layer, before.common.layer);
        assert_eq!(after.common.linetype, before.common.linetype);
        assert_eq!(after.pattern_scale, before.pattern_scale);

        // The same with a fill change: a gradient made from it keeps the
        // colour, the style and the disassociation of the same OK.
        let mut converted = before.clone();
        apply_window_edit(
            &mut converted,
            &HatchWindowEdit {
                fill: Some(FillEdit::ToGradient(spec())),
                color: Some(AcadColor::Index(1)),
                style: Some(HatchStyleType::Ignore),
                disassociate: true,
                origin: None,
            },
        );
        assert_eq!(FillKind::of(&converted), FillKind::Gradient);
        assert_eq!(converted.common.color, AcadColor::Index(1));
        assert_eq!(converted.style, HatchStyleType::Ignore);
        assert!(!converted.is_associative);
        assert!(converted.paths.iter().all(|p| p.boundary_handles.is_empty()));
    }

    #[test]
    fn a_gradient_never_takes_an_origin() {
        // A gradient has no pattern origin: one in the edit is not recorded
        // in its XDATA, whether the gradient is new or edited.
        let edited = GradientPatch {
            centered: Some(true),
            ..Default::default()
        };
        for fill in [FillEdit::ToGradient(spec()), FillEdit::Gradient(edited)] {
            let mut before = bounded_hatch();
            if matches!(fill, FillEdit::Gradient(_)) {
                apply_gradient(&mut before, &spec());
            }
            let mut after = before.clone();
            apply_window_edit(
                &mut after,
                &HatchWindowEdit {
                    fill: Some(fill),
                    origin: Some([4.0, 5.0]),
                    ..Default::default()
                },
            );
            assert_eq!(FillKind::of(&after), FillKind::Gradient);
            assert_eq!(after.common, before.common, "XDATA (the origin) untouched");
            assert_eq!(after.pattern_origin(), before.pattern_origin());
        }
    }

    #[test]
    fn an_empty_edit_changes_nothing() {
        assert!(HatchWindowEdit::default().is_empty());
        let before = bounded_hatch();
        let mut after = before.clone();
        apply_window_edit(&mut after, &HatchWindowEdit::default());
        assert_eq!(after, before);
        let mut gradient = bounded_hatch();
        apply_gradient(&mut gradient, &spec());
        let mut after = gradient.clone();
        apply_window_edit(&mut after, &HatchWindowEdit::default());
        assert_eq!(after, gradient);
    }
}
