# Barra dei menu classica (parte 1 di 4)

Stato: bozza da rivedere. Branch `lavoro`. Data: 2026-10-05.

## 1. Contesto e scopo

Il workspace classico di ArchLine ha già le barre Draw, Modify, la striscia in alto e la riga dei layer
(`src/ui/classic_toolbar.rs`, `src/ui/classic_layers.rs`). Manca la **barra dei menu** di AutoCAD
(File, Edit, View, …, Help). Riferimento visivo: AutoCAD 2025, menu in solo testo sotto la barra del
titolo e sopra le schede documento.

Il lavoro completo richiesto da Mauro ha quattro parti, ognuna con la sua spec e consegnabile da sola:

1. **Barra dei menu** (questa spec).
2. Barre con nome (Standard, Styles, Layers, Properties, Dimension, Draw, Modify) con elenco a spunte
   al clic destro, visibilità salvata in `settings.json`.
3. Trascinamento delle barre dal grip e aggancio ai quattro lati, posizioni salvate.
4. Barre flottanti staccate dal bordo (da decidere allora: pannelli interni alla finestra o finestre di sistema).

Questa spec non decide nulla sulle parti 2–4, ma non le ostacola (vedi §10).

## 2. Fuori portata (parte 1)

Barre con nome e spunte, docking, barre flottanti, menu Express (non esiste codice corrispondente),
mnemonici Alt+lettera, voci disabilitate in base allo stato (per esempio Undo senza storia), spunte
dentro i menu, icone per le voci che non hanno un id nel registro del ribbon.

## 3. Posizione e comportamento

- Visibile solo con `workspace::classic_active(is_start, clean_screen)`, come le altre barre: non
  compare sulla pagina iniziale, in clean screen, né con `ARCHLINE_WORKSPACE=ribbon`.
- Ordine verticale: **menu**, schede documento, striscia in alto, riga layer, area di disegno.
- Un clic sul titolo apre il menu; con un menu aperto, passare sul titolo accanto dovrebbe aprire
  quello (comportamento di `iced_aw::MenuBar`: **non verificato**, da guardare a video).
- Le righe lanciano il comando e chiudono il menu.

## 4. Struttura dei menu

Una voce entra solo se il suo comando esiste (§8, primo test). Le voci non mappabili sono **escluse
dalla struttura**, non disabilitate. Esclusa: *Lineweight* (non esiste un comando `LWEIGHT`; lo spessore
si imposta dal combo della riga layer).

Notazione: `CMD` = `Message::Command`; `MSG` = messaggio diretto; `RIB` = derivata dai gruppi del
ribbon (stesso percorso di clic delle barre classiche).

| Menu | Voci |
|---|---|
| **File** | New `NEW` · Open `OPEN` · Save `SAVE` · Save As `SAVEAS` · Plot `PLOT` · Close `CLOSE` · Exit `QUIT` |
| **Edit** | Undo `UNDO` · Redo `REDO` · — · Cut `CUTCLIP` · Copy `COPYCLIP` · Copy with Base Point `COPYBASE` · Paste `PASTECLIP` · Paste as Block `PASTEBLOCK` · — · Select All `SELECTALL` · Find `FIND` |
| **View** | Regen `REGEN` · Regen All `REGENALL` · — · Zoom ▸ (`ZOOM WINDOW`, `ZOOM PREVIOUS`, `ZOOM EXTENTS`, `ZOOM ALL`) · Pan `PAN` · 3D Orbit `3DORBIT` · Visual Styles ▸ (`RIB` dal gruppo "Visual Style") · — · Clean Screen `CLEANSCREEN` · Properties `PROPERTIES` · Command History `MSG CommandHistoryToggle` |
| **Insert** | Block `BLOCK` · Insert `INSERT` · — · Xref `XATTACH` · Image `IMAGEATTACH` · PDF `PDFATTACH` · — · Field `FIELD` |
| **Format** | Layer `LAYERS` · Color `COLOR` · Linetype `LINETYPE` · — · Text Style `STYLE` · Dimension Style `DIMSTYLE` · — · Units `UNITS` · Limits `LIMITS` |
| **Tools** | Inquiry ▸ (`DIST`, `AREA`, `ID`, `LIST`) · — · UCS `UCS` · Drafting Settings `DSETTINGS` · — · Aliases `ALIASEDIT` · Shortcuts `SHORTCUTS` · Options `OPTIONS` · Plugins `PLUGINS` |
| **Draw** | Line `LINE` · Construction Line `XLINE` · Polyline `PLINE` · Polygon `POLYGON` · Rectangle `RECTANG` · Arc `ARC` · Circle `CIRCLE` · Donut `DONUT` · Spline `SPLINE` · Ellipse `ELLIPSE` · — · Point `POINT` · Hatch `HATCH` · Region `REGION` · — · Text ▸ (`TEXT`, `MTEXT`) |
| **Dimension** | Linear `DIMLINEAR` · Aligned `DIMALIGNED` · Arc Length `DIMARC` · Ordinate `DIMORDINATE` · Radius `DIMRADIUS` · Diameter `DIMDIAMETER` · Angular `DIMANGULAR` · — · Baseline `DIMBASELINE` · Continue `DIMCONTINUE` · Quick Dimension `QDIM` · — · Leader `LEADER` · Multileader `MLEADER` · Tolerance `TOLERANCE` · — · Dimension Style `DIMSTYLE` |
| **Modify** | Erase `ERASE` · Copy `COPY` · Mirror `MIRROR` · Offset `OFFSET` · Array `ARRAY` · — · Move `MOVE` · Rotate `ROTATE` · Scale `SCALE` · Stretch `STRETCH` · — · Trim `TRIM` · Extend `EXTEND` · Break `BREAK` · Chamfer `CHAMFER` · Fillet `FILLET` · — · Explode `EXPLODE` · Polyline Edit `PEDIT` |
| **Parametric** | `RIB` dai gruppi "Geometric", "Dimensional", "Manage" del modulo parametrico, un sottomenu per gruppo. Aggiunta dallo screenshot; si toglie con una riga. |
| **Window** | elenco delle schede aperte (`MSG TabSwitch(i)`, spunta sulla attiva) · — · Close All `MSG DocTabCloseAll` |
| **Help** | Help `HELP` · About `ABOUT` |

Note sui dati:
- Ho verificato i comandi con una scansione del sorgente (id del ribbon, nomi registrati in `inventory`,
  literal nel dispatcher). È una prova grezza: la prova vera è il test del §8.
- `COMMANDHISTORY` non è un comando digitabile ma l'azione della scorciatoia F2
  (`src/app/shortcuts.rs`), quindi usa il messaggio diretto.
- `CLOSEALL` non esiste come comando; esiste `Message::DocTabCloseAll`.
- Il comportamento di `HELP` in un fork offline (`privacy::online()`) non è verificato.
- I sottomenu `RIB` seguono l'ordine e le voci di upstream: nuovi comandi compaiono da soli dopo un merge.

## 5. Modello dati

Nuovo file `src/ui/classic_menu.rs`. L'albero è un dato statico (`OnceLock`), come in `classic_toolbar`.

```text
Menu   { key, entries: Vec<Entry> }
Entry  = Item { key, action, accel: Option<&'static str> }
       | Sub  { key, entries }
       | Sep
       | Dynamic(DynKind)

Action = Cmd(&'static str)          // Message::Command(...)
       | Msg(fn() -> Message)        // messaggio diretto (COMMANDHISTORY, DocTabCloseAll)

DynKind = Tabs                       // elenco schede (Window)
        | RibbonGroup { module, group }   // voci derivate dal registro
```

- `key` è la chiave inglese di traduzione e anche l'etichetta di ripiego.
- Le voci `Dynamic` servono anche alle parti 2 e 3 (per esempio un elenco di barre o "Blocca posizione").
- Per le voci con un id nel registro del ribbon l'icona è quella del registro, se presente.
- Costruzione: `fn menu_bar(ctx: &MenuCtx) -> Element<'static, Message>`, con
  `MenuCtx { bindings, tabs: Vec<TabEntry> }`. Il file `ui` non conosce `OpenCADStudio`.

## 6. Etichette e traduzioni

- Le etichette passano da una funzione dedicata che prova prima la lingua italiana, poi `t!()`.
- **Decisione presa con Mauro: "b"** (titoli italiani come nel suo elenco, *Quota* e *?*). La
  verifica ha trovato un costo non previsto: il test `i18n::tests::every_catalog_covers_and_formats_the_source_catalog`
  impone le **stesse chiavi Fluent in tutte e 20 le lingue**, e 19 etichette della bozza non hanno una
  voce nel catalogo. Farlo nel catalogo vorrebbe dire circa 30 chiavi nuove × 20 file `.ftl` più righe in
  `locale_catalog.rs`, file che il `CLAUDE.md` tiene intenzionalmente intatti per i merge da upstream.
- **Scelta di questa spec (b′, da confermare in revisione):** tabella italiana **di proprietà ArchLine**
  in un file nuovo, consultata solo quando la lingua corrente è l'italiano (`crate::i18n::loader()`,
  `current_languages()`: da confermare in implementazione). Per l'utente italiano il risultato è lo stesso
  di "b". Le altre lingue vedono la traduzione del catalogo dove esiste, l'inglese altrove.
  Se Mauro preferisce "b" alla lettera, si cambia solo questo capitolo.
- Titoli italiani: File · Modifica · Visualizza · Inserisci · Formato · Strumenti · Disegna · Quota ·
  Modifica · Parametrico (proposta) · Finestra · ?
- Nella tabella vanno le etichette senza voce nel catalogo (misurate: Tools, Parametric, Regen All,
  Visual Styles, Command History, Xref, Dimension Style, Inquiry, ID, UCS, Aliases, Construction Line,
  Rectangle, Donut, Single Line Text, Multiline Text, Arc Length, Quick Dimension, Polyline Edit) più
  quelle dove la traduzione del catalogo non è quella voluta (Dimension, Help). Proposte di testo, da
  rivedere da chi conosce la terminologia di AutoCAD in italiano: Rigenera tutto · Stili visivi ·
  Cronologia comandi · Riferimento esterno · Stile di quota · Interroga · SCU · Alias · Linea di
  costruzione · Rettangolo · Ciambella · Testo a riga singola · Testo multilinea · Lunghezza arco ·
  Quota rapida · Modifica polilinea.

## 7. Scorciatoie

- Mostrate a destra della riga, lette da `shortcut_bindings` dell'app (binding correnti, quindi
  riflettono le rimappature fatte in CUI), non scritte a mano.
- Una voce cerca la scorciatoia la cui azione è il suo comando (o il campo `accel` per i `Msg`).
- Se un comando ne ha più di una, vince la più corta, a parità l'ordine alfabetico (Redo: `Ctrl+Y`
  invece di `Ctrl+Shift+Z`).
- Formato: `CTRL+SHIFT+S` → `Ctrl+Shift+S`; i tasti funzione restano `F8`.

## 8. Test (TDD: scritti prima, visti fallire)

Modulo `classic_menu::tests`, eseguibile con `cargo test --locked --lib classic_`.

1. Ogni comando di ogni voce esiste: l'id (primo token) è in uno di tre insiemi: id del ribbon
   (compresi gli item dei dropdown), `crate::command::all_registered_command_names()`, oppure una
   lista esplicita di comandi gestiti solo dal dispatcher. Per i comandi della lista esplicita, una
   scansione `include_str!` dei file di `src/app/commands/` deve trovare il literal tra virgolette.
2. Nessun menu vuoto; nessun separatore all'inizio, alla fine o doppio; nessun comando ripetuto
   dentro lo stesso menu.
3. I 12 titoli sono quelli attesi e nell'ordine atteso.
4. Le voci `RibbonGroup` risolvono: ogni gruppo nominato esiste e non è vuoto.
5. Formato e scelta delle scorciatoie (Redo → `Ctrl+Y`; `F8`; azione senza binding → nessuna scritta).
6. Elenco schede: una riga per scheda, spunta solo sulla attiva.
7. Etichette: con lingua italiana *Dimension* → *Quota*, *Help* → *?*; ogni chiave della tabella
   italiana è usata da almeno una voce (niente override morti).

Fuori test, a carico di Mauro (non c'è GPU in cloud): verifica visiva, §9.

## 9. Aggancio e verifica visiva

- `view_main` ([view/mod.rs](../../../src/app/view/mod.rs), blocco attorno a riga 2181): due righe in un
  `if classic`, **prima** di `doc_tab_bar`. La logica sta in una funzione `#[inline(never)]` in un file
  proprio (`src/app/view/classic.rs` o equivalente), perché `view_main` è enorme e rustc può andare in
  overflow di stack (trappola 1 del `CLAUDE.md`).
- Per le righe del menu uso lo schema della barra di stato (`src/ui/statusbar/status_menu.rs`):
  `iced_aw::MenuBar` con righe `button` in `mouse_area`. Il `ContextMenu` delle barre usa `mouse_area`
  per un motivo diverso (ricostruisce l'overlay a ogni `view`); `MenuBar` ha uno stato proprio.
  Da confermare a video.
- Costo: `iced_aw` costruisce tutte le righe a ogni frame (circa 150 elementi). Non misurato.
- Build: `cargo build --locked --bin OpenCADStudio`; exe in `target\debug\OpenCADStudio.exe`.

Cosa deve controllare Mauro a video, su una scheda disegno:
1. La barra dei menu è sopra le schede, in solo testo, con 12 titoli.
2. Un menu aperto copre le barre sotto e si chiude con un clic fuori o con Esc.
3. Aprendo un menu e passando sul titolo accanto, si apre quello.
4. Un comando parte e il menu si chiude (per esempio Draw → Line).
5. Le scorciatoie a destra corrispondono a quelle vere (Ctrl+S, Ctrl+Z).
6. Zoom, Inquiry, Text e Visual Styles aprono il sottomenu laterale.
7. Window elenca le schede e passa da una all'altra; Close All chiude tutto.
8. Sulla pagina iniziale e con `ARCHLINE_WORKSPACE=ribbon` la barra non compare.
9. Cosa apre Help.

## 10. Compatibilità con le parti 2–4

- La barra dei menu non è una barra con nome e resta fissa: non entra nell'elenco a spunte né nel docking.
- `Dynamic` permette di aggiungere un elenco di barre e "Blocca posizione" in Window/View senza toccare il modello.
- Il codice che costruisce le barre (parte 2) dovrà separare le strisce attuali in barre con nome; non è in questa spec.

## 11. Rischi

- `MenuBar` e i `button` nelle righe (stesso schema della barra di stato, non provato qui).
- Cambio di barra al passaggio del mouse: non verificato.
- Stack di `rustc` in `view_main`: mitigato con funzione separata.
- Terminologia italiana: proposte mie, non verificate su AutoCAD italiano.
- Merge da upstream: toccati solo `view_main` (2 righe) e, se serve, la visibilità di un elemento `pub(crate)`.
