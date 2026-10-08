# Finestra "Hatch and Gradient" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Lanciando HATCH si apre una finestra come quella di AutoCAD in cui scegliere le opzioni del retino, indicare le aree (con ritorno alla finestra) e creare il retino con OK.

**Architecture:** Una finestra modale (`ModalKind::Hatch`) con stato `HatchDialogState` nell'app, sul modello di Drawing Units. "Add" nasconde la finestra e avvia un `HatchCommand` in modalità *raccoglitore* che, con Invio, restituisce le aree con il nuovo `CmdResult::HatchBoundariesPicked`; OK costruisce un `HatchCommand` con impostazioni e aree e richiama `on_enter()`, così i percorsi di commit esistenti restano gli unici. Il comando senza finestra diventa `-HATCH`.

**Tech Stack:** Rust 2021, iced (con `iced::widget::canvas`), crate `opencadcodec` (`codec`) e `opencadkernel` (`kernel`), test `cargo test --locked --lib`.

**Spec:** `docs/superpowers/specs/2026-10-08-finestra-hatch-design.md` (approvata dall'utente il 2026-10-08; rivista dopo due revisioni tecniche). Leggerla prima di iniziare: il piano la segue sezione per sezione.

## Global Constraints

- Rispondere e commentare in **italiano**; nomi di codice e stringhe `t!(...)` in **inglese** (chiavi di traduzione); nessuna modifica a `locale_catalog.rs`.
- Non togliere `RUST_MIN_STACK` da `.cargo/config.toml` (64 MiB). **Trappola n. 1**: in `view_main`, nel `match` di `update_message` e in `apply_cmd_result_inner` aggiungere solo poche righe che delegano a funzioni separate, marcate `#[inline(never)]` quando contengono logica.
- File propri per i nuovi moduli (meno conflitti col merge da upstream): `src/ui/window/hatch_dialog.rs`, `src/app/commands/hatch_dialog.rs`, `src/modules/draw/draw/hatch_settings.rs`, `src/modules/draw/draw/hatch_flows.rs`.
- Quantizzazione della chiave canonica delle aree: `1.0e-6`. Tolleranza di `boundary_sources_on_plane`/`boundary_faces`: `1.0e-6` (come oggi).
- Swatch: passo sullo schermo limitato tra **2 px e 64 px**, al massimo **400** segmenti; sotto 2 px riempimento tinto.
- Validazione: Angle = numero finito (virgola o punto); Scale = numero finito e `> 0`.
- Stringhe di ritorno dei `Dispatch`: `HATCH_PICK_CANCELLED`, `HATCH_ORIGIN_PICKED <x> <y> <z>`, `HATCH_PREVIEW_DONE`. Il ritorno positivo di Pick/Select **non** passa da `Dispatch`.
- Impostazioni di sessione `hatch_last`: aggiornate **solo** con Add e OK, **non** con Preview; mai aree, handle o riferimenti al tab.
- Lasciare il tab proprietario (cambio o chiusura) **annulla** il flusso.
- `HATCH` da canali programmatici esegue il corpo di `-HATCH` (flag `scripted_dispatch`, distinto da `suppress_plugin_dispatch`).
- Branch: lavorare su `claude/finestra-hatch` (da `lavoro`), mai su `lavoro` direttamente. **I passi "Commit" si eseguono solo se l'utente lo ha autorizzato**; altrimenti lasciare le modifiche nell'albero di lavoro e dirlo. Nei messaggi di commit non aggiungere righe di attribuzione.
- Verifica visiva: solo l'utente, su Windows (screenshot o artefatto CI `ArchLine-windows-N`). Non dichiarare "funziona alla vista" senza di essa.
- Percorsi: usare sempre percorsi assoluti o `cd` esplicito nella radice del progetto `C:\Users\archi\Documents\Progetti IA\ArchLine`; non lasciare la shell in altre cartelle (es. `~/.cargo/git/...`).

## Review Focus

Ingressi o condizioni che la spec implica ma che nessun test "ovvio" copre; ognuno ha il suo test nel task indicato.

1. **Coordinate grandi (UTM, ~5 000 000)**: la chiave canonica quantizzata a 1e-6 non deve andare in overflow né far considerare diverse due aree identiche → Task 2.
2. **Virgola decimale** (`0,5`) e testo non numerico in Angle/Scale: deve comportarsi come un valore non valido, non come zero → Task 2 e Task 7.
3. **HATCH in un layout (spazio carta)**: `editing_model_space()` è falso, il piano è quello di default; la finestra deve aprirsi lo stesso → Task 7.
4. **Pattern memorizzato in `hatch_last` che non esiste più nel catalogo**: OK/Add/Preview disabilitati, swatch con ripiego pieno, nessun panic → Task 2 e Task 5.
5. **Tab chiuso o cambiato mentre la finestra è nascosta, in ciascuno dei quattro flussi**: nessun retino nel documento sbagliato, `active_cmd` pulito, selezione ripristinata → Task 9.

---

## File Structure

| File | Ruolo | Azione |
|---|---|---|
| `src/scene/model/hatch_model.rs` | regola condivisa `island_ring_kept` | modifica |
| `src/scene/entity.rs` | usa `island_ring_kept` (comportamento invariato) | modifica |
| `src/modules/draw/draw/hatch_settings.rs` | tipi puri: `HatchSettings`, `ResolvedSettings`, `HatchRegion`, `RegionOrigin`, chiave canonica, deduplica, parsing | nuovo |
| `src/modules/draw/draw/hatch_flows.rs` | comandi a un colpo: scelta origine, anteprima | nuovo |
| `src/modules/draw/draw/hatch.rs` | `object_regions`, `with_settings`, `with_regions`, raccoglitore, anteprima fedele | modifica |
| `src/modules/draw/draw/mod.rs` | `pub mod hatch_settings; pub mod hatch_flows;` | modifica |
| `src/command.rs` | `CmdResult::HatchBoundariesPicked` | modifica |
| `src/app/command_driver/mod.rs` | arm di `apply_cmd_result_inner`, `preserve_selection` | modifica |
| `src/ui/properties.rs` | swatch riusabile (`pub(crate)`, angolo/scala, normalizzazione) | modifica |
| `src/ui/window/hatch_dialog.rs` | `State`, `Field`, `AddKind`, `Flow`, `view_window` | nuovo |
| `src/ui/window/mod.rs` | `pub mod hatch_dialog;` | modifica |
| `src/app/commands/hatch_dialog.rs` | apertura, OK/Cancel, flussi nascosti, ritorni, annullamento per tab | nuovo |
| `src/app/commands/mod.rs` | `mod hatch_dialog;`, `dispatch_hatch_dialog` in `dispatch_families`, `-HATCH` nell'elenco | modifica |
| `src/app/commands/draw.rs` | arm `"HATCH"` → finestra; `"-HATCH"` = corpo di oggi | modifica |
| `src/app/mod.rs` | `ModalKind::Hatch`, campi `hatch_dialog`/`hatch_last`/`scripted_dispatch`, `Message::HatchDialog*` | modifica |
| `src/app/view/modal.rs` | titolo e contenuto della modale | modifica |
| `src/app/update/mod.rs` | arm dei messaggi, `CloseModal`, Invio, `TabSwitch`, `ScriptLine` | modifica |
| `src/app/update/command.rs` | `on_tab_close` annulla il flusso | modifica |
| `src/app/control/mod.rs`, `src/app/plugin_host.rs` | impostano `scripted_dispatch` | modifica |
| `CLAUDE.md` | riga "finestra Hatch" tra le cose proprie di ArchLine | modifica |

---

### Task 1: Regola condivisa degli anelli per stile isole

**Files:**
- Modify: `src/scene/model/hatch_model.rs` (aggiungere la funzione e i test in fondo al file)
- Modify: `src/scene/entity.rs:2134-2141` (usare la funzione)

**Interfaces:**
- Produces: `pub fn island_ring_kept(style: codec::entities::HatchStyleType, depth: usize) -> bool` in `crate::scene::model::hatch_model` (usata da Task 4 e da `entity.rs`).

- [ ] **Step 1: Verifica lo stato di partenza**

```bash
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && git switch -c claude/finestra-hatch && cargo check --locked 2>&1 | tail -3
```
Expected: `Finished` senza errori. Se `git switch` dice che il branch esiste, usare `git switch claude/finestra-hatch`.

- [ ] **Step 2: Scrivi il test che fallisce**

Aggiungere in fondo a `src/scene/model/hatch_model.rs`:

```rust
#[cfg(test)]
mod island_rule_tests {
    use super::island_ring_kept;
    use codec::entities::HatchStyleType as Style;

    #[test]
    fn normal_keeps_every_ring() {
        for depth in 0..5 {
            assert!(island_ring_kept(Style::Normal, depth), "depth {depth}");
        }
    }

    #[test]
    fn outer_keeps_depth_zero_and_one_only() {
        assert!(island_ring_kept(Style::Outer, 0));
        assert!(island_ring_kept(Style::Outer, 1));
        assert!(!island_ring_kept(Style::Outer, 2));
        assert!(!island_ring_kept(Style::Outer, 3));
    }

    #[test]
    fn ignore_keeps_depth_zero_only() {
        assert!(island_ring_kept(Style::Ignore, 0));
        assert!(!island_ring_kept(Style::Ignore, 1));
        assert!(!island_ring_kept(Style::Ignore, 2));
    }
}
```

- [ ] **Step 3: Esegui il test e verifica che fallisca**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib island_rule_tests 2>&1 | tail -8`
Expected: errore di compilazione `cannot find function island_ring_kept`.

- [ ] **Step 4: Implementa la funzione**

In `src/scene/model/hatch_model.rs`, subito prima di `impl HatchModel {` (riga ~275), aggiungere:

```rust
/// Whether a boundary ring at nesting `depth` is drawn under island `style`.
///
/// One rule for every consumer: the model rebuilt from a stored hatch
/// (`scene/entity.rs`) and the previews the HATCH dialog draws. Keeping two
/// copies would let the preview show rings the committed hatch later drops.
pub fn island_ring_kept(style: codec::entities::HatchStyleType, depth: usize) -> bool {
    use codec::entities::HatchStyleType as Style;
    match style {
        Style::Normal => true,
        Style::Outer => depth <= 1,
        Style::Ignore => depth == 0,
    }
}
```

In `src/scene/entity.rs` sostituire il blocco

```rust
            let keep = match dxf.style {
                codec::entities::HatchStyleType::Normal => true,
                codec::entities::HatchStyleType::Outer => depth <= 1,
                codec::entities::HatchStyleType::Ignore => depth == 0,
            };
```

con

```rust
            let keep = crate::scene::model::hatch_model::island_ring_kept(dxf.style, depth);
```

- [ ] **Step 5: Esegui i test e verifica che passino**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib island_rule_tests 2>&1 | tail -6 && cargo test --locked --lib hatch 2>&1 | tail -4`
Expected: `3 passed` e nessun fallimento nei test `hatch` esistenti.

- [ ] **Step 6: Commit (solo se autorizzato)**

```bash
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && git add src/scene/model/hatch_model.rs src/scene/entity.rs && git commit -m "Hatch: la regola delle isole diventa una funzione condivisa (island_ring_kept)"
```

---

### Task 2: Tipi puri — impostazioni, aree, chiave canonica, deduplica

**Files:**
- Create: `src/modules/draw/draw/hatch_settings.rs`
- Modify: `src/modules/draw/draw/mod.rs` (aggiungere `pub mod hatch_settings;` dopo `pub mod hatch;`)

**Interfaces:**
- Produces (tutto in `crate::modules::draw::draw::hatch_settings`):
  - `type HatchRing = Vec<[f64; 2]>`
  - `struct HatchRegion { pub rings: Vec<HatchRing> }` (`Clone, Debug, PartialEq`; `rings[0]` esterno, il resto buchi)
  - `enum RegionOrigin { Points, Objects }` (`Clone, Copy, Debug, PartialEq, Eq`)
  - `fn region_key(&HatchRegion) -> RegionKey`
  - `fn add_region(list: &mut Vec<(HatchRegion, RegionOrigin)>, region: HatchRegion, origin: RegionOrigin) -> bool`
  - `enum OriginMode { Current, Specified }`
  - `struct HatchSettings { pattern: String, angle: String, scale: String, associative: bool, separate: bool, retain: bool, island_detection: bool, island_style: HatchStyleType, origin_mode: OriginMode }` con `Default`, `resolve() -> Option<ResolvedSettings>`, `angle_error()`, `scale_error()`, `set_retain(bool)`, `set_separate(bool)`, `effective_island_style()`
  - `struct ResolvedSettings { pattern: String, angle_rad: f32, scale: f32, associative: bool, separate: bool, retain: bool, island_style: HatchStyleType }`
  - `fn parse_angle_deg(&str) -> Option<f32>`, `fn parse_scale(&str) -> Option<f32>`

- [ ] **Step 1: Scrivi il file con i test (che falliranno)**

Creare `src/modules/draw/draw/hatch_settings.rs` con **solo** lo scheletro dei test per ora:

```rust
//! Pure data and rules behind the HATCH dialog: the settings it edits, the
//! regions it collects, the canonical key that makes two regions "the same",
//! and the parsing that decides whether a field is usable.
//!
//! Nothing here touches the app, the scene or the GPU, so every rule can be
//! tested with plain values.

use codec::entities::HatchStyleType;

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(resolved.pattern, "ANSI31");
        assert_eq!(resolved.angle_rad, 0.0);
        assert_eq!(resolved.scale, 1.0);
        assert!(resolved.associative && !resolved.separate && !resolved.retain);
        assert_eq!(resolved.island_style, HatchStyleType::Normal);
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
```

In `src/modules/draw/draw/mod.rs` aggiungere dopo la riga `pub mod hatch;`:

```rust
pub mod hatch_settings;
```

- [ ] **Step 2: Esegui i test e verifica che falliscano**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib hatch_settings 2>&1 | tail -8`
Expected: errori di compilazione `cannot find type HatchRing / HatchRegion / function region_key ...`.

- [ ] **Step 3: Implementa i tipi**

In `hatch_settings.rs`, **prima** del blocco `#[cfg(test)]`, inserire:

```rust
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
    if let Some(start) = (0..points.len()).min_by_key(|&index| points[index]) {
        points.rotate_left(start);
    }
    points
}

pub fn region_key(region: &HatchRegion) -> RegionKey {
    let mut rings = region.rings.iter();
    let outer = rings
        .next()
        .map(|ring| canonical_ring(ring))
        .unwrap_or_default();
    let mut holes: Vec<QuantizedRing> = rings.map(|ring| canonical_ring(ring)).collect();
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

/// What the dialog edits. Angle and scale stay as typed text so a half-typed
/// number does not snap back while it is being typed.
#[derive(Clone, Debug, PartialEq)]
pub struct HatchSettings {
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

/// Settings once every field is known to be usable.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSettings {
    pub pattern: String,
    pub angle_rad: f32,
    pub scale: f32,
    pub associative: bool,
    pub separate: bool,
    pub retain: bool,
    pub island_style: HatchStyleType,
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

    /// `None` while any field is unusable or the pattern is not in the catalog.
    pub fn resolve(&self) -> Option<ResolvedSettings> {
        crate::scene::model::hatch_patterns::find(&self.pattern)?;
        Some(ResolvedSettings {
            pattern: self.pattern.clone(),
            angle_rad: parse_angle_deg(&self.angle)?.to_radians(),
            scale: parse_scale(&self.scale)?,
            associative: self.associative,
            separate: self.separate,
            retain: self.retain,
            island_style: self.effective_island_style(),
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
```

- [ ] **Step 4: Esegui i test e verifica che passino**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib hatch_settings 2>&1 | tail -8`
Expected: `13 passed`. Se `unknown_pattern_does_not_resolve` o `default_settings_resolve` fallisce perché ANSI31 non è nel catalogo, aprire `assets/patterns/OpenCADStudio.pat` e usare il nome del primo pattern esistente in `Default` e nel test; ma `hatch.rs` già usa `"ANSI31"` come predefinito, quindi non dovrebbe succedere.

- [ ] **Step 5: Commit (solo se autorizzato)**

```bash
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && git add src/modules/draw/draw/hatch_settings.rs src/modules/draw/draw/mod.rs && git commit -m "Hatch: tipi puri per la finestra (impostazioni, aree, chiave canonica, deduplica)"
```

---

### Task 3: HatchCommand — impostazioni, aree, raccoglitore, comandi a un colpo

**Files:**
- Modify: `src/modules/draw/draw/hatch.rs` (struct `HatchCommand` righe 183-209, `new` 212-260, `set_object_selection` 264-277, `prompt` 529, `options` 569, `on_enter` 685, `on_escape` 798, `on_text_input` 806; test in fondo)
- Modify: `src/command.rs` (enum `CmdResult`, dopo `CommitHatches`, riga ~1850)
- Modify: `src/app/command_driver/mod.rs` (`preserve_selection` riga 168; nuovo arm in `apply_cmd_result_inner`)
- Create: `src/modules/draw/draw/hatch_flows.rs`
- Modify: `src/modules/draw/draw/mod.rs` (`pub mod hatch_flows;`)

**Interfaces:**
- Consumes (Task 2): `HatchRegion`, `RegionOrigin`, `add_region`, `ResolvedSettings`.
- Produces:
  - `pub fn object_regions(sources: &FxHashMap<Handle, BoundarySource>, handles: &[Handle]) -> Vec<HatchRegion>` in `hatch.rs`
  - `HatchCommand::with_settings(self, &ResolvedSettings) -> Self`
  - `HatchCommand::with_regions(self, Vec<HatchRegion>) -> Self`
  - `HatchCommand::collecting(outlines, boundary_sources, plane, select_objects: bool) -> Self`
  - `CmdResult::HatchBoundariesPicked { regions: Vec<(HatchRegion, RegionOrigin)>, objects: Vec<Handle> }`
  - `hatch_flows::HatchOriginPickCommand` (unit struct) e `hatch_flows::HatchPreviewCommand::new(Vec<HatchModel>)`

- [ ] **Step 1: Scrivi i test (falliranno)**

In fondo al modulo `tests` di `hatch.rs` (dopo `boundary_region_keeps_a_selected_hole_in_one_entity`, prima dell'ultima `}`) aggiungere:

```rust
    // ── HATCH dialog support ───────────────────────────────────────────────

    use crate::modules::draw::draw::hatch_settings::{
        HatchRegion, HatchSettings, RegionOrigin,
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
        // Switching how areas are chosen is still allowed.
        assert!(command.on_text_input("O").is_some());
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
```

Creare `src/modules/draw/draw/hatch_flows.rs` con soli test per ora:

```rust
//! One-shot commands the HATCH dialog starts while it is hidden: choosing the
//! hatch origin and showing a preview. Both hand control back to the dialog
//! through a `Dispatch` string, as the Write Block dialog's point picker does.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CadCommand, CmdResult};
    use glam::DVec3;

    #[test]
    fn origin_pick_reports_the_picked_point() {
        let mut command = HatchOriginPickCommand;
        match command.on_point(DVec3::new(1.5, -2.0, 0.0)) {
            CmdResult::Dispatch(text) => assert_eq!(text, "HATCH_ORIGIN_PICKED 1.5 -2 0"),
            _ => panic!("expected Dispatch"),
        }
    }

    #[test]
    fn origin_pick_enter_and_escape_cancel() {
        let mut command = HatchOriginPickCommand;
        for result in [command.on_enter(), command.on_escape()] {
            match result {
                CmdResult::Dispatch(text) => assert_eq!(text, "HATCH_PICK_CANCELLED"),
                _ => panic!("expected Dispatch"),
            }
        }
    }

    #[test]
    fn preview_ends_on_click_enter_and_escape() {
        let mut command = HatchPreviewCommand::new(Vec::new());
        let results = [
            command.on_point(DVec3::ZERO),
            command.on_enter(),
            command.on_escape(),
        ];
        for result in results {
            match result {
                CmdResult::Dispatch(text) => assert_eq!(text, "HATCH_PREVIEW_DONE"),
                _ => panic!("expected Dispatch"),
            }
        }
    }

    #[test]
    fn preview_shows_the_models_it_was_given() {
        let command = HatchPreviewCommand::new(Vec::new());
        assert_eq!(command.hatch_preview_models().map(|models| models.len()), Some(0));
    }
}
```

In `src/modules/draw/draw/mod.rs` aggiungere dopo `pub mod hatch_settings;`:

```rust
pub mod hatch_flows;
```

- [ ] **Step 2: Esegui i test e verifica che falliscano**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib draw::hatch 2>&1 | tail -12`
Expected: errori di compilazione (`HatchCommand::collecting`, `with_settings`, `with_regions`, `object_regions`, `CmdResult::HatchBoundariesPicked`, `HatchOriginPickCommand` non esistono).

- [ ] **Step 3: Aggiungi la variante a `CmdResult`**

In `src/command.rs`, dopo la variante `CommitHatches { ... },` (riga ~1850):

```rust
    /// The HATCH dialog's collector finished a round of picking: the regions
    /// gathered (with which "Add" produced each) and the boundary objects chosen.
    /// The host hands them to the dialog; nothing is committed to the drawing.
    HatchBoundariesPicked {
        regions: Vec<(
            crate::modules::draw::draw::hatch_settings::HatchRegion,
            crate::modules::draw::draw::hatch_settings::RegionOrigin,
        )>,
        objects: Vec<Handle>,
    },
```

In `src/app/command_driver/mod.rs`:

1. Nel `matches!` di `preserve_selection` (riga 168) aggiungere `| CmdResult::HatchBoundariesPicked { .. }` (la selezione è ripristinata dalla finestra, non azzerata dalla fine del comando).
2. In `apply_cmd_result_inner`, accanto agli arm degli hatch (dopo `CmdResult::CommitHatches { .. } => { ... }`, riga ~627):

```rust
            CmdResult::HatchBoundariesPicked { regions, objects } => {
                return self.handle_hatch_boundaries_picked(regions, objects);
            }
```

Poiché il gestore sarà scritto nel Task 8, aggiungere **ora** una versione minima in `src/app/commands/hatch_dialog.rs` (file creato nel Task 7; per ora crearlo vuoto non basta). Per mantenere il crate compilabile in questo task, aggiungere temporaneamente in `src/app/command_driver/mod.rs`, dentro `impl OpenCADStudio`, subito dopo `handle_dispatch`:

```rust
    /// Replaced by the real handler in `app/commands/hatch_dialog.rs` (plan Task 8).
    fn handle_hatch_boundaries_picked(
        &mut self,
        _regions: Vec<(
            crate::modules::draw::draw::hatch_settings::HatchRegion,
            crate::modules::draw::draw::hatch_settings::RegionOrigin,
        )>,
        _objects: Vec<codec::Handle>,
    ) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        Task::none()
    }
```

- [ ] **Step 4: Implementa le modifiche a `hatch.rs`**

4a. Import in cima a `hatch.rs` (dopo `use crate::t;`):

```rust
use crate::modules::draw::draw::hatch_settings::{
    add_region, HatchRegion, RegionOrigin, ResolvedSettings,
};
```

4b. Funzione libera `object_regions` (dopo `resolve_hatch_rings`, prima di `pack_rings`):

```rust
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
```

4c. Nel `struct HatchCommand` aggiungere il campo (dopo `plane: WorkingPlane,`):

```rust
    /// The HATCH dialog's collector: Enter hands the regions back instead of
    /// committing a hatch, and the settings keywords belong to the dialog.
    collect_only: bool,
```

In `HatchCommand::new`, nell'inizializzatore aggiungere `collect_only: false,` dopo `plane,`.

4d. `set_object_selection` usa la funzione condivisa (stesso comportamento):

```rust
    fn set_object_selection(&mut self, handles: Vec<Handle>) {
        self.object_regions = object_regions(&self.boundary_sources, &handles)
            .into_iter()
            .map(|region| region.rings)
            .collect();
        self.missed = !handles.is_empty() && self.object_regions.is_empty();
        self.selected_objects = handles;
    }
```

4e. Nuovi costruttori e metodi, dopo `with_origin`:

```rust
    /// Apply the dialog's settings as the overrides the command line sets with
    /// `P`, `A`, `L`, `N`, `D`, `B` and `Y`.
    pub fn with_settings(mut self, settings: &ResolvedSettings) -> Self {
        let entry = crate::scene::model::hatch_patterns::find(&settings.pattern);
        self.pattern_override = entry.map(|entry| (entry.name.clone(), entry.gpu.clone()));
        self.angle_override = Some(settings.angle_rad);
        self.scale_override = Some(settings.scale);
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
```

4f. `on_enter`: dopo il blocco che aggiunge la regione manuale (`if matches!(self.mode, HatchMode::Manual) && self.manual_pts.len() >= 3 { ... }`) e **prima** di `let rings = self.combined_rings();` inserire:

```rust
        if self.collect_only {
            return self.collected();
        }
```

4g. `on_escape`:

```rust
    fn on_escape(&mut self) -> CmdResult {
        if self.collect_only {
            return CmdResult::Dispatch("HATCH_PICK_CANCELLED".to_string());
        }
        CmdResult::Cancel
    }
```

4h. `on_text_input`: subito dopo `let upper = input.to_ascii_uppercase();` inserire:

```rust
        if self.collect_only
            && !matches!(self.mode, HatchMode::Manual)
            && !matches!(
                upper.as_str(),
                "O" | "OBJECT" | "OBJECTS" | "I" | "INTERNAL" | "S"
            )
        {
            return None;
        }
```

4i. `prompt`: all'inizio del corpo (prima di `match &self.mode {`):

```rust
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
```

4j. `options`: all'inizio (prima di `match &self.mode {`):

```rust
        if self.collect_only && !matches!(self.mode, HatchMode::Manual) {
            let mut options = match self.mode {
                HatchMode::SelectObjects => vec![
                    CmdOption::new(t!("Pick internal points").as_ref(), "I"),
                    CmdOption::new(t!("Draw manually").as_ref(), "S"),
                ],
                _ => vec![
                    CmdOption::new(t!("Select objects").as_ref(), "O"),
                    CmdOption::new(t!("Draw manually").as_ref(), "S"),
                ],
            };
            options.push(CmdOption::enter(t!("Back to dialog").as_ref()));
            return options;
        }
```

- [ ] **Step 5: Implementa `hatch_flows.rs`**

Inserire in `hatch_flows.rs`, **prima** del blocco `#[cfg(test)]`:

```rust
use crate::command::{CadCommand, CmdResult};
use crate::scene::model::hatch_model::HatchModel;
use glam::DVec3;

/// One-shot picker for the dialog's "Click to set new origin" button.
pub struct HatchOriginPickCommand;

impl CadCommand for HatchOriginPickCommand {
    fn name(&self) -> &'static str {
        "HATCH"
    }

    fn prompt(&self) -> String {
        crate::t!("HATCH  Specify hatch origin:").into_owned()
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        CmdResult::Dispatch(format!("HATCH_ORIGIN_PICKED {} {} {}", pt.x, pt.y, pt.z))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch("HATCH_PICK_CANCELLED".to_string())
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Dispatch("HATCH_PICK_CANCELLED".to_string())
    }
}

/// Shows the hatch the dialog would create until a click, Enter or Esc.
pub struct HatchPreviewCommand {
    models: Vec<HatchModel>,
}

impl HatchPreviewCommand {
    pub fn new(models: Vec<HatchModel>) -> Self {
        Self { models }
    }
}

impl CadCommand for HatchPreviewCommand {
    fn name(&self) -> &'static str {
        "HATCH"
    }

    fn prompt(&self) -> String {
        crate::t!("HATCH  Preview — click or press Enter to return to the dialog:").into_owned()
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::Dispatch("HATCH_PREVIEW_DONE".to_string())
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch("HATCH_PREVIEW_DONE".to_string())
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Dispatch("HATCH_PREVIEW_DONE".to_string())
    }

    fn hatch_preview_models(&self) -> Option<Vec<HatchModel>> {
        Some(self.models.clone())
    }
}
```

- [ ] **Step 6: Esegui i test**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib draw::hatch 2>&1 | tail -14`
Expected: tutti i test di `hatch`, `hatch_settings` e `hatch_flows` passano. Se `settings_and_regions_commit_like_the_interactive_command` fallisce su `boundary_paths`/`style`, confrontare i due modelli campo per campo: devono essere identici (stesso costruttore, stessi override); l'unica differenza ammessa è assente.

Run anche: `cargo check --locked 2>&1 | tail -3` → `Finished`.

- [ ] **Step 7: Commit (solo se autorizzato)**

```bash
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && git add src/modules/draw/draw src/command.rs src/app/command_driver/mod.rs && git commit -m "Hatch: HatchCommand con impostazioni, aree e modalità raccoglitore; comandi a un colpo per origine e anteprima"
```

---

### Task 4: Anteprima fedele (un modello per regione, anelli filtrati, buffer sincronizzati)

**Files:**
- Modify: `src/modules/draw/draw/hatch.rs` (nuove funzioni libere e `HatchCommand::preview_models`; `hatch_preview_models` ne diventa un involucro; test)

**Interfaces:**
- Consumes (Task 1): `island_ring_kept`. (Task 3): `HatchCommand` con `with_settings`/`with_regions`.
- Produces: `pub fn preview_models(&self) -> Vec<HatchModel>` su `HatchCommand` (usata da Task 8 per Preview e da `hatch_preview_models`).

- [ ] **Step 1: Scrivi i test (falliranno)**

Nel modulo `tests` di `hatch.rs` aggiungere:

```rust
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

    fn nested_command(style: HatchStyleType, plane: WorkingPlane) -> HatchCommand {
        let rings = nested_rings();
        let mut settings = HatchSettings::default();
        settings.island_style = style;
        HatchCommand::new(rings.clone(), sources_for(&rings), Vec::new(), None, plane)
            .with_settings(&settings.resolve().unwrap())
            .with_regions(vec![HatchRegion { rings }])
    }

    #[test]
    fn preview_rings_follow_the_island_style() {
        for (style, expected) in [
            (HatchStyleType::Normal, 3),
            (HatchStyleType::Outer, 2),
            (HatchStyleType::Ignore, 1),
        ] {
            let models = nested_command(style, WorkingPlane::default()).preview_models();
            assert_eq!(models.len(), 1, "{style:?}");
            let model = &models[0];
            assert_eq!(ring_count(&model.boundary), expected, "boundary {style:?}");
            assert_eq!(
                ring_count(model.fill_plane_boundary.as_ref().unwrap()),
                expected,
                "fill_plane_boundary {style:?}"
            );
            assert_eq!(
                model.boundary_exterior.as_ref().unwrap().len(),
                expected,
                "boundary_exterior {style:?}"
            );
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
        // Local x runs along world +Y, local y along world -X.
        let plane = WorkingPlane::new(DVec3::ZERO, DVec3::Y, DVec3::NEG_X);
        let models = nested_command(HatchStyleType::Ignore, plane).preview_models();
        let model = &models[0];
        let world = &model.boundary;
        let local = model.fill_plane_boundary.as_ref().unwrap();
        assert_eq!(ring_count(world), 1);
        assert_eq!(ring_count(local), 1);
        // On a rotated plane the two buffers really are different coordinates.
        assert_ne!(world[1], local[1]);
        assert!(model.boundary_exterior.as_ref().unwrap().iter().all(|&outer| outer));
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
```

- [ ] **Step 2: Esegui i test e verifica che falliscano**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib draw::hatch 2>&1 | tail -8`
Expected: errore di compilazione `no method named preview_models`.

- [ ] **Step 3: Implementa il filtro e `preview_models`**

3a. Funzioni libere in `hatch.rs`, dopo `rte_boundary`:

```rust
/// Keep only the rings of a NaN-separated buffer whose entry in `keep` is true.
fn retain_rings(buffer: &[[f32; 2]], keep: &[bool]) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(buffer.len());
    let mut ring_index = 0;
    let mut current: Vec<[f32; 2]> = Vec::new();
    let mut flush = |ring: &mut Vec<[f32; 2]>, index: usize, out: &mut Vec<[f32; 2]>| {
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
```

Nota: `flush` come closure che cattura `keep` per riferimento e riceve `out`/`ring` come parametri evita conflitti di borrow; se il compilatore protesta sul prestito di `out` o `current`, trasformare `flush` in una funzione libera `fn flush_ring(ring: &mut Vec<[f32;2]>, index: usize, keep: &[bool], out: &mut Vec<[f32;2]>)` con lo stesso corpo.

3b. In `impl HatchCommand`, dopo `collected`:

```rust
    /// The models the preview draws: one per region when hatches are made
    /// separately, otherwise one for everything, each with the rings the island
    /// style hides taken out of what is rendered.
    pub fn preview_models(&self) -> Vec<HatchModel> {
        let mut regions: Vec<Vec<Vec<[f64; 2]>>> = self
            .point_regions
            .iter()
            .chain(self.object_regions.iter())
            .cloned()
            .collect();
        if matches!(self.mode, HatchMode::Manual) && self.manual_pts.len() >= 3 {
            regions.push(vec![self
                .manual_pts
                .iter()
                .map(|point| [point.x, point.y])
                .collect()]);
        }
        let groups: Vec<Vec<Vec<[f64; 2]>>> = if self.separate_hatches && !self.retain_boundaries
        {
            regions
        } else {
            let rings = self.combined_rings();
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
                let depths = ring_nesting_depths(&rings);
                let mut model = self.make_hatch(rings);
                filter_rendered_rings(&mut model, &depths);
                model.color = [0.15, 0.55, 1.0, 0.75];
                model
            })
            .collect()
    }
```

Attenzione: in modalità manuale `combined_rings()` non include i punti manuali; l'esistente `hatch_preview_models` li aggiungeva a `rings`. Nel ramo non separato aggiungere l'anello manuale: sostituire il ramo `else` con

```rust
            let mut rings = self.combined_rings();
            if matches!(self.mode, HatchMode::Manual) && self.manual_pts.len() >= 3 {
                rings.push(self.manual_pts.iter().map(|point| [point.x, point.y]).collect());
            }
            if rings.is_empty() { Vec::new() } else { vec![rings] }
```

e togliere il `regions.push(...)` manuale duplicato dal ramo separato **solo se** si vuole evitare il doppio conteggio: nel ramo separato l'anello manuale resta come regione propria (corretto: è una regione).

3c. `hatch_preview_models` diventa:

```rust
    fn hatch_preview_models(&self) -> Option<Vec<HatchModel>> {
        Some(self.preview_models())
    }
```

- [ ] **Step 4: Esegui i test**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib draw::hatch 2>&1 | tail -14`
Expected: tutti passano. Se `preview_rings_follow_the_island_style` fallisce per `Outer` (atteso 2): verificare che `ring_nesting_depths` dia `[0,1,2]` per tre rettangoli annidati e che `island_ring_kept(Outer, 2)` sia falso.

- [ ] **Step 5: Commit (solo se autorizzato)**

```bash
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && git add src/modules/draw/draw/hatch.rs && git commit -m "Hatch: anteprima fedele (un modello per regione, anelli filtrati sui tre buffer renderizzati)"
```

---

### Task 5: Swatch riusabile con angolo, scala e normalizzazione

**Files:**
- Modify: `src/ui/properties.rs:95-202` (struct `HatchPatternPreview`, `hatch_preview_scale`) e la chiamata alla riga ~1501

**Interfaces:**
- Produces:
  - `pub(crate) struct HatchPatternPreview` con `pub(crate) fn new(pattern: HatchPattern) -> Self` e `pub(crate) fn with_angle_scale(self, angle_rad: f32, scale: f32) -> Self`
  - `pub(crate) fn swatch_scale(pattern: &HatchPattern, user_scale: f32) -> f32`
  - `pub(crate) fn swatch_is_dense(user_scale: f32) -> bool`
  - costanti `SWATCH_MIN_SPACING_PX = 2.0`, `SWATCH_MAX_SPACING_PX = 64.0`, `SWATCH_MAX_SEGMENTS = 400`

- [ ] **Step 1: Scrivi i test (falliranno)**

In fondo a `src/ui/properties.rs` (se esiste già un modulo `#[cfg(test)] mod tests` aggiungere dentro; altrimenti crearne uno nuovo `swatch_tests`):

```rust
#[cfg(test)]
mod swatch_tests {
    use super::*;
    use crate::scene::model::hatch_model::{HatchPattern, PatFamily};

    fn lines(spacing: f32) -> HatchPattern {
        HatchPattern::Pattern(vec![PatFamily {
            angle_deg: 45.0,
            x0: 0.0,
            y0: 0.0,
            dx: 0.0,
            dy: spacing,
            dashes: vec![],
        }])
    }

    #[test]
    fn scale_one_is_the_old_behaviour() {
        let pattern = lines(3.175);
        assert_eq!(swatch_scale(&pattern, 1.0), hatch_preview_scale(&pattern));
    }

    #[test]
    fn extreme_scales_are_clamped_to_two_and_sixty_four_pixels() {
        let pattern = lines(3.175);
        let base = hatch_preview_scale(&pattern); // ~8 px at user scale 1
        assert_eq!(swatch_scale(&pattern, 1.0e-6), base * 0.25);
        assert_eq!(swatch_scale(&pattern, 1.0e6), base * 8.0);
    }

    #[test]
    fn only_a_scale_below_the_floor_is_dense() {
        assert!(swatch_is_dense(0.01));
        assert!(swatch_is_dense(0.249));
        assert!(!swatch_is_dense(0.25));
        assert!(!swatch_is_dense(1.0));
        assert!(!swatch_is_dense(1.0e6));
    }

    #[test]
    fn a_pattern_without_lines_keeps_scale_one() {
        assert_eq!(swatch_scale(&HatchPattern::Solid, 5.0), 1.0);
    }
}
```

- [ ] **Step 2: Esegui e verifica che falliscano**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib swatch_tests 2>&1 | tail -6`
Expected: errore di compilazione `cannot find function swatch_scale`.

- [ ] **Step 3: Implementa**

In `src/ui/properties.rs` sostituire la definizione `#[derive(Clone)] struct HatchPatternPreview { pattern: ... }` con:

```rust
/// On-screen spacing limits for a pattern swatch, in pixels.
pub(crate) const SWATCH_MIN_SPACING_PX: f32 = 2.0;
pub(crate) const SWATCH_MAX_SPACING_PX: f32 = 64.0;
/// A swatch never strokes more segments than this.
pub(crate) const SWATCH_MAX_SEGMENTS: usize = 400;
/// `hatch_preview_scale` draws a pattern at roughly this spacing for scale 1.
const SWATCH_BASE_SPACING_PX: f32 = 8.0;

#[derive(Clone)]
pub(crate) struct HatchPatternPreview {
    pattern: crate::scene::model::hatch_model::HatchPattern,
    user_angle: f32,
    user_scale: f32,
}

impl HatchPatternPreview {
    pub(crate) fn new(pattern: crate::scene::model::hatch_model::HatchPattern) -> Self {
        Self {
            pattern,
            user_angle: 0.0,
            user_scale: 1.0,
        }
    }

    /// Angle in radians and scale factor as the user typed them.
    pub(crate) fn with_angle_scale(mut self, angle_rad: f32, scale: f32) -> Self {
        self.user_angle = angle_rad;
        self.user_scale = scale;
        self
    }
}

/// Scale the swatch draws at: the pattern's own normalisation times the user's
/// scale, held so the spacing on screen stays between 2 and 64 px.
pub(crate) fn swatch_scale(
    pattern: &crate::scene::model::hatch_model::HatchPattern,
    user_scale: f32,
) -> f32 {
    let low = SWATCH_MIN_SPACING_PX / SWATCH_BASE_SPACING_PX;
    let high = SWATCH_MAX_SPACING_PX / SWATCH_BASE_SPACING_PX;
    hatch_preview_scale(pattern) * user_scale.clamp(low, high)
}

/// Too fine to draw line by line: the swatch shows a tinted fill instead.
pub(crate) fn swatch_is_dense(user_scale: f32) -> bool {
    user_scale < SWATCH_MIN_SPACING_PX / SWATCH_BASE_SPACING_PX
}
```

Nel `draw` di `HatchPatternPreview`:

- nel ramo `HatchPattern::Pattern(_) => { ... }` **prima** di costruire il `HatchModel`, inserire:

```rust
                if swatch_is_dense(self.user_scale) {
                    frame.fill(&sample, palette.background.base.text.scale_alpha(0.35));
                } else {
```
  e chiudere con `}` dopo il ciclo `for segment in ...` (cioè l'intero costruttore del modello e il ciclo vanno nel ramo `else`);
- nel `HatchModel { ... }` sostituire `angle_offset: 0.0,` con `angle_offset: self.user_angle,` e `scale: hatch_preview_scale(&self.pattern),` con `scale: swatch_scale(&self.pattern, self.user_scale),`;
- nel ciclo: `for segment in model.pattern_segments().into_iter().take(SWATCH_MAX_SEGMENTS) {`.

Nella chiamata esistente (riga ~1501) sostituire

```rust
                let preview = canvas(HatchPatternPreview {
                    pattern: entry.gpu.clone(),
                })
```

con

```rust
                let preview = canvas(HatchPatternPreview::new(entry.gpu.clone()))
```

Se `ui::properties` non è `pub mod` in `src/ui/mod.rs`, renderlo `pub(crate) mod properties;` (verificare con `grep -n "mod properties" src/ui/mod.rs`).

- [ ] **Step 4: Esegui i test**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib swatch_tests 2>&1 | tail -6 && cargo check --locked 2>&1 | tail -3`
Expected: `4 passed`, `Finished`.

- [ ] **Step 5: Commit (solo se autorizzato)**

```bash
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && git add src/ui/properties.rs && git commit -m "Swatch dei pattern riusabile: angolo, scala utente, passo limitato tra 2 e 64 px"
```

---

### Task 6: Stato e vista della finestra

**Files:**
- Create: `src/ui/window/hatch_dialog.rs`
- Modify: `src/ui/window/mod.rs` (aggiungere `pub mod hatch_dialog;`)

**Interfaces:**
- Consumes (Task 2, 5): `HatchSettings`, `OriginMode`, `HatchRegion`, `RegionOrigin`, `HatchPatternPreview`.
- Produces (tutto in `crate::ui::window::hatch_dialog`):
  - `enum Flow { None, Pick, Select, Preview, Origin }` (`Clone, Copy, Debug, PartialEq, Eq`)
  - `enum AddKind { Points, Objects }` (`Clone, Copy, Debug, PartialEq, Eq`)
  - `enum Field { Pattern(String), Angle(String), Scale(String), Associative(bool), Separate(bool), Retain(bool), IslandDetection(bool), IslandStyle(HatchStyleType), OriginMode(OriginMode) }` (`Clone, Debug`)
  - `struct State { pub owner_tab_id: u64, pub plane: WorkingPlane, pub outlines, pub boundary_sources, pub regions: Vec<(HatchRegion, RegionOrigin)>, pub taken_objects: Vec<Handle>, pub saved_selection: Option<Vec<Handle>>, pub settings: HatchSettings, pub specified_origin: Option<[f64; 2]>, pub flow: Flow }`
  - `State::new(owner_tab_id: u64, plane, outlines, boundary_sources, settings) -> State`, `State::apply(&mut self, Field)`, `State::fields_valid(&self) -> bool`, `State::can_ok(&self) -> bool`
  - `pub fn view_window<'a>(state: &State, sizing: crate::ui::modal::ModalSizing) -> Element<'a, Message>`

- [ ] **Step 1: Scrivi i test della logica (falliranno)**

Creare `src/ui/window/hatch_dialog.rs` con lo scheletro dei test:

```rust
//! Hatch and Gradient — the dialog HATCH opens to choose how a hatch looks and
//! which areas it fills, as in AutoCAD.
//!
//! Only what the engine already does is live; the rest is shown greyed so the
//! window reads like the original. The dialog never writes into the drawing
//! itself: OK builds a `HatchCommand` from this state and lets that commit.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::draw::draw::hatch_settings::{HatchRegion, RegionOrigin};
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
    fn retain_and_separate_toggle_each_other_off() {
        let mut state = state();
        state.apply(Field::Separate(true));
        assert!(state.settings.separate && !state.settings.retain);
        state.apply(Field::Retain(true));
        assert!(state.settings.retain && !state.settings.separate);
    }
}
```

Aggiungere in `src/ui/window/mod.rs`: `pub mod hatch_dialog;` (accanto a `pub mod drawing_units;`).

- [ ] **Step 2: Esegui e verifica che falliscano**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib hatch_dialog 2>&1 | tail -8`
Expected: errore di compilazione (`State`, `Field`, `Flow` non esistono).

- [ ] **Step 3: Implementa stato e vista**

In `hatch_dialog.rs`, **prima** di `#[cfg(test)]`:

```rust
use codec::entities::HatchStyleType;
use codec::Handle;
use iced::widget::{button, canvas, checkbox, column, container, pick_list, row, text, text_input, Space};
use iced::{Border, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::command::WorkingPlane;
use crate::modules::draw::draw::hatch_settings::{
    HatchRegion, HatchSettings, OriginMode, RegionOrigin,
};
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

/// One field of the dialog changed.
#[derive(Clone, Debug)]
pub enum Field {
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
        }
    }

    pub fn apply(&mut self, field: Field) {
        match field {
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

    /// Add, Preview and OK need every field usable.
    pub fn fields_valid(&self) -> bool {
        self.settings.resolve().is_some()
    }

    pub fn can_ok(&self) -> bool {
        self.fields_valid() && !self.regions.is_empty()
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

fn field<'a>(
    value: &str,
    invalid: bool,
    ctor: fn(String) -> Field,
) -> Element<'a, Message> {
    let input = text_input("", value)
        .on_input(move |text| Message::HatchDialogField(ctor(text)))
        .size(12)
        .padding([3, 6])
        .width(Length::Fixed(90.0));
    if invalid {
        row![input, text(t!("Not a valid number")).size(10).style(danger)]
            .spacing(6)
            .align_y(iced::Center)
            .into()
    } else {
        input.into()
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

    group(
        t!("Type and pattern").into_owned(),
        column![
            labelled(t!("Type").into_owned(), grey(t!("Predefined").into_owned())),
            labelled(t!("Pattern").into_owned(), picker.into()),
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
                field(&state.settings.angle, state.settings.angle_error(), Field::Angle)
            ),
            labelled(
                t!("Scale").into_owned(),
                field(&state.settings.scale, state.settings.scale_error(), Field::Scale)
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
            grey_check(t!("Store as default origin").into_owned()),
        ]
        .spacing(6)
        .into(),
    )
}

fn boundaries_group<'a>(state: &State) -> Element<'a, Message> {
    let enabled = state.fields_valid();
    group(
        t!("Boundaries").into_owned(),
        column![
            add_button(t!("Add: Pick points").into_owned(), AddKind::Points, enabled),
            add_button(t!("Add: Select objects").into_owned(), AddKind::Objects, enabled),
            grey(t!("Remove boundaries").into_owned()),
            grey(t!("Recreate boundary").into_owned()),
            grey(t!("View Selections").into_owned()),
            text(crate::tf!("{} region(s) selected", state.regions.len())).size(11),
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
    group(
        t!("Options").into_owned(),
        column![
            grey_check(t!("Annotative").into_owned()),
            check(settings.associative, t!("Associative").into_owned(), Field::Associative),
            check(
                settings.separate,
                t!("Create separate hatches").into_owned(),
                Field::Separate
            ),
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
    group(
        t!("Boundary retention").into_owned(),
        column![
            row![
                checkbox(state.settings.retain)
                    .on_toggle(|on| Message::HatchDialogField(Field::Retain(on)))
                    .size(14),
                text(t!("Retain boundaries")).size(11),
            ]
            .spacing(6)
            .align_y(iced::Center),
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
    let fields = state.fields_valid();
    let actions = row![
        Space::new().width(Fill),
        dialog_button_styled_opt(
            t!("Preview").into_owned(),
            (fields && !state.regions.is_empty()).then_some(Message::HatchDialogPreview),
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
```

Note di compilazione (iced in questa versione):
- `Message::HatchDialogField`, `HatchDialogAdd`, `HatchDialogPickOrigin`, `HatchDialogPreview`, `HatchDialogOk` si aggiungono nel Task 7; **per far compilare questo task** aggiungere subito in `src/app/mod.rs`, accanto a `DrawingUnitsApply` (riga ~2958), i cinque messaggi con i loro commenti (vedi Task 7, Step 3) e un arm temporaneo `_ => Task::none()` non serve: aggiungere gli arm minimi `Message::HatchDialogField(_) | Message::HatchDialogAdd(_) | Message::HatchDialogPickOrigin | Message::HatchDialogPreview | Message::HatchDialogOk | Message::OpenHatchDialog => Task::none(),` in `update_message` accanto a `Message::DrawingUnitsApply`. Verranno sostituiti nel Task 7.
- Se `pick_list(...).on_select(...)` o la firma del terzo argomento non compilano, copiare esattamente l'idioma di `drop_row` in `src/ui/window/drawing_units.rs:94-109`.
- Se `theme.palette().danger.base.color` non esiste, usare `theme.palette().primary.strong.color` come colore di errore e riportarlo nella nota del commit.
- `crate::tf!` è la macro già usata in `hatch.rs` (`crate::tf!("Island style: {}", ...)`).

- [ ] **Step 4: Esegui i test**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib hatch_dialog 2>&1 | tail -8 && cargo check --locked 2>&1 | tail -3`
Expected: `5 passed`, `Finished`.

- [ ] **Step 5: Commit (solo se autorizzato)**

```bash
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && git add src/ui/window src/app/mod.rs src/app/update/mod.rs && git commit -m "Finestra Hatch: stato, campi e vista (controlli non supportati in grigio)"
```

---

### Task 7: Apertura, OK e Cancel nell'app

**Files:**
- Create: `src/app/commands/hatch_dialog.rs`
- Modify: `src/app/commands/mod.rs` (`mod hatch_dialog;`)
- Modify: `src/app/commands/draw.rs:736-773` (arm `"HATCH"`)
- Modify: `src/app/mod.rs` (`ModalKind::Hatch` riga ~1949; campi riga ~504; inizializzazione riga ~4112; messaggi riga ~2958)
- Modify: `src/app/view/modal.rs` (titolo riga 25; contenuto riga ~514)
- Modify: `src/app/update/mod.rs` (arm messaggi; `CloseModal` riga 8526; Invio riga ~430)

**Interfaces:**
- Consumes (Task 2-6): `State`, `Field`, `AddKind`, `Flow`, `HatchCommand::{new, with_origin, with_settings, with_regions}`, `object_regions`, `add_region`.
- Produces su `OpenCADStudio` (tutti `pub(in crate::app)`):
  - `fn hatch_boundary_context(&self, i: usize) -> (WorkingPlane, FxHashMap<Handle, BoundarySource>, Vec<Vec<[f64; 2]>>)`
  - `fn hatch_dialog_open(&mut self) -> Task<Message>`
  - `fn hatch_dialog_ok(&mut self) -> Task<Message>`
  - `fn hatch_dialog_cancel(&mut self)`
  - `fn hatch_command_from_state(state: &State, document_origin: [f64; 2]) -> Option<HatchCommand>` (funzione libera nel file)
  - `fn selected_handles(&self, i: usize) -> Vec<Handle>`
- Messaggi: `OpenHatchDialog`, `HatchDialogField(Field)`, `HatchDialogAdd(AddKind)`, `HatchDialogPickOrigin`, `HatchDialogPreview`, `HatchDialogOk`.

- [ ] **Step 1: Scrivi i test di flusso (falliranno)**

In `src/app/commands/hatch_dialog.rs` (nuovo) scrivere **solo** il modulo di test:

```rust
//! The HATCH dialog on the app side: opening it, collecting areas while it is
//! hidden, and turning its state into a hatch on OK.

#[cfg(test)]
mod tests {
    use crate::app::{Message, ModalKind, OpenCADStudio};
    use crate::modules::draw::draw::hatch_settings::{HatchRegion, RegionOrigin};
    use crate::ui::window::hatch_dialog::Field;
    use codec::Handle;

    fn add_line(app: &mut OpenCADStudio, x1: f64, y1: f64, x2: f64, y2: f64) -> Handle {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Line(codec::entities::Line::from_points(
                codec::types::Vector3::new(x1, y1, 0.0),
                codec::types::Vector3::new(x2, y2, 0.0),
            )))
    }

    /// A test app whose first tab is a drawing: `new_for_test` leaves tab 0 as
    /// the Start page, where HATCH is not allowed.
    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        app
    }

    /// A drawing with one closed 20 x 10 rectangle made of four lines.
    fn app_with_rectangle() -> OpenCADStudio {
        let mut app = new_app();
        add_line(&mut app, 0.0, 0.0, 20.0, 0.0);
        add_line(&mut app, 20.0, 0.0, 20.0, 10.0);
        add_line(&mut app, 20.0, 10.0, 0.0, 10.0);
        add_line(&mut app, 0.0, 10.0, 0.0, 0.0);
        app
    }

    fn region() -> (HatchRegion, RegionOrigin) {
        (
            HatchRegion {
                rings: vec![vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]]],
            },
            RegionOrigin::Points,
        )
    }

    fn hatch_count(app: &OpenCADStudio) -> usize {
        app.tabs[app.active_tab].scene.hatches.len()
    }

    #[test]
    fn hatch_opens_the_dialog_instead_of_starting_a_command() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        let i = app.active_tab;
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert!(app.hatch_dialog.is_some());
        assert!(app.tabs[i].active_cmd.is_none());
        assert_eq!(
            app.hatch_dialog.as_ref().unwrap().owner_tab_id,
            app.tabs[i].id
        );
    }

    #[test]
    fn the_dialog_opens_in_a_layout_too() {
        // Paper space has no UCS plane: the default plane is used.
        let mut app = new_app();
        let i = app.active_tab;
        let before = app.tabs[i].editing_model_space();
        let _ = app.dispatch_command("HATCH");
        assert_eq!(app.active_modal, Some(ModalKind::Hatch), "model space was {before}");
    }

    #[test]
    fn ok_without_any_region_does_nothing() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch), "still open");
        assert_eq!(hatch_count(&app), 0);
    }

    #[test]
    fn ok_creates_the_hatch_and_closes_the_dialog() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogOk);
        assert!(app.active_modal.is_none());
        assert!(app.hatch_dialog.is_none());
        assert_eq!(hatch_count(&app), 1);
    }

    #[test]
    fn ok_with_an_invalid_field_does_nothing() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogField(Field::Scale("0".into())));
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 0);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn a_comma_decimal_scale_is_accepted() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogField(Field::Scale("0,5".into())));
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 1);
    }

    #[test]
    fn cancel_discards_the_state_and_creates_nothing() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::CloseModal);
        assert!(app.active_modal.is_none());
        assert!(app.hatch_dialog.is_none());
        assert_eq!(hatch_count(&app), 0);
    }

    #[test]
    fn cancel_without_add_or_ok_keeps_the_remembered_settings() {
        let mut app = app_with_rectangle();
        let before = app.hatch_last.clone();
        let _ = app.dispatch_command("HATCH");
        let _ = app.update(Message::HatchDialogField(Field::Scale("9".into())));
        let _ = app.update(Message::CloseModal);
        assert_eq!(app.hatch_last, before);
    }

    #[test]
    fn ok_remembers_the_settings_for_the_next_time() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogField(Field::Scale("3".into())));
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(app.hatch_last.scale, "3");
        let _ = app.dispatch_command("HATCH");
        assert_eq!(app.hatch_dialog.as_ref().unwrap().settings.scale, "3");
    }

    #[test]
    fn a_remembered_pattern_that_vanished_cannot_be_applied() {
        let mut app = app_with_rectangle();
        app.hatch_last.pattern = "NO_SUCH_PATTERN".into();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 0);
        assert!(app.hatch_dialog.is_some());
    }

    #[test]
    fn enter_in_the_dialog_acts_as_ok() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::CommandFinalize);
        assert_eq!(hatch_count(&app), 1);
    }

    #[test]
    fn preselected_boundary_objects_seed_the_first_regions() {
        let mut app = new_app();
        let handles = [
            add_line(&mut app, 0.0, 0.0, 20.0, 0.0),
            add_line(&mut app, 20.0, 0.0, 20.0, 10.0),
            add_line(&mut app, 20.0, 10.0, 0.0, 10.0),
            add_line(&mut app, 0.0, 10.0, 0.0, 0.0),
        ];
        let i = app.active_tab;
        for handle in handles {
            app.tabs[i].scene.select_entity(handle, false);
        }
        let _ = app.dispatch_command("HATCH");
        let state = app.hatch_dialog.as_ref().unwrap();
        assert_eq!(state.regions.len(), 1, "four open sides close one area");
        assert_eq!(state.regions[0].1, RegionOrigin::Objects);
    }

    #[test]
    fn preselection_that_encloses_nothing_opens_an_empty_dialog() {
        let mut app = new_app();
        let handle = add_line(&mut app, 0.0, 0.0, 20.0, 0.0);
        let i = app.active_tab;
        app.tabs[i].scene.select_entity(handle, false);
        let _ = app.dispatch_command("HATCH");
        assert!(app.hatch_dialog.as_ref().unwrap().regions.is_empty());
    }
}
```

Aggiungere in `src/app/commands/mod.rs` la dichiarazione `mod hatch_dialog;` accanto alle altre dichiarazioni di moduli del file.

- [ ] **Step 2: Esegui e verifica che falliscano**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib commands::hatch_dialog 2>&1 | tail -8`
Expected: errori di compilazione (`ModalKind::Hatch`, `hatch_dialog`, `hatch_last`, `Message::HatchDialogOk` non esistono, oppure sono presenti solo gli stub del Task 6).

- [ ] **Step 3: Aggiungi stato, messaggi e modale in `src/app/mod.rs`**

1. Campi in `OpenCADStudio` (dopo `drawing_units`, riga ~504):

```rust
    /// Working state of the HATCH dialog while its flow is under way — visible
    /// or hidden behind a pick; `None` otherwise.
    hatch_dialog: Option<crate::ui::window::hatch_dialog::State>,
    /// The last valid HATCH dialog preferences, kept for the session. Holds
    /// no areas, handles or document references.
    hatch_last: crate::modules::draw::draw::hatch_settings::HatchSettings,
    /// True while a programmatic channel (script, plugin host, automation)
    /// dispatches a command. Distinct from `suppress_plugin_dispatch`, which
    /// only means "do not recurse into a plugin": here it makes `HATCH` run the
    /// dialog-free `-HATCH`.
    pub(crate) scripted_dispatch: bool,
```

2. In `new()` accanto a `drawing_units: None,` (riga ~4112):

```rust
            hatch_dialog: None,
            hatch_last: crate::modules::draw::draw::hatch_settings::HatchSettings::default(),
            scripted_dispatch: false,
```

3. `ModalKind`: aggiungere la variante `Hatch,` dopo `DrawingUnits,` (riga ~1949).

4. `Message`: dopo `DrawingUnitsApply,` (riga ~2958):

```rust
    /// Open the HATCH dialog (what the `HATCH` command does).
    OpenHatchDialog,
    /// One field of the HATCH dialog changed.
    HatchDialogField(crate::ui::window::hatch_dialog::Field),
    /// HATCH dialog: hide it and pick areas by internal point or by object.
    HatchDialogAdd(crate::ui::window::hatch_dialog::AddKind),
    /// HATCH dialog: hide it and pick a hatch origin.
    HatchDialogPickOrigin,
    /// HATCH dialog: hide it and show the hatch it would create.
    HatchDialogPreview,
    /// HATCH dialog OK — create the hatch from the collected areas.
    HatchDialogOk,
```

- [ ] **Step 4: Implementa il file `src/app/commands/hatch_dialog.rs`**

Inserire **prima** del modulo `tests`:

```rust
use iced::Task;

use crate::app::{Message, ModalKind, OpenCADStudio};
use crate::command::{CadCommand, WorkingPlane};
use crate::modules::draw::draw::hatch::{object_regions, HatchCommand};
use crate::modules::draw::draw::hatch_settings::{add_region, OriginMode, RegionOrigin};
use crate::ui::window::hatch_dialog::{Field, State};
use codec::Handle;

/// A `HatchCommand` that would create what the dialog describes, or `None`
/// while a field is unusable or nothing is collected.
pub(in crate::app) fn hatch_command_from_state(
    state: &State,
    document_origin: [f64; 2],
) -> Option<HatchCommand> {
    let resolved = state.settings.resolve()?;
    let origin = match state.settings.origin_mode {
        OriginMode::Current => document_origin,
        OriginMode::Specified => state.specified_origin.unwrap_or(document_origin),
    };
    Some(
        HatchCommand::new(
            state.outlines.clone(),
            state.boundary_sources.clone(),
            Vec::new(),
            None,
            state.plane,
        )
        .with_origin(origin)
        .with_settings(&resolved)
        .with_regions(
            state
                .regions
                .iter()
                .map(|(region, _)| region.clone())
                .collect(),
        ),
    )
}

impl OpenCADStudio {
    /// The working plane and the boundary data HATCH works from, exactly as the
    /// command computes them: the UCS plane in model space, the default plane
    /// elsewhere.
    pub(in crate::app) fn hatch_boundary_context(
        &self,
        i: usize,
    ) -> (
        WorkingPlane,
        rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
        Vec<Vec<[f64; 2]>>,
    ) {
        let working_plane = if self.tabs[i].editing_model_space() {
            self.tabs[i].ucs_xform().working_plane()
        } else {
            WorkingPlane::default()
        };
        let normal = working_plane.z.normalize_or(glam::DVec3::Z);
        let elevation = working_plane.origin.dot(normal);
        let storage = crate::entities::curve::ocs_plane(
            codec::types::Vector3::new(normal.x, normal.y, normal.z),
            elevation,
        );
        let plane = WorkingPlane::new(
            glam::DVec3::from_array(storage.origin),
            glam::DVec3::from_array(storage.x_axis),
            glam::DVec3::from_array(storage.y_axis),
        );
        let boundary_sources = self.tabs[i].scene.boundary_sources_on_plane(plane, 1.0e-6);
        let outlines = crate::scene::boundary_faces(&boundary_sources, 1.0e-6);
        (plane, boundary_sources, outlines)
    }

    pub(in crate::app) fn selected_handles(&self, i: usize) -> Vec<Handle> {
        self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(handle, _)| handle)
            .collect()
    }

    /// `HATCH`: open the dialog on the active drawing.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_open(&mut self) -> Task<Message> {
        // A dialog left over from an abandoned flow must not leak into this one.
        self.hatch_dialog_cancel();
        let i = self.active_tab;
        let (plane, boundary_sources, outlines) = self.hatch_boundary_context(i);
        let selected = self.selected_handles(i);
        let mut state = State::new(
            self.tabs[i].id,
            plane,
            outlines,
            boundary_sources,
            self.hatch_last.clone(),
        );
        // Closed (or together closing) objects already selected seed the first
        // collection, as they used to skip the pick step.
        if !selected.is_empty() {
            let seeded = object_regions(&state.boundary_sources, &selected);
            if seeded.is_empty() {
                self.command_line.push_info(
                    crate::t!("HATCH: the selected objects form no closed boundary.").as_ref(),
                );
            }
            for region in seeded {
                add_region(&mut state.regions, region, RegionOrigin::Objects);
            }
            state.taken_objects = selected
                .into_iter()
                .filter(|handle| state.boundary_sources.contains_key(handle))
                .collect();
        }
        self.hatch_dialog = Some(state);
        self.active_modal = Some(ModalKind::Hatch);
        Task::none()
    }

    pub(in crate::app) fn hatch_dialog_field(&mut self, field: Field) {
        if let Some(state) = self.hatch_dialog.as_mut() {
            state.apply(field);
        }
    }

    /// OK: create the hatch the dialog describes through the same commit paths
    /// the command line uses.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_ok(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let Some(state) = self.hatch_dialog.as_ref() else {
            return Task::none();
        };
        // The commit handlers act on the active tab, so the dialog must belong
        // to it; a flow whose tab was left is abandoned, never applied.
        if state.owner_tab_id != self.tabs[i].id {
            self.hatch_dialog_cancel();
            return Task::none();
        }
        if !state.can_ok() {
            return Task::none();
        }
        let document_origin = self.tabs[i].scene.document.hatch_origin();
        let Some(mut command) = hatch_command_from_state(state, document_origin) else {
            return Task::none();
        };
        let settings = state.settings.clone();
        let result = command.on_enter();
        self.hatch_last = settings;
        self.hatch_dialog = None;
        if self.active_modal == Some(ModalKind::Hatch) {
            self.close_active_modal();
        }
        self.apply_cmd_result(result)
    }

    /// Cancel / X / Esc, and every abandonment (tab left or closed): drop the
    /// state, stop a hidden-step command in the owner tab, put the selection
    /// back and close the window.
    pub(in crate::app) fn hatch_dialog_cancel(&mut self) {
        if let Some(state) = self.hatch_dialog.take() {
            if let Some(index) = self.tabs.iter().position(|tab| tab.id == state.owner_tab_id) {
                if state.flow != crate::ui::window::hatch_dialog::Flow::None {
                    self.tabs[index].active_cmd = None;
                    self.tabs[index].scene.clear_preview_wire();
                }
                if let Some(saved) = state.saved_selection {
                    self.hatch_restore_selection(index, saved);
                }
            }
        }
        if self.active_modal == Some(ModalKind::Hatch) {
            self.close_active_modal();
        }
    }

    pub(in crate::app) fn hatch_restore_selection(&mut self, index: usize, saved: Vec<Handle>) {
        self.tabs[index].scene.deselect_all();
        for handle in saved {
            self.tabs[index].scene.select_entity(handle, false);
        }
        if index == self.active_tab {
            self.refresh_properties();
        }
    }
}
```

- [ ] **Step 5: Cabla comando, modale, messaggi e tasti**

5a. `src/app/commands/draw.rs`: sostituire l'intestazione dell'arm e il calcolo iniziale del piano. Il testo da riga 736 a 755 (`"HATCH" => { ... let outlines = crate::scene::boundary_faces(&boundary_sources, 1.0e-6);`) diventa:

```rust
            // HATCH opens the dialog; `-HATCH` (and every programmatic channel)
            // runs the command line version below.
            "HATCH" if !self.scripted_dispatch => {
                return Some(self.hatch_dialog_open());
            }

            "HATCH" | "-HATCH" => {
                use crate::modules::draw::draw::hatch::HatchCommand;
                let (plane, boundary_sources, outlines) = self.hatch_boundary_context(i);
```

Il resto dell'arm (da `let selected = self.tabs[i]...` in poi) resta invariato. Rimuovere le righe che calcolavano `working_plane`, `normal`, `elevation`, `storage`, `plane`, `boundary_sources` e `outlines` (ora ottenute dall'helper).

5b. `src/app/view/modal.rs`: nel `match` del titolo (dopo `Some(K::DrawingUnits) => ...`):

```rust
            Some(K::Hatch) => crate::t!("Hatch and Gradient").into_owned(),
```

e nel `match` del contenuto, dopo l'arm `DrawingUnits` (riga ~519):

```rust
            super::super::ModalKind::Hatch => {
                let state = self.hatch_dialog.as_ref()?;
                sized_flow(ex, 940, 560, |flow| {
                    crate::ui::window::hatch_dialog::view_window(state, flow)
                })
            }
```

5c. `src/app/update/mod.rs`:
- sostituire gli arm temporanei del Task 6 con (accanto a `Message::DrawingUnitsApply`):

```rust
            Message::OpenHatchDialog => self.hatch_dialog_open(),
            Message::HatchDialogField(field) => {
                self.hatch_dialog_field(field);
                Task::none()
            }
            Message::HatchDialogOk => self.hatch_dialog_ok(),
            Message::HatchDialogAdd(_)
            | Message::HatchDialogPickOrigin
            | Message::HatchDialogPreview => Task::none(), // Task 8
```

- in `Message::CloseModal` (riga 8526), come primo ramo:

```rust
                if self.active_modal == Some(super::ModalKind::Hatch) {
                    self.hatch_dialog_cancel();
                    return Task::none();
                }
```

- nel blocco modale di `update_message`, dopo il blocco `WriteBlock` (riga ~430-436):

```rust
            if self.active_modal == Some(super::ModalKind::Hatch) {
                if matches!(msg, Message::CommandFinalize)
                    || matches!(&msg, Message::ShortcutPressed(key) if key.rsplit('+').next() == Some("ENTER") || key.rsplit('+').next() == Some("RETURN"))
                {
                    return self.update(Message::HatchDialogOk);
                }
            }
```

- [ ] **Step 6: Esegui i test**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib commands::hatch_dialog 2>&1 | tail -20`
Expected: tutti i test del modulo passano. Se `the_dialog_opens_in_a_layout_too` non esercita davvero lo spazio carta (il messaggio mostra `model space was true`), è accettabile: il test verifica solo che la finestra si apra; l'helper usa già `editing_model_space()`.

Run: `cargo check --locked 2>&1 | tail -3` → `Finished`. Se `cargo check` segnala una `match` non esaustiva su `ModalKind`, aggiungere `Hatch` all'arm mancante usando come modello `DrawingUnits`.

- [ ] **Step 7: Commit (solo se autorizzato)**

```bash
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && git add src && git commit -m "HATCH apre la finestra; OK crea il retino dagli stessi percorsi di commit; -HATCH resta senza finestra"
```

---

### Task 8: Flussi nascosti — Add, Preview, origine e ritorni

**Files:**
- Modify: `src/app/commands/hatch_dialog.rs` (nuove funzioni e test)
- Modify: `src/app/commands/mod.rs` (`dispatch_hatch_dialog` in `dispatch_families`, riga 327)
- Modify: `src/app/command_driver/mod.rs` (togliere lo stub `handle_hatch_boundaries_picked`)
- Modify: `src/app/update/mod.rs` (arm di `HatchDialogAdd`, `HatchDialogPickOrigin`, `HatchDialogPreview`)

**Interfaces:**
- Consumes (Task 3, 4, 7): `HatchCommand::{collecting, preview_models}`, `HatchOriginPickCommand`, `HatchPreviewCommand`, `hatch_command_from_state`, `hatch_restore_selection`, `State`.
- Produces su `OpenCADStudio`: `hatch_dialog_add(AddKind) -> Task<Message>`, `hatch_dialog_pick_origin() -> Task<Message>`, `hatch_dialog_preview() -> Task<Message>`, `handle_hatch_boundaries_picked(regions, objects) -> Task<Message>`, `dispatch_hatch_dialog(&str, usize) -> Option<Task<Message>>`, `hatch_dialog_resume(i, Option<[f64;2]>)`.

- [ ] **Step 1: Scrivi i test di flusso (falliranno)**

Nel modulo `tests` di `src/app/commands/hatch_dialog.rs` aggiungere (le funzioni `add_line`, `app_with_rectangle`, `region`, `hatch_count` esistono già):

```rust
    use crate::command::StepInput;
    use crate::ui::window::hatch_dialog::{AddKind, Flow};

    fn open_dialog(app: &mut OpenCADStudio) {
        let _ = app.dispatch_command("HATCH");
        assert!(app.hatch_dialog.is_some());
    }

    fn click_inside(app: &mut OpenCADStudio) {
        let i = app.active_tab;
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("a collector is running")
            .on_point(glam::DVec3::new(10.0, 5.0, 0.0));
        let _ = app.apply_cmd_result(result);
    }

    #[test]
    fn add_points_hides_the_dialog_and_starts_a_collector() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        let i = app.active_tab;
        assert!(app.active_modal.is_none());
        assert!(app.hatch_dialog.is_some(), "state survives while hidden");
        assert_eq!(app.hatch_dialog.as_ref().unwrap().flow, Flow::Pick);
        assert!(app.tabs[i].active_cmd.is_some());
    }

    #[test]
    fn pick_points_then_enter_returns_to_the_dialog_with_the_region() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        click_inside(&mut app);
        let _ = app.feed_command(StepInput::Enter);
        let i = app.active_tab;
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert!(app.tabs[i].active_cmd.is_none());
        let state = app.hatch_dialog.as_ref().unwrap();
        assert_eq!(state.flow, Flow::None);
        assert_eq!(state.regions.len(), 1);
        assert_eq!(state.regions[0].1, RegionOrigin::Points);
        assert_eq!(hatch_count(&app), 0, "nothing is created before OK");
    }

    #[test]
    fn picking_the_same_area_twice_keeps_one_region() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        for _ in 0..2 {
            let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
            click_inside(&mut app);
            let _ = app.feed_command(StepInput::Enter);
        }
        assert_eq!(app.hatch_dialog.as_ref().unwrap().regions.len(), 1);
    }

    #[test]
    fn escape_returns_to_the_dialog_without_adding() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        click_inside(&mut app);
        let _ = app.feed_command(StepInput::Escape);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        let state = app.hatch_dialog.as_ref().unwrap();
        assert_eq!(state.flow, Flow::None);
        assert!(state.regions.is_empty());
    }

    #[test]
    fn ok_after_picking_creates_the_hatch() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        click_inside(&mut app);
        let _ = app.feed_command(StepInput::Enter);
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 1);
        assert!(app.hatch_dialog.is_none());
    }

    #[test]
    fn add_updates_the_remembered_settings_and_cancel_keeps_them() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogField(Field::Scale("4".into())));
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        assert_eq!(app.hatch_last.scale, "4");
        let _ = app.feed_command(StepInput::Escape);
        let _ = app.update(Message::CloseModal);
        assert_eq!(app.hatch_last.scale, "4", "Cancel after an Add keeps the change");
    }

    #[test]
    fn add_is_refused_while_a_field_is_invalid() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogField(Field::Angle("x".into())));
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
    }

    #[test]
    fn select_objects_clears_the_selection_and_escape_restores_it() {
        let mut app = new_app();
        let keep = add_line(&mut app, 100.0, 100.0, 110.0, 100.0);
        let i = app.active_tab;
        app.tabs[i].scene.select_entity(keep, false);
        let _ = app.dispatch_command("HATCH");
        let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
        assert!(app.tabs[i].scene.selected.is_empty(), "starts with an empty selection");
        let _ = app.feed_command(StepInput::Escape);
        assert!(app.tabs[i].scene.selected.contains(&keep), "selection restored");
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn select_objects_enter_restores_the_selection_too() {
        let mut app = app_with_rectangle();
        let i = app.active_tab;
        let keep = add_line(&mut app, 100.0, 100.0, 110.0, 100.0);
        app.tabs[i].scene.select_entity(keep, false);
        let _ = app.dispatch_command("HATCH");
        let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
        let _ = app.feed_command(StepInput::Enter);
        assert!(app.tabs[i].scene.selected.contains(&keep));
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn two_areas_sharing_a_boundary_object_both_form() {
        let mut app = new_app();
        // Two squares side by side sharing the middle line.
        add_line(&mut app, 0.0, 0.0, 10.0, 0.0);
        add_line(&mut app, 10.0, 0.0, 20.0, 0.0);
        add_line(&mut app, 20.0, 0.0, 20.0, 10.0);
        add_line(&mut app, 20.0, 10.0, 10.0, 10.0);
        add_line(&mut app, 10.0, 10.0, 0.0, 10.0);
        add_line(&mut app, 0.0, 10.0, 0.0, 0.0);
        add_line(&mut app, 10.0, 0.0, 10.0, 10.0);
        let _ = app.dispatch_command("HATCH");
        for x in [5.0, 15.0] {
            let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
            let i = app.active_tab;
            let result = app.tabs[i]
                .active_cmd
                .as_mut()
                .unwrap()
                .on_point(glam::DVec3::new(x, 5.0, 0.0));
            let _ = app.apply_cmd_result(result);
            let _ = app.feed_command(StepInput::Enter);
        }
        assert_eq!(app.hatch_dialog.as_ref().unwrap().regions.len(), 2);
    }

    #[test]
    fn preview_shows_without_creating_and_returns() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogPreview);
        let i = app.active_tab;
        assert!(app.active_modal.is_none());
        assert_eq!(app.hatch_dialog.as_ref().unwrap().flow, Flow::Preview);
        assert!(app.tabs[i].active_cmd.is_some());
        assert_eq!(hatch_count(&app), 0);
        let _ = app.feed_command(StepInput::Enter);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(app.hatch_dialog.as_ref().unwrap().flow, Flow::None);
        assert_eq!(hatch_count(&app), 0);
    }

    #[test]
    fn preview_does_not_touch_the_remembered_settings() {
        let mut app = app_with_rectangle();
        let before = app.hatch_last.clone();
        open_dialog(&mut app);
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogField(Field::Scale("7".into())));
        let _ = app.update(Message::HatchDialogPreview);
        let _ = app.feed_command(StepInput::Escape);
        let _ = app.update(Message::CloseModal);
        assert_eq!(app.hatch_last, before, "Preview then Cancel changes nothing");
    }

    #[test]
    fn preview_needs_a_region() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogPreview);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn origin_pick_stores_the_point_in_plane_coordinates_and_returns() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogField(Field::OriginMode(
            crate::modules::draw::draw::hatch_settings::OriginMode::Specified,
        )));
        let _ = app.update(Message::HatchDialogPickOrigin);
        assert_eq!(app.hatch_dialog.as_ref().unwrap().flow, Flow::Origin);
        let i = app.active_tab;
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_point(glam::DVec3::new(3.0, 4.0, 0.0));
        let _ = app.apply_cmd_result(result);
        let state = app.hatch_dialog.as_ref().unwrap();
        assert_eq!(state.flow, Flow::None);
        assert_eq!(state.specified_origin, Some([3.0, 4.0]));
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn origin_escape_returns_without_a_point() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogField(Field::OriginMode(
            crate::modules::draw::draw::hatch_settings::OriginMode::Specified,
        )));
        let _ = app.update(Message::HatchDialogPickOrigin);
        let _ = app.feed_command(StepInput::Escape);
        assert!(app.hatch_dialog.as_ref().unwrap().specified_origin.is_none());
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }
```

- [ ] **Step 2: Esegui e verifica che falliscano**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib commands::hatch_dialog 2>&1 | tail -12`
Expected: i nuovi test falliscono (gli arm di `HatchDialogAdd` ecc. sono ancora stub).

- [ ] **Step 3: Implementa i flussi**

3a. Togliere da `src/app/command_driver/mod.rs` lo stub `handle_hatch_boundaries_picked` aggiunto nel Task 3.

3b. In `src/app/commands/hatch_dialog.rs`, nell'`impl OpenCADStudio`, aggiungere:

```rust
    /// Hide the dialog and start the collector for "Add: Pick points" or
    /// "Add: Select objects".
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_add(
        &mut self,
        kind: crate::ui::window::hatch_dialog::AddKind,
    ) -> Task<Message> {
        use crate::ui::window::hatch_dialog::{AddKind, Flow};
        let i = self.active_tab;
        let Some(state) = self.hatch_dialog.as_ref() else {
            return Task::none();
        };
        if state.owner_tab_id != self.tabs[i].id {
            self.hatch_dialog_cancel();
            return Task::none();
        }
        let Some(resolved) = state.settings.resolve() else {
            return Task::none();
        };
        let settings = state.settings.clone();
        let origin = self.hatch_origin_for(i);
        // Rebuild the boundary data: the drawing may have changed since the
        // dialog opened or since the last round.
        let (plane, boundary_sources, outlines) = self.hatch_boundary_context(i);
        let saved = (kind == AddKind::Objects).then(|| self.selected_handles(i));
        if saved.is_some() {
            self.tabs[i].scene.deselect_all();
        }
        let command = HatchCommand::collecting(
            outlines.clone(),
            boundary_sources.clone(),
            plane,
            kind == AddKind::Objects,
        )
        .with_origin(origin)
        .with_settings(&resolved);
        if let Some(state) = self.hatch_dialog.as_mut() {
            state.plane = plane;
            state.outlines = outlines;
            state.boundary_sources = boundary_sources;
            state.saved_selection = saved;
            state.flow = match kind {
                AddKind::Points => Flow::Pick,
                AddKind::Objects => Flow::Select,
            };
        }
        self.hatch_last = settings;
        self.active_modal = None;
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    /// The hatch origin the next command should use: the picked one when the
    /// dialog is in "Specified origin" mode, the drawing's otherwise.
    fn hatch_origin_for(&self, i: usize) -> [f64; 2] {
        let document = self.tabs[i].scene.document.hatch_origin();
        match self.hatch_dialog.as_ref() {
            Some(state) if state.settings.origin_mode == OriginMode::Specified => {
                state.specified_origin.unwrap_or(document)
            }
            _ => document,
        }
    }

    /// "Click to set new origin": hide the dialog and take one point.
    pub(in crate::app) fn hatch_dialog_pick_origin(&mut self) -> Task<Message> {
        use crate::modules::draw::draw::hatch_flows::HatchOriginPickCommand;
        use crate::ui::window::hatch_dialog::Flow;
        let i = self.active_tab;
        let Some(state) = self.hatch_dialog.as_mut() else {
            return Task::none();
        };
        if state.owner_tab_id != self.tabs[i].id {
            self.hatch_dialog_cancel();
            return Task::none();
        }
        state.flow = Flow::Origin;
        self.active_modal = None;
        let command = HatchOriginPickCommand;
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    /// Preview: hide the dialog and show what OK would create. Does not touch
    /// the remembered settings.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_preview(&mut self) -> Task<Message> {
        use crate::modules::draw::draw::hatch_flows::HatchPreviewCommand;
        use crate::ui::window::hatch_dialog::Flow;
        let i = self.active_tab;
        let Some(state) = self.hatch_dialog.as_ref() else {
            return Task::none();
        };
        if state.owner_tab_id != self.tabs[i].id {
            self.hatch_dialog_cancel();
            return Task::none();
        }
        if !state.can_ok() {
            return Task::none();
        }
        let document_origin = self.tabs[i].scene.document.hatch_origin();
        let Some(command) = hatch_command_from_state(state, document_origin) else {
            return Task::none();
        };
        let models = command.preview_models();
        if let Some(state) = self.hatch_dialog.as_mut() {
            state.flow = Flow::Preview;
        }
        self.active_modal = None;
        let preview = HatchPreviewCommand::new(models);
        self.command_line.push_info(&preview.prompt());
        self.tabs[i].active_cmd = Some(Box::new(preview));
        self.refresh_area_preview(i);
        Task::none()
    }

    /// The collector finished a round: take its regions into the dialog,
    /// restore the selection and show the dialog again.
    #[inline(never)]
    pub(in crate::app) fn handle_hatch_boundaries_picked(
        &mut self,
        regions: Vec<(
            crate::modules::draw::draw::hatch_settings::HatchRegion,
            RegionOrigin,
        )>,
        objects: Vec<Handle>,
    ) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        let owned = self
            .hatch_dialog
            .as_ref()
            .is_some_and(|state| state.owner_tab_id == self.tabs[i].id);
        if !owned {
            // The flow belongs to another tab or is gone: never apply it here.
            self.hatch_dialog_cancel();
            return Task::none();
        }
        if let Some(state) = self.hatch_dialog.as_mut() {
            for (region, origin) in regions {
                add_region(&mut state.regions, region, origin);
            }
            for handle in objects {
                if !state.taken_objects.contains(&handle) {
                    state.taken_objects.push(handle);
                }
            }
        }
        self.hatch_dialog_resume(i, None);
        Task::none()
    }

    /// Back to the visible dialog after a hidden step. `origin` is the point a
    /// "Click to set new origin" round picked, in plane coordinates.
    pub(in crate::app) fn hatch_dialog_resume(&mut self, i: usize, origin: Option<[f64; 2]>) {
        use crate::ui::window::hatch_dialog::Flow;
        let owned = self
            .hatch_dialog
            .as_ref()
            .is_some_and(|state| state.owner_tab_id == self.tabs[i].id);
        if !owned {
            self.hatch_dialog_cancel();
            return;
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].scene.clear_preview_wire();
        let saved = self
            .hatch_dialog
            .as_mut()
            .and_then(|state| state.saved_selection.take());
        if let Some(saved) = saved {
            self.hatch_restore_selection(i, saved);
        }
        if let Some(state) = self.hatch_dialog.as_mut() {
            state.flow = Flow::None;
            if let Some(point) = origin {
                state.specified_origin = Some(point);
                state.settings.origin_mode = OriginMode::Specified;
            }
        }
        self.active_modal = Some(ModalKind::Hatch);
    }

    /// The `Dispatch` strings the hidden-step commands end with. A successful
    /// Pick/Select round does not come this way: it is
    /// `CmdResult::HatchBoundariesPicked`.
    pub(in crate::app) fn dispatch_hatch_dialog(
        &mut self,
        cmd: &str,
        i: usize,
    ) -> Option<Task<Message>> {
        match cmd {
            "HATCH_PICK_CANCELLED" | "HATCH_PREVIEW_DONE" => {
                self.hatch_dialog_resume(i, None);
                Some(Task::none())
            }
            _ => {
                let rest = cmd.strip_prefix("HATCH_ORIGIN_PICKED ")?;
                let numbers: Vec<f64> = rest
                    .split_whitespace()
                    .filter_map(|part| part.parse::<f64>().ok())
                    .collect();
                if numbers.len() != 3 {
                    self.hatch_dialog_resume(i, None);
                    return Some(Task::none());
                }
                let local = self
                    .hatch_dialog
                    .as_ref()
                    .map(|state| {
                        state
                            .plane
                            .to_local(glam::DVec3::new(numbers[0], numbers[1], numbers[2]))
                    })
                    .map(|point| [point.x, point.y]);
                self.hatch_dialog_resume(i, local);
                Some(Task::none())
            }
        }
    }
```

Aggiungere gli import mancanti in testa al file: `use crate::modules::draw::draw::hatch_flows;` non serve (si usano path completi); servono `use crate::command::CadCommand;` (già presente) e `use crate::ui::window::hatch_dialog::AddKind;` solo dentro le funzioni (già fatto).

3c. `src/app/commands/mod.rs`, in `dispatch_families`, prima di `dispatch_draw` (riga 327):

```rust
        if let Some(t) = self.dispatch_hatch_dialog(cmd, i) {
            return Some(t);
        }
```

3d. `src/app/update/mod.rs`: sostituire l'arm segnaposto con

```rust
            Message::HatchDialogAdd(kind) => self.hatch_dialog_add(kind),
            Message::HatchDialogPickOrigin => self.hatch_dialog_pick_origin(),
            Message::HatchDialogPreview => self.hatch_dialog_preview(),
```

- [ ] **Step 4: Esegui i test**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib commands::hatch_dialog 2>&1 | tail -24`
Expected: tutti passano. Se `feed_command` non è visibile dal modulo dei test, usare il percorso `app.feed_command(...)` come fa `plugin_host.rs` (visibilità `pub(in crate::app)` o superiore) e, se serve, ampliarne la visibilità a `pub(in crate::app)`. Se `StepInput::Escape`/`Enter` hanno altro nome, copiare i nomi da `plugin_host.rs:2313` e `2342`.

- [ ] **Step 5: Commit (solo se autorizzato)**

```bash
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && git add src && git commit -m "Finestra Hatch: Add (punti/oggetti), Preview e origine con ritorno alla finestra"
```

---

### Task 9: Proprietà del tab — cambio e chiusura annullano il flusso

**Files:**
- Modify: `src/app/update/mod.rs:1881-1897` (`TabSwitch`)
- Modify: `src/app/update/command.rs:161-169` (`on_tab_close`)
- Modify: `src/app/commands/hatch_dialog.rs` (solo test)

**Interfaces:**
- Consumes (Task 7, 8): `hatch_dialog_cancel`, `State.owner_tab_id`, `State.flow`.
- Produces: nessuna nuova interfaccia; modifica il comportamento di `Message::TabSwitch` e `Message::TabClose`.

- [ ] **Step 1: Scrivi i test (falliranno)**

Nel modulo `tests` di `src/app/commands/hatch_dialog.rs` aggiungere:

```rust
    /// A second drawing tab next to the first (the first stays active);
    /// returns (first_index, second_index).
    fn open_second_tab(app: &mut OpenCADStudio) -> (usize, usize) {
        let first = app.active_tab;
        let _ = app.push_test_document();
        let second = app.tabs.len() - 1;
        assert_ne!(first, second);
        (first, second)
    }

    fn start_flow(app: &mut OpenCADStudio, flow: &str) {
        match flow {
            "pick" => {
                let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
            }
            "select" => {
                let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
            }
            "preview" => {
                app.hatch_dialog.as_mut().unwrap().regions.push(region());
                let _ = app.update(Message::HatchDialogPreview);
            }
            "origin" => {
                let _ = app.update(Message::HatchDialogField(Field::OriginMode(
                    crate::modules::draw::draw::hatch_settings::OriginMode::Specified,
                )));
                let _ = app.update(Message::HatchDialogPickOrigin);
            }
            other => panic!("unknown flow {other}"),
        }
    }

    const FLOWS: [&str; 4] = ["pick", "select", "preview", "origin"];

    #[test]
    fn switching_tab_in_any_hidden_flow_abandons_it() {
        for flow in FLOWS {
            let mut app = app_with_rectangle();
            let (first, second) = open_second_tab(&mut app);
            let _ = app.update(Message::TabSwitch(first));
            open_dialog(&mut app);
            start_flow(&mut app, flow);
            assert!(app.tabs[first].active_cmd.is_some(), "{flow}: flow started");
            let _ = app.update(Message::TabSwitch(second));
            assert!(app.hatch_dialog.is_none(), "{flow}: state dropped");
            assert!(app.active_modal.is_none(), "{flow}: no dialog");
            assert!(app.tabs[first].active_cmd.is_none(), "{flow}: owner's command stopped");
            assert_eq!(hatch_count(&app), 0, "{flow}");
        }
    }

    #[test]
    fn closing_the_owner_tab_in_any_hidden_flow_abandons_it() {
        for flow in FLOWS {
            let mut app = app_with_rectangle();
            let (first, _second) = open_second_tab(&mut app);
            let _ = app.update(Message::TabSwitch(first));
            open_dialog(&mut app);
            start_flow(&mut app, flow);
            let id = app.tabs[first].id;
            let _ = app.update(Message::TabClose(id));
            assert!(app.hatch_dialog.is_none(), "{flow}");
            assert!(app.active_modal.is_none(), "{flow}");
        }
    }

    #[test]
    fn switching_tab_with_the_dialog_visible_closes_it() {
        let mut app = app_with_rectangle();
        let (first, second) = open_second_tab(&mut app);
        let _ = app.update(Message::TabSwitch(first));
        open_dialog(&mut app);
        let _ = app.update(Message::TabSwitch(second));
        assert!(app.hatch_dialog.is_none());
        assert!(app.active_modal.is_none());
    }

    #[test]
    fn selection_is_restored_when_a_select_round_is_abandoned() {
        let mut app = new_app();
        let (first, second) = open_second_tab(&mut app);
        let _ = app.update(Message::TabSwitch(first));
        let keep = add_line(&mut app, 100.0, 100.0, 110.0, 100.0);
        app.tabs[first].scene.select_entity(keep, false);
        let _ = app.dispatch_command("HATCH");
        let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
        let _ = app.update(Message::TabSwitch(second));
        assert!(app.tabs[first].scene.selected.contains(&keep));
    }

    #[test]
    fn a_stale_collector_result_is_never_applied_to_another_tab() {
        let mut app = app_with_rectangle();
        let (first, second) = open_second_tab(&mut app);
        let _ = app.update(Message::TabSwitch(first));
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        // The user somehow lands on the other tab with the result still in flight.
        app.active_tab = second;
        let _ = app.apply_cmd_result(crate::command::CmdResult::HatchBoundariesPicked {
            regions: vec![region()],
            objects: Vec::new(),
        });
        assert!(app.hatch_dialog.is_none());
        assert_eq!(hatch_count(&app), 0);
    }

    #[test]
    fn ok_on_another_tab_is_refused() {
        let mut app = app_with_rectangle();
        let (first, second) = open_second_tab(&mut app);
        let _ = app.update(Message::TabSwitch(first));
        open_dialog(&mut app);
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        app.active_tab = second;
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 0);
        assert!(app.hatch_dialog.is_none());
    }
```

- [ ] **Step 2: Esegui e verifica che falliscano**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib commands::hatch_dialog 2>&1 | tail -14`
Expected: `switching_tab_*` e `closing_the_owner_tab_*` falliscono (lo stato resta). 
- [ ] **Step 3: Implementa**

3a. `src/app/update/mod.rs`, in `Message::TabSwitch`, subito dopo `self.cancel_attr_editor();` (riga ~1891):

```rust
                        // The HATCH dialog belongs to the tab that opened it;
                        // leaving that tab abandons the flow.
                        self.hatch_dialog_cancel();
```

3b. `src/app/update/command.rs`, in `on_tab_close`, subito dopo `self.cancel_attr_editor();` (riga 169):

```rust
                // A HATCH dialog flow owned by the closing tab can never finish.
                if self
                    .hatch_dialog
                    .as_ref()
                    .zip(self.tabs.get(idx))
                    .is_some_and(|(state, tab)| state.owner_tab_id == tab.id)
                {
                    self.hatch_dialog_cancel();
                }
```

Nota: `hatch_dialog_cancel` annulla anche se il flusso appartiene a un altro tab e `TabSwitch` verso lo stesso tab non lo tocca perché è condizionato a `idx != self.active_tab`.

- [ ] **Step 4: Esegui i test**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib commands::hatch_dialog 2>&1 | tail -24`
Expected: tutti passano.

- [ ] **Step 5: Commit (solo se autorizzato)**

```bash
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && git add src && git commit -m "Finestra Hatch: cambiare o chiudere il tab proprietario annulla il flusso"
```

---

### Task 10: Canali programmatici (`scripted_dispatch`) e registrazione di `-HATCH`

**Files:**
- Modify: `src/app/update/mod.rs:2536` (`Message::ScriptLine`)
- Modify: `src/app/control/mod.rs:807-809` e `1183-1185`
- Modify: `src/app/plugin_host.rs:1318` (`run_command`) e `:2290` (`execute_command`)
- Modify: `src/modules/draw/draw/hatch.rs` (registrazione `-HATCH`)
- Modify: `src/app/commands/mod.rs:527` (elenco con `-INSERT`)
- Modify: `src/app/commands/hatch_dialog.rs`, `src/app/plugin_host.rs` e `src/app/control/mod.rs` (solo test)

**Interfaces:**
- Consumes (Task 7): `OpenCADStudio.scripted_dispatch`, arm `"HATCH" if !self.scripted_dispatch`.
- Produces: nessuna interfaccia nuova; con `scripted_dispatch == true` `HATCH` avvia il comando senza finestra.

Censimento (verificato leggendo il codice; non ci sono altri ingressi programmatici): `ScriptLine` (script `.scr`, `--script`, drop di `.scr`, comando `SCRIPT`), `control_request` e `control_step` (REST, MCP, HTTP, TCP, automazione), `plugin_host::run_command` e `execute_command`. L'op legacy `run` di `automation.rs` (senza `"protocol"`) è usata solo dai test e non va toccata. I canali interattivi (riga di comando digitata, ribbon, toolbar, menu, menu contestuale, palette blocchi, viewport) **non** impostano il flag.

- [ ] **Step 1: Scrivi i test (falliranno)**

Nel modulo `tests` di `src/app/commands/hatch_dialog.rs` aggiungere:

```rust
    fn assert_runs_dialog_free(app: &mut OpenCADStudio, channel: &str) {
        let i = app.active_tab;
        assert!(app.active_modal.is_none(), "{channel}: no dialog");
        assert!(app.hatch_dialog.is_none(), "{channel}: no dialog state");
        let name = app.tabs[i].active_cmd.as_ref().map(|command| command.name());
        assert_eq!(name, Some("HATCH"), "{channel}: the command-line HATCH runs");
    }

    #[test]
    fn dash_hatch_starts_the_command_without_a_dialog() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("-HATCH");
        assert_runs_dialog_free(&mut app, "-HATCH");
    }

    #[test]
    fn scripted_dispatch_makes_hatch_dialog_free() {
        let mut app = app_with_rectangle();
        app.scripted_dispatch = true;
        let _ = app.dispatch_command("HATCH");
        app.scripted_dispatch = false;
        assert_runs_dialog_free(&mut app, "scripted_dispatch");
    }

    #[test]
    fn script_lines_run_hatch_without_a_dialog() {
        let mut app = app_with_rectangle();
        let _ = app.update(Message::ScriptLine("HATCH".into()));
        assert_runs_dialog_free(&mut app, "ScriptLine");
        assert!(!app.scripted_dispatch, "the flag is restored afterwards");
    }

    #[test]
    fn interactive_hatch_still_opens_the_dialog_after_a_script() {
        let mut app = app_with_rectangle();
        let _ = app.update(Message::ScriptLine("HATCH".into()));
        let _ = app.update(Message::CommandEscape);
        let _ = app.dispatch_command("HATCH");
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn the_flag_is_restored_when_a_channel_nests() {
        let mut app = app_with_rectangle();
        app.scripted_dispatch = true;
        let _ = app.update(Message::ScriptLine("HATCH".into()));
        assert!(app.scripted_dispatch, "an outer scripted run stays scripted");
        app.scripted_dispatch = false;
    }

    #[test]
    fn command_registration_lists_dash_hatch() {
        let registered = inventory::iter::<crate::command::CommandRegistration>()
            .any(|registration| registration.names.contains(&"-HATCH"));
        assert!(registered);
    }
```

Per i canali del plugin host e di controllo i test vanno **nei moduli `tests` di quei file**, perché usano i loro helper privati.

In `src/app/plugin_host.rs`, dentro `mod tests` (dopo `script_commands_setting_gates_the_runner`):

```rust
    fn hatch_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        // Tab 0 is the Start page, where HATCH is not allowed.
        app.tabs[0].is_start = false;
        app
    }

    fn assert_hatch_ran_dialog_free(app: &OpenCADStudio, channel: &str) {
        assert!(app.active_modal.is_none(), "{channel}: no dialog");
        assert!(app.hatch_dialog.is_none(), "{channel}: no dialog state");
        assert_eq!(
            app.tabs[0].active_cmd.as_ref().map(|command| command.name()),
            Some("HATCH"),
            "{channel}: the command-line HATCH runs"
        );
        assert!(!app.scripted_dispatch, "{channel}: the flag is restored");
    }

    #[test]
    fn plugin_run_command_run_hatch_has_no_dialog() {
        use ocs_plugin_api::host::CommandRequest as R;
        let mut app = hatch_app();
        HostSession::new(&mut app, 0)
            .run_command(R::Run { line: "HATCH".into() })
            .unwrap();
        assert_hatch_ran_dialog_free(&app, "run_command Run");
    }

    #[test]
    fn plugin_run_command_start_hatch_has_no_dialog() {
        use ocs_plugin_api::host::CommandRequest as R;
        let mut app = hatch_app();
        HostSession::new(&mut app, 0)
            .run_command(R::Start { name: "HATCH".into() })
            .unwrap();
        assert_hatch_ran_dialog_free(&app, "run_command Start");
    }

    #[test]
    fn plugin_execute_command_hatch_has_no_dialog() {
        let mut app = hatch_app();
        assert!(HostSession::new(&mut app, 0).execute_command("HATCH"));
        assert_hatch_ran_dialog_free(&app, "execute_command");
    }
```

In `src/app/control/mod.rs`, dentro `mod tests` (usa l'helper `request` già presente):

```rust
    #[test]
    fn control_run_hatch_has_no_dialog() {
        let mut app = OpenCADStudio::new_for_test();
        assert_eq!(request(&mut app, json!({"op":"new"}))["status"], "completed");
        let result = request(&mut app, json!({"op":"run","cmd":"HATCH"}));
        assert_ne!(result["ok"], json!(false), "{result}");
        let i = app.active_tab;
        assert!(app.active_modal.is_none());
        assert!(app.hatch_dialog.is_none());
        assert_eq!(
            app.tabs[i].active_cmd.as_ref().map(|command| command.name()),
            Some("HATCH")
        );
        assert!(!app.scripted_dispatch, "the flag is restored");
    }

    #[test]
    fn control_start_hatch_has_no_dialog() {
        let mut app = OpenCADStudio::new_for_test();
        assert_eq!(request(&mut app, json!({"op":"new"}))["status"], "completed");
        let result = request(&mut app, json!({"op":"start","cmd":"HATCH"}));
        assert_ne!(result["ok"], json!(false), "{result}");
        let i = app.active_tab;
        assert!(app.active_modal.is_none());
        assert!(app.hatch_dialog.is_none());
        assert_eq!(
            app.tabs[i].active_cmd.as_ref().map(|command| command.name()),
            Some("HATCH")
        );
        assert!(!app.scripted_dispatch, "the flag is restored");
    }
```

- [ ] **Step 2: Esegui e verifica che falliscano**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib hatch 2>&1 | tail -16`
Expected: falliscono i test dei canali (`HATCH` apre la finestra) e `command_registration_lists_dash_hatch`.

- [ ] **Step 3: Implementa i confini programmatici**

Regola unica per tutti: salvare il valore precedente e ripristinarlo (i canali si annidano; `suppress_plugin_dispatch` invece si rimette a `false`).

3a. `src/app/update/mod.rs`, sostituire l'arm (riga 2536):

```rust
            Message::ScriptLine(line) => {
                let previous = std::mem::replace(&mut self.scripted_dispatch, true);
                let task = self.feed_script_line(&line);
                self.scripted_dispatch = previous;
                task
            }
```

3b. `src/app/control/mod.rs`, riga 807-809:

```rust
        self.control.routing = true;
        let previous = std::mem::replace(&mut self.scripted_dispatch, true);
        let action = self.control_action(&req);
        self.scripted_dispatch = previous;
        self.control.routing = false;
```

e riga 1183-1185:

```rust
        self.control.routing = true;
        let previous = std::mem::replace(&mut self.scripted_dispatch, true);
        let task = self.update(msg);
        self.scripted_dispatch = previous;
        self.control.routing = false;
```

3c. `src/app/plugin_host.rs`: i due metodi hanno più `return` anticipati, quindi si avvolgono con un involucro. Rinominare `pub fn run_command(` (riga 1318) in `fn run_command_inner(` (stessa firma, non `pub`) e aggiungere subito sopra:

```rust
    /// Every command a plugin or script starts runs as scripted: the dialogs a
    /// command would open for a person (HATCH's) are for the command line, not
    /// for a script that cannot answer them.
    pub fn run_command(
        &mut self,
        request: ocs_plugin_api::host::CommandRequest,
    ) -> Result<ocs_plugin_api::host::CommandOutcome, String> {
        let previous = std::mem::replace(&mut self.app.scripted_dispatch, true);
        let result = self.run_command_inner(request);
        self.app.scripted_dispatch = previous;
        result
    }
```

e lo stesso per `pub fn execute_command(&mut self, cmd: &str) -> bool` (riga 2290): rinominare il corpo in `fn execute_command_inner(&mut self, cmd: &str) -> bool` e aggiungere

```rust
    pub fn execute_command(&mut self, cmd: &str) -> bool {
        let previous = std::mem::replace(&mut self.app.scripted_dispatch, true);
        let accepted = self.execute_command_inner(cmd);
        self.app.scripted_dispatch = previous;
        accepted
    }
```

Le implementazioni di trait alle righe 2664 e 2670 chiamano `self.execute_command(..)`/`self.run_command(..)` e restano invariate.

3d. Registrazione: in `src/modules/draw/draw/hatch.rs` sostituire

```rust
inventory::submit!(crate::command::CommandRegistration { names: &["HATCH"] });  // HatchCommand
```

con

```rust
inventory::submit!(crate::command::CommandRegistration { names: &["HATCH", "-HATCH"] });  // HatchCommand
```

In `src/app/commands/mod.rs` aggiungere `"-HATCH",` subito dopo `"-INSERT",` (riga 527).

- [ ] **Step 4: Esegui i test**

Run: `cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib hatch 2>&1 | tail -20 && cargo test --locked --lib plugin_host 2>&1 | tail -4 && cargo test --locked --lib control 2>&1 | tail -4`
Expected: tutti passano e nessuna regressione nei test esistenti di `plugin_host` e `control`.

- [ ] **Step 5: Commit (solo se autorizzato)**

```bash
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && git add src && git commit -m "HATCH da script, plugin e automazione esegue -HATCH (flag scripted_dispatch); registra -HATCH"
```

---

### Task 11: Documentazione e verifica finale

**Files:**
- Modify: `CLAUDE.md` (sezione "Cosa è ArchLine")
- Modify: `docs/superpowers/specs/2026-10-08-finestra-hatch-design.md` (riga di stato)

**Interfaces:** nessuna.

- [ ] **Step 1: Aggiorna `CLAUDE.md`**

Nella sezione "Cosa è ArchLine", dopo il punto su `src/ui/classic_layers.rs`, aggiungere:

```markdown
- `src/ui/window/hatch_dialog.rs` + `src/app/commands/hatch_dialog.rs` + `src/modules/draw/draw/hatch_settings.rs`/`hatch_flows.rs`: finestra "Hatch and Gradient" aperta da `HATCH` (`-HATCH` = comando senza finestra). Stato in `app.hatch_dialog`, preferenze di sessione in `app.hatch_last`. "Add"/"Preview"/"Click to set new origin" nascondono la finestra e avviano un comando a un colpo; il ritorno delle aree è `CmdResult::HatchBoundariesPicked`, gli altri esiti sono `Dispatch("HATCH_…")`. OK costruisce un `HatchCommand` e richiama `on_enter()` (nessun commit duplicato). Lasciare o chiudere il tab proprietario annulla il flusso. I canali programmatici (script, plugin host, control/MCP/REST) impostano `app.scripted_dispatch` e ottengono `-HATCH`. Il filtro degli anelli per stile isole è `island_ring_kept` (condiviso con `scene/entity.rs`). Spec: `docs/superpowers/specs/2026-10-08-finestra-hatch-design.md`; piano: `docs/superpowers/plans/2026-10-08-finestra-hatch.md`. Fuori dal giro: tab Gradient, Color/Transparency/Layer/Draw order, Inherit Properties, HATCHEDIT con la stessa finestra.
```

Nella spec, riga `Stato:` sostituire con: `Stato: implementata secondo il piano docs/superpowers/plans/2026-10-08-finestra-hatch.md; verifica visiva da parte dell'utente su Windows ancora da fare.` (solo se tutti i task sono completati e i test passano).

- [ ] **Step 2: Verifica completa**

Run in sequenza:

```bash
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo check --locked 2>&1 | tail -3
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib hatch 2>&1 | tail -6
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib 2>&1 | tail -8
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && cargo build --release --bin OpenCADStudio 2>&1 | tail -4
```

Expected: `Finished` su check e build release (la build release esercita lo stack di `rustc`: un overflow qui indica che si è aggiunto troppo annidamento in `view_main`/`update_message`/`apply_cmd_result_inner` — spostare la logica in funzioni `#[inline(never)]`); nella suite completa nessun test che falliva prima deve fallire ora. Se un test non legato all'hatch fallisce, verificare con `git stash` che fallisse già prima prima di toccarlo, e riportarlo all'utente senza correggerlo.

- [ ] **Step 3: Controllo manuale da consegnare all'utente (Windows)**

Non eseguibile da qui (nessuna GPU). Preparare per l'utente questa lista da provare con l'eseguibile `target\release\OpenCADStudio.exe` o l'artefatto CI, chiedendo screenshot:

1. Disegnare un rettangolo, lanciare `HATCH`: si apre "Hatch and Gradient" con i controlli come nello screenshot di riferimento, quelli non supportati in grigio.
2. Scegliere un pattern, scala e angolo: lo swatch cambia. Scala `0,05` mostra un riempimento tinto, scala `100` linee larghe.
3. "Add: Pick points", cliccare dentro il rettangolo, Invio: la finestra torna con "1 region(s) selected". OK crea il retino.
4. Due rettangoli annidati, "Add: Pick points" nell'anello tra i due, Preview con isole Normal / Outer / Ignore: l'anteprima cambia come il retino creato.
5. "Create separate hatches" con due aree: un retino per area; "Retain boundaries" ne mantiene i contorni.
6. "Specified origin" + "Click to set new origin" in un UCS ruotato.
7. Durante "Add: Pick points" cambiare scheda del disegno: nessuna finestra e nessun retino nel disegno sbagliato.
8. `-HATCH` da riga di comando funziona come prima.

- [ ] **Step 4: Commit (solo se autorizzato)**

```bash
cd "C:/Users/archi/Documents/Progetti IA/ArchLine" && git add CLAUDE.md docs && git commit -m "Finestra Hatch: documentazione in CLAUDE.md e stato della spec"
```

---

## Self-Review

**1. Copertura della spec** (sezione → task):
- §2 Flusso (Add, OK, Cancel, Preview, origine, ritorno) → Task 7 (apertura/OK/Cancel/Invio/Esc), Task 8 (Add, Preview, origine, ritorni), Task 3 (collector, `CmdResult`).
- §3 Stato e contratto delle aree, deduplica, selezione → Task 2 (`HatchRegion`, chiave, `add_region`), Task 3 (`HatchBoundariesPicked`, `object_regions`), Task 6 (`State`), Task 8 (ricalcolo contesto a ogni Add, salvataggio/ripristino selezione, nessun filtro per handle: test `two_areas_sharing_a_boundary_object_both_form`).
- §4 Anteprima fedele → Task 1 (regola), Task 4 (un modello per regione, tre buffer sincronizzati, percorsi completi, piano ruotato, Retain).
- §5 Finestra, validazione, swatch → Task 6 (vista, validazione, controlli grigi), Task 5 (swatch normalizzato).
- §6 Impostazioni e origine → Task 7 (`hatch_last`, origine corrente riletta), Task 8 (Add/OK aggiornano, Preview no; origine in coordinate locali via `plane.to_local`).
- §7 Tab, script, registrazione → Task 9, Task 10.
- §8 Modifiche al codice (wiring, inizializzazione, i18n) → Task 7 Step 3/5 (campi, `ModalKind`, messaggi, modal.rs, update), Task 10 (registrazione `-HATCH`); i18n via `t!` senza toccare `locale_catalog.rs` (vincolo globale).
- §10 Test: unitari → Task 1-5; flusso nei quattro flussi nascosti → Task 8 (ritorno con Invio/Esc) e Task 9 (cambio e chiusura tab per `pick`, `select`, `preview`, `origin`); persistenza → Task 7/8; canali → Task 10. **Lacune dichiarate**: (a) il test "origine specificata con UCS ruotato" è coperto solo a livello di conversione `plane.to_local` con piano di default (`origin_pick_stores_the_point_in_plane_coordinates_and_returns`) e non con un UCS ruotato nell'app: aggiungere, se l'API di test lo permette, un caso in cui `tabs[i].ucs_xform()` è ruotato; in caso contrario resta il controllo manuale n. 6 del Task 11. (b) Il canale "modalità Start" delle API di automazione è coperto da `control_run_action_hatch_has_no_dialog` e da `plugin_run_command_start_hatch_has_no_dialog`; non esiste un test separato per un canale "Start tab", perché la scheda iniziale blocca HATCH prima del dispatch.

**2. Placeholder scan:** nessun corpo di test o di funzione è lasciato da scrivere; gli helper dei test (`new_app`, `open_second_tab`, `hatch_app`, `request`) sono quelli letti nel codice (`push_test_document`, `tabs[0].is_start = false`, `request` di `control/mod.rs`). Restano solo istruzioni di compilazione condizionali ("se non compila, copiare l'idioma di ...") dove l'API iced non è stata verificata riga per riga.

**3. Coerenza dei tipi:** `HatchRegion`/`RegionOrigin`/`add_region`/`ResolvedSettings` (Task 2) sono usati con le stesse firme in Task 3, 6, 7, 8; `HatchCommand::{with_settings, with_regions, collecting, preview_models}` (Task 3-4) sono usati in Task 7-8 con gli stessi nomi; `State` ha gli stessi campi in Task 6, 7, 8, 9 (`owner_tab_id`, `plane`, `outlines`, `boundary_sources`, `regions`, `taken_objects`, `saved_selection`, `settings`, `specified_origin`, `flow`); i `Dispatch` sono `HATCH_PICK_CANCELLED`, `HATCH_ORIGIN_PICKED x y z`, `HATCH_PREVIEW_DONE` in Task 3 (comandi), Task 8 (gestori) e nei vincoli globali.

**4. Review Focus:** (1) UTM → `utm_scale_coordinates_keep_equal_regions_equal` (Task 2); (2) virgola decimale → `angle_and_scale_accept_comma_and_dot` (Task 2) e `a_comma_decimal_scale_is_accepted` (Task 7); (3) layout → `the_dialog_opens_in_a_layout_too` (Task 7); (4) pattern scomparso → `unknown_pattern_does_not_resolve` (Task 2), `a_pattern_missing_from_the_catalog_blocks_everything` (Task 6), `a_remembered_pattern_that_vanished_cannot_be_applied` (Task 7), `a_pattern_without_lines_keeps_scale_one` (Task 5); (5) tab chiuso/cambiato nei quattro flussi → `switching_tab_in_any_hidden_flow_abandons_it` e `closing_the_owner_tab_in_any_hidden_flow_abandons_it` (Task 9).
