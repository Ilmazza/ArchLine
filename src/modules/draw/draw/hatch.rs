// Hatch, gradient, and boundary commands.

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::IconKind;
use crate::scene::model::hatch_model::{HatchModel, HatchPattern, PatFamily};
use crate::scene::model::wire_model::WireModel;
use codec::Handle;
use kernel::geom2d::{
    bounded_faces, contains, ring_nesting_depths, signed_area, Circle, Curve, Line, Tolerance,
};
use glam::DVec3;
use crate::t;
use crate::modules::draw::draw::hatch_settings::{
    add_region, HatchRegion, RegionOrigin, ResolvedFill, ResolvedSettings,
};

// ── Icons ──────────────────────────────────────────────────────────────────

const ICON_HATCH: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/hatch/hatch_lines.svg"
));
const ICON_GRADIENT: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/hatch/hatch_gradient.svg"
));
const ICON_BOUNDARY: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/hatch/hatch_boundary.svg"
));

// ── Dropdown metadata ──────────────────────────────────────────────────────

pub const DROPDOWN_ID: &str = "HATCH";
pub const ICON: IconKind = ICON_HATCH;

pub const DROPDOWN_ITEMS: &[(&str, &str, IconKind)] = &[
    ("HATCH", "Hatch", ICON_HATCH),
    ("GRADIENT", "Gradient", ICON_GRADIENT),
    ("BOUNDARY", "Boundary", ICON_BOUNDARY),
];

// ── Shared mode ────────────────────────────────────────────────────────────

enum Mode {
    /// Primary: click inside a closed shape → boundary auto-detected.
    PickInside,
    /// Fallback: user manually picks polygon vertices (type "S" to enter).
    Manual,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HatchMode {
    PickInside,
    SelectObjects,
    Manual,
}

// ── CPU point-in-polygon (ray casting) ────────────────────────────────────

fn point_in_polygon(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let vi = poly[i];
        let vj = poly[j];
        if (vi[1] > p[1]) != (vj[1] > p[1]) {
            let x_int = (vj[0] - vi[0]) * (p[1] - vi[1]) / (vj[1] - vi[1]) + vi[0];
            if p[0] < x_int {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Shoelace-area magnitude of a polygon. Used to pick the smallest enclosing
/// outline when a click falls inside several nested boundaries.
fn polygon_area(poly: &[[f64; 2]]) -> f64 {
    let n = poly.len();
    if n < 3 {
        return 0.0;
    }
    let origin = poly[0];
    let mut a = 0.0;
    for i in 1..n - 1 {
        let current = poly[i];
        let next = poly[i + 1];
        a += (current[0] - origin[0]) * (next[1] - origin[1])
            - (next[0] - origin[0]) * (current[1] - origin[1]);
    }
    (a * 0.5).abs()
}

/// True when every vertex of `inner` lies inside `outer`. Sufficient to
/// recognise a closed hatch outline as nested inside another for the common
/// rectangle / closed-polyline case.
fn polygon_contains_polygon(outer: &[[f64; 2]], inner: &[[f64; 2]]) -> bool {
    if inner.len() < 3 {
        return false;
    }
    inner.iter().all(|&v| point_in_polygon(v, outer))
}

/// Resolve the innermost clicked ring and its direct holes.
fn resolve_hatch_rings(
    outlines: &[Vec<[f64; 2]>],
    p: [f64; 2],
) -> Option<Vec<Vec<[f64; 2]>>> {
    let mut containing: Vec<(usize, f64)> = outlines
        .iter()
        .enumerate()
        .filter(|(_, o)| point_in_polygon(p, o))
        .map(|(i, o)| (i, polygon_area(o)))
        .collect();
    if containing.is_empty() {
        return None;
    }
    // Innermost (smallest-area) outline containing the point is the fill.
    containing.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let outer_idx = containing[0].0;
    let outer = &outlines[outer_idx];

    let mut rings = vec![outer.clone()];
    for (i, o) in outlines.iter().enumerate() {
        if i == outer_idx {
            continue;
        }
        // Candidate hole: fully nested inside the fill outline. (An outline the
        // click sits in cannot qualify — it would have been the smaller fill.)
        if !polygon_contains_polygon(outer, o) || point_in_polygon(p, o) {
            continue;
        }
        // Only DIRECT children become holes. If another outline sits strictly
        // between `outer` and `o` (inside `outer`, and enclosing `o`), then `o`
        // belongs to that intermediate region's own fill; flagging it here would
        // re-fill it under even-odd once nesting reaches three levels.
        let has_intermediate = outlines.iter().enumerate().any(|(k, x)| {
            k != i
                && k != outer_idx
                && polygon_contains_polygon(outer, x)
                && polygon_contains_polygon(x, o)
        });
        if !has_intermediate {
            rings.push(o.clone());
        }
    }
    Some(rings)
}

/// The areas the chosen boundary objects enclose, one ring each. Open objects
/// that together close an area count; handles that are not boundary sources
/// are ignored.
pub fn object_regions(
    sources: &rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    handles: &[Handle],
) -> Vec<HatchRegion> {
    let mut segments = Vec::new();
    for handle in handles {
        if let Some(source) = sources.get(handle) {
            segments.extend(source.segments.iter().copied());
        }
    }
    bounded_faces(&segments, Tolerance::new(1.0e-6))
        .into_iter()
        .map(|ring| HatchRegion { rings: vec![ring] })
        .collect()
}

/// Pack one or more rings (outer boundary + optional holes) into the Hatch
/// model storage: the `boundary` f32 ring list (NaN-separated) plus the exact
/// `boundary_wcs` (NaN-separated) used for persistence. The first vertex of the
/// first ring anchors the shared origin.
fn pack_rings(rings: &[Vec<[f64; 2]>]) -> (Vec<[f32; 2]>, [f64; 2], Vec<[f64; 2]>) {
    let mut wcs: Vec<[f64; 2]> = Vec::new();
    let mut first = true;
    for ring in rings {
        if !first {
            wcs.push([f64::NAN, f64::NAN]);
        }
        first = false;
        wcs.extend(ring.iter().copied());
    }
    let (rel, origin) = rte_boundary(wcs.iter().map(|&[x, y]| (x, y)));
    (rel, origin, wcs)
}

/// Store boundary points as precise-origin-relative offsets.
fn rte_boundary(pts: impl Iterator<Item = (f64, f64)>) -> (Vec<[f32; 2]>, [f64; 2]) {
    let pts: Vec<(f64, f64)> = pts.collect();
    let Some(&(ox, oy)) = pts.first() else {
        return (vec![], [0.0; 2]);
    };
    let rel = pts
        .iter()
        .map(|&(x, y)| [(x - ox) as f32, (y - oy) as f32])
        .collect();
    (rel, [ox, oy])
}

/// Keep only the rings of a NaN-separated buffer whose entry in `keep` is true.
fn retain_rings(buffer: &[[f32; 2]], keep: &[bool]) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(buffer.len());
    let mut ring_index = 0;
    let mut current: Vec<[f32; 2]> = Vec::new();
    let flush = |ring: &mut Vec<[f32; 2]>, index: usize, out: &mut Vec<[f32; 2]>| {
        if ring.is_empty() {
            return;
        }
        if keep.get(index).copied().unwrap_or(true) {
            if !out.is_empty() {
                out.push([f32::NAN, f32::NAN]);
            }
            out.append(ring);
        } else {
            ring.clear();
        }
    };
    for &point in buffer {
        if point[0].is_nan() || point[1].is_nan() {
            flush(&mut current, ring_index, &mut out);
            ring_index += 1;
        } else {
            current.push(point);
        }
    }
    flush(&mut current, ring_index, &mut out);
    out
}

/// Drop from the *rendered* buffers the rings the island style hides. The three
/// buffers a renderer reads — `boundary`, `fill_plane_boundary` and
/// `boundary_exterior` — are filtered together so no consumer sees a ring
/// another one does not. The persisted data (`boundary_wcs`, `boundary_paths`,
/// `boundary_sources`) is left whole.
fn filter_rendered_rings(model: &mut HatchModel, depths: &[usize]) {
    let keep: Vec<bool> = depths
        .iter()
        .map(|&depth| {
            crate::scene::model::hatch_model::island_ring_kept(model.style, depth)
        })
        .collect();
    if keep.iter().all(|&kept| kept) {
        return;
    }
    model.boundary = std::sync::Arc::new(retain_rings(&model.boundary, &keep));
    if let Some(local) = &model.fill_plane_boundary {
        model.fill_plane_boundary = Some(std::sync::Arc::new(retain_rings(local, &keep)));
    }
    if let Some(exterior) = &model.boundary_exterior {
        let filtered: Vec<bool> = exterior
            .iter()
            .zip(&keep)
            .filter(|(_, &kept)| kept)
            .map(|(&outer, _)| outer)
            .collect();
        model.boundary_exterior = Some(std::sync::Arc::new(filtered));
    }
}

// ── HATCH command ──────────────────────────────────────────────────────────

pub struct HatchCommand {
    outlines: Vec<Vec<[f64; 2]>>,
    boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    point_regions: Vec<Vec<Vec<[f64; 2]>>>,
    object_regions: Vec<Vec<Vec<[f64; 2]>>>,
    selected_objects: Vec<Handle>,
    mode: HatchMode,
    manual_pts: Vec<DVec3>,
    manual_bulges: Vec<f64>,
    manual_arc_mode: bool,
    manual_arc_midpoint: Option<DVec3>,
    missed: bool,
    retain_boundaries: bool,
    pattern_override: Option<(String, HatchPattern)>,
    angle_override: Option<f32>,
    scale_override: Option<f32>,
    default_origin: [f64; 2],
    associative: bool,
    separate_hatches: bool,
    island_style: codec::entities::HatchStyleType,
    inherited: Option<(
        HatchModel,
        codec::types::Color,
        codec::types::Transparency,
    )>,
    plane: WorkingPlane,
    /// The HATCH dialog's collector: Enter hands the regions back instead of
    /// committing a hatch, and the settings keywords belong to the dialog.
    collect_only: bool,
    /// The dialog's Gradient tab; `None` for a pattern or solid hatch. Set by
    /// `with_settings`.
    gradient: Option<crate::entities::hatch_fill::GradientSpec>,
    /// The colour and transparency the hatch is created with (resolved by the
    /// app, "Use Current" included); `None` keeps the defaults.
    creation_style: Option<(codec::types::Color, codec::types::Transparency)>,
    /// The RGBA the preview of a pattern or solid is drawn with.
    preview_color: Option<[f32; 4]>,
}

impl HatchCommand {
    pub fn new(
        outlines: Vec<Vec<[f64; 2]>>,
        boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
        selected_objects: Vec<Handle>,
        inherited: Option<(
            HatchModel,
            codec::types::Color,
            codec::types::Transparency,
        )>,
        plane: WorkingPlane,
    ) -> Self {
        let selected_objects: Vec<_> = selected_objects
            .into_iter()
            .filter(|handle| boundary_sources.contains_key(handle))
            .collect();
        let has_selection = !selected_objects.is_empty();
        let mut command = Self {
            outlines,
            boundary_sources,
            point_regions: Vec::new(),
            object_regions: Vec::new(),
            selected_objects: Vec::new(),
            mode: if has_selection {
                HatchMode::SelectObjects
            } else {
                HatchMode::PickInside
            },
            manual_pts: vec![],
            manual_bulges: vec![],
            manual_arc_mode: false,
            manual_arc_midpoint: None,
            missed: false,
            retain_boundaries: false,
            pattern_override: None,
            angle_override: None,
            scale_override: None,
            default_origin: [0.0, 0.0],
            associative: true,
            separate_hatches: false,
            island_style: inherited
                .as_ref()
                .map(|(model, _, _)| model.style)
                .unwrap_or(codec::entities::HatchStyleType::Normal),
            inherited,
            plane,
            collect_only: false,
            gradient: None,
            creation_style: None,
            preview_color: None,
        };
        command.set_object_selection(selected_objects);
        command
    }

    pub fn with_origin(mut self, origin: [f64; 2]) -> Self { self.default_origin = origin; self }

    /// The colour and transparency the hatch is made with (the app resolves
    /// "Use Current"). `None`: as always, the entity takes the defaults.
    pub fn with_creation_style(
        mut self,
        style: Option<(codec::types::Color, codec::types::Transparency)>,
    ) -> Self {
        self.creation_style = style;
        self
    }

    /// The RGBA the preview of a pattern or solid is drawn with.
    pub fn with_preview_color(mut self, color: Option<[f32; 4]>) -> Self {
        self.preview_color = color;
        self
    }

    /// The style the commit carries: the one chosen at creation, else the one
    /// inherited from the source hatch (`-HATCH` Inherit).
    fn entity_style(&self) -> Option<(codec::types::Color, codec::types::Transparency)> {
        self.creation_style.clone().or_else(|| {
            self.inherited
                .as_ref()
                .map(|(_, color, transparency)| (color.clone(), *transparency))
        })
    }

    fn commit_one(&self, hatch: HatchModel) -> CmdResult {
        match self.entity_style() {
            Some((color, transparency)) => CmdResult::CommitStyledHatch {
                hatch,
                color,
                transparency,
            },
            None => CmdResult::CommitHatch(hatch),
        }
    }

    /// Apply the dialog's settings as the overrides the command line sets with
    /// `P`, `A`, `L`, `N`, `D`, `B` and `Y`.
    pub fn with_settings(mut self, settings: &ResolvedSettings) -> Self {
        match &settings.fill {
            ResolvedFill::Hatch { pattern, angle_rad, scale, .. } => {
                let entry = crate::scene::model::hatch_patterns::find(pattern);
                self.pattern_override = entry.map(|entry| (entry.name.clone(), entry.gpu.clone()));
                self.angle_override = Some(*angle_rad);
                self.scale_override = Some(*scale);
                self.gradient = None;
            }
            ResolvedFill::Gradient(spec) => {
                self.pattern_override = None;
                self.angle_override = None;
                self.scale_override = None;
                self.gradient = Some(spec.clone());
            }
        }
        self.associative = settings.associative;
        self.retain_boundaries = settings.retain;
        self.separate_hatches = settings.separate && !settings.retain;
        self.island_style = settings.island_style;
        self
    }

    /// Replace the collected areas with `regions` (every region keeps its rings
    /// together, which is what separate hatches are made from).
    pub fn with_regions(mut self, regions: Vec<HatchRegion>) -> Self {
        self.point_regions = regions.into_iter().map(|region| region.rings).collect();
        self.object_regions.clear();
        self.selected_objects.clear();
        self
    }

    /// The dialog's collector: picks areas like HATCH, but Enter returns them
    /// (`CmdResult::HatchBoundariesPicked`) and Esc returns without any.
    pub fn collecting(
        outlines: Vec<Vec<[f64; 2]>>,
        boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
        plane: WorkingPlane,
        select_objects: bool,
    ) -> Self {
        let mut command = Self::new(outlines, boundary_sources, Vec::new(), None, plane);
        command.collect_only = true;
        if select_objects {
            command.mode = HatchMode::SelectObjects;
        }
        command
    }

    fn collected(&self) -> CmdResult {
        let mut regions = Vec::new();
        for rings in &self.point_regions {
            add_region(
                &mut regions,
                HatchRegion {
                    rings: rings.clone(),
                },
                RegionOrigin::Points,
            );
        }
        for rings in &self.object_regions {
            add_region(
                &mut regions,
                HatchRegion {
                    rings: rings.clone(),
                },
                RegionOrigin::Objects,
            );
        }
        CmdResult::HatchBoundariesPicked {
            regions,
            objects: self.selected_objects.clone(),
        }
    }

    /// The models the preview draws: one per region when hatches are made
    /// separately, otherwise one for everything, each with the rings the island
    /// style hides taken out of what is rendered.
    pub fn preview_models(&self) -> Vec<HatchModel> {
        let manual_ring = (matches!(self.mode, HatchMode::Manual) && self.manual_pts.len() >= 3)
            .then(|| {
                self.manual_pts
                    .iter()
                    .map(|point| [point.x, point.y])
                    .collect::<Vec<[f64; 2]>>()
            });
        // Manual mode always commits one combined hatch, so it previews one.
        let groups: Vec<Vec<Vec<[f64; 2]>>> = if self.separate_hatches
            && !self.retain_boundaries
            && !matches!(self.mode, HatchMode::Manual)
        {
            let mut regions: Vec<Vec<Vec<[f64; 2]>>> = self
                .point_regions
                .iter()
                .chain(self.object_regions.iter())
                .cloned()
                .collect();
            if let Some(ring) = manual_ring {
                regions.push(vec![ring]);
            }
            regions
        } else {
            let mut rings = self.combined_rings();
            if let Some(ring) = manual_ring {
                rings.push(ring);
            }
            if rings.is_empty() {
                Vec::new()
            } else {
                vec![rings]
            }
        };
        groups
            .into_iter()
            .filter(|rings| !rings.is_empty())
            .map(|rings| {
                let depths = kernel::geom2d::ring_nesting_depths(&rings);
                let mut model = self.make_hatch(rings);
                filter_rendered_rings(&mut model, &depths);
                if let HatchPattern::Gradient { color2, .. } = &mut model.pattern {
                    // The real colours, translucent like every preview.
                    color2[3] = 0.75;
                    model.color[3] = 0.75;
                } else {
                    model.color = self.preview_color.unwrap_or([0.15, 0.55, 1.0, 0.75]);
                }
                model
            })
            .collect()
    }

    fn set_object_selection(&mut self, handles: Vec<Handle>) {
        self.object_regions = object_regions(&self.boundary_sources, &handles)
            .into_iter()
            .map(|region| region.rings)
            .collect();
        self.missed = !handles.is_empty() && self.object_regions.is_empty();
        self.selected_objects = handles;
    }

    fn add_point_region(&mut self, rings: Vec<Vec<[f64; 2]>>) {
        let duplicate = rings.first().is_some_and(|outer| {
            self.point_regions
                .iter()
                .any(|region| region.first() == Some(outer))
        });
        if !duplicate {
            self.point_regions.push(rings);
        }
    }

    fn region_count(&self) -> usize {
        self.point_regions.len() + self.object_regions.len()
    }

    fn island_style_label(&self) -> &'static str {
        match self.island_style {
            codec::entities::HatchStyleType::Normal => "Normal",
            codec::entities::HatchStyleType::Outer => "Outer",
            codec::entities::HatchStyleType::Ignore => "Ignore",
        }
    }

    fn combined_rings(&self) -> Vec<Vec<[f64; 2]>> {
        let mut rings = Vec::new();
        for ring in self
            .point_regions
            .iter()
            .chain(self.object_regions.iter())
            .flat_map(|region| region.iter())
        {
            if !rings.iter().any(|existing| existing == ring) {
                rings.push(ring.clone());
            }
        }
        rings
    }

    fn make_hatch(&self, rings: Vec<Vec<[f64; 2]>>) -> HatchModel {
        let world_rings: Vec<Vec<[f64; 2]>> = rings
            .iter()
            .map(|ring| {
                ring.iter()
                    .map(|&[x, y]| {
                        let point = self.plane.to_world(DVec3::new(x, y, 0.0));
                        [point.x, point.y]
                    })
                    .collect()
            })
            .collect();
        let (rel, origin, wcs) = pack_rings(&world_rings);
        let mut local_boundary = Vec::new();
        for (index, ring) in rings.iter().enumerate() {
            if index != 0 {
                local_boundary.push([f32::NAN, f32::NAN]);
            }
            local_boundary.extend(ring.iter().map(|&[x, y]| [x as f32, y as f32]));
        }
        let fill_plane = crate::scene::model::hatch_model::FillPlane {
            origin: self.plane.origin.to_array(),
            x_axis: self.plane.x.to_array(),
            y_axis: self.plane.y.to_array(),
        };
        let exterior: Vec<bool> = kernel::geom2d::ring_nesting_depths(&rings)
            .into_iter()
            .map(|depth| depth == 0)
            .collect();
        let mut boundary_sources: Vec<Vec<Handle>> = rings
            .iter()
            .map(|ring| crate::scene::ring_source_handles(ring, &self.boundary_sources))
            .collect();
        let mut boundary_paths = crate::scene::exact_hatch_paths(
            &rings,
            &exterior,
            &self.boundary_sources,
            1.0e-6,
        );
        if !self.associative {
            for handles in &mut boundary_sources {
                handles.clear();
            }
            for path in &mut boundary_paths {
                path.boundary_handles.clear();
                path.flags.set_external(false);
            }
        }
        if let Some(spec) = &self.gradient {
            let (pattern, color) = spec.model_pattern();
            return HatchModel {
                pattern_origin: None,
                render_instance: None,
                boundary: std::sync::Arc::new(rel),
                pattern,
                name: spec.kind.dxf_name(spec.invert).to_string(),
                color,
                aci: 0,
                line_weight_px: 1.0,
                angle_offset: spec.angle_rad as f32,
                scale: 1.0,
                world_origin: origin,
                boundary_wcs: Some(std::sync::Arc::new(wcs)),
                fill_plane: Some(fill_plane),
                fill_plane_boundary: Some(std::sync::Arc::new(local_boundary)),
                boundary_exterior: Some(std::sync::Arc::new(exterior)),
                boundary_sources: Some(std::sync::Arc::new(boundary_sources)),
                boundary_paths: Some(std::sync::Arc::new(boundary_paths)),
                style: self.island_style,
                draw_depth: 0.0,
            };
        }
        if let Some((source, _, _)) = &self.inherited {
            let (name, mut pattern) = self
                .pattern_override
                .clone()
                .unwrap_or_else(|| (source.name.clone(), source.pattern.clone()));
            let angle = self.angle_override.unwrap_or(source.angle_offset);
            let scale = self.scale_override.unwrap_or(source.scale).max(1.0e-6);
            if let HatchPattern::Pattern(families) = &mut pattern {
                let (sin, cos) = angle.sin_cos();
                for family in families {
                    let base_x = source.world_origin[0]
                        + (family.x0 as f64 * cos as f64
                            - family.y0 as f64 * sin as f64)
                            * scale as f64;
                    let base_y = source.world_origin[1]
                        + (family.x0 as f64 * sin as f64
                            + family.y0 as f64 * cos as f64)
                            * scale as f64;
                    let dx = base_x - origin[0];
                    let dy = base_y - origin[1];
                    family.x0 = ((dx * cos as f64 + dy * sin as f64) / scale as f64) as f32;
                    family.y0 = ((-dx * sin as f64 + dy * cos as f64) / scale as f64) as f32;
                }
            }
            return HatchModel {
                pattern_origin: source.pattern_origin,
                render_instance: None,
                boundary: std::sync::Arc::new(rel),
                pattern,
                name,
                color: source.color,
                aci: source.aci,
                line_weight_px: source.line_weight_px,
                angle_offset: angle,
                scale,
                world_origin: origin,
                boundary_wcs: Some(std::sync::Arc::new(wcs)),
                fill_plane: Some(fill_plane),
                fill_plane_boundary: Some(std::sync::Arc::new(local_boundary)),
                boundary_exterior: Some(std::sync::Arc::new(exterior)),
                boundary_sources: Some(std::sync::Arc::new(boundary_sources)),
                boundary_paths: Some(std::sync::Arc::new(boundary_paths)),
                style: self.island_style,
                draw_depth: source.draw_depth,
            };
        }
        // Default: ANSI31 from catalog; fallback to a single 45° family.
        let pat_name = "ANSI31";
        let default_pattern = crate::scene::model::hatch_patterns::find(pat_name)
            .and_then(|e| {
                if let HatchPattern::Pattern(f) = &e.gpu {
                    Some(HatchPattern::Pattern(f.clone()))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                // 45° lines, perpendicular spacing ≈ 5 world units.
                let dy = 5.0_f32 / (45.0_f32.to_radians().cos());
                HatchPattern::Pattern(vec![PatFamily {
                    angle_deg: 45.0,
                    x0: 0.0,
                    y0: 0.0,
                    dx: 0.0,
                    dy,
                    dashes: vec![],
                }])
            });
        let (name, mut pattern) = self
            .pattern_override
            .clone()
            .unwrap_or_else(|| (pat_name.to_string(), default_pattern));
        let angle = self.angle_override.unwrap_or(0.0);
        let scale = self.scale_override.unwrap_or(1.0).max(1.0e-6);
        if let (HatchPattern::Pattern(families), Some(anchor)) = (&mut pattern, local_boundary.first()) {
            let (sin, cos) = (angle as f64).sin_cos();
            let dx = self.default_origin[0] - anchor[0] as f64;
            let dy = self.default_origin[1] - anchor[1] as f64;
            for family in families {
                family.x0 += ((dx * cos + dy * sin) / scale as f64) as f32;
                family.y0 += ((-dx * sin + dy * cos) / scale as f64) as f32;
            }
        }
        HatchModel {
            pattern_origin: Some(self.default_origin),
            render_instance: None,
            boundary: std::sync::Arc::new(rel),
            pattern,
            name,
            color: [0.75, 0.75, 0.75, 0.85],
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: self.angle_override.unwrap_or(0.0),
            scale: self.scale_override.unwrap_or(1.0).max(1.0e-6),
            world_origin: origin,
            boundary_wcs: Some(std::sync::Arc::new(wcs)),
            fill_plane: Some(fill_plane),
            fill_plane_boundary: Some(std::sync::Arc::new(local_boundary)),
            boundary_exterior: Some(std::sync::Arc::new(exterior)),
            boundary_sources: Some(std::sync::Arc::new(boundary_sources)),
            boundary_paths: Some(std::sync::Arc::new(boundary_paths)),
            style: self.island_style,
            draw_depth: 0.0,
        }
    }

    fn manual_boundary_path(&self) -> Option<codec::entities::BoundaryPath> {
        use codec::entities::{BoundaryEdge, BoundaryPath, PolylineEdge};
        use codec::types::Vector3;
        if self.manual_pts.len() < 3 {
            return None;
        }
        let vertices = self
            .manual_pts
            .iter()
            .enumerate()
            .map(|(index, point)| {
                Vector3::new(
                    point.x,
                    point.y,
                    self.manual_bulges.get(index).copied().unwrap_or(0.0),
                )
            })
            .collect();
        let mut path = BoundaryPath::new();
        path.add_edge(BoundaryEdge::Polyline(PolylineEdge {
            vertices,
            is_closed: true,
        }));
        Some(path)
    }
}

fn arc_bulge(start: DVec3, middle: DVec3, end: DVec3) -> Option<f64> {
    let curvature = DVec3::from_array(kernel::space::curve::curvature_through(
        start.to_array(),
        middle.to_array(),
        end.to_array(),
    ));
    let squared = curvature.length_squared();
    if squared <= f64::MIN_POSITIVE {
        return None;
    }
    let centre = start + curvature / squared;
    let circle = Curve::Circle(Circle {
        centre: [centre.x, centre.y],
        radius: squared.sqrt().recip(),
    });
    let first = circle.parameter_at([start.x, start.y]);
    let through = (circle.parameter_at([middle.x, middle.y]) - first).rem_euclid(1.0);
    let ccw = (circle.parameter_at([end.x, end.y]) - first).rem_euclid(1.0);
    let sweep = if through <= ccw + 1.0e-12 {
        ccw * std::f64::consts::TAU
    } else {
        (ccw - 1.0) * std::f64::consts::TAU
    };
    Some((sweep * 0.25).tan())
}

impl CadCommand for HatchCommand {
    fn name(&self) -> &'static str {
        "HATCH"
    }

    fn prompt(&self) -> String {
        if self.collect_only && !matches!(self.mode, HatchMode::Manual) {
            let miss = if self.missed {
                t!("  ⚠ No closed boundary found.").into_owned()
            } else {
                String::new()
            };
            return match self.mode {
                HatchMode::SelectObjects => t!(
                    "HATCH  Select boundary objects (%{objects} objects, %{count} regions; Enter to return to the dialog):%{miss}",
                    objects = self.selected_objects.len(),
                    count = self.region_count(),
                    miss = miss
                )
                .into_owned(),
                _ => t!(
                    "HATCH  Pick internal point (%{count} regions; Enter to return to the dialog):%{miss}",
                    count = self.region_count(),
                    miss = miss
                )
                .into_owned(),
            };
        }
        match &self.mode {
            HatchMode::PickInside => {
                let miss = if self.missed {
                    t!("  ⚠ No closed boundary found.").into_owned()
                } else {
                    String::new()
                };
                t!(
                    "HATCH  Pick internal point (%{count} regions selected; P <pattern> / A <angle> / L <scale>; Enter to apply):%{miss}",
                    count = self.region_count(),
                    miss = miss
                )
                .into_owned()
            }
            HatchMode::SelectObjects => {
                let miss = if self.missed {
                    t!("  ⚠ Selection has no closed boundary.").into_owned()
                } else {
                    String::new()
                };
                t!(
                    "HATCH  Select boundary objects (%{objects} objects, %{count} regions; P <pattern> / A <angle> / L <scale>; Enter to apply):%{miss}",
                    objects = self.selected_objects.len(),
                    count = self.region_count(),
                    miss = miss
                )
                .into_owned()
            }
            HatchMode::Manual => {
                if self.manual_pts.is_empty() {
                    t!("HATCH  Boundary point 1:").into_owned()
                } else {
                    let n = self.manual_pts.len() + 1;
                    t!("HATCH  Point %{n}:", n = n).into_owned()
                }
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.collect_only && !matches!(self.mode, HatchMode::Manual) {
            // How areas are chosen is the dialog's Add button's business: a
            // switch here would bypass the selection it saved and restores.
            return vec![CmdOption::enter(t!("Back to dialog").as_ref())];
        }
        match &self.mode {
            HatchMode::PickInside => {
                let mut options = vec![
                    CmdOption::new(t!("Select objects").as_ref(), "O"),
                    CmdOption::new(t!("Draw manually").as_ref(), "S"),
                    CmdOption::new(
                        if self.retain_boundaries {
                            "Keep boundaries: on"
                        } else {
                            "Keep boundaries: off"
                        },
                        "B",
                    ),
                    CmdOption::new(
                        if self.associative { "Associative: on" } else { "Associative: off" },
                        "N",
                    ),
                    CmdOption::new(
                        if self.separate_hatches { "Separate hatches: on" } else { "Separate hatches: off" },
                        "D",
                    ),
                    CmdOption::new(
                        &crate::tf!("Island style: {}", self.island_style_label()),
                        "Y",
                    ),
                ];
                if self.region_count() > 0 {
                    options.push(CmdOption::enter(t!("Accept").as_ref()));
                }
                options
            }
            HatchMode::SelectObjects => {
                let mut options = vec![
                    CmdOption::new(t!("Pick internal points").as_ref(), "I"),
                    CmdOption::new(t!("Draw manually").as_ref(), "S"),
                    CmdOption::new(
                        if self.retain_boundaries {
                            "Keep boundaries: on"
                        } else {
                            "Keep boundaries: off"
                        },
                        "B",
                    ),
                    CmdOption::new(
                        if self.associative { "Associative: on" } else { "Associative: off" },
                        "N",
                    ),
                    CmdOption::new(
                        if self.separate_hatches { "Separate hatches: on" } else { "Separate hatches: off" },
                        "D",
                    ),
                    CmdOption::new(
                        &crate::tf!("Island style: {}", self.island_style_label()),
                        "Y",
                    ),
                ];
                if self.region_count() > 0 {
                    options.push(CmdOption::enter(t!("Accept").as_ref()));
                }
                options
            }
            HatchMode::Manual => {
                let mut options = vec![
                    CmdOption::new(
                        if self.manual_arc_mode { "Line" } else { "Arc" },
                        if self.manual_arc_mode { "L" } else { "A" },
                    ),
                ];
                if self.manual_pts.len() >= 3 {
                    options.push(CmdOption::new("Close", "C"));
                    options.push(CmdOption::enter(t!("Accept").as_ref()));
                }
                options
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let pt = self.plane.to_local(pt);
        match &self.mode {
            HatchMode::PickInside => {
                let xy = [pt.x, pt.y];
                match resolve_hatch_rings(&self.outlines, xy) {
                    Some(rings) => {
                        self.missed = false;
                        self.add_point_region(rings);
                        CmdResult::NeedPoint
                    }
                    None => {
                        self.missed = true;
                        CmdResult::NeedPoint
                    }
                }
            }
            HatchMode::SelectObjects => CmdResult::NeedPoint,
            HatchMode::Manual => {
                if self.manual_pts.is_empty() || !self.manual_arc_mode {
                    if !self.manual_pts.is_empty() {
                        self.manual_bulges.push(0.0);
                    }
                    self.manual_pts.push(pt);
                } else if let Some(middle) = self.manual_arc_midpoint.take() {
                    let start = *self.manual_pts.last().unwrap();
                    self.manual_bulges
                        .push(arc_bulge(start, middle, pt).unwrap_or(0.0));
                    self.manual_pts.push(pt);
                } else {
                    self.manual_arc_midpoint = Some(pt);
                }
                CmdResult::NeedPoint
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if matches!(self.mode, HatchMode::Manual) && self.manual_arc_midpoint.is_some() {
            return CmdResult::NeedPoint;
        }
        if matches!(self.mode, HatchMode::Manual) && self.manual_pts.len() >= 3 {
            let ring = self.manual_pts.iter().map(|p| [p.x, p.y]).collect();
            self.add_point_region(vec![ring]);
        }
        if self.collect_only {
            return self.collected();
        }
        let rings = self.combined_rings();
        if rings.is_empty() {
            CmdResult::Cancel
        } else if matches!(self.mode, HatchMode::Manual) {
            let mut hatch = self.make_hatch(rings);
            if let Some(path) = self.manual_boundary_path() {
                hatch.boundary_paths = Some(std::sync::Arc::new(vec![path]));
            }
            self.commit_one(hatch)
        } else if self.separate_hatches && !self.retain_boundaries {
            let hatches = self
                .point_regions
                .iter()
                .chain(self.object_regions.iter())
                .cloned()
                .map(|region| self.make_hatch(region))
                .collect();
            CmdResult::CommitHatches {
                hatches,
                entity_style: self.entity_style(),
            }
        } else if self.retain_boundaries {
            CmdResult::CommitHatchWithBoundaries {
                hatch: self.make_hatch(rings.clone()),
                boundaries: crate::scene::boundary_entities_from_sources(
                    &rings,
                    self.plane,
                    &self.boundary_sources,
                    1.0e-6,
                ),
                entity_style: self.entity_style(),
            }
        } else {
            self.commit_one(self.make_hatch(rings))
        }
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.mode, HatchMode::SelectObjects)
    }

    fn selection_forces_add(&self) -> bool {
        matches!(self.mode, HatchMode::SelectObjects)
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if matches!(self.mode, HatchMode::SelectObjects) {
            self.set_object_selection(handles);
            CmdResult::NeedPoint
        } else {
            CmdResult::Cancel
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if matches!(self.mode, HatchMode::Manual) {
            if self.manual_arc_midpoint.take().is_some() {
                return Some(CmdResult::NeedPoint);
            }
            if self.manual_pts.pop().is_some() {
                self.manual_bulges.pop();
                return Some(CmdResult::NeedPoint);
            }
        }
        if matches!(self.mode, HatchMode::PickInside) && self.point_regions.pop().is_some() {
            Some(CmdResult::NeedPoint)
        } else {
            None
        }
    }

    fn hatch_preview_models(&self) -> Option<Vec<HatchModel>> {
        Some(self.preview_models())
    }

    fn on_escape(&mut self) -> CmdResult {
        if self.collect_only {
            return CmdResult::Dispatch("HATCH_PICK_CANCELLED".to_string());
        }
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let input = text.trim();
        let upper = input.to_ascii_uppercase();
        // The dialog owns the settings and the way areas are chosen (O / I
        // would bypass the selection it saves and restores), and drawing a
        // boundary by hand (arcs included) is outside what it can carry back,
        // so none of them is offered here.
        if self.collect_only && !matches!(self.mode, HatchMode::Manual) {
            return None;
        }
        if matches!(self.mode, HatchMode::Manual) {
            return match upper.as_str() {
                "A" | "ARC" => {
                    self.manual_arc_mode = true;
                    self.manual_arc_midpoint = None;
                    Some(CmdResult::NeedPoint)
                }
                "L" | "LINE" => {
                    self.manual_arc_mode = false;
                    self.manual_arc_midpoint = None;
                    Some(CmdResult::NeedPoint)
                }
                "C" | "CLOSE" if self.manual_pts.len() >= 3 => Some(self.on_enter()),
                _ => None,
            };
        }
        if upper == "ASSOCIATIVE" {
            self.associative = !self.associative;
            return Some(CmdResult::NeedPoint);
        }
        if let Some(rest) = upper.strip_prefix('P') {
            let name = rest.trim();
            if !name.is_empty() {
                if let Some(entry) = crate::scene::model::hatch_patterns::find(name) {
                    self.pattern_override = Some((entry.name.clone(), entry.gpu.clone()));
                }
            }
            return Some(CmdResult::NeedPoint);
        }
        if let Some(rest) = upper.strip_prefix('A') {
            if let Ok(value) = rest.trim().replace(',', ".").parse::<f32>() {
                self.angle_override = Some(value.to_radians());
            }
            return Some(CmdResult::NeedPoint);
        }
        if let Some(rest) = upper.strip_prefix('L') {
            if let Ok(value) = rest.trim().replace(',', ".").parse::<f32>() {
                if value > 0.0 {
                    self.scale_override = Some(value);
                }
            }
            return Some(CmdResult::NeedPoint);
        }
        match upper.as_str() {
            "O" | "OBJECT" | "OBJECTS" => {
                self.mode = HatchMode::SelectObjects;
                self.missed = false;
                Some(CmdResult::NeedPoint)
            }
            "I" | "INTERNAL" => {
                self.mode = HatchMode::PickInside;
                self.missed = false;
                Some(CmdResult::NeedPoint)
            }
            "S" => {
                self.mode = HatchMode::Manual;
                self.missed = false;
                Some(CmdResult::NeedPoint)
            }
            "B" | "BOUNDARY" | "BOUNDARIES" => {
                self.retain_boundaries = !self.retain_boundaries;
                if self.retain_boundaries {
                    self.separate_hatches = false;
                }
                Some(CmdResult::NeedPoint)
            }
            "N" | "ASSOCIATIVE" => {
                self.associative = !self.associative;
                Some(CmdResult::NeedPoint)
            }
            "D" | "SEPARATE" => {
                self.separate_hatches = !self.separate_hatches;
                if self.separate_hatches {
                    self.retain_boundaries = false;
                }
                Some(CmdResult::NeedPoint)
            }
            "Y" | "ISLAND" => {
                self.island_style = match self.island_style {
                    codec::entities::HatchStyleType::Normal => {
                        codec::entities::HatchStyleType::Outer
                    }
                    codec::entities::HatchStyleType::Outer => {
                        codec::entities::HatchStyleType::Ignore
                    }
                    codec::entities::HatchStyleType::Ignore => {
                        codec::entities::HatchStyleType::Normal
                    }
                };
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> { let pt = pt.as_vec3();
        if let HatchMode::Manual = &self.mode {
            if self.manual_pts.is_empty() {
                return None;
            }
            let mut pts: Vec<[f32; 3]> = self
                .manual_pts
                .iter()
                .map(|&p| self.plane.to_world(p).as_vec3().to_array())
                .collect();
            pts.push(pt.to_array());
            pts.push(self.plane.to_world(self.manual_pts[0]).as_vec3().to_array());
            return Some(WireModel::solid(
                "rubber_band".into(),
                pts,
                WireModel::CYAN,
                false,
            ));
        }
        None
    }
}

// ── GRADIENT command ───────────────────────────────────────────────────────

pub struct GradientCommand {
    outlines: Vec<Vec<[f64; 2]>>,
    boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    mode: Mode,
    manual_pts: Vec<DVec3>,
    missed: bool,
    /// Gradient shape, switchable via the prompt options (#415).
    kind: crate::scene::model::hatch_model::GradientKind,
    /// Swap the two colour stops.
    invert: bool,
}

impl GradientCommand {
    pub fn new(
        outlines: Vec<Vec<[f64; 2]>>,
        boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    ) -> Self {
        Self {
            outlines,
            boundary_sources,
            mode: Mode::PickInside,
            manual_pts: vec![],
            missed: false,
            kind: crate::scene::model::hatch_model::GradientKind::Linear,
            invert: false,
        }
    }

    fn make_hatch(&self, rings: Vec<Vec<[f64; 2]>>) -> HatchModel {
        let (rel, origin, wcs) = pack_rings(&rings);
        let exterior: Vec<bool> = kernel::geom2d::ring_nesting_depths(&rings)
            .into_iter()
            .map(|depth| depth == 0)
            .collect();
        let boundary_sources = rings
            .iter()
            .map(|ring| crate::scene::ring_source_handles(ring, &self.boundary_sources))
            .collect();
        let boundary_paths = crate::scene::exact_hatch_paths(
            &rings,
            &exterior,
            &self.boundary_sources,
            1.0e-6,
        );
        HatchModel {
            pattern_origin: None,
            render_instance: None,
            boundary: std::sync::Arc::new(rel),
            pattern: HatchPattern::Gradient {
                angle_deg: 0.0,
                color2: [0.18, 0.18, 0.18, 0.0],
                kind: self.kind,
                invert: self.invert,
                shift: 0.0,
                one_color: false,
                tint: 1.0,
            },
            name: self.kind.dxf_name(self.invert).into(),
            color: [0.30, 0.60, 0.95, 0.80],
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: 0.0,
            scale: 1.0,
            world_origin: origin,
            boundary_wcs: Some(std::sync::Arc::new(wcs)),
            fill_plane: None,
            fill_plane_boundary: None,
            boundary_exterior: Some(std::sync::Arc::new(exterior)),
            boundary_sources: Some(std::sync::Arc::new(boundary_sources)),
            boundary_paths: Some(std::sync::Arc::new(boundary_paths)),
            style: codec::entities::HatchStyleType::Normal,
            draw_depth: 0.0,
        }
    }
}

impl CadCommand for GradientCommand {
    fn name(&self) -> &'static str {
        "GRADIENT"
    }

    fn prompt(&self) -> String {
        match &self.mode {
            Mode::PickInside => {
                let miss = if self.missed {
                    t!("  ⚠ No closed boundary found.")
                } else {
                    std::borrow::Cow::Borrowed("")
                };
                t!(
                    "GRADIENT (%{kind}%{invert})  Pick internal point:%{miss}",
                    kind = t!(self.kind.choice_label(self.invert)),
                    invert = std::borrow::Cow::Borrowed(""),
                    miss = miss
                )
                .into_owned()
            }
            Mode::Manual => {
                if self.manual_pts.is_empty() {
                    t!("GRADIENT  Boundary point 1:").into_owned()
                } else {
                    t!("GRADIENT  Point %{n}:", n = self.manual_pts.len() + 1).into_owned()
                }
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match &self.mode {
            Mode::PickInside => {
                let mut opts = vec![CmdOption::new("Draw manually", "S")];
                for (kind, inverted) in
                    crate::scene::model::hatch_model::GradientKind::CHOICES
                {
                    if kind != self.kind || inverted != self.invert {
                        let label = kind.choice_label(inverted);
                        opts.push(CmdOption::new(label, label));
                    }
                }
                opts
            }
            Mode::Manual => {
                // Enter accepts the boundary once at least 3 points are picked.
                if self.manual_pts.len() >= 3 {
                    vec![CmdOption::enter("Accept")]
                } else {
                    vec![]
                }
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.mode {
            Mode::PickInside => {
                let xy = [pt.x, pt.y];
                match resolve_hatch_rings(&self.outlines, xy) {
                    Some(rings) => {
                        self.missed = false;
                        return CmdResult::CommitHatch(self.make_hatch(rings));
                    }
                    None => {
                        self.missed = true;
                        CmdResult::NeedPoint
                    }
                }
            }
            Mode::Manual => {
                // Keep the typed/snapped point exact (issue #311).
                self.manual_pts.push(pt);
                CmdResult::NeedPoint
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match &self.mode {
            Mode::PickInside => CmdResult::Cancel,
            Mode::Manual => {
                if self.manual_pts.len() < 3 {
                    return CmdResult::Cancel;
                }
                let wcs = self.manual_pts.iter().map(|p| [p.x, p.y]).collect();
                CmdResult::CommitHatch(self.make_hatch(vec![wcs]))
            }
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.mode, Mode::PickInside)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        if t.eq_ignore_ascii_case("s") {
            self.mode = Mode::Manual;
            self.missed = false;
            return Some(CmdResult::NeedPoint);
        }
        if let Some((kind, inverted)) =
            crate::scene::model::hatch_model::GradientKind::from_choice_label(t)
        {
            self.kind = kind;
            self.invert = inverted;
            return Some(CmdResult::NeedPoint);
        }
        None
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> { let pt = pt.as_vec3();
        if let Mode::Manual = &self.mode {
            if self.manual_pts.is_empty() {
                return None;
            }
            let mut pts: Vec<[f32; 3]> = self
                .manual_pts
                .iter()
                .map(|p| [p.x as f32, p.y as f32, p.z as f32])
                .collect();
            pts.push([pt.x, pt.y, pt.z]);
            pts.push([
                self.manual_pts[0].x as f32,
                self.manual_pts[0].y as f32,
                self.manual_pts[0].z as f32,
            ]);
            return Some(WireModel::solid(
                "rubber_band".into(),
                pts,
                WireModel::CYAN,
                false,
            ));
        }
        None
    }
}

// ── BOUNDARY command ───────────────────────────────────────────────────────

pub struct BoundaryCommand {
    sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    active_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    restrict_sources: bool,
    outlines: Vec<Vec<[f64; 2]>>,
    point_regions: Vec<Vec<Vec<[f64; 2]>>>,
    selected_objects: Vec<Handle>,
    mode: BoundaryMode,
    island_style: BoundaryIslandStyle,
    gap_tolerance: f64,
    plane: WorkingPlane,
    missed: bool,
    output_region: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundaryMode {
    PickInside,
    Advanced,
    ObjectType,
    SelectObjects,
    GapTolerance { return_to_selection: bool },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundaryIslandStyle {
    Normal,
    Outer,
    Ignore,
}

impl BoundaryCommand {
    pub fn new(
        sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
        selected_objects: Vec<Handle>,
        plane: WorkingPlane,
    ) -> Self {
        let outlines = crate::scene::boundary_faces(&sources, 1.0e-6);
        let mut command = Self {
            sources,
            active_sources: rustc_hash::FxHashMap::default(),
            restrict_sources: false,
            outlines,
            point_regions: Vec::new(),
            selected_objects: Vec::new(),
            mode: BoundaryMode::PickInside,
            island_style: BoundaryIslandStyle::Normal,
            gap_tolerance: 1.0e-6,
            plane,
            missed: false,
            output_region: false,
        };
        if !selected_objects.is_empty() {
            command.set_boundary_set(selected_objects);
        }
        command
    }

    fn set_boundary_set(&mut self, handles: Vec<Handle>) {
        let restrict_sources = !handles.is_empty();
        let selected_sources: rustc_hash::FxHashMap<_, _> = handles
            .iter()
            .filter_map(|handle| self.sources.get(handle).cloned().map(|source| (*handle, source)))
            .collect();
        self.active_sources = selected_sources;
        self.restrict_sources = restrict_sources;
        let sources = if self.restrict_sources {
            &self.active_sources
        } else {
            &self.sources
        };
        self.outlines = crate::scene::boundary_faces(sources, self.gap_tolerance);
        self.missed = !handles.is_empty() && self.outlines.is_empty();
        self.selected_objects = handles;
        self.point_regions.clear();
    }

    fn rebuild_faces(&mut self) {
        self.set_boundary_set(self.selected_objects.clone());
    }

    fn region_count(&self) -> usize {
        self.point_regions.len()
    }

    fn island_label(&self) -> &'static str {
        match self.island_style {
            BoundaryIslandStyle::Normal => "Normal",
            BoundaryIslandStyle::Outer => "Outer",
            BoundaryIslandStyle::Ignore => "Ignore",
        }
    }

    fn picked_region(&self, point: [f64; 2]) -> Option<Vec<Vec<[f64; 2]>>> {
        let tolerance = Tolerance::new(self.gap_tolerance);
        let curves = |outline: &Vec<[f64; 2]>| {
            outline
                .iter()
                .copied()
                .zip(outline.iter().copied().cycle().skip(1))
                .take(outline.len())
                .map(|(start, end)| Curve::Line(Line { start, end }))
                .collect::<Vec<_>>()
        };
        let mut containing: Vec<(usize, f64)> = self
            .outlines
            .iter()
            .enumerate()
            .filter(|(_, outline)| contains(&curves(outline), point, tolerance))
            .map(|(index, outline)| (index, signed_area(outline).abs()))
            .collect();
        containing.sort_by(|left, right| left.1.total_cmp(&right.1));
        let outer_index = containing.first()?.0;
        let outer = &self.outlines[outer_index];
        let outer_curves = curves(outer);
        let depths = ring_nesting_depths(&self.outlines);
        let outer_depth = depths.get(outer_index).copied().unwrap_or(0);
        let mut rings = vec![outer.clone()];
        if self.island_style == BoundaryIslandStyle::Ignore {
            return Some(rings);
        }
        for (index, candidate) in self.outlines.iter().enumerate() {
            let Some(seed) = candidate.first().copied() else {
                continue;
            };
            let candidate_curves = curves(candidate);
            let candidate_depth = depths.get(index).copied().unwrap_or(0);
            if index == outer_index
                || candidate_depth <= outer_depth
                || !contains(&outer_curves, seed, tolerance)
                || contains(&candidate_curves, point, tolerance)
            {
                continue;
            }
            if self.island_style == BoundaryIslandStyle::Outer
                && candidate_depth != outer_depth + 1
            {
                continue;
            }
            rings.push(candidate.clone());
        }
        Some(rings)
    }

    fn add_point_region(&mut self, region: Vec<Vec<[f64; 2]>>) {
        if !self.point_regions.iter().any(|existing| existing == &region) {
            self.point_regions.push(region);
        }
    }

    fn make_entities(&self) -> Vec<codec::EntityType> {
        let sources = if self.restrict_sources {
            &self.active_sources
        } else {
            &self.sources
        };
        if self.output_region {
            return self.point_regions.iter().filter_map(|rings| {
                crate::scene::model::presspull_model::boundary_region(
                    sources,
                    rings,
                    self.plane,
                )
            }).collect();
        }
        crate::scene::boundary_polyline_entities(
            &self.point_regions,
            self.plane,
            sources,
            self.gap_tolerance,
        )
    }
}

impl CadCommand for BoundaryCommand {
    fn name(&self) -> &'static str {
        "BOUNDARY"
    }

    fn prompt(&self) -> String {
        let miss = if self.missed {
            t!("  ⚠ No closed boundary found.").into_owned()
        } else {
            String::new()
        };
        match self.mode {
            BoundaryMode::PickInside => {
                format!("BOUNDARY  Specify internal point or [Advanced options]:{miss}")
            }
            BoundaryMode::Advanced=>"BOUNDARY  Enter an option [Object type]:".into(),
            BoundaryMode::ObjectType=>format!("BOUNDARY  Enter type of boundary object [Region/Polyline] <{}>:",if self.output_region{"Region"}else{"Polyline"}),
            BoundaryMode::SelectObjects => {
                t!("%{cmd}  Select objects:", cmd = self.name()).into_owned()
            }
            BoundaryMode::GapTolerance { .. } => format!(
                "BOUNDARY  {} <{}>:",
                t!("Tolerance"),
                self.gap_tolerance
            ),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.mode==BoundaryMode::Advanced {return vec![CmdOption::new("Object type","O")];}
        if self.mode==BoundaryMode::ObjectType {return vec![CmdOption::new("Region","R"),CmdOption::new("Polyline","P")];}
        if matches!(self.mode, BoundaryMode::GapTolerance { .. }) {
            return Vec::new();
        }
        if matches!(self.mode, BoundaryMode::SelectObjects) {
            return vec![CmdOption::enter(t!("Accept").as_ref())];
        }
        let island = format!(
            "{}: {}",
            t!("Island detection style"),
            t!(self.island_label())
        );
        let mut options = vec![
            CmdOption::new("Advanced options", "A"),
            CmdOption::new(t!("Boundary").as_ref(), "O"),
            CmdOption::new(&island, "S"),
            CmdOption::new(t!("Tolerance").as_ref(), "G"),
        ];
        if self.region_count() > 0 {
            options.push(CmdOption::enter("Accept"));
        }
        options
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if matches!(self.mode, BoundaryMode::PickInside) {
            let local = self.plane.to_local(pt);
            match self.picked_region([local.x, local.y]) {
                Some(region) => {
                    self.missed = false;
                    self.add_point_region(region);
                }
                None => self.missed = true,
            }
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.mode==BoundaryMode::ObjectType {self.mode=BoundaryMode::Advanced;return CmdResult::NeedPoint;}
        if self.mode==BoundaryMode::Advanced {self.mode=BoundaryMode::PickInside;return CmdResult::NeedPoint;}
        if let BoundaryMode::GapTolerance { return_to_selection } = self.mode {
            self.mode = if return_to_selection {
                BoundaryMode::SelectObjects
            } else {
                BoundaryMode::PickInside
            };
            return CmdResult::NeedPoint;
        }
        if matches!(self.mode, BoundaryMode::SelectObjects) {
            self.mode = BoundaryMode::PickInside;
            return CmdResult::NeedPoint;
        }
        let entities = self.make_entities();
        if entities.is_empty() {
            CmdResult::Cancel
        } else {
            CmdResult::CommitEntitiesAndExit(entities)
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn dyn_field(&self) -> crate::command::DynField {
        if matches!(self.mode, BoundaryMode::GapTolerance { .. }) {
            crate::command::DynField::Scalar
        } else {
            crate::command::DynField::Point
        }
    }

    fn dyn_commit_as_text(&self) -> bool {
        matches!(self.mode, BoundaryMode::GapTolerance { .. })
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.mode==BoundaryMode::Advanced {
            if matches!(text.trim().to_ascii_uppercase().as_str(),"O"|"OBJECT"|"OBJECT TYPE") {self.mode=BoundaryMode::ObjectType;}
            return Some(CmdResult::NeedPoint);
        }
        if self.mode==BoundaryMode::ObjectType {
            match text.trim().to_ascii_uppercase().as_str(){
                "R"|"REGION"=>self.output_region=true,
                "P"|"POLYLINE"=>self.output_region=false,
                _=>return Some(CmdResult::NeedPoint),
            }
            self.mode=BoundaryMode::Advanced;return Some(CmdResult::NeedPoint);
        }
        if let BoundaryMode::GapTolerance { return_to_selection } = self.mode {
            if let Ok(value) = text.trim().parse::<f64>() {
                if value.is_finite() && value > 0.0 {
                    self.gap_tolerance = value;
                    self.rebuild_faces();
                }
            }
            self.mode = if return_to_selection {
                BoundaryMode::SelectObjects
            } else {
                BoundaryMode::PickInside
            };
            return Some(CmdResult::NeedPoint);
        }
        match text.trim().to_ascii_uppercase().as_str() {
            "A"|"ADVANCED"=>self.mode=BoundaryMode::Advanced,
            "O" | "OBJECT" | "OBJECTS" => {
                self.mode = BoundaryMode::SelectObjects;
                self.missed = false;
            }
            "I" | "INTERNAL" | "POINTS" => {
                self.mode = BoundaryMode::PickInside;
                self.missed = false;
            }
            "S" | "ISLAND" | "ISLANDS" => {
                let style = match self.island_style {
                    BoundaryIslandStyle::Normal => BoundaryIslandStyle::Outer,
                    BoundaryIslandStyle::Outer => BoundaryIslandStyle::Ignore,
                    BoundaryIslandStyle::Ignore => BoundaryIslandStyle::Normal,
                };
                self.island_style = style;
                self.point_regions.clear();
            }
            "NORMAL" => {
                self.island_style = BoundaryIslandStyle::Normal;
                self.point_regions.clear();
            }
            "OUTER" => {
                self.island_style = BoundaryIslandStyle::Outer;
                self.point_regions.clear();
            }
            "IGNORE" => {
                self.island_style = BoundaryIslandStyle::Ignore;
                self.point_regions.clear();
            }
            "G" | "GAP" | "TOLERANCE" => {
                self.mode = BoundaryMode::GapTolerance {
                    return_to_selection: matches!(self.mode, BoundaryMode::SelectObjects),
                };
            }
            _ => return None,
        }
        Some(CmdResult::NeedPoint)
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.mode, BoundaryMode::SelectObjects)
    }

    fn selection_forces_add(&self) -> bool {
        matches!(self.mode, BoundaryMode::SelectObjects)
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if matches!(self.mode, BoundaryMode::SelectObjects) {
            self.set_boundary_set(handles);
            CmdResult::NeedPoint
        } else {
            CmdResult::Cancel
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if matches!(self.mode, BoundaryMode::PickInside) {
            self.point_regions.pop().map(|_| CmdResult::NeedPoint)
        } else {
            None
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if !matches!(self.mode, BoundaryMode::PickInside) {
            return None;
        }
        let local = self.plane.to_local(pt);
        let hovered = self.picked_region([local.x, local.y]);
        let mut rings: Vec<&Vec<[f64; 2]>> = Vec::new();
        for ring in self.point_regions.iter().flat_map(|region| region.iter()) {
            if !rings.iter().any(|existing| *existing == ring) {
                rings.push(ring);
            }
        }
        if let Some(region) = &hovered {
            for ring in region {
                if !rings.iter().any(|existing| *existing == ring) {
                    rings.push(ring);
                }
            }
        }
        if rings.is_empty() {
            return None;
        }
        let mut points = Vec::new();
        for ring in rings {
            if !points.is_empty() {
                points.push([f64::NAN; 3]);
            }
            for [x, y] in ring {
                points.push(self.plane.to_world(DVec3::new(*x, *y, 0.0)).to_array());
            }
            if let Some([x, y]) = ring.first() {
                points.push(self.plane.to_world(DVec3::new(*x, *y, 0.0)).to_array());
            }
        }
        Some(WireModel::solid_f64(
            "boundary_preview".into(),
            points,
            WireModel::CYAN,
            false,
        ))
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["BOUNDARY"] });  // BoundaryCommand
inventory::submit!(crate::command::CommandRegistration { names: &["GRADIENT", "-GRADIENT"] });  // GradientCommand
inventory::submit!(crate::command::CommandRegistration { names: &["HATCH", "-HATCH"] });  // HatchCommand

#[cfg(test)]
mod tests {
    use super::*;
    use codec::EntityType;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    // Two nested rectangles, regardless of draw order, the resolution must be
    // deterministic and independent of which was drawn first.
    fn nested(draw_order: bool) -> Vec<Vec<[f64; 2]>> {
        let big = rect(-10.0, -10.0, 10.0, 10.0);
        let small = rect(-5.0, -5.0, 5.0, 5.0);
        if draw_order {
            vec![big, small]
        } else {
            vec![small, big]
        }
    }

    #[test]
    fn click_inside_small_hatches_only_small() {
        for order in [true, false] {
            let rings = resolve_hatch_rings(&nested(order), [0.0, 0.0]).unwrap();
            // Exactly one ring (no hole) and it is the small rectangle.
            assert_eq!(rings.len(), 1, "order {order}");
            assert_eq!(rings[0].len(), 4);
            assert!((rings[0][0][0] - (-5.0)).abs() < 1e-9, "order {order}");
        }
    }

    #[test]
    fn click_between_hatches_ring_with_hole() {
        for order in [true, false] {
            let rings = resolve_hatch_rings(&nested(order), [8.0, 0.0]).unwrap();
            // Outer ring + the small rectangle as a hole.
            assert_eq!(rings.len(), 2, "order {order}");
            // Outer is the big rectangle.
            assert!((rings[0][0][0] - (-10.0)).abs() < 1e-9, "order {order}");
            // Hole is the small rectangle.
            assert!((rings[1][0][0] - (-5.0)).abs() < 1e-9, "order {order}");
        }
    }

    #[test]
    fn click_outside_returns_none() {
        assert!(resolve_hatch_rings(&nested(true), [50.0, 50.0]).is_none());
    }

    #[test]
    fn three_nested_levels() {
        let a = rect(-30.0, -30.0, 30.0, 30.0);
        let b = rect(-15.0, -15.0, 15.0, 15.0);
        let c = rect(-5.0, -5.0, 5.0, 5.0);
        // Click in the middle ring (between b and c).
        let rings = resolve_hatch_rings(&[a.clone(), b.clone(), c.clone()], [10.0, 0.0]).unwrap();
        assert_eq!(rings.len(), 2, "middle ring fill with inner hole");
        // Click inside the innermost.
        let rings = resolve_hatch_rings(&[a, b, c], [0.0, 0.0]).unwrap();
        assert_eq!(rings.len(), 1, "innermost fill has no hole");
    }

    #[test]
    fn click_outer_band_only_direct_child_is_hole() {
        let a = rect(-30.0, -30.0, 30.0, 30.0);
        let b = rect(-15.0, -15.0, 15.0, 15.0);
        let c = rect(-5.0, -5.0, 5.0, 5.0);
        // Click in the outermost band (between a and b): fill = a with only its
        // direct child b as a hole. The grandchild c must be excluded — adding
        // it would flip the innermost square back on under even-odd fill.
        let rings = resolve_hatch_rings(&[a, b, c], [20.0, 0.0]).unwrap();
        assert_eq!(rings.len(), 2, "outer band = a with b as its only hole");
        assert!((rings[0][0][0] - (-30.0)).abs() < 1e-9, "outer ring is a");
        assert!((rings[1][0][0] - (-15.0)).abs() < 1e-9, "hole is direct child b");
    }

    #[test]
    fn boundary_region_keeps_a_selected_hole_in_one_entity() {
        let rings = vec![rect(-10.0, -10.0, 10.0, 10.0), rect(-2.0, -2.0, 2.0, 2.0)];
        let segments: Vec<Line> = rings.iter().flat_map(|ring| {
            ring.iter().copied().zip(ring.iter().copied().cycle().skip(1))
                .take(ring.len()).map(|(start, end)| Line { start, end })
        }).collect();
        let mut sources = rustc_hash::FxHashMap::default();
        sources.insert(Handle::new(1), crate::scene::BoundarySource {
            curves: segments.iter().cloned().map(Curve::Line).collect(),
            segments,
        });
        let mut command = BoundaryCommand::new(sources, Vec::new(), WorkingPlane::default());
        command.point_regions = vec![rings];
        command.output_region = true;

        let entities = command.make_entities();
        let [EntityType::Region(region)] = entities.as_slice() else {
            panic!("expected one region");
        };
        assert_eq!(region.wires.len(), 2);
    }

    // ── HATCH dialog support ───────────────────────────────────────────────

    use crate::modules::draw::draw::hatch_settings::{
        FillTab, HatchRegion, HatchSettings, RegionOrigin,
    };

    fn sources_for(
        rings: &[Vec<[f64; 2]>],
    ) -> rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource> {
        let mut map = rustc_hash::FxHashMap::default();
        for (n, ring) in rings.iter().enumerate() {
            let segments: Vec<Line> = ring
                .iter()
                .copied()
                .zip(ring.iter().copied().cycle().skip(1))
                .take(ring.len())
                .map(|(start, end)| Line { start, end })
                .collect();
            map.insert(
                Handle::new(n as u64 + 1),
                crate::scene::BoundarySource {
                    curves: segments.iter().cloned().map(Curve::Line).collect(),
                    segments,
                },
            );
        }
        map
    }

    fn committed(result: CmdResult) -> HatchModel {
        match result {
            CmdResult::CommitHatch(hatch) => hatch,
            _ => panic!("expected CommitHatch"),
        }
    }

    fn other_pattern_name() -> String {
        crate::scene::model::hatch_patterns::catalog()
            .iter()
            .map(|entry| entry.name.clone())
            .find(|name| !name.eq_ignore_ascii_case("ANSI31"))
            .expect("the catalog has more than one pattern")
    }

    #[test]
    fn with_settings_follows_the_tab_and_never_mixes_the_two_fills() {
        let command = || {
            HatchCommand::new(Vec::new(), Default::default(), Vec::new(), None, WorkingPlane::default())
        };
        let mut gradient_settings = HatchSettings {
            tab: FillTab::Gradient,
            ..HatchSettings::default()
        };
        gradient_settings.gradient.shape = 3;
        gradient_settings.gradient.angle = "90".into();
        let gradient_resolved = gradient_settings.resolve().unwrap();
        let hatch_resolved = HatchSettings::default().resolve().unwrap();

        let by_gradient = command().with_settings(&gradient_resolved);
        let spec = by_gradient.gradient.as_ref().expect("the gradient is kept");
        assert_eq!(spec, &gradient_settings.gradient.spec().unwrap());
        assert!(by_gradient.pattern_override.is_none());
        assert!(by_gradient.angle_override.is_none() && by_gradient.scale_override.is_none());

        let by_pattern = command().with_settings(&hatch_resolved);
        assert!(by_pattern.gradient.is_none());
        assert_eq!(by_pattern.pattern_override.as_ref().map(|(n, _)| n.as_str()), Some("ANSI31"));
        assert_eq!(by_pattern.angle_override, Some(0.0));
        assert_eq!(by_pattern.scale_override, Some(1.0));

        // Going back to a pattern clears the gradient, and the other way round.
        let back = by_gradient.with_settings(&hatch_resolved);
        assert!(back.gradient.is_none() && back.pattern_override.is_some());
        let again = back.with_settings(&gradient_resolved);
        assert!(again.gradient.is_some() && again.pattern_override.is_none());
        assert!(again.angle_override.is_none() && again.scale_override.is_none());
    }

    #[test]
    fn settings_and_regions_commit_like_the_interactive_command() {
        let rings = vec![rect(-10.0, -10.0, 10.0, 10.0)];
        let sources = sources_for(&rings);
        let pattern = other_pattern_name();

        let mut interactive = HatchCommand::new(
            rings.clone(),
            sources.clone(),
            Vec::new(),
            None,
            WorkingPlane::default(),
        );
        let _ = interactive.on_text_input(&format!("P {pattern}"));
        let _ = interactive.on_text_input("A 30");
        let _ = interactive.on_text_input("L 2");
        let _ = interactive.on_point(DVec3::new(0.0, 0.0, 0.0));
        let expected = committed(interactive.on_enter());

        let settings = HatchSettings {
            pattern: pattern.clone(),
            angle: "30".into(),
            scale: "2".into(),
            ..HatchSettings::default()
        };
        let resolved = settings.resolve().expect("valid settings");
        let region = HatchRegion {
            rings: resolve_hatch_rings(&rings, [0.0, 0.0]).unwrap(),
        };
        let mut dialog = HatchCommand::new(
            rings,
            sources,
            Vec::new(),
            None,
            WorkingPlane::default(),
        )
        .with_settings(&resolved)
        .with_regions(vec![region]);
        let got = committed(dialog.on_enter());

        assert_eq!(got.name, expected.name);
        assert_eq!(got.scale, expected.scale);
        assert_eq!(got.angle_offset, expected.angle_offset);
        assert_eq!(got.style, expected.style);
        assert_eq!(*got.boundary, *expected.boundary);
        assert_eq!(
            got.boundary_paths.as_ref().map(|paths| paths.len()),
            expected.boundary_paths.as_ref().map(|paths| paths.len())
        );
    }

    #[test]
    fn separate_setting_commits_one_hatch_per_region() {
        let rings = vec![
            rect(0.0, 0.0, 10.0, 10.0),
            rect(30.0, 0.0, 40.0, 10.0),
        ];
        let resolved = {
            let mut settings = HatchSettings::default();
            settings.set_separate(true);
            settings.resolve().unwrap()
        };
        let regions = rings
            .iter()
            .map(|ring| HatchRegion {
                rings: vec![ring.clone()],
            })
            .collect();
        let mut command = HatchCommand::new(
            rings.clone(),
            sources_for(&rings),
            Vec::new(),
            None,
            WorkingPlane::default(),
        )
        .with_settings(&resolved)
        .with_regions(regions);
        match command.on_enter() {
            CmdResult::CommitHatches { hatches, .. } => assert_eq!(hatches.len(), 2),
            _ => panic!("expected CommitHatches"),
        }
    }

    #[test]
    fn retain_setting_commits_hatch_with_boundaries() {
        let rings = vec![rect(0.0, 0.0, 10.0, 10.0)];
        let resolved = {
            let mut settings = HatchSettings::default();
            settings.set_retain(true);
            settings.resolve().unwrap()
        };
        let mut command = HatchCommand::new(
            rings.clone(),
            sources_for(&rings),
            Vec::new(),
            None,
            WorkingPlane::default(),
        )
        .with_settings(&resolved)
        .with_regions(vec![HatchRegion {
            rings: rings.clone(),
        }]);
        assert!(matches!(
            command.on_enter(),
            CmdResult::CommitHatchWithBoundaries { .. }
        ));
    }

    #[test]
    fn no_regions_cancels() {
        let rings = vec![rect(0.0, 0.0, 10.0, 10.0)];
        let mut command = HatchCommand::new(
            rings.clone(),
            sources_for(&rings),
            Vec::new(),
            None,
            WorkingPlane::default(),
        )
        .with_regions(Vec::new());
        assert!(matches!(command.on_enter(), CmdResult::Cancel));
    }

    #[test]
    fn collector_returns_picked_regions_instead_of_committing() {
        let rings = vec![rect(-10.0, -10.0, 10.0, 10.0)];
        let mut command = HatchCommand::collecting(
            rings.clone(),
            sources_for(&rings),
            WorkingPlane::default(),
            false,
        );
        assert!(matches!(
            command.on_point(DVec3::new(0.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        match command.on_enter() {
            CmdResult::HatchBoundariesPicked { regions, objects } => {
                assert_eq!(regions.len(), 1);
                assert_eq!(regions[0].1, RegionOrigin::Points);
                assert_eq!(regions[0].0.rings.len(), 1);
                assert!(objects.is_empty());
            }
            _ => panic!("expected HatchBoundariesPicked"),
        }
    }

    #[test]
    fn collector_with_nothing_picked_still_returns_to_the_dialog() {
        let rings = vec![rect(-10.0, -10.0, 10.0, 10.0)];
        let mut command = HatchCommand::collecting(
            rings.clone(),
            sources_for(&rings),
            WorkingPlane::default(),
            false,
        );
        match command.on_enter() {
            CmdResult::HatchBoundariesPicked { regions, .. } => assert!(regions.is_empty()),
            _ => panic!("expected HatchBoundariesPicked"),
        }
    }

    #[test]
    fn collector_escape_dispatches_the_cancel_string() {
        let rings = vec![rect(-10.0, -10.0, 10.0, 10.0)];
        let mut command = HatchCommand::collecting(
            rings.clone(),
            sources_for(&rings),
            WorkingPlane::default(),
            false,
        );
        match command.on_escape() {
            CmdResult::Dispatch(text) => assert_eq!(text, "HATCH_PICK_CANCELLED"),
            _ => panic!("expected Dispatch"),
        }
    }

    #[test]
    fn plain_command_escape_still_cancels() {
        let mut command = HatchCommand::new(
            Vec::new(),
            Default::default(),
            Vec::new(),
            None,
            WorkingPlane::default(),
        );
        assert!(matches!(command.on_escape(), CmdResult::Cancel));
    }

    #[test]
    fn collector_in_object_mode_reports_the_chosen_objects() {
        let rings = vec![rect(-10.0, -10.0, 10.0, 10.0)];
        let sources = sources_for(&rings);
        let handle = *sources.keys().next().unwrap();
        let mut command =
            HatchCommand::collecting(rings, sources, WorkingPlane::default(), true);
        let _ = command.on_selection_complete(vec![handle]);
        match command.on_enter() {
            CmdResult::HatchBoundariesPicked { regions, objects } => {
                assert_eq!(regions.len(), 1);
                assert_eq!(regions[0].1, RegionOrigin::Objects);
                assert_eq!(objects, vec![handle]);
            }
            _ => panic!("expected HatchBoundariesPicked"),
        }
    }

    #[test]
    fn collector_ignores_settings_keywords() {
        let rings = vec![rect(-10.0, -10.0, 10.0, 10.0)];
        let mut command = HatchCommand::collecting(
            rings.clone(),
            sources_for(&rings),
            WorkingPlane::default(),
            false,
        );
        // The dialog owns these; typing them in the collector must not change anything.
        for text in ["P ANSI31", "A 30", "L 2", "B", "N", "D", "Y"] {
            assert!(command.on_text_input(text).is_none(), "{text}");
        }
    }

    #[test]
    fn object_regions_close_an_area_from_several_open_segments() {
        let corners = [
            ([0.0, 0.0], [10.0, 0.0]),
            ([10.0, 0.0], [10.0, 5.0]),
            ([10.0, 5.0], [0.0, 5.0]),
            ([0.0, 5.0], [0.0, 0.0]),
        ];
        let mut sources = rustc_hash::FxHashMap::default();
        let mut handles = Vec::new();
        for (n, (start, end)) in corners.iter().enumerate() {
            let line = Line {
                start: *start,
                end: *end,
            };
            let handle = Handle::new(n as u64 + 1);
            handles.push(handle);
            sources.insert(
                handle,
                crate::scene::BoundarySource {
                    curves: vec![Curve::Line(line)],
                    segments: vec![line],
                },
            );
        }
        let regions = object_regions(&sources, &handles);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].rings.len(), 1);
        // Three of the four sides do not enclose anything.
        assert!(object_regions(&sources, &handles[..3]).is_empty());
    }

    #[test]
    fn object_regions_ignore_unknown_handles() {
        let sources = rustc_hash::FxHashMap::default();
        assert!(object_regions(&sources, &[Handle::new(99)]).is_empty());
    }

    #[test]
    fn collector_does_not_offer_manual_drawing() {
        let rings = vec![rect(-10.0, -10.0, 10.0, 10.0)];
        let sources = sources_for(&rings);
        for select_objects in [false, true] {
            let mut command = HatchCommand::collecting(
                rings.clone(),
                sources.clone(),
                WorkingPlane::default(),
                select_objects,
            );
            let before = command.mode;
            assert!(
                command.options().iter().all(|option| option.keyword != "S"),
                "S offered (select_objects={select_objects})"
            );
            assert!(command.on_text_input("S").is_none());
            assert!(command.on_text_input("s").is_none());
            assert!(command.mode == before, "S changed the mode");
            assert!(!matches!(command.mode, HatchMode::Manual));
        }
    }

    #[test]
    fn plain_command_still_offers_and_enters_manual_drawing() {
        let mut command = HatchCommand::new(
            Vec::new(),
            Default::default(),
            Vec::new(),
            None,
            WorkingPlane::default(),
        );
        assert!(command.options().iter().any(|option| option.keyword == "S"));
        assert!(command.on_text_input("S").is_some());
        assert!(matches!(command.mode, HatchMode::Manual));
    }

    // ── Faithful preview ───────────────────────────────────────────────────

    use codec::entities::HatchStyleType;

    /// Rings in a NaN-separated buffer.
    fn ring_count(buffer: &[[f32; 2]]) -> usize {
        if buffer.is_empty() {
            0
        } else {
            1 + buffer.iter().filter(|point| point[0].is_nan()).count()
        }
    }

    fn ring_count_f64(buffer: &[[f64; 2]]) -> usize {
        if buffer.is_empty() {
            0
        } else {
            1 + buffer.iter().filter(|point| point[0].is_nan()).count()
        }
    }

    fn nested_rings() -> Vec<Vec<[f64; 2]>> {
        vec![
            rect(-30.0, -30.0, 30.0, 30.0),
            rect(-15.0, -15.0, 15.0, 15.0),
            rect(-5.0, -5.0, 5.0, 5.0),
        ]
    }

    fn command_for(
        rings: Vec<Vec<[f64; 2]>>,
        style: HatchStyleType,
        plane: WorkingPlane,
    ) -> HatchCommand {
        let mut settings = HatchSettings::default();
        settings.island_style = style;
        HatchCommand::new(rings.clone(), sources_for(&rings), Vec::new(), None, plane)
            .with_settings(&settings.resolve().unwrap())
            .with_regions(vec![HatchRegion { rings }])
    }

    fn nested_command(style: HatchStyleType, plane: WorkingPlane) -> HatchCommand {
        command_for(nested_rings(), style, plane)
    }

    const ALL_STYLES: [HatchStyleType; 3] = [
        HatchStyleType::Normal,
        HatchStyleType::Outer,
        HatchStyleType::Ignore,
    ];

    /// Half-sizes of the three nested squares, in the orders they are given
    /// (the second and third put the outer square somewhere other than first,
    /// so a filter that just keeps "the first N" is caught).
    const NESTING_ORDERS: [[f64; 3]; 3] = [
        [30.0, 15.0, 5.0],
        [15.0, 30.0, 5.0],
        [5.0, 15.0, 30.0],
    ];

    /// The rings of a NaN-separated buffer, one vector each.
    fn split_rings(buffer: &[[f32; 2]]) -> Vec<Vec<[f32; 2]>> {
        if buffer.is_empty() {
            return Vec::new();
        }
        buffer
            .split(|point| point[0].is_nan() || point[1].is_nan())
            .map(|ring| ring.to_vec())
            .collect()
    }

    /// Nesting depth of the square of half-size `half` among 30 / 15 / 5.
    fn square_depth(half: f64) -> usize {
        [30.0, 15.0, 5.0]
            .iter()
            .position(|&size| size == half)
            .expect("one of the nested squares")
    }

    /// The island rule of the spec, written out independently of the code.
    fn style_keeps(style: HatchStyleType, depth: usize) -> bool {
        match style {
            HatchStyleType::Normal => true,
            HatchStyleType::Outer => depth <= 1,
            HatchStyleType::Ignore => depth == 0,
        }
    }

    fn min_max(points: impl Iterator<Item = [f64; 2]>) -> [f64; 4] {
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for [x, y] in points {
            bounds = [
                bounds[0].min(x),
                bounds[1].min(y),
                bounds[2].max(x),
                bounds[3].max(y),
            ];
        }
        bounds
    }

    fn assert_close(a: [f64; 4], b: [f64; 4], what: &str) {
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1.0e-3, "{what}: {a:?} vs {b:?}");
        }
    }

    /// Every rendered buffer of the preview holds exactly the rings the style
    /// keeps, in the order they were given, with the coordinates each buffer
    /// is meant to have (world offsets for `boundary`, plane-local for
    /// `fill_plane_boundary`), and the consumers (CPU pattern, GPU box) agree.
    fn assert_preview_follows(style: HatchStyleType, plane: WorkingPlane, halves: &[f64]) {
        let label = format!("{style:?} {halves:?}");
        let rings: Vec<Vec<[f64; 2]>> = halves.iter().map(|&h| rect(-h, -h, h, h)).collect();
        let models = command_for(rings.clone(), style, plane).preview_models();
        assert_eq!(models.len(), 1, "{label}");
        let model = &models[0];

        // Expected, computed here from the input rings and the plane.
        let world_of = |point: [f64; 2]| {
            let world = plane.to_world(DVec3::new(point[0], point[1], 0.0));
            [world.x, world.y]
        };
        let anchor = world_of(rings[0][0]);
        let kept: Vec<usize> = (0..halves.len())
            .filter(|&i| style_keeps(style, square_depth(halves[i])))
            .collect();
        let expected_world: Vec<Vec<[f32; 2]>> = kept
            .iter()
            .map(|&i| {
                rings[i]
                    .iter()
                    .map(|&point| {
                        let world = world_of(point);
                        [(world[0] - anchor[0]) as f32, (world[1] - anchor[1]) as f32]
                    })
                    .collect()
            })
            .collect();
        let expected_local: Vec<Vec<[f32; 2]>> = kept
            .iter()
            .map(|&i| rings[i].iter().map(|&[x, y]| [x as f32, y as f32]).collect())
            .collect();
        let expected_exterior: Vec<bool> = kept
            .iter()
            .map(|&i| square_depth(halves[i]) == 0)
            .collect();
        // Outer must leave two rings, Ignore one, Normal all three.
        let survivors = match style {
            HatchStyleType::Normal => 3,
            HatchStyleType::Outer => 2,
            HatchStyleType::Ignore => 1,
        };
        assert_eq!(kept.len(), survivors, "{label}");

        let local_buffer = model.fill_plane_boundary.as_ref().unwrap();
        assert_eq!(split_rings(&model.boundary), expected_world, "boundary {label}");
        assert_eq!(
            split_rings(local_buffer),
            expected_local,
            "fill_plane_boundary {label}"
        );
        assert_eq!(
            **model.boundary_exterior.as_ref().unwrap(),
            expected_exterior,
            "boundary_exterior {label}"
        );
        assert_eq!(model.world_origin, anchor, "{label}");

        // GPU consumer: the box comes from `fill_plane_boundary`; mapped to the
        // world it has to be the box of what `boundary` describes.
        let gpu = min_max(
            local_buffer
                .iter()
                .filter(|point| point[0].is_finite() && point[1].is_finite())
                .map(|&[x, y]| [x as f64, y as f64]),
        );
        let local_box = min_max(kept.iter().flat_map(|&i| rings[i].iter().copied()));
        assert_close(gpu, local_box, &format!("gpu box {label}"));
        let gpu_in_world = min_max(
            [
                [gpu[0], gpu[1]],
                [gpu[2], gpu[1]],
                [gpu[2], gpu[3]],
                [gpu[0], gpu[3]],
            ]
            .into_iter()
            .map(world_of),
        );
        let cpu_box = min_max(
            model
                .boundary
                .iter()
                .filter(|point| point[0].is_finite() && point[1].is_finite())
                .map(|&[x, y]| {
                    [
                        x as f64 + model.world_origin[0],
                        y as f64 + model.world_origin[1],
                    ]
                }),
        );
        assert_close(gpu_in_world, cpu_box, &format!("gpu vs cpu box {label}"));

        // CPU consumer: the pattern is clipped against `boundary`, so it must
        // equal what the expected rings alone give, and the islands the style
        // keeps (and only those) must show in it.
        let mut reference = model.clone();
        let mut joined: Vec<[f32; 2]> = Vec::new();
        for ring in &expected_world {
            if !joined.is_empty() {
                joined.push([f32::NAN, f32::NAN]);
            }
            joined.extend(ring.iter().copied());
        }
        reference.boundary = std::sync::Arc::new(joined);
        let segments = model.pattern_segments();
        assert!(!segments.is_empty(), "{label}");
        assert_eq!(segments, reference.pattern_segments(), "pattern {label}");
        let (mut core, mut band) = (false, false);
        for [a, b] in &segments {
            let middle = plane.to_local(DVec3::new(
                (a[0] + b[0]) / 2.0,
                (a[1] + b[1]) / 2.0,
                0.0,
            ));
            let reach = middle.x.abs().max(middle.y.abs());
            core |= reach < 5.0;
            band |= reach > 5.0 && reach < 15.0;
        }
        let (want_core, want_band) = match style {
            HatchStyleType::Normal => (true, false),
            HatchStyleType::Outer => (false, false),
            HatchStyleType::Ignore => (true, true),
        };
        assert_eq!((core, band), (want_core, want_band), "pattern islands {label}");
    }

    #[test]
    fn preview_rings_follow_the_island_style() {
        for style in ALL_STYLES {
            for halves in NESTING_ORDERS {
                assert_preview_follows(style, WorkingPlane::default(), &halves);
            }
        }
    }

    #[test]
    fn preview_keeps_the_persisted_paths_complete() {
        let models =
            nested_command(HatchStyleType::Ignore, WorkingPlane::default()).preview_models();
        let model = &models[0];
        assert_eq!(model.boundary_paths.as_ref().unwrap().len(), 3);
        assert_eq!(model.boundary_sources.as_ref().unwrap().len(), 3);
        assert_eq!(ring_count_f64(model.boundary_wcs.as_ref().unwrap()), 3);
    }

    #[test]
    fn preview_buffers_stay_in_step_on_a_rotated_plane() {
        // Local x runs along world +Y, local y along world -X, and the plane
        // sits away from the world origin.
        let plane = WorkingPlane::new(DVec3::new(100.0, -40.0, 0.0), DVec3::Y, DVec3::NEG_X);
        for style in ALL_STYLES {
            for halves in NESTING_ORDERS {
                assert_preview_follows(style, plane, &halves);
            }
            // On a rotated plane the two buffers really are different coordinates.
            let models = nested_command(style, plane).preview_models();
            let model = &models[0];
            assert_ne!(
                model.boundary[1],
                model.fill_plane_boundary.as_ref().unwrap()[1],
                "{style:?}"
            );
        }
    }

    #[test]
    fn preview_matches_the_rings_the_entity_path_draws() {
        // The same persisted paths rebuilt through the DXF entity (what a
        // committed hatch goes through) keep the same rings, in the same order.
        let world_boxes = |model: &HatchModel| -> Vec<[f64; 4]> {
            split_rings(&model.boundary)
                .iter()
                .map(|ring| {
                    min_max(ring.iter().map(|&[x, y]| {
                        [
                            x as f64 + model.world_origin[0],
                            y as f64 + model.world_origin[1],
                        ]
                    }))
                })
                .collect()
        };
        for style in ALL_STYLES {
            for halves in NESTING_ORDERS {
                let label = format!("{style:?} {halves:?}");
                let rings: Vec<Vec<[f64; 2]>> =
                    halves.iter().map(|&h| rect(-h, -h, h, h)).collect();
                let models = command_for(rings, style, WorkingPlane::default()).preview_models();
                let preview = &models[0];
                let mut dxf = codec::entities::Hatch::new();
                dxf.is_solid = true;
                dxf.style = style;
                dxf.paths = preview.boundary_paths.as_ref().unwrap().as_ref().clone();
                let entity = crate::scene::Scene::hatch_model_from_dxf(&dxf, [1.0; 4])
                    .expect("entity model");

                let from_preview = world_boxes(preview);
                let from_entity = world_boxes(&entity);
                assert_eq!(from_preview.len(), from_entity.len(), "{label}");
                for (a, b) in from_preview.iter().zip(&from_entity) {
                    assert_close(*a, *b, &format!("ring box {label}"));
                }
                assert_eq!(preview.boundary_exterior, entity.boundary_exterior, "{label}");
                assert_eq!(
                    split_rings(preview.fill_plane_boundary.as_ref().unwrap()).len(),
                    split_rings(entity.fill_plane_boundary.as_ref().unwrap()).len(),
                    "{label}"
                );
            }
        }
    }

    #[test]
    fn separate_preview_makes_one_model_per_region() {
        let rings = vec![
            rect(0.0, 0.0, 10.0, 10.0),
            rect(30.0, 0.0, 40.0, 10.0),
            rect(60.0, 0.0, 70.0, 10.0),
        ];
        let mut settings = HatchSettings::default();
        settings.set_separate(true);
        let regions = rings
            .iter()
            .map(|ring| HatchRegion {
                rings: vec![ring.clone()],
            })
            .collect();
        let command = HatchCommand::new(
            rings.clone(),
            sources_for(&rings),
            Vec::new(),
            None,
            WorkingPlane::default(),
        )
        .with_settings(&settings.resolve().unwrap())
        .with_regions(regions);
        assert_eq!(command.preview_models().len(), 3);
    }

    #[test]
    fn retain_preview_is_a_single_model_even_for_several_regions() {
        let rings = vec![rect(0.0, 0.0, 10.0, 10.0), rect(30.0, 0.0, 40.0, 10.0)];
        let mut settings = HatchSettings::default();
        settings.set_retain(true);
        let regions = rings
            .iter()
            .map(|ring| HatchRegion {
                rings: vec![ring.clone()],
            })
            .collect();
        let command = HatchCommand::new(
            rings.clone(),
            sources_for(&rings),
            Vec::new(),
            None,
            WorkingPlane::default(),
        )
        .with_settings(&settings.resolve().unwrap())
        .with_regions(regions);
        assert_eq!(command.preview_models().len(), 1);
    }

    #[test]
    fn preview_with_nothing_picked_is_empty() {
        let rings = vec![rect(0.0, 0.0, 10.0, 10.0)];
        let command = HatchCommand::new(
            rings.clone(),
            sources_for(&rings),
            Vec::new(),
            None,
            WorkingPlane::default(),
        );
        assert!(command.preview_models().is_empty());
    }

    #[test]
    fn command_line_preview_uses_the_same_models() {
        let command = nested_command(HatchStyleType::Outer, WorkingPlane::default());
        let from_trait = command.hatch_preview_models().unwrap();
        assert_eq!(from_trait.len(), command.preview_models().len());
        assert_eq!(ring_count(&from_trait[0].boundary), 2);
    }

    #[test]
    fn manual_preview_is_one_model_like_the_commit_even_with_separate_on() {
        let rings = vec![rect(0.0, 0.0, 10.0, 10.0), rect(30.0, 0.0, 40.0, 10.0)];
        let mut settings = HatchSettings::default();
        settings.set_separate(true);
        let regions = rings
            .iter()
            .map(|ring| HatchRegion {
                rings: vec![ring.clone()],
            })
            .collect();
        let mut command = HatchCommand::new(
            rings.clone(),
            sources_for(&rings),
            Vec::new(),
            None,
            WorkingPlane::default(),
        )
        .with_settings(&settings.resolve().unwrap())
        .with_regions(regions);
        assert_eq!(command.preview_models().len(), 2, "two regions, two hatches");
        assert!(command.on_text_input("S").is_some());
        for point in [[60.0, 0.0], [70.0, 0.0], [70.0, 10.0]] {
            let _ = command.on_point(DVec3::new(point[0], point[1], 0.0));
        }
        assert_eq!(
            command.preview_models().len(),
            1,
            "Manual mode commits one hatch, so the preview shows one"
        );
        assert!(matches!(command.on_enter(), CmdResult::CommitHatch(_)));
    }

    #[test]
    fn collector_neither_offers_nor_takes_the_mode_switch() {
        let rings = vec![rect(-10.0, -10.0, 10.0, 10.0)];
        let sources = sources_for(&rings);
        for select_objects in [false, true] {
            let mut command = HatchCommand::collecting(
                rings.clone(),
                sources.clone(),
                WorkingPlane::default(),
                select_objects,
            );
            let before = command.mode;
            assert!(
                command
                    .options()
                    .iter()
                    .all(|option| option.keyword != "O" && option.keyword != "I"),
                "a mode switch is offered (select_objects={select_objects})"
            );
            for text in ["O", "o", "OBJECTS", "I", "i", "INTERNAL"] {
                assert!(command.on_text_input(text).is_none(), "{text}");
            }
            assert!(command.mode == before, "the mode changed");
        }
    }

    #[test]
    fn plain_command_still_switches_between_points_and_objects() {
        let mut command = HatchCommand::new(
            Vec::new(),
            Default::default(),
            Vec::new(),
            None,
            WorkingPlane::default(),
        );
        assert!(command.options().iter().any(|option| option.keyword == "O"));
        assert!(command.on_text_input("O").is_some());
        assert!(matches!(command.mode, HatchMode::SelectObjects));
        assert!(command.options().iter().any(|option| option.keyword == "I"));
        assert!(command.on_text_input("I").is_some());
        assert!(matches!(command.mode, HatchMode::PickInside));
    }

    // ── Spec §10: every combination commits like the command line ──────────

    /// Everything a commit result carries, as text, so two results can be
    /// compared field by field (patterns and their phase included).
    fn describe(result: CmdResult) -> Vec<String> {
        match result {
            CmdResult::CommitHatch(hatch) => vec!["CommitHatch".into(), format!("{hatch:?}")],
            CmdResult::CommitStyledHatch {
                hatch,
                color,
                transparency,
            } => vec![
                "CommitStyledHatch".into(),
                format!("{hatch:?}"),
                format!("{color:?} {transparency:?}"),
            ],
            CmdResult::CommitHatches {
                hatches,
                entity_style,
            } => {
                let mut lines = vec!["CommitHatches".into(), format!("{entity_style:?}")];
                lines.extend(hatches.iter().map(|hatch| format!("{hatch:?}")));
                lines
            }
            CmdResult::CommitHatchWithBoundaries {
                hatch,
                boundaries,
                entity_style,
            } => vec![
                "CommitHatchWithBoundaries".into(),
                format!("{hatch:?}"),
                format!("{boundaries:?}"),
                format!("{entity_style:?}"),
            ],
            _ => panic!("expected a commit"),
        }
    }

    #[test]
    fn every_combination_of_settings_commits_like_the_interactive_command() {
        // Two plain squares and a ring with a hole: the third click lands
        // between the big square and the small one inside it.
        let rings = vec![
            rect(0.0, 0.0, 10.0, 10.0),
            rect(30.0, 0.0, 40.0, 10.0),
            rect(60.0, -10.0, 100.0, 30.0),
            rect(70.0, 0.0, 90.0, 20.0),
        ];
        let sources = sources_for(&rings);
        let clicks = [[5.0, 5.0], [35.0, 5.0], [65.0, 5.0]];
        let pattern = other_pattern_name();
        // The identity plane, a plane away from the origin turned about Z, and
        // one standing on the XZ plane.
        let planes = [
            WorkingPlane::default(),
            WorkingPlane::new(DVec3::new(100.0, -40.0, 7.0), DVec3::Y, DVec3::NEG_X),
            WorkingPlane::new(DVec3::new(0.0, 5.0, 0.0), DVec3::X, DVec3::Z),
        ];
        let origin = [3.0, 4.0];
        let mut compared = 0;
        for plane in planes {
            for associative in [true, false] {
                for shape in ["plain", "separate", "retain"] {
                    for (island, detection) in [
                        (HatchStyleType::Normal, true),
                        (HatchStyleType::Outer, true),
                        (HatchStyleType::Ignore, true),
                        (HatchStyleType::Outer, false),
                    ] {
                        let label = format!(
                            "assoc={associative} {shape} {island:?} detection={detection} plane={plane:?}"
                        );
                        let effective = if detection { island } else { HatchStyleType::Ignore };

                        // The command line, with the keywords a user types.
                        let mut interactive = HatchCommand::new(
                            rings.clone(),
                            sources.clone(),
                            Vec::new(),
                            None,
                            plane,
                        )
                        .with_origin(origin);
                        let _ = interactive.on_text_input(&format!("P {pattern}"));
                        let _ = interactive.on_text_input("A 30");
                        let _ = interactive.on_text_input("L 2");
                        if !associative {
                            let _ = interactive.on_text_input("N");
                        }
                        match shape {
                            "separate" => {
                                let _ = interactive.on_text_input("D");
                            }
                            "retain" => {
                                let _ = interactive.on_text_input("B");
                            }
                            _ => {}
                        }
                        let turns = match effective {
                            HatchStyleType::Normal => 0,
                            HatchStyleType::Outer => 1,
                            HatchStyleType::Ignore => 2,
                        };
                        for _ in 0..turns {
                            let _ = interactive.on_text_input("Y");
                        }
                        for click in clicks {
                            let _ = interactive
                                .on_point(plane.to_world(DVec3::new(click[0], click[1], 0.0)));
                        }
                        let expected = describe(interactive.on_enter());

                        // The dialog: settings plus the regions it collected.
                        let mut settings = HatchSettings {
                            pattern: pattern.clone(),
                            angle: "30".into(),
                            scale: "2".into(),
                            associative,
                            island_detection: detection,
                            island_style: island,
                            ..HatchSettings::default()
                        };
                        match shape {
                            "separate" => settings.set_separate(true),
                            "retain" => settings.set_retain(true),
                            _ => {}
                        }
                        let regions = clicks
                            .iter()
                            .map(|click| HatchRegion {
                                rings: resolve_hatch_rings(&rings, *click).unwrap(),
                            })
                            .collect();
                        let mut dialog = HatchCommand::new(
                            rings.clone(),
                            sources.clone(),
                            Vec::new(),
                            None,
                            plane,
                        )
                        .with_origin(origin)
                        .with_settings(&settings.resolve().unwrap())
                        .with_regions(regions);
                        let got = describe(dialog.on_enter());

                        let kind = match shape {
                            "separate" => "CommitHatches",
                            "retain" => "CommitHatchWithBoundaries",
                            _ => "CommitHatch",
                        };
                        assert_eq!(expected[0], kind, "{label}: the command line");
                        assert_eq!(got, expected, "{label}");
                        compared += 1;
                    }
                }
            }
        }
        assert_eq!(compared, 72);
    }

    #[test]
    fn the_equivalence_check_can_tell_settings_apart() {
        // Guards the test above: two different settings must not describe the same.
        let rings = vec![rect(0.0, 0.0, 10.0, 10.0)];
        let make = |angle: &str| {
            let settings = HatchSettings {
                angle: angle.into(),
                ..HatchSettings::default()
            };
            describe(
                HatchCommand::new(
                    rings.clone(),
                    sources_for(&rings),
                    Vec::new(),
                    None,
                    WorkingPlane::default(),
                )
                .with_settings(&settings.resolve().unwrap())
                .with_regions(vec![HatchRegion {
                    rings: rings.clone(),
                }])
                .on_enter(),
            )
        };
        assert_ne!(make("0"), make("30"));
    }

    use crate::entities::hatch_fill::{rgba_of, tinted_second_color};
    use crate::modules::draw::draw::hatch_settings::GradientSettings;

    fn square_command() -> HatchCommand {
        let ring = rect(0.0, 0.0, 10.0, 10.0);
        HatchCommand::new(
            vec![ring.clone()],
            sources_for(std::slice::from_ref(&ring)),
            Vec::new(),
            None,
            WorkingPlane::default(),
        )
        .with_regions(vec![HatchRegion { rings: vec![ring] }])
    }

    fn gradient_settings(edit: impl FnOnce(&mut GradientSettings)) -> HatchSettings {
        let mut settings = HatchSettings::default();
        settings.tab = FillTab::Gradient;
        edit(&mut settings.gradient);
        settings
    }

    fn gradient_command(settings: &HatchSettings) -> HatchCommand {
        square_command().with_settings(&settings.resolve().expect("valid"))
    }

    #[test]
    fn a_gradient_command_commits_a_gradient_model() {
        let settings = gradient_settings(|g| {
            g.shape = 5; // CHOICES[5] = curved
            g.angle = "30".into();
            g.centered = false;
        });
        let model = committed(gradient_command(&settings).on_enter());
        let HatchPattern::Gradient { angle_deg, kind, invert, shift, one_color, .. } = model.pattern else {
            panic!("a gradient model")
        };
        assert_eq!(kind, crate::scene::model::hatch_model::GradientKind::Curved);
        assert!(!invert && !one_color);
        assert!((angle_deg - 30.0).abs() < 1e-4);
        assert_eq!(shift, 1.0);
        assert_eq!(model.name, "CURVED");
        assert!(model.pattern_origin.is_none());
        assert!((model.angle_offset - 30f32.to_radians()).abs() < 1e-5);
    }

    #[test]
    fn the_models_second_colour_is_the_effective_one() {
        let settings = gradient_settings(|g| {
            g.one_color = true;
            g.tint = 0.25;
            g.color1 = codec::types::Color::Rgb { r: 200, g: 100, b: 50 };
            g.color2 = codec::types::Color::Rgb { r: 1, g: 2, b: 3 }; // hidden
        });
        let model = committed(gradient_command(&settings).on_enter());
        let base = rgba_of(codec::types::Color::Rgb { r: 200, g: 100, b: 50 }).unwrap();
        let expected = rgba_of(tinted_second_color(base, 0.25)).unwrap();
        let HatchPattern::Gradient { color2, one_color, tint, .. } = model.pattern else {
            panic!("a gradient model")
        };
        assert!(one_color);
        assert_eq!(tint, 0.25);
        assert_eq!(color2, expected, "never the raw hidden Color 2");
        assert_eq!(model.color, base);
    }

    #[test]
    fn a_gradient_command_keeps_the_rings_the_sources_and_the_island_style() {
        let mut settings = gradient_settings(|_| {});
        settings.island_detection = false; // Ignore
        let model = committed(gradient_command(&settings).on_enter());
        assert_eq!(model.style, codec::entities::HatchStyleType::Ignore);
        assert!(model.boundary_paths.is_some() && model.fill_plane.is_some());
        assert_eq!(model.boundary_exterior.as_deref().map(|e| e.len()), Some(1));
    }

    #[test]
    fn a_creation_style_turns_every_commit_into_a_styled_one() {
        use codec::types::{Color, Transparency};
        let style = (Color::Index(1), Transparency::ByLayer);
        let settings = HatchSettings::default();
        // One hatch.
        let mut one = square_command()
            .with_settings(&settings.resolve().unwrap())
            .with_creation_style(Some(style.clone()));
        match one.on_enter() {
            CmdResult::CommitStyledHatch { color, transparency, .. } => {
                assert_eq!((color, transparency), style)
            }
            _ => panic!("a styled hatch"),
        }
        // Separate hatches.
        let mut separate = settings.clone();
        separate.set_separate(true);
        let mut command = square_command()
            .with_settings(&separate.resolve().unwrap())
            .with_creation_style(Some(style.clone()));
        match command.on_enter() {
            CmdResult::CommitHatches { hatches, entity_style } => {
                assert_eq!(hatches.len(), 1);
                assert_eq!(entity_style, Some(style.clone()));
            }
            _ => panic!("separate hatches"),
        }
        // Retained boundaries.
        let mut retain = settings.clone();
        retain.set_retain(true);
        let mut command = square_command()
            .with_settings(&retain.resolve().unwrap())
            .with_creation_style(Some(style.clone()));
        match command.on_enter() {
            CmdResult::CommitHatchWithBoundaries { entity_style, .. } => {
                assert_eq!(entity_style, Some(style))
            }
            _ => panic!("hatch with boundaries"),
        }
    }

    #[test]
    fn a_creation_style_also_styles_a_manual_boundary() {
        use codec::types::{Color, Transparency};
        let style = (Color::Index(3), Transparency::ByLayer);
        let mut command = HatchCommand::new(
            Vec::new(),
            rustc_hash::FxHashMap::default(),
            Vec::new(),
            None,
            WorkingPlane::default(),
        )
        .with_creation_style(Some(style.clone()));
        command.mode = HatchMode::Manual;
        for point in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)] {
            command.manual_pts.push(DVec3::new(point.0, point.1, 0.0));
        }
        match command.on_enter() {
            CmdResult::CommitStyledHatch { color, transparency, .. } => {
                assert_eq!((color, transparency), style)
            }
            _ => panic!("a styled manual hatch"),
        }
    }

    #[test]
    fn without_a_creation_style_the_commit_is_what_it_always_was() {
        let mut command = square_command().with_settings(&HatchSettings::default().resolve().unwrap());
        assert!(matches!(command.on_enter(), CmdResult::CommitHatch(_)));
    }

    #[test]
    fn the_collector_is_given_neither_style_nor_preview_colour() {
        let ring = rect(0.0, 0.0, 10.0, 10.0);
        let collector = HatchCommand::collecting(
            vec![ring.clone()],
            sources_for(std::slice::from_ref(&ring)),
            WorkingPlane::default(),
            false,
        );
        assert!(collector.creation_style.is_none() && collector.preview_color.is_none());
    }

    #[test]
    fn the_preview_uses_the_chosen_colour_and_blue_by_default() {
        let settings = HatchSettings::default().resolve().unwrap();
        let blue = square_command().with_settings(&settings).preview_models();
        assert_eq!(blue[0].color, [0.15, 0.55, 1.0, 0.75]);
        let chosen = square_command()
            .with_settings(&settings)
            .with_preview_color(Some([1.0, 0.0, 0.0, 0.75]))
            .preview_models();
        assert_eq!(chosen[0].color, [1.0, 0.0, 0.0, 0.75]);
    }

    #[test]
    fn the_gradient_preview_shows_both_real_colours_translucent() {
        let settings = gradient_settings(|g| {
            g.color1 = codec::types::Color::Rgb { r: 255, g: 0, b: 0 };
            g.color2 = codec::types::Color::Rgb { r: 0, g: 0, b: 255 };
        });
        let models = gradient_command(&settings)
            .with_preview_color(Some([0.0, 1.0, 0.0, 0.75])) // must not win over the gradient
            .preview_models();
        assert_eq!(models[0].color, [1.0, 0.0, 0.0, 0.75]);
        let HatchPattern::Gradient { color2, .. } = &models[0].pattern else {
            panic!("a gradient preview")
        };
        assert_eq!(*color2, [0.0, 0.0, 1.0, 0.75]);
    }

    #[test]
    fn a_one_colour_preview_shows_the_tint_and_ignores_the_hidden_colour() {
        let preview = |color2| {
            let settings = gradient_settings(|g| {
                g.one_color = true;
                g.tint = 0.25;
                g.color1 = codec::types::Color::Rgb { r: 200, g: 100, b: 50 };
                g.color2 = color2;
            });
            gradient_command(&settings).preview_models().remove(0)
        };
        let a = preview(codec::types::Color::Rgb { r: 1, g: 2, b: 3 });
        let b = preview(codec::types::Color::Rgb { r: 250, g: 250, b: 250 });
        let colour2 = |m: &HatchModel| match &m.pattern {
            HatchPattern::Gradient { color2, .. } => *color2,
            _ => panic!("gradient"),
        };
        assert_eq!(colour2(&a), colour2(&b), "the hidden Color 2 changes nothing");
        let base = rgba_of(codec::types::Color::Rgb { r: 200, g: 100, b: 50 }).unwrap();
        let mut expected = rgba_of(tinted_second_color(base, 0.25)).unwrap();
        expected[3] = 0.75;
        assert_eq!(colour2(&a), expected);
    }

    #[test]
    fn separate_gradient_hatches_get_one_gradient_each() {
        let ring_a = rect(0.0, 0.0, 10.0, 10.0);
        let ring_b = rect(20.0, 0.0, 30.0, 10.0);
        let rings = [ring_a.clone(), ring_b.clone()];
        let mut settings = gradient_settings(|_| {});
        settings.set_separate(true);
        let mut command = HatchCommand::new(
            rings.to_vec(),
            sources_for(&rings),
            Vec::new(),
            None,
            WorkingPlane::default(),
        )
        .with_regions(vec![
            HatchRegion { rings: vec![ring_a] },
            HatchRegion { rings: vec![ring_b] },
        ])
        .with_settings(&settings.resolve().unwrap());
        match command.on_enter() {
            CmdResult::CommitHatches { hatches, .. } => {
                assert_eq!(hatches.len(), 2);
                assert!(hatches.iter().all(|h| matches!(h.pattern, HatchPattern::Gradient { .. })));
            }
            _ => panic!("separate hatches"),
        }
    }
}
