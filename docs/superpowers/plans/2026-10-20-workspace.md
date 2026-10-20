# Workspace: pulsante, menu e disposizioni salvate — Piano di implementazione

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** il pulsante Workspace Switching nella barra di stato con il menu di AutoCAD, due workspace predefiniti (AutoCAD Classic, OpenCAD Studio), workspace dell'utente salvabili, e le quattro voci sotto l'elenco funzionanti.

**Architecture:** modello puro in `src/workspaces.rs` (serde, in `settings.json` come sezione `workspaces`); l'interfaccia classico/ribbon diventa uno stato modificabile in `src/workspace.rs`; un solo `Message::Workspace(WorkspaceMsg)` gestito da `src/app/workspaces.rs`; finestre su `ModalKind`; il pulsante riusa `status_menu::menu_bar`.

**Tech Stack:** Rust, iced (`iced_test` per i test dei flussi), serde. Lingua dell'interfaccia: stringhe in inglese con `crate::t!`, come il resto dell'app.

**Spec:** `docs/superpowers/specs/2026-10-20-workspace-design.md` (§ citati sotto come "spec §N").

## Global Constraints

- Rispondere e scrivere i messaggi di commit **in italiano**, senza righe di attribuzione.
- Lavorare sul branch `claude/merge-upstream-2026.42`. **Non** toccare `main`, non pushare `lavoro`.
- File propri dove possibile (riduce i conflitti col merge da upstream); nei file di upstream (`src/app/mod.rs`, `src/app/view/mod.rs`, `src/ui/statusbar/mod.rs`, `src/app/update/mod.rs`) solo punti di aggancio minimi.
- `view_main` e `start_page_content` non si allungano: logica nuova in funzioni `#[inline(never)]` (CLAUDE.md, trappola 1).
- Le righe di un menu che si ricostruisce a ogni `view` sono `mouse_area`, mai `button` (trappola 2). Il menu del pulsante usa `status_menu::menu_bar`, che accetta righe `button` (lo fa già `statusbar_menu`).
- Nomi dei workspace: unici senza distinzione di maiuscole, non vuoti dopo `trim`, al massimo **40** caratteri; "AutoCAD Classic" e "OpenCAD Studio" sono riservati.
- `auto_save` spento e `show_label` spento di default. Interfaccia dal vivo, senza riavvio.
- `ToolbarLayout::default()` **non cambia** (spec §7.2).
- Verifica a ogni task: `cargo test --locked --lib <filtro>` verde; a fine piano `cargo build --release --bin ArchLine`. Con `ArchLine.exe` aperto il link fallisce: chiedere all'utente di chiuderlo, non chiuderlo.
- Test del dock che disegnano il ribbon: si eseguono con `ARCHLINE_WORKSPACE=ribbon`.
- La disposizione di lavoro (`App.toolbars`, sezione `toolbars` di `settings.json`) **non si sovrascrive all'avvio**: un workspace carica il suo layout solo quando l'utente lo sceglie (`apply_workspace`). All'avvio si imposta solo l'interfaccia.
- Nomi reali nel codice: la funzione che costruisce l'`AppConfig` da salvare è `current_config()` (`src/app/update/file.rs`), non `build_config`; `AppConfig::save` non fa nulla nei test, quindi i test controllano `current_config()`.

## Review Focus

Condizioni che la spec lascia implicite; ognuna ha il suo test nel task indicato.

1. `settings.json` di una versione precedente, senza sezione `workspaces` (Task 3, Task 4).
2. Nome vuoto, di soli spazi, di 41 caratteri, uguale a un predefinito o a un altro con maiuscole diverse (Task 3).
3. `current` che nomina un workspace inesistente, o eliminazione del corrente (Task 3, Task 5).
4. Cambio di workspace con un trascinamento di barra in corso, o con la scheda Start attiva, o in clean screen (Task 5).
5. Auto-save acceso e si lascia un workspace predefinito: non si crea nulla e non si sovrascrive il predefinito (Task 3).

## File Structure

| File | Responsabilità |
|---|---|
| `src/workspace.rs` (modifica) | `is_classic()` da `OnceLock` a stato modificabile; `set_classic(bool)` |
| `src/ui/toolbar_layout.rs` (modifica) | `ToolbarLayout::autocad_classic()`, `ToolbarId::from_title()` |
| `src/workspaces.rs` (nuovo, in `lib.rs`) | modello puro: `Interface`, `Workspace`, `WorkspaceSet`, predefiniti, regole dei nomi, migrazione |
| `src/app/config.rs` (modifica) | campo `workspaces`, default di `toolbars` |
| `src/app/update/file.rs` (modifica) | `apply_config`/`current_config` con `workspaces` |
| `src/app/mod.rs` (modifica) | campi `workspaces`, `workspace_ui`; `ModalKind::{Workspaces, WorkspaceSaveAs}`; `Message::Workspace` |
| `src/app/workspaces.rs` (nuovo) | `on_workspace(WorkspaceMsg)` e le azioni |
| `src/ui/window/workspaces.rs` (nuovo) | `WorkspaceMsg`, stato `WorkspaceUi`, viste: finestra a due schede e Save As |
| `src/ui/statusbar/workspace_menu.rs` (nuovo) | `WorkspaceMenu` (dati), `entries()`, `button()`, `label()` |
| `src/ui/statusbar/mod.rs` (modifica) | `StatusMenuData.workspace`, pulsante e etichetta |
| `src/app/view/mod.rs`, `src/app/view/modal.rs`, `src/app/update/mod.rs` (modifiche) | dati del pulsante, viste dei modali, chiusura dei modali |
| `CLAUDE.md` (modifica) | voce sui workspace |

---

### Task 1: Interfaccia classico/ribbon dal vivo

**Files:**
- Modify: `src/workspace.rs`
- Test: stesso file, modulo `tests`

**Interfaces:**
- Produces: `pub fn set_classic(classic: bool)`; `pub fn is_classic() -> bool` (stessa firma di oggi).

- [x] **Step 1: Test** in `workspace.rs`: `set_classic_flips_every_predicate` — chiama `set_classic(false)` e verifica `ribbon_shown(false, false)`, `!menu_bar_shown(false)`, `!classic_active(false, false)`; poi `set_classic(true)` e il contrario; alla fine ripristina il valore iniziale. Lo stato è globale, e `new_for_test()` passa da `apply_config`: sotto `cfg(test)` lo stato è un `thread_local!` `Cell` (inizializzato dalla variabile d'ambiente in ogni thread), così i test in parallelo non si disturbano e `with_stack` dei test del dock (thread proprio) resta valido.
- [x] **Step 2: Run** `cargo test --locked --lib workspace::` — FAIL (`set_classic` non esiste).
- [x] **Step 3: Implementare** lo stato (in produzione un `AtomicU8`: 0 = da leggere, 1 = classico, 2 = ribbon; sotto `cfg(test)` il `thread_local!` di cui sopra): `is_classic()` inizializza al primo uso da `ARCHLINE_WORKSPACE` (stessa regola di oggi: `ribbon`, senza distinzione di maiuscole, → ribbon), `set_classic()` scrive. Le altre funzioni restano invariate.
- [x] **Step 4: Run** — PASS, e `ARCHLINE_WORKSPACE=ribbon cargo test --locked --lib dock` verde come prima.
- [x] **Step 5: Audit.** Elencare con `Grep` tutti gli usi di `is_classic`, `classic_active`, `ribbon_shown`, `menu_bar_shown`, `bottom_chrome_shown` fuori da `workspace.rs` e confermare nel messaggio di commit che ognuno è letto a ogni `view` (non in cache all'avvio). Le chiamate note sono `app/view/mod.rs` (2487–2582, 2806, 3701) e i test: nessuna è in cache e nessuna costante di altezza dipende dalla modalità. Se ne emerge una nuova, correggerla qui.
- [x] **Step 6: Commit** `workspace: l'interfaccia classico/ribbon si cambia a metà sessione`.

### Task 2: Disposizione di AutoCAD Classic

**Files:**
- Modify: `src/ui/toolbar_layout.rs`
- Test: `src/ui/toolbar_registry.rs` (modulo `tests`, ha già `bar_named`)

**Interfaces:**
- Produces: `ToolbarId::from_title(title: &str) -> Option<ToolbarId>` (cerca nel registro per nome visualizzato, senza distinzione di maiuscole); `ToolbarLayout::autocad_classic() -> ToolbarLayout` (uguale al proprio `.sanitized()`, perché `first_run` li confronta).

- [x] **Step 1: Test** `autocad_classic_places_every_bar_of_the_screenshot` in `toolbar_registry.rs`: con `l = ToolbarLayout::autocad_classic()`, `l.lanes(Edge::Top)` è, per titoli, `[["Standard","Dimension","Inquiry"],["Layers","Styles"],["Insert","Reference"]]`; `lanes(Left)` = `[["Draw"]]`; `lanes(Right)` = `[["Modify","Draw Order"]]`; `lanes(Bottom)` = `[["Visual Styles","Preset Views","Modeling","Solid Editing"]]`. Ogni altra barra di `ToolbarId::all()` è nascosta. `autocad_classic_is_already_sanitized`: `l == l.clone().sanitized()`. Aggiungere `from_title_is_case_insensitive` (`"styles"` → `Some`, `"nope"` → `None`).
- [x] **Step 2: Run** `cargo test --locked --lib toolbar_registry` — FAIL.
- [x] **Step 3: Implementare** `autocad_classic()` scrivendo le corsie direttamente con `write_lanes(edge, &[...])` per ognuno dei quattro bordi (non con `move_to` in sequenza: ognuno sposta gli altri), partendo da `bars` vuoto. Draw, Modify, Layers e le tre barre di `DEFAULT_TOP` **non** sono nascoste quando assenti (`default_placement`): vanno tutte nelle corsie della tabella (spec §4), quindi nessuna resta col default; le altre sono assenti dalla mappa e quindi nascoste. `from_title` sopra `toolbar_registry::entries()`.
- [x] **Step 4: Run** — PASS; `cargo test --locked --lib toolbar` verde (nessun test esistente cambia: `default()` è intatto).
- [x] **Step 5: Commit** `barre: disposizione predefinita di AutoCAD Classic`.

### Task 3: Modello dei workspace

**Files:**
- Create: `src/workspaces.rs`; Modify: `src/lib.rs` (`pub mod workspaces;`)
- Test: stesso file

**Interfaces:**
- Consumes: `ToolbarLayout`, `ToolbarLayout::autocad_classic()` (Task 2).
- Produces (tutto `pub`):
  - `pub const AUTOCAD_CLASSIC: &str = "AutoCAD Classic"; pub const OPENCAD_STUDIO: &str = "OpenCAD Studio"; pub const MAX_NAME: usize = 40;`
  - `pub enum Interface { Classic, Ribbon }` (serde, `Copy`, `PartialEq`).
  - `pub struct Workspace { pub name: String, pub interface: Interface, pub toolbars: ToolbarLayout }`.
  - `pub struct WorkspaceSet { pub current: String, pub saved: Vec<Workspace>, pub auto_save: bool, pub show_label: bool }` (`#[serde(default)]`, `Default`: `current` vuoto).
  - `pub fn builtin() -> [Workspace; 2]`.
  - `impl WorkspaceSet`: `names() -> Vec<String>` (predefiniti, poi `saved` nell'ordine); `get(&self, name) -> Option<Workspace>` (cerca anche tra i predefiniti); `is_builtin(name) -> bool`; `validate_name(&self, name: &str, replacing: Option<&str>) -> Result<String, NameError>` (restituisce il nome pulito); `save(&mut self, name: &str, interface, toolbars)` (sostituisce o aggiunge, imposta `current`); `delete(&mut self, name) -> bool` (rifiuta i predefiniti; se era il corrente imposta `current = AUTOCAD_CLASSIC`); `rename`, `move_up`, `move_down`; `leave(&mut self, working: &ToolbarLayout)` (con `auto_save` acceso e corrente dell'utente, ne aggiorna il layout; altrimenti non fa nulla); `first_run(&mut self, working: &ToolbarLayout, env_ribbon: bool)` (spec §7.1–§7.2; con `env_ribbon` e un layout personalizzato vince "My Workspace": l'ambiente conta solo se il layout è `autocad_classic()`); `sanitized(self) -> WorkspaceSet` (usato al caricamento: scarta i nomi vuoti, riservati o doppi di `saved`, tronca a 40 caratteri, e se `current` non esiste lo porta a `AUTOCAD_CLASSIC`).
  - `pub enum NameError { Empty, TooLong, Reserved, Duplicate }`.

- [ ] **Step 1: Test** (uno per riga, tutti in `workspaces.rs`): `builtin_classic_matches_autocad_classic_layout`; `builtin_ribbon_is_ribbon_interface`; `validate_name_trims_and_rejects` (vuoto, `"   "`, 41 caratteri → `TooLong`, `"autocad classic"` → `Reserved`, `"Mio"` due volte con maiuscole diverse → `Duplicate`, ma `replacing: Some("Mio")` lo accetta); `save_replaces_same_name_and_sets_current`; `delete_current_falls_back_to_autocad_classic`; `delete_refuses_a_builtin`; `missing_current_resolves_to_autocad_classic` (un metodo `resolved(&self) -> Workspace`); `leave_with_auto_save_updates_a_user_workspace_only` (acceso: utente aggiornato, predefinito intatto e nessun workspace creato; spento: nulla cambia); `old_settings_without_section_deserialises` (`serde_json::from_str::<WorkspaceSet>("{}")`); `round_trip_serde`; `first_run_keeps_a_custom_layout_as_my_workspace` (layout ≠ `autocad_classic()` → `saved == ["My Workspace"]`, corrente "My Workspace"); `first_run_on_the_classic_layout_picks_autocad_classic`; `first_run_with_env_ribbon_and_default_picks_opencad_studio`; `first_run_with_env_ribbon_and_a_custom_layout_picks_my_workspace`; `sanitized_drops_bad_names_and_a_missing_current` (settings modificato a mano: duplicati con maiuscole diverse, nome riservato, nome vuoto); `first_run_is_idempotent` (a `current` non vuoto non fa nulla).
- [ ] **Step 2: Run** `cargo test --locked --lib workspaces::` — FAIL (modulo mancante).
- [ ] **Step 3: Implementare** secondo le firme sopra; `Workspace` e `WorkspaceSet` derivano `Clone`, `Debug`, `PartialEq`, `Serialize`, `Deserialize`. I predefiniti non sono mai serializzati.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `workspace: modello dei workspace e dei loro nomi`.

### Task 4: Configurazione e avvio

**Files:**
- Modify: `src/app/config.rs`, `src/app/update/file.rs`, `src/app/mod.rs`
- Test: `src/app/config.rs` (modulo `tests`)

**Interfaces:**
- Consumes: `WorkspaceSet` (Task 3), `set_classic` (Task 1), `ToolbarLayout::autocad_classic()`.
- Produces: `AppConfig.workspaces: WorkspaceSet`; `App.workspaces: WorkspaceSet` (campo `pub(crate)`).

- [ ] **Step 1: Test** in `config.rs`: `workspaces_default_when_absent_and_round_trip`; `a_fresh_install_has_the_autocad_classic_toolbars` (`AppConfig::default().toolbars == ToolbarLayout::autocad_classic()`); aggiornare `toolbars_default_when_absent_and_round_trip` (riga ~740): un `settings.json` senza la sezione dà `AppConfig::default().toolbars`, cioè ora `autocad_classic()` (`AppConfig` è `#[serde(default)]`), e non più `ToolbarLayout::default()`. In `app/update/file.rs` (o `app/dock/tests.rs`, dove ci sono test dell'app): `apply_config_runs_first_run_and_sets_the_interface` — un `AppConfig` con `toolbars` personalizzati e `workspaces` vuoto, applicato a `OpenCADStudio::new_for_test()`, lascia `workspaces.current == "My Workspace"` e `is_classic()` vero; con `workspaces.current = "OpenCAD Studio"` dopo `apply_config` `is_classic()` è falso; `apply_config_keeps_the_working_toolbars` — con `current = "AutoCAD Classic"` e `toolbars` diversi dal layout salvato, `app.toolbars` resta quello caricato (nessuna sovrascrittura all'avvio). Dopo `apply_config` con `workspaces` vuoto, `current_config().workspaces.current` non è vuoto.
- [ ] **Step 2: Run** `cargo test --locked --lib -- config apply_config` — FAIL. Nota: `new_for_test()` chiama `apply_config(AppConfig::default())`.
- [ ] **Step 3: Implementare** il campo `workspaces` in `AppConfig` (con default) e `toolbars: ToolbarLayout::autocad_classic()` in `AppConfig::default()`; in `apply_config`, dopo `self.toolbars = ...`, `self.workspaces = cfg.workspaces.sanitized()`, `first_run(&self.toolbars, env_ribbon)` (`env_ribbon` = `ARCHLINE_WORKSPACE` è ribbon) e **solo** `set_classic(current.interface == Classic)`: `self.toolbars` resta com'è. `current_config()` scrive `workspaces`. `last_saved_config` viene inizializzato dopo `apply_config` (`app/mod.rs` ~5405): se `first_run` ha cambiato qualcosa, chiamare `save_config()` alla fine dell'avvio perché la migrazione sia scritta.
- [ ] **Step 4: Run** — PASS, più `cargo test --locked --lib config` e `app::update::toolbar` (aggiornare l'asserzione di `ToolbarMsg::Reset`: il reset ora porta a `ToolbarLayout::autocad_classic()`; in `src/app/update/toolbar.rs` cambiare `Default::default()` in `autocad_classic()` e il test che lo confronta, riga ~289).
- [ ] **Step 5: Commit** `workspace: configurazione, primo avvio e reset delle barre`.

### Task 5: Azioni dei workspace (senza interfaccia)

**Files:**
- Create: `src/app/workspaces.rs`, `src/ui/window/workspaces.rs` (solo `WorkspaceMsg` e `WorkspaceUi` per ora); Modify: `src/app/mod.rs`, `src/ui/window/mod.rs`, `src/app/update/mod.rs` (una riga: `Message::Workspace(m) => self.on_workspace(m)`)
- Test: `src/app/workspaces.rs` (modulo `tests`, stile di `app/update/toolbar.rs`: `OpenCADStudio::new_for_test()` e `app.update(Message::Workspace(..))`)

**Interfaces:**
- Produces:
  - `pub enum WorkspaceMsg { Switch(String), ToggleLabel, ToggleAutoSave, OpenSaveAs, OpenSettings, OpenCustomize, SaveAsName(String), SaveAsApply, SaveAsConfirmReplace, Delete(String), Rename(String, String), MoveUp(String), MoveDown(String), SetCurrent(String), ToggleBar(ToolbarId), Restore, Tab(WorkspaceTab) }`
  - `pub enum WorkspaceTab { Settings, Customize }`
  - `pub struct WorkspaceUi { pub tab: WorkspaceTab, pub save_name: String, pub save_error: Option<NameError>, pub confirm_replace: bool, pub rename: Option<(String, String)> }`
  - `App.workspace_ui: Option<WorkspaceUi>`; `ModalKind::{Workspaces, WorkspaceSaveAs}`.
  - `impl OpenCADStudio { pub(super) fn on_workspace(&mut self, m: WorkspaceMsg) -> Task<Message>; fn apply_workspace(&mut self, name: &str) }`.

- [ ] **Step 1: Test** (nomi → asserzioni): `switch_to_opencad_studio_turns_the_ribbon_on` (`is_classic()` falso, `workspaces.current` aggiornato, `toolbars` invariate); `switch_back_restores_the_classic_layout`; `switch_clears_a_toolbar_drag_in_progress` (`app.toolbar_drag = Some(..)` → `None`); `switch_works_on_the_start_tab_and_with_clean_screen` (nessun panic, `view` costruibile: usare l'helper dei test di `app/view/classic.rs`); `switch_with_auto_save_on_writes_back_a_user_workspace_not_a_builtin`; `switch_to_a_missing_name_changes_nothing`; `switch_cancels_a_running_command_and_clears_menus` (comando attivo, `ribbon.open_dropdown = Some(..)`, tutto azzerato); `leaving_a_builtin_with_unsaved_bar_changes_prints_a_notice` (riga in riga di comando: "le modifiche alle barre non sono state salvate; usa Save Current As…"); `save_as_with_a_valid_name_adds_and_selects_it`; `save_as_with_an_invalid_name_sets_the_error_and_saves_nothing`; `save_as_an_existing_name_asks_before_replacing` (`confirm_replace` vero, nessuna modifica fino a `SaveAsConfirmReplace`); `delete_the_current_applies_autocad_classic`; `rename_and_move_keep_the_set_consistent`; `restore_reloads_the_saved_layout` (modificare `app.toolbars`, `Restore` → uguale al salvato; per un predefinito, a `builtin()`); `toggle_bar_hides_and_shows_in_the_working_layout`; `toggle_label_and_auto_save_flip_and_persist` (il valore è in `current_config().workspaces`).
- [ ] **Step 2: Run** `cargo test --locked --lib app::workspaces` — FAIL.
- [ ] **Step 3: Implementare** secondo le firme. `apply_workspace`: `self.workspaces.leave(&self.toolbars)`, `self.toolbar_drag = None`, azzera `tool_hold`, `ribbon.open_dropdown` e `dock_edge_menu`, annulla il comando in corso come fa `Message::TabSwitch` (leggere `src/app/update/mod.rs` ~2610), carica il workspace con `resolved()`/`get`, `set_classic`, se classico `self.toolbars = ws.toolbars.sanitized()`, `self.workspaces.current = ws.name`, `self.save_config()`. `ToggleBar` riusa `ToolbarMsg::Toggle` (e la sua regola "almeno una barra visibile"). Aggiungere in `src/app/view/modal.rs` i due rami di `ModalKind::{Workspaces, WorkspaceSaveAs}` in `modal_title` e in `modal_content` (senza `_ =>` i `match` non compilerebbero): titoli "Workspaces" e "Save Workspace As" e, per ora, un contenuto segnaposto (le viste arrivano nel Task 7). `OpenSaveAs/OpenSettings/OpenCustomize` impostano `workspace_ui`, chiamano `reset_modal_geometry()` e impostano `active_modal` (come `block_editor.rs` ~425). La chiusura del modale (in `update/mod.rs`, accanto a `Some(BlockSaveAs) => ...`) azzera `workspace_ui`.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `workspace: azioni di cambio, salvataggio ed eliminazione`.

### Task 6: Pulsante e menu nella barra di stato

**Files:**
- Create: `src/ui/statusbar/workspace_menu.rs`; Modify: `src/ui/statusbar/mod.rs`, `src/app/view/mod.rs` (solo il riempimento dei dati)
- Test: `workspace_menu.rs`; un test del flusso in `src/app/dialog_flow_tests.rs`

**Interfaces:**
- Produces: `pub enum Row { Workspace { name: String, current: bool }, Separator, SaveAs, Settings, Customize, Label { on: bool } }` e `pub fn rows(menu: &WorkspaceMenu) -> Vec<Row>` (puro; `entries()` le trasforma in `Entry`); `pub struct WorkspaceMenu { pub names: Vec<String>, pub current: String, pub show_label: bool }` (`Clone`, `Default`); `pub fn entries(menu: &WorkspaceMenu) -> Vec<Entry<'static>>`; `pub fn button() -> Element<'static, Message>`; `pub fn label(menu: &WorkspaceMenu) -> Option<Element<'static, Message>>`. `StatusMenuData` guadagna `pub workspace: WorkspaceMenu`.
- Consumes: `WorkspaceMsg` (Task 5), `status_menu::{menu_bar, Entry}`.

- [ ] **Step 1: Test**: in `workspace_menu.rs`, `entries_follow_the_screenshot_order` (su `rows(menu) -> Vec<Row>`, vedi sotto: n nomi, poi esattamente 4 righe di azione e 2 separatori nelle posizioni di spec §5; la riga del corrente è spuntata; i campi di `status_menu::Entry` sono privati, quindi il test non può contare le `Entry`); `label_is_only_shown_when_asked`; `workspace_menu_click_switches_the_interface`. **Prima uno spike**: nessun test esistente apre un `iced_aw::MenuBar` col simulatore (e `StatusBar::view` ha una decina di argomenti, `StatusMenuData` non ha `Default`). Provare `status_menu::menu_bar(button(), entries(&menu), 220.0)` da solo con `point_at` e `ButtonPressed`; se l'overlay non è raggiungibile col simulatore, ripiegare su un test a livello di messaggi (`Message::Workspace(WorkspaceMsg::Switch(..))` dalle righe prodotte da `rows`) e dirlo nel commit.
- [ ] **Step 2: Run** `cargo test --locked --lib -- workspace_menu workspace_menu_click` — FAIL.
- [ ] **Step 3: Implementare**: `button()` è un `button` con l'icona a ingranaggio (usare `crate::ui::icons` esistente, es. `gear.svg` in `assets/icons/ui/`), stile `button::subtle`, come `customize_btn`; in `statusbar/mod.rs` aggiungere il pulsante (avvolto in `status_menu::menu_bar(.., entries, 220.0)` e in `menu_tip` con la descrizione "Workspace Switching") **prima** della maniglia di personalizzazione, e `label()` subito prima del pulsante; `app/view/mod.rs` riempie `menu_data.workspace` con una funzione `#[inline(never)]` (`workspace_menu_data(&self) -> WorkspaceMenu`).
- [ ] **Step 4: Run** — PASS; `cargo test --locked --lib statusbar` verde.
- [ ] **Step 5: Commit** `barra di stato: pulsante e menu dei workspace`.

### Task 7: Finestre Save As e Workspaces

**Files:**
- Modify: `src/ui/window/workspaces.rs` (viste), `src/app/view/modal.rs` (titoli e viste), `src/ui/window/mod.rs`
- Test: `src/app/dialog_flow_tests.rs`

**Interfaces:**
- Consumes: `WorkspaceUi`, `WorkspaceMsg`, `WorkspaceSet` (Task 3 e 5), `toolbar_dock::menu_entries` per l'elenco delle barre.
- Produces: `pub(crate) fn view_save_as(ui: &WorkspaceUi, sizing: ModalSizing) -> Element<'_, Message>`; `pub(crate) fn view_window(ui: &WorkspaceUi, set: &WorkspaceSet, bars: &[BarEntry], classic: bool, sizing: ModalSizing) -> Element<'_, Message>`.

- [ ] **Step 1: Test** (flussi con i messaggi dell'interfaccia): `save_as_dialog_creates_a_workspace_from_the_menu` (Message `OpenSaveAs`, scrivere il nome, `SaveAsApply`: compare nel menu e `current` cambia); `save_as_dialog_shows_the_error_and_disables_ok` (`ui.save_error` vero con nome vuoto e il bottone OK non pubblica nulla); `settings_tab_lists_builtins_read_only` (nessun `Delete`/`Rename` pubblicati per i predefiniti); `customize_tab_toggles_a_bar` (clic sulla riga → barra nascosta nel layout di lavoro); `customize_tab_explains_when_the_ribbon_is_active`.
- [ ] **Step 2: Run** `cargo test --locked --lib dialog_flow` — FAIL.
- [ ] **Step 3: Implementare** le due viste (stile di `block_save_as::view`: campo + OK/Annulla; finestra a due schede con elenco, pulsanti Imposta come corrente/Rinomina/Elimina/Su/Giù, interruttore auto-save, e nella scheda Customize l'elenco delle barre con caselle più "Ripristina workspace"); in `modal.rs` il titolo ("Save Workspace As", "Workspaces") e la vista dentro `sized_flow(ex, 430, 220, ..)` e `sized_flow(ex, 640, 520, ..)`; Invio nel Save As si gestisce come per `BlockSaveAs` in `update/mod.rs`.
- [ ] **Step 4: Run** — PASS; `cargo test --locked --lib i18n` verde (se fallisce per stringhe nuove, aggiungere le voci come indica il messaggio del test).
- [ ] **Step 5: Commit** `workspace: finestre Save As, Settings e Customize`.

### Task 8: Documentazione e verifica finale

**Files:**
- Modify: `CLAUDE.md` (voce "Cosa è ArchLine": workspace, file nuovi, regole), `docs/superpowers/specs/2026-10-20-workspace-design.md` (stato: "implementata")

- [ ] **Step 1:** Aggiungere a `CLAUDE.md` una voce `src/workspaces.rs` + `src/app/workspaces.rs` + `src/ui/window/workspaces.rs` + `src/ui/statusbar/workspace_menu.rs` con: cosa fa, `Message::Workspace`, che `ToolbarLayout::default()` resta il default storico e `autocad_classic()` è quello di AutoCAD, e che `ARCHLINE_WORKSPACE` vale solo come valore iniziale.
- [ ] **Step 2: Run** `cargo test --locked --lib -- workspace toolbar classic statusbar config i18n dialog_flow` — tutto verde; poi `cargo check --locked`.
- [ ] **Step 3: Run** `cargo build --release --bin ArchLine` (con ArchLine chiuso) — `Finished`.
- [ ] **Step 4:** Chiedere all'utente uno screenshot del menu aperto, della finestra Customize e del passaggio a OpenCAD Studio e ritorno; correggere ciò che non va a schermo.
- [ ] **Step 5: Commit** `workspace: documentazione`.
