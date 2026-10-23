# BIM parte 1 — stato e piante di ristrutturazione: piano di implementazione

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** zone demolite (gialle) e nuove (rosse) sul rilievo DWG a due linee, locali con superficie netta per fase, estrazione di tre DWG 2D ordinari (stato di fatto, demolizioni e costruzioni, stato di progetto).

**Architecture:** modulo proprio `src/bim/` con logica pura e testabile senza interfaccia (unità, zone, muri, taglio, geometria di progetto, locali, estrazione) e comandi interattivi `CadCommand`; il lato app sta in un'unica famiglia `src/app/commands/bim.rs` (i campi di `OpenCADStudio` sono visibili solo dentro `crate::app`), che prepara le istantanee della geometria e avvia i comandi. I dati vivono nel DWG: zone = `HATCH` solide con XDATA `ARCHLINE_ZONA`, locali = polilinea + testo con XDATA `ARCHLINE_LOCALE`, impostazioni nel record `ARCHLINE_PROGETTO` del dizionario degli oggetti con nome.

**Tech Stack:** Rust; codec `acadrust` (importato come `codec`); `kernel::geom2d` (crate `opencadkernel`, già dipendenza); `rustc_hash`; `inventory` per la registrazione dei comandi; iced per la finestra di estrazione.

**Spec:** `docs/superpowers/specs/2026-10-23-bim-stato-piante-design.md` (approvata da Mauro il 2026-10-23). Chi esegue legge spec e piano insieme.

## Global Constraints

- Nessuna dipendenza da altri software e **nessuna crate nuova** in `Cargo.toml`: bastano `kernel::geom2d`, `glam`, `rustc_hash` già presenti.
- File nuovi: `src/bim/*`, `src/app/commands/bim.rs`, `src/app/bim_tests.rs`, `src/ui/window/bim_extract.rs`, `tests/conformance/bim_extract.py`. File di upstream toccati solo dove un task lo dice (agganci di una o poche righe).
- Layer: `AL-DEMOLIZIONI` (ACI 2, giallo), `AL-NUOVE` (ACI 1, rosso), `AL-LOCALI`, `AL-LOCALI-PROGETTO` (ACI 7). Ogni layer che inizia con `AL-` è escluso dalla geometria dei muri.
- APPID: `ARCHLINE_ZONA`, `ARCHLINE_LOCALE`. Record del disegno: `ARCHLINE_PROGETTO` (XRecord nel dizionario radice).
- Comandi: `AL_DEMOLISCI`, `AL_NUOVO`, `AL_STATO`, `AL_LOCALE`, `AL_AGGIORNA`, `AL_ESTRAI`; interno `AL_UNITA`. Tutti registrati con `inventory::submit!(crate::command::CommandRegistration { names: &[...] })`.
- Muri = solo `LINE`, `ARC`, `LWPOLYLINE`, `POLYLINE` 2D (`EntityType::Polyline2D`) dello spazio modello, su layer **accesi e non congelati** (i bloccati contano), esclusi i layer `AL-*`.
- Le entità del rilievo non si modificano mai: stesso handle, geometria e attributi dopo qualunque comando.
- `INSUNITS` (`doc.header.insertion_units`) **non si scrive mai**. Supportati 4 = mm, 5 = cm, 6 = m. Con 0 si chiede l'unità e la risposta va nel record; con ogni altro codice si rifiuta.
- Tolleranza geometrica: `1.0e-6` m espressa in unità del disegno (`DrawingUnit::tolerance()`); per i locali `max(tolleranza, HPGAPTOL)`.
- Aree in m² con virgola decimale e due decimali: `12,00 m²`.
- DWG estratti: solo spazio modello, nessun XDATA e nessun APPID `ARCHLINE_*`, versione del disegno di partenza, nomi `<nome>_stato-di-fatto.dwg`, `<nome>_demolizioni-costruzioni.dwg`, `<nome>_stato-di-progetto.dwg`, mai sovrascritti senza conferma.
- Nessun valore inventato: spessore non trovato → si chiede; nomi dei locali solo quelli di Mauro o la loro unione meccanica con « + ».
- Verifica solo in locale (CI spenta): `cargo check --locked` e `cargo test --locked --lib <filtro>`. Test lib instabili già noti in `CLAUDE.md`: se falliscono in parallelo, rilanciarli da soli.
- Trappole: nessuna logica dentro `view_main` / `start_page_content`; righe dei menu contestuali con `mouse_area`, mai `button`; `RUST_MIN_STACK` resta a 64 MiB.
- Commit in italiano, senza righe di attribuzione. Niente push senza conferma di Mauro.

## Review Focus

1. **Entità fuori dal piano XY** (z ≠ 0) nel rilievo: si ignorano e il conteggio esce in un avviso, mai in silenzio. Test in Task 3.
2. **Linee doppie sovrapposte**, frequenti nei rilievi: superfici invariate. Test in Task 3.
3. **Piccoli vuoti agli angoli** (2 mm): il locale risulta «non chiuso» con messaggio; con `HPGAPTOL` 5 mm si chiude e misura giusto. Test in Task 13 e Task 14.
4. **Muri su layer bloccato:** contano come muri. Test in Task 3.
5. **Disegno in pollici** (`INSUNITS` = 1): rifiuto con messaggio, nessuna zona creata. Test in Task 6.

---

## PR 1 — Zone

### Task 1: Fondamenta (unità, record del disegno, avvisi)

**Files:**
- Create: `src/bim/mod.rs`, `src/bim/units.rs`, `src/bim/record.rs`, `src/bim/report.rs`
- Modify: `src/lib.rs` (aggiungere `pub mod bim;` nell'elenco dei moduli, righe 4-40)

**Interfaces:**
- Produces, in `src/bim/mod.rs`: `pub const LAYER_DEMOLIZIONI: &str = "AL-DEMOLIZIONI"; LAYER_NUOVE = "AL-NUOVE"; LAYER_LOCALI = "AL-LOCALI"; LAYER_LOCALI_PROGETTO = "AL-LOCALI-PROGETTO"; LAYER_PREFIX = "AL-"; APP_ZONA = "ARCHLINE_ZONA"; APP_LOCALE = "ARCHLINE_LOCALE"; RECORD_PROGETTO = "ARCHLINE_PROGETTO"; ACI_GIALLO: i16 = 2; ACI_ROSSO: i16 = 1; MAX_WALL_THICKNESS_M: f64 = 1.5; BASE_TOLERANCE_M: f64 = 1.0e-6;` più le dichiarazioni dei sottomoduli (aggiungerle task per task) e `#[cfg(test)] pub(crate) mod fixtures;` (Task 3).
- Produces, in `units.rs`: `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum DrawingUnit { Millimetri, Centimetri, Metri }` con `from_insunits(i16) -> Option<Self>`, `insunits(self) -> i16`, `per_metre(self) -> f64` (1000 / 100 / 1), `tolerance(self) -> f64` (= `BASE_TOLERANCE_M * per_metre`), `from_keyword(&str) -> Option<Self>` ("MM" / "CM" / "M", maiuscole o minuscole), `metres(self, m: f64) -> f64` (metri → unità del disegno). Inoltre `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum UnitLookup { Known(DrawingUnit), Unspecified, Unsupported(i16) }`, `pub fn resolve(doc: &codec::CadDocument) -> UnitLookup`, `pub fn area_m2(area: f64, unit: DrawingUnit) -> f64` e `pub fn format_m2(m2: f64) -> String`.
- Produces, in `record.rs`: `#[derive(Clone, Debug, Default, PartialEq)] pub struct ProjectRecord { pub unit: Option<DrawingUnit>, pub new_walls_layer: Option<String> }`, `pub fn read(doc: &CadDocument) -> ProjectRecord`, `pub fn write(doc: &mut CadDocument, rec: &ProjectRecord)`.
- Produces, in `report.rs`: `#[derive(Clone, Debug, PartialEq)] pub enum Notice` con le varianti elencate allo Step 5, e `impl Notice { pub fn text(&self) -> String }`.

- [ ] **Step 1: Write the failing tests** (in `#[cfg(test)] mod tests` dei rispettivi file)

```rust
// units.rs
#[test] fn insunits_codes() {
    assert_eq!(DrawingUnit::from_insunits(4), Some(DrawingUnit::Millimetri));
    assert_eq!(DrawingUnit::from_insunits(5), Some(DrawingUnit::Centimetri));
    assert_eq!(DrawingUnit::from_insunits(6), Some(DrawingUnit::Metri));
    assert_eq!(DrawingUnit::from_insunits(1), None);
}
#[test] fn resolve_reads_insunits_then_record() {
    let mut doc = codec::CadDocument::new();
    doc.header.insertion_units = 0;
    assert_eq!(resolve(&doc), UnitLookup::Unspecified);
    crate::bim::record::write(&mut doc, &ProjectRecord { unit: Some(DrawingUnit::Centimetri), ..Default::default() });
    assert_eq!(resolve(&doc), UnitLookup::Known(DrawingUnit::Centimetri));
    assert_eq!(doc.header.insertion_units, 0, "INSUNITS non si scrive mai");
    doc.header.insertion_units = 6;
    assert_eq!(resolve(&doc), UnitLookup::Known(DrawingUnit::Metri));
    doc.header.insertion_units = 1;
    assert_eq!(resolve(&doc), UnitLookup::Unsupported(1));
}
#[test] fn areas_and_format() {
    assert!((area_m2(120_000.0, DrawingUnit::Centimetri) - 12.0).abs() < 1e-12);
    assert_eq!(format_m2(12.0), "12,00 m²");
    assert_eq!(format_m2(0.09), "0,09 m²");
    assert_eq!(format_m2(11.905), "11,91 m²");
}
// record.rs
#[test] fn record_roundtrip_and_update() {
    let mut doc = codec::CadDocument::new();
    assert_eq!(read(&doc), ProjectRecord::default());
    write(&mut doc, &ProjectRecord { unit: Some(DrawingUnit::Metri), new_walls_layer: Some("MURI".into()) });
    write(&mut doc, &ProjectRecord { unit: Some(DrawingUnit::Centimetri), new_walls_layer: Some("MURI".into()) });
    assert_eq!(read(&doc), ProjectRecord { unit: Some(DrawingUnit::Centimetri), new_walls_layer: Some("MURI".into()) });
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --locked --lib bim::` — Expected: errore di compilazione (modulo `bim` assente).

- [ ] **Step 3: Implement units and record**

`resolve`: se `insertion_units` vale 4/5/6 → `Known`; se vale 0 → l'unità del record, se c'è, altrimenti `Unspecified`; altrimenti `Unsupported(code)`. `format_m2`: arrotondamento a due decimali, separatore decimale `,`, suffisso ` m²`.

Il record è un `XRecord` con chiave `ARCHLINE_PROGETTO` direttamente nel dizionario radice (`doc.header.named_objects_dict_handle`). Voci: codice 90 `Int32` versione 1; codice 70 `Int16` codice INSUNITS dell'unità (0 = nessuna); codice 8 `String` layer dei muri nuovi ("" = nessuno). Per creare e aggiornare si segue la sequenza di `HostSession::create_xrecord` e `update_xrecord` (`src/app/plugin_host.rs:2960-3050` e `3085-3116`): `allocate_handle`, `owner` = dizionario radice, `objects.insert(h, ObjectType::XRecord(..))`, `add_entry`; nell'aggiornamento sostituire le voci e svuotare i campi grezzi come fa `update_xrecord`. Non usare `ensure_xrecord` / `xrecord_mut` del codec, che puntano al dizionario di estensione di un oggetto.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked --lib bim::` — Expected: PASS.

- [ ] **Step 5: Implement `report.rs`** (non ha test propri: lo provano i task che lo usano)

Varianti, con il testo esatto di `text()` (coordinate con due decimali, in unità del disegno; aree già in m²):

| Variante | `text()` |
|---|---|
| `FuoriPiano { count: usize }` | `"{count} entità fuori dal piano XY ignorate."` |
| `ZonaSenzaMuri` | `"La zona demolita non incrocia nessuna linea del rilievo."` |
| `NuovoSuEsistente` | `"La zona nuova si sovrappone a un muro esistente non demolito."` |
| `StessoStatoSovrapposte` | `"La zona si sovrappone in parte a un'altra zona dello stesso stato."` |
| `ConfiniNonRimossi { at: [f64; 2] }` | `"Zona senza asse in ({x}; {y}): i confini non sono stati rimossi."` |
| `GiunzioneDaVerificare { at }` | `"Giunzione da verificare in ({x}; {y})."` |
| `TaglioNonChiuso { at }` | `"Taglio non chiuso in ({x}; {y})."` |
| `NuoveSovrapposte { at }` | `"Zone nuove sovrapposte solo in parte in ({x}; {y})."` |
| `VarcoNonSupportato { at }` | `"Varco non supportato in ({x}; {y}): serve un rettangolo che attraversi il muro."` |
| `NonTagliata { handle: Handle, kind: String }` | `"{kind} {handle:X} attraversa una zona demolita: lasciata intera."` |
| `Esclusa { handle, kind }` | `"{kind} {handle:X} dentro una zona demolita: esclusa dal progetto."` |
| `NonCopiata { handle, kind }` | `"{kind} {handle:X} non copiata nel DWG estratto."` |
| `LocaleFuso { names: Vec<String>, area_m2: f64 }` | `"Locali {a + b} fusi in uno ({area}): nome da confermare."` |
| `LocaleSenzaNome { area_m2 }` | `"Locale senza nome ({area})."` |
| `SemeInMuroNuovo { name: String }` | `"Il locale di progetto {name} cade dentro un muro nuovo."` |
| `LocaleNonChiuso { name }` | `"Il locale {name} non è più chiuso: lasciato com'era."` |
| `Isole { name, count: usize }` | `"Locale {name}: {count} isole sottratte."` |
| `Varco { area_m2 }` | `"Varco {area} escluso dai locali."` |
| `ContornoApprossimato { name }` | `"Locale {name}: contorno curvo approssimato."` |

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/bim
git commit -m "BIM: modulo bim, unità del disegno, record ARCHLINE_PROGETTO, avvisi"
```

### Task 2: Zona (modello, XDATA, campitura, layer)

**Files:**
- Create: `src/bim/zone.rs`
- Modify: `src/bim/mod.rs` (`pub mod zone;`)

**Interfaces:**
- Consumes: costanti di `bim::mod`.
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum ZoneState { Demolito, Nuovo }`
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum ZoneOrigin { MuroDuePunti, LineaSpessore, PuntoInterno, Rettangolo, Poligono, PolilineaEsistente }`, con testi `muro-due-punti`, `linea-spessore`, `punto-interno`, `rettangolo`, `poligono`, `polilinea-esistente`
  - `#[derive(Clone, Debug, PartialEq)] pub struct Zone { pub state: ZoneState, pub origin: ZoneOrigin, pub axis: Option<([f64; 2], [f64; 2])>, pub thickness: Option<f64>, pub height: Option<f64>, pub varco: bool }`
  - `#[derive(Clone, Debug)] pub struct ZoneGeom { pub handle: codec::Handle, pub zone: Zone, pub rings: Vec<Vec<[f64; 2]>> }`, dove `rings[0]` è l'esterno e i successivi sono buchi
  - `pub fn encode(zone: &Zone) -> Vec<XDataValue>` e `pub fn decode(rec: &ExtendedDataRecord) -> Option<Zone>`
  - `pub fn zone_hatch(zone: &Zone, rings: &[Vec<[f64; 2]>]) -> EntityType`
  - `pub fn zone_hatch_from_paths(zone: &Zone, paths: Vec<codec::entities::BoundaryPath>) -> EntityType`
  - `pub fn read_zone(handle: Handle, entity: &EntityType) -> Option<ZoneGeom>`
  - `pub fn zones_in(doc: &CadDocument) -> Vec<ZoneGeom>` (solo spazio modello)
  - `pub fn restyle(entity: &mut EntityType, zone: &Zone)`: layer, colore e XDATA secondo lo stato
  - `pub fn missing_layers(doc: &CadDocument) -> bool` e `pub fn ensure_layers(doc: &mut CadDocument)`

Sequenza XDATA (esatta, in quest'ordine; `decode` legge per chiave e salta le chiavi sconosciute):
`Integer16(1)`, `String("stato")`, `String("demolito"|"nuovo")`, `String("origine")`, `String(<origine>)`, `String("varco")`, `Integer16(0|1)`, poi le chiavi facoltative presenti: `String("asse"), Point3D(p1), Point3D(p2)`; `String("spessore"), Real(t)`; `String("altezza"), Real(h)`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn xdata_roundtrip_all_fields() {
    let z = Zone { state: ZoneState::Demolito, origin: ZoneOrigin::MuroDuePunti,
        axis: Some(([4.05, 0.0], [4.05, 3.0])), thickness: Some(0.10), height: Some(2.70), varco: true };
    let mut rec = ExtendedDataRecord::new(APP_ZONA);
    for v in encode(&z) { rec.add_value(v); }
    assert_eq!(decode(&rec), Some(z));
}
#[test] fn hatch_is_solid_on_state_layer_with_aci() {
    let ring = vec![[4.0, 0.0], [4.1, 0.0], [4.1, 3.0], [4.0, 3.0]];
    let z = Zone { state: ZoneState::Demolito, origin: ZoneOrigin::Rettangolo, axis: None, thickness: None, height: None, varco: false };
    let EntityType::Hatch(h) = zone_hatch(&z, &[ring.clone()]) else { panic!() };
    assert!(h.is_solid);
    assert_eq!(h.common.layer, LAYER_DEMOLIZIONI);
    assert_eq!(h.common.color, codec::Color::from_index(ACI_GIALLO));
    assert!((crate::entities::hatch::boundary_area(&h) - 0.30).abs() < 1e-9);
    let n = Zone { state: ZoneState::Nuovo, ..z };
    let EntityType::Hatch(h) = zone_hatch(&n, &[ring]) else { panic!() };
    assert_eq!((h.common.layer.as_str(), h.common.color), (LAYER_NUOVE, codec::Color::from_index(ACI_ROSSO)));
}
#[test] fn ensure_layers_creates_once_with_colors() {
    let mut doc = codec::CadDocument::new();
    assert!(missing_layers(&doc));
    ensure_layers(&mut doc);
    assert!(!missing_layers(&doc));
    assert_eq!(doc.layers.get(LAYER_DEMOLIZIONI).unwrap().color, codec::Color::from_index(2));
    assert_eq!(doc.layers.get(LAYER_NUOVE).unwrap().color, codec::Color::from_index(1));
    let n = doc.layers.iter().count();
    ensure_layers(&mut doc);
    assert_eq!(doc.layers.iter().count(), n);
}
#[test] fn read_zone_roundtrip_through_document() { /* add zone_hatch to a CadDocument, zones_in returns 1 ZoneGeom with the same Zone and a 4-point outer ring */ }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --locked --lib bim::zone` — Expected: FAIL (le funzioni non esistono).

- [ ] **Step 3: Implement `zone.rs`**

Campitura: `codec::entities::Hatch::new()` (è già solida). Anello esterno con flag `OUTERMOST | EXTERNAL`, buchi con `DEFAULT`, come `src/scene/entity.rs:3106-3130`. Ogni anello diventa `BoundaryEdge::Polyline(PolylineEdge::new(ring, true))`. Il record XDATA si aggiunge a `common.extended_data`. Il layer va creato con `allocate_handle()` (modello: `src/app/commands/count.rs:475-483`); senza handle si perde al salvataggio DWG. Gli anelli in lettura si ottengono con `crate::scene::hatch_boundary_rings`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked --lib bim::zone` — Expected: PASS.

- [ ] **Step 5: Commit** — `git commit -m "BIM: zona come HATCH solida con XDATA ARCHLINE_ZONA e layer AL-*"`

### Task 3: Muri, facce, locali e superficie netta (con disegni di prova)

**Files:**
- Create: `src/bim/walls.rs`, `src/bim/fixtures.rs` (`#[cfg(test)]`)
- Modify: `src/scene/boundary.rs:53` e `:79` (`fn entity_curves_on_plane` e `fn entity_boundary_source_on_plane` diventano `pub(crate)`); `src/scene/mod.rs:85-90` (riesportare le due funzioni accanto a `boundary_faces`); `src/bim/mod.rs`

**Interfaces:**
- Consumes: `crate::scene::{BoundarySource, boundary_faces, exact_hatch_paths, entity_boundary_source_on_plane}`, `crate::entities::hatch::boundary_area`, `kernel::geom2d::{contains, signed_area, Curve, Line, Tolerance}`, `crate::command::WorkingPlane::default()` (sempre piano XY del mondo).
- Produces:
  - `pub struct WallSet { pub entities: Vec<(Handle, EntityType)>, pub sources: FxHashMap<Handle, BoundarySource>, pub off_plane: usize }`
  - `pub fn is_wall_entity(doc: &CadDocument, entity: &EntityType) -> bool`
  - `pub fn wall_set(doc: &CadDocument, tol: f64) -> WallSet`
  - `pub fn sources_of(entities: &[(Handle, EntityType)], tol: f64) -> (FxHashMap<Handle, BoundarySource>, usize)` (il secondo valore conta le entità fuori piano)
  - `pub fn faces(sources: &FxHashMap<Handle, BoundarySource>, tol: f64) -> Vec<Vec<[f64; 2]>>`
  - `#[derive(Clone, Debug, PartialEq)] pub struct Region { pub outer: Vec<[f64; 2]>, pub holes: Vec<Vec<[f64; 2]>> }`
  - `pub fn region_at(faces: &[Vec<[f64; 2]>], p: [f64; 2], tol: f64) -> Option<Region>`
  - `pub fn net_area(region: &Region, sources: &FxHashMap<Handle, BoundarySource>, tol: f64) -> (f64, bool)`, dove il booleano vale `true` se l'area è esatta
- Produces (fixtures):
  - `pub(crate) enum Form { Continua, ChiusaPerMuro }`
  - `pub(crate) fn case_a(form: Form, unit: DrawingUnit) -> Vec<EntityType>`
  - `pub(crate) fn pilastro(unit: DrawingUnit) -> EntityType`
  - `pub(crate) fn disturbi(unit: DrawingUnit) -> Vec<EntityType>`
  - `pub(crate) fn scene_with(entities: Vec<EntityType>, insunits: i16) -> crate::scene::Scene`

**Disegno di prova, caso A** (coordinate in metri, moltiplicate per `unit.per_metre()`; muri sul layer `MURI`):
- **Form::Continua**, tutte `LINE`:
  - contorno esterno: (-0,30; -0,30) – (8,30; -0,30) – (8,30; 3,30) – (-0,30; 3,30);
  - locale A: (0; 0) – (4,00; 0) – (4,00; 3,00) – (0; 3,00);
  - locale B: (4,10; 0) – (8,00; 0) – (8,00; 3,00) – (4,10; 3,00).
- **Form::ChiusaPerMuro**, una `LWPOLYLINE` chiusa per muro:
  - sud: [(-0,30; -0,30), (8,30; -0,30), (8,30; 0), (-0,30; 0)];
  - nord: [(-0,30; 3,00), (8,30; 3,00), (8,30; 3,30), (-0,30; 3,30)];
  - ovest: [(-0,30; 0), (0; 0), (0; 3,00), (-0,30; 3,00)];
  - est: [(8,00; 0), (8,30; 0), (8,30; 3,00), (8,00; 3,00)];
  - tramezzo: [(4,00; 0), (4,10; 0), (4,10; 3,00), (4,00; 3,00)].
- `pilastro`: `LWPOLYLINE` chiusa [1,00 – 1,30]² sul layer `PILASTRI`.
- `disturbi`: un `TEXT` e un `MTEXT` dentro A, una campitura solida dentro A sul layer `ARREDI`, e una polilinea chiusa [0,5 – 3,5]×[0,5 – 2,5] sul layer `AL-LOCALI`.

- [ ] **Step 1: Write the failing tests** (in `walls.rs`; l'helper `area_at(entities, insunits, p) -> f64` costruisce scena, insieme dei muri, facce, regione e area in m²)

```rust
#[test] fn case_a_rooms_both_forms() {
    for form in [Form::Continua, Form::ChiusaPerMuro] {
        assert!((area_at(case_a(form, DrawingUnit::Metri), 6, [2.0, 1.5]) - 12.00).abs() < 1e-9);
        assert!((area_at(case_a(form, DrawingUnit::Metri), 6, [6.0, 1.5]) - 11.70).abs() < 1e-9);
    }
}
#[test] fn case_c_centimetres_same_m2() {
    assert!((area_at(case_a(Form::Continua, DrawingUnit::Centimetri), 5, [200.0, 150.0]) - 12.00).abs() < 1e-9);
}
#[test] fn case_d_island_and_layer_off() {
    let mut e = case_a(Form::Continua, DrawingUnit::Metri); e.push(pilastro(DrawingUnit::Metri));
    assert!((area_at(e.clone(), 6, [2.0, 2.5]) - 11.91).abs() < 1e-9);
    // stesso disegno con il layer PILASTRI spento: 12,00
}
#[test] fn case_f_disturbances_do_not_change_area() { /* case_a + disturbi → 12.00 */ }
#[test] fn wall_filter_types_and_layers() { /* TEXT, MTEXT, HATCH, layer AL-LOCALI: false; LINE su MURI: true; LINE su layer congelato: false; LINE su layer bloccato: true */ }
// Review Focus 1, 2, 4
#[test] fn off_plane_entities_are_counted_not_used() { /* LINE a z = 1.0 → off_plane == 1; area di A invariata 12.00 */ }
#[test] fn duplicate_lines_do_not_change_area() { /* case_a + una copia identica della linea (4,00; 0)-(4,00; 3,00) → 12.00 */ }
#[test] fn locked_wall_layer_still_bounds() { /* layer MURI bloccato → 12.00 */ }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --locked --lib bim::walls` — Expected: FAIL.

- [ ] **Step 3: Implement `walls.rs` and `fixtures.rs`**

- `is_wall_entity`: tipo nell'elenco, `owner` = blocco dello spazio modello, `layer.is_visible()`, nome che non inizia con `AL-`.
- `sources_of`: chiama `entity_boundary_source_on_plane(entity, WorkingPlane::default(), tol, None)`; `None` per una curva conta come fuori piano.
- `region_at`: sceglie la faccia più piccola che contiene `p` (`contains` con le facce come `Curve::Line`, area minima con `signed_area().abs()`). I buchi sono i figli diretti: facce dentro l'esterno, che non contengono `p`, senza nessuna faccia intermedia. È la stessa regola di `resolve_hatch_rings` (`src/modules/draw/draw/hatch.rs:107-148`), che è privata e va riscritta qui.
- `net_area`: costruisce una `Hatch` con `exact_hatch_paths(&rings, &exterior, sources, tol)` e misura con `boundary_area`. Se il risultato è vuoto o non finito, ripiega sulla formula del laccio applicata agli anelli e restituisce `false`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked --lib bim::walls` — Expected: PASS. Run also `cargo test --locked --lib scene::boundary` — Expected: PASS (la sola visibilità è cambiata).

- [ ] **Step 5: Commit** — `git commit -m "BIM: geometria dei muri filtrata, locali e superficie netta esatta; disegni di prova"`

### Task 4: Muro tra due punti (asse → rettangolo)

**Files:**
- Modify: `src/bim/walls.rs`

**Interfaces:**
- Produces:
  - `#[derive(Clone, Debug, PartialEq)] pub struct WallRect { pub corners: [[f64; 2]; 4], pub axis: ([f64; 2], [f64; 2]), pub thickness: f64 }`
  - `impl WallRect { pub fn ring(&self) -> Vec<[f64; 2]>; pub fn long_sides(&self) -> [([f64; 2], [f64; 2]); 2]; pub fn end_sides(&self) -> [([f64; 2], [f64; 2]); 2] }`
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum AxisMiss { Degenerate, NoFaces, OneFace }`
  - `pub fn rect_from_axis(sources: &FxHashMap<Handle, BoundarySource>, p1: [f64; 2], p2: [f64; 2], max_thickness: f64, tol: f64) -> Result<WallRect, AxisMiss>`
  - `pub fn rect_from_axis_thickness(p1: [f64; 2], p2: [f64; 2], thickness: f64) -> Option<WallRect>`

Definizioni (decidono la geometria, vanno seguite):
- `d = (p2 − p1) / |p2 − p1|`; `n = (−d.y, d.x)` è la normale sinistra.
- Un segmento delle sorgenti è un candidato se è parallelo a `d` entro 0,5° e la sua proiezione su `d` copre più di `tol` dell'intervallo `(0, |p2 − p1|)`. Il suo scostamento è `s = (seg.start − p1) · n`.
- Faccia sinistra: il candidato con `s > tol` più vicino a 0. Faccia destra: il candidato con `s < −tol` più vicino a 0. Se manca un lato ma c'è un candidato con `|s| ≤ tol` (l'utente ha cliccato sulla faccia stessa), quel candidato fa da lato mancante.
- Una faccia vale solo con `|s| ≤ max_thickness`, e lo spessore `s_l − s_r` deve essere `≤ max_thickness`.
- Angoli: `c0 = p1 + n·s_r`, `c1 = p2 + n·s_r`, `c2 = p2 + n·s_l`, `c3 = p1 + n·s_l`. L'anello `c0..c3` gira in senso antiorario.
- Lati lunghi: `(c0, c1)` e `(c2, c3)`. Lati di estremità: `(c1, c2)` e `(c3, c0)`.
- `axis` è la mezzeria: `p + n·(s_r + s_l)/2`.
- `rect_from_axis_thickness` centra lo spessore sull'asse.

- [ ] **Step 1: Write the failing tests** (helper `sources_for(entities) -> (FxHashMap<Handle, BoundarySource>, f64)` = `wall_set` su `fixtures::scene_with(entities, 6)` con tolleranza `1e-6`, che restituisce le sorgenti e la tolleranza)

```rust
#[test] fn axis_on_tramezzo_finds_both_faces() {
    let (s, tol) = sources_for(case_a(Form::Continua, DrawingUnit::Metri));
    let r = rect_from_axis(&s, [4.05, 0.0], [4.05, 3.0], 1.5, tol).unwrap();
    assert!((r.thickness - 0.10).abs() < 1e-9);
    let xs: Vec<f64> = r.corners.iter().map(|c| c[0]).collect();
    assert!(xs.iter().all(|x| (x - 4.0).abs() < 1e-9 || (x - 4.1).abs() < 1e-9));
    assert!((kernel::geom2d::signed_area(&r.ring()) - 0.30).abs() < 1e-9);
}
#[test] fn axis_off_centre_and_on_a_face() {
    // (4,02; 0,5)–(4,02; 2,5): stesse facce, y da 0,5 a 2,5, area 0,20
    // (4,00; 0)–(4,00; 3,00), sulla faccia sinistra: spessore 0,10
}
#[test] fn axis_without_faces() {
    // asse in mezzo al locale A, (2; 0,5)–(2; 2,5), max 1,5: le facce più vicine (x = 0 e x = 4,00) distano 2,00 → Err(NoFaces)
    // asse a 0,05 dalla faccia x = 0 dentro A, max 0,10: un solo lato entro il limite → Err(OneFace)
    // p1 == p2 → Err(Degenerate)
}
```

- [ ] **Step 2: Run the tests to verify they fail** — `cargo test --locked --lib bim::walls::` → FAIL.
- [ ] **Step 3: Implement as defined above.**
- [ ] **Step 4: Run the tests to verify they pass** — PASS.
- [ ] **Step 5: Commit** — `git commit -m "BIM: muro tra due punti, facce cercate ai lati dell'asse"`

### Task 5: Comando zona (`ZoneCommand`)

**Files:**
- Create: `src/bim/commands.rs`
- Modify: `src/bim/mod.rs`

**Interfaces:**
- Consumes: Task 2 (`Zone`, `zone_hatch`, `zone_hatch_from_paths`), Task 3 (`faces`, `region_at`), Task 4 (`rect_from_axis`, `rect_from_axis_thickness`), `crate::command::{CadCommand, CmdResult, CmdOption, InputKind, WorkingPlane}`.
- Produces:
  - `pub struct ZoneSnapshot { pub walls: FxHashMap<Handle, BoundarySource>, pub faces: Vec<Vec<[f64; 2]>>, pub zones: Vec<ZoneGeom>, pub unit: DrawingUnit, pub tol: f64, pub last_thickness: Option<f64> }`
  - `pub struct ZoneCommand` con `pub fn new(state: ZoneState, snap: ZoneSnapshot) -> Self`
  - `pub fn validate(candidate: &[Vec<[f64; 2]>], state: ZoneState, snap: &ZoneSnapshot) -> Vec<Notice>`

Comportamento (prompt esatti):

| Passo | Prompt | Input |
|---|---|---|
| iniziale demolisci | `AL_DEMOLISCI  Primo punto sull'asse del muro o [Punto interno/Rettangolo/Poligono/Selezione]:` | punto, o parola chiave `I` / `R` / `P` / `S` |
| iniziale nuovo | `AL_NUOVO  Primo punto sull'asse del muro nuovo o [Punto interno/Rettangolo/Poligono/Selezione]:` | come sopra |
| secondo punto | `Secondo punto sull'asse:` | punto |
| spessore (sempre per `AL_NUOVO`; per `AL_DEMOLISCI` solo se `rect_from_axis` fallisce) | `Spessore del muro <{last}>:` oppure `Facce del muro non trovate. Spessore del muro:` | numero > 0 (`,` accettata come separatore decimale); Invio prende `last` se c'è |
| punto interno | `Punto interno dell'area:` | punto → `region_at(snap.faces, ..)`; senza regione ripete il prompt con il prefisso `Nessuna area chiusa attorno al punto. ` |
| rettangolo | `Primo angolo:` / `Angolo opposto:` | due punti |
| poligono | `Punto del contorno (Invio per chiudere):` | ≥ 3 punti, poi Invio |
| selezione | `Seleziona una polilinea chiusa:` | `needs_entity_pick()` + `inject_before_entity_pick()`; la polilinea iniettata deve essere una `LwPolyline` chiusa, altrimenti si ripete il prompt |
| varco (solo `AL_DEMOLISCI`) | `Zona {area}. Varco (passaggio)? [Sì/No] <No>:` | `S` / `N` / Invio = No |
| conferma (solo se `validate` restituisce avvisi) | `{avvisi uniti da " "} Confermi la zona? [Sì/No] <Sì>:` | `S` / Invio = Sì; `N` → `Cancel` |

Fine: `CmdResult::CommitEntitiesAndExit(vec![hatch])`, con `preserve_commit_layer()` e `preserve_commit_style()` = `true`. `origin` segue il modo scelto (asse → `MuroDuePunti` per `AL_DEMOLISCI` e `LineaSpessore` per `AL_NUOVO`). `axis` e `thickness` si scrivono solo nei modi ad asse. Il punto interno usa `zone_hatch_from_paths` con `exact_hatch_paths`; gli altri modi usano `zone_hatch`.

Regole di `validate`:
- `Demolito` senza nessun segmento delle sorgenti che attraversi o tocchi l'interno → `ZonaSenzaMuri`.
- `Nuovo` con un segmento che passa nell'interno e non sta dentro una zona `Demolito` esistente → `NuovoSuEsistente`.
- Sovrapposizione parziale con una zona dello stesso stato (né disgiunte né coincidenti) → `StessoStatoSovrapposte`.
- Le sovrapposizioni tra stati opposti non danno avviso.

- [ ] **Step 1: Write the failing tests** (in `commands.rs`; l'helper `snap(entities)` costruisce lo `ZoneSnapshot` in metri con tolleranza `1e-6`)

```rust
#[test] fn demolish_by_axis_commits_yellow_zone() {
    let mut c = ZoneCommand::new(ZoneState::Demolito, snap(case_a(Form::Continua, DrawingUnit::Metri)));
    assert!(matches!(c.on_point(DVec3::new(4.05, 0.0, 0.0)), CmdResult::NeedPoint));
    assert!(matches!(c.on_point(DVec3::new(4.05, 3.0, 0.0)), CmdResult::NeedPoint));
    assert!(c.prompt().starts_with("Zona 0,30 m². Varco"));
    let CmdResult::CommitEntitiesAndExit(v) = c.on_text_input("N").unwrap() else { panic!() };
    let z = zone::read_zone(Handle::new(1), &v[0]).unwrap().zone;
    assert_eq!((z.state, z.origin, z.varco), (ZoneState::Demolito, ZoneOrigin::MuroDuePunti, false));
    assert!((z.thickness.unwrap() - 0.10).abs() < 1e-9);
}
#[test] fn new_wall_by_axis_and_thickness() { /* AL_NUOVO (2;0)-(2;3), "0.12" → rettangolo [1,94; 2,06]×[0; 3]; stato Nuovo; origine LineaSpessore; rosso */ }
#[test] fn demolish_asks_thickness_when_faces_missing() { /* asse (2;0,5)-(2;2,5) dentro A → prompt "Facce del muro non trovate. Spessore del muro:" */ }
#[test] fn rectangle_mode_and_varco() { /* "R", (4;1), (4,1;1,9), "S" → varco = true, origine Rettangolo, axis None */ }
#[test] fn validation_asks_confirmation() { /* AL_DEMOLISCI R (1;1) (2;2) dentro A (nessuna linea) → prompt contiene "non incrocia" e "Confermi la zona?"; "N" → Cancel */ }
#[test] fn internal_point_on_closed_tramezzo() { /* forma ChiusaPerMuro, "I", punto (4,05; 1,5) → zona da 0,30 */ }
```

- [ ] **Step 2: Run the tests to verify they fail** — `cargo test --locked --lib bim::commands` → FAIL.
- [ ] **Step 3: Implement `ZoneCommand` and `validate`.** Il limite di ricerca delle facce è `snap.unit.metres(MAX_WALL_THICKNESS_M)`, cioè 1,50 m in unità del disegno. `input_kind()`: `InputKind::Point` nei passi a punti con `point_step_accepts_keywords() = true` al passo iniziale; `InputKind::SingleToken` nei passi di spessore, varco e conferma. Anteprima con `on_mouse_move` → `WireModel::solid_f64(..)`, rosso per `Nuovo` e giallo per `Demolito` (spec §6).
- [ ] **Step 4: Run the tests to verify they pass** — PASS.
- [ ] **Step 5: Commit** — `git commit -m "BIM: comando zona (asse, spessore, punto interno, rettangolo, poligono, selezione, varco)"`

### Task 6: Famiglia app `AL_DEMOLISCI` / `AL_NUOVO` / `AL_UNITA`

**Files:**
- Create: `src/app/commands/bim.rs`, `src/app/bim_tests.rs`
- Modify: `src/app/commands/mod.rs` (`mod bim;` nell'elenco a `:7-45`; `if let Some(t) = self.dispatch_bim(cmd, i) { return Some(t); }` in `dispatch_families` accanto a `dispatch_table_export`, `:511`); `src/app/mod.rs` (campo `pub(crate) bim_pending: Option<String>` inizializzato a `None`; `#[cfg(test)] mod bim_tests;` accanto a `dialog_flow_tests`, `:36-37`)

**Interfaces:**
- Consumes: Task 1, 2, 3, 5; `super::hatchvars::gap_tolerance()`; `crate::command::KeywordCommand`.
- Produces: `pub(super) fn dispatch_bim(&mut self, cmd: &str, i: usize) -> Option<Task<Message>>`, che riconosce il primo token in maiuscolo tra `AL_*`, più l'helper `fn bim_unit_or_ask(&mut self, i: usize, cmd: &str) -> Option<DrawingUnit>`. Registrazione: `inventory::submit!(crate::command::CommandRegistration { names: &["AL_DEMOLISCI", "AL_NUOVO", "AL_STATO", "AL_LOCALE", "AL_AGGIORNA", "AL_ESTRAI", "AL_UNITA"] });`.

Flusso di `AL_DEMOLISCI` / `AL_NUOVO`:
1. `bim_unit_or_ask` usa `units::resolve` sul documento del tab.
   - `Known(u)` → prosegue.
   - `Unsupported(c)` → `push_error("Unità del disegno non supportata (INSUNITS = {c}): usa millimetri, centimetri o metri.")` e fine.
   - `Unspecified` → `bim_pending = Some(cmd)`, poi avvia `KeywordCommand::new("AL_UNITA", "Unità del disegno non specificata (INSUNITS = 0). [Millimetri/Centimetri/Metri]:", vec![("Millimetri", "MM", None), ("Centimetri", "CM", None), ("Metri", "M", None)])`.
2. `AL_UNITA <kw>`: `push_undo_snapshot(i, "AL_UNITA")`, `record::write` con l'unità, poi rilancia `bim_pending.take()` con `dispatch_command`.
3. Se `zone::missing_layers` → `push_undo_snapshot(i, "AL_LAYER")`, `ensure_layers`, `finish_pending_history(i)`. È un passo di annullamento a sé, solo la prima volta; mai layer creati fuori da un passo di annullamento.
4. Istantanea:
   - `wall_set(doc, u.tolerance())`;
   - `faces(.., max(u.tolerance(), gap_tolerance()))`;
   - `zones_in(doc)`;
   - `last_thickness` dall'ultima zona `Nuovo` nel disegno.
5. `self.command_line.push_info(&c.prompt()); self.tabs[i].active_cmd = Some(Box::new(c));` poi `Some(self.finish_dispatch(cmd))`.

- [ ] **Step 1: Write the failing tests** (in `src/app/bim_tests.rs`; helper `fresh_app()` come `dialog_flow_tests.rs:13-28`; `load(app, entities, insunits)` aggiunge le entità alla scena del tab attivo e imposta `insertion_units`; `run(app, "…")` = `app.automation_op(&format!(r#"{{"op":"run","cmd":"{}"}}"#, …))` + `finish_pending_history`)

```rust
#[test] fn demolish_tramezzo_from_command_line() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 6);
    let before = rilievo_snapshot(&app); // (handle, EntityType) di ogni entità MURI
    run(&mut app, "AL_DEMOLISCI 4.05,0 4.05,3 N");
    let zones = crate::bim::zone::zones_in(&app.tabs[app.active_tab].scene.document);
    assert_eq!(zones.len(), 1);
    assert_eq!(zones[0].zone.state, ZoneState::Demolito);
    assert_eq!(rilievo_snapshot(&app), before, "il rilievo non si tocca");
    // un solo annullamento rimuove la zona
}
#[test] fn unit_is_asked_and_insunits_untouched() {
    // insunits 0 → "AL_DEMOLISCI" lascia attivo AL_UNITA; "CM" → record.unit == Centimetri, insertion_units == 0, e AL_DEMOLISCI riparte
}
#[test] fn inches_are_refused() { // Review Focus 5
    // insunits 1 → la cronologia (command_line.history_plain_text()) contiene "non supportata (INSUNITS = 1)"; nessuna zona
}
#[test] fn layers_created_once_with_colors() { /* dopo il primo AL_NUOVO esistono AL-DEMOLIZIONI (ACI 2) e AL-NUOVE (ACI 1) */ }
```

- [ ] **Step 2: Run the tests to verify they fail** — `cargo test --locked --lib app::bim_tests` → FAIL.
- [ ] **Step 3: Implement the family as described.**
- [ ] **Step 4: Run the tests to verify they pass** — PASS; poi `cargo test --locked --lib classic_toolbar` per controllare che il registro non abbia rotto nulla.
- [ ] **Step 5: Commit** — `git commit -m "BIM: comandi AL_DEMOLISCI, AL_NUOVO e scelta dell'unità (AL_UNITA)"`

### Task 7: `AL_STATO`

**Files:**
- Modify: `src/app/commands/bim.rs`, `src/app/bim_tests.rs`

**Interfaces:**
- Consumes: `zone::{read_zone, restyle}`, `crate::command::SelectThenKeywordCommand`, `set_entity_xdata`.
- Produces: rami `"AL_STATO"` e `"AL_STATO <kw> [valore]"` di `dispatch_bim`.

`AL_STATO` → `SelectThenKeywordCommand::new("AL_STATO", "AL_STATO  [Demolito/Nuovo/Varco/Altezza/Spessore]:", vec![("Demolito", "D", None), ("Nuovo", "N", None), ("Varco", "V", None), ("Altezza", "A", Some("Altezza:")), ("Spessore", "S", Some("Spessore:"))], has_selection)` (modello: `src/app/commands/dim.rs:127-146`).

`AL_STATO <kw> [valore]` sugli handle selezionati:
- un solo `push_undo_snapshot(i, "AL_STATO")`;
- per ogni zona: `D` / `N` cambia lo stato con `restyle`; `V` inverte `varco`, solo per le zone `Demolito`; `A` / `S` impostano un numero > 0 (virgola ammessa);
- in fondo `push_info("AL_STATO: {n} zone modificate.")`;
- se ci sono oggetti non zona: `push_info("{m} oggetti non sono zone e sono stati ignorati.")`;
- `V` su una zona nuova: `push_info("Il varco vale solo per le zone demolite.")`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn stato_switches_to_nuovo() { /* zona demolita selezionata, "AL_STATO N" → layer AL-NUOVE, colore ACI 1, stato Nuovo */ }
#[test] fn stato_varco_only_on_demolished() { /* "AL_STATO V" su una demolita → varco true; su una nuova → messaggio e nessun cambio */ }
#[test] fn stato_altezza_value() { /* "AL_STATO A 3,20" → height == Some(3.20) */ }
#[test] fn stato_ignores_non_zones() { /* selezione con una LINE MURI + una zona → "1 oggetti non sono zone" e LINE invariata */ }
```

- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** — `git commit -m "BIM: AL_STATO (demolito/nuovo, varco, altezza, spessore)"`

## PR 2 — Estrazione dello stato di fatto e di demolizioni e costruzioni

### Task 8: Nucleo di estrazione (fatto e comparativa) e scrittura dei file

**Files:**
- Create: `src/bim/extract.rs`
- Modify: `src/bim/mod.rs`

**Interfaces:**
- Consumes: `crate::modules::insert::wblock::extract_entities_into`, `crate::io::save_as_version`, `crate::scene::view::dispatch::set_entity_xdata` (o la sua riesportazione), Task 2, Task 3.
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Elaborato { StatoDiFatto, DemolizioniCostruzioni, StatoDiProgetto }` con `pub fn suffix(self) -> &'static str` (`"stato-di-fatto"`, `"demolizioni-costruzioni"`, `"stato-di-progetto"`)
  - `pub struct ExtractOptions { pub elaborati: Vec<Elaborato>, pub new_walls_layer: String, pub unit: DrawingUnit, pub tol: f64 }`
  - `pub struct Extracted { pub docs: Vec<(Elaborato, CadDocument)>, pub notices: Vec<Notice> }`
  - `pub fn extract(src: &CadDocument, opts: &ExtractOptions) -> Extracted` (in questo task gestisce `StatoDiFatto` e `DemolizioniCostruzioni`; `StatoDiProgetto` arriva al Task 12)
  - `pub fn file_name(base: &str, e: Elaborato) -> String` → `"{base}_{suffix}.dwg"`
  - `#[derive(Debug, PartialEq)] pub enum WriteError { Exists(Vec<PathBuf>), Io(String) }`
  - `pub fn write(ex: &Extracted, dir: &Path, base: &str, overwrite: bool) -> Result<Vec<PathBuf>, WriteError>`

Procedura per ogni elaborato:
1. `let mut work = src.clone();`.
2. Si scelgono gli handle dello spazio modello:
   - **fatto**: tutto, tranne le zone e le entità sui layer `AL-LOCALI-PROGETTO`;
   - **comparativa**: tutto, tranne le entità sui layer `AL-LOCALI` e `AL-LOCALI-PROGETTO`.
3. Si tolgono i record `ARCHLINE_ZONA` e `ARCHLINE_LOCALE` da quegli handle in `work`.
4. Si prepara `out = CadDocument::new()` con `out.version = src.version` (se la versione non è tra AC1014 e AC1032, si usa AC1032). Poi `extract_entities_into(&work, &handles, &mut out)`, e `out.header.insertion_units = src.header.insertion_units`.
5. Per ogni layer di `out`, se il suo tipo di linea manca in `out`, lo si copia da `src`. È una lacuna nota di `ClipboardDeps`.
6. Ogni handle che `copy_to_clipboard_kernel` non restituisce diventa `NonCopiata`.

`write`: controlla prima tutti i percorsi e, se uno esiste e manca `overwrite`, restituisce `Exists(tutti quelli esistenti)` senza scrivere nulla. Altrimenti chiama `save_as_version(doc, path, doc.version)`.

- [ ] **Step 1: Write the failing tests** (helper `opts(e: &[Elaborato]) -> ExtractOptions` = `ExtractOptions { elaborati: e.to_vec(), new_walls_layer: "MURI".into(), unit: DrawingUnit::Metri, tol: 1e-6 }`)

```rust
#[test] fn comparativa_has_yellow_and_red_without_our_data() {
    let src = doc_with_zones(); // caso A in metri + zona demolita sul tramezzo + zona nuova (2;0,5)-(2;2,5) spessore 0,12
    let ex = extract(&src, &opts(&[Elaborato::DemolizioniCostruzioni]));
    let (_, out) = &ex.docs[0];
    let hatches: Vec<_> = out.entities().filter_map(|e| if let EntityType::Hatch(h) = e { Some(h) } else { None }).collect();
    assert_eq!(hatches.len(), 2);
    assert!(hatches.iter().any(|h| h.common.layer == "AL-DEMOLIZIONI" && h.common.color == Color::from_index(2) && h.is_solid));
    assert!(hatches.iter().any(|h| h.common.layer == "AL-NUOVE" && h.common.color == Color::from_index(1) && h.is_solid));
    assert!(out.entities().all(|e| e.common().extended_data.records().iter().all(|r| !r.application_name.starts_with("ARCHLINE"))));
    assert!(!out.app_ids.iter().any(|a| a.name.starts_with("ARCHLINE")));
}
#[test] fn fatto_has_no_zones_and_same_lines() { /* nessuna HATCH sui layer AL-*; ogni LINE MURI di src ha in out una LINE con gli stessi estremi entro 1e-6 */ }
#[test] fn version_units_and_layer_linetype_follow_source() { /* src AC1015, insunits 6, layer MURI con linetype "DASHED": out.version == AC1015, insunits 6, linetype DASHED presente */ }
#[test] fn write_refuses_existing_files() { /* prima write OK (2 file con file_name corretti); seconda senza overwrite → Err(Exists(..)) e date di modifica invariate; con overwrite → Ok */ }
#[test] fn written_dwg_reloads() { /* crate::io::load_file su ogni file scritto → Ok, stesso numero di entità */ }
```

- [ ] **Step 2: Run** — `cargo test --locked --lib bim::extract` → FAIL. **Step 3: Implement.** **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** — `git commit -m "BIM: estrazione in DWG 2D dello stato di fatto e di demolizioni e costruzioni"`

### Task 9: `AL_ESTRAI` (riga di comando e finestra)

**Files:**
- Create: `src/ui/window/bim_extract.rs`
- Modify:
  - `src/ui/window/mod.rs` (`pub mod bim_extract;`)
  - `src/app/commands/bim.rs`
  - `src/app/mod.rs`:
    - `ModalKind::BimExtract` in `enum ModalKind`, `:2314`;
    - campo `pub(crate) bim_extract: Option<crate::ui::window::bim_extract::ExtractDialog>`;
    - variante `Bim(crate::ui::window::bim_extract::BimMsg)` in `enum Message`, accanto a `Workspace`, `:4341`.
  - `src/app/update/mod.rs`:
    - instradamento `Message::Bim(m) => self.on_bim(m)` accanto a `:11100`;
    - chiusura `Some(BimExtract) => self.bim_extract = None` accanto a `:248`.
  - `src/app/view/modal.rs`:
    - titolo `"Estrai elaborati"` accanto a `:97`;
    - corpo `sized_flow(ex, 460, 300, |flow| crate::ui::window::bim_extract::view(state, flow))` accanto a `:412`.
  - `src/app/bim_tests.rs`

**Interfaces:**
- Consumes: Task 8; `crate::sys::file_dialog().pick_folder()` (modello: `src/app/commands/sheet_set.rs:1033-1044`); `app.scripted_dispatch`.
- Produces:
  - `pub struct ExtractDialog { pub dir: Option<PathBuf>, pub fatto: bool, pub comparativa: bool, pub progetto: bool, pub layer: String, pub layers: Vec<String>, pub confirm_overwrite: bool }`
  - `#[derive(Debug, Clone)] pub enum BimMsg { Browse, Picked(Option<PathBuf>), Toggle(Elaborato), Layer(String), Ok, Cancel }`
  - `pub(super) fn on_bim(&mut self, m: BimMsg) -> Task<Message>` in `src/app/commands/bim.rs`
  - `fn bim_run_extract(&mut self, i: usize, dir: &Path, elaborati: Vec<Elaborato>, layer: String, overwrite: bool) -> Result<Vec<PathBuf>, WriteError>`

Flusso:
- `AL_ESTRAI <cartella>`: esegue subito, con tutti e tre gli elaborati e senza sovrascrivere. Con file esistenti: `push_error("AL_ESTRAI: i file esistono già: {elenco}. Niente è stato scritto.")`.
- `AL_ESTRAI` da solo, con `scripted_dispatch`: `ValuePromptCommand::new("AL_ESTRAI", "Cartella di destinazione:")`, che restituisce `Dispatch("AL_ESTRAI <testo>")`.
- `AL_ESTRAI` da solo, senza `scripted_dispatch`: apre la finestra. Valori iniziali:
  - cartella del disegno, se esiste;
  - tre caselle attive;
  - layer = `record.new_walls_layer` oppure `extract::propose_new_walls_layer(doc)` (Task 12; fino ad allora stringa vuota);
  - `layers` = elenco dei layer del disegno, esclusi gli `AL-*`.
- `Ok`:
  - se ci sono file esistenti e `!confirm_overwrite` → `confirm_overwrite = true` e la finestra mostra `"I file esistono già: premi di nuovo OK per sovrascriverli."`;
  - altrimenti scrive con `overwrite = confirm_overwrite` e chiude.
- Nome di base: radice del file del disegno, oppure titolo del tab se il disegno non è salvato.
- Rapporto finale: `push_info("AL_ESTRAI: scritti {n} file in {cartella}.")` più una riga per ogni `Notice::text()`.
- Il layer scelto si salva nel record con `push_undo_snapshot(i, "AL_ESTRAI")` + `record::write`, solo se è cambiato.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn scripted_extract_writes_three_files() { /* caso A + zone; run("AL_ESTRAI {tmp}") → esistono {base}_stato-di-fatto.dwg e {base}_demolizioni-costruzioni.dwg (il progetto arriva nel Task 12: qui almeno questi due) */ }
#[test] fn scripted_extract_never_overwrites() { /* secondo run → cronologia contiene "i file esistono già"; date di modifica invariate */ }
#[test] fn dialog_flow_overwrite_needs_second_ok() { /* send(Message::Bim(BimMsg::Picked(Some(tmp)))), Ok → scrive; Ok di nuovo (nuova apertura) → confirm_overwrite = true e nessuna scrittura; Ok → scrive */ }
```

- [ ] **Step 2: Run** — FAIL. **Step 3: Implement** (la vista: `text_input` per il layer con suggerimenti da `layers`, tre `checkbox`, pulsanti Sfoglia / OK / Annulla; nessuna logica in `view_main`). **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** — `git commit -m "BIM: AL_ESTRAI da riga di comando e finestra, senza sovrascrivere"`

## PR 3 — Stato di progetto

### Task 10: Taglio delle entità (pezzi che restano)

**Files:**
- Create: `src/bim/clip.rs`
- Modify: `src/bim/mod.rs`

**Interfaces:**
- Consumes: le curve per segmento di `BoundarySource.curves` (ordine dei segmenti = ordine dei vertici della polilinea; il segmento di chiusura è l'ultimo).
- Produces: `pub fn kept_pieces(entity: &EntityType, removed: &[(usize, [f64; 2])]) -> Vec<EntityType>`. `removed` è l'elenco `(indice del segmento, intervallo di parametro [t0, t1] in 0..1)` da togliere. Restituisce i pezzi che restano, con gli attributi dell'originale (layer, colore, tipo di linea, spessore, `constant_width`) e handle `NULL`. Per una `LwPolyline` o una `Polyline2D`, i segmenti contigui che restano formano un'unica polilinea aperta; una polilinea chiusa tagliata si apre partendo dal punto di taglio. Per un arco, il pezzo è un arco con gli angoli ricalcolati; per un segmento curvo di polilinea, il nuovo bulge è `tan(θ'/4)` con il segno dell'originale. Senza tagli restituisce l'entità intera.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn line_middle_removed() { /* LINE (0;0)-(10;0), removed [(0,[0.4,0.6])] → due LINE (0;0)-(4;0) e (6;0)-(10;0) */ }
#[test] fn closed_square_opens_at_cut_keeping_attributes() {
    // LWPOLYLINE chiusa (0;0)(10;0)(10;10)(0;10), layer MURI, colore ACI 3, linetype DASHED, constant_width 0.02
    // removed [(1,[0.4,0.6])] → una LWPOLYLINE aperta con vertici (10;6)(10;10)(0;10)(0;0)(10;0)(10;4), stessi attributi
}
#[test] fn arc_half_removed() { /* ARC centro (0;0) r 1 da 0° a 90°, removed [(0,[0.5,1.0])] → ARC da 0° a 45° */ }
#[test] fn bulge_segment_half_removed() { /* LWPOLYLINE (1;0)-(0;1) con bulge tan(90°/4), removed [(0,[0.5,1.0])] → bulge tan(45°/4) */ }
```

- [ ] **Step 2: Run** — FAIL. **Step 3: Implement** (riferimenti per i costruttori dei pezzi: `extrim_line`, `extrim_arc`, `extract_sub_polyline` in `src/modules/draw/modify/trim.rs:5490, 5512, 1031`, che sono privati: riscrivere qui la parte che serve). **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** — `git commit -m "BIM: taglio di linee, archi e polilinee con gli attributi dell'originale"`

### Task 11: Geometria dello stato di progetto

**Files:**
- Create: `src/bim/project.rs`
- Modify: `src/bim/mod.rs`

**Interfaces:**
- Consumes: Task 3 (`WallSet`, `sources_of`), Task 4 (`WallRect`), Task 10, `kernel::geom2d::{inside_spans, distance_to, Curve, Line, Tolerance}`.
- Produces:
  - `pub struct ProjectGeometry { pub drawn: Vec<EntityType>, pub kept_originals: Vec<Handle>, pub room_entities: Vec<(Handle, EntityType)>, pub excluded: Vec<Handle>, pub notices: Vec<Notice> }`. `drawn` contiene pezzi, chiusure e linee dei muri nuovi; `kept_originals` sono le entità muro non toccate. `room_entities` è tutto ciò che delimita i locali di progetto: le entità muro non toccate (con il loro handle), più `drawn` e i lati lunghi dei varchi, con handle sintetici da `Handle::new(0xF000_0000)` in su. Così va passato direttamente a `walls::sources_of`.
  - `pub fn project_geometry(doc: &CadDocument, walls: &WallSet, zones: &[ZoneGeom], new_walls_layer: &str, tol: f64) -> ProjectGeometry`

Regole (dalla spec §5.4; l'ordine conta):
1. **Tagli.** Per ogni segmento di ogni entità muro e per ogni zona `Demolito`, `inside_spans(bordo, curva, tol)` dà gli intervalli dentro, bordo compreso. Si classifica ogni intervallo con il suo punto medio. Se il punto medio dista più di `tol` dal bordo, l'intervallo è interno e **si toglie**. Se sta sul bordo, si toglie solo quando la zona ha `axis` e il lato più vicino è uno dei due `long_sides`; altrimenti resta. Per le zone senza `axis` che toccano il bordo di un muro: un solo `ConfiniNonRimossi { at: centroide della zona }` per zona.
2. **Chiusure** (solo zone con `axis`). Per ciascuno dei due `end_sides`:
   - se coincide (collineare con sovrapposizione > `tol`) con un segmento muro che resta → `GiunzioneDaVerificare { at: punto medio }` e nessuna linea;
   - altrimenti, se entrambi gli estremi distano ≤ `tol` da segmenti muro → `LINE` con gli attributi dell'entità muro più vicina al primo estremo;
   - altrimenti → `TaglioNonChiuso { at }`.
3. **Varco.** Zona `Demolito` con `varco = true` e `axis`: i due `long_sides` vanno solo in `room_entities`, come `LINE`. Zona con `varco = true` senza `axis` → `VarcoNonSupportato`.
4. **Zone nuove.** Si prendono tutti i lati delle zone `Nuovo` e le chiusure del punto 2. Le parti collineari sovrapposte presenti in esattamente due lati si tolgono da entrambi (questo elimina le cuciture e le chiusure coperte da un muro ricostruito). Poi si fondono i segmenti collineari contigui. Un lato nuovo collineare con un pezzo di muro esistente che resta non si aggiunge e dà `GiunzioneDaVerificare`. Sovrapposizioni parziali di aree tra zone `Nuovo`, cioè interni che si intersecano senza coincidere → `NuoveSovrapposte`. Le linee nuove vanno sul layer `new_walls_layer`, con colore e tipo di linea `ByLayer`.
5. **Elementi puntuali.** Blocchi e testi (`Insert`, `Text`, `MText`) con punto di inserimento dentro una zona `Demolito` finiscono in `excluded`, con un `Esclusa` ciascuno.
6. **Entità non tagliabili.** Entità non muro che attraversano una zona `Demolito`, cioè con almeno un vertice o un punto di controllo dentro e uno fuori, restano intere, con `NonTagliata`.

- [ ] **Step 1: Write the failing tests** (caso A in metri; `project(entities, zones)` = `project_geometry` su `fixtures::scene_with(entities, 6)` con il layer dei muri nuovi `"MURI"` e tolleranza `1e-6`; `axis_zone(p1, p2, varco)` costruisce uno `ZoneGeom` demolito da `rect_from_axis`; `has_line(pg, a, b)` cerca in `drawn` una `LINE` con quegli estremi entro `1e-6`, in qualunque verso; `no_wall_inside(pg, xs, (y0, y1))` è vero se nessun pezzo in `drawn` né originale in `kept_originals` ha un tratto su `x = xs[k]` con `y0 < y < y1`)

```rust
#[test] fn total_demolition_continuous_closes_the_outer_faces() {
    let pg = project(case_a(Form::Continua, DrawingUnit::Metri), vec![axis_zone([4.05,0.0],[4.05,3.0], false)]);
    assert!(has_line(&pg, [4.0, 0.0], [4.1, 0.0]) && has_line(&pg, [4.0, 3.0], [4.1, 3.0]));
    assert!(no_wall_inside(&pg, [4.0, 4.1], [0.0, 3.0])); // nessun tratto x = 4,00 o x = 4,10 con 0 < y < 3
    assert!(pg.notices.is_empty());
}
#[test] fn total_demolition_closed_walls_reports_junctions() {
    // ChiusaPerMuro → notices == [Giunzione (4,05;0), Giunzione (4,05;3)] in qualunque ordine; la linea sud y=0 resta intera
}
#[test] fn varco_keeps_jambs_and_soglie_only_for_rooms() {
    // asse (4,05;1,00)-(4,05;1,90) varco → chiusure a y=1,00 e y=1,90; in drawn nessun tratto x=4,00/4,10 con 1,00<y<1,90; room_entities contiene (4,00;1,00)-(4,00;1,90) e (4,10;1,00)-(4,10;1,90)
}
#[test] fn new_l_shape_has_no_seam() {
    // zone nuove [1,00;1,12]×[0;2,12] e [1,12;2,50]×[2,00;2,12] → 6 linee nuove sul layer "MURI"; nessuna linea su x=1,12 con 2,00<y<2,12
}
#[test] fn rebuilt_on_demolished_leaves_no_caps() {
    // demolita sul tramezzo + nuova con lo stesso asse e spessore 0,10 → nessuna LINE (4,00;0)-(4,10;0) né (4,00;3)-(4,10;3); linee nuove x=4,00 e x=4,10 da 0 a 3
}
#[test] fn new_against_existing_reports_junctions() { /* nuova asse (2;0)-(2;3) spessore 0,12 → Giunzione in (2;0) e (2;3); lati nuovi x=1,94 e x=2,06 */ }
#[test] fn point_and_uncuttable_entities() { /* TEXT dentro la zona demolita → excluded + Esclusa; SPLINE che la attraversa → in kept/drawn intera + NonTagliata */ }
#[test] fn zone_without_axis_leaves_borders() { /* zona demolita da Rettangolo [4,00;4,10]×[0;3] → linee x=4,00 e x=4,10 intatte; ConfiniNonRimossi */ }
```

- [ ] **Step 2: Run** — `cargo test --locked --lib bim::project` → FAIL. **Step 3: Implement.** **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** — `git commit -m "BIM: geometria dello stato di progetto (tagli, chiusure, varchi, muri nuovi, giunzioni)"`

### Task 12: Stato di progetto nell'estrazione e layer dei muri nuovi

**Files:**
- Modify: `src/bim/extract.rs`, `src/app/commands/bim.rs`, `src/app/bim_tests.rs`

**Interfaces:**
- Consumes: Task 11.
- Produces:
  - in `extract`, il ramo `Elaborato::StatoDiProgetto`, con queste entità: `kept_originals`, più `drawn` aggiunte a `work`, più le entità non muro dello spazio modello, tranne zone, layer `AL-LOCALI*` ed `excluded`. Gli avvisi di `project_geometry` vanno in `Extracted.notices`.
  - `pub fn propose_new_walls_layer(doc: &CadDocument) -> Option<String>`: il layer più frequente tra le entità muro; a parità vince l'ordine alfabetico.
  - Se `new_walls_layer` non esiste in `out`, lo si crea con `allocate_handle()` e con colore e tipo di linea del layer omonimo di `src`, se c'è.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn progetto_closes_outer_faces_and_has_no_tramezzo() { /* extract StatoDiProgetto su caso A + demolizione totale → in out c'è LINE (4,00;0)-(4,10;0), nessuna LINE x=4,00 con 0<y<3, nessuna HATCH sui layer AL-* */ }
#[test] fn proposal_is_most_frequent_wall_layer() { /* 12 LINE su MURI + 2 su TRAMEZZI → Some("MURI") */ }
// app: run("AL_ESTRAI {tmp}") ora scrive anche {base}_stato-di-progetto.dwg e la cronologia elenca le giunzioni del caso ChiusaPerMuro
```

- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** — `git commit -m "BIM: estrazione dello stato di progetto, layer dei muri nuovi proposto e ricordato"`

## PR 4 — Locali

### Task 13: Modello dei locali e calcolo per fase

**Files:**
- Create: `src/bim/rooms.rs`
- Modify: `src/bim/mod.rs`

**Interfaces:**
- Consumes: Task 3 (`faces`, `region_at`, `net_area`, `sources_of`), Task 11 (`ProjectGeometry.room_entities`, passato a `sources_of` e poi a `faces` per ottenere `pg_sources` e `pg_faces`), `crate::scene::boundary_polyline_entities`.
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Phase { Fatto, Progetto }`
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum RoomPart { Contorno, Testo, Seme }`
  - `#[derive(Clone, Debug, PartialEq)] pub struct Room { pub phase: Phase, pub name: String, pub number: String, pub seed: [f64; 2] }`
  - `pub fn encode(room: &Room, part: RoomPart) -> Vec<XDataValue>` e `pub fn decode(rec: &ExtendedDataRecord) -> Option<(Room, RoomPart)>`. Sequenza: `Integer16(1)`, `String("fase")`, `String("fatto"|"progetto")`, `String("parte")`, `String("contorno"|"testo"|"seme")`, `String("nome")`, `String(..)`, `String("numero")`, `String(..)`, `String("punto")`, `Point3D(seed)`, `String("origine")`, `String("punto-interno")` (spec §4.2).
  - `pub fn rooms_in(doc: &CadDocument) -> Vec<(Handle, Room, RoomPart)>`
  - `pub fn label(name: &str, area_m2: f64, confirm: bool) -> String` → `"{name}\\PS = {format_m2}"`, con `" (da confermare)"` dopo il nome quando `confirm`
  - `pub struct Computed { pub name: String, pub number: String, pub area_m2: f64, pub region: Region, pub confirm: bool, pub exact: bool, pub islands: usize, pub seed: [f64; 2] }`
  - `pub fn compute_at(faces: &[Vec<[f64; 2]>], sources: &FxHashMap<Handle, BoundarySource>, seed: [f64; 2], unit: DrawingUnit, tol: f64) -> Option<(Region, f64, bool, usize)>`, che restituisce regione, area in m², esattezza e numero di isole
  - `pub fn room_entities(c: &Computed, phase: Phase, sources: &FxHashMap<..>, text_height: f64, tol: f64) -> Vec<EntityType>`: una polilinea sul layer `AL-LOCALI` (dall'anello esterno, con `boundary_polyline_entities`) e un `MTEXT` sul seme con `label`, entrambi con XDATA (`Contorno` e `Testo`)
  - `pub fn seed_entity(room: &Room, text_height: f64) -> EntityType`: un `MTEXT` con il solo nome, layer `AL-LOCALI-PROGETTO`, XDATA `Seme`
  - `pub fn project_rooms(pg_faces: &[Vec<[f64; 2]>], pg_sources: &FxHashMap<..>, fatto: &[Room], fatto_regions: &[Region], progetto: &[Room], zones: &[ZoneGeom], unit: DrawingUnit, tol: f64) -> (Vec<Computed>, Vec<Notice>)`

Regole di `project_rooms` (spec §5.4):
- Per ogni seme, di progetto o di fatto, si calcola la regione sulla geometria di progetto. Più semi nella stessa faccia esterna formano un gruppo.
- Un gruppo con almeno un seme di progetto prende il primo nome di progetto, con `confirm = false`.
- Un gruppo con soli semi di fatto prende i nomi uniti da `" + "`, nell'ordine dei semi; `confirm` vale `true` se i semi sono più di uno, e ogni fusione dà un `LocaleFuso`.
- Un seme di progetto dentro una zona `Nuovo` dà `SemeInMuroNuovo`.
- Le facce senza seme danno un locale `"(senza nome)"` con `LocaleSenzaNome`, solo se hanno un punto interno dentro l'esterno di una regione di fatto e non sono né un buco di una regione calcolata né interne a una zona. Il punto interno è il baricentro del triangolo più grande di `kernel::geom2d::triangulate`.
- Le facce interne a una zona con `varco = true` danno `Varco { area }`.
- Islands > 0 → `Isole`; area non esatta → `ContornoApprossimato`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn case_a_project_merges_into_24() { /* fatto A seme (2;1,5), B seme (6;1,5), demolizione totale → un Computed "A + B", 24,00, confirm true; notices contiene LocaleFuso */ }
#[test] fn project_seed_wins_the_name() { /* + seme di progetto "Soggiorno" in (3;1) → nome "Soggiorno", confirm false */ }
#[test] fn case_b_varco_keeps_two_rooms() { /* varco (4,05;1,00)-(4,05;1,90) → A 12,00, B 11,70, notices contiene Varco { area_m2: 0,09 } */ }
#[test] fn split_room_has_unnamed_half() { /* nuova asse (2;0)-(2;3) spessore 0,12, seme A (1;1,5) → A 5,82 e "(senza nome)" 5,82 */ }
#[test] fn seed_in_new_wall_is_reported() { /* seme di progetto "X" in (2;1,5) dentro la zona nuova → SemeInMuroNuovo */ }
#[test] fn label_text() { assert_eq!(label("A", 12.0, false), "A\\PS = 12,00 m²"); assert_eq!(label("A + B", 24.0, true), "A + B (da confermare)\\PS = 24,00 m²"); }
// Review Focus 3
#[test] fn corner_gap_needs_tolerance() { /* caso A con l'angolo (0;0) di A aperto di 0,002: compute_at con tol 1e-6 → None; con tol 0,005 → 12,00 entro 1e-3 */ }
```

- [ ] **Step 2: Run** — `cargo test --locked --lib bim::rooms` → FAIL. **Step 3: Implement.** **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** — `git commit -m "BIM: locali di fatto e di progetto, nomi meccanici, varco escluso"`

### Task 14: `AL_LOCALE` e `AL_AGGIORNA`

**Files:**
- Modify: `src/bim/commands.rs`, `src/app/commands/bim.rs`, `src/app/bim_tests.rs`

**Interfaces:**
- Produces:
  - `pub struct RoomSnapshot { pub fatto: (Vec<Vec<[f64; 2]>>, FxHashMap<Handle, BoundarySource>), pub progetto: (Vec<Vec<[f64; 2]>>, FxHashMap<Handle, BoundarySource>), pub new_zone_rings: Vec<Vec<[f64; 2]>>, pub unit: DrawingUnit, pub tol: f64, pub text_height: f64 }`
  - `pub struct RoomCommand` con `pub fn new(snap: RoomSnapshot) -> Self`
  - ramo `"AL_LOCALE"` in `dispatch_bim`: istantanea con `tol = max(u.tolerance(), gap_tolerance())`, `text_height = doc.header.text_height` e geometria di progetto dal Task 11
  - ramo `"AL_AGGIORNA"` in `dispatch_bim`

Prompt di `RoomCommand`:

| Passo | Prompt | Input |
|---|---|---|
| punto | `AL_LOCALE ({Fatto|Progetto})  Punto interno del locale o [Fase]:` | punto; `F` alterna la fase (partenza: Fatto) |
| punto senza locale | `Nessun locale chiuso attorno al punto (chiudi i vani con una linea di soglia o usa HPGAPTOL). AL_LOCALE (…)  Punto interno del locale o [Fase]:` | punto |
| seme di progetto dentro un muro nuovo | `Il punto cade dentro un muro nuovo. AL_LOCALE (Progetto)  Punto interno del locale o [Fase]:` | punto |
| nome | `Superficie netta {area}{ (n isole)}. Nome del locale:` | testo libero (`InputKind::FreeText`), non vuoto |
| numero | `Numero <nessuno>:` | testo libero; Invio = vuoto |

Fine: fase Fatto → `CommitEntitiesAndExit(room_entities(..))`; fase Progetto → `CommitEntitiesAndExit(vec![seed_entity(..)])`. In entrambi i casi `preserve_commit_layer` e `preserve_commit_style` valgono `true`.

`AL_AGGIORNA`:
- un solo `push_undo_snapshot(i, "AL_AGGIORNA")`;
- per ogni coppia `Contorno` + `Testo` di fase Fatto (accoppiate per seme entro `tol`), ricalcola con `compute_at` sull'insieme dei muri attuale;
- se trova il locale, sostituisce i vertici della polilinea (stesso handle, via `get_entity_mut`) e il testo dell'`MTEXT`; altrimenti dà `LocaleNonChiuso` e lascia tutto com'è;
- in fondo `push_info("AL_AGGIORNA: {n} locali aggiornati.")` più gli avvisi.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn locale_fatto_writes_outline_and_label() { /* run("AL_LOCALE 2,1.5 A") con numero vuoto → una LWPOLYLINE su AL-LOCALI e un MTEXT "A\\PS = 12,00 m²"; la cronologia contiene "Superficie netta 12,00 m²" */ }
#[test] fn locale_progetto_stores_only_the_seed() { /* "AL_LOCALE F 3,1 Soggiorno" → un solo MTEXT "Soggiorno" su AL-LOCALI-PROGETTO */ }
#[test] fn aggiorna_follows_the_survey() { /* dopo AL_LOCALE su A, spostare la LINE x=4,00 a x=3,90 (get_entity_mut) → AL_AGGIORNA → MTEXT "A\\PS = 11,70 m²", stessi handle */ }
#[test] fn aggiorna_reports_open_room() { /* togliere la LINE (0;0)-(4,00;0) → "non è più chiuso" e polilinea invariata */ }
// Review Focus 3, lato app
#[test] fn gap_closed_by_hpgaptol() { /* angolo aperto di 0,002: "HPGAPTOL 0.005" poi AL_LOCALE → 12,00 */ }
```

- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** — `git commit -m "BIM: AL_LOCALE (fatto e progetto) e AL_AGGIORNA"`

### Task 15: Locali nelle estrazioni e prova completa sugli originali

**Files:**
- Modify: `src/bim/extract.rs`, `src/app/bim_tests.rs`

**Interfaces:**
- Consumes: Task 13.
- Produces nel ramo di `extract`:
  - **fatto**: in `work`, i locali di fatto si ricalcolano da capo come in `AL_AGGIORNA`; i non chiusi danno `LocaleNonChiuso` e restano com'erano.
  - **progetto**: vi entrano i `Computed` di `project_rooms` come `room_entities`, con contorno e testo, senza XDATA dopo la pulizia; gli avvisi vanno in `notices`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn progetto_has_merged_room_label() { /* caso A + locali A e B + demolizione totale → in out un MTEXT "A + B (da confermare)\\PS = 24,00 m²" */ }
#[test] fn fatto_recomputes_stale_labels() { /* locale A scritto, poi tramezzo spostato senza AL_AGGIORNA → nel DWG di fatto l'MTEXT dice 11,70 m² */ }
#[test] fn full_flow_never_touches_the_survey() {
    // app: caso A ChiusaPerMuro; AL_DEMOLISCI, AL_NUOVO, AL_STATO V, AL_LOCALE (fatto e progetto), AL_AGGIORNA, AL_ESTRAI {tmp}
    // → ogni entità MURI ha lo stesso handle, la stessa geometria e gli stessi attributi di prima
}
```

- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** — PASS; poi `cargo test --locked --lib bim::` e `cargo test --locked --lib app::bim_tests` tutti verdi.
- [ ] **Step 5: Commit** — `git commit -m "BIM: locali ricalcolati nelle estrazioni; prova completa sugli originali"`

## PR 5 — Interfaccia e banco di conformità

### Task 16: Menu Strumenti > Ristrutturazione e barra

**Files:**
- Modify: `src/ui/classic_menu.rs`, `src/ui/toolbar_layout.rs:242-271`, `src/ui/toolbar_registry.rs`

**Interfaces:**
- Produces:
  - nel menu `Tools` di `build()` (`classic_menu.rs:203`), dopo il sottomenu `Inquiry` e il suo separatore: `sub("Ristrutturazione", vec![cmd("Demolisci", "AL_DEMOLISCI"), cmd("Nuovo", "AL_NUOVO"), cmd("Stato", "AL_STATO"), cmd("Locale", "AL_LOCALE"), cmd("Aggiorna locali", "AL_AGGIORNA"), cmd("Estrai elaborati", "AL_ESTRAI")])`;
  - la barra `menu:Ristrutturazione`, generata da sé (`toolbar_registry.rs:359-378`);
  - in `ToolbarLayout::autocad_classic()`, la terza riga in alto diventa `[Insert, Reference, Ristrutturazione]`.
  - Le icone ripiegano sul glifo di due lettere (`toolbar_icon` → `glyph_for`): scelta per il primo giro, nessun SVG nuovo.

- [ ] **Step 1: Update the failing tests first**: in `toolbar_registry.rs`, `autocad_classic_places_every_bar_of_the_screenshot` (`:678-705`) attende la terza riga `[Insert, Reference, Ristrutturazione]` e `shown == 3+2+3+1+2+4`. Aggiungere poi `ristrutturazione_bar_has_the_six_commands`, che controlla i comandi `AL_DEMOLISCI`, `AL_NUOVO`, `AL_STATO`, `AL_LOCALE`, `AL_AGGIORNA`, `AL_ESTRAI` in quest'ordine.
- [ ] **Step 2: Run** — `cargo test --locked --lib classic_toolbar` e `cargo test --locked --lib toolbar_registry` → FAIL.
- [ ] **Step 3: Implement** il sottomenu e la riga.
- [ ] **Step 4: Run** — PASS. Verificare anche `every_command_exists` (`classic_menu.rs:775-804`), che passa grazie alla registrazione del Task 6, e `titles_follow_the_autocad_order`, invariato perché non si aggiunge un menu di primo livello.
- [ ] **Step 5: Commit** — `git commit -m "BIM: menu Strumenti > Ristrutturazione e barra nel workspace AutoCAD Classic"`

### Task 17: Caso nel banco di conformità

**Files:**
- Create: `tests/conformance/bim_extract.py`
- Modify: `tests/conformance/README.md` (sezione «Estrazione BIM»)

**Interfaces:**
- Consumes: `ArchLine --serve` (righe JSON su stdin e stdout: aspettare la riga di pronto, poi `{"op":"open","path":..}`, `{"op":"run","cmd":..}`, ognuna con la sua risposta), `ArchLine --export IN OUT`, ezdxf.

Comportamento dello script (`python tests/conformance/bim_extract.py target\release\ArchLine.exe`, esce con 1 se un controllo fallisce):
1. In una cartella `tempfile.mkdtemp()` (il percorso non deve contenere spazi, perché `run` divide sugli spazi), ezdxf costruisce il caso A forma Continua in metri, con `$INSUNITS = 6` e i muri sul layer `MURI`, e lo salva come DXF R2018.
2. Comandi via `--serve`:
   - `open`;
   - `run "AL_DEMOLISCI 4.05,0 4.05,3 N"`;
   - `run "AL_NUOVO 2,0.5 2,2.5 0.12"`;
   - `run "AL_ESTRAI <cartella>"`.
3. Ogni DWG prodotto si converte con `--export x.dwg x.dxf` e si legge con ezdxf. Controlli:
   - comparativa: due `HATCH` solide, una di colore 2 su `AL-DEMOLIZIONI` e una di colore 1 su `AL-NUOVE`;
   - nessuna entità con XDATA il cui APPID inizia con `ARCHLINE` e nessun APPID `ARCHLINE*` in tabella, in tutti e tre i file;
   - progetto: esiste una `LINE` da (4,00; 0) a (4,10; 0) entro 1e-6;
   - fatto: le linee `MURI` hanno gli stessi estremi del DXF di partenza.
4. Stampa una riga OK / FALLITO per controllo.

- [ ] **Step 1:** scrivere lo script e la sezione del README.
- [ ] **Step 2:** `cargo build --release --bin ArchLine`, poi `python tests/conformance/bim_extract.py target\release\ArchLine.exe` → tutti OK. Se l'exe è in uso, la build dà «Accesso negato» (trappola 6 di `CLAUDE.md`).
- [ ] **Step 3: Commit** — `git commit -m "Banco di conformità: estrazione BIM controllata con ezdxf"`

---

## Dopo l'ultimo task

- `cargo check --locked`, `cargo test --locked --lib bim::`, `cargo test --locked --lib app::bim_tests`, `cargo test --locked --lib classic_toolbar`: tutti verdi.
- Aggiornare `CLAUDE.md` (sezione «Cosa è ArchLine») con i file propri del modulo BIM, i comandi e i limiti noti (giunzioni lasciate com'è, archi esatti solo dove `exact_hatch_paths` ricostruisce le curve, icone a glifo).
- Verifica a schermo da parte di Mauro su Windows: zone gialle e rosse, prompt in riga di comando, finestra di estrazione, barra Ristrutturazione, i tre DWG aperti in AutoCAD.
