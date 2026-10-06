//! ArchLine: layout model of the classic toolbars.
//!
//! Each toolbar is either docked on one of the four window edges (in a *lane*:
//! a row or column of side-by-side bars; lane 0 hugs the window edge) or
//! floating at a point inside the window. Pure data, no iced, so every rule is
//! unit-tested here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Distance from a window edge within which a dragged bar docks (px).
pub const SNAP_BAND: f32 = 60.0;
/// Thickness of one lane: 36 px button + 2×2 bar padding + 2×3 lane padding.
pub const LANE_THICKNESS: f32 = 46.0;
/// Height of menu bar + document tabs above the toolbar area (estimate).
pub const TOP_CHROME: f32 = 62.0;
/// Height of the status bar below the toolbar area (estimate).
pub const BOTTOM_CHROME: f32 = 30.0;
/// First floating position given to a bar shown from the list, and the step
/// between consecutive ones (a cascade, wrapping after `CASCADE_STEPS`).
pub const CASCADE_X: f32 = 120.0;
pub const CASCADE_Y: f32 = 130.0;
pub const CASCADE_STEP: f32 = 28.0;
pub const CASCADE_STEPS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolbarId {
    Draw,
    Modify,
    Annotation,
    Block,
    Measure,
    Layers,
}

impl ToolbarId {
    pub const ALL: [ToolbarId; 6] = [
        ToolbarId::Draw,
        ToolbarId::Modify,
        ToolbarId::Annotation,
        ToolbarId::Block,
        ToolbarId::Measure,
        ToolbarId::Layers,
    ];

    /// Stable name used as the key in `settings.json`.
    pub fn key(self) -> &'static str {
        match self {
            ToolbarId::Draw => "Draw",
            ToolbarId::Modify => "Modify",
            ToolbarId::Annotation => "Annotation",
            ToolbarId::Block => "Block",
            ToolbarId::Measure => "Measure",
            ToolbarId::Layers => "Layers",
        }
    }

    pub fn from_key(key: &str) -> Option<ToolbarId> {
        Self::ALL.into_iter().find(|id| id.key() == key)
    }

    /// Title shown on a floating bar.
    pub fn title(self) -> &'static str {
        self.key()
    }

    /// `Layers` holds combo boxes and is horizontal only.
    pub fn allowed_on(self, edge: Edge) -> bool {
        !(self == ToolbarId::Layers && edge.is_vertical())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    pub fn is_vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockSlot {
    pub edge: Edge,
    pub lane: u8,
    pub index: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Placement {
    Docked(DockSlot),
    /// `home` is the last docked slot, for the double-click return.
    Floating {
        x: f32,
        y: f32,
        home: Option<DockSlot>,
    },
    /// Not shown. `home` is the last docked slot, kept for when it comes back.
    Hidden { home: Option<DockSlot> },
}

/// Where a drag would drop the bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Dock(DockSlot),
    Float { x: f32, y: f32 },
}

fn default_slot(id: ToolbarId) -> DockSlot {
    use Edge::*;
    use ToolbarId::*;
    let (edge, lane, index) = match id {
        Draw => (Left, 0, 0),
        Modify => (Right, 0, 0),
        Annotation => (Top, 0, 0),
        Block => (Top, 0, 1),
        Measure => (Top, 0, 2),
        Layers => (Top, 1, 0),
    };
    DockSlot { edge, lane, index }
}

/// Keyed by bar name (a `String`) rather than `ToolbarId` so that a key this
/// build does not know is ignored instead of failing the whole settings file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolbarLayout {
    pub bars: BTreeMap<String, Placement>,
}

impl Default for ToolbarLayout {
    fn default() -> Self {
        Self {
            bars: ToolbarId::ALL
                .into_iter()
                .map(|id| (id.key().to_string(), Placement::Docked(default_slot(id))))
                .collect(),
        }
    }
}

impl ToolbarLayout {
    pub fn placement(&self, id: ToolbarId) -> Placement {
        self.bars
            .get(id.key())
            .copied()
            .unwrap_or(Placement::Docked(default_slot(id)))
    }

    /// Docked bars of `edge`, grouped by lane (lane 0 first), each lane in
    /// index order. Gaps in the stored numbers are ignored.
    pub fn lanes(&self, edge: Edge) -> Vec<Vec<ToolbarId>> {
        let mut docked: Vec<(DockSlot, ToolbarId)> = ToolbarId::ALL
            .into_iter()
            .filter_map(|id| match self.placement(id) {
                Placement::Docked(s) if s.edge == edge => Some((s, id)),
                _ => None,
            })
            .collect();
        docked.sort_by_key(|(s, id)| (s.lane, s.index, *id));
        let mut lanes: Vec<Vec<ToolbarId>> = Vec::new();
        let mut last = None;
        for (s, id) in docked {
            if last != Some(s.lane) {
                lanes.push(Vec::new());
                last = Some(s.lane);
            }
            if let Some(lane) = lanes.last_mut() {
                lane.push(id);
            }
        }
        lanes
    }

    pub fn floating(&self) -> Vec<(ToolbarId, f32, f32)> {
        ToolbarId::ALL
            .into_iter()
            .filter_map(|id| match self.placement(id) {
                Placement::Floating { x, y, .. } => Some((id, x, y)),
                _ => None,
            })
            .collect()
    }

    /// Rewrite the slots of `edge` from `lanes`, dropping empty lanes.
    fn write_lanes(&mut self, edge: Edge, lanes: &[Vec<ToolbarId>]) {
        let mut lane_no = 0u8;
        for lane in lanes.iter().filter(|l| !l.is_empty()) {
            for (i, &id) in lane.iter().enumerate() {
                self.bars.insert(
                    id.key().to_string(),
                    Placement::Docked(DockSlot {
                        edge,
                        lane: lane_no,
                        index: i as u8,
                    }),
                );
            }
            lane_no = lane_no.saturating_add(1);
        }
    }

    fn compact(&mut self, edge: Edge) {
        let lanes = self.lanes(edge);
        self.write_lanes(edge, &lanes);
    }

    pub fn move_to(&mut self, id: ToolbarId, target: Target) {
        let previous = self.placement(id);
        let home = match previous {
            Placement::Docked(s) => Some(s),
            Placement::Floating { home, .. } | Placement::Hidden { home } => home,
        };
        match target {
            Target::Float { x, y } => {
                self.bars
                    .insert(id.key().to_string(), Placement::Floating { x, y, home });
                if let Placement::Docked(s) = previous {
                    self.compact(s.edge);
                }
            }
            Target::Dock(slot) => {
                let edge = if id.allowed_on(slot.edge) {
                    slot.edge
                } else {
                    Edge::Top
                };
                // Park the bar outside every edge so `lanes` excludes it.
                self.bars.insert(
                    id.key().to_string(),
                    Placement::Floating { x: 0.0, y: 0.0, home },
                );
                let mut lanes = self.lanes(edge);
                let lane = (slot.lane as usize).min(lanes.len());
                if lane == lanes.len() {
                    lanes.push(Vec::new());
                }
                let index = (slot.index as usize).min(lanes[lane].len());
                lanes[lane].insert(index, id);
                self.write_lanes(edge, &lanes);
                if let Placement::Docked(s) = previous {
                    if s.edge != edge {
                        self.compact(s.edge);
                    }
                }
            }
        }
    }

    pub fn is_visible(&self, id: ToolbarId) -> bool {
        !matches!(self.placement(id), Placement::Hidden { .. })
    }

    /// Show a hidden bar (floating, in a cascade) or hide a visible one.
    /// Anything else is a no-op.
    pub fn set_visible(&mut self, id: ToolbarId, visible: bool) {
        let key = id.key().to_string();
        match (self.placement(id), visible) {
            (Placement::Hidden { home }, true) => {
                let n = (self.floating().len() % CASCADE_STEPS) as f32;
                self.bars.insert(
                    key,
                    Placement::Floating {
                        x: CASCADE_X + CASCADE_STEP * n,
                        y: CASCADE_Y + CASCADE_STEP * n,
                        home,
                    },
                );
            }
            (Placement::Docked(s), false) => {
                self.bars.insert(key, Placement::Hidden { home: Some(s) });
                self.compact(s.edge);
            }
            (Placement::Floating { home, .. }, false) => {
                self.bars.insert(key, Placement::Hidden { home });
            }
            _ => {}
        }
    }

    /// Double-click on a floating bar: back to where it was docked.
    pub fn redock(&mut self, id: ToolbarId) {
        if let Placement::Floating { home, .. } = self.placement(id) {
            self.move_to(id, Target::Dock(home.unwrap_or_else(|| default_slot(id))));
        }
    }

    /// Make a layout read from disk safe to use: forget unknown bars, fill
    /// missing ones with their default, fix bars on an edge they cannot use,
    /// replace non-finite coordinates, and renumber every lane.
    pub fn sanitized(mut self) -> Self {
        self.bars.retain(|k, _| ToolbarId::from_key(k).is_some());
        for id in ToolbarId::ALL {
            let fixed = match self.placement(id) {
                Placement::Docked(s) if !id.allowed_on(s.edge) => Placement::Docked(DockSlot {
                    edge: Edge::Top,
                    lane: 0,
                    index: u8::MAX,
                }),
                Placement::Floating { x, y, home } => Placement::Floating {
                    x: if x.is_finite() { x } else { 0.0 },
                    y: if y.is_finite() { y } else { 0.0 },
                    home: home.filter(|s| id.allowed_on(s.edge)),
                },
                Placement::Hidden { home } => Placement::Hidden {
                    home: home.filter(|s| id.allowed_on(s.edge)),
                },
                other => other,
            };
            self.bars.insert(id.key().to_string(), fixed);
        }
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            self.compact(edge);
        }
        self
    }
}

/// What `resolve_drop` needs to know about the window and the bars.
pub struct DropCtx<'a> {
    /// Window size in px.
    pub win: (f32, f32),
    /// Estimated length of a bar along its edge, in px.
    pub length: &'a dyn Fn(ToolbarId) -> f32,
}

/// Where releasing `dragged` with the pointer at `cursor` would put it.
/// `float_at` is the top-left to use when the bar ends up floating.
///
/// Lane and index come from the pointer position and the *estimated* bar
/// lengths, not from widget bounds (spec §3.5).
pub fn resolve_drop(
    layout: &ToolbarLayout,
    dragged: ToolbarId,
    cursor: (f32, f32),
    float_at: (f32, f32),
    ctx: &DropCtx,
) -> Target {
    let (x, y) = cursor;
    let candidates = [
        (Edge::Top, (y - TOP_CHROME).max(0.0)),
        (Edge::Bottom, (ctx.win.1 - BOTTOM_CHROME - y).max(0.0)),
        (Edge::Left, x.max(0.0)),
        (Edge::Right, (ctx.win.0 - x).max(0.0)),
    ];
    // `dragged` is parked outside every edge so it does not count as a lane
    // occupant of the edge it is leaving.
    let mut probe = layout.clone();
    probe.bars.insert(
        dragged.key().to_string(),
        Placement::Floating {
            x: 0.0,
            y: 0.0,
            home: None,
        },
    );
    // The snap band covers every existing lane plus one more, so a new lane
    // can always be opened below the last one.
    let best = candidates
        .into_iter()
        .filter(|(edge, d)| {
            let band = SNAP_BAND.max((probe.lanes(*edge).len() + 1) as f32 * LANE_THICKNESS);
            dragged.allowed_on(*edge) && *d < band
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((edge, dist)) = best else {
        return Target::Float {
            x: float_at.0,
            y: float_at.1,
        };
    };

    let lanes = probe.lanes(edge);
    let lane = ((dist / LANE_THICKNESS) as usize).min(lanes.len());
    let index = if lane < lanes.len() {
        let along = if edge.is_vertical() { y - TOP_CHROME } else { x };
        let mut start = 0.0;
        let mut index = 0usize;
        for id in &lanes[lane] {
            let l = (ctx.length)(*id);
            if along > start + l / 2.0 {
                index += 1;
            }
            start += l;
        }
        index
    } else {
        0
    };
    Target::Dock(DockSlot {
        edge,
        lane: lane.min(u8::MAX as usize) as u8,
        index: index.min(u8::MAX as usize) as u8,
    })
}

/// Keep a floating bar of `size` fully inside a window of size `win`.
pub fn clamp_floating(pos: (f32, f32), size: (f32, f32), win: (f32, f32)) -> (f32, f32) {
    let max_x = (win.0 - size.0).max(0.0);
    let max_y = (win.1 - size.1).max(0.0);
    (pos.0.clamp(0.0, max_x), pos.1.clamp(0.0, max_y))
}

/// Rectangle `(x, y, w, h)` of the lane `slot` points at, for the drop preview.
pub fn band_rect(slot: DockSlot, win: (f32, f32)) -> (f32, f32, f32, f32) {
    let lane = slot.lane as f32;
    let side_h = (win.1 - TOP_CHROME - BOTTOM_CHROME).max(0.0);
    match slot.edge {
        Edge::Top => (0.0, TOP_CHROME + lane * LANE_THICKNESS, win.0, LANE_THICKNESS),
        Edge::Bottom => (
            0.0,
            win.1 - BOTTOM_CHROME - (lane + 1.0) * LANE_THICKNESS,
            win.0,
            LANE_THICKNESS,
        ),
        Edge::Left => (lane * LANE_THICKNESS, TOP_CHROME, LANE_THICKNESS, side_h),
        Edge::Right => (
            win.0 - (lane + 1.0) * LANE_THICKNESS,
            TOP_CHROME,
            LANE_THICKNESS,
            side_h,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn len(_: ToolbarId) -> f32 {
        100.0
    }

    fn ctx() -> DropCtx<'static> {
        DropCtx {
            win: (1600.0, 900.0),
            length: &len,
        }
    }

    fn slot(edge: Edge, lane: u8, index: u8) -> DockSlot {
        DockSlot { edge, lane, index }
    }

    #[test]
    fn default_matches_todays_layout() {
        use ToolbarId::*;
        let l = ToolbarLayout::default();
        assert_eq!(l.lanes(Edge::Left), vec![vec![Draw]]);
        assert_eq!(l.lanes(Edge::Right), vec![vec![Modify]]);
        assert_eq!(
            l.lanes(Edge::Top),
            vec![vec![Annotation, Block, Measure], vec![Layers]]
        );
        assert!(l.lanes(Edge::Bottom).is_empty());
        assert!(l.floating().is_empty());
    }

    #[test]
    fn move_to_dock_inserts_and_compacts_the_old_edge() {
        use ToolbarId::*;
        let mut l = ToolbarLayout::default();
        l.move_to(Block, Target::Dock(slot(Edge::Left, 0, 0)));
        assert_eq!(l.lanes(Edge::Left), vec![vec![Block, Draw]]);
        assert_eq!(l.lanes(Edge::Top), vec![vec![Annotation, Measure], vec![Layers]]);
        assert_eq!(l.placement(Measure), Placement::Docked(slot(Edge::Top, 0, 1)));
    }

    #[test]
    fn leaving_a_lane_empty_removes_it_and_remembers_home() {
        use ToolbarId::*;
        let mut l = ToolbarLayout::default();
        l.move_to(Layers, Target::Float { x: 120.0, y: 200.0 });
        assert_eq!(l.lanes(Edge::Top), vec![vec![Annotation, Block, Measure]]);
        assert_eq!(
            l.placement(Layers),
            Placement::Floating {
                x: 120.0,
                y: 200.0,
                home: Some(slot(Edge::Top, 1, 0)),
            }
        );
        assert_eq!(l.floating(), vec![(Layers, 120.0, 200.0)]);
    }

    #[test]
    fn redock_returns_a_floating_bar_to_its_home() {
        use ToolbarId::*;
        let mut l = ToolbarLayout::default();
        l.move_to(Layers, Target::Float { x: 10.0, y: 10.0 });
        l.redock(Layers);
        assert_eq!(l.placement(Layers), Placement::Docked(slot(Edge::Top, 1, 0)));
    }

    #[test]
    fn layers_never_docks_on_a_side() {
        use ToolbarId::*;
        let mut l = ToolbarLayout::default();
        l.move_to(Layers, Target::Dock(slot(Edge::Left, 0, 0)));
        match l.placement(Layers) {
            Placement::Docked(s) => assert_eq!(s.edge, Edge::Top),
            other => panic!("expected docked, got {other:?}"),
        }
        assert!(!Layers.allowed_on(Edge::Right));
        assert!(Layers.allowed_on(Edge::Bottom));
    }

    #[test]
    fn out_of_range_lane_and_index_are_clamped() {
        use ToolbarId::*;
        let mut l = ToolbarLayout::default();
        l.move_to(Draw, Target::Dock(slot(Edge::Top, 9, 50)));
        assert_eq!(l.lanes(Edge::Top).len(), 3, "a new lane is opened, not lane 9");
        assert_eq!(l.lanes(Edge::Top).last(), Some(&vec![Draw]));
        l.move_to(Modify, Target::Dock(slot(Edge::Top, 0, 50)));
        assert_eq!(l.lanes(Edge::Top)[0].last(), Some(&Modify));
        assert!(l.lanes(Edge::Left).is_empty() && l.lanes(Edge::Right).is_empty());
    }

    #[test]
    fn resolve_drop_snaps_to_the_nearest_allowed_edge() {
        use ToolbarId::*;
        let l = ToolbarLayout::default();
        // 10 px under the top chrome: top edge, lane 0, after the three bars.
        assert_eq!(
            resolve_drop(&l, Draw, (800.0, TOP_CHROME + 10.0), (0.0, 0.0), &ctx()),
            Target::Dock(slot(Edge::Top, 0, 3))
        );
        // 50 px: second lane; the pointer is right of its only bar (100 px).
        assert_eq!(
            resolve_drop(&l, Draw, (300.0, TOP_CHROME + 50.0), (0.0, 0.0), &ctx()),
            Target::Dock(slot(Edge::Top, 1, 1))
        );
        // Left edge, lane 0, Draw's own lane is empty once it is removed.
        assert_eq!(
            resolve_drop(&l, Draw, (10.0, 450.0), (0.0, 0.0), &ctx()),
            Target::Dock(slot(Edge::Left, 0, 0))
        );
        // Middle of the window: floating at the requested point.
        assert_eq!(
            resolve_drop(&l, Draw, (800.0, 450.0), (790.0, 440.0), &ctx()),
            Target::Float { x: 790.0, y: 440.0 }
        );
    }

    #[test]
    fn resolve_drop_reaches_a_new_lane_below_the_existing_ones() {
        // Top already holds two lanes; a bar released under them must open a
        // third instead of floating (the snap band grows with the lanes).
        use ToolbarId::*;
        let l = ToolbarLayout::default();
        assert_eq!(
            resolve_drop(&l, Draw, (800.0, TOP_CHROME + 100.0), (0.0, 0.0), &ctx()),
            Target::Dock(slot(Edge::Top, 2, 0))
        );
        // Still far from everything: floating.
        assert_eq!(
            resolve_drop(&l, Draw, (800.0, TOP_CHROME + 300.0), (7.0, 9.0), &ctx()),
            Target::Float { x: 7.0, y: 9.0 }
        );
    }

    #[test]
    fn resolve_drop_never_offers_a_side_to_layers() {
        let l = ToolbarLayout::default();
        assert_eq!(
            resolve_drop(&l, ToolbarId::Layers, (10.0, 450.0), (5.0, 440.0), &ctx()),
            Target::Float { x: 5.0, y: 440.0 }
        );
    }

    #[test]
    fn clamp_keeps_a_floating_bar_inside_the_window() {
        assert_eq!(clamp_floating((1500.0, 880.0), (300.0, 46.0), (1600.0, 900.0)), (1300.0, 854.0));
        assert_eq!(clamp_floating((-20.0, -5.0), (300.0, 46.0), (1600.0, 900.0)), (0.0, 0.0));
        // A bar wider than the window pins to the origin instead of panicking.
        assert_eq!(clamp_floating((50.0, 10.0), (2000.0, 46.0), (1600.0, 900.0)), (0.0, 10.0));
    }

    #[test]
    fn serde_round_trip_and_old_settings() {
        let mut l = ToolbarLayout::default();
        l.move_to(ToolbarId::Draw, Target::Float { x: 120.5, y: 80.0 });
        let json = serde_json::to_string(&l).unwrap();
        let back: ToolbarLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(back, l);
        // A settings file written before this feature has no `bars` at all.
        let old: ToolbarLayout = serde_json::from_str("{}").unwrap();
        assert_eq!(old, ToolbarLayout::default());
    }

    #[test]
    fn sanitized_drops_unknown_bars_and_repairs_bad_placements() {
        let json = r#"{"bars":{
            "Standard":{"Floating":{"x":1.0,"y":2.0,"home":null}},
            "Layers":{"Docked":{"edge":"Left","lane":0,"index":0}}
        }}"#;
        let l: ToolbarLayout = serde_json::from_str(json).unwrap();
        let l = l.sanitized();
        assert_eq!(l.bars.len(), 6, "unknown key gone, missing bars filled");
        match l.placement(ToolbarId::Layers) {
            Placement::Docked(s) => assert_eq!(s.edge, Edge::Top),
            other => panic!("expected docked, got {other:?}"),
        }
        assert_eq!(
            l.placement(ToolbarId::Draw),
            Placement::Docked(slot(Edge::Left, 0, 0)),
            "bars missing from the file take their default place"
        );
    }

    #[test]
    fn band_rect_hugs_the_requested_edge_and_lane() {
        let win = (1600.0, 900.0);
        assert_eq!(band_rect(slot(Edge::Top, 1, 0), win), (0.0, TOP_CHROME + LANE_THICKNESS, 1600.0, LANE_THICKNESS));
        assert_eq!(band_rect(slot(Edge::Left, 0, 0), win).0, 0.0);
        let (x, _, w, _) = band_rect(slot(Edge::Right, 0, 0), win);
        assert_eq!(x + w, 1600.0);
    }

    #[test]
    fn hiding_a_docked_bar_removes_it_compacts_lanes_and_remembers_home() {
        use ToolbarId::*;
        let mut l = ToolbarLayout::default();
        l.set_visible(Layers, false);
        assert!(!l.is_visible(Layers));
        assert_eq!(l.lanes(Edge::Top), vec![vec![Annotation, Block, Measure]]);
        assert_eq!(
            l.placement(Layers),
            Placement::Hidden { home: Some(slot(Edge::Top, 1, 0)) }
        );
        assert!(l.floating().is_empty());
    }

    #[test]
    fn showing_a_hidden_bar_opens_it_floating_in_a_cascade() {
        use ToolbarId::*;
        let mut l = ToolbarLayout::default();
        l.set_visible(Draw, false);
        l.set_visible(Modify, false);
        l.set_visible(Draw, true);
        l.set_visible(Modify, true);
        assert_eq!(
            l.placement(Draw),
            Placement::Floating { x: CASCADE_X, y: CASCADE_Y, home: Some(slot(Edge::Left, 0, 0)) }
        );
        assert_eq!(
            l.placement(Modify),
            Placement::Floating {
                x: CASCADE_X + CASCADE_STEP,
                y: CASCADE_Y + CASCADE_STEP,
                home: Some(slot(Edge::Right, 0, 0)),
            }
        );
    }

    #[test]
    fn the_cascade_wraps_so_many_shown_bars_stay_in_reach() {
        // Review focus 3.
        use ToolbarId::*;
        let mut l = ToolbarLayout::default();
        let ids = [Draw, Modify, Annotation, Block, Measure, Layers];
        for id in ids {
            l.set_visible(id, false);
        }
        for id in ids {
            l.set_visible(id, true);
        }
        assert_eq!(l.floating().len(), 6);
        let last = ids.iter().map(|id| l.placement(*id)).last().unwrap();
        match last {
            Placement::Floating { x, y, .. } => {
                assert!(x <= CASCADE_X + CASCADE_STEP * CASCADE_STEPS as f32);
                assert!(y <= CASCADE_Y + CASCADE_STEP * CASCADE_STEPS as f32);
            }
            other => panic!("expected floating, got {other:?}"),
        }
    }

    #[test]
    fn a_shown_bar_can_return_to_its_home_and_a_floating_one_can_be_hidden() {
        use ToolbarId::*;
        let mut l = ToolbarLayout::default();
        l.set_visible(Draw, false);
        l.set_visible(Draw, true);
        l.redock(Draw);
        assert_eq!(l.placement(Draw), Placement::Docked(slot(Edge::Left, 0, 0)));

        l.move_to(Layers, Target::Float { x: 50.0, y: 60.0 });
        l.set_visible(Layers, false);
        assert_eq!(
            l.placement(Layers),
            Placement::Hidden { home: Some(slot(Edge::Top, 1, 0)) }
        );
        // Hiding twice or showing a visible bar changes nothing.
        let before = l.clone();
        l.set_visible(Layers, false);
        l.set_visible(Draw, true);
        assert_eq!(l, before);
    }

    #[test]
    fn hidden_survives_serde_and_sanitize() {
        use ToolbarId::*;
        let mut l = ToolbarLayout::default();
        l.set_visible(Block, false);
        let json = serde_json::to_string(&l).unwrap();
        let back: ToolbarLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(back, l);
        let back = back.sanitized();
        assert!(!back.is_visible(Block));
        assert_eq!(back.lanes(Edge::Top), vec![vec![Annotation, Measure], vec![Layers]]);
    }
}
