# BIM parte 3: porte e finestre — piano di implementazione

Spec: `docs/superpowers/specs/2026-11-05-bim-porte-finestre-design.md` (proposta, non rivista da Mauro).
Branch: `claude/bim-aperture`, da `claude/bim-decisioni` (= `lavoro` a `ac8d92456` + R84/R91/R93 + spec).
Registro delle decisioni: `docs/superpowers/ledgers/2026-11-05-bim-porte-finestre.md` (R96…).

Regole del piano: un task = un commit, test prima del codice; dopo ogni task `cargo test --locked --lib
bim::`; dopo i task con comandi anche `--lib app::bim_tests` e `--lib commands::bim_a`. Build e test in
primo piano, nessun altro `cargo` in parallelo nello stesso worktree. Nessuna riscrittura del rilievo:
ogni task che legge muri aggiunge un test «originali intatti».

## Correzioni alla spec emerse dal codice (vanno nel registro come R96…)

- **R96 – niente ancora per le aperture.** L'apertura è **una sola entità** (la polilinea del vano con il
  suo record), non una famiglia contorno+testo come i locali: una copia è un'apertura indipendente e giusta.
  Quindi niente `renew_anchors`, niente `ambiguous_parts`. L'asse è `Position3D` (segue MOVE/ROTATE/SCALE/
  MIRROR), lo spessore `Distance`, altezza e davanzale `Real` (non scalano: dichiarati, come l'altezza
  delle zone). Resta da fare solo la migrazione dei record (nessuno esiste ancora: solo la funzione, con
  il test che `migrate_document` non tocca le aperture).
- **R97 – la chiusura vale anche per il fatto.** Lo stato di fatto non usa `room_entities` della geometria
  di progetto (`refresh.rs` misura sui muri del rilievo): una porta esistente nel rilievo deve chiudere il
  vano anche lì. Le soglie delle aperture si aggiungono ai muri **sia** in `measure_fatto` **sia** in
  `project_geometry`, con le fasi: fatto = `esistente` + `demolita`; progetto = `esistente` + `nuova`.
- **R98 – layer e APPID a parte.** `zone::ensure_layers` crea i quattro `AL-*` e gli APPID di `APPS`; il test
  `ensure_layers_creates_once_with_colors` fissa il numero. Le aperture usano `aperture::ensure_layer` (layer
  `AL-APERTURE` + APPID `ARCHLINE_APERTURA`), come `abaco_table::ensure_layer`, senza toccare quel test.

## Task

### 1. Modello (`src/bim/aperture.rs`, nuovo)
- Costanti in `bim/mod.rs`: `LAYER_APERTURE = "AL-APERTURE"`, `APP_APERTURA = "ARCHLINE_APERTURA"`.
- `ApertureKind {Porta, Finestra, Portafinestra}`, `ApertureState {Esistente, Demolita, Nuova}` (con `key`,
  `from_key`, `aci`: demolita 2, nuova 1, esistente BYLAYER), `Aperture {kind, state, axis:
  ([f64;2],[f64;2]), thickness, height: Option<f64>, sill: Option<f64>, threshold: bool, lintel: bool,
  bearing: Option<f64>}`.
- `encode`/`decode` come `zone.rs` (versione `Integer16(1)`, chiavi per nome, sconosciute ignorate;
  `Position3D`, `Distance`, `Real`), `aperture_polyline(&Aperture)` (rettangolo del vano da `WallRect`,
  `LwPolyline` chiusa su `AL-APERTURE`, colore dello stato), `apertures_in(&Document)` (solo modello),
  `ensure_layer`.
- Test: round trip XDATA, ordine delle chiavi documentato, colori per stato, lettura che ignora ciò che
  non è un'apertura, DXF e DWG (come `zones_survive_a_dxf_save_and_reload`), MOVE/MIRROR spostano l'asse
  (come il test dei locali), `migrate_document` non cambia nulla.

### 2. Comandi `AL_PORTA` e `AL_FINESTRA` (`src/bim/aperture_command.rs`, `src/app/commands/bim_aperture.rs`)
- `ApertureCommand` sul modello di `ZoneCommand`: punto 1, punto 2 (asse alle spalle del vano) →
  `rect_from_axis` (max 1,50 m); `NoFaces`/`OneFace` → chiede lo spessore (mai indovinato) →
  `rect_from_axis_thickness`; stato `[Esistente/Demolita/Nuova]`; altezza (invio = `n.d.`); davanzale per le
  finestre; eco in metri e avvisi fuori intervallo (`height_echo`, `bim_height_range_warning`).
- Commit con `CmdResult::CommitEntitiesAndExit`, `preserve_commit_layer`/`_style`, layer in un passo
  `bim_ensure_layers`-simile a sé la prima volta, `inventory::submit!` e `dispatch_bim` (due nomi nuovi),
  rifiuto in spazio carta e nell'editor di blocchi, `AL_UNITA` se l'unità manca.
- Test: casi A (polilinee e linee) con porta nel tramezzo: larghezza 0,90 dal disegno, spessore 0,10; asse
  fuori dai muri → chiede lo spessore; due nomi di comando; storia degli annullamenti; rilievo intatto.

### 3. Chiusura del locale (`aperture::thresholds`, `refresh.rs`, `project.rs`)
- `thresholds(apertures, fase) -> Vec<EntityType>`: per `porta` e `portafinestra` con `threshold = sì`, i
  due lati lunghi del vano (`WallRect::long_sides`) come linee sul layer `"0"` con handle sintetici, come i
  varchi in `project.rs`. Un'apertura non rettangolare o con spessore ≤ `tol` non genera nulla.
- Agganci: `measure_fatto` (R97) e `project_geometry` → `room_entities`. Nessun disegno nel DWG.
- Test: caso A senza linea di soglia, porta 0,90 nel tramezzo: A 12,00 e B 11,70 m² (due locali, stesse
  superfici di prima), nessun «non chiuso»; area del vano non nei locali; stesso risultato con la linea di
  soglia a mano; `finestra` non chiude nulla; fase: una porta `nuova` non chiude il fatto, una `demolita`
  non chiude il progetto; `HPGAPTOL` invariato.

### 4. `AL_APERTURA_DATI` (`bim_aperture.rs`)
- Opzioni `Stato/Altezza/Davanzale/Soglia/Architrave/Appoggio/Rimuovi` su aperture selezionate, come
  `AL_ALTEZZA S`: un passo (`apply_property_op`), layer bloccati saltati e contati, eco e avviso di intervallo,
  scrittura con `set_entity_xdata(..., APP_APERTURA, ...)` che scarta le voci raw DWG. `Appoggio` anche di
  progetto: nuova voce nel record `ARCHLINE_PROGETTO` (`record.rs`, solo se c'è, come la voce 40).
- Test: ogni opzione, selezione mista, layer bloccato, annullamento in un passo.

### 5. Quantità (`src/bim/aperture_quantities.rs`)
- `ApertureRow` con codice (`P1`, `F1`, `PF1`), vano m² (larghezza × altezza), volume del vano, soglia,
  architrave (larghezza + 2 × appoggio, `n.d.` senza appoggio), davanzale; stati come `height.rs`
  (`dal disegno`, `inserita`, `n.d.` con il motivo); riga completa = ha il vano.
- Detrazioni: `deductions_for_demolitions`, `..._for_new_walls` (zona con asse che contiene il vano,
  `tol`), `wall_surface_of_rooms` (perimetro × altezza − vani il cui asse interseca un lato dell'anello;
  `n.d.` se manca un'altezza incontrata, mai una parete parziale).
- Test: i numeri della spec §7 a mano (0,09 m², 1,89 m², 0,189 m³, 0,81 m³, 1,68 m²), caso `n.d.`.

### 6. Tabella e CSV (`abaco.rs`, `abaco_render.rs`, `abaco_table.rs`)
- Un campo in `Abaco` (`aperture`), `TableKind::Aperture` (+ `kind_key`/`kind_from_key`/`kind_name`),
  `render_tables` crea la tabella solo se ha righe, colonna «Pareti» nella tabella dei locali, `to_csv`
  con le righe `apertura` e `detrai`, `drawing_only` invariato. R92 e le regole di sostituzione sul posto
  restano.
- Test: rendering, CSV (colonne e `detrai`), sostituzione della quarta tabella, blocco `*T`.

### 7. Estrazione (`extract.rs`)
- Ogni elaborato tiene le aperture della sua fase (§5.4 della spec), come contorni normali senza XDATA né
  APPID (`strip_our_xdata`/`scrub_our_data` già lo fanno); l'esclusione per fase è l'analogo di
  `carries_zone_record`. L'abaco ricalcolato dell'estratto include la tabella Aperture (linee e testi).
- Test: tre DWG riletti, colori, nessun `ARCHLINE_*`, rilievo intatto.

### 8. Menu, barra, ribbon, icone
- Tre comandi (`AL_PORTA`, `AL_FINESTRA`, `AL_APERTURA_DATI`) in `classic_menu.rs`, tabella `RIBBON` di
  `menu_icons.rs` con tre svg (24×24, colori solo come parole chiave `yellow`/`red`, accento `#6DB7ED`),
  `modules/ristrutturazione/mod.rs`, test dei nove→dodici comandi in `toolbar_registry.rs`,
  `menu_icons.rs` e `modules/ristrutturazione`. Nessun cambio in `toolbar_layout.rs`.

### 9. Banco, documenti, registro
- `tests/conformance/bim_extract.py`: aperture nel disegno sintetico, controlli su `AL-APERTURE`, sul blocco
  `*T` della quarta tabella, sui tre DWG. Aggiornare `CLAUDE.md` (sezione BIM parte 3: modello, comandi,
  mappa dei file, ganci, test) e il registro delle decisioni con «Da confermare» e «Da vedere a schermo».

## Ganci in codice di upstream (da tenere minimi)

`src/app/commands/mod.rs` (`mod bim_aperture;`), `src/bim/mod.rs` (`pub mod aperture;` …), nessun altro. Se
serve un gancio in `command_driver/utilities.rs` (argomenti con spazi) si motiva nel registro.

## Verifica finale

`cargo test --locked --lib bim::`, `--lib app::bim_tests`, `--lib commands::bim_a`; `cargo build --release
--bin ArchLine` e `python tests/conformance/bim_extract.py target\release\ArchLine.exe`; `unittest` del
banco. Da vedere a schermo su Windows: tre icone e barra a dodici comandi, prompt dei comandi, contorni
giallo/rosso. Da vedere in AutoCAD vero: la quarta `TABLE`, il contorno delle aperture nei DWG estratti.
