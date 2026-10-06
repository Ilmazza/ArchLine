# Elenco delle barre, visibilità e clic prolungato — Piano di implementazione

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tasto destro su una barra = elenco di tutte le barre (una per gruppo del ribbon) con spunte; spuntare apre la barra flottante, togliere la spunta la nasconde; tasto sinistro prolungato su un'icona con varianti apre il flyout delle varianti.

**Architecture:** `Placement` guadagna `Hidden`; `ToolbarId` guadagna la variante `Group(&'static str)` per le barre generate dai gruppi del ribbon, e un registro (`toolbar_registry.rs`) le costruisce una volta dai moduli. Il menu è un `iced_aw::ContextMenu` attorno a ogni barra; il clic prolungato usa un `mouse_area` (press/release separati) e un timer a sottoscrizione, con il flyout ancorato dall'overlay dei dropdown del ribbon (`PosReport::owned` + `place_dropdown`).

**Tech Stack:** Rust, iced (rev 23604ff: `mouse_area`, `hover`, `pin`, `Stack`), `iced_aw::ContextMenu`, serde, `iced_test` (simulatore headless, già usato nel repo).

**Spec:** `docs/superpowers/specs/2026-10-06-barre-elenco-visibilita-design.md`

## Global Constraints

- Lavora su `lavoro`; un commit per task; **nessuna riga di attribuzione** nei messaggi di commit. Non pushare.
- Codice nuovo in file propri (`toolbar_registry.rs`) o nei file delle barre già propri; punti di aggancio in codice upstream minimi: `src/app/view/mod.rs` (una chiamata per l'overlay del flyout, una sottoscrizione, la firma di `frame`), `src/app/mod.rs` (stato `tool_hold`), `src/app/update/mod.rs` (nessuna modifica prevista).
- `view_main` è enorme: **nessuna logica nuova lì**, solo chiamate a funzioni `#[inline(never)]` (trappola 1 di `CLAUDE.md`).
- Le righe di un `iced_aw::ContextMenu` sono `mouse_area` con `on_press`, mai `button` (trappola 2).
- Non rimuovere `RUST_MIN_STACK` da `.cargo/config.toml`.
- Stringhe UI in inglese tramite `crate::t!(...)`.
- Chiavi salvate: le 6 barre storiche mantengono `Draw`, `Modify`, `Annotation`, `Block`, `Measure`, `Layers`; le altre `modulo:Titolo` (es. `annotate:Dimensions`). Un `settings.json` con solo le 6 chiavi deve continuare a caricarsi.
- Soglia clic prolungato `HOLD_MS = 400`; cascata `x = 120 + 28·n`, `y = 130 + 28·n`, `n` = barre flottanti già presenti modulo 8.
- Comandi: `cargo check --locked`; `cargo test --locked --lib <filtro>`; build finale `cargo build --release --bin OpenCADStudio` (se compare "Accesso negato" chiedere all'utente di chiudere ArchLine). `cargo test --lib` completo: si sa già che `gdi_fallback_lands_ink_on_the_printed_page` fallisce (stampante) e `searchable_layer_coexists_with_outlines` è instabile in parallelo: non sono legati alle barre.
- Gli script Python di patch si scrivono su file (non heredoc con apostrofi nel comando) e si eseguono; i file del repo hanno fine riga CRLF: usare `newline=""` e preservare `\r\n`.

## Review Focus

1. `settings.json` con solo le 6 chiavi storiche, con una chiave di gruppo che il registro non conosce più, o con `Hidden` per una barra built-in → si carica, nessun errore (Task 1, Task 2).
2. Barre nascoste di default: ~30 gruppi non devono finire nel file salvato né comparire sui bordi (Task 2).
3. Spuntare dieci barre di fila → tutte flottanti e raggiungibili (cascata modulo 8, clamp al render) (Task 1).
4. Nascondere una barra agganciata → le lane si compattano e il riaggancio da `home` funziona (Task 1).
5. Clic breve su un'icona con varianti → comando eseguito; rilascio fuori dal pulsante o uscita del cursore → nessun comando; clic tenuto → flyout, nessun comando (Task 4).
6. Tasto destro: il menu elenca anche le barre nascoste e il click su una riga lo chiude (Task 3).

---

### Task 1: Modello — `Hidden`, `set_visible`, cascata

**Files:**
- Modify: `src/ui/toolbar_layout.rs`

**Interfaces:**
- Consumes: `ToolbarLayout`, `Placement`, `DockSlot` (esistenti).
- Produces (usati dai task 2–4):
  - `Placement::Hidden { home: Option<DockSlot> }`.
  - `ToolbarLayout::is_visible(&self, id: ToolbarId) -> bool`.
  - `ToolbarLayout::set_visible(&mut self, id: ToolbarId, visible: bool)`.
  - costanti `CASCADE_X: f32 = 120.0`, `CASCADE_Y: f32 = 130.0`, `CASCADE_STEP: f32 = 28.0`, `CASCADE_STEPS: usize = 8`.

- [ ] **Step 1: Scrivere i test che falliscono**

In `src/ui/toolbar_layout.rs`, nel modulo `tests`, aggiungere in fondo (prima dell'ultima `}` del modulo):

```rust
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
```

- [ ] **Step 2: Verificare che falliscano**

Run: `cargo test --locked --lib toolbar_layout 2>&1 | grep -E "^error" | sort | uniq -c | head`
Expected: errori di compilazione `no variant Hidden`, `no method is_visible/set_visible`, `CASCADE_X` non trovato.

- [ ] **Step 3: Implementare**

Script di patch `scratchpad/t1.py` (eseguirlo con `python`):

```python
import os
os.chdir("c:/Users/archi/Documents/Progetti IA/ArchLine")
def sub(p, old, new, count=1):
    s = open(p, encoding="utf-8", newline="").read()
    nl = "\r\n" if "\r\n" in s else "\n"
    old = old.replace("\n", nl); new = new.replace("\n", nl)
    assert s.count(old) == count, (p, old[:70], s.count(old))
    open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))
p = "src/ui/toolbar_layout.rs"
sub(p, "pub const BOTTOM_CHROME: f32 = 30.0;\n", """pub const BOTTOM_CHROME: f32 = 30.0;
/// First floating position given to a bar shown from the list, and the step
/// between consecutive ones (a cascade, wrapping after `CASCADE_STEPS`).
pub const CASCADE_X: f32 = 120.0;
pub const CASCADE_Y: f32 = 130.0;
pub const CASCADE_STEP: f32 = 28.0;
pub const CASCADE_STEPS: usize = 8;
""")
sub(p, """        home: Option<DockSlot>,
    },
}
""", """        home: Option<DockSlot>,
    },
    /// Not shown. `home` is the last docked slot, kept for when it comes back.
    Hidden { home: Option<DockSlot> },
}
""")
sub(p, """            Placement::Floating { home, .. } => home,
        };
        match target {""", """            Placement::Floating { home, .. } | Placement::Hidden { home } => home,
        };
        match target {""")
sub(p, "                Placement::Docked(_) => None,\n            })\n            .collect()\n    }\n\n    /// Rewrite the slots", "                _ => None,\n            })\n            .collect()\n    }\n\n    /// Rewrite the slots")
sub(p, """                other => other,
            };
            self.bars.insert(id.key().to_string(), fixed);""", """                Placement::Hidden { home } => Placement::Hidden {
                    home: home.filter(|s| id.allowed_on(s.edge)),
                },
                other => other,
            };
            self.bars.insert(id.key().to_string(), fixed);""")
sub(p, "    /// Double-click on a floating bar: back to where it was docked.", """    pub fn is_visible(&self, id: ToolbarId) -> bool {
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

    /// Double-click on a floating bar: back to where it was docked.""")
print("t1 ok")
```

- [ ] **Step 4: Eseguire i test**

Run: `cargo test --locked --lib toolbar_layout 2>&1 | tail -6` → PASS (17 test); poi `cargo check --locked 2>&1 | grep -E "^(error|warning)" -A5 | head` → nessun errore (se un `match` su `Placement` altrove diventa non esaustivo, aggiungere l'arm `Hidden`).

- [ ] **Step 5: Commit**

```bash
git add src/ui/toolbar_layout.rs
git commit -m "Barre classiche: stato nascosto e cascata di apertura nel modello"
```

---

### Task 2: Registro delle barre (una per gruppo del ribbon)

**Files:**
- Create: `src/ui/toolbar_registry.rs`
- Modify: `src/ui/mod.rs` (`pub mod toolbar_registry;`), `src/ui/toolbar_layout.rs` (variante `Group`, `all()`, default), `src/ui/classic_toolbar.rs` (`buttons_of`, `items_for`), `src/ui/toolbar_dock.rs` (`floating_id`), `src/app/view/classic.rs` (test: `ToolbarId::ALL`)

**Interfaces:**
- Consumes: `Placement::Hidden` (Task 1), `flatten` di `classic_toolbar`.
- Produces (usati dai task 3–4):
  - `ToolbarId::Group(&'static str)`; `ToolbarId::BUILTIN: [ToolbarId; 6]`; `ToolbarId::all() -> &'static [ToolbarId]`; `ToolbarId::is_builtin(self) -> bool`; `ToolbarId::title(self) -> &'static str` (nome nell'elenco).
  - `toolbar_registry::{all_ids() -> &'static [ToolbarId], entries() -> &'static [(ToolbarId, &'static str)] /* ordinato per nome */, display_name(ToolbarId) -> &'static str, group_items(ToolbarId) -> &'static [ClassicItem]}`.
  - `classic_toolbar::buttons_of(&[RibbonItem]) -> Vec<ClassicItem>` (`pub(super)`).

- [ ] **Step 1: Scrivere i test che falliscono**

Creare `src/ui/toolbar_registry.rs` contenente SOLO i test (l'implementazione allo step 3) e registrare il modulo in `src/ui/mod.rs` (`pub mod toolbar_registry;` dopo `pub mod toolbar_dock;`):

```rust
//! ArchLine: registry of every toolbar the user can show.
//!
//! The six historic bars keep their names; every other ribbon group with at
//! least one button becomes a bar keyed `module:Title`.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::classic_toolbar::{items_for, ClassicItem};
    use crate::ui::toolbar_layout::{ToolbarId, ToolbarLayout};

    fn groups() -> Vec<ToolbarId> {
        all_ids().iter().copied().filter(|id| !id.is_builtin()).collect()
    }

    #[test]
    fn the_historic_keys_still_resolve() {
        for key in ["Draw", "Modify", "Annotation", "Block", "Measure", "Layers"] {
            let id = ToolbarId::from_key(key).unwrap_or_else(|| panic!("{key} missing"));
            assert_eq!(id.key(), key);
            assert!(id.is_builtin());
        }
        assert_eq!(ToolbarId::BUILTIN.len(), 6);
    }

    #[test]
    fn group_bars_exist_with_unique_prefixed_keys() {
        let g = groups();
        assert!(g.len() >= 5, "only {} group bars", g.len());
        let mut keys: Vec<_> = all_ids().iter().map(|id| id.key()).collect();
        let n = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), n, "duplicate bar keys");
        for id in g {
            assert!(id.key().contains(':'), "{} is not module:Title", id.key());
            assert_eq!(ToolbarId::from_key(id.key()), Some(id));
        }
    }

    #[test]
    fn every_offered_group_bar_has_buttons() {
        for id in groups() {
            assert!(
                items_for(id).iter().any(|i| matches!(i, ClassicItem::Button(_))),
                "{} has no buttons",
                id.key()
            );
        }
    }

    #[test]
    fn names_are_unique_sorted_and_builtins_stay_plain() {
        let e = entries();
        assert_eq!(e.len(), all_ids().len());
        let lower: Vec<String> = e.iter().map(|(_, n)| n.to_lowercase()).collect();
        let mut sorted = lower.clone();
        sorted.sort();
        assert_eq!(lower, sorted, "entries are alphabetical");
        let mut unique = lower.clone();
        unique.dedup();
        assert_eq!(unique.len(), lower.len(), "two bars share a name: {lower:?}");
        for b in ToolbarId::BUILTIN {
            assert_eq!(display_name(b), b.key(), "built-in names stay plain");
        }
    }

    #[test]
    fn group_bars_are_hidden_by_default_and_not_written_to_the_file() {
        // Review focus 2.
        let l = ToolbarLayout::default();
        assert_eq!(l.bars.len(), 6, "only the built-ins are materialised");
        let g = groups();
        assert!(!l.is_visible(g[0]));
        let l = l.sanitized();
        assert_eq!(l.bars.len(), 6, "sanitize must not materialise ~30 hidden bars");
    }

    #[test]
    fn sanitize_keeps_known_group_keys_and_drops_unknown_ones() {
        // Review focus 1.
        let g = groups()[0];
        let json = format!(
            r#"{{"bars":{{"{}":{{"Floating":{{"x":10.0,"y":20.0,"home":null}}}},"gone:Removed":"Hidden"}}}}"#,
            g.key()
        );
        let l: ToolbarLayout = serde_json::from_str(&json).unwrap();
        let l = l.sanitized();
        assert!(l.is_visible(g), "a saved group bar keeps its position");
        assert!(!l.bars.contains_key("gone:Removed"));
    }
}
```

- [ ] **Step 2: Verificare che falliscano**

Run: `cargo test --locked --lib toolbar_registry 2>&1 | grep -E "^error" | sort | uniq -c | head`
Expected: errori `cannot find function all_ids`, `no function or associated item BUILTIN/is_builtin` ecc.

- [ ] **Step 3: Implementare**

Script `scratchpad/t2.py` che applica le modifiche (eseguirlo):

```python
import os
os.chdir("c:/Users/archi/Documents/Progetti IA/ArchLine")
def sub(p, old, new, count=1):
    s = open(p, encoding="utf-8", newline="").read()
    nl = "\r\n" if "\r\n" in s else "\n"
    old = old.replace("\n", nl); new = new.replace("\n", nl)
    assert s.count(old) == count, (p, old[:70], s.count(old))
    open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))

L = "src/ui/toolbar_layout.rs"
# --- ToolbarId: Group variant, BUILTIN, all(), is_builtin ---
sub(L, "    Measure,\n    Layers,\n}\n\nimpl ToolbarId {\n    pub const ALL: [ToolbarId; 6] = [",
"""    Measure,
    Layers,
    /// Any other ribbon group with buttons; the payload is its key
    /// (`module:Title`), built once by `toolbar_registry`.
    Group(&'static str),
}

impl ToolbarId {
    /// Every bar the user can show: the six historic ones, then one per
    /// ribbon group.
    pub fn all() -> &'static [ToolbarId] {
        super::toolbar_registry::all_ids()
    }

    pub fn is_builtin(self) -> bool {
        !matches!(self, ToolbarId::Group(_))
    }

    pub const BUILTIN: [ToolbarId; 6] = [""")
sub(L, "            ToolbarId::Layers => \"Layers\",\n        }\n    }\n\n    pub fn from_key(key: &str) -> Option<ToolbarId> {\n        Self::ALL.into_iter().find(|id| id.key() == key)\n    }\n\n    /// Title shown on a floating bar.\n    pub fn title(self) -> &'static str {\n        self.key()\n    }",
"""            ToolbarId::Layers => "Layers",
            ToolbarId::Group(key) => key,
        }
    }

    pub fn from_key(key: &str) -> Option<ToolbarId> {
        Self::all().iter().copied().find(|id| id.key() == key)
    }

    /// Name shown in the bar list and on a floating bar.
    pub fn title(self) -> &'static str {
        super::toolbar_registry::display_name(self)
    }""")
# --- defaults ---
sub(L, "        Layers => (Top, 1, 0),\n    };\n    DockSlot { edge, lane, index }\n}\n",
"""        Layers => (Top, 1, 0),
        // Only reached to re-dock a group bar that has no remembered home.
        Group(_) => (Top, 0, u8::MAX),
    };
    DockSlot { edge, lane, index }
}

fn default_placement(id: ToolbarId) -> Placement {
    match id {
        ToolbarId::Group(_) => Placement::Hidden { home: None },
        other => Placement::Docked(default_slot(other)),
    }
}
""")
sub(L, """            bars: ToolbarId::ALL
                .into_iter()
                .map(|id| (id.key().to_string(), Placement::Docked(default_slot(id))))
                .collect(),""", """            bars: ToolbarId::BUILTIN
                .into_iter()
                .map(|id| (id.key().to_string(), default_placement(id)))
                .collect(),""")
sub(L, "            .unwrap_or(Placement::Docked(default_slot(id)))", "            .unwrap_or(default_placement(id))")
sub(L, "        let mut docked: Vec<(DockSlot, ToolbarId)> = ToolbarId::ALL\n            .into_iter()\n            .filter_map(|id| match self.placement(id) {", "        let mut docked: Vec<(DockSlot, ToolbarId)> = ToolbarId::all()\n            .iter()\n            .copied()\n            .filter_map(|id| match self.placement(id) {")
sub(L, "    pub fn floating(&self) -> Vec<(ToolbarId, f32, f32)> {\n        ToolbarId::ALL\n            .into_iter()\n            .filter_map(", "    pub fn floating(&self) -> Vec<(ToolbarId, f32, f32)> {\n        ToolbarId::all()\n            .iter()\n            .copied()\n            .filter_map(")
sub(L, "        for id in ToolbarId::ALL {\n            let fixed = match self.placement(id) {", """        for &id in ToolbarId::all() {
            // Hidden group bars are the default: do not write ~30 of them out.
            if !id.is_builtin() && !self.bars.contains_key(id.key()) {
                continue;
            }
            let fixed = match self.placement(id) {""")

# --- classic_toolbar: buttons_of + items_for ---
C = "src/ui/classic_toolbar.rs"
sub(C, "/// Buttons of the draw-module groups named in `titles`", """/// Buttons of one ribbon group's items (dropdowns collapse to one button).
pub(super) fn buttons_of(tools: &[RibbonItem]) -> Vec<ClassicItem> {
    let mut out = Vec::new();
    for item in tools {
        flatten(item, &mut out);
    }
    out
}

/// Buttons of the draw-module groups named in `titles`""")
sub(C, "        Layers => &[],\n    }\n}", "        Layers => &[],\n        Group(_) => super::toolbar_registry::group_items(id),\n    }\n}")

# --- toolbar_dock: floating id for group bars ---
D = "src/ui/toolbar_dock.rs"
sub(D, '        ToolbarId::Layers => "toolbar-float-Layers",\n', '        ToolbarId::Layers => "toolbar-float-Layers",\n        ToolbarId::Group(key) => key,\n')

# --- classic.rs test: ALL -> all() ---
sub("src/app/view/classic.rs", "        for (i, id) in ToolbarId::ALL.into_iter().enumerate() {", "        for (i, id) in ToolbarId::all().iter().copied().enumerate() {")
print("t2 patched")
```

Poi inserire in `src/ui/toolbar_registry.rs`, **sopra** `#[cfg(test)] mod tests`:

```rust
use std::sync::OnceLock;

use super::classic_toolbar::{buttons_of, ClassicItem};
use super::toolbar_layout::ToolbarId;
use crate::modules::registry;

/// Ribbon groups of the `draw` module that are already the built-in bars.
const BUILTIN_GROUPS: [&str; 6] = ["Draw", "Modify", "Annotation", "Layers", "Block", "Measure"];

struct Group {
    id: ToolbarId,
    items: Vec<ClassicItem>,
}

struct Registry {
    ids: Vec<ToolbarId>,
    groups: Vec<Group>,
    /// Every bar with its list name, alphabetical.
    names: Vec<(ToolbarId, &'static str)>,
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn build() -> Registry {
    // (key id, group title, module title, items)
    let mut raw: Vec<(ToolbarId, &'static str, &'static str, Vec<ClassicItem>)> = Vec::new();
    for module in registry::all_modules() {
        for group in module.ribbon_groups() {
            if module.id() == "draw" && BUILTIN_GROUPS.contains(&group.title) {
                continue;
            }
            let items = buttons_of(&group.tools);
            if !items.iter().any(|i| matches!(i, ClassicItem::Button(_))) {
                continue;
            }
            let key = leak(format!("{}:{}", module.id(), group.title));
            raw.push((ToolbarId::Group(key), group.title, module.title(), items));
        }
    }
    // A title shared by two bars is told apart by its module.
    let shared = |title: &str| {
        let builtin = ToolbarId::BUILTIN
            .iter()
            .filter(|b| b.key().eq_ignore_ascii_case(title))
            .count();
        let groups = raw.iter().filter(|r| r.1.eq_ignore_ascii_case(title)).count();
        builtin + groups > 1
    };
    let mut names: Vec<(ToolbarId, &'static str)> =
        ToolbarId::BUILTIN.iter().map(|&id| (id, id.key())).collect();
    for (id, title, module_title, _) in &raw {
        let name = if shared(title) {
            leak(format!("{title} ({module_title})"))
        } else {
            *title
        };
        names.push((*id, name));
    }
    names.sort_by_key(|(_, n)| n.to_lowercase());
    let ids = ToolbarId::BUILTIN
        .iter()
        .copied()
        .chain(raw.iter().map(|r| r.0))
        .collect();
    let groups = raw
        .into_iter()
        .map(|(id, _, _, items)| Group { id, items })
        .collect();
    Registry { ids, groups, names }
}

fn registry_data() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(build)
}

/// Every bar, built-ins first.
pub fn all_ids() -> &'static [ToolbarId] {
    &registry_data().ids
}

/// Every bar with its list name, alphabetical (case-insensitive).
pub fn entries() -> &'static [(ToolbarId, &'static str)] {
    &registry_data().names
}

pub fn display_name(id: ToolbarId) -> &'static str {
    registry_data()
        .names
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, n)| *n)
        .unwrap_or_else(|| id.key())
}

/// Buttons of a group bar (empty for a built-in).
pub fn group_items(id: ToolbarId) -> &'static [ClassicItem] {
    registry_data()
        .groups
        .iter()
        .find(|g| g.id == id)
        .map(|g| g.items.as_slice())
        .unwrap_or(&[])
}
```

- [ ] **Step 4: Eseguire i test**

Run: `python scratchpad/t2.py`, poi `cargo test --locked --lib toolbar_registry 2>&1 | tail -10` → PASS (6 test). **Annotare nel ledger il numero reale di barre di gruppo** (`group_bars_exist…` stampa solo in caso di errore: usare `cargo test --locked --lib group_bars_exist -- --nocapture` dopo aver aggiunto temporaneamente un `println!("{}", g.len())`, oppure contare con un test one-off; poi rimuovere). Se `g.len() >= 5` fallisce, abbassare la soglia al valore reale e ledgerare il ruling.
Run: `cargo test --locked --lib toolbar 2>&1 | tail -4` → tutti PASS (i test delle barre esistenti restano verdi: la vista con ~36 barre e 6 visibili funziona).
Run: `cargo check --locked 2>&1 | grep -E "^(error|warning)" -A5 | head` → nessun errore.

- [ ] **Step 5: Commit**

```bash
git add -A src
git commit -m "Barre classiche: una barra per ogni gruppo del ribbon (registro)"
```

---

### Task 3: Menu delle barre col tasto destro e `Toggle`

**Files:**
- Modify: `src/ui/toolbar_dock.rs` (menu, plumbing), `src/ui/classic_toolbar.rs` (tolto il flyout col tasto destro; `panel_style` `pub(super)`), `src/app/update/toolbar.rs` (`Toggle`), `src/app/view/mod.rs` (firma di `frame`), `src/app/view/classic.rs` (test, firma di `frame`)

**Interfaces:**
- Consumes: `entries()`, `ToolbarLayout::is_visible/set_visible` (Task 1–2), `icons::themed_check_cell`.
- Produces: `ToolbarMsg::Toggle(ToolbarId)`; `toolbar_dock::frame(classic, layout, ribbon, dragging, win_h: f32, center)` (nuovo parametro `win_h`); `BarEntry`, `menu_entries(layout)`.
- Nota: da qui fino al Task 4 le varianti dei pulsanti **non sono raggiungibili** (il tasto destro ora apre l'elenco). Ledgerarlo come ruling e non toccare `flyout`/`flyout_row` (servono al Task 4).

- [ ] **Step 1: Test che falliscono**

In `src/app/update/toolbar.rs`, nel modulo `tests`, aggiungere:

```rust
    #[test]
    fn toggling_a_hidden_group_bar_opens_it_floating_and_again_hides_it() {
        let mut app = app();
        let id = *ToolbarId::all()
            .iter()
            .find(|id| !id.is_builtin())
            .expect("a group bar");
        assert!(!app.toolbars.is_visible(id));
        toolbar(&mut app, ToolbarMsg::Toggle(id));
        assert!(matches!(app.toolbars.placement(id), Placement::Floating { .. }));
        toolbar(&mut app, ToolbarMsg::Toggle(id));
        assert!(!app.toolbars.is_visible(id));
    }

    #[test]
    fn toggling_a_builtin_hides_it_and_compacts_its_lane() {
        let mut app = app();
        toolbar(&mut app, ToolbarMsg::Toggle(ToolbarId::Layers));
        assert!(!app.toolbars.is_visible(ToolbarId::Layers));
        assert_eq!(app.toolbars.lanes(Edge::Top).len(), 1);
        toolbar(&mut app, ToolbarMsg::Toggle(ToolbarId::Layers));
        assert!(matches!(
            app.toolbars.placement(ToolbarId::Layers),
            Placement::Floating { .. }
        ));
    }
```

In `src/app/view/classic.rs`, nel modulo `tests`, aggiungere il test sul widget reale (aggiornare anche le chiamate esistenti a `frame(...)`: il nuovo quinto argomento è `app.win_size.1`):

```rust
    #[test]
    fn right_click_on_a_bar_opens_the_list_and_a_row_toggles_it() {
        use crate::ui::toolbar_layout::ToolbarId;
        let mut app = OpenCADStudio::new_for_test();
        app.toolbars = Default::default();
        app.win_size = (1600.0, 900.0);
        let center: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let el = crate::ui::toolbar_dock::frame(
            true,
            &app.toolbars,
            &app.ribbon,
            None,
            app.win_size.1,
            center,
        );
        let mut ui = iced_test::simulator(el);
        // The Draw bar hugs the left edge: (20, 20) is on it.
        let at = iced::Point::new(20.0, 20.0);
        ui.point_at(at);
        ui.simulate([iced_core::Event::Mouse(iced::mouse::Event::ButtonPressed(
            iced::mouse::Button::Right,
        ))]);
        ui.click("Modify").expect("the bar list shows Modify");
        for message in ui.into_messages() {
            let _ = app.update(message);
        }
        assert!(
            !app.toolbars.is_visible(ToolbarId::Modify),
            "clicking the ticked row hides the bar"
        );
    }
```

Run: `cargo test --locked --lib toolbar 2>&1 | grep -E "^error" | sort | uniq -c | head`
Expected: errori (`no variant Toggle`, argomenti di `frame`).

- [ ] **Step 2: Implementare**

Script `scratchpad/t3.py`:

```python
import os
os.chdir("c:/Users/archi/Documents/Progetti IA/ArchLine")
def sub(p, old, new, count=1):
    s = open(p, encoding="utf-8", newline="").read()
    nl = "\r\n" if "\r\n" in s else "\n"
    old = old.replace("\n", nl); new = new.replace("\n", nl)
    assert s.count(old) == count, (p, old[:70], s.count(old))
    open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))

D = "src/ui/toolbar_dock.rs"
sub(D, "    /// Put every bar back where it was at first launch.\n    Reset,\n", "    /// Put every bar back where it was at first launch.\n    Reset,\n    /// Tick / untick a bar in the right-click list.\n    Toggle(ToolbarId),\n")
sub(D, "use iced::widget::{column, container, mouse_area, opaque, pin, row, scrollable, text, Space, Stack};",
       "use std::sync::Arc;\n\nuse iced::widget::{column, container, mouse_area, opaque, pin, row, scrollable, text, Space, Stack};")
sub(D, "use super::classic_toolbar::{item_el, items_for, strip_style, ClassicItem, BTN_SIZE};",
       "use super::classic_toolbar::{item_el, items_for, panel_style, strip_style, ClassicItem, BTN_SIZE};")
sub(D, "/// Estimated length of a bar along its own axis", '''/// One row of the right-click bar list.
#[derive(Clone, Debug)]
pub struct BarEntry {
    pub id: ToolbarId,
    pub name: &'static str,
    pub visible: bool,
}

/// What every bar needs to open the list: the entries (built once per view and
/// shared) and the height the list may take.
#[derive(Clone)]
struct BarsMenu {
    entries: Arc<[BarEntry]>,
    max_h: f32,
}

/// Every bar, alphabetical, with whether it is currently shown.
pub fn menu_entries(layout: &ToolbarLayout) -> Arc<[BarEntry]> {
    super::toolbar_registry::entries()
        .iter()
        .map(|&(id, name)| BarEntry {
            id,
            name,
            visible: layout.is_visible(id),
        })
        .collect::<Vec<_>>()
        .into()
}

fn bars_menu_for(layout: &ToolbarLayout, win_h: f32) -> BarsMenu {
    BarsMenu {
        entries: menu_entries(layout),
        max_h: (win_h - 80.0).clamp(160.0, 600.0),
    }
}

/// Rows are `mouse_area`s publishing on press, never buttons: `ContextMenu`
/// rebuilds its overlay every view (trap 2 in CLAUDE.md).
fn bars_menu_panel(menu: &BarsMenu) -> Element<'static, Message> {
    let rows: Vec<Element<'static, Message>> = menu
        .entries
        .iter()
        .map(|e| {
            mouse_area(
                container(
                    row![
                        super::icons::themed_check_cell::<Message>(e.visible),
                        text(crate::t!(e.name).into_owned()).size(12)
                    ]
                    .spacing(8)
                    .align_y(iced::Center),
                )
                .padding([3, 10])
                .width(Length::Fill),
            )
            .on_press(Message::Toolbar(ToolbarMsg::Toggle(e.id)))
            .interaction(iced::mouse::Interaction::Pointer)
            .into()
        })
        .collect();
    container(scrollable(column(rows)))
        .padding(2)
        .width(Length::Fixed(260.0))
        .max_height(menu.max_h)
        .style(panel_style)
        .into()
}

fn with_bars_menu(el: Element<'static, Message>, menu: &BarsMenu) -> Element<'static, Message> {
    let menu = menu.clone();
    iced_aw::ContextMenu::new(el, move || bars_menu_panel(&menu)).into()
}

/// Estimated length of a bar along its own axis''')

# bar_el / lane_el / edge_el / frame plumbing
sub(D, "    ribbon: &Ribbon,\n    being_dragged: bool,\n) -> Element<'static, Message> {\n    let body = bar_body(id, vertical, ribbon);",
       "    ribbon: &Ribbon,\n    being_dragged: bool,\n    menu: &BarsMenu,\n) -> Element<'static, Message> {\n    let body = bar_body(id, vertical, ribbon);")
sub(D, "    container(inner)\n        .padding(2)\n        .style(move |theme: &Theme| {\n            let p = theme.palette();\n            container::Style {\n                background: being_dragged",
       "    let bar = container(inner)\n        .padding(2)\n        .style(move |theme: &Theme| {\n            let p = theme.palette();\n            container::Style {\n                background: being_dragged")
sub(D, "                ..Default::default()\n            }\n        })\n        .into()\n}\n\nfn lane_el(",
       "                ..Default::default()\n            }\n        });\n    with_bars_menu(bar.into(), menu)\n}\n\nfn lane_el(")
sub(D, "    ribbon: &Ribbon,\n    dragging: Option<ToolbarId>,\n) -> Element<'static, Message> {\n    let els: Vec<Element<'static, Message>> = bars\n        .iter()\n        .map(|&id| bar_el(id, vertical, ribbon, dragging == Some(id)))",
       "    ribbon: &Ribbon,\n    dragging: Option<ToolbarId>,\n    menu: &BarsMenu,\n) -> Element<'static, Message> {\n    let els: Vec<Element<'static, Message>> = bars\n        .iter()\n        .map(|&id| bar_el(id, vertical, ribbon, dragging == Some(id), menu))")
sub(D, "    edge: Edge,\n    ribbon: &Ribbon,\n    dragging: Option<ToolbarId>,\n) -> Element<'static, Message> {\n    let mut lanes",
       "    edge: Edge,\n    ribbon: &Ribbon,\n    dragging: Option<ToolbarId>,\n    menu: &BarsMenu,\n) -> Element<'static, Message> {\n    let mut lanes")
sub(D, "        .map(|l| lane_el(l, vertical, ribbon, dragging))", "        .map(|l| lane_el(l, vertical, ribbon, dragging, menu))")
sub(D, "    dragging: Option<ToolbarId>,\n    center: Element<'a, Message>,\n) -> Element<'a, Message> {\n    if !classic {\n        return center;\n    }\n    let middle = row![\n        edge_el(layout, Edge::Left, ribbon, dragging),\n        container(center).width(Length::Fill).height(Length::Fill),\n        edge_el(layout, Edge::Right, ribbon, dragging),\n    ]",
       "    dragging: Option<ToolbarId>,\n    win_h: f32,\n    center: Element<'a, Message>,\n) -> Element<'a, Message> {\n    if !classic {\n        return center;\n    }\n    let menu = bars_menu_for(layout, win_h);\n    let middle = row![\n        edge_el(layout, Edge::Left, ribbon, dragging, &menu),\n        container(center).width(Length::Fill).height(Length::Fill),\n        edge_el(layout, Edge::Right, ribbon, dragging, &menu),\n    ]")
sub(D, "        edge_el(layout, Edge::Top, ribbon, dragging),\n        middle,\n        edge_el(layout, Edge::Bottom, ribbon, dragging),",
       "        edge_el(layout, Edge::Top, ribbon, dragging, &menu),\n        middle,\n        edge_el(layout, Edge::Bottom, ribbon, dragging, &menu),")
# floating bars
sub(D, "fn floating_el(id: ToolbarId, ribbon: &Ribbon, being_dragged: bool) -> Element<'static, Message> {",
       "fn floating_el(\n    id: ToolbarId,\n    ribbon: &Ribbon,\n    being_dragged: bool,\n    menu: &BarsMenu,\n) -> Element<'static, Message> {")
sub(D, "    mouse_area(frame)\n        .on_press(Message::Toolbar(ToolbarMsg::Grab(id)))\n        .on_double_click(Message::Toolbar(ToolbarMsg::Redock(id)))\n        .interaction(iced::mouse::Interaction::Grab)\n        .into()\n}",
       "    let grab = mouse_area(frame)\n        .on_press(Message::Toolbar(ToolbarMsg::Grab(id)))\n        .on_double_click(Message::Toolbar(ToolbarMsg::Redock(id)))\n        .interaction(iced::mouse::Interaction::Grab);\n    with_bars_menu(grab.into(), menu)\n}")
sub(D, "    win: (f32, f32),\n    dragging: Option<ToolbarId>,\n) -> Option<Element<'static, Message>> {\n    let bars = floating_positions(layout, win);",
       "    win: (f32, f32),\n    dragging: Option<ToolbarId>,\n    menu: &BarsMenu,\n) -> Option<Element<'static, Message>> {\n    let bars = floating_positions(layout, win);")
sub(D, "pin(opaque(floating_el(id, ribbon, dragging == Some(id))))", "pin(opaque(floating_el(id, ribbon, dragging == Some(id), menu)))")
sub(D, "    if let Some(floating) = floating_layer(layout, ribbon, win, drag.map(|d| d.id)) {",
       "    let menu = bars_menu_for(layout, win.1);\n    if let Some(floating) = floating_layer(layout, ribbon, win, drag.map(|d| d.id), &menu) {")

# classic_toolbar: right click no longer opens the variants (long press does, Task 4)
C = "src/ui/classic_toolbar.rs"
sub(C, "    if b.variants.is_empty() {\n        return with_tip;\n    }\n    let variants = b.variants.clone();\n    iced_aw::ContextMenu::new(with_tip, move || flyout(&variants)).into()\n}",
       "    // Variants open on a long press (see the hold flyout); right click now\n    // opens the bar list on the whole bar.\n    with_tip\n}")
sub(C, "fn flyout_row(", "#[allow(dead_code)] // used again by the long-press flyout\nfn flyout_row(")
sub(C, "fn flyout(variants", "#[allow(dead_code)] // used again by the long-press flyout\nfn flyout(variants")
sub(C, "fn panel_style(", "pub(super) fn panel_style(")

# handler
T = "src/app/update/toolbar.rs"
sub(T, "            ToolbarMsg::Reset => {", """            ToolbarMsg::Toggle(id) => {
                let shown = self.toolbars.is_visible(id);
                self.toolbars.set_visible(id, !shown);
                self.save_config();
            }
            ToolbarMsg::Reset => {""")

# view hook
V = "src/app/view/mod.rs"
sub(V, "                self.toolbar_drag.as_ref().map(|d| d.id),\n                center_stack,", "                self.toolbar_drag.as_ref().map(|d| d.id),\n                self.win_size.1,\n                center_stack,")
print("t3 patched")
```

Poi aggiornare le chiamate a `frame(...)` nei test esistenti di `src/app/view/classic.rs` (tre: aggiungere `app.win_size.1,` prima di `center`). Se `.max_height` o `iced_aw` non risolvono, controllare i nomi con `cargo check` (l'API di container ha `max_height(impl Into<Pixels>)`).

- [ ] **Step 3: Eseguire i test**

Run: `python scratchpad/t3.py`, poi `cargo check --locked 2>&1 | grep -E "^(error|warning)" -A6 | head -30` → nessun errore.
Run: `cargo test --locked --lib toolbar 2>&1 | grep -E "FAILED|test result"` → PASS.
Run: `cargo test --locked --lib "view::classic" 2>&1 | grep -E "FAILED|test result"` → PASS, incluso `right_click_on_a_bar_opens_the_list_and_a_row_toggles_it`. Se `ui.click("Modify")` non trova la riga: l'overlay non si è aperto (controllare che il punto `(20,20)` cada su una barra: la barra Draw occupa la colonna a sinistra del frame) o che il simulatore richieda un secondo `simulate` per un frame di layout dell'overlay; adattare come fa `xref_manager.rs:2598`.

- [ ] **Step 4: Commit**

```bash
git add -A src
git commit -m "Barre classiche: elenco delle barre col tasto destro, spunta apre la barra flottante"
```

---

### Task 4: Clic prolungato per le varianti

**Files:**
- Modify: `src/ui/classic_toolbar.rs` (pulsanti con varianti, `flyout_overlay`, `FLYOUT_PREFIX`), `src/ui/toolbar_dock.rs` (`ToolbarMsg`), `src/app/update/toolbar.rs` (handler), `src/app/mod.rs` (`tool_hold`), `src/app/view/mod.rs` (overlay + sottoscrizione)

**Interfaces:**
- Consumes: `flyout`, `flyout_row`, `panel_style` (esistenti), `Ribbon::place_dropdown`, `PosReport::owned`.
- Produces: `ToolbarMsg::{HoldStart(&'static str), HoldCancel, HoldTick(iced::time::Instant), HoldEnd { tool_id: String, event: ModuleEvent }}`; `OpenCADStudio.tool_hold: Option<ToolHold>`; `classic_toolbar::{FLYOUT_PREFIX, flyout_overlay(&Ribbon, win_w) -> Option<Element>}`; `toolbar_dock::HOLD_MS`.
- Nota: `on_ribbon_tool_click` chiude già qualunque dropdown aperto (`dialog.rs:119`), quindi il clic su una riga del flyout lo chiude senza altro aggancio.

- [ ] **Step 1: Test che falliscono (handler)**

In `src/app/update/toolbar.rs`, modulo `tests`, aggiungere:

```rust
    /// A draw-bar button that has variants (e.g. CIRCLE).
    fn variant_tool() -> &'static str {
        use crate::ui::classic_toolbar::{items_for, ClassicItem};
        items_for(ToolbarId::Draw)
            .iter()
            .find_map(|i| match i {
                ClassicItem::Button(b) if !b.variants.is_empty() => Some(b.main.id),
                _ => None,
            })
            .expect("the Draw bar has a dropdown")
    }

    fn tick(app: &mut OpenCADStudio, ms: u64) {
        let now = iced::time::Instant::now() + std::time::Duration::from_millis(ms);
        toolbar(app, ToolbarMsg::HoldTick(now));
    }

    #[test]
    fn a_long_press_opens_the_variants_flyout() {
        let mut app = app();
        let tool = variant_tool();
        toolbar(&mut app, ToolbarMsg::HoldStart(tool));
        tick(&mut app, 100);
        assert!(app.ribbon.open_dropdown.is_none(), "too early");
        tick(&mut app, 600);
        assert_eq!(
            app.ribbon.open_dropdown.as_deref(),
            Some(format!("{}{tool}", crate::ui::classic_toolbar::FLYOUT_PREFIX).as_str())
        );
        assert!(crate::ui::classic_toolbar::flyout_overlay(&app.ribbon, 1600.0).is_some());
    }

    #[test]
    fn a_short_press_runs_the_command_and_opens_nothing() {
        // Review focus 5.
        let mut app = OpenCADStudio::new_for_test();
        app.toolbars = ToolbarLayout::default();
        let _ = app.run_command_line("NEW");
        let i = app.active_tab;
        let tool = variant_tool();
        let event = crate::modules::ModuleEvent::Command(tool.to_string());
        toolbar(&mut app, ToolbarMsg::HoldStart(tool));
        toolbar(
            &mut app,
            ToolbarMsg::HoldEnd { tool_id: tool.to_string(), event },
        );
        assert!(app.tool_hold.is_none());
        assert!(app.ribbon.open_dropdown.is_none());
        assert!(app.tabs[i].active_cmd.is_some(), "{tool} should have started");
    }

    #[test]
    fn a_held_press_does_not_run_the_command_on_release() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.run_command_line("NEW");
        let i = app.active_tab;
        let tool = variant_tool();
        toolbar(&mut app, ToolbarMsg::HoldStart(tool));
        tick(&mut app, 600);
        let event = crate::modules::ModuleEvent::Command(tool.to_string());
        toolbar(
            &mut app,
            ToolbarMsg::HoldEnd { tool_id: tool.to_string(), event },
        );
        assert!(app.tabs[i].active_cmd.is_none(), "the flyout opened instead");
        assert!(app.ribbon.open_dropdown.is_some(), "the flyout stays open");
    }

    #[test]
    fn leaving_the_button_cancels_the_hold() {
        let mut app = app();
        let tool = variant_tool();
        toolbar(&mut app, ToolbarMsg::HoldStart(tool));
        toolbar(&mut app, ToolbarMsg::HoldCancel);
        tick(&mut app, 600);
        assert!(app.tool_hold.is_none());
        assert!(app.ribbon.open_dropdown.is_none());
    }
```

Run: `cargo test --locked --lib update::toolbar 2>&1 | grep -E "^error" | sort | uniq -c | head` → errori (`HoldStart` ecc. inesistenti).

- [ ] **Step 2: Implementare (handler, stato, flyout)**

Script `scratchpad/t4.py`:

```python
import os
os.chdir("c:/Users/archi/Documents/Progetti IA/ArchLine")
def sub(p, old, new, count=1):
    s = open(p, encoding="utf-8", newline="").read()
    nl = "\r\n" if "\r\n" in s else "\n"
    old = old.replace("\n", nl); new = new.replace("\n", nl)
    assert s.count(old) == count, (p, old[:70], s.count(old))
    open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))

D = "src/ui/toolbar_dock.rs"
sub(D, "    /// Tick / untick a bar in the right-click list.\n    Toggle(ToolbarId),\n", """    /// Tick / untick a bar in the right-click list.
    Toggle(ToolbarId),
    /// A button with variants was pressed: start timing the hold.
    HoldStart(&'static str),
    /// The pointer left the button before the hold fired.
    HoldCancel,
    /// Time passed while a hold is pending.
    HoldTick(iced::time::Instant),
    /// The button was released: run it, unless the hold already opened the
    /// variants flyout.
    HoldEnd {
        tool_id: String,
        event: crate::modules::ModuleEvent,
    },
""")
sub(D, "pub const DRAG_THRESHOLD: f32 = 4.0;\n", "pub const DRAG_THRESHOLD: f32 = 4.0;\n/// How long a press must last to open the variants flyout (ms).\npub const HOLD_MS: u64 = 400;\n")

M = "src/app/mod.rs"
sub(M, "    pub(crate) toolbar_drag: Option<crate::ui::toolbar_dock::ToolbarDrag>,\n", """    pub(crate) toolbar_drag: Option<crate::ui::toolbar_dock::ToolbarDrag>,
    /// A press on a toolbar button that has variants, waiting to become a long
    /// press (flyout) or a click.
    pub(crate) tool_hold: Option<ToolHold>,
""")
sub(M, "            toolbar_drag: None,\n", "            toolbar_drag: None,\n            tool_hold: None,\n")

T = "src/app/update/toolbar.rs"
sub(T, "            ToolbarMsg::Toggle(id) => {", """            ToolbarMsg::HoldStart(tool) => {
                self.tool_hold = Some(crate::app::ToolHold {
                    tool,
                    pressed_at: iced::time::Instant::now(),
                    fired: false,
                });
            }
            ToolbarMsg::HoldCancel => {
                self.tool_hold = None;
            }
            ToolbarMsg::HoldTick(now) => {
                if let Some(h) = &mut self.tool_hold {
                    let held = now.saturating_duration_since(h.pressed_at);
                    if !h.fired && held >= std::time::Duration::from_millis(HOLD_MS) {
                        h.fired = true;
                        self.ribbon.open_dropdown = Some(format!(
                            "{}{}",
                            crate::ui::classic_toolbar::FLYOUT_PREFIX,
                            h.tool
                        ));
                    }
                }
            }
            ToolbarMsg::HoldEnd { tool_id, event } => {
                let opened_flyout = self.tool_hold.take().is_some_and(|h| h.fired);
                if !opened_flyout {
                    return self.update(Message::RibbonToolClick { tool_id, event });
                }
            }
            ToolbarMsg::Toggle(id) => {""")
sub(T, "    bar_length, bar_size, ToolbarDrag, ToolbarMsg, DRAG_THRESHOLD, GRIP_ANCHOR,\n};", "    bar_length, bar_size, ToolbarDrag, ToolbarMsg, DRAG_THRESHOLD, GRIP_ANCHOR, HOLD_MS,\n};")
print("t4 handler patched")
```

In `src/app/mod.rs`, accanto alla definizione di `OpenCADStudio` (o dove stanno gli altri tipi di stato `pub(crate)` dell'app), aggiungere:

```rust
/// A pending press on a toolbar button with variants.
#[derive(Clone, Debug)]
pub(crate) struct ToolHold {
    pub(crate) tool: &'static str,
    pub(crate) pressed_at: iced::time::Instant,
    pub(crate) fired: bool,
}
```

Poi in `src/ui/classic_toolbar.rs`:

1. Aggiungere agli `use`: `use iced::widget::{hover, mouse_area};` (e togliere `mouse_area` se già importato), `use crate::ui::wrap_bar::PosReport;`, `use crate::ui::ribbon::Ribbon;`, `use crate::ui::toolbar_dock::ToolbarMsg;`.
2. Costante e overlay:

```rust
/// Id prefix of the variants flyout in the ribbon's dropdown machinery.
pub const FLYOUT_PREFIX: &str = "cflyout:";

fn variants_of(tool_id: &str) -> Option<Vec<ToolDef>> {
    use crate::ui::toolbar_layout::ToolbarId;
    ToolbarId::all()
        .iter()
        .flat_map(|id| items_for(*id).iter())
        .find_map(|i| match i {
            ClassicItem::Button(b) if b.main.id == tool_id && !b.variants.is_empty() => {
                Some(b.variants.clone())
            }
            _ => None,
        })
}

/// The open variants flyout, anchored under its button (which reports its
/// bounds under the same id), if one is open.
pub fn flyout_overlay<'a>(ribbon: &'a Ribbon, win_w: f32) -> Option<Element<'a, Message>> {
    let open = ribbon.open_dropdown.as_deref()?;
    let tool_id = open.strip_prefix(FLYOUT_PREFIX)?;
    let variants = variants_of(tool_id)?;
    Some(ribbon.place_dropdown(open, flyout(&variants), FLYOUT_WIDTH, win_w))
}
```
3. Rimuovere i due `#[allow(dead_code)]` su `flyout_row` e `flyout`.
4. Riscrivere `tool_button` per i pulsanti con varianti. Sostituire l'intera funzione con:

```rust
fn tool_button(b: &ClassicButton) -> Element<'static, Message> {
    let inner: Element<'static, Message> = if b.variants.is_empty() {
        button(icon_el(&b.main.icon, ICON_SIZE))
            .on_press(click(&b.main))
            .width(Length::Fixed(BTN_SIZE))
            .height(Length::Fixed(BTN_SIZE))
            .style(|theme: &Theme, status| {
                let palette = theme.palette();
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: hovered.then_some(Background::Color(palette.background.strong.color)),
                    border: Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                    text_color: palette.background.base.text,
                    ..Default::default()
                }
            })
            .into()
    } else {
        // A button with variants is pressed and released as two events: a
        // short press runs it, a long press opens the flyout. An iced `button`
        // captures the press, so a `mouse_area` around it would never see it;
        // draw the hover highlight with `hover` instead.
        let plain = container(icon_el(&b.main.icon, ICON_SIZE))
            .width(Length::Fixed(BTN_SIZE))
            .height(Length::Fixed(BTN_SIZE))
            .center_x(Length::Fixed(BTN_SIZE))
            .center_y(Length::Fixed(BTN_SIZE));
        let lit = container(icon_el(&b.main.icon, ICON_SIZE))
            .width(Length::Fixed(BTN_SIZE))
            .height(Length::Fixed(BTN_SIZE))
            .center_x(Length::Fixed(BTN_SIZE))
            .center_y(Length::Fixed(BTN_SIZE))
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.strong.color)),
                border: Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
        mouse_area(PosReport::owned(
            format!("{FLYOUT_PREFIX}{}", b.main.id),
            hover(plain, lit),
        ))
        .on_press(Message::Toolbar(ToolbarMsg::HoldStart(b.main.id)))
        .on_release(Message::Toolbar(ToolbarMsg::HoldEnd {
            tool_id: b.main.id.to_string(),
            event: b.main.event.clone(),
        }))
        .on_exit(Message::Toolbar(ToolbarMsg::HoldCancel))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
    };
    tooltip(inner, tip(b.main.label, !b.variants.is_empty()), tooltip::Position::Bottom)
        .gap(4)
        .into()
}
```

Poi in `src/app/view/mod.rs`:
- nel `dropdown_layer`: sostituire il ramo `else { self.ribbon.dropdown_overlay(...).unwrap_or_else(|| Space...) }` così che, prima del fallback, si provi il flyout: aggiungere `.or_else(|| crate::ui::classic_toolbar::flyout_overlay(&self.ribbon, self.win_size.0))` subito dopo la chiamata a `.dropdown_overlay(...)` e prima di `.unwrap_or_else(...)`.
- nelle sottoscrizioni, accanto a `caret_blink`:

```rust
        // Time a press on a toolbar button with variants until it becomes a
        // long press (the flyout) — only while one is pending.
        let tool_hold_tick = if self.tool_hold.as_ref().is_some_and(|h| !h.fired) {
            iced::time::every(std::time::Duration::from_millis(50))
                .map(|now| Message::Toolbar(crate::ui::toolbar_dock::ToolbarMsg::HoldTick(now)))
        } else {
            Subscription::none()
        };
```
e includere `tool_hold_tick` nel `Subscription::batch([...])` finale (cercare dove viene usato `caret_blink` e aggiungerlo accanto).

- [ ] **Step 3: Test e controllo**

Run: `python scratchpad/t4.py`, applicare le modifiche manuali sopra, poi `cargo check --locked 2>&1 | grep -E "^(error|warning)" -A6 | head -30` → nessun errore (adattare i nomi iced se `center_x`/`hover` hanno firma diversa; `hover` è in `iced::widget::hover`).
Run: `cargo test --locked --lib update::toolbar 2>&1 | grep -E "FAILED|test result"` → PASS (i 4 nuovi test e i precedenti).
Run: `cargo test --locked --lib "view::classic" 2>&1 | grep -E "FAILED|test result"` → PASS.
Verifica: nel test `a_short_press_runs_the_command_and_opens_nothing` se `NEW` via `run_command_line` non crea un documento attivo in test, usare `app.automation_op(r#"{"op":"new"}"#)` come in `display.rs` tests (`fresh_app`).

- [ ] **Step 4: Test sul widget reale del clic tenuto**

In `src/app/view/classic.rs`, modulo `tests`, aggiungere:

```rust
    #[test]
    fn pressing_a_variant_button_publishes_hold_start_then_hold_end_with_the_real_widget() {
        use crate::ui::classic_toolbar::{items_for, ClassicItem};
        use crate::ui::toolbar_dock::ToolbarMsg;
        use crate::ui::toolbar_layout::ToolbarId;
        let tool = items_for(ToolbarId::Draw)
            .iter()
            .find_map(|i| match i {
                ClassicItem::Button(b) if !b.variants.is_empty() => Some(b),
                _ => None,
            })
            .expect("a Draw dropdown");
        let el = crate::ui::classic_toolbar::item_el(&ClassicItem::Button(crate::ui::classic_toolbar::ClassicButton {
            main: tool.main.clone(),
            variants: tool.variants.clone(),
        }), true);
        let mut ui = iced_test::simulator(el);
        let at = iced::Point::new(18.0, 18.0);
        ui.point_at(at);
        ui.simulate([
            iced_core::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
            iced_core::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)),
        ]);
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Toolbar(ToolbarMsg::HoldStart(id)) if *id == tool.main.id)),
            "press must publish HoldStart: {messages:?}"
        );
        assert!(
            messages.iter().any(|m| matches!(m, Message::Toolbar(ToolbarMsg::HoldEnd { .. }))),
            "release must publish HoldEnd"
        );
    }
```
(`ClassicButton` e `item_el` devono essere `pub`/`pub(crate)`: se non lo sono, renderli `pub(crate)`.)
Run: `cargo test --locked --lib pressing_a_variant_button 2>&1 | tail -6` → PASS. Se non pubblica nulla perché il simulatore non consegna press/release al `mouse_area`, controllare che il punto cada sul pulsante (36×36 a (0,0)).

- [ ] **Step 5: Commit**

```bash
git add -A src
git commit -m "Barre classiche: varianti col clic prolungato"
```

---

### Task 5: Spec allineata, suite completa e build

**Files:**
- Modify: `docs/superpowers/specs/2026-10-06-barre-elenco-visibilita-design.md`

- [ ] **Step 1: Allineare la spec a quanto realizzato**

- §3: `ToolbarId` resta un enum con una variante `Group(&'static str)` (anziché un newtype), per non toccare i ~100 usi esistenti; `all()` e `BUILTIN` al posto di `ALL`.
- §4: `set_visible(id, visible)` calcola da solo l'indice di cascata (nessun parametro `cascade_n`).
- §6: il pulsante con varianti usa `mouse_area` + `hover` (un `button` cattura la pressione, il `mouse_area` esterno non la vedrebbe); la chiusura del flyout al clic su una riga è già garantita da `on_ribbon_tool_click` (`dialog.rs:119`), nessun aggancio aggiuntivo; il rilascio è gestito da `HoldEnd`, il tempo da `HoldTick` (sottoscrizione 50 ms attiva solo durante la pressione).

- [ ] **Step 2: Suite completa**

Run: `cargo test --locked --lib > scratchpad/full.txt 2>&1; tail -5 scratchpad/full.txt; grep -E "^test .*FAILED" scratchpad/full.txt`
Expected: solo i due fallimenti già noti (`gdi_fallback_lands_ink_on_the_printed_page`; eventualmente `searchable_layer_coexists_with_outlines`, instabile in parallelo). Qualunque altro fallimento va indagato prima di proseguire.

- [ ] **Step 3: Build release e commit**

Run: `tasklist | grep -i opencadstudio` (deve essere vuoto, altrimenti chiedere all'utente di chiudere ArchLine), poi `cargo build --release --bin OpenCADStudio 2>&1 | tail -3` → `Finished`.

```bash
git add -A docs src
git commit -m "Barre classiche: spec allineata alla realizzazione"
```

- [ ] **Step 4: Verifica visiva (utente)**

Chiedere all'utente di avviare `target\release\OpenCADStudio.exe` e controllare, con screenshot:
1. Tasto destro su un'icona, su un'impugnatura e sul titolo di una barra flottante: si apre l'elenco con le spunte sulle barre visibili (le 6 storiche).
2. Spuntare una barra nuova (es. Dimensions): si apre flottante in cascata; spuntarne altre: restano in vista; togliere la spunta la nasconde.
3. Chiudere e riaprire ArchLine: le barre aperte sono ricordate.
4. Clic breve su un'icona con varianti (Cerchio): esegue il comando. Clic tenuto ~0,4 s: compare il flyout sotto l'icona, senza eseguire il comando; clic su una riga: esegue la variante e chiude il flyout.
5. Il flyout si apre bene anche da una barra verticale a destra e da una barra flottante.
6. L'elenco scorre con la rotella se è più alto della finestra.

---

## Self-review (fatta)

- **Copertura spec:** §3 registro → Task 2; §4 modello e persistenza → Task 1–2; §5 menu → Task 3; §6 clic prolungato → Task 4; §7 rischi → Step 4 di Task 5 e step di verifica nei task; §8 test → distribuiti.
- **Placeholder:** nessuno. Le istruzioni condizionali ("se `.max_height` non risolve…", "se il simulatore non consegna…") indicano il controllo e la correzione.
- **Coerenza dei tipi:** `ToolbarId::Group(&'static str)`, `all()`, `BUILTIN`, `is_builtin`, `title`, `Placement::Hidden { home }`, `is_visible`, `set_visible`, `entries()`, `display_name`, `group_items`, `buttons_of`, `BarEntry`, `menu_entries`, `ToolbarMsg::{Toggle, HoldStart, HoldCancel, HoldTick, HoldEnd}`, `ToolHold { tool, pressed_at, fired }`, `FLYOUT_PREFIX`, `flyout_overlay`, `frame(classic, layout, ribbon, dragging, win_h, center)` hanno la stessa firma ovunque compaiano.
- **Review Focus:** 1 → Task 1 (`hidden_survives_serde_and_sanitize`) e Task 2 (`sanitize_keeps_known_group_keys…`); 2 → Task 2 (`group_bars_are_hidden_by_default…`); 3 → Task 1 (`the_cascade_wraps…`); 4 → Task 1 (`hiding_a_docked_bar…`, `a_shown_bar_can_return…`); 5 → Task 4 (tre test dell'handler + test sul widget); 6 → Task 3 (widget reale).
