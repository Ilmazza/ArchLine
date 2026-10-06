# Barre sganciabili, agganciabili e flottanti (parti 3 e 4)

Stato: bozza da rivedere. Branch `lavoro`. Data: 2026-10-06.

## 1. Contesto e scopo

Nella spec della barra dei menu (`2026-10-05-barra-menu-classica-design.md`, §1) il lavoro sulle barre è
diviso in quattro parti. Questa spec copre insieme la **3** (trascinamento dal grip e aggancio ai quattro
bordi, posizioni salvate) e la **4** (barre flottanti). La parte 2 (barre con nome Standard/Styles/Dimension,
elenco a spunte, visibilità) resta fuori e questa spec non la ostacola (§9).

Oggi le barre del workspace classico hanno posizione fissa: Draw a sinistra e Modify a destra
(`wrap_center`), Annotation+Block+Measure fuse in una sola striscia in alto (`top_bar`), la riga dei layer
sotto (`layer_bar`). Non esiste un'identità per barra, e `dock.rs` gestisce solo pannelli larghi ai lati
sinistro e destro, senza stato flottante.

Obiettivo: come in AutoCAD, ogni barra si può trascinare da un'impugnatura, agganciare a uno dei quattro
bordi (anche più barre affiancate sullo stesso bordo) oppure lasciare **flottante**; la posizione è
ricordata tra le sessioni.

Decisioni prese con l'utente:

- Le barre flottanti stanno **dentro la finestra** di ArchLine (sopra l'area di lavoro). Le finestre di
  sistema separate (secondo monitor) sono escluse; eventuale seconda fase.
- Su un bordo possono stare **più barre affiancate**; se non c'è spazio si apre una riga o colonna in più.
- Approccio: **modello di layout dedicato in file propri**, non estensione di `DockState`.

## 2. Fuori portata

Barre con nome e spunte di visibilità; pulsante di chiusura delle barre; barre come finestre di sistema;
barra dei menu e barra delle schede documento (restano fisse); ribbon (`ARCHLINE_WORKSPACE=ribbon`) e
pagina iniziale (invariati); personalizzazione dei pulsanti; la barra verticale flottante `side_toolbar.rs`
(non è una barra classica).

## 3. Modello (`src/ui/toolbar_layout.rs`, puro, senza iced)

- `ToolbarId`: `Draw`, `Modify`, `Annotation`, `Block`, `Measure`, `Layers`. Annotation, Block e Measure
  oggi sono unite in `ClassicTools.extra`: vanno separate in tre liste, per gruppo del ribbon.
- `Edge`: `Top` (sotto le schede documento), `Bottom` (sopra la barra di stato), `Left`, `Right`.
- `Placement`: `Docked { edge, lane, index }` oppure `Floating { x, y, home: Option<DockSlot> }`.
  La *lane* è la riga o colonna nel bordo: 0 è la più vicina al bordo della finestra (per `Top`, la più alta); `index` è l'ordine nella lane.
  `home` è l'ultima posizione agganciata, per il ritorno con doppio clic.
- `ToolbarLayout`: mappa `ToolbarId -> Placement`.

**Default** = layout di oggi: Draw `Left`, Modify `Right`, Annotation/Block/Measure `Top` lane 0 (indici 0,1,2),
Layers `Top` lane 1.

**Regole** (funzioni pure, testate):

1. Orientamento: `Left`/`Right` verticale, `Top`/`Bottom` orizzontale, flottante orizzontale.
2. `Layers` ha combo e dropdown: ammesso solo su `Top`, `Bottom` o flottante. Una posizione non ammessa è
   corretta in `Top`.
3. `move_to(id, target)`: toglie la barra dalla vecchia posizione, la inserisce, compatta lane e indici
   vuoti (nessun buco).
4. `resolve_drop(cursor, win_size, layout) -> Target`: se il cursore è entro ~60 px da un bordo della
   finestra, aggancia a quel bordo con lane (dalla distanza dal bordo, in passi di una lane, ≈42 px; oltre
   l'ultima lane esistente ne apre una nuova) e `index` (numero di barre della lane il cui punto medio
   precede il cursore). Altrimenti `Floating` nel punto del rilascio.
5. La lunghezza di una barra per `resolve_drop` si stima: pulsanti × 36 px + separatori; per `Layers` una
   costante. È un'approssimazione dichiarata, senza leggere i bounds dei widget.
6. `clamp_floating(pos, size, win)`: tiene la barra flottante dentro la finestra.
7. Sanificazione in lettura: identificativi sconosciuti ignorati, mancanti riempiti col default, posizioni
   non ammesse corrette.

## 4. Persistenza

Campo `toolbars: ToolbarLayout` in `AppConfig` (`src/app/config.rs`), `#[serde(default)]`: i `settings.json`
esistenti si caricano senza errori. Tre punti da collegare, come per il dock: campo e `Default` in
`config.rs`, `current_config()` (`src/app/update/file.rs`), ripristino in `file.rs`. `save_config()` al
rilascio del drag e al ripristino. Le posizioni flottanti si salvano ma non si riscrivono quando la finestra
viene ridimensionata (il clamp avviene al render).

## 5. Rendering (`src/ui/toolbar_dock.rs`)

Colonna principale: menu, schede, **bordo Top**, `[bordo Left | centro | bordo Right]`, **bordo Bottom**,
barra di stato. Il centro resta com'è, pannelli del dock inclusi. Funzioni `#[inline(never)]` (trappola 1
di `CLAUDE.md`); in `src/app/view/mod.rs` restano poche righe che sostituiscono `top_bar`, `layer_bar` e
`wrap_center` con: la cornice dei bordi, il livello flottante, il livello di drag.

- **Barra** = impugnatura (punti, cursore "afferra") + contenuto. Il contenuto riusa le liste `&'static` e
  `item_el`/`separator` di `classic_toolbar.rs`; `Layers` riusa il contenuto di `classic_layers.rs`.
  Le barre prendono la dimensione naturale (non più `Fill`); è la **lane** a scorrere in caso di overflow.
- **Lane** = riga (orizzontale) o colonna (verticale) di barre affiancate, con `strip_style`.
- **Flottanti**: `pin` sopra il centro, striscia col nome come impugnatura, doppio clic sul nome =
  ritorno a `home`. Z-order sopra il centro, sotto modali e dropdown.

## 6. Trascinamento

Messaggi: `ToolbarGrab(id, offset)`, `ToolbarDragMove(Point)`, `ToolbarDragRelease`, `ToolbarDragCancel`
(`Esc`). Stato transitorio in `OpenCADStudio` come per il dock (`dock_dragging` ecc.): barra, offset di
presa, ultimo punto, target risolto.

- Durante il drag la barra resta al suo posto, attenuata (nessun salto di layout). Seguono il cursore un
  contorno "fantasma" della barra e l'evidenziazione della zona di aggancio con la linea di inserimento.
- Livello di drag: `mouse_area` a schermo intero con `on_move`/`on_release`, montato solo durante il drag
  (stesso schema del dock, `view/mod.rs` ~2092, e di `modal.rs`). Deve coprire tutta l'area sotto la barra
  del titolo, così le coordinate locali coincidono con quelle della finestra.
- Rilascio: `resolve_drop` → `move_to` → `save_config()`.

## 7. Rischi e punti da verificare

1. I dropdown dei combo di `Layers` (Colore, Tipo linea, Spessore) e il menu layer si ancorano ai bounds
   riportati da `PosReport`: da verificare dentro `pin`. Se non funziona, soluzione dedicata.
2. `iced_aw::ContextMenu` (flyout delle varianti) in barre flottanti e verticali: deve continuare a
   funzionare (trappola 2: righe `mouse_area`, mai `button`).
3. Stima delle lunghezze (§3.5) imprecisa per `Layers`: l'indice di inserimento può risultare approssimato.
4. Stack di `rustc`: nessuna logica nuova in `view_main`.

## 8. Test e fasi

**Unitari (modello):** default uguale al layout di oggi; `move_to` con compattazione; rilascio fuori dai
bordi = flottante; `Layers` rifiutata sui lati; clamp; round-trip serde; lettura di un `settings.json` senza
il campo; sanificazione. **Vista:** costruzione con barre su ogni bordo e con barre flottanti, sul modello di
`classic.rs`. **Esistenti:** restano verdi i test di `classic_toolbar`; quello sui duplicati va adattato alla
separazione di Annotation/Block/Measure. **Visivi:** screenshot dell'utente su Windows.

Fasi, ognuna consegnabile da sola:

1. Modello, persistenza, barre separate e rendering ai bordi, layout di default identico a oggi.
2. Trascinamento con aggancio a un bordo, anteprima, persistenza.
3. Barre flottanti, ritorno a `home`, verifica dei dropdown.
4. Comando di ripristino del layout e rifiniture.

## 9. Compatibilità con la parte 2

`ToolbarId` è un enum estendibile (Standard, Styles, Dimension si aggiungono come varianti) e il modello può
ricevere in seguito un flag di visibilità per barra senza cambiare le regole di posizione.

## 10. File

Nuovi: `src/ui/toolbar_layout.rs`, `src/ui/toolbar_dock.rs`. Toccati, con modifiche minime:
`src/ui/classic_toolbar.rs` (liste separate, contenuto riusabile), `src/ui/classic_layers.rs` (contenuto
riusabile), `src/app/view/mod.rs` (aggancio), `src/app/mod.rs` (stato del drag), `src/app/config.rs` e
`src/app/update/file.rs` (persistenza), `src/app/update/*` (handler dei messaggi).
