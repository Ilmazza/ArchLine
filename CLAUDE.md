# ArchLine — istruzioni per chi lavora sul codice (Claude Code locale o cloud)

## Progetto
Fork di [Open CAD Studio](https://github.com/HakanSeven12/OpenCADStudio) (Rust, iced; app GPL-3.0, codec DWG/DXF MPL-2.0). Nome: **ArchLine**. Obiettivo: workspace "AutoCAD classico" (barre Draw/Modify docked, riga di comando, palette) e, in seguito, uno strato architettonico come plugin (locali, abachi, IFC, quantità con fonte e stato).
Utente: Mauro Mazzarelli, architetto a Roma. Rispondere **in italiano**, da pari a pari, prima il deliverable. **Nessun valore inventato** su prezzi, quantità, calcoli; distinguere dato deterministico (IFC/DXF) da dato desunto da raster/PDF (bozza da verificare). Se il tipo di intervento (nuova costruzione, ristrutturazione, manutenzione, restauro) è ambiguo, chiedere prima.

## Build e test
```
cargo check --locked
cargo build --release --bin OpenCADStudio        # target\release\OpenCADStudio.exe
cargo test --locked --lib classic_toolbar        # altri filtri: start, i18n
python -m pip install ezdxf                       # una volta, per il banco di conformità
python -m unittest discover -s tests/conformance -p "test_*.py"   # test del banco (~40 s)
python tests/conformance/run.py target\release\OpenCADStudio.exe  # misura DXF/DWG con oracolo ezdxf (~35 s)
```
- `.cargo/config.toml`: `RUST_MIN_STACK` è a 64 MiB (upstream 8 MiB). Non rimuoverlo: senza, `rustc` è andato in stack overflow su Windows in release.
- CI: `.github/workflows/archline-windows.yml` compila su `windows-latest` a ogni push su `claude/**` e pubblica l'artefatto `ArchLine-windows-N` (Actions → corsa → Artifacts). `Tests` è il `ci.yml` di upstream. Lo stesso workflow esegue il banco di conformità (passo non bloccante, `continue-on-error`) e mette `conformance-report.md` nell'artefatto.
- Il container cloud non ha GPU: la verifica visiva si fa solo su Windows, con screenshot dell'utente.

## Branch e flusso
- L'utente lavora in locale su `lavoro` (da `claude/archline-integration`); GitHub è il backup (`git add -A`, `git commit`, `git push`).
- Il lavoro assistito va su branch `claude/…`; si unisce con PR. Non pushare su `lavoro` mentre l'utente lavora.
- `main` del fork = upstream: **non toccarlo senza conferma esplicita**. Upstream: `HakanSeven12/OpenCADStudio` (`git remote add upstream …`, merge periodico a lavoro salvato).
- Cronologia delle PR: #2 rebrand+offline, #3 workspace classico, #4 pagina iniziale ripulita, #5 CI Windows, #6 salta Web build check, #7 nasconde OCS Web/Send Feedback, #8 barre compatte. `claude/archline-integration` le contiene tutte.

## Cosa è ArchLine (file propri, per ridurre i conflitti col merge da upstream)
- `src/privacy.rs`: `APP_NAME`, `online()` (`ARCHLINE_ONLINE=1` riattiva aggiornamenti/Discussions/video/Patreon), `show_promo()` (`ARCHLINE_PROMO=1` riporta Donate, Sponsors, Reddit, Patreon, OCS Web, Send Feedback).
- `src/workspace.rs`: `is_classic()` (`ARCHLINE_WORKSPACE=ribbon` ripristina il ribbon); in classico la pagina iniziale ha solo la barra dei menu, senza ribbon né barre (`ribbon_shown`, `menu_bar_shown`).
- `src/ui/classic_toolbar.rs`: barre Draw (sinistra), Modify (destra), Layers e le barre dai menu (Dimension, Insert, Inquiry in alto di default; le altre dall'elenco col tasto destro), registro in `src/ui/toolbar_registry.rs`. Un pulsante per comando; clic prolungato sui sottomenu = flyout con le varianti.
- `src/ui/classic_start.rs`: pagina iniziale in stile AutoCAD (barra laterale, Recent a schede con sort/ricerca/griglia-elenco, colonna destra solo con `ARCHLINE_ONLINE`/`ARCHLINE_PROMO`); stato in `app.start_ui`, messaggio `Message::Start`, date di modifica caricate fuori dal `view` (`app/recent.rs`). Sotto 760 px, o con `ARCHLINE_WORKSPACE=ribbon`, resta la pagina a schede di prima. Spec: `docs/superpowers/specs/2026-10-07-pagina-iniziale-autocad-design.md`.
- `src/ui/classic_layers.rs`: seconda riga sotto la barra alta: gestione layer, menu layer, i 10 comandi layer del ribbon, menu Colore/Tipo linea/Spessore. Riusa overlay e messaggi del ribbon (`ToggleRibbonDropdown` + `PosReport`); i comandi si leggono dal gruppo "Layers" del ribbon. Segue l'oggetto selezionato come il ribbon (`sync_ribbon_from_selection`); con una selezione mista lascia vuoto il campo (`ribbon.mixed`), come la barra di AutoCAD.
- `tests/conformance/`: banco di conformità DXF/DWG (Python, ezdxf come oracolo indipendente; non è un test Rust e non entra in `cargo test`). Misura `OpenCADStudio --export` su 9 casi sintetici × R2000/R2018 × DXF→DXF e DXF→DWG→DXF; `expected.json` registra le divergenze note, con nota. Stati: OK / ATTESO / REGRESSIONE / MIGLIORATO; esce con 1 solo con `--strict`. Limiti: DWG verificato solo in modo indiretto (ezdxf non lo legge), estensioni calcolate con i testi sostituiti da un segnaposto, il caso `colors` non è misurato da nessun controllo, solo disegni sintetici (repo pubblico). Spec: `docs/superpowers/specs/2026-10-07-banco-conformita-design.md`; uso: `tests/conformance/README.md`.
- Punti di aggancio in codice upstream (tenerli minimi): `src/app/mod.rs` (task di rete all'avvio), `src/app/view/mod.rs` (`view_main`, `start_page_content`), `src/ui/ribbon` (ID dei menu e `combo_btn_style` resi `pub(crate)`).
- Non toccati di proposito: chiavi di registro, `locale_catalog.rs`, nome del package Cargo.

## Trappole già incontrate
1. `view_main` e `start_page_content` sono funzioni enormi: aggiungere annidamento lì può far andare in overflow lo stack di `rustc`. Mettere la logica in funzioni separate con `#[inline(never)]` e lasciare nel chiamante poche righe.
2. `iced_aw::ContextMenu` ricostruisce l'overlay a ogni `view`: le righe del menu devono essere `mouse_area` con `on_press`, **mai `button`** (il rilascio non pubblicherebbe nulla). Vedi la nota in `src/ui/window/xref_manager.rs`. `ContextMenu` si apre solo col tasto destro.
3. Il controllo `wasm32` (Web build check) è rosso anche su upstream (`src/app/update/blocks_palette.rs` usa API solo desktop). Nel fork è saltato da `if: github.repository_owner == 'HakanSeven12'` in `web-check.yml`; se compare rosso su un branch, quel branch non contiene la correzione.
4. Il hook di fine turno di Claude (cloud) segnala "commit non pubblicati" sui branch del repo aggiunto in sessione: è un falso positivo se `git ls-remote --heads origin <branch>` mostra lo stesso commit di `git rev-parse HEAD`.

## Aperti
barra dei menu (`iced_aw` feature `menu`, già attiva); icone più grandi e contrastate; hover nelle righe del flyout; analisi di gap `docs/gap-ocs-vs-autocad.md`; banco di conformità, passi successivi (comandi pilotati dall'API di automazione, disegni reali dell'utente fuori dal repo, promozione a cancello in CI una volta sistemati i difetti registrati in `expected.json`); plugin architettonico; segnalazione a monte del bug di codepage R2000 del codec (accenti con header ANSI_1252); registrazione del marchio ArchLine (da creare: a carico dell'utente).
