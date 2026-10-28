# BIM parte 2: abachi e quantità — piano di implementazione

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dalla pianta escono quantità di demolizioni (m³), nuove murature (m²) e locali (m²), ciascuna con fonte e stato, in tre tabelle nel DWG (`AL_ABACO`) e in un CSV italiano (`AL_ABACO_CSV`), con altezza a tre livelli (`AL_ALTEZZA`).

**Architecture:** Logica pura in file nuovi di `src/bim/` (`height.rs`, `quantities.rs`, `abaco.rs`, `abaco_table.rs`); i comandi stanno in `src/app/commands/bim_abaco.rs` e si agganciano a `dispatch_bim`. Le quantità si calcolano da zone (`zone::zones_in`) e locali con le stesse misure e tolleranze di `AL_AGGIORNA`/`AL_ESTRAI` (R61), mai da etichette scritte. Le righe incomplete (altezza mancante) restano nelle tabelle come «n.d.» e non entrano né nei totali né nel CSV.

**Tech Stack:** Rust (`codec` = opencaddocument, `kernel::geom2d`), `iced`; banco Python con `ezdxf`.

**Spec:** `docs/superpowers/specs/2026-10-28-bim-abachi-quantita-design.md` (approvata il 2026-10-28). Parte 1: `…2026-10-23-bim-stato-piante-design.md`, registro `docs/superpowers/ledgers/2026-10-23-bim-stato-piante.md`, commenti di modulo di `src/bim/`.

## Global Constraints

- Nessun valore inventato: ogni numero ha fonte e stato; mai un'altezza di ripiego (spec §2).
- Una riga con altezza mancante **non** entra nei totali né nel CSV; mai un campo vuoto al posto di un'altezza mancante (spec §2, §5.3).
- Il varco **non** eredita l'altezza: serve la sua, altrimenti `n.d.` con motivo «altezza del varco da inserire» (spec §4.1).
- L'abaco non modifica mai rilievo, zone o locali; `AL_ALTEZZA` è l'unico comando che li scrive. `INSUNITS` non si scrive mai.
- Altezze digitate e salvate **in unità del disegno**; messaggio con eco in metri; avviso fuori da 0,50–10 m (spec §3).
- Numeri in tabella: 2 decimali, virgola; CSV: `;`, 4 decimali, virgola decimale, BOM UTF-8, CRLF; totali sui valori non arrotondati.
- Nessuna dipendenza nuova. File propri sotto `src/bim/`; ganci in codice di upstream al minimo (`src/bim/mod.rs`, registrazione comandi, menu, registro barre, `menu_icons.rs`).
- Test: `cargo test --locked --lib bim::`, poi `--lib app::bim_tests`, `python tests/conformance/bim_extract.py target\release\ArchLine.exe`, `cargo test --locked --lib` intero. Un solo cargo alla volta, in primo piano, timeout 600000. «Accesso negato» = `ArchLine.exe` in uso: chiedere a Mauro di chiuderlo.
- Branch `claude/bim-quantita`; commit in italiano, senza righe di attribuzione; niente push senza conferma di Mauro. CI spenta. Disegni di prova solo sintetici.
- Negli script i numeri col punto decimale (`2.70`): la virgola verrebbe letta come separatore di coordinate.

## Review Focus

- `INSUNITS` = 0 e nessun record: `AL_ALTEZZA`, `AL_ABACO`, `AL_ABACO_CSV` chiedono l'unità e rilanciano, non calcolano mai in unità presunte (Task 3, 7, 8).
- Nome di locale con `;`, virgolette o a capo: il CSV resta una riga valida (Task 8).
- Zona degenere (asse lunghezza 0, area 0) o altezza non finita/≤ 0 nell'XDATA di un file: riga `n.d.`, nessun panico, nessuna divisione per zero (Task 2, 4).
- Tabelle già presenti: copie duplicate dello stesso tipo, una cancellata, una su layer bloccato (Task 7).
- Nessuna zona né locale: nessuna tabella creata e un messaggio; gruppi vuoti non creano tabelle vuote (Task 7).

---

### Task 1: Altezza nei dati (record di progetto e locale)

**Files:**
- Modify: `src/bim/record.rs` (voce nuova), `src/bim/rooms.rs` (campo `height` in `Room`, chiave XDATA), e i punti che costruiscono `Room { .. }` (`src/bim/room_command.rs`, `src/bim/extract.rs`, test di `rooms.rs` e `src/app/bim_tests.rs`: il compilatore li elenca, aggiungere `height: None`).

**Interfaces:**
- Produces: `ProjectRecord.default_height: Option<f64>` (unità del disegno, XRecord codice 40, `XRecordEntry::double(40, v)`, scritto solo se `Some`; letta con `XRecordValue::Double`); `Room.height: Option<f64>` (chiave `"altezza"` + `XDataValue::Real` in coda a `encode`; `decode` la legge per chiave e ignora valori non finiti o ≤ 0).
- Consumes: —

- [ ] **Step 1: test (in `record.rs` e `rooms.rs`)** — `record_roundtrips_default_height` (scrive `default_height: Some(2.7)` con unità metri, rilegge uguale; un record scritto senza voce rilegge `None`); `record_without_height_entry_still_reads` (record costruito a mano con le sole voci 90/70/8). `room_xdata_roundtrips_height` (`Room { height: Some(3.0), .. }` con `encode`/`decode` torna uguale); `room_xdata_without_height_key_decodes_none`; `room_xdata_ignores_non_positive_height` (`-1.0`, `0.0`, `NaN` → `None`).
- [ ] **Step 2:** `cargo test --locked --lib bim::record bim::rooms` — attesi FAIL di compilazione.
- [ ] **Step 3:** aggiungere campo e voce; `record::write` non scrive la voce se `None`; `Room.height` aggiunto in coda a `encode` solo se `Some`. Sistemare le costruzioni di `Room`.
- [ ] **Step 4:** `cargo test --locked --lib bim::` — tutti verdi (nessuna regressione sui test esistenti).
- [ ] **Step 5: commit** — `git commit -m "BIM parte 2: altezza nel record di progetto e nel locale"`.

### Task 2: Risoluzione dell'altezza a tre livelli (pura)

**Files:**
- Create: `src/bim/height.rs`. Modify: `src/bim/mod.rs` (`pub mod height;`), `src/bim/report.rs` (nuovi `Notice`).

**Interfaces:**
- Produces:
  - `pub enum HeightState { Inserita, Ereditata, NonDisponibile }`.
  - `pub struct Height { pub value_m: Option<f64>, pub state: HeightState, pub note: Option<String> }` con `Height::missing(note: Option<&str>) -> Height`.
  - `pub fn resolve(own: Option<f64>, default: Option<f64>, unit: DrawingUnit) -> Height` (valori in unità del disegno; non finito o ≤ 0 conta come assente).
  - `pub fn resolve_varco(own: Option<f64>, unit: DrawingUnit) -> Height` (mai eredita; senza valore: nota «altezza del varco da inserire»).
  - `pub fn resolve_project_room(seed_own: Option<f64>, fatto: &[(String, Height)], default: Option<f64>, unit: DrawingUnit) -> (Height, Option<Notice>)` (regola della spec §4.1: seme proprio; poi locali di fatto concordi con nota «dal locale di fatto» e stato `Inserita` se almeno uno inserito, altrimenti `Ereditata`; discordi o con uno `NonDisponibile` → `NonDisponibile` e `Notice::AltezzeDiscordi`; senza locali di fatto, la predefinita).
  - `pub fn range_warning(value: f64, unit: DrawingUnit) -> Option<Notice>` (fuori da 0,50–10 m).
  - Nuovi `Notice` in `report.rs`, con `text()` italiano e voce nel test che elenca tutte le varianti: `AltezzaFuoriIntervallo { value_m: f64 }`, `AltezzeDiscordi { names: Vec<String>, values_m: Vec<Option<f64>> }`, `SpessoreDiverso { code: String, declared_m: f64, measured_m: f64 }`, `ZoneSovrapposteSostanzialmente { codes: Vec<String> }`, `RigaIncompleta { code: String, reason: String }` (nessun'altra variante: non aggiungere ciò che nessun task usa).
- Consumes: `DrawingUnit::{metres, per_metre}`, `Notice`.

- [ ] **Step 1: test (in `height.rs`)** — `own_height_wins_over_default` (`resolve(Some(3.0), Some(2.7), Metri)` → 3,0 `Inserita`); `default_is_inherited` (→ 2,7 `Ereditata`); `missing_everywhere_is_not_available`; `invalid_own_falls_back_to_default` (`NaN`, `0.0`, `-1.0` con default 2,7 → `Ereditata`; senza default → `NonDisponibile`); `centimetre_drawing_converts` (`resolve(Some(270.0), None, Centimetri)` → 2,70 m); `varco_never_inherits` (`resolve_varco(None, Metri)` → `NonDisponibile` con nota; `Some(2.1)` → `Inserita` 2,1); `project_room_own_seed_wins`; `project_room_concordant_fatto_inherits_state_and_note` (2,7 e 2,7 `Ereditata` → `Ereditata`; uno `Inserita` 3,0 e uno `Ereditata` 3,0 → `Inserita`); `project_room_discordant_fatto_is_not_available_with_notice` (2,7 e 3,0); `project_room_without_fatto_uses_default`; `range_warning_bounds` (0,4 m e 11 m avvisano; 2,7 m no; in un disegno in cm il valore 2,7 avvisa).
- [ ] **Step 2:** `cargo test --locked --lib bim::height` — FAIL.
- [ ] **Step 3:** implementare le firme sopra; un solo helper privato `valid(v: Option<f64>) -> Option<f64>` per «finito e > 0».
- [ ] **Step 4:** `cargo test --locked --lib bim::` — verdi.
- [ ] **Step 5: commit** — «BIM parte 2: risoluzione dell'altezza a tre livelli».

### Task 3: `AL_ALTEZZA`

**Files:**
- Create: `src/app/commands/bim_abaco.rs` (solo i metodi di questo comando in questo task). Modify: `src/app/commands/bim.rs` (`inventory::submit!` con i tre nomi nuovi, rami di `dispatch_bim`), `src/app/commands/mod.rs` (`mod bim_abaco;`), `src/bim/rooms.rs` (funzione pura sotto), test in `src/app/bim_tests.rs`.

**Interfaces:**
- Produces: `rooms::room_height_edits(doc: &Document, selected: &[Handle], height: Option<f64>, tol: f64) -> Vec<(Handle, Room, RoomPart)>` — per ogni parte di locale selezionata, tutte le parti di locale con stessa fase e seme entro `tol`, col `Room` riscritto (altezza nuova o tolta); senza doppioni. `OpenCADStudio::bim_altezza(&mut self, i: usize, cmd: &str) -> Task<Message>` (chiamato da `dispatch_bim` per `"AL_ALTEZZA"`).
- Sintassi: `AL_ALTEZZA L <v>` (predefinita del livello, record, un passo `AL_ALTEZZA`), `AL_ALTEZZA S <v>` (zone e locali selezionati: zone con `zone::encode` + `restyle` non serve; solo `set_entity_xdata(.., APP_ZONA, ..)`; locali con `rooms::encode` + `set_entity_xdata(.., APP_LOCALE, ..)`; un solo passo con `apply_property_op`), `AL_ALTEZZA R` (toglie sulla selezione), senza argomenti `KeywordCommand`/`SelectThenKeywordCommand` con `[Livello/Selezione/Rimuovi]`. Valore via `parse_positive` di `bim.rs` (renderlo `pub(super)`); eco «altezza 270 cm = 2,70 m»; `range_warning` come messaggio; spazio carta/editor di blocchi rifiutati con `bim_in_model_space`; unità con `bim_unit_or_ask`.
- Consumes: Task 1 (`default_height`, `Room.height`), Task 2 (`range_warning`).

- [ ] **Step 1: test (in `bim_tests.rs`, con `fresh_app`, `load`, `run`, `case_a`)** — `altezza_livello_saves_default_and_echoes_metres` (`INSUNITS` 6, `run "AL_ALTEZZA L 2.70"` → `record::read(doc).default_height == Some(2.7)`, storia contiene «2,70 m», un passo di annullamento in più); `altezza_livello_in_cm_echoes_and_warns` (`INSUNITS` 5: `L 270` → storia contiene «270 cm = 2,70 m»; `L 2.7` → avviso di intervallo); `altezza_selection_writes_zones_and_rooms_in_one_step` (zona + locale di fatto selezionati, `S 3.00` → `zone.height == Some(3.0)`, contorno e testo del locale con altezza `Some(3.0)`, `undo_steps` +1, rilievo identico a prima; `R` → tutto `None`); `altezza_rejects_bad_values` (`L -1`, `L abc`, `L 0`, `L 1 2` → errore in storia, record e passi invariati); `altezza_asks_the_unit_when_insunits_is_zero` (come `unit_is_asked_and_insunits_untouched` per `AL_STATO`); `altezza_refuses_paper_space` (come gli altri comandi, se esiste già un test analogo copiarne il modo).
- [ ] **Step 2:** `cargo test --locked --lib app::bim_tests::altezza` — FAIL.
- [ ] **Step 3:** implementare `room_height_edits` (con test unitario in `rooms.rs`: `height_edits_cover_both_parts_of_a_fatto_room`) e il comando.
- [ ] **Step 4:** `cargo test --locked --lib bim:: app::bim_tests` — verdi.
- [ ] **Step 5: commit** — «BIM parte 2: AL_ALTEZZA».

### Task 4: Quantità delle zone (pura)

**Files:**
- Create: `src/bim/quantities.rs` (`pub mod quantities;` in `mod.rs`).

**Interfaces:**
- Produces:
  - `pub enum ZoneKind { Demolizione, Varco, Nuova }`.
  - `pub struct ZoneRow { pub handle: Handle, pub kind: ZoneKind, pub code: String /* D1, V1, N1 per ordine di handle */, pub plan_area_m2: f64, pub length_m: Option<f64>, pub thickness_m: Option<f64> /* effettivo = area ÷ lunghezza */, pub declared_thickness_m: Option<f64>, pub height: Height, pub wall_area_m2: Option<f64> /* demolizione: L×H */, pub volume_m3: Option<f64>, pub surface_m2: Option<f64> /* nuova: L×H */, pub note: Option<String> }` con `fn is_complete(&self) -> bool` (principale calcolata: volume per Demolizione/Varco, superficie per Nuova) e `fn main(&self) -> Option<f64>`.
  - `pub fn zone_rows(zones: &[ZoneGeom], default_height: Option<f64>, unit: DrawingUnit, tol: f64) -> (Vec<ZoneRow>, Vec<Notice>)`.
  - `pub fn group_new_by_thickness(rows: &[ZoneRow]) -> Vec<ThicknessGroup>` con `ThicknessGroup { thickness_mm: Option<i64>, rows: Vec<usize>, total_surface_m2: f64, total_length_m: f64 }` (spessore al millimetro; `None` = «spessore n.d.»; i totali sommano solo le righe complete).
- Regole (spec §4.3): area in pianta = `|signed_area(anello esterno)|` meno i buchi (`kernel::geom2d::signed_area`), poi `units::area_m2`; lunghezza dall'`axis`; spessore effettivo = area ÷ lunghezza solo se c'è l'asse e la lunghezza > `tol`; `SpessoreDiverso` se `declared` differisce da effettivo oltre `tol`; altezza: `resolve` (varco: `resolve_varco`); demolizione: volume = area × H (senza asse comunque), parete solo con asse; varco: volume = L × spessore effettivo × H; nuova: superficie = L × H solo con asse (senza asse: `surface_m2 = None`, nota «zona senza asse»), volume secondario = area × H; sovrapposizione sostanziale tra zone dello stesso stato = area comune > 25% dell'area della più piccola (area comune per triangolazione `triangulate_rings` e ritaglio di triangoli convessi, con prefiltro per riquadri) → `ZoneSovrapposteSostanzialmente`; gli incroci (≤ 25%) non danno avviso; un varco dentro una demolizione dello stesso stato conta come sovrapposizione.
- Consumes: `zone::{zones_in, ZoneGeom}`, Task 2.

- [ ] **Step 1: test (in `quantities.rs`, zone costruite a mano con `zone::zone_hatch`/`ZoneGeom`, metri)** — `demolition_without_height_default_is_not_available` (caso H: nessuna altezza → `volume_m3 == None`, `!is_complete()`); `demolition_inherits_default` (caso A: tramezzo 0,10 × 3,00, asse presente, default 2,70 → area 0,30, lunghezza 3,00, spessore effettivo 0,10, volume 0,81, parete 8,10, altezza `Ereditata`); `demolition_own_height` (3,00 → 0,90 e 9,00, `Inserita`); `demolition_without_axis_still_has_volume` (area × H, parete `None`); `varco_requires_own_height` (0,10 × 0,90, default 2,70 impostata → `NonDisponibile`; con 2,10 → volume 0,189); `new_wall_surface_and_group` (asse 2,00, spessore 0,08, default 2,70 → superficie 5,40, volume 0,432, gruppo spessore 80 mm; due zone stesso spessore → un gruppo, somma esatta); `new_wall_without_axis_has_no_surface` (nota «zona senza asse»); `declared_thickness_differs_from_measured` (metadato 0,12, anelli 0,10 → spessore 0,10, `Notice::SpessoreDiverso`, `L × W × H` = volume); `degenerate_zone_does_not_panic` (area 0, asse lunghezza 0, altezza `NaN`: riga `n.d.`, nessun panico); `corner_overlap_gives_no_notice` (due zone nuove ortogonali sovrapposte 0,10 × 0,10); `substantial_overlap_gives_notice` (sovrapposte oltre 25%); `varco_inside_demolition_gives_notice`; `codes_follow_handle_order`.
- [ ] **Step 2:** `cargo test --locked --lib bim::quantities` — FAIL.
- [ ] **Step 3:** implementare; il ritaglio convesso (Sutherland–Hodgman su triangoli) è un helper privato `overlap_area(a: &[Vec<[f64;2]>], b: &[Vec<[f64;2]>]) -> f64`.
- [ ] **Step 4:** `cargo test --locked --lib bim::` — verdi.
- [ ] **Step 5: commit** — «BIM parte 2: quantità delle zone».

### Task 5: Quantità dei locali (pura, con due estrazioni senza cambio di comportamento)

**Files:**
- Modify: `src/bim/refresh.rs` (estrarre da `refresh_rooms` la parte che accoppia, misura e raggruppa i fusi), `src/bim/extract.rs` (estrarre da `project_room_entities` il calcolo dei `Computed`). Create: `src/bim/quantities_rooms.rs`. Modify: `src/bim/mod.rs`.

**Interfaces:**
- Produces:
  - `pub(crate) fn measure_fatto(doc: &Document, unit: DrawingUnit, tol: f64) -> FattoMeasures` in `refresh.rs` con `FattoMeasures { pub rooms: Vec<FattoRoom>, pub walls: WallSet, pub notices: Vec<Notice> }`, `FattoRoom { pub room: Room, pub measured: Option<Measured>, pub fused_with: Vec<String> /* nomi degli altri del gruppo R55; vuoto se solo */ }`. `refresh_rooms` lo usa; **i test esistenti di `refresh.rs` restano il banco di prova e non cambiano**.
  - `pub(crate) fn computed_project_rooms(src: &Document, walls: &WallSet, pg_room_entities: &[(Handle, EntityType)], zones: &[ZoneGeom], unit: DrawingUnit, tol: f64, room_tol: f64) -> (Vec<Computed>, Vec<Notice>, Vec<Room> /* fatto */, Vec<Room> /* semi di progetto */)` in `extract.rs`; `project_room_entities` lo usa.
  - In `quantities_rooms.rs`: `pub struct RoomRow { pub phase: Phase, pub code: String /* LF1, LP1 per ordine */, pub name: String, pub number: String, pub area_m2: Option<f64>, pub perimeter_m: Option<f64> /* anello esterno, isole escluse */, pub height: Height, pub volume_m3: Option<f64>, pub approximate: bool /* !exact */, pub note: Option<String> }`; `pub fn fatto_room_rows(doc: &Document, unit: DrawingUnit, room_tol: f64, default_height: Option<f64>) -> (Vec<RoomRow>, Vec<Notice>)`; `pub fn project_room_rows(doc: &Document, unit: DrawingUnit, tol: f64, room_tol: f64, default_height: Option<f64>, fatto: &[RoomRow]) -> (Vec<ProjectRoomRow>, Vec<Notice>)` con `ProjectRoomRow { pub row: RoomRow, pub fatto_members: Vec<usize> /* indici in fatto, via region_holds sul suo contorno */ }`.
- Regole: fatto non chiuso → area `None`, nota «non chiuso» (e `LocaleNonChiuso`); fusi (R55) → area `None`, nota «fuso con X»; volume = area × H; altezza fatto: `resolve(room.height, default)`; altezza di progetto: `resolve_project_room(seme proprio, [(nome, altezza fatto dei membri)], default)`; perimetro misurato sul contorno a corde (errore < 0,1 mm per segmento, dichiarato nella nota solo se `!exact`).
- Consumes: Task 1, 2; `rooms::{measure_at, project_rooms, region_holds, quality_notices}`, `project::project_geometry`, `walls::{wall_set_without_rooms, sources_of, faces}`.

- [ ] **Step 1: verifica del banco prima di toccare** — `cargo test --locked --lib bim::refresh bim::extract` verdi (riferimento).
- [ ] **Step 2: test (in `quantities_rooms.rs`, fixture `case_a`/`pilastro`/`disturbi` in due forme, metri, default 2,70)** — `fatto_rows_case_a` (A: 12,00 m², perimetro 14,00, volume 32,40; B: 11,70, 13,80, 31,59; altezza `Ereditata`); `fatto_row_own_height` (A con altezza 3,00 `Inserita` → volume 36,00); `open_room_has_no_area` (vano senza chiusura → area `None`, nota «non chiuso»); `fused_rooms_have_no_area` (due semi nella stessa regione); `island_is_subtracted_not_in_perimeter` (caso D: 11,91 m², perimetro 14,00); `project_rows_merge_with_fatto_heights` (caso A demolito: un locale 24,00 m², nome «da confermare», `fatto_members` = i due fatto; altezze 2,70 e 3,00 → `NonDisponibile` + avviso; 3,00 e 3,00 → 3,00 `Inserita`, nota «dal locale di fatto»); `project_row_without_own_seed_takes_paired_fatto_height` (caso L); `hpgaptol_room_is_approximate` (vuoto d'angolo 2 mm chiuso da `room_tol` 5 mm → `approximate == true`).
- [ ] **Step 3:** eseguire le due estrazioni (nessun cambio di comportamento), poi implementare `quantities_rooms.rs`.
- [ ] **Step 4:** `cargo test --locked --lib bim::` — tutto verde, compresi i test di `refresh` ed `extract` senza modifiche.
- [ ] **Step 5: commit** — «BIM parte 2: quantità dei locali, con estrazioni senza cambio di comportamento».

### Task 6: Abaco (assemblaggio, accoppiamento, resa in testo)

**Files:**
- Create: `src/bim/abaco.rs`. Modify: `src/bim/mod.rs` (`pub mod abaco;`, costanti `LAYER_ABACO = "AL-ABACO"`, `APP_ABACO = "ARCHLINE_ABACO"`).

**Interfaces:**
- Produces:
  - `pub struct AbacoOptions { pub unit: DrawingUnit, pub tol: f64, pub room_tol: f64 }`.
  - `pub struct Abaco { pub demolizioni: Vec<ZoneRow>, pub varchi: Vec<ZoneRow>, pub nuove: Vec<ZoneRow>, pub locali: Vec<LocaleLine>, pub notices: Vec<Notice> }` con `LocaleLine { pub fatto: Option<RoomRow>, pub progetto: Option<RoomRow>, pub variation_m2: Option<f64>, pub note: Option<String> }` (accoppiamento: un locale di progetto con esattamente un membro di fatto, che non appartiene ad altri → riga accoppiata con variazione; altrimenti righe separate con nota «da A + B» / «unito in C», nessuna somma implicita).
  - `pub fn compute_abaco(doc: &Document, opts: &AbacoOptions) -> Abaco` (legge `record::read(doc).default_height`, `zones_in`, Task 4 e 5).
  - `pub enum TableKind { Demolizioni, Nuove, Locali }`; `pub struct Cell { pub text: String, pub missing: bool }`; `pub struct TableText { pub kind: TableKind, pub title: String, pub columns: Vec<String>, pub rows: Vec<Vec<Cell>>, pub totals: Vec<Vec<Cell>>, pub notes: Vec<String> }`; `pub fn render_tables(abaco: &Abaco) -> Vec<TableText>` (tre tabelle come nel mockup A approvato; gruppi senza righe → nessuna tabella; Demolizioni include in fondo le righe dei varchi con il loro totale; Nuove con intestazione e totale per gruppo di spessore e colonna «Detrazione vani» = «non calcolata»; `n.d.` con `missing = true`; totale con la dicitura «righe complete» e quante escluse; nota sull'arrotondamento e, se ci sono zone nuove, la nota sugli incroci).
  - `pub fn format_fixed(value: f64, decimals: usize) -> String` (virgola, senza separatore delle migliaia).
- Consumes: Task 2, 4, 5.

- [ ] **Step 1: test (in `abaco.rs`)** — `case_a_demolition_tables` (caso A con zona demolita: tabella Demolizioni con la riga «0,81» e parete «8,10», totale 0,81, nessuna tabella Nuove né Locali di progetto vuota se non ci sono semi); `incomplete_rows_are_flagged_and_excluded_from_totals` (caso H: cella `n.d.` con `missing`, totale 0,00 e «1 riga esclusa»); `new_wall_group_totals` (caso G); `locali_pairing_one_to_one_has_variation` (caso A con una zona nuova 0,10 × 3,00 addossata alla faccia interna ovest di A, rettangolo senza asse: A fatto 12,00, A progetto 11,70, variazione −0,30, riga accoppiata); `locali_merge_has_no_numeric_sum` (A+B fusi: nessuna variazione, nota «da A + B»); `empty_drawing_gives_no_tables`; `computation_matches_refresh_and_extract` (area e nome dei locali di fatto uguali a `refresh_rooms` e a `extract` sullo stesso disegno, R61); `format_fixed_uses_comma` (11,905 → «11,91» con 2 decimali; 0,189 → «0,1890» con 4).
- [ ] **Step 2:** `cargo test --locked --lib bim::abaco` — FAIL.
- [ ] **Step 3:** implementare.
- [ ] **Step 4:** `cargo test --locked --lib bim::` — verdi.
- [ ] **Step 5: commit** — «BIM parte 2: abaco (assemblaggio e resa in testo)».

### Task 7: `AL_ABACO` — tabelle nel DWG

**Files:**
- Create: `src/bim/abaco_table.rs` (scrittura e riconoscimento delle `TABLE`). Modify: `src/bim/mod.rs`, `src/app/commands/bim_abaco.rs` (comando), `src/app/commands/bim.rs` (`dispatch_bim`), `src/bim/zone.rs` (`ensure_layers`/`missing_layers` **non** si toccano: il layer `AL-ABACO` si crea dal comando con la stessa tecnica, in un passo a sé).

**Interfaces:**
- Produces:
  - `abaco_table::tables_in(doc: &Document) -> Vec<(Handle, TableKind)>` (entità `TABLE` dello spazio modello con XDATA `ARCHLINE_ABACO`: chiavi `"tabella"` + `"demolizioni"|"nuove"|"locali"` e versione `Integer16(1)`).
  - `abaco_table::build(text: &TableText, at: [f64; 2], unit: DrawingUnit) -> EntityType` (`codec::entities::TableBuilder::new(rows, cols).at(..).build()`, `set_cell_text`; `n.d.` evidenziato se l'API delle celle lo permette, altrimenti `n.d.` maiuscolo; stile tabella corrente del disegno).
  - `OpenCADStudio::bim_abaco(&mut self, i: usize, cmd: &str) -> Task<Message>` e `AbacoPointCommand` (`CadCommand` che al clic risponde `CmdResult::Relaunch("AL_ABACO <x>,<y>", vec![])`, coordinate con il punto decimale).
- Comportamento: `AL_ABACO [x,y]`. Spazio modello, unità (`bim_unit_or_ask`), `compute_abaco` con `room_tolerance(unit)`. Senza zone né locali: messaggio, nessuna tabella, nessun passo. Tabelle già presenti → ciascuna sostituita **dove si trova** (stesso punto d'inserimento; copie duplicate comprese; una su layer bloccato non si tocca e si conta); mancanti → se ne manca una sola sotto l'ultima esistente, se mancano tutte in `[x,y]`, oppure al clic (interattivo, `AbacoPointCommand`), oppure da `scripted_dispatch` a destra di `scene.model_space_extents()` con margine pari alla larghezza della tabella. Layer `AL-ABACO` creato se manca in un passo `AL_LAYER` a sé (`ColorACI` 7), un solo passo `AL_ABACO` per sostituzioni e creazioni. In riga di comando: quante righe, quante incomplete e perché (`RigaIncompleta`), avvisi di `compute_abaco`. Il rilievo, le zone e i locali non cambiano.
- Consumes: Task 6; `zone::missing_layers`/pattern di `bim_ensure_layers`; `set_entity_xdata`.

- [ ] **Step 1: spike di fattibilità (prima di tutto il resto del task)** — test `table_survives_dwg_roundtrip`: costruire una `TABLE` con celle e XDATA `ARCHLINE_ABACO`, salvarla in DWG e riaprirla con le funzioni di salvataggio e lettura già usate dai test di `extract.rs` (stesso modo di quei test); controllare testo di una cella e XDATA. **Se fallisce**: fermarsi e applicare il ripiego della spec §5.2 (linee e `MTEXT` raggruppati, con lo stesso XDATA su ogni parte per riconoscerli e sostituirli), cambiando `tables_in`/`build` in `tables_in`/`build_group` e dicendolo nel commit; il resto del task non cambia. Registrare l'esito nel messaggio di commit e a Mauro.
- [ ] **Step 2: test (in `bim_tests.rs`)** — `abaco_writes_three_tables_then_replaces_them_in_place` (caso A con una zona demolita e due locali di fatto, `AL_ALTEZZA L 2.70`, `AL_ABACO 20,0`: tabelle Demolizioni e Locali create, nessuna copia al secondo lancio, stesso punto d'inserimento dopo una modifica del rilievo, un passo di annullamento per lancio; un annullamento toglie le tabelle, un secondo toglie il layer); `abaco_without_zones_or_rooms_creates_nothing` ; `abaco_does_not_touch_the_survey` (`rilievo_snapshot` identico, handle di zone e locali identici, `INSUNITS` invariato); `abaco_replaces_every_duplicate_copy` ; `abaco_skips_a_table_on_a_locked_layer_and_counts_it` ; `abaco_recreates_only_the_missing_table_below_the_last` ; `abaco_script_without_point_goes_right_of_the_extents` (`scripted_dispatch = true`, posizione a destra del massimo x del modello); `abaco_asks_the_unit_when_insunits_is_zero` ; `abaco_refuses_paper_space`.
- [ ] **Step 3:** implementare.
- [ ] **Step 4:** `cargo test --locked --lib bim:: app::bim_tests` — verdi.
- [ ] **Step 5: commit** — «BIM parte 2: AL_ABACO, tabelle nel DWG».

### Task 8: `AL_ABACO_CSV`

**Files:**
- Modify: `src/bim/abaco.rs` (`to_csv`), `src/app/commands/bim_abaco.rs` (comando), `src/ui/window/bim_extract.rs` (`BimMsg::AbacoCsvPath(Option<PathBuf>, String)`), `src/app/commands/bim.rs` (`on_bim` ramo nuovo).

**Interfaces:**
- Produces: `abaco::to_csv(abaco: &Abaco) -> String` — intestazione `tipo;codice;descrizione;parti;lunghezza;larghezza;altezza;detrai;quantita;um;stato_altezza;fonte;note`, una riga per misura delle **sole righe complete**, quattro decimali, virgola, BOM UTF-8 (`\u{FEFF}`), CRLF, campi con `;`, `"` o a capo tra virgolette con `""` raddoppiate. Mappatura (spec §5.3): demolizione con asse e varco `1 × L × spessore effettivo × H` (m³); demolizione senza asse `lunghezza` = area in pianta, descrizione «Demolizione area in pianta (zona senza asse)», `larghezza` vuota, quantità = area × H; nuova muratura `1 × L × H` (m², `larghezza` vuota); locale `1 × area` (`lunghezza` = area, m²); `codice` D1/V1/N1/LF1/LP1, `descrizione` = nome del locale o «Demolizione muro spessore 0,100», «Varco», «Nuova muratura spessore 0,080». `OpenCADStudio::bim_abaco_csv(&mut self, i: usize, cmd: &str) -> Task<Message>`.
- Comportamento: `AL_ABACO_CSV [percorso]` (resto della riga, anche tra virgolette). Con percorso: se esiste, **non si scrive nulla** e lo dice; altrimenti scrive (errore: messaggio, nessun file a metà: scrivere su file temporaneo accanto e rinominare). Senza percorso: `scripted_dispatch` → errore «serve il percorso»; interattivo → `crate::sys::file_dialog().save_file()` con `Task::perform`, risposta `BimMsg::AbacoCsvPath`. Righe incomplete: conteggio e motivo in riga di comando.
- Consumes: Task 6; pattern di `table_export.rs` (`Message::TableExportPath`) per il dialogo.

- [ ] **Step 1: test** — in `abaco.rs`: `csv_has_bom_crlf_and_semicolons`; `csv_omits_incomplete_rows` (caso H: solo l'intestazione); `csv_product_equals_quantity` (per ogni riga `parti × lunghezza × larghezza × altezza` con i fattori vuoti saltati = `quantita`, entro 1e-4: casi A, A', B con 2,10, G, demolizione senza asse, locale); `csv_quotes_special_characters` (nome «Bagno; "WC"\nsecondo» → una sola riga logica leggibile con `;` e virgolette); `csv_demolition_without_axis_uses_area_as_length`. In `bim_tests.rs`: `abaco_csv_writes_file_in_a_new_folder` (cartella nuova sotto la temporanea, tolta a fine test); `abaco_csv_never_overwrites_from_script`; `abaco_csv_script_needs_a_path`; `abaco_csv_asks_the_unit_when_insunits_is_zero`.
- [ ] **Step 2:** `cargo test --locked --lib bim::abaco app::bim_tests::abaco_csv` — FAIL.
- [ ] **Step 3:** implementare.
- [ ] **Step 4:** `cargo test --locked --lib bim:: app::bim_tests` — verdi.
- [ ] **Step 5: commit** — «BIM parte 2: AL_ABACO_CSV».

### Task 9: Menu, barra e icone

**Files:**
- Modify: `src/ui/classic_menu.rs` (tre voci in Tools > Ristrutturazione: «Altezza» `AL_ALTEZZA`, «Abaco» `AL_ABACO`, «Abaco CSV» `AL_ABACO_CSV`), `src/ui/toolbar_registry.rs` (test `ristrutturazione_bar_has_the_six_commands` → nove comandi, rinominato `…_has_the_nine_commands`, e `icons.len() == 9`), `src/ui/menu_icons.rs` (tabella `RIBBON`, elenco dei comandi, test). Create: `assets/icons/ristr_altezza.svg`, `ristr_abaco.svg`, `ristr_abaco_csv.svg`.

**Interfaces:** Produces: tre SVG 24×24 allineati alla griglia dei 16 px, disegnati da zero con linee `#B4B6B9` e accenti `#6DB7ED` e **nessun esadecimale della lista fissa di `icons::recolor_semantic_svg`**, niente `white` (se serve un bianco `#FEFEFE`); colori fissi solo come parole chiave (`yellow`, `red`). Idee: altezza = muro in alzato con quota verticale; abaco = tabella a righe; abaco CSV = tabella con freccia verso un foglio. Nessuna icona uguale a un'altra esistente.
- Consumes: —

- [ ] **Step 1: test** — aggiornare i test esistenti di `menu_icons.rs` (ogni comando dell'elenco ha un'icona, vietati gli esadecimali non ammessi nelle icone `ristr_*`, un controllo dei pixel per le nuove: almeno un pixel dell'accento `#6DB7ED`), `toolbar_registry.rs` (nove comandi nell'ordine del menu, nessuna `Glyph`, nessuna icona condivisa) e `classic_menu.rs` (`NO_ICON` resta vuoto). Controllare anche, con `grep -rn "six\|6," src/ui/toolbar_layout.rs`, che nessun conteggio fisso di pulsanti nei test di layout si rompa.
- [ ] **Step 2:** `cargo test --locked --lib classic_menu toolbar_registry menu_icons` — FAIL.
- [ ] **Step 3:** disegnare le icone, registrarle, aggiungere le voci di menu.
- [ ] **Step 4:** i tre filtri verdi, poi `cargo test --locked --lib classic_toolbar`.
- [ ] **Step 5: commit** — «BIM parte 2: voci di menu, barra e tre icone».

### Task 10: Banco BIM Python e controllo a vista

**Files:**
- Modify: `tests/conformance/bim_extract.py` (+ la sua documentazione di testa), `tests/conformance/README.md` se descrive il banco BIM.

**Interfaces:** Consumes: `Session.request({"op": "run", "cmd": ...})`, `Report.check`, `read_dxf` del banco; la cartella di prova senza spazi (`pick_work_base`).

- [ ] **Step 1: aggiungere i controlli** dopo i comandi già eseguiti nel banco (zone create dal caso A): `AL_ALTEZZA L 2.70`, poi `AL_ABACO_CSV <cartella>/abaco.csv`; leggere il CSV con `csv` della libreria standard (`delimiter=";"`, `utf-8-sig`): intestazione attesa; per la zona demolita ad asse esatto `4.05,0 4.05,3` una riga con `quantita` = `0,8100` e `um` = `m³`; `parti × lunghezza × larghezza × altezza` = `quantita` per ogni riga; nessuna riga con altezza vuota. Controllo delle `TABLE` con `ezdxf` **solo se** riconvertendo con `ArchLine --export` la `TABLE` compare nel DXF con le celle; altrimenti stampare «TABLE non verificata (ezdxf): solo CSV» come limite dichiarato, non come FALLITO. Il file di prova resta byte per byte invariato (controllo SHA-256 già nel banco).
- [ ] **Step 2:** `cargo build --release --bin ArchLine` (chiedere di chiudere `ArchLine.exe` se «Accesso negato»), poi `python tests/conformance/bim_extract.py target\release\ArchLine.exe` — tutti i controlli OK.
- [ ] **Step 3: commit** — «BIM parte 2: caso dell'abaco nel banco di conformità».
- [ ] **Step 4: da vedere a schermo da Mauro** (non lo faccio io: trappola 5): tabelle con `n.d.` evidenziato, sostituzione sul posto, CSV in Excel italiano, e **la `TABLE` aperta in AutoCAD vero**.

### Task 11: Estrazione dell'abaco (separabile)

**Files:**
- Modify: `src/bim/extract.rs`; test accanto.

**Interfaces:** Produces: nel solo DWG `DemolizioniCostruzioni`, se il disegno di lavoro ha tabelle d'abaco (`abaco_table::tables_in`), le tre tabelle ricalcolate con `compute_abaco` all'estrazione e convertite in linee e testi ordinari (`LINE` per le griglie, `MTEXT` per le celle, layer `AL-ABACO` o il layer corrente dello stile, **nessun XDATA**, nessun APPID `ARCHLINE_*`), nello stesso punto d'inserimento del disegno di lavoro. Le `TABLE` d'abaco del disegno di lavoro non si copiano e **non** danno più `Notice::NonCopiata`; le altre `TABLE` restano `NonCopiata`. Consumes: Task 6, 7.

- [ ] **Step 1: test (in `extract.rs`)** — `abaco_tables_are_not_reported_as_not_copied`; `extract_writes_fresh_abaco_as_lines_and_texts_in_demolizioni_only` (dopo aver cambiato il rilievo senza rilanciare `AL_ABACO` l'estratto ha i numeri nuovi, non quelli della tabella vecchia; nessun XDATA né APPID `ARCHLINE_*` nell'estratto; nessuna tabella nei DWG di fatto e di progetto); `other_tables_are_still_not_copied`.
- [ ] **Step 2:** `cargo test --locked --lib bim::extract::tests::abaco` — FAIL.
- [ ] **Step 3:** implementare. **Se Mauro preferisce rinviare questo passo**, fermarsi dopo il solo silenzio sull'avviso e dirlo.
- [ ] **Step 4:** `cargo test --locked --lib bim::` e `python tests/conformance/bim_extract.py target\release\ArchLine.exe`.
- [ ] **Step 5: commit** — «BIM parte 2: l'abaco nell'estrazione».

### Task 12: Verifica finale e documenti

**Files:** Modify: `CLAUDE.md` (sezione BIM: parte 2, file nuovi, comandi, limiti, scelte della spec §9), `docs/superpowers/ledgers/` (nuovo `2026-10-28-bim-abachi-quantita.md` con le decisioni R70… in forma di tabella costo-se-sbagliata, come quello della parte 1, solo per le scelte non ovvie).

- [ ] **Step 1:** `cargo check --locked`, `cargo test --locked --lib bim::`, `cargo test --locked --lib app::bim_tests`, `cargo test --locked --lib` intero (i cinque test instabili elencati in CLAUDE.md possono fallire in parallelo: rilanciarli da soli e dirlo), `cargo build --release --bin ArchLine`, `python -m unittest discover -s tests/conformance -p "test_*.py"`, `python tests/conformance/bim_extract.py target\release\ArchLine.exe`.
- [ ] **Step 2:** aggiornare `CLAUDE.md` e il registro; ricordare a Mauro i tre controlli aperti (DWG estratti e TABLE in AutoCAD vero, MSI su macchina vera, ricerca di marchio su ARCHLine.XP) e che nessun caso reale è stato provato.
- [ ] **Step 3: commit** — «BIM parte 2: documenti e verifica finale». Nessun push senza conferma.

---

## Self-review (copertura della spec)

- §4.1 altezza a tre livelli e varco senza ereditarietà → Task 1, 2; fusione e locale di progetto → Task 2, 5; valori digitati e avviso → Task 3.
- §4.2 riga e stati → Task 4, 5, 6. §4.3 zone, spessore effettivo, sovrapposizioni, raggruppamenti → Task 4; locali e accoppiamento → Task 5, 6.
- §5.1 `AL_ALTEZZA` → Task 3. §5.2 `AL_ABACO` (sostituzione, punto, fattibilità `TABLE`, ripiego) → Task 7. §5.3 CSV → Task 8. §5.4 estrazione → Task 11.
- §6 interfaccia e icone → Task 9. §7 casi A, A', B, G, H, I, J, K, L e «script» → test dei Task 3–8 (caso J in Task 3; K in Task 4; L in Task 5; I in Task 5/6); banco Python → Task 10; «limite della verifica» → Task 12.
- §3 vincoli (nessuna dipendenza, tolleranze come parte 1, CI spenta) → Global Constraints.
- Non coperti di proposito: prezzi, import in ArchCme, `.xlsx`, parete dei locali, oggetti 3D (spec §2).
