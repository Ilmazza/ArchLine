# Barra dei menu classica Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Aggiungere al workspace classico di ArchLine la barra dei menu di AutoCAD (File, Edit, View, Insert, Format, Tools, Draw, Dimension, Modify, Parametric, Window, Help), sopra le schede documento, con comandi verificati da test.

**Architecture:** Un albero di dati scritto a mano (`src/ui/classic_menu.rs`) descrive menu, sottomenu e righe; ogni riga lancia `Message::Command` (o un messaggio diretto) e un test controlla che ogni comando esista. La vista usa `iced_aw::MenuBar` con righe `button`, come la barra di stato. Il collegamento con lo stato dell'app sta in un file proprio (`src/app/view/classic.rs`) e `view_main` riceve solo due righe.

**Tech Stack:** Rust, iced (rev `23604ff2`), `iced_aw` (feature `menu`, già attiva), `rustc-hash`.

**Spec:** `docs/superpowers/specs/2026-10-05-barra-menu-classica-design.md` (approvata da Mauro il 2026-10-05).

**Scostamenti dalla spec (dettagli di implementazione, stesso comportamento):**
- `Action::Cmd` tiene una `String`, non `&'static str`, perché le voci derivate dal ribbon hanno il comando in una `String`.
- I gruppi del ribbon (Visual Style, Parametric) sono risolti quando l'albero viene costruito, con la funzione `group()`. Non esiste una voce `Dynamic(RibbonGroup)`: resta a runtime solo `Entry::Tabs`.
- Le icone e il test "il comando esiste" usano `registry::command_icon()` e `registry::ribbon_commands()`, che esistono già.

## Global Constraints

- Si lavora sul branch `lavoro`. Commit in italiano, messaggio breve, **senza attribuzioni**. **Niente push.** Non committare `.claude/`. Non toccare `main`.
- `view_main` riceve al massimo 2 righe (più 1 riga `mod classic;` in `view/mod.rs`): la logica sta in funzioni `#[inline(never)]` in file propri, perché `view_main` è enorme e `rustc` può andare in overflow di stack (trappola 1 del `CLAUDE.md`).
- `.cargo/config.toml`: `RUST_MIN_STACK` a 64 MiB. Non rimuoverlo.
- Le righe del menu sono `button` dentro `iced_aw::MenuBar`, come in `src/ui/statusbar/status_menu.rs`. Non usare lo schema del `ContextMenu` (`mouse_area`): lì serve perché l'overlay si ricostruisce a ogni `view`. Il comportamento a video non è verificato.
- Le etichette passano da `crate::t!(chiave)` **non letterale**. Nessuna modifica a `src/locale_catalog.rs` né ai file `locales/*/opencadstudio.ftl`. Lo script `scripts/test_locales.py` controlla solo le chiamate letterali.
- La barra compare solo con `crate::workspace::classic_active(is_start, clean_screen)`: mai sulla pagina iniziale, in clean screen, né con `ARCHLINE_WORKSPACE=ribbon`.
- Titoli, in quest'ordine esatto: File, Edit, View, Insert, Format, Tools, Draw, Dimension, Modify, Parametric, Window, Help.
- Nessun dato inventato: ogni comando nel menu deve esistere (test del Task 1).
- Test veloci: `cargo test --locked --lib classic_menu` e `cargo test --locked --lib view::classic`. Suite completa: `cargo test --locked --lib`. Il test `io::print_to_printer::…gdi_fallback_prints_landscape_sheets_landscape` è instabile in parallelo (passa da solo): non è una regressione, ma va segnalato se compare.
- Ogni ciclo `cargo test` ricompila: contare 1–3 minuti.
- Build: `cargo build --locked --bin OpenCADStudio`; l'exe è in `target\debug\OpenCADStudio.exe`. Il container non ha GPU: la verifica visiva la fa Mauro su Windows.

## Review Focus

Ingressi e condizioni che la spec implica ma che nessun test "felice" esercita, dal più probabile:

1. **Nessuna scorciatoia salvata** (profilo nuovo o tutte rimosse in CUI): le righe non mostrano scritte a destra e non c'è panico. → Task 2 (`no_binding_means_no_accelerator`) e Task 3 (`the_bar_builds_without_bindings_or_tabs`).
2. **Nomi di scheda lunghi o non ASCII** (per esempio "Pianta piano terra — àèìòù…"): la riga viene accorciata, senza panico. → Task 3 (`tab_names_are_elided`, `the_bar_builds_with_many_long_non_ascii_tabs`).
3. **Molte schede aperte** (60): il menu Window si costruisce e non va in panico. → Task 3 (stesso test; la raggiungibilità a video è nella checklist del Task 5).
4. **Etichetta senza voce nel catalogo** (lingua diversa dall'inglese): resta l'inglese, mai vuota. → Task 3 (`a_label_without_a_catalog_entry_stays_english`).
5. **Pagina iniziale e clean screen**: nessun menu. → Task 4 (`the_bar_is_hidden_on_start_and_clean_screen`).

## File Structure

| File | Cosa fa |
|---|---|
| `src/ui/classic_menu.rs` (nuovo) | Albero dei menu (dati), scorciatoie, vista `iced_aw`. Test nello stesso file. |
| `src/ui/mod.rs` (1 riga) | `pub mod classic_menu;` |
| `src/app/view/classic.rs` (nuovo) | Collega lo stato dell'app (schede, scorciatoie) alla barra. Test con `OpenCADStudio::new_for_test()`. |
| `src/app/view/mod.rs` (3 righe) | `mod classic;` e due righe in `view_main`, prima delle schede documento. |

Riuso, senza modifiche: `ui::classic_toolbar::strip_style`, `modules::registry::{all_modules, ribbon_commands, command_icon}`, `command::all_registered_command_names`, `ui::icons::semantic`, `ui::text_util::elide`.

---

### Task 1: Albero dei menu e suoi controlli

**Files:**
- Create: `src/ui/classic_menu.rs`
- Modify: `src/ui/mod.rs:15-16` (aggiungere `pub mod classic_menu;` tra `classic_layers` e `classic_toolbar`)
- Test: nello stesso `src/ui/classic_menu.rs`, modulo `tests`

**Interfaces:**
- Consumes: `registry::all_modules()`, `registry::ribbon_commands()`, `crate::command::all_registered_command_names()`, `Message::{CommandHistoryToggle, DocTabCloseAll}`.
- Produces (usati dai Task 2–4):
  - `pub enum Action { Cmd(String), Msg(fn() -> Message) }` (`Clone`)
  - `pub enum Entry { Item { label: &'static str, action: Action, accel: Option<&'static str> }, Sub { label: &'static str, entries: Vec<Entry> }, Sep, Tabs }`
  - `pub struct MenuDef { pub title: &'static str, pub entries: Vec<Entry> }`
  - `pub fn menus() -> &'static [MenuDef]`
  - `fn group(module: &str, group: &str) -> Vec<Entry>`

- [ ] **Step 1: Creare il file con i tipi, un albero vuoto e i test**

Aggiungere in `src/ui/mod.rs`, dopo `pub mod classic_layers;`:

```rust
pub mod classic_menu;
```

Creare `src/ui/classic_menu.rs`:

```rust
//! Menu bar of the AutoCAD-classic workspace (File, Edit, View, …, Help).
//!
//! The tree below is data. Every leaf names a command (the string the command
//! line takes) or a direct message, and a test checks that each command
//! exists, so a renamed command fails the build instead of leaving a dead row.
//! Rows publish `Message::Command`, the path the shortcuts and the command
//! line already use.

use std::sync::OnceLock;

use crate::app::Message;
use crate::modules::{registry, CadModule, ModuleEvent, RibbonItem, ToolDef};

/// What a menu row does when clicked.
#[derive(Clone)]
pub enum Action {
    /// `Message::Command(..)`: the same as typing the command.
    Cmd(String),
    /// A message that is not a typeable command (F2 history, Close All).
    Msg(fn() -> Message),
}

/// One row of a menu.
pub enum Entry {
    Item {
        /// English label; also the translation key.
        label: &'static str,
        action: Action,
        /// Shortcut-table action whose key is shown on the right, when it is
        /// not the command itself.
        accel: Option<&'static str>,
    },
    Sub {
        label: &'static str,
        entries: Vec<Entry>,
    },
    Sep,
    /// The open document tabs (Window menu), built from app state at draw time.
    Tabs,
}

pub struct MenuDef {
    pub title: &'static str,
    pub entries: Vec<Entry>,
}

fn cmd(label: &'static str, command: &str) -> Entry {
    Entry::Item {
        label,
        action: Action::Cmd(command.to_string()),
        accel: None,
    }
}

fn msg(label: &'static str, make: fn() -> Message, accel: &'static str) -> Entry {
    Entry::Item {
        label,
        action: Action::Msg(make),
        accel: Some(accel),
    }
}

fn sub(label: &'static str, entries: Vec<Entry>) -> Entry {
    Entry::Sub { label, entries }
}

/// Rows taken from a ribbon group, in ribbon order. Dropdowns are spread into
/// their items; only tools that run a command are kept.
fn group(_module: &str, _group: &str) -> Vec<Entry> {
    Vec::new()
}

fn build() -> Vec<MenuDef> {
    Vec::new()
}

/// The menus, built once.
pub fn menus() -> &'static [MenuDef] {
    static MENUS: OnceLock<Vec<MenuDef>> = OnceLock::new();
    MENUS.get_or_init(build)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn leaves<'a>(entries: &'a [Entry], out: &mut Vec<&'a Action>) {
        for e in entries {
            match e {
                Entry::Item { action, .. } => out.push(action),
                Entry::Sub { entries, .. } => leaves(entries, out),
                Entry::Sep | Entry::Tabs => {}
            }
        }
    }

    fn commands(menu: &MenuDef) -> Vec<String> {
        let mut actions = Vec::new();
        leaves(&menu.entries, &mut actions);
        actions
            .into_iter()
            .filter_map(|a| match a {
                Action::Cmd(c) => Some(c.clone()),
                Action::Msg(_) => None,
            })
            .collect()
    }

    /// First word of a command line, upper case: `ZOOM WINDOW` → `ZOOM`.
    fn head(command: &str) -> String {
        command
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_uppercase()
    }

    fn check_shape(entries: &[Entry], at: &str) {
        assert!(!entries.is_empty(), "{at} is empty");
        assert!(
            !matches!(entries.first(), Some(Entry::Sep)),
            "{at} starts with a separator"
        );
        assert!(
            !matches!(entries.last(), Some(Entry::Sep)),
            "{at} ends with a separator"
        );
        for pair in entries.windows(2) {
            assert!(
                !(matches!(pair[0], Entry::Sep) && matches!(pair[1], Entry::Sep)),
                "{at} has two separators in a row"
            );
        }
        for e in entries {
            match e {
                Entry::Item { label, .. } => assert!(!label.is_empty(), "{at} has an empty label"),
                Entry::Sub { label, entries } => {
                    assert!(!label.is_empty(), "{at} has an empty submenu label");
                    check_shape(entries, label);
                }
                Entry::Sep | Entry::Tabs => {}
            }
        }
    }

    #[test]
    fn titles_follow_the_autocad_order() {
        let titles: Vec<_> = menus().iter().map(|m| m.title).collect();
        assert_eq!(
            titles,
            [
                "File", "Edit", "View", "Insert", "Format", "Tools", "Draw", "Dimension",
                "Modify", "Parametric", "Window", "Help"
            ]
        );
    }

    #[test]
    fn no_menu_is_empty_and_separators_only_sit_between_rows() {
        for m in menus() {
            check_shape(&m.entries, m.title);
        }
    }

    #[test]
    fn no_command_repeats_inside_one_menu() {
        for m in menus() {
            let cmds = commands(m);
            let unique: HashSet<_> = cmds.iter().collect();
            assert_eq!(unique.len(), cmds.len(), "{} repeats a command: {cmds:?}", m.title);
        }
    }

    #[test]
    fn every_command_exists() {
        let registered: HashSet<String> = crate::command::all_registered_command_names()
            .into_iter()
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let ribbon = registry::ribbon_commands();
        let ribbon_heads: HashSet<String> = ribbon.keys().map(|k| head(k)).collect();
        let mut missing = Vec::new();
        for m in menus() {
            for c in commands(m) {
                let known = ribbon.contains_key(&c.to_ascii_uppercase())
                    || registered.contains(&head(&c))
                    || ribbon_heads.contains(&head(&c));
                if !known {
                    missing.push(format!("{}: {c}", m.title));
                }
            }
        }
        assert!(missing.is_empty(), "commands with no source: {missing:#?}");
    }

    #[test]
    fn derived_groups_resolve_to_commands() {
        for (module, name) in [
            ("view", "Visual Style"),
            ("parametric", "Geometric"),
            ("parametric", "Dimensional"),
            ("parametric", "Manage"),
        ] {
            assert!(!group(module, name).is_empty(), "{module}/{name} is empty");
        }
        let styles = group("view", "Visual Style");
        assert!(styles.iter().all(|e| matches!(
            e,
            Entry::Item { action: Action::Cmd(c), .. } if c.starts_with("VISUALSTYLES ")
        )));
    }
}
```

- [ ] **Step 2: Eseguire i test e vedere il fallimento**

Run: `cargo test --locked --lib classic_menu`
Expected: **FAIL** per `titles_follow_the_autocad_order` e `derived_groups_resolve_to_commands`. Gli altri tre passano a vuoto perché l'albero è ancora vuoto: è normale.

- [ ] **Step 3: Implementare `group()` e l'albero**

Sostituire la `group()` provvisoria con:

```rust
fn push_tool(out: &mut Vec<Entry>, t: &ToolDef) {
    if let ModuleEvent::Command(command) = &t.event {
        out.push(cmd(t.label, command));
    }
}

/// Rows taken from a ribbon group, in ribbon order. Dropdowns are spread into
/// their items; only tools that run a command are kept.
fn group(module: &str, group: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    for m in registry::all_modules() {
        if m.id() != module {
            continue;
        }
        for g in m.ribbon_groups() {
            if g.title != group {
                continue;
            }
            for item in &g.tools {
                match item {
                    RibbonItem::Tool(t) | RibbonItem::LabeledTool(t) | RibbonItem::LargeTool(t) => {
                        push_tool(&mut out, t)
                    }
                    RibbonItem::Dropdown { items, .. }
                    | RibbonItem::LabeledDropdown { items, .. }
                    | RibbonItem::LargeDropdown { items, .. } => {
                        for (command, label, _) in items {
                            out.push(cmd(label, command));
                        }
                    }
                    RibbonItem::ToolGrid { columns } => {
                        for t in columns.iter().flatten() {
                            push_tool(&mut out, t);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}
```

e la `build()` provvisoria con:

```rust
fn build() -> Vec<MenuDef> {
    use Entry::Sep;
    vec![
        MenuDef {
            title: "File",
            entries: vec![
                cmd("New", "NEW"),
                cmd("Open", "OPEN"),
                cmd("Save", "SAVE"),
                cmd("Save As", "SAVEAS"),
                cmd("Plot", "PLOT"),
                cmd("Close", "CLOSE"),
                cmd("Exit", "QUIT"),
            ],
        },
        MenuDef {
            title: "Edit",
            entries: vec![
                cmd("Undo", "UNDO"),
                cmd("Redo", "REDO"),
                Sep,
                cmd("Cut", "CUTCLIP"),
                cmd("Copy", "COPYCLIP"),
                cmd("Copy with Base Point", "COPYBASE"),
                cmd("Paste", "PASTECLIP"),
                cmd("Paste as Block", "PASTEBLOCK"),
                Sep,
                cmd("Select All", "SELECTALL"),
                cmd("Find", "FIND"),
            ],
        },
        MenuDef {
            title: "View",
            entries: vec![
                cmd("Regen", "REGEN"),
                cmd("Regen All", "REGENALL"),
                Sep,
                sub(
                    "Zoom",
                    vec![
                        cmd("Window", "ZOOM WINDOW"),
                        cmd("Previous", "ZOOM PREVIOUS"),
                        cmd("Extents", "ZOOM EXTENTS"),
                        cmd("All", "ZOOM ALL"),
                    ],
                ),
                cmd("Pan", "PAN"),
                cmd("3D Orbit", "3DORBIT"),
                sub("Visual Styles", group("view", "Visual Style")),
                Sep,
                cmd("Clean Screen", "CLEANSCREEN"),
                cmd("Properties", "PROPERTIES"),
                msg("Command History", || Message::CommandHistoryToggle, "COMMANDHISTORY"),
            ],
        },
        MenuDef {
            title: "Insert",
            entries: vec![
                cmd("Block", "BLOCK"),
                cmd("Insert", "INSERT"),
                Sep,
                cmd("Xref", "XATTACH"),
                cmd("Image", "IMAGEATTACH"),
                cmd("PDF", "PDFATTACH"),
                Sep,
                cmd("Field", "FIELD"),
            ],
        },
        MenuDef {
            title: "Format",
            entries: vec![
                cmd("Layer", "LAYERS"),
                cmd("Color", "COLOR"),
                cmd("Linetype", "LINETYPE"),
                Sep,
                cmd("Text Style", "STYLE"),
                cmd("Dimension Style", "DIMSTYLE"),
                Sep,
                cmd("Units", "UNITS"),
                cmd("Limits", "LIMITS"),
            ],
        },
        MenuDef {
            title: "Tools",
            entries: vec![
                sub(
                    "Inquiry",
                    vec![
                        cmd("Distance", "DIST"),
                        cmd("Area", "AREA"),
                        cmd("ID", "ID"),
                        cmd("List", "LIST"),
                    ],
                ),
                Sep,
                cmd("UCS", "UCS"),
                cmd("Drafting Settings", "DSETTINGS"),
                Sep,
                cmd("Aliases", "ALIASEDIT"),
                cmd("Shortcuts", "SHORTCUTS"),
                cmd("Options", "OPTIONS"),
                cmd("Plugins", "PLUGINS"),
            ],
        },
        MenuDef {
            title: "Draw",
            entries: vec![
                cmd("Line", "LINE"),
                cmd("Construction Line", "XLINE"),
                cmd("Polyline", "PLINE"),
                cmd("Polygon", "POLYGON"),
                cmd("Rectangle", "RECTANG"),
                cmd("Arc", "ARC"),
                cmd("Circle", "CIRCLE"),
                cmd("Donut", "DONUT"),
                cmd("Spline", "SPLINE"),
                cmd("Ellipse", "ELLIPSE"),
                Sep,
                cmd("Point", "POINT"),
                cmd("Hatch", "HATCH"),
                cmd("Region", "REGION"),
                Sep,
                sub(
                    "Text",
                    vec![cmd("Single Line Text", "TEXT"), cmd("Multiline Text", "MTEXT")],
                ),
            ],
        },
        MenuDef {
            title: "Dimension",
            entries: vec![
                cmd("Linear", "DIMLINEAR"),
                cmd("Aligned", "DIMALIGNED"),
                cmd("Arc Length", "DIMARC"),
                cmd("Ordinate", "DIMORDINATE"),
                cmd("Radius", "DIMRADIUS"),
                cmd("Diameter", "DIMDIAMETER"),
                cmd("Angular", "DIMANGULAR"),
                Sep,
                cmd("Baseline", "DIMBASELINE"),
                cmd("Continue", "DIMCONTINUE"),
                cmd("Quick Dimension", "QDIM"),
                Sep,
                cmd("Leader", "LEADER"),
                cmd("Multileader", "MLEADER"),
                cmd("Tolerance", "TOLERANCE"),
                Sep,
                cmd("Dimension Style", "DIMSTYLE"),
            ],
        },
        MenuDef {
            title: "Modify",
            entries: vec![
                cmd("Erase", "ERASE"),
                cmd("Copy", "COPY"),
                cmd("Mirror", "MIRROR"),
                cmd("Offset", "OFFSET"),
                cmd("Array", "ARRAY"),
                Sep,
                cmd("Move", "MOVE"),
                cmd("Rotate", "ROTATE"),
                cmd("Scale", "SCALE"),
                cmd("Stretch", "STRETCH"),
                Sep,
                cmd("Trim", "TRIM"),
                cmd("Extend", "EXTEND"),
                cmd("Break", "BREAK"),
                cmd("Chamfer", "CHAMFER"),
                cmd("Fillet", "FILLET"),
                Sep,
                cmd("Explode", "EXPLODE"),
                cmd("Polyline Edit", "PEDIT"),
            ],
        },
        MenuDef {
            title: "Parametric",
            entries: vec![
                sub("Geometric", group("parametric", "Geometric")),
                sub("Dimensional", group("parametric", "Dimensional")),
                sub("Manage", group("parametric", "Manage")),
            ],
        },
        MenuDef {
            title: "Window",
            entries: vec![
                Entry::Tabs,
                Sep,
                msg("Close All", || Message::DocTabCloseAll, "DOCTABCLOSEALL"),
            ],
        },
        MenuDef {
            title: "Help",
            entries: vec![cmd("Help", "HELP"), cmd("About", "ABOUT")],
        },
    ]
}
```

(`DOCTABCLOSEALL` non è nella tabella delle scorciatoie: serve solo come nome di azione, e `accel_for` restituirà `None`.)

- [ ] **Step 4: Eseguire i test**

Run: `cargo test --locked --lib classic_menu`
Expected: **PASS** (5 test). Se `every_command_exists` fallisce, l'errore elenca `Menu: COMANDO` per ciascun comando senza fonte. Per ognuno:
1. controllare il refuso nell'albero;
2. se il dispatcher lo gestisce davvero (cercare `"COMANDO"` in `src/app/commands/`) ma non è né nel ribbon né in `inventory`, aggiungerlo a questa lista nel modulo `tests` e far saltare il controllo `known` solo per quegli id, **con** il test seguente che prova che il literal compare nel dispatcher:

```rust
    /// Commands the dispatcher in `src/app/commands/` handles that are neither
    /// ribbon tools nor registered with `inventory`.
    const DISPATCH_ONLY: &[&str] = &["COMANDO_TROVATO"];

    #[test]
    fn dispatch_only_commands_appear_in_the_dispatcher() {
        const DISPATCHER: &str = concat!(
            include_str!("../app/commands/fileops.rs"),
            include_str!("../app/commands/display.rs"),
            include_str!("../app/commands/blocks.rs"),
            include_str!("../app/commands/view.rs"),
            include_str!("../app/commands/styleprops.rs"),
            include_str!("../app/commands/draw.rs"),
            include_str!("../app/commands/mod.rs"),
        );
        for c in DISPATCH_ONLY {
            assert!(DISPATCHER.contains(&format!("\"{c}\"")), "{c} not in the dispatcher");
        }
    }
```

   (sostituire `COMANDO_TROVATO` con gli id reali e aggiungere `|| DISPATCH_ONLY.contains(&head(&c).as_str())` alla condizione `known`);
3. se il comando non esiste in nessun posto, **toglierlo dal menu** (regola della spec: le voci non mappabili sono escluse, non disabilitate) e annotarlo nel messaggio di commit.

- [ ] **Step 5: Commit**

```bash
git add src/ui/mod.rs src/ui/classic_menu.rs
git commit -m "Menu classico: albero delle voci e test sui comandi"
```

---

### Task 2: Scorciatoie mostrate nel menu

**Files:**
- Modify: `src/ui/classic_menu.rs` (aggiunte sopra il modulo `tests`, test dentro `tests`)

**Interfaces:**
- Consumes: niente dai Task precedenti.
- Produces (usati dal Task 3):
  - `pub fn format_accel(key: &str) -> String`
  - `pub fn accel_for(bindings: &rustc_hash::FxHashMap<String, String>, action: &str) -> Option<String>`

- [ ] **Step 1: Scrivere i test che falliscono**

Aggiungere nel modulo `tests` di `src/ui/classic_menu.rs`:

```rust
    use rustc_hash::FxHashMap;

    fn bindings(pairs: &[(&str, &str)]) -> FxHashMap<String, String> {
        pairs
            .iter()
            .map(|(k, a)| (k.to_string(), a.to_string()))
            .collect()
    }

    #[test]
    fn accelerators_are_written_like_a_menu() {
        assert_eq!(format_accel("CTRL+SHIFT+S"), "Ctrl+Shift+S");
        assert_eq!(format_accel("CTRL+O"), "Ctrl+O");
        assert_eq!(format_accel("F8"), "F8");
        assert_eq!(format_accel("F12"), "F12");
        assert_eq!(format_accel("DELETE"), "Delete");
    }

    #[test]
    fn the_shortest_key_wins_and_ties_are_alphabetical() {
        let b = bindings(&[("CTRL+SHIFT+Z", "REDO"), ("CTRL+Y", "REDO"), ("CTRL+Z", "UNDO")]);
        assert_eq!(accel_for(&b, "REDO").as_deref(), Some("Ctrl+Y"));
        assert_eq!(accel_for(&b, "UNDO").as_deref(), Some("Ctrl+Z"));
        let tie = bindings(&[("CTRL+H", "FIND"), ("CTRL+F", "FIND")]);
        assert_eq!(accel_for(&tie, "FIND").as_deref(), Some("Ctrl+F"));
    }

    #[test]
    fn no_binding_means_no_accelerator() {
        assert_eq!(accel_for(&FxHashMap::default(), "SAVE"), None);
        let b = bindings(&[("CTRL+S", "SAVE")]);
        assert_eq!(accel_for(&b, "SAVEAS"), None);
        assert_eq!(accel_for(&b, "DOCTABCLOSEALL"), None);
    }

    #[test]
    fn the_action_name_is_matched_without_regard_to_case() {
        let b = bindings(&[("CTRL+S", "save")]);
        assert_eq!(accel_for(&b, "SAVE").as_deref(), Some("Ctrl+S"));
    }
```

- [ ] **Step 2: Eseguire i test e vedere il fallimento**

Run: `cargo test --locked --lib classic_menu`
Expected: **errore di compilazione** `cannot find function format_accel` / `accel_for`.

- [ ] **Step 3: Implementare**

Aggiungere in `src/ui/classic_menu.rs`, sopra il modulo `tests`:

```rust
use rustc_hash::FxHashMap;

/// `CTRL+SHIFT+S` → `Ctrl+Shift+S`. Single keys and function keys stay as
/// written (`S`, `F8`).
pub fn format_accel(key: &str) -> String {
    key.split('+')
        .map(|part| {
            let is_function_key = part.len() > 1
                && part.starts_with('F')
                && part[1..].chars().all(|c| c.is_ascii_digit());
            if part.chars().count() == 1 || is_function_key {
                return part.to_string();
            }
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// The shortcut shown for `action`: the shortest key bound to it, ties broken
/// alphabetically (so Redo shows `Ctrl+Y`, not `Ctrl+Shift+Z`).
pub fn accel_for(bindings: &FxHashMap<String, String>, action: &str) -> Option<String> {
    bindings
        .iter()
        .filter(|(_, bound)| bound.eq_ignore_ascii_case(action))
        .map(|(key, _)| key)
        .min_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)))
        .map(|key| format_accel(key))
}
```

(`use rustc_hash::FxHashMap;` va con gli altri `use` in cima al file. L'`use` dentro `tests` può restare: un'importazione esplicita prevale su quella portata da `use super::*`.)

- [ ] **Step 4: Eseguire i test**

Run: `cargo test --locked --lib classic_menu`
Expected: **PASS** (9 test).

- [ ] **Step 5: Commit**

```bash
git add src/ui/classic_menu.rs
git commit -m "Menu classico: scorciatoie lette dalla tabella corrente"
```

---

### Task 3: La barra con `iced_aw`

**Files:**
- Modify: `src/ui/classic_menu.rs` (vista sopra il modulo `tests`, test in un nuovo modulo `build_tests`)

**Interfaces:**
- Consumes: `menus()`, `Entry`, `Action`, `accel_for` (Task 1–2); `ui::classic_toolbar::strip_style` (`pub(super)`, visibile in `ui`); `registry::command_icon(&str) -> Option<&'static [u8]>`; `ui::icons::semantic(bytes, size)`; `ui::text_util::elide(&str, usize)`.
- Produces (usati dal Task 4):
  - `pub struct TabEntry { pub index: usize, pub name: String, pub active: bool }`
  - `pub struct MenuCtx<'a> { pub bindings: &'a FxHashMap<String, String>, pub tabs: Vec<TabEntry> }`
  - `pub fn menu_bar(ctx: &MenuCtx<'_>) -> Element<'static, Message>`

- [ ] **Step 1: Scrivere i test che falliscono**

Aggiungere in fondo a `src/ui/classic_menu.rs`:

```rust
#[cfg(test)]
mod build_tests {
    use super::*;

    fn tabs(n: usize, name: &str) -> Vec<TabEntry> {
        (0..n)
            .map(|index| TabEntry {
                index,
                name: format!("{name} {index}"),
                active: index == 0,
            })
            .collect()
    }

    /// The bar is a full-width strip. Reading its size asserts something real
    /// and proves the whole widget tree was built without panicking.
    fn spans_the_window(el: &Element<'static, Message>) -> bool {
        use iced::advanced::Widget;
        el.as_widget().size().width == Length::Fill
    }

    #[test]
    fn the_bar_builds_without_bindings_or_tabs() {
        let bindings = FxHashMap::default();
        let bar = menu_bar(&MenuCtx {
            bindings: &bindings,
            tabs: Vec::new(),
        });
        assert!(spans_the_window(&bar));
    }

    #[test]
    fn the_bar_builds_with_many_long_non_ascii_tabs() {
        let bindings = FxHashMap::default();
        let bar = menu_bar(&MenuCtx {
            bindings: &bindings,
            tabs: tabs(60, "Pianta piano terra — lungo nome àèìòù con molte parole"),
        });
        assert!(spans_the_window(&bar));
    }

    #[test]
    fn tab_names_are_elided() {
        let long = "x".repeat(200);
        assert!(tab_label(&long).chars().count() <= TAB_NAME_MAX + 1);
        assert_eq!(tab_label("Drawing1.dwg"), "Drawing1.dwg");
    }

    #[test]
    fn a_label_without_a_catalog_entry_stays_english() {
        let key: &str = "Parametric";
        assert_eq!(label_text(key), "Parametric");
        assert!(!label_text("Visual\nStyle").contains('\n'));
    }
}
```

- [ ] **Step 2: Eseguire i test e vedere il fallimento**

Run: `cargo test --locked --lib classic_menu`
Expected: **errore di compilazione** `cannot find function menu_bar` / `tab_label` / `label_text`.

- [ ] **Step 3: Implementare la vista**

Aggiungere in `src/ui/classic_menu.rs`, sopra il modulo `tests` (e gli `use` in cima al file):

```rust
use iced::widget::{button, container, row, text, Space};
use iced::{Background, Border, Color, Element, Length, Shadow, Theme};
use iced_aw::menu::{DrawPath, Item, Menu, MenuBar};

type BarItem = Item<'static, Message, Theme, iced::Renderer>;
type BarMenu = Menu<'static, Message, Theme, iced::Renderer>;

const MENU_W: f32 = 270.0;
const ICON: f32 = 16.0;
const TAB_NAME_MAX: usize = 40;

/// One open document tab, for the Window menu.
pub struct TabEntry {
    pub index: usize,
    pub name: String,
    pub active: bool,
}

/// App state the menu needs, so this file does not depend on the app type.
pub struct MenuCtx<'a> {
    pub bindings: &'a FxHashMap<String, String>,
    pub tabs: Vec<TabEntry>,
}

/// Translated label; the English key itself when the catalog has no entry.
/// Line breaks of ribbon labels become spaces.
fn label_text(key: &str) -> String {
    crate::t!(key).replace('\n', " ")
}

fn tab_label(name: &str) -> String {
    crate::ui::text_util::elide(name, TAB_NAME_MAX).to_string()
}

fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.6)),
    }
}

fn lead(icon: Option<&'static [u8]>) -> Element<'static, Message> {
    match icon {
        Some(bytes) => crate::ui::icons::semantic(bytes, ICON),
        None => Space::new().width(ICON).height(ICON).into(),
    }
}

fn row_content(
    lead: Element<'static, Message>,
    label: String,
    accel: Option<String>,
    arrow: bool,
) -> Element<'static, Message> {
    let mut r = row![lead, text(label).size(12), Space::new().width(Length::Fill)]
        .spacing(8)
        .align_y(iced::Center);
    if let Some(accel) = accel {
        r = r.push(text(accel).size(11).style(muted));
    }
    if arrow {
        r = r.push(text("▸").size(12));
    }
    container(r).padding([4, 10]).width(Length::Fill).into()
}

fn row_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then_some(Background::Color(palette.primary.weak.color)),
        text_color: if hovered {
            palette.primary.weak.text
        } else {
            palette.background.base.text
        },
        border: Border {
            radius: 2.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn row_button(content: Element<'static, Message>, message: Message) -> Element<'static, Message> {
    button(content)
        .on_press(message)
        .width(Length::Fill)
        .padding(0)
        .style(row_style)
        .into()
}

fn separator_row() -> Element<'static, Message> {
    let line = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.strong.color)),
            ..Default::default()
        });
    container(line).padding([3, 6]).width(Length::Fill).into()
}

fn click(action: &Action) -> Message {
    match action {
        Action::Cmd(command) => Message::Command(command.clone()),
        Action::Msg(make) => make(),
    }
}

fn entry_items(entries: &[Entry], ctx: &MenuCtx<'_>) -> Vec<BarItem> {
    let mut out: Vec<BarItem> = Vec::new();
    for e in entries {
        match e {
            Entry::Item {
                label,
                action,
                accel,
            } => {
                let key: Option<&str> = (*accel).or(match action {
                    Action::Cmd(c) => Some(c.as_str()),
                    Action::Msg(_) => None,
                });
                let accel_text = key.and_then(|k| accel_for(ctx.bindings, k));
                let icon = match action {
                    Action::Cmd(c) => registry::command_icon(c),
                    Action::Msg(_) => None,
                };
                let content = row_content(lead(icon), label_text(label), accel_text, false);
                out.push(Item::new(row_button(content, click(action))).close_on_click(true));
            }
            Entry::Sub { label, entries } => {
                let header = row_button(
                    row_content(lead(None), label_text(label), None, true),
                    Message::Noop,
                );
                let menu: BarMenu = Menu::new(entry_items(entries, ctx))
                    .width(Length::Fixed(MENU_W))
                    .padding(2)
                    .spacing(0)
                    .offset(0.0);
                out.push(Item::with_menu(header, menu).close_on_click(false));
            }
            Entry::Sep => out.push(Item::new(separator_row())),
            Entry::Tabs => {
                for tab in &ctx.tabs {
                    let mark: Element<'static, Message> = if tab.active {
                        text("✓").size(12).width(ICON).into()
                    } else {
                        lead(None)
                    };
                    let content = row_content(mark, tab_label(&tab.name), None, false);
                    out.push(
                        Item::new(row_button(content, Message::TabSwitch(tab.index)))
                            .close_on_click(true),
                    );
                }
            }
        }
    }
    out
}

fn root_button(title: &'static str) -> Element<'static, Message> {
    button(text(label_text(title)).size(12))
        .on_press(Message::Noop)
        .padding([3, 10])
        .style(row_style)
        .into()
}

/// The menu bar, to sit above the document tabs.
///
/// Kept out of `view_main` on purpose (see `classic_toolbar::top_bar`).
#[inline(never)]
pub fn menu_bar(ctx: &MenuCtx<'_>) -> Element<'static, Message> {
    let roots: Vec<BarItem> = menus()
        .iter()
        .map(|m| {
            let menu: BarMenu = Menu::new(entry_items(&m.entries, ctx))
                .width(Length::Fixed(MENU_W))
                .padding(2)
                .spacing(0)
                .offset(1.0)
                .close_on_background_click(true);
            Item::with_menu(root_button(m.title), menu)
        })
        .collect();
    let bar = MenuBar::new(roots)
        .spacing(0)
        .draw_path(DrawPath::Backdrop)
        .close_on_background_click_global(true)
        .style(|theme: &Theme, _| {
            let palette = theme.palette();
            iced_aw::style::menu_bar::Style {
                bar_background: Background::Color(Color::TRANSPARENT),
                bar_border: Border::default(),
                bar_shadow: Shadow::default(),
                menu_background: Background::Color(palette.background.weakest.color),
                menu_border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                menu_shadow: Shadow {
                    color: palette.background.strongest.color.scale_alpha(0.35),
                    offset: iced::Vector::new(0.0, 2.0),
                    blur_radius: 6.0,
                },
                path: Background::Color(palette.primary.weak.color),
                path_border: Border {
                    color: palette.primary.base.color,
                    width: 1.0,
                    radius: 2.0.into(),
                },
            }
        });
    container(bar)
        .width(Length::Fill)
        .style(super::classic_toolbar::strip_style)
        .into()
}
```

Note per chi implementa: i tipi e i metodi qui sopra sono ricavati da `status_menu.rs` e dal sorgente di `iced_aw`. Se il compilatore protesta su un dettaglio (per esempio `Space::new().width(..)`, il nome del tipo `Renderer`, o un campo di `button::Style`), copiare la forma usata dal codice vicino: `classic_toolbar.rs` per `Space`, `button::Style` e `container::Style`; `status_menu.rs` per `MenuBar` e `Menu`. Non cambiare l'architettura.

- [ ] **Step 4: Eseguire i test**

Run: `cargo test --locked --lib classic_menu`
Expected: **PASS** (13 test).

- [ ] **Step 5: Commit**

```bash
git add src/ui/classic_menu.rs
git commit -m "Menu classico: barra iced_aw con icone, scorciatoie e schede"
```

---

### Task 4: Aggancio in `view_main`

**Files:**
- Create: `src/app/view/classic.rs`
- Modify: `src/app/view/mod.rs` (riga `mod classic;` prima di `mod controls;` a riga 20; due righe in `view_main` dopo `mark("ribbon");` e prima di `if self.show_file_tabs {`, intorno a riga 2181)
- Test: nello stesso `src/app/view/classic.rs`

**Interfaces:**
- Consumes: `TabEntry`, `MenuCtx`, `menu_bar` (Task 3); `OpenCADStudio::{tabs, active_tab, shortcut_bindings}` (campi privati del modulo `app`, visibili ai discendenti); `DocumentTab::tab_display_name()` (`pub(super)` in `app::document`); `OpenCADStudio::new_for_test()` (`pub(crate)`).
- Produces: `OpenCADStudio::classic_menu_tabs(&self) -> Vec<TabEntry>` e `OpenCADStudio::classic_menu_bar(&self) -> Element<'static, Message>`, entrambi `pub(super)`.

- [ ] **Step 1: Scrivere il file con i test che falliscono**

Creare `src/app/view/classic.rs` con i soli test (le funzioni arrivano allo Step 3):

```rust
//! ArchLine: app-state glue for the classic menu bar.
//!
//! Kept out of `view_main` (see `ui::classic_toolbar::top_bar`): that function
//! is so large that extra nesting there can overflow rustc's stack.

#[cfg(test)]
mod tests {
    use crate::app::OpenCADStudio;

    #[test]
    fn the_window_menu_lists_every_tab_and_flags_the_active_one() {
        let app = OpenCADStudio::new_for_test();
        let tabs = app.classic_menu_tabs();
        assert_eq!(tabs.len(), app.tabs.len());
        assert_eq!(tabs.iter().filter(|t| t.active).count(), 1);
        assert!(tabs[app.active_tab].active);
        assert!(tabs.iter().all(|t| !t.name.is_empty()));
    }

    #[test]
    fn the_bar_builds_from_app_state() {
        use iced::advanced::Widget;
        let app = OpenCADStudio::new_for_test();
        let bar = app.classic_menu_bar();
        assert_eq!(bar.as_widget().size().width, iced::Length::Fill);
    }

    #[test]
    fn the_bar_is_hidden_on_start_and_clean_screen() {
        assert!(!crate::workspace::classic_active(true, false));
        assert!(!crate::workspace::classic_active(false, true));
    }
}
```

Aggiungere in `src/app/view/mod.rs`, prima di `mod controls;`:

```rust
mod classic;
```

- [ ] **Step 2: Eseguire i test e vedere il fallimento**

Run: `cargo test --locked --lib view::classic`
Expected: **errore di compilazione** `no method named classic_menu_tabs found for struct OpenCADStudio`.

- [ ] **Step 3: Implementare**

Inserire in `src/app/view/classic.rs`, tra il commento di testa e il modulo `tests`:

```rust
use crate::app::{Message, OpenCADStudio};
use crate::ui::classic_menu::{menu_bar, MenuCtx, TabEntry};
use iced::Element;

impl OpenCADStudio {
    /// One entry per open document tab, the active one flagged.
    pub(super) fn classic_menu_tabs(&self) -> Vec<TabEntry> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| TabEntry {
                index,
                name: tab.tab_display_name(),
                active: index == self.active_tab,
            })
            .collect()
    }

    /// The classic menu bar for the current state.
    #[inline(never)]
    pub(super) fn classic_menu_bar(&self) -> Element<'static, Message> {
        menu_bar(&MenuCtx {
            bindings: &self.shortcut_bindings,
            tabs: self.classic_menu_tabs(),
        })
    }
}
```

In `src/app/view/mod.rs`, dentro `view_main`, tra `mark("ribbon");` e `if self.show_file_tabs {`:

```rust
            mark("ribbon");
            if classic {
                col = col.push(self.classic_menu_bar());
            }
            if self.show_file_tabs {
```

- [ ] **Step 4: Eseguire i test**

Run: `cargo test --locked --lib view::classic`
Expected: **PASS** (3 test).

Poi, per verificare che l'intero crate compili con l'aggancio:

Run: `cargo check --locked`
Expected: nessun errore.

- [ ] **Step 5: Commit**

```bash
git add src/app/view/classic.rs src/app/view/mod.rs
git commit -m "Menu classico: aggancio in view_main sopra le schede"
```

---

### Task 5: Verifica completa e consegna a Mauro

**Files:** nessuna modifica di codice.

**Interfaces:** consuma tutto; non produce niente.

- [ ] **Step 1: Test del menu e suite completa**

Run: `cargo test --locked --lib classic_`
Expected: **PASS** (test di `classic_toolbar`, `classic_layers`, `classic_menu`).

Run: `cargo test --locked --lib`
Expected: **PASS**. Se compare `io::print_to_printer::…gdi_fallback_prints_landscape_sheets_landscape`, rilanciarlo da solo con `cargo test --locked --lib gdi_fallback_prints_landscape_sheets_landscape`: se passa è l'instabilità nota in parallelo, non una regressione. Riferire il risultato com'è, senza arrotondare.

- [ ] **Step 2: Build debug**

Run: `cargo build --locked --bin OpenCADStudio`
Expected: build riuscita; l'exe è in `target\debug\OpenCADStudio.exe`.

- [ ] **Step 3: Consegnare la checklist a Mauro**

Dire cosa è verificato (test, build) e cosa **no** (tutto ciò che si vede a video). Checklist per Mauro, su una scheda disegno, con l'interfaccia in inglese (Opzioni, Lingua, English):

1. La barra dei menu è sopra le schede documento, in solo testo, con 12 titoli: File, Edit, View, Insert, Format, Tools, Draw, Dimension, Modify, Parametric, Window, Help.
2. Un menu aperto copre le barre sotto e si chiude con un clic fuori o con Esc.
3. Aprendo un menu e passando sul titolo accanto, si apre quello (non verificato in codice).
4. Un comando parte e il menu si chiude (per esempio Draw, Line).
5. Le scorciatoie a destra corrispondono alle vere (Save: Ctrl+S, Undo: Ctrl+Z, Redo: Ctrl+Y).
6. Zoom, Inquiry, Text, Visual Styles e le tre voci di Parametric aprono il sottomenu laterale.
7. Window elenca le schede con la spunta sulla attiva e passa da una all'altra; Close All chiude tutto.
8. Sulla pagina iniziale e con `ARCHLINE_WORKSPACE=ribbon` la barra non compare.
9. Cosa apre Help (non verificato nel fork offline).
10. Con 10 o più schede aperte, il menu Window resta raggiungibile.

Se Mauro segnala differenze, correggere nel file del Task relativo e rifare i suoi test.

- [ ] **Step 4: Stato del branch**

Run: `git status --short` e `git log --oneline -6`
Expected: solo `.claude/` non tracciato; i commit dei Task 1–4 su `lavoro`; **nessun push**.

Il push lo decide Mauro.
