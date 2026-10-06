# Barre sganciabili, agganciabili e flottanti — Piano di implementazione

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ogni barra classica (Draw, Modify, Annotation, Block, Measure, Layers) si trascina dall'impugnatura, si aggancia a uno dei quattro bordi (più barre affiancate) o resta flottante dentro la finestra, e la posizione è salvata.

**Architecture:** Un modello puro (`ToolbarLayout`: per ogni barra un `Placement` agganciato o flottante, con regole di spostamento e di rilascio) sta in `src/ui/toolbar_layout.rs`. Il rendering (bordi a lane, barre flottanti, livello di drag) sta in `src/ui/toolbar_dock.rs`. Il layout è un campo di `AppConfig`; il drag usa il pattern del dock (`mouse_area` a schermo intero solo durante il drag).

**Tech Stack:** Rust, iced (rev 23604ff: `pin`, `opaque`, `Stack::with_children`, `mouse_area` con `on_move`/`on_release`/`on_double_click`), `iced_aw::ContextMenu` (invariato), serde/serde_json.

**Spec:** `docs/superpowers/specs/2026-10-06-barre-sganciabili-design.md`

## Global Constraints

- Lavora su `lavoro`; un commit per task; **nessuna riga di attribuzione** nei messaggi di commit. Non pushare.
- Tutto il codice nuovo in file propri (`toolbar_layout.rs`, `toolbar_dock.rs`, `app/update/toolbar.rs`); punti di aggancio in codice upstream minimi (`src/app/view/mod.rs`, `src/app/mod.rs`, `src/app/config.rs`, `src/app/update/file.rs`, `src/app/update/mod.rs`, `src/app/commands/display.rs`, `src/app/commands/mod.rs`).
- `view_main` e `start_page_content` sono enormi: **nessuna logica nuova lì**, solo chiamate a funzioni `#[inline(never)]` (trappola 1 di `CLAUDE.md`).
- Le righe di un `iced_aw::ContextMenu` restano `mouse_area` con `on_press`, mai `button` (trappola 2). Non toccare `flyout_row`.
- Non rimuovere `RUST_MIN_STACK` da `.cargo/config.toml`.
- Stringhe UI in inglese tramite `crate::t!(...)` (il catalogo italiano non si tocca).
- Workspace ribbon (`ARCHLINE_WORKSPACE=ribbon`), pagina iniziale e clean screen restano invariati: tutto vale solo con `workspace::classic_active(is_start, clean_screen)`.
- Barra dei menu e barra delle schede documento restano fisse.
- Costanti di geometria (valori da spec §3): fascia di aggancio `SNAP_BAND = 60.0`, spessore lane `LANE_THICKNESS = 46.0`, altezza chrome sopra le barre `TOP_CHROME = 62.0`, sotto `BOTTOM_CHROME = 30.0`. Sono stime da tarare a occhio con gli screenshot (Task 5).
- Comandi di verifica: `cargo check --locked`; `cargo test --locked --lib <filtro>`; build finale `cargo build --release --bin OpenCADStudio` (fallisce con "Accesso negato" se `OpenCADStudio.exe` è aperto: chiedere all'utente di chiuderlo).

## Review Focus

Casi che la spec implica ma che i test "felici" non esercitano; ognuno ha il suo test nel task indicato.

1. `settings.json` vecchio (senza `toolbars`) o con una chiave barra sconosciuta → nessun errore, layout di default; il resto della config non va perso (Task 1, Task 2).
2. Rilascio con `lane`/`index` fuori range (lane 9, index 50) → clampati, nessun panic, nessuna lane vuota (Task 1).
3. `Layers` rilasciata vicino al bordo sinistro/destro → mai agganciata in verticale (Task 1).
4. Finestra rimpicciolita mentre una barra flottante sta lontano a destra/in basso → la barra resta raggiungibile (`clamp_floating`) (Task 1, Task 5).
5. Clic sull'impugnatura senza muovere il mouse (Grab, Release, nessun `DragMove`) → il layout non cambia e non si scrive nulla (Task 4).
6. Tutte e sei le barre flottanti → i quattro bordi sono vuoti e la finestra si costruisce lo stesso (Task 3).

---

### Task 1: Modello del layout (puro)

**Files:**
- Create: `src/ui/toolbar_layout.rs`
- Modify: `src/ui/mod.rs` (aggiungere `pub mod toolbar_layout;` accanto a `pub mod classic_toolbar;`)

**Interfaces:**
- Consumes: niente.
- Produces (usati dai task 2–6):
  - `enum ToolbarId { Draw, Modify, Annotation, Block, Measure, Layers }` con `ALL: [ToolbarId; 6]`, `key(self) -> &'static str`, `from_key(&str) -> Option<ToolbarId>`, `title(self) -> &'static str`, `allowed_on(self, Edge) -> bool`.
  - `enum Edge { Top, Bottom, Left, Right }` con `is_vertical(self) -> bool`.
  - `struct DockSlot { edge: Edge, lane: u8, index: u8 }`.
  - `enum Placement { Docked(DockSlot), Floating { x: f32, y: f32, home: Option<DockSlot> } }`.
  - `enum Target { Dock(DockSlot), Float { x: f32, y: f32 } }`.
  - `struct ToolbarLayout { bars: BTreeMap<String, Placement> }` con `placement(id)`, `lanes(edge) -> Vec<Vec<ToolbarId>>`, `floating() -> Vec<(ToolbarId, f32, f32)>`, `move_to(id, Target)`, `redock(id)`, `sanitized(self) -> Self`.
  - `struct DropCtx<'a> { win: (f32, f32), length: &'a dyn Fn(ToolbarId) -> f32 }`; `resolve_drop(layout, dragged, cursor, float_at, ctx) -> Target`.
  - `clamp_floating(pos, size, win) -> (f32, f32)`; `band_rect(slot, win) -> (f32, f32, f32, f32)`.
  - costanti `SNAP_BAND`, `LANE_THICKNESS`, `TOP_CHROME`, `BOTTOM_CHROME`.

- [ ] **Step 1: Registrare il modulo e scrivere i test che falliscono**

In `src/ui/mod.rs`, dopo `pub mod classic_toolbar;` aggiungere:

```rust
pub mod toolbar_layout;
```

Creare `src/ui/toolbar_layout.rs` contenente SOLO il modulo di test (l'implementazione arriva allo step 3):

```rust
//! ArchLine: layout model of the classic toolbars.
//!
//! Each toolbar is either docked on one of the four window edges (in a *lane*:
//! a row or column of side-by-side bars; lane 0 hugs the window edge) or
//! floating at a point inside the window. Pure data, no iced, so every rule is
//! unit-tested here.

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
        // 50 px: second lane.
        assert_eq!(
            resolve_drop(&l, Draw, (300.0, TOP_CHROME + 50.0), (0.0, 0.0), &ctx()),
            Target::Dock(slot(Edge::Top, 1, 0))
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
}
```

- [ ] **Step 2: Verificare che non compili**

Run: `cargo test --locked --lib toolbar_layout 2>&1 | tail -15`
Expected: FAIL, errori `cannot find type ToolbarId`, `DropCtx`, … (l'implementazione non c'è ancora).

- [ ] **Step 3: Scrivere l'implementazione**

Inserire in `src/ui/toolbar_layout.rs`, **sopra** `#[cfg(test)] mod tests`, subito dopo i commenti di modulo:

```rust
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
                Placement::Docked(_) => None,
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
            Placement::Floating { home, .. } => home,
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
    let best = candidates
        .into_iter()
        .filter(|(edge, d)| dragged.allowed_on(*edge) && *d < SNAP_BAND)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((edge, dist)) = best else {
        return Target::Float {
            x: float_at.0,
            y: float_at.1,
        };
    };

    let mut probe = layout.clone();
    probe.bars.insert(
        dragged.key().to_string(),
        Placement::Floating {
            x: 0.0,
            y: 0.0,
            home: None,
        },
    );
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
```

- [ ] **Step 4: Eseguire i test**

Run: `cargo test --locked --lib toolbar_layout 2>&1 | tail -25`
Expected: PASS, 11 test. Se `resolve_drop_snaps_to_the_nearest_allowed_edge` fallisce sull'indice, controllare l'aritmetica: tre barre da 100 px, `x = 800` supera il punto medio di tutte → indice 3.

- [ ] **Step 5: Commit**

```bash
git add src/ui/toolbar_layout.rs src/ui/mod.rs
git commit -m "Barre classiche: modello del layout (agganciate o flottanti)"
```

---

### Task 2: Persistenza e stato nell'app

**Files:**
- Modify: `src/app/config.rs` (campo `toolbars` in `AppConfig` e `Default`, test)
- Modify: `src/app/update/file.rs:1161-1181` (`current_config`) e `:1237-1239` (ripristino)
- Modify: `src/app/mod.rs` (campo `toolbars` accanto a `dock`, ~riga 828; inizializzazione ~riga 4231)

**Interfaces:**
- Consumes: `ToolbarLayout` (Task 1).
- Produces: `OpenCADStudio.toolbars: crate::ui::toolbar_layout::ToolbarLayout` (`pub(crate)`), persistito in `AppConfig.toolbars`.

- [ ] **Step 1: Scrivere il test che fallisce**

In `src/app/config.rs`, nel modulo `tests` esistente (cerca `fn test_parse_theme_name`), aggiungere:

```rust
    #[test]
    fn toolbars_default_when_absent_and_round_trip() {
        // A settings.json from before the dockable toolbars has no section.
        let old: AppConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(old.toolbars, crate::ui::toolbar_layout::ToolbarLayout::default());

        let mut cfg = AppConfig::default();
        cfg.toolbars.move_to(
            crate::ui::toolbar_layout::ToolbarId::Draw,
            crate::ui::toolbar_layout::Target::Float { x: 40.0, y: 90.0 },
        );
        let json = serde_json::to_string(&cfg).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.toolbars, cfg.toolbars);
    }
```

- [ ] **Step 2: Verificare che fallisca**

Run: `cargo test --locked --lib toolbars_default_when_absent 2>&1 | tail -8`
Expected: FAIL (errore di compilazione: campo `toolbars` inesistente).

- [ ] **Step 3: Implementare**

`src/app/config.rs`, in `struct AppConfig`, dopo il campo `dock`:

```rust
    /// Where each classic toolbar is: docked on an edge, or floating.
    pub toolbars: crate::ui::toolbar_layout::ToolbarLayout,
```

in `impl Default for AppConfig`, dopo `dock: ...`:

```rust
            toolbars: crate::ui::toolbar_layout::ToolbarLayout::default(),
```

`src/app/mod.rs`, dopo il campo `pub(crate) dock: crate::ui::dock::DockState,` aggiungere:

```rust
    /// Classic toolbar placement (docked lane / floating position).
    pub(crate) toolbars: crate::ui::toolbar_layout::ToolbarLayout,
```

e nell'inizializzazione, dopo `dock: Default::default(),`:

```rust
            toolbars: Default::default(),
```

`src/app/update/file.rs`, in `current_config`, dopo il blocco `dock: { ... },`:

```rust
            toolbars: self.toolbars.clone(),
```

e nel ripristino, dopo `self.dock = dock;`:

```rust
        self.toolbars = cfg.toolbars.sanitized();
```

- [ ] **Step 4: Eseguire i test**

Run: `cargo test --locked --lib toolbars_default_when_absent 2>&1 | tail -6` → PASS.
Run: `cargo test --locked --lib config 2>&1 | tail -6` → PASS (nessuna regressione).
Run: `cargo check --locked 2>&1 | tail -3` → nessun errore.

- [ ] **Step 5: Commit**

```bash
git add src/app/config.rs src/app/mod.rs src/app/update/file.rs
git commit -m "Barre classiche: layout salvato in settings.json"
```

---

### Task 3: Barre separate e rendering ai bordi (layout di default = oggi)

**Files:**
- Modify: `src/ui/classic_toolbar.rs` (liste separate, `items_for`, costanti `pub(super)`; rimozione di `extra`, `top_bar`, `wrap_center`, `vertical`, `horizontal`)
- Modify: `src/ui/classic_layers.rs` (estrarre `layer_row`, rimuovere `layer_bar`)
- Create: `src/ui/toolbar_dock.rs`
- Modify: `src/ui/mod.rs` (`pub mod toolbar_dock;`)
- Modify: `src/app/view/mod.rs:2192-2197` (aggancio)
- Test: `src/app/view/classic.rs` (modulo `tests`)

**Interfaces:**
- Consumes: `ToolbarLayout::{lanes, floating}` (Task 1); `OpenCADStudio.toolbars` (Task 2).
- Produces:
  - `classic_toolbar::items_for(id: ToolbarId) -> &'static [ClassicItem]`; `BTN_SIZE` e `ICON_SIZE` `pub(super)`.
  - `classic_layers::layer_row(ribbon: &Ribbon) -> iced::widget::Row<'static, Message>`.
  - `toolbar_dock::frame(classic: bool, layout: &ToolbarLayout, ribbon: &Ribbon, dragging: Option<ToolbarId>, center: Element<'a, Message>) -> Element<'a, Message>`.
  - `toolbar_dock::bar_length(id) -> f32`, `bar_size(id, vertical) -> (f32, f32)`, costante `GRIP_ANCHOR: f32`.
  - `enum ToolbarMsg { Grab(ToolbarId), DragMove(iced::Point), DragRelease, Redock(ToolbarId), Reset }` (definito qui perché le barre emettono `Message::Toolbar(ToolbarMsg::Grab(id))`; il handler arriva nel Task 4).
  - `Message::Toolbar(crate::ui::toolbar_dock::ToolbarMsg)` in `src/app/mod.rs`.

- [ ] **Step 1: Test che falliscono (liste separate)**

In `src/ui/classic_toolbar.rs`, nel modulo `tests`, aggiungere:

```rust
    #[test]
    fn annotation_block_and_measure_are_separate_bars_with_buttons() {
        use crate::ui::toolbar_layout::ToolbarId::*;
        for id in [Annotation, Block, Measure] {
            assert!(
                !buttons(items_for(id)).is_empty(),
                "{id:?} has no buttons"
            );
        }
        // Layers is built by classic_layers, not from an item list.
        assert!(items_for(Layers).is_empty());
        // The three lists together are exactly what the old single strip held.
        let union: Vec<_> = [Annotation, Block, Measure]
            .iter()
            .flat_map(|id| buttons(items_for(*id)))
            .map(|b| b.main.id)
            .collect();
        let old: Vec<_> = buttons(&tools().extra).iter().map(|b| b.main.id).collect();
        assert_eq!(union, old);
    }
```

Run: `cargo test --locked --lib annotation_block_and_measure 2>&1 | tail -6` → FAIL (`items_for` non esiste).

Se dopo l'implementazione un gruppo risulta senza pulsanti (assert 1) o l'unione non coincide con `extra`, **fermarsi e riferire**: significa che quel gruppo ribbon non ha elementi riducibili a pulsanti e la separazione va ripensata.

- [ ] **Step 2: Separare le liste in `classic_toolbar.rs`**

Cambiare le costanti in `pub(super)`:

```rust
pub(super) const BTN_SIZE: f32 = 36.0;
```

Nel `struct ClassicTools` aggiungere (lasciando `extra` per ora):

```rust
    pub annotation: Vec<ClassicItem>,
    pub block: Vec<ClassicItem>,
    pub measure: Vec<ClassicItem>,
```

in `tools()` aggiungere i tre campi:

```rust
        annotation: group_items(&["Annotation"]),
        block: group_items(&["Block"]),
        measure: group_items(&["Measure"]),
```

e dopo `tools()`:

```rust
/// Item list of one toolbar. `Layers` is built by `classic_layers`, so its
/// list is empty.
pub fn items_for(id: crate::ui::toolbar_layout::ToolbarId) -> &'static [ClassicItem] {
    use crate::ui::toolbar_layout::ToolbarId::*;
    let t = tools();
    match id {
        Draw => &t.draw,
        Modify => &t.modify,
        Annotation => &t.annotation,
        Block => &t.block,
        Measure => &t.measure,
        Layers => &[],
    }
}
```

Run: `cargo test --locked --lib annotation_block_and_measure 2>&1 | tail -6` → PASS.

- [ ] **Step 3: Estrarre `layer_row` in `classic_layers.rs`**

Sostituire la funzione `layer_bar` (righe 147–189) con:

```rust
/// The layer / properties controls as one row, without its strip container
/// (the dock wraps it in a bar like any other).
///
/// Kept out of `view_main` on purpose (see `toolbar_dock::frame`).
#[inline(never)]
pub fn layer_row(ribbon: &Ribbon) -> iced::widget::Row<'static, Message> {
    let tools = layer_tools();
    let mut r = row![].spacing(3).align_y(iced::Center);
    if let Some(m) = &tools.manager {
        r = r.push(item_el(&plain(m), false));
    }
    r = r.push(layer_combo(ribbon));
    r = r.push(separator(false));
    for c in &tools.commands {
        r = r.push(item_el(&plain(c), false));
    }
    r = r.push(separator(false));
    let (color_swatch, _) = acad_color_display(ribbon.active_color);
    r = r.push(prop_combo(
        ribbon,
        PROP_COLOR_ID,
        Some(color_swatch),
        &crate::ui::color_select::color_display_name(ribbon.active_color),
    ));
    r = r.push(prop_combo(
        ribbon,
        PROP_LINETYPE_ID,
        None,
        &linetype_display_name(&ribbon.active_linetype),
    ));
    r.push(prop_combo(
        ribbon,
        PROP_LW_ID,
        None,
        &LwItem(ribbon.active_lineweight).to_string(),
    ))
}
```

e ripulire gli `use` non più necessari (`scrollable`, `Length` se non usati altrove, `strip_style`): lasciare che `cargo check` indichi i warning e rimuoverli.

- [ ] **Step 4: Creare `src/ui/toolbar_dock.rs`**

Aggiungere in `src/ui/mod.rs` dopo `pub mod toolbar_layout;`: `pub mod toolbar_dock;`.

```rust
//! ArchLine: rendering of the dockable classic toolbars.
//!
//! Window layout: menu, tabs, **Top edge**, `[Left edge | centre | Right edge]`,
//! **Bottom edge**, status bar. Each edge is a stack of *lanes*; a lane is a
//! row (or column) of bars side by side. Lane 0 hugs the window edge.
//!
//! The model (`toolbar_layout`) decides where bars are; this module only draws.
//! Kept out of `view_main` on purpose: that function is so large that extra
//! nesting there can overflow rustc's stack on Windows release builds.

use iced::widget::{column, container, mouse_area, row, scrollable, text, Space};
use iced::{Background, Border, Color, Element, Length, Point, Theme};

use super::classic_layers::layer_row;
use super::classic_toolbar::{item_el, items_for, strip_style, ClassicItem, BTN_SIZE};
use super::ribbon::Ribbon;
use super::toolbar_layout::{Edge, ToolbarId, ToolbarLayout};
use crate::app::Message;

/// Offset from the pointer to a dragged bar's top-left: the user holds the
/// grip, which sits in the bar's corner.
pub const GRIP_ANCHOR: f32 = 10.0;
const GRIP_W: f32 = 8.0;
/// Estimated length of the layer / properties bar (combo 220, 3×130 combos,
/// 11 buttons, separators, spacing).
const LAYERS_LENGTH: f32 = 1050.0;

#[derive(Clone, Debug)]
pub enum ToolbarMsg {
    /// The grip (or a floating bar's title) was pressed.
    Grab(ToolbarId),
    /// Pointer moved while dragging (position in the toolbar frame).
    DragMove(Point),
    DragRelease,
    /// Double click on a floating bar's title.
    Redock(ToolbarId),
    /// Put every bar back where it was at first launch.
    Reset,
}

/// Estimated length of a bar along its own axis (px), grip included.
pub fn bar_length(id: ToolbarId) -> f32 {
    if id == ToolbarId::Layers {
        return LAYERS_LENGTH;
    }
    let items: f32 = items_for(id)
        .iter()
        .map(|i| match i {
            ClassicItem::Button(_) => BTN_SIZE + 3.0,
            ClassicItem::Separator => 4.0,
        })
        .sum();
    items + GRIP_W + 12.0
}

/// `(width, height)` of a bar.
pub fn bar_size(id: ToolbarId, vertical: bool) -> (f32, f32) {
    let l = bar_length(id);
    let t = super::toolbar_layout::LANE_THICKNESS;
    if vertical {
        (t, l)
    } else {
        (l, t)
    }
}

fn grip(id: ToolbarId, vertical: bool) -> Element<'static, Message> {
    let line = || {
        container(Space::new()).style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.neutral.color)),
            ..Default::default()
        })
    };
    let span = Length::Fixed(BTN_SIZE - 8.0);
    let lines: Element<'static, Message> = if vertical {
        column![line().width(span).height(2), line().width(span).height(2)]
            .spacing(2)
            .into()
    } else {
        row![line().width(2).height(span), line().width(2).height(span)]
            .spacing(2)
            .into()
    };
    mouse_area(container(lines).padding(2))
        .on_press(Message::Toolbar(ToolbarMsg::Grab(id)))
        .interaction(iced::mouse::Interaction::Grab)
        .into()
}

fn bar_body(id: ToolbarId, vertical: bool, ribbon: &Ribbon) -> Element<'static, Message> {
    if id == ToolbarId::Layers {
        // Combo boxes: horizontal only (the model never docks it on a side).
        return layer_row(ribbon).into();
    }
    let items = items_for(id);
    if vertical {
        column(items.iter().map(|i| item_el(i, true))).spacing(3).into()
    } else {
        row(items.iter().map(|i| item_el(i, false)))
            .spacing(3)
            .align_y(iced::Center)
            .into()
    }
}

/// One docked bar: grip + buttons. `being_dragged` tints it while its ghost
/// follows the pointer.
fn bar_el(
    id: ToolbarId,
    vertical: bool,
    ribbon: &Ribbon,
    being_dragged: bool,
) -> Element<'static, Message> {
    let body = bar_body(id, vertical, ribbon);
    let inner: Element<'static, Message> = if vertical {
        column![grip(id, true), body].spacing(3).into()
    } else {
        row![grip(id, false), body]
            .spacing(3)
            .align_y(iced::Center)
            .into()
    };
    container(inner)
        .padding(2)
        .style(move |theme: &Theme| {
            let p = theme.palette();
            container::Style {
                background: being_dragged
                    .then(|| Background::Color(p.background.weakest.color.scale_alpha(0.5))),
                border: Border {
                    color: if being_dragged {
                        p.primary.base.color
                    } else {
                        Color::TRANSPARENT
                    },
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            }
        })
        .into()
}

fn lane_el(
    bars: &[ToolbarId],
    vertical: bool,
    ribbon: &Ribbon,
    dragging: Option<ToolbarId>,
) -> Element<'static, Message> {
    let els: Vec<Element<'static, Message>> = bars
        .iter()
        .map(|&id| bar_el(id, vertical, ribbon, dragging == Some(id)))
        .collect();
    if vertical {
        container(scrollable(column(els).spacing(3)))
            .padding(3)
            .height(Length::Fill)
            .style(strip_style)
            .into()
    } else {
        container(
            scrollable(row(els).spacing(3).align_y(iced::Center)).direction(
                scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::new().width(4).scroller_width(4),
                ),
            ),
        )
        .padding(3)
        .width(Length::Fill)
        .style(strip_style)
        .into()
    }
}

fn edge_el(
    layout: &ToolbarLayout,
    edge: Edge,
    ribbon: &Ribbon,
    dragging: Option<ToolbarId>,
) -> Element<'static, Message> {
    let mut lanes = layout.lanes(edge);
    if lanes.is_empty() {
        return Space::new().width(0).height(0).into();
    }
    // Lane 0 hugs the window edge: it is last on the bottom and right edges.
    if matches!(edge, Edge::Bottom | Edge::Right) {
        lanes.reverse();
    }
    let vertical = edge.is_vertical();
    let els: Vec<Element<'static, Message>> = lanes
        .iter()
        .map(|l| lane_el(l, vertical, ribbon, dragging))
        .collect();
    if vertical {
        row(els).height(Length::Fill).into()
    } else {
        column(els).into()
    }
}

/// Surround `center` with the four toolbar edges when `classic` is set;
/// otherwise return it untouched.
#[inline(never)]
pub fn frame<'a>(
    classic: bool,
    layout: &ToolbarLayout,
    ribbon: &Ribbon,
    dragging: Option<ToolbarId>,
    center: Element<'a, Message>,
) -> Element<'a, Message> {
    if !classic {
        return center;
    }
    let middle = row![
        edge_el(layout, Edge::Left, ribbon, dragging),
        container(center).width(Length::Fill).height(Length::Fill),
        edge_el(layout, Edge::Right, ribbon, dragging),
    ]
    .height(Length::Fill);
    column![
        edge_el(layout, Edge::Top, ribbon, dragging),
        middle,
        edge_el(layout, Edge::Bottom, ribbon, dragging),
    ]
    .into()
}
```

Nota: `text` è importato per il Task 5; se `cargo check` segnala `unused import`, rimuoverlo qui e riaggiungerlo nel Task 5.

- [ ] **Step 5: Il messaggio e l'aggancio nella vista**

`src/app/mod.rs`, dopo `Dock(crate::ui::dock::DockMsg),` (riga ~3636):

```rust
    /// A drag / redock / reset on the classic toolbars.
    Toolbar(crate::ui::toolbar_dock::ToolbarMsg),
```

`src/app/update/mod.rs`, accanto a `Message::Dock(m) => self.on_dock(m),` aggiungere provvisoriamente (il handler vero è nel Task 4):

```rust
            Message::Toolbar(_) => Task::none(),
```

`src/app/view/mod.rs`, sostituire le righe 2192–2197:

```rust
            if classic {
                col = col.push(crate::ui::classic_toolbar::top_bar());
                col = col.push(crate::ui::classic_layers::layer_bar(&self.ribbon));
            }
            let center_stack = crate::ui::classic_toolbar::wrap_center(classic, center_stack);
            col.push(center_stack)
```

con:

```rust
            let center_stack = crate::ui::toolbar_dock::frame(
                classic,
                &self.toolbars,
                &self.ribbon,
                None,
                center_stack,
            );
            col.push(center_stack)
```

- [ ] **Step 6: Rimuovere il codice che non serve più**

In `src/ui/classic_toolbar.rs` eliminare `top_bar`, `wrap_center`, `vertical`, `horizontal` e il campo `extra` (e la sua riga in `tools()`). Nei test sostituire:
- in `draw_toolbar_is_compact_and_other_bars_are_populated`: `assert!(!buttons(&t.extra).is_empty());` → `assert!(!buttons(&t.annotation).is_empty());`
- in `no_main_button_is_duplicated`: `for list in [&t.draw, &t.modify, &t.extra]` → `for list in [&t.draw, &t.modify, &t.annotation, &t.block, &t.measure]`
- eliminare il test `annotation_block_and_measure_are_separate_bars_with_buttons` nella parte che confronta con `extra` (le ultime 6 righe, da `// The three lists together` in poi), tenendo il resto.

Aggiornare il commento di testa del file: "(Draw left, Modify right, extras on top)" → "(dockable: see `toolbar_layout`)".

- [ ] **Step 7: Test di vista (bordi vuoti compresi)**

In `src/app/view/classic.rs`, nel modulo `tests` aggiungere:

```rust
    #[test]
    fn the_toolbar_frame_builds_for_default_and_all_floating_layouts() {
        use crate::ui::toolbar_layout::{Target, ToolbarId};
        let mut app = OpenCADStudio::new_for_test();
        app.toolbars = Default::default();
        let center: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let el = crate::ui::toolbar_dock::frame(true, &app.toolbars, &app.ribbon, None, center);
        assert_eq!(el.as_widget().size().height, iced::Length::Fill);

        // Review focus: every bar floating leaves all four edges empty.
        for (i, id) in ToolbarId::ALL.into_iter().enumerate() {
            app.toolbars.move_to(id, Target::Float { x: 10.0 * i as f32, y: 10.0 });
        }
        let center: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let _ = crate::ui::toolbar_dock::frame(true, &app.toolbars, &app.ribbon, None, center);

        // Not classic: the centre comes back untouched.
        let center: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let el = crate::ui::toolbar_dock::frame(false, &app.toolbars, &app.ribbon, None, center);
        assert_eq!(el.as_widget().size().width, iced::Length::Shrink);
    }
```

- [ ] **Step 8: Eseguire i test e il controllo**

Run: `cargo check --locked 2>&1 | tail -15` → nessun errore (rimuovere gli import inutilizzati segnalati).
Run: `cargo test --locked --lib classic 2>&1 | tail -10` → PASS.
Run: `cargo test --locked --lib toolbar 2>&1 | tail -6` → PASS.

- [ ] **Step 9: Commit**

```bash
git add -A src/ui src/app
git commit -m "Barre classiche: bordi a lane da layout, Annotation/Block/Measure separate"
```

Verifica visiva (utente, dopo la build del Task 6): con layout di default l'aspetto deve essere quello di prima, più un'impugnatura a punti sul bordo di ogni barra.

---

### Task 4: Trascinamento e aggancio a un bordo

**Files:**
- Create: `src/app/update/toolbar.rs`
- Modify: `src/app/update/mod.rs` (`mod toolbar;`, routing, Esc)
- Modify: `src/app/mod.rs` (campo `toolbar_drag`)
- Modify: `src/ui/toolbar_dock.rs` (`ToolbarDrag`, `decorate`, anteprima)
- Modify: `src/app/view/mod.rs` (aggancio di `decorate` prima di `composed`; `dragging` reale in `frame`)
- Test: `src/app/update/toolbar.rs`

**Interfaces:**
- Consumes: `ToolbarMsg`, `bar_length`, `bar_size`, `GRIP_ANCHOR` (Task 3); `resolve_drop`, `clamp_floating`, `band_rect`, `DropCtx` (Task 1).
- Produces: `ToolbarDrag { id: ToolbarId, cursor: Option<Point>, target: Option<Target> }`; `OpenCADStudio.toolbar_drag: Option<ToolbarDrag>`; `toolbar_dock::decorate(base, layout, ribbon, drag, win, classic)`.

- [ ] **Step 1: Scrivere i test del handler (falliscono)**

Creare `src/app/update/toolbar.rs`:

```rust
//! ArchLine: update handling for the dockable classic toolbars.

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};
    use crate::ui::toolbar_dock::ToolbarMsg;
    use crate::ui::toolbar_layout::{DockSlot, Edge, Placement, ToolbarId, ToolbarLayout};
    use iced::Point;

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        // Tests load the user's persisted config; reset to a known layout.
        app.toolbars = ToolbarLayout::default();
        app.win_size = (1600.0, 900.0);
        app
    }

    #[test]
    fn dragging_a_bar_to_the_left_edge_docks_it_there() {
        let mut app = app();
        let _ = app.update(Message::Toolbar(ToolbarMsg::Grab(ToolbarId::Block)));
        assert!(app.toolbar_drag.is_some());
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragMove(Point::new(8.0, 400.0))));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragRelease));
        assert!(app.toolbar_drag.is_none());
        assert_eq!(
            app.toolbars.placement(ToolbarId::Block),
            Placement::Docked(DockSlot { edge: Edge::Left, lane: 0, index: 1 })
        );
    }

    #[test]
    fn dragging_into_the_middle_floats_the_bar_inside_the_window() {
        let mut app = app();
        let _ = app.update(Message::Toolbar(ToolbarMsg::Grab(ToolbarId::Draw)));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragMove(Point::new(800.0, 450.0))));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragRelease));
        match app.toolbars.placement(ToolbarId::Draw) {
            Placement::Floating { x, y, .. } => {
                assert!(x >= 0.0 && y >= 0.0);
                assert!(x < 800.0 && y < 450.0, "top-left is offset from the pointer");
            }
            other => panic!("expected floating, got {other:?}"),
        }
    }

    #[test]
    fn a_click_on_the_grip_without_moving_changes_nothing() {
        // Review focus 5.
        let mut app = app();
        let before = app.toolbars.clone();
        let _ = app.update(Message::Toolbar(ToolbarMsg::Grab(ToolbarId::Modify)));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragRelease));
        assert_eq!(app.toolbars, before);
        assert!(app.toolbar_drag.is_none());
    }

    #[test]
    fn escape_cancels_a_drag_and_keeps_the_layout() {
        let mut app = app();
        let before = app.toolbars.clone();
        let _ = app.update(Message::Toolbar(ToolbarMsg::Grab(ToolbarId::Draw)));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragMove(Point::new(800.0, 450.0))));
        let _ = app.update(Message::CommandEscape);
        assert!(app.toolbar_drag.is_none());
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragRelease));
        assert_eq!(app.toolbars, before);
    }

    #[test]
    fn redock_and_reset_restore_positions() {
        let mut app = app();
        let _ = app.update(Message::Toolbar(ToolbarMsg::Grab(ToolbarId::Layers)));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragMove(Point::new(800.0, 450.0))));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragRelease));
        assert!(matches!(
            app.toolbars.placement(ToolbarId::Layers),
            Placement::Floating { .. }
        ));
        let _ = app.update(Message::Toolbar(ToolbarMsg::Redock(ToolbarId::Layers)));
        assert_eq!(
            app.toolbars.placement(ToolbarId::Layers),
            Placement::Docked(DockSlot { edge: Edge::Top, lane: 1, index: 0 })
        );
        let _ = app.update(Message::Toolbar(ToolbarMsg::Grab(ToolbarId::Draw)));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragMove(Point::new(800.0, 450.0))));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragRelease));
        let _ = app.update(Message::Toolbar(ToolbarMsg::Reset));
        assert_eq!(app.toolbars, ToolbarLayout::default());
    }
}
```

Run: `cargo test --locked --lib update::toolbar 2>&1 | tail -8` → FAIL (nessun `toolbar_drag`, modulo non dichiarato).

- [ ] **Step 2: Stato e handler**

`src/app/mod.rs`, dopo `toolbars` (Task 2):

```rust
    /// Bar being dragged by its grip, with the live drop target.
    pub(crate) toolbar_drag: Option<crate::ui::toolbar_dock::ToolbarDrag>,
```
e nell'inizializzazione `toolbar_drag: None,`.

`src/ui/toolbar_dock.rs`, dopo `ToolbarMsg`:

```rust
/// Transient state of a toolbar drag.
#[derive(Clone, Debug)]
pub struct ToolbarDrag {
    pub id: ToolbarId,
    /// Last pointer position, in the toolbar frame; `None` until it moves.
    pub cursor: Option<Point>,
    /// Where releasing now would put the bar; `None` until it moves.
    pub target: Option<super::toolbar_layout::Target>,
}
```

`src/app/update/mod.rs`: dichiarare `mod toolbar;` accanto agli altri `mod` (riga ~99–103), sostituire il routing provvisorio con:

```rust
            Message::Toolbar(m) => self.on_toolbar(m),
```
e **all'inizio** di `update_message` (prima di `if let Some(tab) = self.tabs.get(self.active_tab) {`):

```rust
        // Esc abandons a toolbar drag before it reaches the command line.
        if self.toolbar_drag.is_some()
            && (matches!(msg, Message::CommandEscape)
                || matches!(&msg, Message::ShortcutPressed(key) if key.rsplit('+').next() == Some("ESCAPE")))
        {
            self.toolbar_drag = None;
            return Task::none();
        }
```

Inserire in `src/app/update/toolbar.rs`, sopra il modulo `tests`:

```rust
use crate::app::{Message, OpenCADStudio};
use crate::ui::toolbar_dock::{bar_length, bar_size, ToolbarDrag, ToolbarMsg, GRIP_ANCHOR};
use crate::ui::toolbar_layout::{clamp_floating, resolve_drop, DropCtx, Target};

impl OpenCADStudio {
    pub(super) fn on_toolbar(&mut self, m: ToolbarMsg) -> iced::Task<Message> {
        match m {
            ToolbarMsg::Grab(id) => {
                self.toolbar_drag = Some(ToolbarDrag {
                    id,
                    cursor: None,
                    target: None,
                });
            }
            ToolbarMsg::DragMove(p) => {
                if let Some(drag) = &mut self.toolbar_drag {
                    let win = self.win_size;
                    let size = bar_size(drag.id, false);
                    let float_at = clamp_floating(
                        (p.x - GRIP_ANCHOR, p.y - GRIP_ANCHOR),
                        size,
                        win,
                    );
                    let ctx = DropCtx {
                        win,
                        length: &bar_length,
                    };
                    drag.cursor = Some(p);
                    drag.target = Some(resolve_drop(
                        &self.toolbars,
                        drag.id,
                        (p.x, p.y),
                        float_at,
                        &ctx,
                    ));
                }
            }
            ToolbarMsg::DragRelease => {
                if let Some(drag) = self.toolbar_drag.take() {
                    if let Some(target) = drag.target {
                        self.toolbars.move_to(drag.id, target);
                        self.save_config();
                    }
                }
            }
            ToolbarMsg::Redock(id) => {
                self.toolbars.redock(id);
                self.save_config();
            }
            ToolbarMsg::Reset => {
                self.toolbars = Default::default();
                self.toolbar_drag = None;
                self.save_config();
            }
        }
        iced::Task::none()
    }
}
```

Nota: `Target` serve solo se il compilatore lo richiede; rimuovere l'import se inutilizzato.

- [ ] **Step 3: Livello di drag e anteprima in `toolbar_dock.rs`**

Aggiungere in fondo a `src/ui/toolbar_dock.rs` (prima di eventuali test), e aggiungere agli `use`: `mouse_area` è già importato; aggiungere `use super::toolbar_layout::{band_rect, clamp_floating, Target};` e `use iced::widget::{opaque, pin, Stack};`:

```rust
fn block_el(x: f32, y: f32, w: f32, h: f32, fill_alpha: f32) -> Element<'static, Message> {
    pin(container(Space::new())
        .width(Length::Fixed(w))
        .height(Length::Fixed(h))
        .style(move |theme: &Theme| {
            let c = theme.palette().primary.base.color;
            container::Style {
                background: Some(Background::Color(c.scale_alpha(fill_alpha))),
                border: Border {
                    color: c,
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            }
        }))
    .position(Point::new(x.max(0.0), y.max(0.0)))
    .into()
}

/// The highlighted lane the bar would dock into, plus a ghost outline
/// following the pointer.
fn drag_visuals(d: &ToolbarDrag, win: (f32, f32)) -> Vec<Element<'static, Message>> {
    let Some(cursor) = d.cursor else {
        return Vec::new();
    };
    let mut v = Vec::new();
    if let Some(Target::Dock(slot)) = d.target {
        let (x, y, w, h) = band_rect(slot, win);
        v.push(block_el(x, y, w, h, 0.18));
    }
    let vertical = matches!(d.target, Some(Target::Dock(s)) if s.edge.is_vertical());
    let (w, h) = bar_size(d.id, vertical);
    v.push(block_el(cursor.x - GRIP_ANCHOR, cursor.y - GRIP_ANCHOR, w, h, 0.10));
    v
}

/// Wrap the main window content with the floating bars (Task 5) and, while a
/// bar is being dragged, the full-size pointer tracker + previews.
#[inline(never)]
pub fn decorate<'a>(
    base: Element<'a, Message>,
    _layout: &ToolbarLayout,
    _ribbon: &Ribbon,
    drag: Option<&ToolbarDrag>,
    win: (f32, f32),
    classic: bool,
) -> Element<'a, Message> {
    if !classic {
        return base;
    }
    let mut layers: Vec<Element<'a, Message>> = vec![base];
    if let Some(d) = drag {
        layers.extend(drag_visuals(d, win));
    }
    let stacked: Element<'a, Message> = Stack::with_children(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    if drag.is_some() {
        mouse_area(stacked)
            .on_move(|p| Message::Toolbar(ToolbarMsg::DragMove(p)))
            .on_release(Message::Toolbar(ToolbarMsg::DragRelease))
            .interaction(iced::mouse::Interaction::Grabbing)
            .into()
    } else {
        stacked
    }
}
```

(`opaque` e `text` servono al Task 5; se il compilatore segnala import inutilizzati, rimuoverli ora e riaggiungerli lì.)

- [ ] **Step 4: Aggancio in `view_main`**

`src/app/view/mod.rs`: nel blocco di Task 3 passare il vero `dragging`:

```rust
            let center_stack = crate::ui::toolbar_dock::frame(
                classic,
                &self.toolbars,
                &self.ribbon,
                self.toolbar_drag.as_ref().map(|d| d.id),
                center_stack,
            );
```

e prima di `let composed = stack![` (riga ~2397) inserire:

```rust
        let main_ui = crate::ui::toolbar_dock::decorate(
            main_ui.into(),
            &self.toolbars,
            &self.ribbon,
            self.toolbar_drag.as_ref(),
            self.win_size,
            crate::workspace::classic_active(
                self.tabs[self.active_tab].is_start,
                self.clean_screen,
            ),
        );
```

- [ ] **Step 5: Eseguire i test**

Run: `cargo test --locked --lib update::toolbar 2>&1 | tail -12` → PASS (5 test).
Run: `cargo check --locked 2>&1 | tail -5` → nessun errore.
Run: `cargo test --locked --lib classic 2>&1 | tail -4` → PASS.

Se `dragging_a_bar_to_the_left_edge_docks_it_there` fallisce sull'indice: con barre da `bar_length` reali e `along = y - TOP_CHROME = 338` l'indice dipende dalla lunghezza di Draw; controllare `Placement` reale e correggere l'atteso a `index: 1` o `0` coerentemente con `bar_length(Draw)` (≈ 12 pulsanti × 39 + 20 ≈ 490 → il punto medio 245 < 338 → indice 1).

- [ ] **Step 6: Commit**

```bash
git add -A src/ui src/app
git commit -m "Barre classiche: trascinamento dall'impugnatura e aggancio ai bordi"
```

---

### Task 5: Barre flottanti

**Files:**
- Modify: `src/ui/toolbar_dock.rs` (`floating_el`, `floating_layer`, uso in `decorate`)
- Test: `src/app/view/classic.rs`

**Interfaces:**
- Consumes: `ToolbarLayout::floating`, `clamp_floating`, `bar_size`, `ToolbarMsg::{Grab, Redock}`.
- Produces: `decorate` ora disegna anche le barre flottanti (sopra il centro, sotto i dropdown e le finestre modali).

- [ ] **Step 1: Test di vista (fallisce)**

In `src/app/view/classic.rs`, nel modulo `tests`:

```rust
    #[test]
    fn decorate_draws_floating_bars_inside_the_window() {
        use crate::ui::toolbar_layout::{Target, ToolbarId};
        let mut app = OpenCADStudio::new_for_test();
        app.toolbars = Default::default();
        // Far outside a small window: clamped at render, never lost.
        app.toolbars.move_to(ToolbarId::Layers, Target::Float { x: 5000.0, y: 4000.0 });
        let base: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let el = crate::ui::toolbar_dock::decorate(
            base,
            &app.toolbars,
            &app.ribbon,
            None,
            (800.0, 600.0),
            true,
        );
        assert_eq!(el.as_widget().size().width, iced::Length::Fill);
        let (x, y) = crate::ui::toolbar_layout::clamp_floating(
            (5000.0, 4000.0),
            crate::ui::toolbar_dock::bar_size(ToolbarId::Layers, false),
            (800.0, 600.0),
        );
        assert!(x <= 800.0 && y <= 600.0);
    }
```

Run: `cargo test --locked --lib decorate_draws_floating 2>&1 | tail -8`. Con il `decorate` del Task 4 il test **compila e passa già** per la parte di dimensioni (fallisce solo se si rompe qualcosa): serve come rete di sicurezza per lo step successivo. Il comportamento visivo si verifica con la build.

- [ ] **Step 2: Implementare le barre flottanti**

In `src/ui/toolbar_dock.rs`, aggiungere:

```rust
fn floating_el(
    id: ToolbarId,
    ribbon: &Ribbon,
    being_dragged: bool,
) -> Element<'static, Message> {
    let title = mouse_area(
        container(text(crate::t!(id.title()).into_owned()).size(11))
            .padding([2, 6])
            .width(Length::Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.strong.color)),
                text_color: Some(theme.palette().background.strong.text),
                ..Default::default()
            }),
    )
    .on_press(Message::Toolbar(ToolbarMsg::Grab(id)))
    .on_double_click(Message::Toolbar(ToolbarMsg::Redock(id)))
    .interaction(iced::mouse::Interaction::Grab);
    container(column![title, bar_body(id, false, ribbon)].spacing(2))
        .padding(2)
        .style(move |theme: &Theme| {
            let p = theme.palette();
            container::Style {
                background: Some(Background::Color(p.background.weak.color)),
                border: Border {
                    color: if being_dragged {
                        p.primary.base.color
                    } else {
                        p.background.neutral.color
                    },
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        })
        .into()
}

fn floating_layer(
    layout: &ToolbarLayout,
    ribbon: &Ribbon,
    win: (f32, f32),
    dragging: Option<ToolbarId>,
) -> Option<Element<'static, Message>> {
    let bars = layout.floating();
    if bars.is_empty() {
        return None;
    }
    let layers: Vec<Element<'static, Message>> = bars
        .into_iter()
        .map(|(id, x, y)| {
            // A floating bar is a title strip over a horizontal body.
            let (w, h) = bar_size(id, false);
            let (cx, cy) = clamp_floating((x, y), (w, h + 18.0), win);
            pin(opaque(floating_el(id, ribbon, dragging == Some(id))))
                .position(Point::new(cx, cy))
                .into()
        })
        .collect();
    Some(
        Stack::with_children(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    )
}
```

e in `decorate`, sostituire i parametri `_layout` e `_ribbon` con `layout` e `ribbon` e, dopo `let mut layers = vec![base];`:

```rust
    if let Some(floating) = floating_layer(layout, ribbon, win, drag.map(|d| d.id)) {
        layers.push(floating);
    }
```

- [ ] **Step 3: Eseguire i test**

Run: `cargo check --locked 2>&1 | tail -8` → nessun errore.
Run: `cargo test --locked --lib classic 2>&1 | tail -6` → PASS.
Run: `cargo test --locked --lib toolbar 2>&1 | tail -6` → PASS.

- [ ] **Step 4: Commit**

```bash
git add -A src/ui src/app
git commit -m "Barre classiche: barre flottanti con ritorno alla posizione agganciata"
```

---

### Task 6: Comando di ripristino, spec allineata, build e verifica

**Files:**
- Modify: `src/app/commands/display.rs` (arm `TOOLBARRESET`, accanto a `CLEANSCREEN`, ~riga 576)
- Modify: `src/app/commands/mod.rs` (nome in elenco, accanto a `"CLEANSCREEN"`, ~riga 458)
- Modify: `docs/superpowers/specs/2026-10-06-barre-sganciabili-design.md`

**Interfaces:**
- Consumes: `Message::Toolbar(ToolbarMsg::Reset)` (Task 3/4).
- Produces: comando `TOOLBARRESET` dalla riga di comando.

- [ ] **Step 1: Comando**

`src/app/commands/display.rs`, subito dopo l'arm `"CLEANSCREEN" => { ... }`:

```rust
            // ── TOOLBARRESET — put the classic toolbars back where they started ──
            "TOOLBARRESET" => {
                return Some(Task::done(Message::Toolbar(
                    crate::ui::toolbar_dock::ToolbarMsg::Reset,
                )));
            }
```

`src/app/commands/mod.rs`, nell'elenco `names: &[` aggiungere dopo `"CLEANSCREEN",`:

```rust
        "TOOLBARRESET",
```

- [ ] **Step 2: Allineare la spec a quanto realizzato**

In `docs/superpowers/specs/2026-10-06-barre-sganciabili-design.md`:
- §6 messaggi: sostituire "`ToolbarGrab(id, offset)`, `ToolbarDragMove(Point)`, `ToolbarDragRelease`, `ToolbarDragCancel` (`Esc`)" con "`ToolbarMsg::{Grab(id), DragMove(Point), DragRelease, Redock(id), Reset}`; `Esc` annulla il drag da `update_message` (stato `toolbar_drag`)". Aggiungere: "Il punto di presa non è noto (`mouse_area::on_press` non dà la posizione): la barra flottante si posiziona con l'angolo a `GRIP_ANCHOR` px dal cursore."
- §6 "attenuata": sostituire con "evidenziata con sfondo semitrasparente e bordo di accento".
- §8 fase 4: aggiungere "comando `TOOLBARRESET`".

- [ ] **Step 3: Suite completa e build**

Run: `cargo test --locked --lib 2>&1 | tail -15` → tutti PASS (se un test non legato alle barre fallisce, controllare con `git stash` se fallisce anche senza le modifiche prima di intervenire).
Run: `cargo build --release --bin OpenCADStudio 2>&1 | tail -4` → `Finished`. Se compare "Accesso negato", chiedere all'utente di chiudere ArchLine.

- [ ] **Step 4: Commit**

```bash
git add -A src docs
git commit -m "Barre classiche: comando TOOLBARRESET, spec allineata"
```

- [ ] **Step 5: Verifica visiva (utente)**

Chiedere all'utente di avviare `target\release\OpenCADStudio.exe` e controllare, con screenshot:
1. Layout iniziale uguale a prima, con le impugnature.
2. Trascinare Draw sul bordo alto, sul basso, a destra: si aggancia, cambia orientamento, la linea di anteprima sta sul bordo giusto.
3. Trascinare Annotation accanto a Layers: finiscono nella stessa riga; trascinare una barra tra due lane.
4. Trascinare una barra in mezzo alla finestra: diventa flottante; doppio clic sul titolo la riporta.
5. **Layers flottante**: aprire i combo Colore/Tipo linea/Spessore e il combo layer: i menu devono comparire sotto il bottone (rischio §7.1 della spec). Se compaiono nel posto sbagliato, riferire.
6. Tasto destro su un pulsante con varianti, in una barra verticale e in una flottante: il flyout si apre.
7. Chiudere e riaprire l'app: posizioni ricordate. Riga di comando: `TOOLBARRESET` riporta tutto al default.
8. Tarare `TOP_CHROME`, `BOTTOM_CHROME`, `SNAP_BAND` se la zona di aggancio non coincide con quanto si vede.

---

## Self-review (fatta)

- **Copertura spec:** §3 modello → Task 1; §4 persistenza → Task 2; §5 rendering e separazione Annotation/Block/Measure → Task 3; §6 trascinamento, anteprima, Esc → Task 4; flottanti e ritorno a `home` → Task 5; comando di ripristino e fase 4 → Task 6; §7 rischi → verifica visiva Task 6 step 5; §8 test → distribuiti nei task.
- **Placeholder:** nessuno. Due note condizionali ("se il compilatore segnala import inutilizzati", "se un gruppo non ha pulsanti riferire") sono istruzioni di verifica con azione definita.
- **Coerenza dei tipi:** `ToolbarId`, `Edge`, `DockSlot`, `Placement`, `Target`, `ToolbarLayout`, `DropCtx`, `resolve_drop(layout, dragged, cursor, float_at, ctx)`, `clamp_floating(pos, size, win)`, `band_rect(slot, win)`, `ToolbarMsg`, `ToolbarDrag`, `frame(classic, layout, ribbon, dragging, center)`, `decorate(base, layout, ribbon, drag, win, classic)` hanno la stessa firma ovunque compaiano.
- **Review Focus:** 1 → Task 1 (`serde_round_trip_and_old_settings`, `sanitized_drops_unknown_bars…`) e Task 2; 2 → Task 1 (`out_of_range_lane_and_index_are_clamped`); 3 → Task 1 (`layers_never_docks…`, `resolve_drop_never_offers…`); 4 → Task 1 (`clamp_keeps…`) e Task 5; 5 → Task 4 (`a_click_on_the_grip…`); 6 → Task 3 (test di vista).
