//! Shared fill logic for hatch entities: what kind of fill a hatch has and
//! the one place that writes a gradient into one. Pure: only `&mut Hatch`,
//! no app, scene or GPU. The HATCH window, the Properties panel and
//! `Scene::add_hatch` all write gradients through here, so a gradient can
//! never be persisted two different ways.

use codec::entities::hatch::GradientColorEntry;
use codec::entities::Hatch;
use codec::types::Color as AcadColor;

use crate::scene::model::hatch_model::GradientKind;

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

/// The gradient a stored hatch holds, with defaults for whatever a file from
/// another program left out (stops, name, tint).
pub fn read_gradient(hatch: &Hatch) -> GradientSpec {
    let g = &hatch.gradient_color;
    let (kind, invert) = GradientKind::from_name(&g.name);
    let stop = |index: usize, default: AcadColor| {
        g.colors
            .get(index)
            .map(|entry| entry.color)
            .unwrap_or(default)
    };
    GradientSpec {
        kind,
        invert,
        one_color: g.is_single_color,
        color1: stop(0, DEFAULT_GRADIENT_COLOR1),
        color2: stop(1, DEFAULT_GRADIENT_COLOR2),
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
}
