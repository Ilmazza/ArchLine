# Finestra Hatch and Gradient: pattern, solido e sfumato Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** La finestra "Hatch and Gradient" crea e modifica allo stesso modo pattern, solidi e sfumati (schede Hatch e Gradient, colore selezionabile, "Use Current" di default), con conversione tra i tre tipi in un solo undo.

**Architecture:** Un file nuovo `src/entities/hatch_fill.rs` (solo su `&mut Hatch`) è l'unico punto che scrive sfumato e pattern di catalogo nell'entità; lo usano creazione (`Scene::add_hatch`), pannello Proprietà e finestra. Lo stato della finestra (`HatchSettings`) resta dato puro e guadagna scheda, colore e `GradientSettings`; il colore corrente si risolve solo nell'app (`hatch_dialog_ok` / `hatch_dialog_preview`). La modifica passa da una nuova `HatchEditOperation::Window` che applica tutto sotto un solo snapshot.

**Tech Stack:** Rust, iced (rev pinnato in `Cargo.toml`), `iced_aw`, crate `opencadcodec` (rev fe69506, `codec::entities::Hatch`), `kernel::geom2d`.

**Spec:** `docs/superpowers/specs/2026-10-08-finestra-hatch-gradient-design.md` (leggerla prima di ogni task; la spec precedente è `2026-10-08-finestra-hatch-design.md`).

## Global Constraints

- Rispondere/commentare in italiano nei messaggi di commit e nei documenti; **i commenti nel codice e le stringhe `t!` restano in inglese** come nei file vicini. Nessuna riga di attribuzione nei commit.
- Branch di lavoro: `claude/finestra-hatch` (HEAD attuale 616df941). **Nessun `git push`, nessuna PR** fino al task finale. Non toccare `main` né `lavoro`.
- Trappola n. 1 (CLAUDE.md): in `view_main`, `update_message`, `apply_cmd_result_inner`, `on_viewport_left_press/release` aggiungere **solo poche righe** che delegano a funzioni `#[inline(never)]` in file propri. Non togliere `RUST_MIN_STACK` da `.cargo/config.toml`.
- Non modificare `src/locale_catalog.rs`: le stringhe nuove `t!("…")` in inglese ricadono sull'inglese.
- Repo `eol=lf`. Percorsi sempre assoluti o `cd` esplicito (shell Bash di Git per Windows).
- Nessuna GPU: l'aspetto visivo lo verifica solo l'utente su Windows. **Non scrivere mai "si vede bene"**; dichiarare "fatto" solo con l'output dei comandi.
- Build/test: `cargo check --locked`; `cargo test --locked --lib <filtro>` (filtri: `hatch_fill`, `hatch_settings`, `hatch_edit`, `hatch_gradient`, `hatch`); suite completa `cargo test --locked --lib` (oggi 2407 passati, non devono diminuire se non per test invertiti); `cargo build --release --bin OpenCADStudio` (~5 minuti con cache; lanciarla in background).
- Valori di partenza dello sfumato: colore 1 `Rgb{77,153,242}`, colore 2 `Rgb{46,46,46}`, tinta 1.0, centrato, angolo "0", forma 0 (Linear), "Two colors".
- Angolo dello sfumato separato da quello del pattern; HATCH apre sulla scheda Hatch, GRADIENT sulla scheda Gradient; `-HATCH` e `-GRADIENT` restano com'erano (colore compreso).
- I test nuovi vanno prima visti **rossi** (per il motivo giusto), poi verdi; i test di conteggio verificano anche il contenuto; dove il piano indica una mutazione (M:), l'implementatore la applica a mano una volta, vede il test fallire e la ripristina.
- Nuova logica dell'app in file propri (`src/app/commands/hatch_gradient.rs`, `src/entities/hatch_fill.rs`); negli upstream solo poche righe d'aggancio.

## Review Focus

Classi di input che la spec implica ma che nessun test "ovvio" copre; ognuna ha il suo test nel task indicato.

1. **Sfumato letto da un file di altri CAD con meno di due fermate, nome vuoto o `color_tint` a 0 in "Two colors"** (modifica non deve andare in panico, e passando a "One color" la tinta mostrata deve essere quella che verrà scritta). → Task 1 (`read_gradient`), Task 9.
2. **Tinta agli estremi con base nera o bianca** (`gradient_tint_color` con luminosità 0 o 1: nessun NaN, nessun canale fuori 0..255). → Task 1.
3. **Esc/Invio con la finestra "Select Color" o l'elenco colori aperti, e cambio/chiusura del tab con "Select Color" aperta** (nessun retino creato, nessuno stato residuo). → Task 11.
4. **Sfumato creato su UCS ruotato e in un layout (spazio carta)** (piano di riempimento e normale giusti, nessun retino sul piano XY). → Task 7.
5. **Conversione di un retino associativo con più percorsi/isole** (boundary path, handle di associazione e stile isole restano identici; cambia solo il riempimento). → Task 9.

---

### Task 1: `hatch_fill.rs` — tipo di riempimento e scrittura dello sfumato

**Files:**
- Create: `src/entities/hatch_fill.rs`
- Modify: `src/entities/mod.rs` (aggiungere `pub mod hatch_fill;` accanto a `pub mod hatch;`)
- Modify: `src/scene/entity.rs` (togliere `fn gradient_tint_color` a ~riga 104-132 e importarla da `hatch_fill`)

**Interfaces:**
- Consumes: `crate::scene::model::hatch_model::GradientKind` (`dxf_name`, `from_name`, `CHOICES`, `Copy + PartialEq`).
- Produces (tutto `pub`, in `crate::entities::hatch_fill`):
  - `enum FillKind { Pattern, Solid, Gradient }` con `FillKind::of(&Hatch) -> FillKind`
  - `const DEFAULT_GRADIENT_COLOR1: AcadColor`, `const DEFAULT_GRADIENT_COLOR2: AcadColor`
  - `struct GradientSpec { kind: GradientKind, invert: bool, one_color: bool, color1: AcadColor, color2: AcadColor, tint: f64, angle_rad: f64, centered: bool }` (`Clone, Debug, PartialEq`) con `fn effective_color2(&self) -> AcadColor`
  - `struct GradientPatch { kind: Option<(GradientKind, bool)>, one_color: Option<bool>, color1: Option<AcadColor>, color2: Option<AcadColor>, tint: Option<f64>, angle_rad: Option<f64>, centered: Option<bool> }` (`Clone, Debug, Default, PartialEq`) con `fn is_empty(&self) -> bool`
  - `fn read_gradient(&Hatch) -> GradientSpec`
  - `fn apply_gradient(&mut Hatch, &GradientSpec)` (scrittura completa)
  - `fn apply_gradient_patch(&mut Hatch, &GradientPatch)` (solo i campi presenti; no-op se il retino non è uno sfumato)
  - `fn gradient_tint_color(base: [f32; 4], target: f32) -> [f32; 4]` (spostata da `scene/entity.rs`)
  - `fn tinted_second_color(base: [f32; 4], tint: f32) -> AcadColor` (tinta arrotondata a 8 bit)
  - `fn rgba_of(AcadColor) -> Option<[f32; 4]>`
  - `fn gradient_profile(kind: GradientKind, invert: bool, t: f32) -> f32`

- [ ] **Step 1: Scrivere il file con i soli test (rossi)**

Creare `src/entities/hatch_fill.rs` con l'intestazione di modulo e `#[cfg(test)] mod tests` sotto, **senza** ancora le funzioni (il file non compila: è il "rosso"). Aggiungere `pub mod hatch_fill;` in `src/entities/mod.rs`.

```rust
//! Shared fill logic for hatch entities: what kind of fill a hatch has and
//! the one place that writes a gradient into one. Pure: only `&mut Hatch`,
//! no app, scene or GPU. The HATCH window, the Properties panel and
//! `Scene::add_hatch` all write gradients through here, so a gradient can
//! never be persisted two different ways.

use codec::entities::hatch::GradientColorEntry;
use codec::entities::Hatch;
use codec::types::Color as AcadColor;

use crate::scene::model::hatch_model::GradientKind;

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
```

- [ ] **Step 2: Verificare che il rosso sia per il motivo giusto**

Run: `cd "c:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib hatch_fill 2>&1 | tail -30`
Expected: errori di compilazione `cannot find type GradientSpec / FillKind / function apply_gradient …` (le funzioni non esistono ancora).

- [ ] **Step 3: Implementare, sopra il modulo `tests`**

```rust
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
    // (body moved unchanged from scene/entity.rs, plus the guard below)
    let max = base[0].max(base[1]).max(base[2]);
    let min = base[0].min(base[1]).min(base[2]);
    let lightness = (max + min) * 0.5;
    let target = if target.is_nan() { lightness } else { target.clamp(0.0, 1.0) };
    let mut result = base;
    if target <= lightness {
        let factor = if lightness > 1.0e-6 { target / lightness } else { 0.0 };
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
    if invert { 1.0 - shaped } else { shaped }
}

/// The gradient a stored hatch holds, with defaults for whatever a file from
/// another program left out (stops, name, tint).
pub fn read_gradient(hatch: &Hatch) -> GradientSpec {
    let g = &hatch.gradient_color;
    let (kind, invert) = GradientKind::from_name(&g.name);
    let stop = |index: usize, default: AcadColor| {
        g.colors.get(index).map(|entry| entry.color).unwrap_or(default)
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
```

In `src/scene/entity.rs`: cancellare la vecchia `fn gradient_tint_color` e il suo commento, e aggiungere in testa `use crate::entities::hatch_fill::gradient_tint_color;` (il solo uso è a ~2201 nel lettore).

- [ ] **Step 4: Eseguire i test**

Run: `cd "c:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib hatch_fill 2>&1 | tail -30`
Expected: tutti i test di `hatch_fill` PASS. Poi `cargo check --locked` senza errori.

- [ ] **Step 5: Mutazioni**

(M) in `effective_color2` restituire sempre `self.color2` → `one_color_writes_the_tinted_second_stop_and_the_tint` e `the_effective_second_color_follows_the_mode` devono fallire. (M) in `apply_gradient_patch` riscrivere `g.colors` per intero → `a_patch_with_only_the_kind_touches_only_the_name` deve fallire. Ripristinare.

- [ ] **Step 6: Commit**

```bash
cd "c:/Users/archi/Documents/Progetti IA/ArchLine" && git add src/entities/hatch_fill.rs src/entities/mod.rs src/scene/entity.rs && git commit -m "Hatch fill: tipo di riempimento e scrittura dello sfumato in un punto solo (hatch_fill.rs)"
```

---

### Task 2: `hatch_fill` — pattern di catalogo e aggiornamento; i due siti esistenti lo usano

**Files:**
- Modify: `src/entities/hatch_fill.rs` (aggiungere funzioni e test)
- Modify: `src/app/command_driver/modify.rs:439-515` (ramo `Update`)
- Modify: `src/app/update/command.rs:2305-2351` (`on_prop_hatch_pattern_changed`)

**Interfaces:**
- Consumes: Task 1; `crate::scene::model::hatch_patterns::{find, build_dxf_pattern, PatternEntry}`; `crate::entities::hatch::{scale_pattern_geometry, rotate_pattern_geometry, translate_pattern_geometry}`; metodi di `Hatch`: `pattern_origin()`, `set_pattern_origin(Vector2)`, `scale_pattern_about_origin(f64)`, `rotate_pattern_about_origin(f64)`.
- Produces:
  - `fn set_catalog_pattern(h: &mut Hatch, entry: &PatternEntry, scale: f64, angle: f64)` — mette il pattern (o SOLID) e **azzera** `gradient_color`; non tocca `pattern_scale`/`pattern_angle`.
  - `fn apply_common_update(h: &mut Hatch, origin: Option<(f64, f64)>, disassociate: bool, style: Option<HatchStyleType>)`
  - `fn apply_pattern_update(h: &mut Hatch, name: &str, scale: f32, angle: f32, origin: Option<(f64, f64)>, disassociate: bool, style: Option<HatchStyleType>)` — il corpo oggi inline in `modify.rs` ramo `Update` (compresi `keep_scale`/`keep_angle`), tranne `store_origin` e `annotative` che restano nel chiamante.

- [ ] **Step 1: Test rossi** (in `hatch_fill.rs`, modulo `tests`)

```rust
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
```
(Se `BoundaryPathFlags::is_external` non esiste con quel nome, usare `flags.bits() & EXTERNAL.bits() != 0`; leggere `codec::entities::hatch::BoundaryPathFlags`.)

- [ ] **Step 2:** `cargo test --locked --lib hatch_fill 2>&1 | tail -20` → errori di compilazione (funzioni mancanti).

- [ ] **Step 3: Implementare** in `hatch_fill.rs`

```rust
use codec::entities::{HatchPatternType, HatchStyleType};

use crate::scene::model::hatch_patterns::{self, PatternEntry};

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
    let requested_scale = if keep_scale { h.pattern_scale } else { scale.max(1.0e-6) as f64 };
    let requested_angle = if keep_angle { h.pattern_angle } else { (angle as f64).to_radians() };
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
```

- [ ] **Step 4: Collegare i due siti**

`modify.rs`, ramo `HatchEditOperation::Update` (righe 439-515): sostituire il blocco `if let Some(codec::EntityType::Hatch(hatch)) = …get_entity_mut(handle) { … }` con

```rust
                    if let Some(codec::EntityType::Hatch(hatch)) =
                        self.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        crate::entities::hatch_fill::apply_pattern_update(
                            hatch,
                            &name,
                            scale,
                            angle,
                            origin,
                            disassociate,
                            style,
                        );
                    }
```
lasciando `store_origin` (prima) e `annotative` (dopo) dove sono.

`command.rs::on_prop_hatch_pattern_changed`: dentro il ciclo, al posto del blocco `let mut pattern = … dxf.gradient_color.enabled = false;` mettere

```rust
                                crate::entities::hatch_fill::set_catalog_pattern(
                                    dxf,
                                    entry,
                                    dxf.pattern_scale,
                                    dxf.pattern_angle,
                                );
```
(il borrow di `dxf.pattern_scale` va copiato in locali prima della chiamata se il compilatore lo chiede).

- [ ] **Step 5: Eseguire**

Run: `cargo test --locked --lib hatch 2>&1 | tail -15`
Expected: tutti i test `hatch` (compresi i test di modifica già esistenti in `app/commands/hatch_dialog.rs`, che esercitano `Update` da app) PASS. Se un test di modifica cambia, **fermarsi**: il refactor deve preservare il comportamento (unica differenza voluta: `gradient_color` azzerato).

- [ ] **Step 6: Mutazione e commit**

(M) in `apply_pattern_update` ignorare `keep_scale` → `an_unchanged_scale_and_angle_are_kept_to_the_last_bit` fallisce. Ripristinare.

```bash
git add src/entities/hatch_fill.rs src/app/command_driver/modify.rs src/app/update/command.rs && git commit -m "Hatch fill: set_catalog_pattern e apply_pattern_update condivisi da HATCHEDIT e dal pannello Proprietà"
```

---

### Task 3: Pannello Proprietà — i campi gradiente passano da `apply_gradient_patch`

**Files:**
- Modify: `src/entities/hatch.rs:642-677` (`apply_geom_prop`: `fill_type`, `gradient_type`, `gradient_centered`, `gradient_tint`, `pattern_angle` ramo gradiente)
- Modify: `src/app/update/mod.rs:7267-7293` (colori 1/2 dal pannello)
- Test: nuovo `#[cfg(test)] mod tests` in coda a `src/entities/hatch.rs` + un test di app in `src/app/commands/hatch_gradient.rs` (file creato in questo task con la sola intestazione di test; il Task 7 lo estende)

**Interfaces:**
- Consumes: `GradientPatch`, `apply_gradient_patch` (Task 1).
- Produces: nessuna nuova interfaccia pubblica; comportamento invariato (test di caratterizzazione).

- [ ] **Step 1: Test di caratterizzazione sul codice ATTUALE (devono essere verdi subito)**

In coda a `src/entities/hatch.rs`:

```rust
#[cfg(test)]
mod gradient_property_tests {
    use super::*;
    use crate::entities::hatch_fill::{apply_gradient, GradientSpec};
    use crate::entities::traits::PropertyEditable;
    use crate::scene::model::hatch_model::GradientKind;

    fn gradient() -> Hatch {
        let mut hatch = Hatch::solid();
        apply_gradient(
            &mut hatch,
            &GradientSpec {
                kind: GradientKind::Linear,
                invert: false,
                one_color: false,
                color1: codec::types::Color::Index(1),
                color2: codec::types::Color::Index(5),
                tint: 0.25,
                angle_rad: 0.5,
                centered: true,
            },
        );
        hatch
    }

    fn after(field: &str, value: &str) -> Hatch {
        let mut hatch = gradient();
        PropertyEditable::apply_geom_prop(&mut hatch, field, value);
        hatch
    }

    #[test]
    fn the_gradient_type_changes_only_the_name() {
        let mut expected = gradient();
        expected.gradient_color.name = "CYLINDER".into();
        assert_eq!(after("gradient_type", "Cylindrical"), expected);
        expected.gradient_color.name = "INVCURVED".into();
        assert_eq!(after("gradient_type", "Inverted curved"), expected);
    }

    #[test]
    fn one_color_sets_the_tint_to_one_and_two_color_keeps_it() {
        let mut expected = gradient();
        expected.gradient_color.is_single_color = true;
        expected.gradient_color.color_tint = 1.0;
        assert_eq!(after("fill_type", "One color"), expected);
        let mut already = after("fill_type", "One color");
        PropertyEditable::apply_geom_prop(&mut already, "gradient_tint", "0.3");
        PropertyEditable::apply_geom_prop(&mut already, "fill_type", "One color");
        assert_eq!(already.gradient_color.color_tint, 0.3, "no reset when already one colour");
        PropertyEditable::apply_geom_prop(&mut already, "fill_type", "Two color");
        assert!(!already.gradient_color.is_single_color);
        assert_eq!(already.gradient_color.color_tint, 0.3);
    }

    #[test]
    fn the_tint_is_clamped() {
        assert_eq!(after("gradient_tint", "5").gradient_color.color_tint, 1.0);
        assert_eq!(after("gradient_tint", "-2").gradient_color.color_tint, 0.0);
        assert_eq!(after("gradient_tint", "0.3").gradient_color.color_tint, 0.3);
    }

    #[test]
    fn centered_toggles_and_sets() {
        assert_eq!(after("gradient_centered", "toggle").gradient_color.shift, 1.0);
        assert_eq!(after("gradient_centered", "false").gradient_color.shift, 1.0);
        assert_eq!(after("gradient_centered", "true").gradient_color.shift, 0.0);
    }

    #[test]
    fn the_angle_of_a_gradient_moves_both_angles() {
        let got = after("pattern_angle", "30");
        assert!((got.gradient_color.angle - 30f64.to_radians()).abs() < 1e-12);
        assert_eq!(got.pattern_angle, got.gradient_color.angle);
        // Everything else untouched.
        let mut expected = gradient();
        expected.gradient_color.angle = got.gradient_color.angle;
        expected.pattern_angle = got.gradient_color.angle;
        assert_eq!(got, expected);
    }
}
```
Run: `cargo test --locked --lib gradient_property_tests` → **PASS** (caratterizzano il comportamento di oggi). Se uno fallisce, il test descrive male il codice attuale: correggere il test, non il codice.

- [ ] **Step 2: Test di app per i colori dal pannello** (nuovo file `src/app/commands/hatch_gradient.rs`, dichiarato `mod hatch_gradient;` in `src/app/commands/mod.rs` accanto a `mod hatch_dialog;`)

```rust
//! The HATCH window's gradient side on the app: resolving "Use Current",
//! the colour picks, the keys with an overlay open. (Tests below also cover
//! the Properties panel's gradient colours.)

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};
    use codec::types::Color;

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        app
    }

    /// A drawing holding one gradient hatch built straight in the document.
    fn app_with_gradient(stops: usize) -> (OpenCADStudio, codec::Handle) {
        use crate::entities::hatch_fill::{apply_gradient, GradientSpec};
        use crate::scene::model::hatch_model::GradientKind;
        let mut app = new_app();
        let i = app.active_tab;
        let mut hatch = codec::entities::Hatch::solid();
        apply_gradient(
            &mut hatch,
            &GradientSpec {
                kind: GradientKind::Linear,
                invert: false,
                one_color: false,
                color1: Color::Index(1),
                color2: Color::Index(5),
                tint: 1.0,
                angle_rad: 0.0,
                centered: true,
            },
        );
        hatch.gradient_color.colors.truncate(stops);
        let handle = app.tabs[i].scene.add_entity(codec::EntityType::Hatch(hatch));
        app.tabs[i].scene.select_entity(handle, true);
        app.refresh_properties();
        (app, handle)
    }

    fn stops(app: &OpenCADStudio, handle: codec::Handle) -> Vec<(f64, Color)> {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::Hatch(h)) => {
                h.gradient_color.colors.iter().map(|e| (e.value, e.color)).collect()
            }
            other => panic!("not a hatch: {other:?}"),
        }
    }

    #[test]
    fn the_panel_colour_fields_replace_one_stop() {
        let (mut app, handle) = app_with_gradient(2);
        let _ = app.update(Message::PropColorFieldChanged {
            field: "gradient_color_2".into(),
            color: Color::Index(3),
        });
        assert_eq!(stops(&app, handle), vec![(0.0, Color::Index(1)), (1.0, Color::Index(3))]);
        let _ = app.update(Message::PropColorFieldChanged {
            field: "gradient_color_1".into(),
            color: Color::Index(2),
        });
        assert_eq!(stops(&app, handle), vec![(0.0, Color::Index(2)), (1.0, Color::Index(3))]);
    }

    #[test]
    fn a_gradient_with_no_stops_gets_them_created() {
        let (mut app, handle) = app_with_gradient(0);
        let _ = app.update(Message::PropColorFieldChanged {
            field: "gradient_color_2".into(),
            color: Color::Index(3),
        });
        assert_eq!(stops(&app, handle), vec![(0.0, Color::Index(7)), (1.0, Color::Index(3))]);
    }
}
```
Run: `cargo test --locked --lib hatch_gradient` → PASS sul codice attuale.

- [ ] **Step 3: Refactor** — `apply_geom_prop` in `entities/hatch.rs`:

sostituire i bracci `"fill_type"`, `"gradient_type"`, `"gradient_centered"` con

```rust
        "fill_type" => {
            crate::entities::hatch_fill::apply_gradient_patch(
                h,
                &crate::entities::hatch_fill::GradientPatch {
                    one_color: Some(value == "One color"),
                    ..Default::default()
                },
            );
            return;
        }
        "gradient_type" => {
            use crate::scene::model::hatch_model::GradientKind;
            if let Some(kind) = GradientKind::from_choice_label(value) {
                crate::entities::hatch_fill::apply_gradient_patch(
                    h,
                    &crate::entities::hatch_fill::GradientPatch {
                        kind: Some(kind),
                        ..Default::default()
                    },
                );
            }
            return;
        }
        "gradient_centered" => {
            let centered = if value == "toggle" {
                h.gradient_color.shift >= 0.5
            } else {
                value == "true"
            };
            crate::entities::hatch_fill::apply_gradient_patch(
                h,
                &crate::entities::hatch_fill::GradientPatch {
                    centered: Some(centered),
                    ..Default::default()
                },
            );
            return;
        }
```
e i due bracci numerici gradiente (`"pattern_angle" if h.gradient_color.enabled`, `"gradient_tint" if …`) con
`GradientPatch { angle_rad: Some(v.to_radians()), .. }` e `GradientPatch { tint: Some(v), .. }` (il clamp è dentro `apply_gradient_patch`).

`src/app/update/mod.rs` ~7268: il closure diventa

```rust
                    let patch = if field == "gradient_color_2" {
                        crate::entities::hatch_fill::GradientPatch {
                            color2: Some(color.clone()),
                            ..Default::default()
                        }
                    } else {
                        crate::entities::hatch_fill::GradientPatch {
                            color1: Some(color.clone()),
                            ..Default::default()
                        }
                    };
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                        if let Some(codec::EntityType::Hatch(h)) =
                            app.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            crate::entities::hatch_fill::apply_gradient_patch(h, &patch);
                        }
                    });
```
(il ciclo `while … colors.push` sparisce). Lasciare invariata la riga `populate_hatches_from_document` che segue.

- [ ] **Step 4:** `cargo test --locked --lib gradient_property_tests hatch_gradient hatch_fill` e poi `cargo test --locked --lib hatch` → tutti PASS (prima e dopo il refactor identici).

- [ ] **Step 5: Commit**

```bash
git add src/entities/hatch.rs src/app/update/mod.rs src/app/commands/hatch_gradient.rs src/app/commands/mod.rs && git commit -m "Proprieta': i campi dello sfumato passano da apply_gradient_patch (test di caratterizzazione prima del refactor)"
```

---

### Task 4: `HatchPattern::Gradient` porta `one_color` e `tint`; `add_hatch` e il lettore usano `hatch_fill`

**Files:**
- Modify: `src/scene/model/hatch_model.rs:184-191` (variante `Gradient`)
- Modify: `src/modules/draw/draw/hatch.rs` (`GradientCommand::make_hatch` ~1202)
- Modify: `src/scene/entity.rs` (`hatch_model_from_dxf` ~2190-2214; `add_hatch` ~2943-2987)
- Modify: `src/entities/hatch_fill.rs` (aggiungere `GradientSpec::model_pattern`)
- Modify: altri siti che non compilano (`pipeline/hatch_gpu/storage.rs:270`, `texture.rs:248`, `scene/view/render.rs:1505,3274`, `io/pdf_export.rs:1484`, `wipeout_gpu.rs:194`): aggiungere `..` ai pattern che destrutturano la variante senza `..`

**Interfaces:**
- Consumes: Task 1.
- Produces:
  - variante `HatchPattern::Gradient { angle_deg: f32, color2: [f32;4], kind: GradientKind, invert: bool, shift: f32, one_color: bool, tint: f32 }`
  - `impl GradientSpec { pub fn model_pattern(&self) -> (HatchPattern, [f32; 4]) }` — la variante `Gradient` con `color2 = rgba(effective_color2())` e il colore 1 in RGBA (secondo elemento)
  - `Scene::add_hatch` scrive sempre `is_single_color`, `color_tint` e le fermate via `apply_gradient`; il lettore `hatch_model_from_dxf` ricostruisce `one_color`/`tint` e il `color2` effettivo con `tinted_second_color`.

- [ ] **Step 1: Test rossi** in `src/scene/entity.rs` (modulo `#[cfg(test)]` in coda; se non esiste, crearlo):

```rust
#[cfg(test)]
mod gradient_fill_tests {
    use super::*;
    use crate::entities::hatch_fill::{rgba_of, tinted_second_color};
    use crate::modules::draw::draw::hatch::GradientCommand;
    use crate::command::{CadCommand, CmdResult};
    use crate::scene::model::hatch_model::{GradientKind, HatchPattern};

    /// The model a click inside a 10 x 10 square makes.
    fn model() -> HatchModel {
        let ring = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let mut command = GradientCommand::new(vec![ring], Default::default());
        match command.on_point(glam::DVec3::new(5.0, 5.0, 0.0)) {
            CmdResult::CommitHatch(model) => model,
            _ => panic!("expected CommitHatch"),
        }
    }

    fn with_pattern(mut model: HatchModel, pattern: HatchPattern) -> HatchModel {
        model.pattern = pattern;
        model
    }

    fn stored(scene: &Scene, handle: Handle) -> codec::entities::Hatch {
        match scene.document.get_entity(handle) {
            Some(EntityType::Hatch(h)) => h.clone(),
            other => panic!("not a hatch: {other:?}"),
        }
    }

    #[test]
    fn a_two_colour_gradient_is_written_with_both_stops_and_a_zero_tint_flag() {
        let mut scene = Scene::new();
        let handle = scene.add_hatch(model(), None, None);
        let h = stored(&scene, handle);
        assert!(h.gradient_color.enabled && h.is_solid);
        assert!(!h.gradient_color.is_single_color);
        assert_eq!(h.gradient_color.colors.len(), 2);
    }

    #[test]
    fn a_one_colour_gradient_persists_the_flag_the_tint_and_the_tinted_stop() {
        let base = model();
        let HatchPattern::Gradient { angle_deg, kind, invert, shift, .. } = base.pattern.clone() else {
            panic!("a gradient")
        };
        // The model's first colour is a whole-byte colour, as a stored stop is.
        let color1 = crate::entities::hatch_fill::rgba_of(
            crate::entities::hatch_fill::DEFAULT_GRADIENT_COLOR1,
        )
        .unwrap();
        let tinted = tinted_second_color(color1, 0.25);
        let mut one = with_pattern(
            base,
            HatchPattern::Gradient {
                angle_deg,
                color2: rgba_of(tinted).unwrap(),
                kind,
                invert,
                shift,
                one_color: true,
                tint: 0.25,
            },
        );
        one.color = color1;
        let mut scene = Scene::new();
        let handle = scene.add_hatch(one, None, None);
        let h = stored(&scene, handle);
        assert!(h.gradient_color.is_single_color, "was never written before");
        assert_eq!(h.gradient_color.color_tint, 0.25);
        assert_eq!(h.gradient_color.colors[1].color, tinted);
        // The rebuilt model agrees.
        let rebuilt = scene.hatches.get(&handle).expect("model");
        let HatchPattern::Gradient { one_color, tint, color2, .. } = &rebuilt.pattern else {
            panic!("rebuilt as a gradient")
        };
        assert!(*one_color);
        assert_eq!(*tint, 0.25);
        assert_eq!(*color2, rgba_of(tinted).unwrap(), "the same second colour, to the bit");
    }

    #[test]
    fn the_kind_and_inversion_survive_the_round_trip() {
        let base = model();
        let HatchPattern::Gradient { angle_deg, color2, shift, one_color, tint, .. } = base.pattern.clone() else {
            panic!("a gradient")
        };
        let inverted = with_pattern(
            base,
            HatchPattern::Gradient {
                angle_deg, color2, shift, one_color, tint,
                kind: GradientKind::Cylinder,
                invert: true,
            },
        );
        let mut scene = Scene::new();
        let handle = scene.add_hatch(inverted, None, None);
        assert_eq!(stored(&scene, handle).gradient_color.name, "INVCYLINDER");
    }
}
```
(`GradientCommand::new` aspetta `outlines, boundary_sources`; il secondo argomento `Default::default()` è la mappa vuota.)

- [ ] **Step 2:** `cargo test --locked --lib gradient_fill_tests 2>&1 | tail` → errore di compilazione (campi `one_color`/`tint` inesistenti).

- [ ] **Step 3: Implementare**

1. `hatch_model.rs`: aggiungere alla variante `Gradient` i campi `one_color: bool,` e `tint: f32,` con doc (`/// Colour 2 is the tint of colour 1 (a one-colour gradient).`, `/// 0 = black end, 1 = white end; only with `one_color`.`).
2. `GradientCommand::make_hatch`: aggiungere `one_color: false, tint: 1.0,`.
3. `hatch_fill.rs`:
```rust
impl GradientSpec {
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
```
4. Lettore (`hatch_model_from_dxf`): nel ramo gradiente, sostituire il calcolo di `color2` con
```rust
            let single = dxf.gradient_color.is_single_color;
            let tint = dxf.gradient_color.color_tint as f32;
            let color2 = if single {
                crate::entities::hatch_fill::rgba_of(
                    crate::entities::hatch_fill::tinted_second_color(color1, tint),
                )
                .unwrap_or(color1)
            } else {
                stop(1).unwrap_or(color)
            };
```
e aggiungere `one_color: single, tint,` alla variante costruita (la `use` di `gradient_tint_color` aggiunta nel Task 1 non serve più qui: toglierla se `unused`).
5. `add_hatch`: sostituire l'intero blocco `if let HatchPattern::Gradient { … } = &model.pattern { … }` (≈ righe 2947-2987) con
```rust
        if let crate::scene::model::hatch_model::HatchPattern::Gradient {
            angle_deg,
            color2,
            kind,
            invert,
            shift,
            one_color,
            tint,
        } = &model.pattern
        {
            let to_color = |c: [f32; 4]| codec::types::Color::Rgb {
                r: (c[0] * 255.0).round().clamp(0.0, 255.0) as u8,
                g: (c[1] * 255.0).round().clamp(0.0, 255.0) as u8,
                b: (c[2] * 255.0).round().clamp(0.0, 255.0) as u8,
            };
            // `color2` of the model is already the effective second colour.
            crate::entities::hatch_fill::apply_gradient(
                &mut dxf,
                &crate::entities::hatch_fill::GradientSpec {
                    kind: *kind,
                    invert: *invert,
                    one_color: *one_color,
                    color1: to_color(model.color),
                    color2: to_color(*color2),
                    tint: *tint as f64,
                    angle_rad: (*angle_deg as f64).to_radians(),
                    centered: *shift < 0.5,
                },
            );
        }
```
Attenzione: con `one_color` vero `apply_gradient` ricalcola la seconda fermata da `color1`+`tint` (stesso risultato di `color2` del modello); con due colori scrive `color2`. ✓.
6. Correggere gli altri `match` che non compilano aggiungendo `..` (il compilatore li elenca).

- [ ] **Step 4:** `cargo test --locked --lib gradient_fill_tests hatch_fill hatch` → PASS; `cargo check --locked` pulito.

- [ ] **Step 5: Mutazione e commit**

(M) in `add_hatch` non passare `*one_color` → `a_one_colour_gradient_persists…` fallisce. Ripristinare.

```bash
git add -A src && git commit -m "Sfumato: add_hatch scrive is_single_color e color_tint tramite hatch_fill; il modello porta one_color e tint"
```

---

### Task 5: Dati della finestra — scheda, colore, `GradientSettings`, `Field`, `State::apply`

**Files:**
- Modify: `src/modules/draw/draw/hatch_settings.rs` (dati puri)
- Modify: `src/ui/window/hatch_dialog.rs` (`Field`, `HatchColorSlot`, `State.color_list`, `apply`, `fields_valid`, messaggi)
- Modify: `src/modules/draw/draw/hatch.rs` (`HatchCommand::with_settings` e un campo `gradient: Option<GradientSpec>`)
- Modify (solo compilazione): i letterali `HatchSettings { … }` nei test (es. `src/app/commands/hatch_dialog.rs` ~2545): aggiungere `..HatchSettings::default()`; i test che leggono `resolved.pattern/angle_rad/scale` di `ResolvedSettings` passano a `resolved.fill`.

**Interfaces:**
- Consumes: `GradientSpec`, `DEFAULT_GRADIENT_COLOR1/2` (Task 1); `GradientKind::CHOICES`.
- Produces (in `hatch_settings`):
  - `enum FillTab { Hatch, Gradient }` (`Clone, Copy, Debug, PartialEq, Eq`)
  - `enum HatchColor { UseCurrent, Color(AcadColor) }` (`Clone, Copy, Debug, PartialEq`)
  - `struct GradientSettings { shape: usize, one_color: bool, color1: AcadColor, color2: AcadColor, tint: f32, centered: bool, angle: String }` con `Default`, `angle_error() -> bool`, `kind_invert() -> (GradientKind, bool)`, `spec() -> Option<GradientSpec>`
  - `HatchSettings` guadagna `tab: FillTab`, `color: HatchColor`, `gradient: GradientSettings` (predefiniti: `Hatch`, `UseCurrent`, default)
  - `ResolvedSettings { associative, separate, retain, island_style, fill: ResolvedFill }` e `enum ResolvedFill { Hatch { pattern: String, angle_rad: f32, scale: f32, color: HatchColor }, Gradient(GradientSpec) }`
  - `HatchSettings::resolve()` **dipende da `tab`**
  - `HatchSettings::gradient_angle_error()`
- Produces (in `ui/window/hatch_dialog.rs`):
  - `enum HatchColorSlot { Fill, Gradient1, Gradient2 }` (`Clone, Copy, Debug, PartialEq, Eq`) con `fn field(self, AcadColor) -> Field`
  - `Field::{Tab(FillTab), Color(HatchColor), GradientShape(usize), GradientOneColor(bool), GradientColor1(AcadColor), GradientColor2(AcadColor), GradientTint(f32), GradientCentered(bool), GradientAngle(String), ColorList(Option<HatchColorSlot>), SelectColor(HatchColorSlot)}`
  - `State.color_list: Option<HatchColorSlot>`; `State::gradient_angle_message() -> Option<String>`
- Produces (in `HatchCommand`): campo privato `gradient: Option<GradientSpec>` impostato da `with_settings` (lo consuma il Task 6).

- [ ] **Step 1: Test rossi** — in `hatch_settings.rs`, nel modulo `tests` esistente:

```rust
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
        let ResolvedFill::Gradient(spec) = s.resolve().unwrap().fill else {
            panic!("a gradient")
        };
        assert_eq!((spec.kind, spec.invert), (GradientKind::Cylinder, true));
        assert!(spec.one_color && !spec.centered);
        assert!((spec.tint - 0.4).abs() < 1e-6);
        assert!((spec.angle_rad - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    }

    #[test]
    fn an_out_of_range_shape_is_held_to_the_last_choice() {
        let g = GradientSettings { shape: 99, ..GradientSettings::default() };
        assert_eq!(g.kind_invert(), *crate::scene::model::hatch_model::GradientKind::CHOICES.last().unwrap());
    }
```
(aggiungere in testa ai test `use codec::types::Color as AcadColor;`). In `ui/window/hatch_dialog.rs`, nel modulo `tests`:

```rust
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
    }

    #[test]
    fn the_slot_makes_the_matching_field() {
        use codec::types::Color;
        assert!(matches!(
            HatchColorSlot::Fill.field(Color::Index(3)),
            Field::Color(HatchColor::Color(c)) if c == Color::Index(3)
        ));
        assert!(matches!(HatchColorSlot::Gradient1.field(Color::Index(3)), Field::GradientColor1(_)));
        assert!(matches!(HatchColorSlot::Gradient2.field(Color::Index(3)), Field::GradientColor2(_)));
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
```
(la funzione `edit_state` esiste già nei test; `state()`/`one_region()` pure. Aggiungere le `use` mancanti: `use crate::modules::draw::draw::hatch_settings::{FillTab, HatchColor};`.)

- [ ] **Step 2:** `cargo test --locked --lib hatch_settings 2>&1 | tail -15` → errori di compilazione (tipi mancanti).

- [ ] **Step 3: Implementare** `hatch_settings.rs`:

```rust
use codec::types::Color as AcadColor;

use crate::entities::hatch_fill::{GradientSpec, DEFAULT_GRADIENT_COLOR1, DEFAULT_GRADIENT_COLOR2};
use crate::scene::model::hatch_model::GradientKind;

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

    /// `None` while the angle is unusable.
    pub fn spec(&self) -> Option<GradientSpec> {
        let angle = parse_angle_deg(&self.angle)?;
        let (kind, invert) = self.kind_invert();
        Some(GradientSpec {
            kind,
            invert,
            one_color: self.one_color,
            color1: self.color1,
            color2: self.color2,
            tint: self.tint.clamp(0.0, 1.0) as f64,
            angle_rad: (angle as f64).to_radians(),
            centered: self.centered,
        })
    }
}
```
`HatchSettings`: aggiungere i tre campi (+ `Default`), e sostituire `ResolvedSettings`/`resolve()`:

```rust
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

    pub fn gradient_angle_error(&self) -> bool {
        self.gradient.angle_error()
    }

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
```
Aggiornare i test esistenti `default_settings_resolve` e simili: `resolved.fill` invece di `.pattern/.angle_rad/.scale`.

`ui/window/hatch_dialog.rs`:
```rust
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
```
`Field` nuove varianti (vedi Interfaces); `State` campo `color_list: Option<HatchColorSlot>` (init `None` in `new`); in `apply` (prima del `match field` generale, dentro il blocco `if let Some(edit)` aggiungere `Field::Color(HatchColor::UseCurrent) => return,` e `Field::Tab(_) => {}` consentito):

```rust
            Field::Tab(tab) => self.settings.tab = tab,
            Field::Color(color) => {
                self.settings.color = color;
                self.color_list = None;
            }
            Field::GradientShape(index) => {
                self.settings.gradient.shape = index.min(GradientKind::CHOICES.len() - 1)
            }
            Field::GradientOneColor(on) => self.settings.gradient.one_color = on,
            Field::GradientColor1(color) => {
                self.settings.gradient.color1 = color;
                self.color_list = None;
            }
            Field::GradientColor2(color) => {
                self.settings.gradient.color2 = color;
                self.color_list = None;
            }
            Field::GradientTint(tint) => self.settings.gradient.tint = tint.clamp(0.0, 1.0),
            Field::GradientCentered(on) => self.settings.gradient.centered = on,
            Field::GradientAngle(text) => self.settings.gradient.angle = text,
            Field::ColorList(slot) => self.color_list = slot,
            // Opening "Select Color" is the app's job (it needs the colour
            // window); the state only closes the list.
            Field::SelectColor(_) => self.color_list = None,
```
`gradient_angle_message()`: come `angle_message` ma su `settings.gradient.angle_error()`.

`fields_valid()` per creazione resta `self.settings.resolve().is_some()` (già tab-aware). `pattern_message()` e i messaggi Angle/Scale del Hatch tab restano.

`HatchCommand`: aggiungere il campo `gradient: Option<GradientSpec>` (inizializzato `None` in `new`), e riscrivere `with_settings`:

```rust
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
```

- [ ] **Step 4:** `cargo test --locked --lib hatch 2>&1 | tail -15` → tutti PASS (compresi i test preesistenti, adattati ai letterali). `cargo check --locked` pulito (può comparire un warning `gradient` mai letto: sparisce nel Task 6).

- [ ] **Step 5: Mutazione e commit**

(M) in `resolve()` ignorare `self.tab` (usare sempre il ramo Hatch) → `resolve_follows_the_tab` fallisce. Ripristinare.

```bash
git add -A src && git commit -m "Finestra Hatch: scheda, colore e impostazioni dello sfumato nei dati puri; resolve() segue la scheda"
```

---

### Task 6: `HatchCommand` — riempimento sfumato, stile di creazione, colore di anteprima

**Files:**
- Modify: `src/modules/draw/draw/hatch.rs` (`HatchCommand`: campi, builder, `make_hatch`, `on_enter`, `preview_models`; test nel modulo `tests`)

**Interfaces:**
- Consumes: Task 4 (`GradientSpec::model_pattern`), Task 5 (`gradient`, `with_settings`).
- Produces:
  - `HatchCommand::with_creation_style(self, Option<(codec::types::Color, codec::types::Transparency)>) -> Self`
  - `HatchCommand::with_preview_color(self, Option<[f32; 4]>) -> Self`
  - `make_hatch` con `gradient` impostato → `HatchModel` con `HatchPattern::Gradient{..}` (color2 effettivo, `one_color`, `tint`), `name = kind.dxf_name(invert)`, `color` = colore 1, `angle_offset` = angolo dello sfumato, `pattern_origin = None`
  - `on_enter` usa `entity_style()` in tutti i rami (`CommitStyledHatch`, `CommitHatches{entity_style}`, `CommitHatchWithBoundaries{entity_style}`)
  - `preview_models`: pattern/solido col colore di `with_preview_color` (blu se `None`); sfumato coi due colori veri, alpha 0.75 entrambi.

- [ ] **Step 1: Test rossi** (modulo `tests` di `hatch.rs`, che ha già `rect`, `sources_for`, `committed`):

```rust
    use crate::entities::hatch_fill::{rgba_of, tinted_second_color, GradientSpec};
    use crate::modules::draw::draw::hatch_settings::{
        FillTab, GradientSettings, HatchColor, ResolvedFill,
    };

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
        let style = (Color::Index(1), Transparency::default());
        let settings = HatchSettings::default();
        // One hatch.
        let mut one = square_command()
            .with_settings(&settings.resolve().unwrap())
            .with_creation_style(Some(style));
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
            .with_creation_style(Some(style));
        match command.on_enter() {
            CmdResult::CommitHatches { hatches, entity_style } => {
                assert_eq!(hatches.len(), 1);
                assert_eq!(entity_style, Some(style));
            }
            _ => panic!("separate hatches"),
        }
        // Retained boundaries.
        let mut retain = settings.clone();
        retain.set_retain(true);
        let mut command = square_command()
            .with_settings(&retain.resolve().unwrap())
            .with_creation_style(Some(style));
        match command.on_enter() {
            CmdResult::CommitHatchWithBoundaries { entity_style, .. } => {
                assert_eq!(entity_style, Some(style))
            }
            _ => panic!("hatch with boundaries"),
        }
    }

    #[test]
    fn without_a_creation_style_the_commit_is_what_it_always_was() {
        let mut command = square_command().with_settings(&HatchSettings::default().resolve().unwrap());
        assert!(matches!(command.on_enter(), CmdResult::CommitHatch(_)));
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
```
(Se `Transparency` non implementa `Default`, usare `Transparency::ByLayer` o il valore costante disponibile in `codec::types`; verificare con grep. Se `CmdResult::CommitHatches.entity_style` non implementa `PartialEq` per `assert_eq!`, confrontare con `matches!`.)

- [ ] **Step 2:** `cargo test --locked --lib hatch 2>&1 | tail` → errori (`with_creation_style` mancante, ecc.).

- [ ] **Step 3: Implementare** in `hatch.rs`:

Campi in `HatchCommand` (`creation_style: Option<(codec::types::Color, codec::types::Transparency)>`, `preview_color: Option<[f32; 4]>`), inizializzati a `None` in `new`.

```rust
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

    fn entity_style(&self) -> Option<(codec::types::Color, codec::types::Transparency)> {
        self.creation_style.or_else(|| {
            self.inherited
                .as_ref()
                .map(|(_, color, transparency)| (*color, *transparency))
        })
    }
```
`make_hatch`: subito **prima** di `if let Some((source, _, _)) = &self.inherited {` inserire

```rust
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
```
`on_enter`: sostituire i quattro punti che costruiscono lo stile da `self.inherited` con `self.entity_style()`:

```rust
        } else if matches!(self.mode, HatchMode::Manual) {
            let mut hatch = self.make_hatch(rings);
            if let Some(path) = self.manual_boundary_path() {
                hatch.boundary_paths = Some(std::sync::Arc::new(vec![path]));
            }
            self.commit_one(hatch)
        } else if self.separate_hatches && !self.retain_boundaries {
            …
            CmdResult::CommitHatches { hatches, entity_style: self.entity_style() }
        } else if self.retain_boundaries {
            CmdResult::CommitHatchWithBoundaries { hatch: …, boundaries: …, entity_style: self.entity_style() }
        } else {
            self.commit_one(self.make_hatch(rings))
        }
```
con
```rust
    fn commit_one(&self, hatch: HatchModel) -> CmdResult {
        match self.entity_style() {
            Some((color, transparency)) => CmdResult::CommitStyledHatch { hatch, color, transparency },
            None => CmdResult::CommitHatch(hatch),
        }
    }
```
`preview_models`: nel `.map(|rings| {…})` sostituire `model.color = [0.15, 0.55, 1.0, 0.75];` con

```rust
                if let HatchPattern::Gradient { color2, .. } = &mut model.pattern {
                    // The real colours, translucent like every preview.
                    color2[3] = 0.75;
                    model.color[3] = 0.75;
                } else {
                    model.color = self.preview_color.unwrap_or([0.15, 0.55, 1.0, 0.75]);
                }
```

- [ ] **Step 4:** `cargo test --locked --lib hatch` → PASS (anche i test dei flussi preesistenti: `-HATCH` con `inherited` deve restare identico).

- [ ] **Step 5: Mutazione e commit**

(M) in `make_hatch` usare `color2` grezzo al posto di `model_pattern()` → `the_models_second_colour_is_the_effective_one` e `a_one_colour_preview…` falliscono. Ripristinare.

```bash
git add src/modules/draw/draw/hatch.rs && git commit -m "HatchCommand: riempimento sfumato, stile di creazione e colore di anteprima risolti fuori dal comando"
```

---

### Task 7: Creazione dall'app — HATCH/GRADIENT/`-GRADIENT`, colore corrente, anteprima

**Files:**
- Modify: `src/app/commands/hatch_gradient.rs` (creato nel Task 3: aggiungere `impl OpenCADStudio` con la risoluzione del colore, sopra il modulo `tests`)
- Modify: `src/app/commands/hatch_dialog.rs` (`hatch_dialog_open(tab)`, `hatch_dialog_ok`, `hatch_dialog_preview`)
- Modify: `src/app/commands/draw.rs:738-742` e `:825` (arm HATCH/GRADIENT)
- Modify: `src/app/commands/mod.rs` (~533, elenco comandi: aggiungere `"-GRADIENT"`)
- Modify: `src/modules/draw/draw/hatch.rs:1799` (`names: &["GRADIENT", "-GRADIENT"]`)

**Interfaces:**
- Consumes: Task 5 (`FillTab`, `HatchColor`), Task 6 (`with_creation_style`, `with_preview_color`).
- Produces:
  - `OpenCADStudio::hatch_dialog_open(&mut self, tab: FillTab) -> Task<Message>`
  - `OpenCADStudio::hatch_creation_style(&self, i: usize, color: HatchColor) -> (Color, Transparency)` (`pub(in crate::app)`)
  - `OpenCADStudio::hatch_preview_rgba(&self, i: usize, color: HatchColor) -> [f32; 4]` (`pub(in crate::app)`; alpha 0.75)

- [ ] **Step 1: Test rossi** — nel modulo `tests` di `src/app/commands/hatch_gradient.rs` (aggiungere alle `use` e agli helper già presenti dal Task 3):

```rust
    use crate::app::ModalKind;
    use crate::entities::hatch_fill::{read_gradient, rgba_of, tinted_second_color, FillKind};
    use crate::modules::draw::draw::hatch_settings::{
        FillTab, HatchColor, HatchRegion, RegionOrigin,
    };
    use crate::scene::model::hatch_model::GradientKind;
    use crate::ui::window::hatch_dialog::Field;

    fn add_line(app: &mut OpenCADStudio, x1: f64, y1: f64, x2: f64, y2: f64) {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Line(codec::entities::Line::from_points(
                codec::types::Vector3::new(x1, y1, 0.0),
                codec::types::Vector3::new(x2, y2, 0.0),
            )));
    }

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

    /// Open the window with `command`, give it the rectangle and apply `fields`.
    fn open_with(app: &mut OpenCADStudio, command: &str, fields: &[Field]) {
        let _ = app.dispatch_command(command);
        app.hatch_dialog.as_mut().expect("the window opened").regions.push(region());
        for field in fields {
            let _ = app.update(Message::HatchDialogField(field.clone()));
        }
    }

    fn all_hatches(app: &OpenCADStudio) -> Vec<codec::entities::Hatch> {
        app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                codec::EntityType::Hatch(hatch) => Some(hatch.clone()),
                _ => None,
            })
            .collect()
    }

    fn the_hatch(app: &OpenCADStudio) -> codec::entities::Hatch {
        let mut hatches = all_hatches(app);
        assert_eq!(hatches.len(), 1, "exactly one hatch was made");
        hatches.remove(0)
    }

    fn ok(app: &mut OpenCADStudio) {
        let _ = app.update(Message::HatchDialogOk);
    }

    #[test]
    fn hatch_opens_on_the_hatch_tab_and_gradient_on_the_gradient_tab() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        assert_eq!(app.hatch_dialog.as_ref().unwrap().settings.tab, FillTab::Hatch);
        let _ = app.update(Message::CloseModal);
        let _ = app.dispatch_command("GRADIENT");
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(app.hatch_dialog.as_ref().unwrap().settings.tab, FillTab::Gradient);
        assert_eq!(app.tabs[app.active_tab].last_cmd.as_deref(), Some("GRADIENT"));
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
    }

    #[test]
    fn the_remembered_tab_never_decides_how_the_window_opens() {
        let mut app = app_with_rectangle();
        app.hatch_last.tab = FillTab::Gradient;
        let _ = app.dispatch_command("HATCH");
        assert_eq!(app.hatch_dialog.as_ref().unwrap().settings.tab, FillTab::Hatch);
    }

    #[test]
    fn the_gradient_tab_makes_a_gradient_whatever_the_pattern_name() {
        let mut app = app_with_rectangle();
        // The Hatch tab's pattern is SOLID, but the tab in force is Gradient.
        open_with(
            &mut app,
            "GRADIENT",
            &[Field::Pattern("SOLID".into()), Field::GradientShape(2)],
        );
        ok(&mut app);
        let hatch = the_hatch(&app);
        assert_eq!(FillKind::of(&hatch), FillKind::Gradient);
        assert_eq!(hatch.gradient_color.name, "INVCYLINDER");
    }

    #[test]
    fn the_hatch_tab_with_solid_makes_a_solid_and_not_a_gradient() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "HATCH", &[Field::Pattern("SOLID".into())]);
        ok(&mut app);
        let hatch = the_hatch(&app);
        assert_eq!(FillKind::of(&hatch), FillKind::Solid);
        assert!(!hatch.gradient_color.enabled);
    }

    #[test]
    fn every_one_of_the_nine_shapes_is_made_and_read_back() {
        for (shape, (kind, invert)) in GradientKind::CHOICES.iter().copied().enumerate() {
            let mut app = app_with_rectangle();
            open_with(&mut app, "GRADIENT", &[Field::GradientShape(shape)]);
            ok(&mut app);
            let hatch = the_hatch(&app);
            assert_eq!(hatch.gradient_color.name, kind.dxf_name(invert), "shape {shape}");
            let read = read_gradient(&hatch);
            assert_eq!((read.kind, read.invert), (kind, invert), "shape {shape}");
        }
    }

    #[test]
    fn a_one_colour_gradient_keeps_its_tint_and_ignores_the_hidden_colour() {
        let mut app = app_with_rectangle();
        let red = Color::Index(1);
        open_with(
            &mut app,
            "GRADIENT",
            &[
                Field::GradientOneColor(true),
                Field::GradientTint(0.25),
                Field::GradientColor1(red),
                Field::GradientColor2(Color::Index(3)),
                Field::GradientCentered(false),
                Field::GradientAngle("45".into()),
            ],
        );
        ok(&mut app);
        let hatch = the_hatch(&app);
        let g = &hatch.gradient_color;
        assert!(g.is_single_color);
        assert_eq!(g.color_tint, 0.25);
        assert_eq!(g.colors[0].color, red);
        assert_eq!(
            g.colors[1].color,
            tinted_second_color(rgba_of(red).unwrap(), 0.25),
            "the second stop is the tint, not Color 2"
        );
        assert_eq!(g.shift, 1.0);
        assert!((g.angle - 45f64.to_radians()).abs() < 1e-9);
        // The model the scene shows agrees with the stored one.
        let handle = *app.tabs[app.active_tab].scene.hatches.keys().next().unwrap();
        let model = &app.tabs[app.active_tab].scene.hatches[&handle];
        match &model.pattern {
            crate::scene::model::hatch_model::HatchPattern::Gradient { one_color, tint, .. } => {
                assert!(*one_color);
                assert_eq!(*tint, 0.25);
            }
            other => panic!("a gradient model, got {other:?}"),
        }
    }

    #[test]
    fn use_current_follows_the_ribbon_colour_and_a_chosen_colour_wins() {
        // Default current colour is ByLayer: the entity is ByLayer, as before.
        let mut app = app_with_rectangle();
        open_with(&mut app, "HATCH", &[]);
        ok(&mut app);
        assert_eq!(the_hatch(&app).common.color, Color::ByLayer);

        let mut app = app_with_rectangle();
        app.ribbon.active_color = Color::Index(1);
        open_with(&mut app, "HATCH", &[]);
        ok(&mut app);
        assert_eq!(the_hatch(&app).common.color, Color::Index(1));

        let mut app = app_with_rectangle();
        app.ribbon.active_color = Color::Index(1);
        open_with(&mut app, "HATCH", &[Field::Color(HatchColor::Color(Color::Index(3)))]);
        ok(&mut app);
        assert_eq!(the_hatch(&app).common.color, Color::Index(3));
    }

    #[test]
    fn use_current_also_applies_to_solids_and_separate_hatches() {
        let mut app = app_with_rectangle();
        app.ribbon.active_color = Color::Index(5);
        open_with(
            &mut app,
            "HATCH",
            &[Field::Pattern("SOLID".into()), Field::Separate(true)],
        );
        ok(&mut app);
        let hatch = the_hatch(&app);
        assert_eq!(hatch.common.color, Color::Index(5));
        assert!(hatch.is_solid);
    }

    #[test]
    fn the_gradient_tab_does_not_use_the_fill_colour() {
        let mut app = app_with_rectangle();
        app.ribbon.active_color = Color::Index(1);
        open_with(&mut app, "GRADIENT", &[]);
        ok(&mut app);
        assert_eq!(the_hatch(&app).common.color, Color::ByLayer);
    }

    #[test]
    fn ok_leaves_one_undo_step() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "GRADIENT", &[]);
        let before = app.tabs[app.active_tab].history.undo_stack.len();
        ok(&mut app);
        assert_eq!(app.tabs[app.active_tab].history.undo_stack.len(), before + 1);
    }

    #[test]
    fn dash_gradient_and_scripted_gradient_run_without_a_window() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("-GRADIENT");
        assert!(app.active_modal.is_none() && app.hatch_dialog.is_none());
        let i = app.active_tab;
        assert_eq!(app.tabs[i].active_cmd.as_ref().map(|c| c.name()), Some("GRADIENT"));
        let _ = app.update(Message::CommandEscape);

        app.scripted_dispatch = true;
        let _ = app.dispatch_command("GRADIENT");
        app.scripted_dispatch = false;
        assert!(app.active_modal.is_none() && app.hatch_dialog.is_none());
        assert_eq!(app.tabs[i].active_cmd.as_ref().map(|c| c.name()), Some("GRADIENT"));
    }

    #[test]
    fn dash_gradient_is_registered() {
        assert!(inventory::iter::<crate::command::CommandRegistration>()
            .any(|registration| registration.names.contains(&"-GRADIENT")));
    }

    #[test]
    fn the_remembered_settings_keep_both_tabs() {
        let mut app = app_with_rectangle();
        open_with(
            &mut app,
            "GRADIENT",
            &[
                Field::Scale("3".into()),
                Field::GradientShape(6),
                Field::Color(HatchColor::Color(Color::Index(2))),
            ],
        );
        ok(&mut app);
        assert_eq!(app.hatch_last.scale, "3");
        assert_eq!(app.hatch_last.gradient.shape, 6);
        assert_eq!(app.hatch_last.color, HatchColor::Color(Color::Index(2)));
    }

    #[test]
    fn add_pick_points_works_from_the_gradient_tab() {
        use crate::command::CadCommand;
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("GRADIENT");
        let _ = app.update(Message::HatchDialogAdd(
            crate::ui::window::hatch_dialog::AddKind::Points,
        ));
        let i = app.active_tab;
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("a collector is running")
            .on_point(glam::DVec3::new(10.0, 5.0, 0.0));
        let _ = app.apply_cmd_result(result);
        let _ = app.feed_command(crate::command::StepInput::Enter);
        let state = app.hatch_dialog.as_ref().unwrap();
        assert_eq!(state.regions.len(), 1);
        assert_eq!(state.settings.tab, FillTab::Gradient, "back on the same tab");
        ok(&mut app);
        assert_eq!(FillKind::of(&the_hatch(&app)), FillKind::Gradient);
    }

    #[test]
    fn separate_gradients_are_one_each() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("GRADIENT");
        let state = app.hatch_dialog.as_mut().unwrap();
        state.regions.push(region());
        state.regions.push((
            HatchRegion {
                rings: vec![vec![[30.0, 0.0], [50.0, 0.0], [50.0, 10.0], [30.0, 10.0]]],
            },
            RegionOrigin::Points,
        ));
        let _ = app.update(Message::HatchDialogField(Field::Separate(true)));
        ok(&mut app);
        let hatches = all_hatches(&app);
        assert_eq!(hatches.len(), 2);
        assert!(hatches.iter().all(|h| FillKind::of(h) == FillKind::Gradient));
    }

    // Review focus: a gradient on a layout and on a rotated plane.
    #[test]
    fn a_gradient_in_a_layout_lies_on_the_default_plane() {
        let mut app = new_app();
        let i = app.active_tab;
        app.tabs[i].scene.current_layout = "Layout1".to_string();
        for (a, b, c, d) in [(0.0, 0.0, 20.0, 0.0), (20.0, 0.0, 20.0, 10.0), (20.0, 10.0, 0.0, 10.0), (0.0, 10.0, 0.0, 0.0)] {
            add_line(&mut app, a, b, c, d);
        }
        open_with(&mut app, "GRADIENT", &[]);
        ok(&mut app);
        let hatch = the_hatch(&app);
        assert_eq!(hatch.normal, codec::types::Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(hatch.elevation, 0.0);
        assert_eq!(FillKind::of(&hatch), FillKind::Gradient);
    }

    #[test]
    fn a_gradient_on_a_rotated_plane_is_not_made_on_the_xy_plane() {
        let mut app = new_app();
        let i = app.active_tab;
        // The XZ plane at y = 5: local x along world X, local y along world Z.
        let mut ucs = codec::tables::Ucs::new("*ACTIVE*");
        ucs.origin = codec::types::Vector3::new(0.0, 5.0, 0.0);
        ucs.x_axis = codec::types::Vector3::new(1.0, 0.0, 0.0);
        ucs.y_axis = codec::types::Vector3::new(0.0, 0.0, 1.0);
        app.tabs[i].active_ucs = Some(ucs);
        open_with(&mut app, "GRADIENT", &[]);
        ok(&mut app);
        let hatch = the_hatch(&app);
        assert!(hatch.normal.z.abs() < 1.0e-9, "{:?}", hatch.normal);
        assert!(hatch.normal.y.abs() > 0.99, "{:?}", hatch.normal);
    }

    // ── Preview ────────────────────────────────────────────────────────────

    fn preview_models(app: &mut OpenCADStudio) -> Vec<crate::scene::model::hatch_model::HatchModel> {
        let _ = app.update(Message::HatchDialogPreview);
        let i = app.active_tab;
        app.tabs[i]
            .active_cmd
            .as_ref()
            .expect("the preview runs")
            .hatch_preview_models()
            .expect("it has models")
    }

    #[test]
    fn the_preview_of_a_pattern_uses_the_current_colour_or_the_layers() {
        let mut app = app_with_rectangle();
        app.ribbon.active_color = Color::Index(1);
        open_with(&mut app, "HATCH", &[]);
        let models = preview_models(&mut app);
        assert_eq!(models[0].color, [1.0, 0.0, 0.0, 0.75]);

        let mut app = app_with_rectangle();
        open_with(&mut app, "HATCH", &[]); // current colour ByLayer
        let i = app.active_tab;
        let mut layer = app.tabs[i].scene.layer_color(&app.tabs[i].active_layer);
        layer[3] = 0.75;
        assert_eq!(preview_models(&mut app)[0].color, layer);
    }

    #[test]
    fn the_preview_of_a_chosen_colour_is_that_colour() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "HATCH", &[Field::Color(HatchColor::Color(Color::Index(3)))]);
        assert_eq!(preview_models(&mut app)[0].color, [0.0, 1.0, 0.0, 0.75]);
    }

    #[test]
    fn the_gradient_preview_shows_the_tint_of_a_one_colour_gradient() {
        let mut app = app_with_rectangle();
        let red = Color::Index(1);
        open_with(
            &mut app,
            "GRADIENT",
            &[
                Field::GradientOneColor(true),
                Field::GradientColor1(red),
                Field::GradientTint(0.5),
                Field::GradientColor2(Color::Index(3)),
            ],
        );
        let models = preview_models(&mut app);
        let mut expected = rgba_of(tinted_second_color(rgba_of(red).unwrap(), 0.5)).unwrap();
        expected[3] = 0.75;
        match &models[0].pattern {
            crate::scene::model::hatch_model::HatchPattern::Gradient { color2, .. } => {
                assert_eq!(*color2, expected)
            }
            other => panic!("a gradient preview, got {other:?}"),
        }
    }
```
(usare gli import già presenti `Message`, `Color`; aggiungere `OpenCADStudio::new_for_test` come negli altri test. Se `Vector3` non implementa `PartialEq`, confrontare i componenti.)

- [ ] **Step 2:** `cargo test --locked --lib hatch_gradient 2>&1 | tail -20` → errori di compilazione/asserzioni rosse (la finestra non apre sulla scheda Gradient, ecc.).

- [ ] **Step 3: Implementare**

`hatch_gradient.rs` (sopra i test):

```rust
use codec::types::{Color, Transparency};

use crate::app::OpenCADStudio;
use crate::modules::draw::draw::hatch_settings::HatchColor;

impl OpenCADStudio {
    /// The colour and transparency a hatch made by the window gets in tab
    /// `i`: "Use Current" is the drawing's current colour, as for every other
    /// drawing command; a chosen colour is that colour.
    pub(in crate::app) fn hatch_creation_style(
        &self,
        i: usize,
        color: HatchColor,
    ) -> (Color, Transparency) {
        let color = match color {
            HatchColor::UseCurrent => self.ribbon.active_color,
            HatchColor::Color(color) => color,
        };
        (color, self.tabs[i].scene.document.current_entity_transparency())
    }

    /// The RGBA the preview of a pattern or solid is drawn with: the colour
    /// the hatch would get, with ByLayer resolved through the current layer.
    pub(in crate::app) fn hatch_preview_rgba(&self, i: usize, color: HatchColor) -> [f32; 4] {
        let (color, _) = self.hatch_creation_style(i, color);
        let mut rgba = match color {
            Color::ByLayer => self.tabs[i].scene.layer_color(&self.tabs[i].active_layer),
            other => crate::scene::convert::tess_util::aci_to_rgba(&other),
        };
        rgba[3] = 0.75;
        rgba
    }
}
```
`hatch_dialog.rs`:
- `hatch_dialog_open(&mut self, tab: FillTab)`: `let mut settings = self.hatch_last.clone(); settings.tab = tab;` passare a `State::new(.., settings)`; ultima riga: `self.tabs[i].last_cmd = Some(if tab == FillTab::Gradient { "GRADIENT" } else { "HATCH" }.to_string());`.
- `hatch_dialog_ok` (ramo creazione), dopo `let Some(command) = hatch_command_from_state(...)`:
```rust
        let style = (state.settings.tab == FillTab::Hatch)
            .then(|| self.hatch_creation_style(i, state.settings.color));
        let command = command.with_creation_style(style);
```
- `hatch_dialog_preview`, dopo la costruzione del comando:
```rust
        let preview_color = (state.settings.tab == FillTab::Hatch)
            .then(|| self.hatch_preview_rgba(i, state.settings.color));
        let command = command.with_preview_color(preview_color);
```
`draw.rs`:
```rust
            "HATCH" if !self.scripted_dispatch => {
                return Some(self.hatch_dialog_open(FillTab::Hatch));
            }
            "GRADIENT" if !self.scripted_dispatch => {
                return Some(self.hatch_dialog_open(FillTab::Gradient));
            }
```
e il braccio `"GRADIENT" =>` esistente diventa `"GRADIENT" | "-GRADIENT" =>`. `commands/mod.rs`: aggiungere `"-GRADIENT",` accanto a `"-HATCH",`. `hatch.rs:1799`: `names: &["GRADIENT", "-GRADIENT"]`. Aggiungere `use crate::modules::draw::draw::hatch_settings::FillTab;` dove serve.

- [ ] **Step 4:** `cargo test --locked --lib hatch_gradient` e `cargo test --locked --lib hatch` → tutti PASS.

- [ ] **Step 5: Mutazioni e commit**

(M) in `hatch_dialog_ok` passare sempre `None` come stile → `use_current_follows_the_ribbon_colour…` fallisce. (M) far decidere la scheda dal nome (`pattern == "SOLID"`) → `the_gradient_tab_makes_a_gradient_whatever_the_pattern_name` fallisce. Ripristinare.

```bash
git add -A src && git commit -m "HATCH e GRADIENT aprono la finestra sulla loro scheda; colore corrente e anteprima risolti dall'app; -GRADIENT"
```

---

### Task 8: Swatch con colori e sfumato

**Files:**
- Modify: `src/ui/properties.rs` (`HatchPatternPreview`, ~103-278; test nel modulo `tests` del file o in un `#[cfg(test)] mod swatch_color_tests` in coda)

**Interfaces:**
- Consumes: `gradient_profile` (Task 1); `kernel::geom2d::gradient_frame(boundary: &[[f64;2]], angle_rad: f64, shift: f64, Tolerance) -> Option<GradientFrame>` (campi `projection_min`, `projection_span`, `center: [f64;2]`, `radius`); la stessa funzione che usa il renderer (`hatch_model.rs::gradient_frame`).
- Produces:
  - `HatchPatternPreview::with_color(self, iced::Color) -> Self` (pattern: colore delle linee; solido: colore del riempimento; sfumato: colore 1)
  - `pub(crate) fn swatch_linear_stops(kind, invert, c1: [f32;4], c2: [f32;4]) -> Vec<(f32, [f32;4])>`
  - `pub(crate) fn swatch_ring_colors(kind, invert, c1: [f32;4], c2: [f32;4], rings: usize) -> Vec<(f32, [f32;4])>` (dall'esterno al centro, `(frazione di raggio, colore)`)
  - lo sfumato si disegna con `HatchPattern::Gradient{ color2, .. }` + `with_color(colore1)`; senza `with_color` resta il riempimento piatto di oggi.

- [ ] **Step 1: Test rossi**

```rust
#[cfg(test)]
mod swatch_color_tests {
    use super::*;
    use crate::scene::model::hatch_model::GradientKind;

    const C1: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const C2: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

    #[test]
    fn linear_has_two_stops_cylinder_three_and_curved_nine() {
        assert_eq!(swatch_linear_stops(GradientKind::Linear, false, C1, C2).len(), 2);
        assert_eq!(swatch_linear_stops(GradientKind::Cylinder, false, C1, C2).len(), 3);
        assert_eq!(swatch_linear_stops(GradientKind::Curved, false, C1, C2).len(), 9);
    }

    #[test]
    fn the_stops_follow_the_shader_profile() {
        let linear = swatch_linear_stops(GradientKind::Linear, false, C1, C2);
        assert_eq!(linear[0], (0.0, C1));
        assert_eq!(linear[1], (1.0, C2));
        // Cylinder: colour 1 at both edges, colour 2 in the middle.
        let cylinder = swatch_linear_stops(GradientKind::Cylinder, false, C1, C2);
        assert_eq!(cylinder[0].1, C1);
        assert_eq!(cylinder[1], (0.5, C2));
        assert_eq!(cylinder[2].1, C1);
        // Inverted: the other way round.
        let inverted = swatch_linear_stops(GradientKind::Cylinder, true, C1, C2);
        assert_eq!(inverted[1].1, C1);
        assert_eq!(inverted[0].1, C2);
        // Curved eases in: the middle is a quarter of the way, not a half.
        let curved = swatch_linear_stops(GradientKind::Curved, false, C1, C2);
        let middle = curved[4];
        assert_eq!(middle.0, 0.5);
        assert!((middle.1[2] - 0.25).abs() < 1e-6, "{middle:?}");
    }

    #[test]
    fn rings_run_from_the_outside_in_with_colour_two_at_the_centre() {
        let rings = swatch_ring_colors(GradientKind::Spherical, false, C1, C2, 16);
        assert_eq!(rings.len(), 16);
        assert_eq!(rings[0].0, 1.0);
        assert!(rings[15].0 > 0.0 && rings[15].0 < 0.1);
        // Radial stops run outside-in: colour 1 outside, colour 2 inside.
        assert!(rings[0].1[0] > 0.9, "{:?}", rings[0]);
        assert!(rings[15].1[2] > 0.9, "{:?}", rings[15]);
        // Radii strictly decrease.
        assert!(rings.windows(2).all(|w| w[0].0 > w[1].0));
    }

    #[test]
    fn hemispherical_reaches_colour_one_sooner_than_spherical() {
        let sphere = swatch_ring_colors(GradientKind::Spherical, false, C1, C2, 16);
        let hemi = swatch_ring_colors(GradientKind::Hemispherical, false, C1, C2, 16);
        // Same ring, nearer the edge: the sqrt profile has moved further to colour 1.
        assert!(hemi[8].1[0] > sphere[8].1[0]);
    }

    #[test]
    fn the_swatch_keeps_the_colour_it_is_given() {
        let preview = HatchPatternPreview::new(crate::scene::model::hatch_model::HatchPattern::Solid)
            .with_color(iced::Color::from_rgb(1.0, 0.0, 0.0));
        assert_eq!(preview.color, Some(iced::Color::from_rgb(1.0, 0.0, 0.0)));
    }
}
```

- [ ] **Step 2:** `cargo test --locked --lib swatch_color_tests 2>&1 | tail` → rosso (funzioni mancanti).

- [ ] **Step 3: Implementare** in `properties.rs`

```rust
fn mix4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t)
}

/// Stops (offset, RGBA) of a linear swatch gradient, taken from the shader's
/// profile where it bends: the ends for Linear, the middle too for Cylinder,
/// nine samples for Curved.
pub(crate) fn swatch_linear_stops(
    kind: crate::scene::model::hatch_model::GradientKind,
    invert: bool,
    c1: [f32; 4],
    c2: [f32; 4],
) -> Vec<(f32, [f32; 4])> {
    use crate::entities::hatch_fill::gradient_profile;
    use crate::scene::model::hatch_model::GradientKind;
    let offsets: &[f32] = match kind {
        GradientKind::Cylinder => &[0.0, 0.5, 1.0],
        GradientKind::Curved => &[0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0],
        _ => &[0.0, 1.0],
    };
    offsets
        .iter()
        .map(|&t| (t, mix4(c1, c2, gradient_profile(kind, invert, t))))
        .collect()
}

/// Concentric rings of a radial swatch, outermost first: (fraction of the
/// radius, colour). Radial stops run outside-in, as in the shader: colour 2 at
/// the centre.
pub(crate) fn swatch_ring_colors(
    kind: crate::scene::model::hatch_model::GradientKind,
    invert: bool,
    c1: [f32; 4],
    c2: [f32; 4],
    rings: usize,
) -> Vec<(f32, [f32; 4])> {
    use crate::entities::hatch_fill::gradient_profile;
    (0..rings)
        .map(|k| {
            let outer = 1.0 - k as f32 / rings as f32;
            let middle = outer - 0.5 / rings as f32;
            (outer, mix4(c2, c1, gradient_profile(kind, invert, middle)))
        })
        .collect()
}
```
`HatchPatternPreview`: campo `color: Option<iced::Color>` (init `None`), `with_color`. In `draw`:
- `Solid`: `frame.fill(&sample, self.color.unwrap_or(palette.background.base.text.scale_alpha(0.72)))`.
- `Pattern`: `stroke` colore = `self.color.unwrap_or(palette.background.base.text)`; il riempimento tinto di ripiego usa lo stesso colore con alpha 0.35.
- `Gradient{ angle_deg, shift, kind, invert, color2, .. }`: se `self.color` è `None` → riempimento piatto di oggi. Altrimenti, con `c1 = color.into_rgba()` come `[f32;4]` e `c2 = *color2`:
  - rettangolo del campione in coordinate y-in-su `rect = [[pad,pad],[w-pad,pad],[w-pad,h-pad],[pad,h-pad]]` (f64); `frame_g = kernel::geom2d::gradient_frame(&rect, (angle_deg as f64).to_radians(), shift as f64, kernel::geom2d::Tolerance::default())`.
  - kind non radiale (`!kind.radial()`): direzione `d = (cos a, sin a)`; `p0 = d * projection_min`, `p1 = d * (projection_min + projection_span)`; convertire in coordinate canvas `(x, h - y)`; costruire `canvas::gradient::Linear::new(p0, p1)` con `.add_stop(offset, Color::from_rgba(..))` per ogni stop di `swatch_linear_stops`, e riempire `sample` con `canvas::Fill { style: canvas::Style::Gradient(canvas::Gradient::Linear(linear)), ..Default::default() }`. **Verificare l'API esatta** nel checkout di iced pinnato (`~/.cargo/git/checkouts/iced-*/23604ff*/widget/src/canvas` / `graphics/src/gradient.rs`); se `Fill::from(Gradient)` esiste, usarlo.
  - kind radiale: per ogni `(frac, colore)` di `swatch_ring_colors(.., 16)` riempire un cerchio di centro `frame_g.center` (convertito in canvas) e raggio `frac * frame_g.radius`, dentro `frame.with_clip(sample_rect, |f| …)` (se `with_clip` non è disponibile in questa versione, disegnare i cerchi su un `Frame` di dimensione pari al campione e traslarlo).
  - se `gradient_frame` restituisce `None` → riempimento piatto con `c1`.
  Il bordo del campione si disegna dopo, come oggi. **Il limite di 400 segmenti e la tinta per scale minuscole dei pattern restano invariati.**

- [ ] **Step 4:** `cargo test --locked --lib swatch_color_tests properties` → PASS; `cargo check --locked` pulito. (Il rendering a schermo lo verifica l'utente.)

- [ ] **Step 5: Mutazione e commit**

(M) invertire l'ordine dei colori in `swatch_ring_colors` → `rings_run_from_the_outside_in…` fallisce. Ripristinare.

```bash
git add src/ui/properties.rs && git commit -m "Swatch: colore scelto per pattern e solido, sfumato con le stesse curve dello shader"
```

---

### Task 9: Modifica — cosa cambia a OK, e una sola operazione che lo applica

**Files:**
- Modify: `src/entities/hatch_fill.rs` (`FillEdit`, `HatchWindowEdit`, `apply_window_edit`)
- Modify: `src/modules/draw/draw/hatch_edit_settings.rs` (`EditTarget` con `kind`, `changes` per scheda, `apply_result`, `fields_valid`; `HatchEditChanges` diventa alias di `HatchWindowEdit`)
- Modify: `src/command.rs:15` (`HatchEditOperation::Window(Box<HatchWindowEdit>)`)
- Modify: `src/app/command_driver/modify.rs` (ramo `Window` accanto a `Update`, riga ~422)
- Modify (solo compilazione): `src/app/commands/hatch_dialog.rs::hatch_dialog_ok_edit` (usa `HatchWindowEdit::is_empty`)

**Interfaces:**
- Consumes: Task 1-2 (`apply_gradient`, `apply_gradient_patch`, `set_catalog_pattern`, `apply_pattern_update`, `apply_common_update`, `FillKind`, `read_gradient`), Task 5 (`HatchSettings` con `tab`, `color`, `gradient`).
- Produces:
  - `enum FillEdit { Pattern { pattern: Option<String>, scale: Option<f32>, angle_deg: Option<f32> }, ToPattern { name: String, scale: f32, angle_deg: f32 }, Gradient(GradientPatch), ToGradient(GradientSpec) }` (`Clone, Debug, PartialEq`)
  - `struct HatchWindowEdit { fill: Option<FillEdit>, color: Option<AcadColor>, style: Option<HatchStyleType>, disassociate: bool, origin: Option<[f64; 2]> }` (`Clone, Debug, Default, PartialEq`) con `fn is_empty(&self) -> bool`
  - `fn apply_window_edit(h: &mut Hatch, edit: &HatchWindowEdit)`
  - `pub type HatchEditChanges = HatchWindowEdit;` in `hatch_edit_settings`
  - `EditTarget { handle, kind: FillKind, initial: HatchSettings, pattern: String, scale: f32, angle_deg: f32 }`; `EditTarget::from_hatch(Handle, &Hatch)`; `changes(&self, &HatchSettings, Option<[f64;2]>) -> Option<HatchWindowEdit>`; `apply_result(&self, &HatchWindowEdit) -> CmdResult`
  - `pub fn gradient_settings_from(&GradientSpec) -> GradientSettings` (in `hatch_edit_settings`)
  - `HatchEditOperation::Window(Box<HatchWindowEdit>)`

- [ ] **Step 1: Test rossi di `apply_window_edit`** (modulo `tests` di `hatch_fill.rs`)

```rust
    use codec::entities::HatchStyleType;

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
        assert_eq!(after.pattern_scale, before.pattern_scale);
    }

    #[test]
    fn an_empty_edit_changes_nothing() {
        assert!(HatchWindowEdit::default().is_empty());
        let before = bounded_hatch();
        let mut after = before.clone();
        apply_window_edit(&mut after, &HatchWindowEdit::default());
        assert_eq!(after, before);
    }
```

- [ ] **Step 2:** `cargo test --locked --lib hatch_fill 2>&1 | tail` → errori di compilazione (tipi mancanti).

- [ ] **Step 3: Implementare in `hatch_fill.rs`**

```rust
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
    pub disassociate: bool,
    /// New pattern origin in the hatch's plane (Hatch tab only).
    pub origin: Option<[f64; 2]>,
}

impl HatchWindowEdit {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

pub fn apply_window_edit(h: &mut Hatch, edit: &HatchWindowEdit) {
    let origin = edit.origin.map(|point| (point[0], point[1]));
    match &edit.fill {
        Some(FillEdit::Pattern { pattern, scale, angle_deg }) => {
            let name = pattern.clone().unwrap_or_else(|| h.pattern.name.clone());
            let scale = scale.unwrap_or(h.pattern_scale as f32);
            let angle = angle_deg.unwrap_or(h.pattern_angle.to_degrees() as f32);
            apply_pattern_update(h, &name, scale, angle, origin, edit.disassociate, edit.style);
        }
        Some(FillEdit::ToPattern { name, scale, angle_deg }) => {
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
```
(`use` aggiuntivi: `GradientKind` già importato; `Vector2` è nel modulo test.)

- [ ] **Step 4:** `cargo test --locked --lib hatch_fill` → PASS.

- [ ] **Step 5: Test rossi di `EditTarget::changes`** — in `hatch_edit_settings.rs`. Riscrivere i test esistenti per la nuova forma (stessa semantica): ogni `HatchEditChanges { scale: Some(4.0), ..Default }` diventa
`HatchEditChanges { fill: Some(FillEdit::Pattern { pattern: None, scale: Some(4.0), angle_deg: None }), ..Default::default() }`; `pattern` e `angle_deg` analoghi; `style`, `disassociate`, `origin` restano campi di primo livello; l'helper `update_of` ora estrae il `Box<HatchWindowEdit>` da `HatchEditOperation::Window`. I test `the_update_hands_back_the_hatch_values_for_the_untouched_fields` e `the_update_carries_the_changed_fields` verificano ora che `apply_result` restituisca `operation: Window(edit)` con `edit == changes`, e che `name/scale/angle` di contorno siano quelli del retino. Poi **aggiungere**:

```rust
    use crate::entities::hatch_fill::{apply_gradient, FillEdit, GradientPatch, GradientSpec, HatchWindowEdit};
    use crate::modules::draw::draw::hatch_settings::{FillTab, GradientSettings, HatchColor};
    use crate::scene::model::hatch_model::GradientKind;
    use codec::types::Color;

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

    fn gradient_target() -> EditTarget {
        let mut hatch = Hatch::solid();
        apply_gradient(&mut hatch, &gradient_spec());
        EditTarget::from_hatch(Handle::new(7), &hatch)
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
    fn the_hatch_tab_of_a_gradient_starts_from_the_defaults_not_from_the_gradient() {
        let s = gradient_target().initial;
        let defaults = HatchSettings::default();
        assert_eq!((s.pattern.as_str(), s.angle.as_str(), s.scale.as_str()), (defaults.pattern.as_str(), "0", "1"));
    }

    #[test]
    fn nothing_changed_on_a_gradient_is_empty() {
        let target = gradient_target();
        assert!(edit(&target, |_| {}).unwrap().is_empty());
        // The same angle written differently is not a change.
        assert!(edit(&target, |s| s.gradient.angle = "28,64789".into()).unwrap().is_empty());
    }

    #[test]
    fn one_gradient_field_is_one_patch_field() {
        let target = gradient_target();
        let patch = |change: fn(&mut GradientSettings)| match edit(&target, |s| change(&mut s.gradient)).unwrap().fill {
            Some(FillEdit::Gradient(patch)) => patch,
            other => panic!("expected a gradient patch, got {other:?}"),
        };
        assert_eq!(
            patch(|g| g.shape = 5),
            GradientPatch { kind: Some(GradientKind::CHOICES[5]), ..Default::default() }
        );
        assert_eq!(
            patch(|g| g.color1 = Color::Index(2)),
            GradientPatch { color1: Some(Color::Index(2)), ..Default::default() }
        );
        assert_eq!(
            patch(|g| g.color2 = Color::Index(3)),
            GradientPatch { color2: Some(Color::Index(3)), ..Default::default() }
        );
        assert_eq!(
            patch(|g| g.centered = false),
            GradientPatch { centered: Some(false), ..Default::default() }
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
        let mut hatch = Hatch::solid();
        apply_gradient(&mut hatch, &GradientSpec { one_color: true, tint: 0.5, ..gradient_spec() });
        let one = EditTarget::from_hatch(Handle::new(2), &hatch);
        assert!(edit(&one, |s| s.gradient.color2 = Color::Index(2)).unwrap().is_empty());
        assert!(edit(&one, |s| s.gradient.tint = 0.9).unwrap().fill.is_some(), "the tint is visible there");
    }

    #[test]
    fn the_tab_decides_the_conversion() {
        // Pattern -> Gradient tab: a whole gradient, even with nothing touched.
        let pattern = target();
        let to_gradient = edit(&pattern, |s| s.tab = FillTab::Gradient).unwrap();
        assert!(matches!(to_gradient.fill, Some(FillEdit::ToGradient(_))));
        // And back on the Hatch tab: nothing to do.
        assert!(edit(&pattern, |s| s.tab = FillTab::Hatch).unwrap().is_empty());
        // Gradient -> Hatch tab: the default pattern; SOLID if chosen.
        let gradient = gradient_target();
        let to_pattern = edit(&gradient, |s| s.tab = FillTab::Hatch).unwrap();
        assert_eq!(
            to_pattern.fill,
            Some(FillEdit::ToPattern { name: "ANSI31".into(), scale: 1.0, angle_deg: 0.0 })
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
        assert_eq!(solid.initial.pattern, "SOLID");
        let to_ansi = edit(&solid, |s| s.pattern = "ANSI31".into()).unwrap();
        assert!(matches!(to_ansi.fill, Some(FillEdit::Pattern { pattern: Some(ref p), .. }) if p == "ANSI31"));
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
    }

    #[test]
    fn validity_follows_the_tab() {
        let target = target();
        let mut s = target.initial.clone();
        s.scale = "0".into();
        assert!(!target.fields_valid(&s));
        s.tab = FillTab::Gradient;
        assert!(target.fields_valid(&s), "a bad pattern scale does not block the Gradient tab");
        s.gradient.angle = "x".into();
        assert!(!target.fields_valid(&s));
        assert!(target.changes(&s, None).is_none());
    }

    #[test]
    fn apply_result_carries_the_whole_edit_in_one_operation() {
        let target = gradient_target();
        let changes = edit(&target, |s| s.gradient.shape = 5).unwrap();
        match target.apply_result(&changes) {
            CmdResult::HatcheditApply { handle, operation: HatchEditOperation::Window(window), .. } => {
                assert_eq!(handle, Handle::new(7));
                assert_eq!(*window, changes);
            }
            _ => panic!("expected the window operation"),
        }
    }
```
(L'helper `target()` esistente è un pattern ANSI31; nei test del Task 9 `Hatch::solid()` richiede `pattern_scale`: i default bastano.) Il formato dell'angolo "28.64789" è `format_number(0.5.to_degrees())` a sette cifre: se l'output reale differisce solo nell'ultima cifra, adeguare la stringa attesa al valore di `format_number`.

- [ ] **Step 6:** `cargo test --locked --lib hatch_edit 2>&1 | tail` → rosso.

- [ ] **Step 7: Implementare `hatch_edit_settings.rs`**

```rust
use super::hatch_settings::{
    parse_angle_deg, parse_scale, FillTab, GradientSettings, HatchColor, HatchSettings, OriginMode,
};
use crate::command::{CmdResult, HatchEditOperation};
use crate::entities::hatch_fill::{
    read_gradient, FillEdit, FillKind, GradientPatch, GradientSpec, HatchWindowEdit,
};
use crate::scene::model::hatch_model::GradientKind;

pub type HatchEditChanges = HatchWindowEdit;

/// The gradient tab's fields for a gradient read from a hatch. A two-colour
/// gradient shows tint 1.0: that is what switching it to One color writes.
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
        tint: if spec.one_color { spec.tint as f32 } else { 1.0 },
        centered: spec.centered,
        angle: format_number(spec.angle_rad.to_degrees()),
    }
}
```
`EditTarget`: aggiungere `pub kind: FillKind`; `from_hatch`:

```rust
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
            _ => (
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
            pattern,
            angle,
            scale,
            associative: hatch.is_associative,
            separate: false,
            retain: false,
            island_detection: hatch.style != HatchStyleType::Ignore,
            island_style: hatch.style,
            origin_mode: OriginMode::Current,
            tab,
            color: HatchColor::Color(hatch.common.color),
            gradient,
        };
        Self {
            handle,
            kind,
            initial,
            pattern: hatch.pattern.name.clone(),
            scale: hatch.pattern_scale as f32,
            angle_deg: hatch.pattern_angle.to_degrees() as f32,
        }
    }
```
`fields_valid`:
```rust
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
```
`changes`:
```rust
    pub fn changes(&self, settings: &HatchSettings, specified_origin: Option<[f64; 2]>) -> Option<HatchWindowEdit> {
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
                    out.fill = Some(FillEdit::Pattern { pattern, scale, angle_deg });
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
```
funzioni libere nello stesso file:
```rust
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
```
`apply_result`:
```rust
    pub fn apply_result(&self, changes: &HatchEditChanges) -> CmdResult {
        CmdResult::HatcheditApply {
            handle: self.handle,
            name: self.pattern.clone(),
            scale: self.scale,
            angle: self.angle_deg,
            operation: HatchEditOperation::Window(Box::new(changes.clone())),
        }
    }
```
(`is_pattern_hatch` e il suo test restano fino al Task 10.) `HatchEditChanges` non è più una struct propria: togliere la vecchia definizione e `impl is_empty`.

- [ ] **Step 8: `command.rs` e `modify.rs`**

`command.rs` (in `enum HatchEditOperation`): `Window(Box<crate::entities::hatch_fill::HatchWindowEdit>),`. In `modify.rs`, nel `match operation` dopo `self.push_undo_snapshot(i, "HATCHEDIT");` (accanto al ramo `Update`):

```rust
                HatchEditOperation::Window(edit) => {
                    if let Some(codec::EntityType::Hatch(hatch)) =
                        self.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        crate::entities::hatch_fill::apply_window_edit(hatch, &edit);
                    }
                    // The cached fill model is rebuilt from the entity: a change of
                    // kind (pattern/solid/gradient) is not a patch of the old one.
                    self.tabs[i].scene.refresh_fill_model(handle);
                    self.tabs[i]
                        .scene
                        .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                    self.refresh_properties();
                }
```
Eventuali altri `match` esaustivi su `HatchEditOperation` (es. `hatchedit.rs`) si correggono col compilatore.

`hatch_dialog_ok_edit`: nessuna logica nuova; `.filter(|changes| !changes.is_empty())` compila già.

- [ ] **Step 9:** `cargo test --locked --lib hatch 2>&1 | tail -20` → PASS (compresi i test di modifica da app, ora attraverso `Window`). Se uno fallisce, confrontare lo stato dell'entità: la differenza ammessa rispetto a prima è solo `gradient_color` azzerato.

- [ ] **Step 10: Mutazioni e commit**

(M) in `gradient_patch` confrontare `color2` anche con `one_color` → `hidden_gradient_fields_never_enable_ok` fallisce. (M) in `changes` ignorare `settings.tab` → `the_tab_decides_the_conversion` fallisce. Ripristinare.

```bash
git add -A src && git commit -m "Hatch Edit: cosa cambia a OK per i tre tipi e una sola operazione (Window) che lo applica"
```

---

### Task 10: Modifica — la finestra si apre su qualunque retino

**Files:**
- Modify: `src/app/commands/hatch_dialog.rs` (`hatch_dialog_open_edit`, `hatchedit_window_target`, `hatch_double_click_on_grip`, `hot_pattern_hatch_grip` → `hot_hatch_grip`, commenti; test)
- Modify: `src/modules/draw/draw/hatch_edit_settings.rs` (rimuovere `is_pattern_hatch` e il suo test)

**Interfaces:**
- Consumes: Task 9.
- Produces: `hatch_dialog_open_edit(handle)` apre su ogni `EntityType::Hatch`; `hatchedit_window_target(i)` è l'unico retino selezionato di qualunque tipo; i due percorsi del doppio clic sul grip valgono per ogni retino.

- [ ] **Step 1: Invertire i test che dicono "non apre"** (`app/commands/hatch_dialog.rs`, riferimenti: ~2983, ~3116, ~3237, ~3248). Sostituirli con:

```rust
    /// A gradient made through the window (tab Gradient) and stored.
    fn gradient_made(app: &mut OpenCADStudio, fields: &[Field]) -> Handle {
        let mut all = vec![Field::Tab(crate::modules::draw::draw::hatch_settings::FillTab::Gradient)];
        all.extend_from_slice(fields);
        hatch_made_with(app, &all)
    }

    #[test]
    fn a_solid_hatch_opens_the_window_on_the_hatch_tab() {
        let mut app = app_with_rectangle();
        let solid = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);
        assert!(stored(&app, solid).is_solid);
        let _ = app.hatch_dialog_open_edit(solid);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(edit_handle(&app), Some(solid));
        let settings = &app.hatch_dialog.as_ref().unwrap().settings;
        assert_eq!(settings.tab, crate::modules::draw::draw::hatch_settings::FillTab::Hatch);
        assert_eq!(settings.pattern, "SOLID");
        assert_eq!(app.hatch_dialog_title(), "Hatch Edit");
    }

    #[test]
    fn a_gradient_hatch_opens_the_window_on_the_gradient_tab_with_its_values() {
        use crate::modules::draw::draw::hatch_settings::FillTab;
        let mut app = app_with_rectangle();
        let gradient = gradient_made(
            &mut app,
            &[
                Field::GradientShape(3),
                Field::GradientColor1(codec::types::Color::Index(1)),
                Field::GradientAngle("30".into()),
            ],
        );
        let _ = app.hatch_dialog_open_edit(gradient);
        let settings = &app.hatch_dialog.as_ref().unwrap().settings;
        assert_eq!(settings.tab, FillTab::Gradient);
        assert_eq!(settings.gradient.shape, 3);
        assert_eq!(settings.gradient.color1, codec::types::Color::Index(1));
        assert_eq!(settings.gradient.angle, "30");
    }

    #[test]
    fn hatchedit_on_a_solid_or_gradient_opens_the_window_but_dash_hatchedit_does_not() {
        let mut app = app_with_rectangle();
        let solid = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);
        select_only(&mut app, solid);
        let _ = app.dispatch_command("HATCHEDIT");
        assert_eq!(edit_handle(&app), Some(solid));
        let _ = app.update(Message::CloseModal);
        let _ = app.dispatch_command("-HATCHEDIT");
        assert!(app.hatch_dialog.is_none());
        assert_eq!(command_name(&app, app.active_tab), Some("HATCHEDIT"));

        let mut app = app_with_rectangle();
        let gradient = gradient_made(&mut app, &[]);
        select_only(&mut app, gradient);
        let _ = app.dispatch_command("HATCHEDIT");
        assert_eq!(edit_handle(&app), Some(gradient));
    }

    #[test]
    fn double_clicking_a_solid_or_gradient_hatch_opens_the_window() {
        let mut app = app_with_rectangle();
        let solid = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);
        frame(&mut app);
        double_click(&mut app, 4.0, 3.0);
        assert_eq!(edit_handle(&app), Some(solid));

        let mut app = app_with_rectangle();
        let gradient = gradient_made(&mut app, &[]);
        frame(&mut app);
        double_click(&mut app, 4.0, 3.0);
        assert_eq!(edit_handle(&app), Some(gradient));
    }

    #[test]
    fn a_gradient_with_no_stops_from_another_program_opens_too() {
        let (mut app, hatch) = app_with_hatch();
        set_stored(&mut app, hatch, |h| h.gradient_color.enabled = true);
        let _ = app.hatch_dialog_open_edit(hatch);
        assert_eq!(edit_handle(&app), Some(hatch));
        assert_eq!(
            app.hatch_dialog.as_ref().unwrap().settings.gradient.color1,
            crate::entities::hatch_fill::DEFAULT_GRADIENT_COLOR1
        );
    }
```
Per i due test sul **grip** (associativo, senza linee di pattern): copiare `double_clicking_the_centre_of_a_hatch_opens_the_window_not_a_grip_edit` (~3328) e `double_clicking_the_centre_of_an_already_selected_hatch_opens_the_window` (~3370), cambiando **solo** la prima riga di setup in `let mut app = app_with_rectangle(); let hatch = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);` (e una variante con `gradient_made`); rinominare in `…_solid_…` / `…_gradient_…`. Verificare che il retino sia associativo (`stored(&app, hatch).is_associative`).

Aggiungere i test di **modifica OK**:

```rust
    /// OK on `field` alone must change exactly what `expected` says and leave
    /// everything else of the hatch as it was; one undo restores it.
    fn edit_one(
        app: &mut OpenCADStudio,
        hatch: Handle,
        fields: &[Field],
        expected: impl FnOnce(&mut codec::entities::Hatch),
    ) {
        let before = stored(app, hatch);
        let depth = undo_depth(app);
        let _ = app.hatch_dialog_open_edit(hatch);
        for f in fields {
            field(app, f.clone());
        }
        let _ = app.update(Message::HatchDialogOk);
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none(), "closed");
        let mut want = before.clone();
        expected(&mut want);
        assert_eq!(stored(app, hatch), want);
        assert_eq!(undo_depth(app), depth + 1, "one undo step");
        let _ = app.update(Message::Undo);
        assert_eq!(stored(app, hatch), before, "one undo restores it all");
    }

    #[test]
    fn each_gradient_field_changes_only_itself() {
        use codec::types::Color;
        let mut app = app_with_rectangle();
        let hatch = gradient_made(&mut app, &[]);
        edit_one(&mut app, hatch, &[Field::GradientShape(5)], |h| {
            h.gradient_color.name = "CURVED".into()
        });
        edit_one(&mut app, hatch, &[Field::GradientColor1(Color::Index(2))], |h| {
            h.gradient_color.colors[0].color = Color::Index(2)
        });
        edit_one(&mut app, hatch, &[Field::GradientColor2(Color::Index(4))], |h| {
            h.gradient_color.colors[1].color = Color::Index(4)
        });
        edit_one(&mut app, hatch, &[Field::GradientCentered(false)], |h| {
            h.gradient_color.shift = 1.0
        });
        edit_one(&mut app, hatch, &[Field::GradientAngle("30".into())], |h| {
            h.gradient_color.angle = 30f64.to_radians();
            h.pattern_angle = 30f64.to_radians();
        });
        edit_one(&mut app, hatch, &[Field::GradientOneColor(true)], |h| {
            h.gradient_color.is_single_color = true;
            h.gradient_color.color_tint = 1.0;
        });
    }

    #[test]
    fn a_hidden_gradient_field_does_not_enable_ok() {
        let mut app = app_with_rectangle();
        let hatch = gradient_made(&mut app, &[]); // two colours: the tint is hidden
        let before = stored(&app, hatch);
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::GradientTint(0.1));
        assert!(!app.hatch_dialog.as_ref().unwrap().can_ok());
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch), "nothing to apply: still open");
        assert_eq!(stored(&app, hatch), before);
    }

    #[test]
    fn a_solid_edits_like_a_hatch_and_keeps_its_colour() {
        let mut app = app_with_rectangle();
        let solid = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);
        edit_one(&mut app, solid, &[Field::IslandStyle(HatchStyleType::Outer)], |h| {
            h.style = HatchStyleType::Outer
        });
        edit_one(
            &mut app,
            solid,
            &[Field::Color(crate::modules::draw::draw::hatch_settings::HatchColor::Color(
                codec::types::Color::Index(1),
            ))],
            |h| h.common.color = codec::types::Color::Index(1),
        );
    }

    /// Every way from one kind of fill to another is one OK and one undo, and
    /// leaves boundaries, links and island style alone (they are compared by
    /// `edit_one`'s full-entity equality, so only the fill may differ).
    #[test]
    fn all_six_conversions_are_one_undo_each() {
        use crate::entities::hatch_fill::{FillKind, read_gradient};
        use crate::modules::draw::draw::hatch_settings::FillTab;
        let kind = |app: &OpenCADStudio, h: Handle| FillKind::of(&stored(app, h));

        // pattern -> solid, pattern -> gradient
        let (mut app, hatch) = app_with_hatch();
        let before = stored(&app, hatch);
        let depth = undo_depth(&app);
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Pattern("SOLID".into()));
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(kind(&app, hatch), FillKind::Solid);
        assert_eq!(stored(&app, hatch).paths, before.paths);
        assert_eq!(undo_depth(&app), depth + 1);
        let _ = app.update(Message::Undo);
        assert_eq!(stored(&app, hatch), before);

        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Tab(FillTab::Gradient));
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(kind(&app, hatch), FillKind::Gradient);
        assert_eq!(stored(&app, hatch).paths, before.paths);
        assert_eq!(stored(&app, hatch).is_associative, before.is_associative);
        assert_eq!(stored(&app, hatch).style, before.style);
        let model = &app.tabs[app.active_tab].scene.hatches[&hatch];
        assert!(
            matches!(model.pattern, crate::scene::model::hatch_model::HatchPattern::Gradient { .. }),
            "the scene's model follows the kind"
        );
        let _ = app.update(Message::Undo);
        assert_eq!(stored(&app, hatch), before);

        // solid -> pattern, solid -> gradient
        let mut app = app_with_rectangle();
        let solid = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);
        let _ = app.hatch_dialog_open_edit(solid);
        field(&mut app, Field::Pattern("ANSI31".into()));
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(kind(&app, solid), FillKind::Pattern);
        let _ = app.update(Message::Undo);
        assert_eq!(kind(&app, solid), FillKind::Solid);
        let _ = app.hatch_dialog_open_edit(solid);
        field(&mut app, Field::Tab(FillTab::Gradient));
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(kind(&app, solid), FillKind::Gradient);

        // gradient -> pattern, gradient -> solid (same stored name "SOLID")
        let mut app = app_with_rectangle();
        let gradient = gradient_made(&mut app, &[Field::GradientShape(4)]);
        let before = stored(&app, gradient);
        let _ = app.hatch_dialog_open_edit(gradient);
        field(&mut app, Field::Tab(FillTab::Hatch));
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(kind(&app, gradient), FillKind::Pattern);
        assert!(!stored(&app, gradient).gradient_color.enabled);
        let _ = app.update(Message::Undo);
        assert_eq!(stored(&app, gradient), before);
        assert_eq!(read_gradient(&stored(&app, gradient)).kind, GradientKind::CHOICES[4].0);
        let _ = app.hatch_dialog_open_edit(gradient);
        field(&mut app, Field::Tab(FillTab::Hatch));
        field(&mut app, Field::Pattern("SOLID".into()));
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(kind(&app, gradient), FillKind::Solid);
    }

    #[test]
    fn converting_an_associative_hatch_with_islands_leaves_its_structure_alone() {
        // Review focus: several paths, links to source objects, island style.
        let mut app = app_with_rectangle();
        let hatch = hatch_made_with(&mut app, &[Field::IslandStyle(HatchStyleType::Outer)]);
        let before = stored(&app, hatch);
        assert!(before.is_associative);
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Tab(crate::modules::draw::draw::hatch_settings::FillTab::Gradient));
        let _ = app.update(Message::HatchDialogOk);
        let after = stored(&app, hatch);
        assert_eq!(after.paths, before.paths);
        assert_eq!(after.is_associative, before.is_associative);
        assert_eq!(after.style, before.style);
        assert_eq!(after.common, before.common);
    }
```
(`GradientKind` ha bisogno di `use crate::scene::model::hatch_model::GradientKind;` nel modulo test.)

- [ ] **Step 2:** `cargo test --locked --lib hatch 2>&1 | tail -20` → i test nuovi/invertiti sono rossi (il codice filtra ancora per `is_pattern_hatch`).

- [ ] **Step 3: Implementare**

`hatch_dialog_open_edit`: 
```rust
        let opened = match self.tabs[i].scene.document.get_entity(handle) {
            Some(codec::EntityType::Hatch(hatch)) => {
                (EditTarget::from_hatch(handle, hatch), hatch_plane(hatch))
            }
            _ => return Task::none(),
        };
        let (target, plane) = opened;
```
(togliere `use … is_pattern_hatch`, il ramo del messaggio "solid and gradient hatches are edited from…" e la stringa.) `hatchedit_window_target`: `Some(codec::EntityType::Hatch(_)) => Some(*handle)`. `hatch_double_click_on_grip`: sostituire `pattern_hatch` con `matches!(…, Some(codec::EntityType::Hatch(_)))`. `hot_pattern_hatch_grip` → rinominare `hot_hatch_grip` (2 usi) con lo stesso test. Aggiornare i doc comment ("pattern hatch" → "hatch"). In `hatch_edit_settings.rs` cancellare `is_pattern_hatch` e il test `only_pattern_fills_are_edited_in_the_window`; spostare/aggiungere i casi di `FillKind::of` (già coperti nel Task 1).

- [ ] **Step 4:** `cargo test --locked --lib hatch` → PASS. Poi `cargo test --locked --lib 2>&1 | tail -5` (suite completa): nessun test non-hatch deve essere rosso; il totale sale rispetto a 2407.

- [ ] **Step 5: Mutazione e commit**

(M) in `apply_window_edit` eseguire uno `push_undo_snapshot` doppio (due snapshot) → `all_six_conversions_are_one_undo_each` fallisce. (M) far saltare `refresh_fill_model` nel ramo `Window` → l'asserzione sul modello della scena dopo `pattern -> gradient` fallisce. Ripristinare.

```bash
git add -A src && git commit -m "Hatch Edit: doppio clic, grip e HATCHEDIT aprono la finestra su pattern, solidi e sfumati; test invertiti"
```

---

### Task 11: "Select Color", elenco colori e tastiera

**Files:**
- Modify: `src/app/mod.rs:1601` (`ColorPickTarget::Hatch(crate::ui::window::hatch_dialog::HatchColorSlot)`)
- Modify: `src/app/update/style.rs:1148-1170` (un ramo)
- Modify: `src/ui/window/hatch_dialog.rs` (`pub fn color_pick_message`)
- Modify: `src/app/commands/hatch_dialog.rs` (`hatch_dialog_field` → `Task`; `hatch_dialog_cancel` chiude la finestra colore)
- Modify: `src/app/commands/hatch_gradient.rs` (`hatch_dialog_select_color`, `hatch_dialog_escape_overlay`, `hatch_dialog_enter`)
- Modify: `src/app/update/mod.rs` (~4844 arm `HatchDialogField`; Esc ~414; Invio ~441)

**Interfaces:**
- Consumes: Task 5 (`HatchColorSlot`, `Field::{ColorList, SelectColor, …}`).
- Produces:
  - `ColorPickTarget::Hatch(HatchColorSlot)`
  - `ui::window::hatch_dialog::color_pick_message(slot: HatchColorSlot, color: AcadColor) -> Option<Message>` (`None` per ByLayer/ByBlock/None su Gradient1/2)
  - `OpenCADStudio::hatch_dialog_field(&mut self, Field) -> Task<Message>`
  - `hatch_dialog_select_color(&mut self, HatchColorSlot) -> Task<Message>`, `hatch_dialog_escape_overlay(&mut self) -> Option<Task<Message>>`, `hatch_dialog_enter(&mut self) -> Task<Message>` (tutte `pub(in crate::app)`, `#[inline(never)]`)

- [ ] **Step 1: Test rossi** (modulo `tests` di `hatch_gradient.rs`; riusa helper del Task 7)

```rust
    use crate::app::ColorPickTarget;
    use crate::ui::window::hatch_dialog::HatchColorSlot;

    fn target_slot(app: &OpenCADStudio) -> Option<HatchColorSlot> {
        match &app.color_pick_target {
            Some((ColorPickTarget::Hatch(slot), _)) => Some(*slot),
            _ => None,
        }
    }

    fn open_picker(app: &mut OpenCADStudio, slot: HatchColorSlot) {
        let _ = app.update(Message::HatchDialogField(Field::SelectColor(slot)));
        assert_eq!(target_slot(app), Some(slot), "Select Color is open on the slot");
    }

    fn window_open(app: &OpenCADStudio) -> bool {
        app.active_modal == Some(ModalKind::Hatch) && app.hatch_dialog.is_some()
    }

    #[test]
    fn select_color_opens_the_colour_window_with_the_slots_colour_and_closes_the_list() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "GRADIENT", &[Field::GradientColor2(Color::Index(4))]);
        let _ = app.update(Message::HatchDialogField(Field::ColorList(Some(HatchColorSlot::Gradient2))));
        open_picker(&mut app, HatchColorSlot::Gradient2);
        assert_eq!(app.color_pick_target.as_ref().unwrap().1, Color::Index(4));
        assert_eq!(app.hatch_dialog.as_ref().unwrap().color_list, None);
    }

    #[test]
    fn use_current_shows_the_ribbon_colour_in_the_colour_window() {
        let mut app = app_with_rectangle();
        app.ribbon.active_color = Color::Index(6);
        open_with(&mut app, "HATCH", &[]);
        open_picker(&mut app, HatchColorSlot::Fill);
        assert_eq!(app.color_pick_target.as_ref().unwrap().1, Color::Index(6));
    }

    #[test]
    fn a_pick_reaches_the_slot_it_was_opened_for() {
        for (slot, read) in [
            (HatchColorSlot::Fill, (|a: &OpenCADStudio| a.hatch_dialog.as_ref().unwrap().settings.color == HatchColor::Color(Color::Index(3))) as fn(&OpenCADStudio) -> bool),
            (HatchColorSlot::Gradient1, |a| a.hatch_dialog.as_ref().unwrap().settings.gradient.color1 == Color::Index(3)),
            (HatchColorSlot::Gradient2, |a| a.hatch_dialog.as_ref().unwrap().settings.gradient.color2 == Color::Index(3)),
        ] {
            let mut app = app_with_rectangle();
            open_with(&mut app, "HATCH", &[]);
            open_picker(&mut app, slot);
            let _ = app.update(Message::ColorWindowPick(Color::Index(3)));
            assert!(read(&app), "{slot:?}");
            assert!(app.color_pick_target.is_none(), "the colour window closed");
            assert!(window_open(&app), "the Hatch window is still there");
        }
    }

    #[test]
    fn a_gradient_colour_refuses_the_logical_colours() {
        for logical in [Color::ByLayer, Color::ByBlock, Color::None] {
            let mut app = app_with_rectangle();
            open_with(&mut app, "GRADIENT", &[]);
            let before = app.hatch_dialog.as_ref().unwrap().settings.gradient.color1;
            open_picker(&mut app, HatchColorSlot::Gradient1);
            let _ = app.update(Message::ColorWindowPick(logical));
            assert_eq!(app.hatch_dialog.as_ref().unwrap().settings.gradient.color1, before, "{logical:?}");
        }
        // The fill colour does take ByLayer.
        let mut app = app_with_rectangle();
        open_with(&mut app, "HATCH", &[]);
        open_picker(&mut app, HatchColorSlot::Fill);
        let _ = app.update(Message::ColorWindowPick(Color::ByLayer));
        assert_eq!(app.hatch_dialog.as_ref().unwrap().settings.color, HatchColor::Color(Color::ByLayer));
    }

    #[test]
    fn a_pick_after_the_window_is_gone_does_nothing() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "HATCH", &[]);
        open_picker(&mut app, HatchColorSlot::Fill);
        app.hatch_dialog = None;
        let _ = app.update(Message::ColorWindowPick(Color::Index(3)));
        assert!(app.hatch_dialog.is_none());
    }

    // ── keys ───────────────────────────────────────────────────────────────

    #[test]
    fn escape_with_select_color_open_closes_only_the_colour_window() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "GRADIENT", &[Field::GradientShape(4)]);
        open_picker(&mut app, HatchColorSlot::Gradient1);
        let before = app.hatch_dialog.as_ref().unwrap().settings.clone();
        let _ = app.update(Message::CommandEscape);
        assert!(app.color_pick_target.is_none());
        assert!(window_open(&app), "the Hatch window and its state stay");
        assert_eq!(app.hatch_dialog.as_ref().unwrap().settings, before);
        // The next Esc closes the window as it always did.
        let _ = app.update(Message::CommandEscape);
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
    }

    #[test]
    fn the_keyboard_escape_takes_the_same_path() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "HATCH", &[]);
        open_picker(&mut app, HatchColorSlot::Fill);
        let _ = app.update(Message::ShortcutPressed("ESCAPE".into()));
        assert!(app.color_pick_target.is_none() && window_open(&app));
    }

    #[test]
    fn escape_with_the_colour_list_open_closes_only_the_list() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "HATCH", &[]);
        let _ = app.update(Message::HatchDialogField(Field::ColorList(Some(HatchColorSlot::Fill))));
        let _ = app.update(Message::CommandEscape);
        assert!(window_open(&app));
        assert_eq!(app.hatch_dialog.as_ref().unwrap().color_list, None);
    }

    #[test]
    fn enter_with_select_color_open_confirms_the_colour_and_makes_no_hatch() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "HATCH", &[]); // a region is collected: OK would make a hatch
        open_picker(&mut app, HatchColorSlot::Fill);
        let _ = app.update(Message::ColorPickerColorChanged(Color::Index(3)));
        let _ = app.update(Message::CommandFinalize);
        assert!(app.color_pick_target.is_none());
        assert_eq!(
            app.hatch_dialog.as_ref().unwrap().settings.color,
            HatchColor::Color(Color::Index(3))
        );
        assert!(window_open(&app));
        assert_eq!(all_hatches(&app).len(), 0, "Enter acted on the colour window only");
        // And the shortcut form of Enter does the same.
        open_picker(&mut app, HatchColorSlot::Fill);
        let _ = app.update(Message::ColorPickerColorChanged(Color::Index(5)));
        let _ = app.update(Message::ShortcutPressed("ENTER".into()));
        assert_eq!(
            app.hatch_dialog.as_ref().unwrap().settings.color,
            HatchColor::Color(Color::Index(5))
        );
        assert_eq!(all_hatches(&app).len(), 0);
    }

    #[test]
    fn enter_on_the_true_colour_page_does_nothing() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "HATCH", &[]);
        open_picker(&mut app, HatchColorSlot::Fill);
        app.color_picker_tab = crate::app::ColorPickerTab::TrueColor;
        let _ = app.update(Message::CommandFinalize);
        assert_eq!(target_slot(&app), Some(HatchColorSlot::Fill), "still open");
        assert_eq!(all_hatches(&app).len(), 0);
        assert!(window_open(&app));
    }

    #[test]
    fn enter_with_the_colour_list_open_only_closes_it() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "HATCH", &[]);
        let _ = app.update(Message::HatchDialogField(Field::ColorList(Some(HatchColorSlot::Fill))));
        let _ = app.update(Message::CommandFinalize);
        assert_eq!(app.hatch_dialog.as_ref().unwrap().color_list, None);
        assert_eq!(all_hatches(&app).len(), 0);
        // With nothing open Enter is OK again.
        let _ = app.update(Message::CommandFinalize);
        assert_eq!(all_hatches(&app).len(), 1);
    }

    // ── life cycle ─────────────────────────────────────────────────────────

    #[test]
    fn cancel_closes_the_colour_window_with_the_dialog() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "HATCH", &[]);
        open_picker(&mut app, HatchColorSlot::Fill);
        app.hatch_dialog_cancel();
        assert!(app.color_pick_target.is_none() && app.hatch_dialog.is_none());
    }

    #[test]
    fn switching_tab_with_select_color_open_drops_everything() {
        let mut app = app_with_rectangle();
        let first = app.active_tab;
        let _ = app.push_test_document();
        let second = app.tabs.len() - 1;
        let _ = app.update(Message::TabSwitch(first));
        open_with(&mut app, "HATCH", &[]);
        open_picker(&mut app, HatchColorSlot::Fill);
        let _ = app.update(Message::TabSwitch(second));
        assert!(app.color_pick_target.is_none(), "no stray colour window");
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
        assert!(app.tabs[first].scene.hatches.is_empty() && app.tabs[second].scene.hatches.is_empty());
    }

    #[test]
    fn another_windows_colour_picker_is_not_touched_by_the_hatch_rules() {
        let mut app = app_with_rectangle();
        let _ = app.update(Message::OpenColorWindow(ColorPickTarget::Ribbon, Color::Index(1)));
        app.hatch_dialog_cancel(); // no Hatch window at all
        assert!(app.color_pick_target.is_some());
        assert!(app.hatch_dialog_escape_overlay().is_none() || app.active_modal != Some(ModalKind::Hatch));
    }
```
Nota: `test` `a_pick_reaches…` usa array di tuple con `fn` pointer; se il compilatore protesta sul tipo, riscriverlo come tre blocchi.

- [ ] **Step 2:** `cargo test --locked --lib hatch_gradient 2>&1 | tail` → errori (`ColorPickTarget::Hatch` mancante ecc.).

- [ ] **Step 3: Implementare**

1. `app/mod.rs`: nel `enum ColorPickTarget` aggiungere
```rust
    /// A colour of the HATCH window: the fill colour or one of the gradient's.
    Hatch(crate::ui::window::hatch_dialog::HatchColorSlot),
```
2. `ui/window/hatch_dialog.rs`:
```rust
/// The message that sets `slot` to the colour "Select Color" returned. A
/// gradient's colours are true colours: ByLayer, ByBlock and None are refused.
pub fn color_pick_message(slot: HatchColorSlot, color: codec::types::Color) -> Option<Message> {
    use codec::types::Color;
    if slot != HatchColorSlot::Fill && matches!(color, Color::ByLayer | Color::ByBlock | Color::None) {
        return None;
    }
    Some(Message::HatchDialogField(slot.field(color)))
}
```
3. `style.rs` nel `match` di `on_color_window_pick`, prima di `Some(PlotStyle) => None`:
```rust
                    Some(crate::app::ColorPickTarget::Hatch(slot)) => {
                        crate::ui::window::hatch_dialog::color_pick_message(slot, color)
                    }
```
4. `commands/hatch_dialog.rs`: `hatch_dialog_field` come descritto (ritorna `Task`, intercetta `Field::SelectColor`); in `hatch_dialog_cancel`, in coda (anche se non c'era stato):
```rust
        if matches!(
            self.color_pick_target,
            Some((crate::app::ColorPickTarget::Hatch(_), _))
        ) {
            self.color_pick_target = None;
        }
```
5. `hatch_gradient.rs` (sopra i test):
```rust
use iced::Task;

use crate::app::{ColorPickTarget, ColorPickerTab, Message};
use crate::modules::draw::draw::hatch_settings::HatchColor;
use crate::ui::window::hatch_dialog::HatchColorSlot;

impl OpenCADStudio {
    /// "Select Color..." in a colour list of the window: close the list and
    /// open the colour window for `slot`, showing the slot's colour.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_select_color(&mut self, slot: HatchColorSlot) -> Task<Message> {
        if self.hatch_palette_open() {
            return Task::none();
        }
        let Some(state) = self.hatch_dialog.as_mut() else {
            return Task::none();
        };
        state.color_list = None;
        let current = match slot {
            HatchColorSlot::Fill => match state.settings.color {
                HatchColor::UseCurrent => self.ribbon.active_color,
                HatchColor::Color(color) => color,
            },
            HatchColorSlot::Gradient1 => state.settings.gradient.color1,
            HatchColorSlot::Gradient2 => state.settings.gradient.color2,
        };
        self.update(Message::OpenColorWindow(ColorPickTarget::Hatch(slot), current))
    }

    /// Esc with the HATCH window up: an overlay it owns closes first (the
    /// colour window, then an open colour list). `None` when there is none,
    /// and Esc goes on to the window itself.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_escape_overlay(&mut self) -> Option<Task<Message>> {
        if self.color_pick_target.is_some() {
            self.color_pick_target = None;
            return Some(Task::none());
        }
        let closed = self
            .hatch_dialog
            .as_mut()
            .is_some_and(|state| state.color_list.take().is_some());
        closed.then(Task::none)
    }

    /// Enter with the HATCH window up: the colour window confirms its pending
    /// colour (Index page) or ignores Enter (True Color page); an open colour
    /// list closes; otherwise Enter is OK.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_enter(&mut self) -> Task<Message> {
        if let Some((_, pending)) = self.color_pick_target.as_ref().map(|(t, c)| (t.clone(), *c)) {
            return match self.color_picker_tab {
                ColorPickerTab::Index => self.update(Message::ColorWindowPick(pending)),
                ColorPickerTab::TrueColor => Task::none(),
            };
        }
        let closed = self
            .hatch_dialog
            .as_mut()
            .is_some_and(|state| state.color_list.take().is_some());
        if closed {
            return Task::none();
        }
        self.update(Message::HatchDialogOk)
    }
}
```
6. `update/mod.rs`: arm `Message::HatchDialogField(field) => self.hatch_dialog_field(field),`; nel ramo Esc, prima di `return self.update(Message::CloseModal);`:
```rust
                if self.active_modal == Some(super::ModalKind::Hatch) {
                    if let Some(task) = self.hatch_dialog_escape_overlay() {
                        return task;
                    }
                }
```
e nel ramo Invio (riga ~441) `return self.update(Message::HatchDialogOk);` → `return self.hatch_dialog_enter();`.

- [ ] **Step 4:** `cargo test --locked --lib hatch_gradient hatch` → PASS.

- [ ] **Step 5: Mutazioni e commit**

(M) far agire Invio direttamente come `HatchDialogOk` con la finestra colore aperta → `enter_with_select_color_open_confirms…` fallisce (crea un retino). (M) togliere il controllo `color_pick_target` in `hatch_dialog_escape_overlay` → `escape_with_select_color_open…` fallisce. Ripristinare.

```bash
git add -A src && git commit -m "Finestra Hatch: Select Color e elenco colori; Invio ed Esc agiscono solo sull'overlay aperto"
```

---

### Task 12: Vista — schede, riga colore, scheda Gradient

**Files:**
- Modify: `src/ui/color_select.rs` (aggiungere `color_selector_labelled`)
- Modify: `src/ui/window/hatch_dialog.rs` (`view_window(state, current, sizing)`, schede cliccabili, riga Color, gruppi della scheda Gradient, swatch colorato)
- Modify: `src/app/view/modal.rs:521-528` (passare `self.ribbon.active_color`)

**Interfaces:**
- Consumes: Task 5 (`Field`, `HatchColorSlot`, `State.color_list`), Task 8 (`HatchPatternPreview::with_color`), `GradientSpec::model_pattern` (Task 4).
- Produces:
  - `color_selector_labelled(current: AcadColor, label: Option<String>, open: bool, extras: ColorExtras, on_select: impl Fn(AcadColor) -> Message + 'a, on_toggle: Message, on_more: Message) -> Element<'a, Message>`
  - `view_window(state: &State, current: AcadColor, sizing: ModalSizing) -> Element<'a, Message>`
  - funzioni pure testabili: `gradient_shape_labels() -> Vec<String>`, `gradient_shape_index(label: &str) -> Option<usize>`, `State::gradient_swatch(&self) -> (HatchPattern, [f32; 4])`.

- [ ] **Step 1: Test rossi** (modulo `tests` di `ui/window/hatch_dialog.rs`):

```rust
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
```

- [ ] **Step 2:** `cargo test --locked --lib hatch_dialog 2>&1 | tail` → rosso.

- [ ] **Step 3: Implementare**

`color_select.rs`:
```rust
/// Like [`color_selector_with_name`] but the head can read any text (e.g.
/// "Use Current") while still showing `current`'s swatch.
pub fn color_selector_labelled<'a>(
    current: AcadColor,
    label: Option<String>,
    open: bool,
    extras: ColorExtras,
    on_select: impl Fn(AcadColor) -> Message + 'a,
    on_toggle: Message,
    on_more: Message,
) -> Element<'a, Message> {
    let (bg, _) = acad_color_display(current);
    let name = label.unwrap_or_else(|| color_display_name(current));
    color_selector_with_indicator(swatch(bg), name, open, extras, on_select, on_toggle, on_more)
}
```
`hatch_dialog.rs` (view):
```rust
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

impl State {
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
```
Funzioni di vista (nuove, `#[inline(never)]` non necessario ma tenere ogni gruppo in una funzione propria, come le esistenti):

```rust
fn tab_button<'a>(label: String, tab: FillTab, active: bool) -> Element<'a, Message> {
    button(text(label).size(12))
        .padding([4, 14])
        .style(if active { button::primary } else { button::secondary })
        .on_press(Message::HatchDialogField(Field::Tab(tab)))
        .into()
}

/// One colour control: the shared selector, with "Select Color..." wired to
/// the colour window of this slot.
fn color_row<'a>(state: &State, slot: HatchColorSlot, current: AcadColor) -> Element<'a, Message> {
    use crate::ui::color_select::{color_selector_labelled, ColorExtras};
    let settings = &state.settings;
    let (shown, label) = match slot {
        HatchColorSlot::Fill => match settings.color {
            HatchColor::UseCurrent => (current, Some(t!("Use Current").into_owned())),
            HatchColor::Color(color) => (color, None),
        },
        HatchColorSlot::Gradient1 => (settings.gradient.color1, None),
        HatchColorSlot::Gradient2 => (settings.gradient.color2, None),
    };
    let open = state.color_list == Some(slot);
    let logical = slot == HatchColorSlot::Fill;
    let selector = color_selector_labelled(
        shown,
        label,
        open,
        ColorExtras { by_layer: logical, by_block: logical, ..Default::default() },
        move |color| Message::HatchDialogField(slot.field(color)),
        Message::HatchDialogField(Field::ColorList((!open).then_some(slot))),
        Message::HatchDialogField(Field::SelectColor(slot)),
    );
    selector
}
```
`type_and_pattern`: sostituire la riga grigia `labelled("Color", grey("Use Current"))` con

```rust
            labelled(
                t!("Color").into_owned(),
                row![
                    color_row(state, HatchColorSlot::Fill, current),
                    use_current_button(state),
                ]
                .spacing(6)
                .into(),
            ),
```
con
```rust
fn use_current_button<'a>(state: &State) -> Element<'a, Message> {
    if state.edit.is_some() {
        return Space::new().into(); // editing shows the hatch's own colour
    }
    dialog_button_styled_opt(
        t!("Use Current").into_owned(),
        (state.settings.color != HatchColor::UseCurrent)
            .then_some(Message::HatchDialogField(Field::Color(HatchColor::UseCurrent))),
        button::secondary,
    )
    .into()
}
```
e lo swatch del pattern: dopo aver costruito `HatchPatternPreview::new(gpu).with_angle_scale(angle, scale)` aggiungere `.with_color(iced_color)` dove `iced_color` = colore dell'`HatchColor::Color(c)` con `c.rgb()` (Index/Rgb), oppure il colore corrente `current` se `UseCurrent` e `current.rgb()` esiste; ByLayer/ByBlock → nessun `with_color` (colore del tema).

Scheda Gradient:
```rust
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
                text(format!("{:.0}%", g.tint * 100.0)).size(11),
            ]
            .spacing(6)
            .into(),
        )
    } else {
        labelled(t!("Color 2").into_owned(), color_row(state, HatchColorSlot::Gradient2, current))
    };
    let labels = gradient_shape_labels();
    let selected = labels.get(g.shape.min(labels.len() - 1)).cloned();
    let lookup = labels.clone();
    let shapes = pick_list(selected, labels, |label: &String| label.clone())
        .on_select(move |label: String| {
            Message::HatchDialogField(Field::GradientShape(
                lookup.iter().position(|c| *c == label).unwrap_or(0),
            ))
        })
        .text_size(12)
        .padding([3, 6])
        .width(Fill);
    let (pattern, first) = state.gradient_swatch();
    let swatch = canvas(
        crate::ui::properties::HatchPatternPreview::new(pattern)
            .with_color(iced::Color::from_rgba(first[0], first[1], first[2], first[3])),
    )
    .width(Fill)
    .height(Length::Fixed(44.0));
    column![
        group(
            t!("Color").into_owned(),
            column![
                modes,
                labelled(t!("Color 1").into_owned(), color_row(state, HatchColorSlot::Gradient1, current)),
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
```
`view_window(state, current, sizing)`: sostituire la riga `tabs` con
```rust
    let active = state.settings.tab;
    let tabs = row![
        tab_button(t!("Hatch").into_owned(), FillTab::Hatch, active == FillTab::Hatch),
        tab_button(t!("Gradient").into_owned(), FillTab::Gradient, active == FillTab::Gradient),
    ]
    .spacing(8);
```
(`page` della palette riceve `tabs.into()` come oggi) e la colonna sinistra:
```rust
    let left = match active {
        FillTab::Hatch => column![
            type_and_pattern(state, current),
            angle_and_scale(state),
            origin_group(state)
        ]
        .spacing(8)
        .width(Fill)
        .into(),
        FillTab::Gradient => gradient_left(state, current),
    };
```
(`type_and_pattern` prende `current`). In `modal.rs`:
```rust
                sized_flow(ex, 940, 760, |flow| {
                    crate::ui::window::hatch_dialog::view_window(state, self.ribbon.active_color, flow)
                })
```
Il limite 940×760 **non cambia**. Contare le righe della colonna sinistra della scheda Gradient (~440 px stimati) e lasciare il commento accanto a `sized_flow` aggiornato ("la scheda Gradient è più bassa di quella Hatch").

- [ ] **Step 4:** `cargo test --locked --lib hatch_dialog` → PASS; `cargo check --locked` pulito. Nessun test verifica l'aspetto: la verifica è dell'utente (§ Controlli manuali).

- [ ] **Step 5: Commit**

```bash
git add -A src && git commit -m "Finestra Hatch: schede cliccabili, selettore colore e scheda Gradient"
```

---

### Task 13: Documentazione e verifica finale

**Files:**
- Modify: `CLAUDE.md` (paragrafo `src/ui/window/hatch_dialog.rs …`: togliere "tab Gradient, Color/Transparency/Layer/Draw order, Inherit Properties, HATCHEDIT con la stessa finestra" da "Fuori dal giro"; aggiungere `src/entities/hatch_fill.rs`, `src/app/commands/hatch_gradient.rs`, `-GRADIENT`, il colore "Use Current" e `ColorPickTarget::Hatch`)
- Modify: `docs/superpowers/specs/2026-10-08-finestra-hatch-gradient-design.md` (riga di stato in testa: "implementata secondo il piano …")

- [ ] **Step 1: Aggiornare i due documenti** come sopra (testo breve, in italiano, nello stile del paragrafo esistente).

- [ ] **Step 2: Verifica completa, con l'output dei comandi**

```bash
cd "c:/Users/archi/Documents/Progetti IA/ArchLine" && cargo check --locked 2>&1 | tail -3
cd "c:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib hatch 2>&1 | tail -5
cd "c:/Users/archi/Documents/Progetti IA/ArchLine" && cargo test --locked --lib 2>&1 | tail -5
```
Expected: nessun errore; la suite completa passa (≥ 2407 − i test invertiti + i nuovi). In background: `cargo build --release --bin OpenCADStudio` (≈5 minuti). Riportare i numeri reali.

- [ ] **Step 3: Controllo della trappola n. 1 e dei file toccati**

`git diff --stat lavoro..HEAD -- src/app/update/mod.rs src/app/view/mod.rs src/app/command_driver/mod.rs src/app/update/viewport.rs` deve mostrare solo poche righe d'aggancio; `git diff lavoro..HEAD -- .cargo/config.toml src/locale_catalog.rs` deve essere vuoto.

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md docs && git commit -m "Docs: finestra Hatch and Gradient per pattern, solido e sfumato"
```

---

## Chiusura (dopo l'ultimo task; non sono task di implementazione)

1. Revisione dell'intero branch con il modello più capace e **una sola ondata di correzioni** (flusso subagent-driven-development; ledger in `.superpowers/sdd/`, escluso da git).
2. Consegnare all'utente la lista dei controlli manuali su Windows (§14 della spec) prima di pubblicare.
3. Solo con verifiche verdi, revisione pulita e lista consegnata: `git push -u origin claude/finestra-hatch` (mai `lavoro`/`main`) e `gh pr create --base lavoro` con titolo e descrizione in italiano, senza righe di attribuzione, che includa **tutto** il lavoro del branch rispetto a `lavoro` (finestra Hatch, browser dei pattern, modifica con doppio clic, questa estensione a solidi e sfumati) a blocchi; nella descrizione: cosa cambia per l'utente (incluso il colore "Use Current" dei nuovi retini e GRADIENT che ora apre la finestra), i controlli manuali ancora da fare, i problemi noti. **Non unire la PR.** Indicare il link alla corsa CI `ArchLine-windows-N`.
