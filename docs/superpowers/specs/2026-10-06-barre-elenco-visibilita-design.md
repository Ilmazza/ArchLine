# Elenco delle barre, visibilità e clic prolungato (parte 2)

Stato: bozza da rivedere. Branch `lavoro`. Data: 2026-10-06.

## 1. Contesto e scopo

È la parte 2 del lavoro sulle barre (vedi `2026-10-05-barre-menu-classica-design.md` §1). Le parti 3 e 4
(`2026-10-06-barre-sganciabili-design.md`) hanno dato barre agganciabili ai quattro bordi e flottanti dentro la
finestra, ma le barre sono 6 fisse e sempre visibili.

Obiettivo, come in AutoCAD:

- **Tasto destro** su un'icona (o sull'impugnatura, o sul titolo di una barra flottante): si apre l'elenco di
  **tutte le barre esistenti**, in ordine alfabetico, con la **spunta** su quelle visibili.
- **Spuntare** una barra nascosta la apre **flottante**; togliere la spunta la nasconde. La scelta è salvata.
- **Tasto sinistro prolungato** su un'icona con varianti: apre il flyout delle varianti (oggi sul tasto destro).

Decisioni prese con l'utente:

- Le barre sono **una per ogni gruppo del ribbon** con almeno un pulsante (circa 30), non solo le 6 attuali.
- Tasto destro = sempre l'elenco delle barre; varianti = clic prolungato (non un menu unico).
- Approccio al clic prolungato: **timer a messaggi + overlay dei dropdown del ribbon** (non un widget
  personalizzato).

## 1bis. Correzione: le barre rispecchiano i menu (dopo la prova dell'utente)

La prima realizzazione generava una barra per ogni gruppo del ribbon (~34): barre "a spezzatino" che non
corrispondevano ai menu a tendina (Dimensions, Visual Style…), con Draw e Modify incomplete rispetto ai menu.
Decisione dell'utente: **una barra per menu, un pulsante per comando**.

- Le barre Draw e Modify (chiavi storiche) hanno gli stessi comandi, nello stesso ordine e con gli stessi
  separatori dei menu Draw e Modify (`classic_menu::menus()`).
- Ogni altro menu con comandi propri è una barra `menu:Titolo` (File, Edit, View, Insert, Format, Tools, Dimension,
  Help); ogni sottomenu con almeno 2 comandi è una barra a sé (Zoom, Visual Styles, Inquiry, Text, Geometric,
  Dimensional, Manage). Parametric, fatto solo di sottomenu, non è una barra.
- Una voce di menu = un pulsante; un sottomenu dentro una barra = un pulsante con le varianti sul clic prolungato.
  Le righe che non sono comandi (cronologia, schede finestra) non compaiono.
- Icona: quella del menu (ribbon o glifi del quick access); dove l'app non ne ha una, due lettere (iniziali). Il
  tooltip porta il nome intero.
- Annotation, Block, Measure e Layers restano barre storiche (stesse chiavi di `settings.json`); le chiavi dei vecchi
  gruppi del ribbon (`modulo:Titolo`) non esistono più e vengono scartate al caricamento.

## 2. Fuori portata

Pulsante di chiusura sulle barre flottanti (si chiudono dall'elenco); personalizzazione dei pulsanti; barre fatte
solo di combo (gruppi `LayerComboGroup`, `PropertiesGroup`, `StyleComboGroup`); barre come finestre di sistema;
barre di plugin aggiunte a runtime dopo l'avvio (si leggono al lancio).

## 3. Identità e registro delle barre (`src/ui/toolbar_registry.rs`)

`ToolbarId` resta un enum `Copy` con una variante in più, `Group(&'static str)`, per le barre generate dai gruppi
del ribbon (anziché diventare un newtype: così non si toccano i ~100 usi esistenti). Il registro dei moduli si
costruisce una volta (`OnceLock`); `ToolbarId::all()` e `ToolbarId::BUILTIN` prendono il posto di `ALL`.

- **Chiavi.** Le 6 barre attuali mantengono la chiave già salvata in `settings.json`: `Draw`, `Modify`,
  `Annotation`, `Block`, `Measure`, `Layers`. Tutte le altre: `modulo:Titolo` (es. `annotate:Dimensions`,
  `insert:Block`).
- **Quali gruppi diventano barre.** Ogni gruppo `ribbon_groups()` di ogni modulo con almeno un elemento
  riducibile a pulsante (`Tool`, `LabeledTool`, `LargeTool`, `Dropdown*`, `ToolGrid`: la stessa `flatten` di
  oggi, ora senza il filtro sul solo modulo `draw`). `Layers` resta il caso speciale con i suoi combo
  (`classic_layers::layer_row`).
- **Nomi nell'elenco.** Il titolo del gruppo; se due barre hanno lo stesso titolo, ogni barra non built-in prende il
  modulo: `Block` (quella storica di Draw) e `Block (Insert)`, `Plot (Layout)` e `Plot (View)`. Ordine alfabetico, senza distinguere
  maiuscole.
- **`items_for(id)`** cerca nel registro invece di fare `match` su un enum; `bar_length`, `floating_id` e
  `allowed_on` lavorano sulla stringa. `allowed_on` resta: solo `Layers` è limitata a Top/Bottom/flottante.

## 4. Modello (`src/ui/toolbar_layout.rs`)

- Nuova variante `Placement::Hidden { home: Option<DockSlot> }`, serializzata come le altre.
- **Default** per una chiave non presente nel file: le 6 barre attuali agganciate come oggi; tutte le altre
  `Hidden`. `lanes(edge)` e `floating()` ignorano le `Hidden`.
- `ToolbarLayout::set_visible(id, bool)` (calcola da solo l'indice di cascata, nessun parametro `cascade_n`):
  - mostrare una barra `Hidden` la rende `Floating { x, y, home }` a cascata: `x = 120 + 28·n`,
    `y = 130 + 28·n`, con `n` = numero di barre flottanti già presenti modulo 8 (il clamp al render la tiene in
    finestra); `home` è il vecchio `home` della barra;
  - nascondere una barra agganciata o flottante la rende `Hidden { home }` con l'ultima posizione agganciata;
    le lane che restano vuote si compattano.
- `sanitized()`: scarta le chiavi che il registro non conosce, riempie le mancanti col default (senza scrivere le
  ~30 barre di gruppo nascoste), corregge `Layers` su un lato.
- Retrocompatibile: un `settings.json` con solo le 6 chiavi di oggi si legge senza errori; le chiavi `Hidden`
  non compaiono nei file vecchi.

## 5. Menu delle barre (tasto destro)

`iced_aw::ContextMenu` su: ogni pulsante di barra, l'impugnatura, il frame di una barra flottante. Sostituisce il
flyout che oggi vive sul tasto destro delle icone con varianti.

- Contenuto: tutte le barre del registro, in ordine alfabetico, ciascuna con `icons::themed_check_cell(visibile)`
  e il nome; dentro uno `scrollable` con altezza massima `min(finestra − 80, 600)` px.
- Righe = `mouse_area` con `on_press`, **mai `button`** (trappola 2 di `CLAUDE.md`).
- Il menu si chiude da solo al rilascio del clic sinistro (comportamento di `iced_aw`), quindi spuntare una voce lo
  chiude, come in AutoCAD.
- L'elenco (`Arc<[BarEntry]>` con id, nome, visibile) si costruisce una volta per vista in `toolbar_dock::frame` e
  si condivide con tutti i pulsanti.
- `ToolbarMsg::Toggle(id)`: `set_visible(id, !visibile)`, poi `save_config()`.

## 6. Clic prolungato sulle varianti

- Per i pulsanti **con varianti** la pressione non esegue più il comando: il comando parte al **rilascio** se il
  tasto è stato tenuto meno di `HOLD_MS = 400`; oltre, si apre il flyout e il rilascio non esegue nulla. I
  pulsanti **senza varianti** restano immediati.
- Stato in `OpenCADStudio`: `tool_hold: Option<ToolHold { tool, pressed_at: Instant, fired: bool }>`. Finché è
  `Some` e non `fired`, la sottoscrizione aggiunge `iced::time::every(50 ms)` → `ToolbarMsg::HoldTick(Instant)`,
  che apre il flyout al superamento della soglia (stesso schema delle sottoscrizioni condizionali già presenti).
  Un rilascio fuori dal pulsante o l'uscita del cursore annulla senza eseguire.
- Il pulsante con varianti usa `mouse_area` + `hover` (un `button` cattura la pressione e il `mouse_area` esterno non
  la vedrebbe); il rilascio è gestito da `HoldEnd`, il tempo da `HoldTick`.
- Flyout: contenuto di `flyout_row`/`flyout` di oggi (pubblica `RibbonToolClick`); aperto con l'overlay dei
  dropdown del ribbon: il pulsante è avvolto in `PosReport::owned("cflyout:<id>")`, e
  `Ribbon::place_dropdown` lo ancora sotto. L'overlay si aggiunge in `view_main` con un solo `.or_else(...)` dopo
  `dropdown_overlay` (la logica sta in `classic_toolbar::flyout_overlay`).
- Il clic su una riga chiude il flyout senza altro aggancio: `on_ribbon_tool_click` chiude già qualunque dropdown
  aperto (`dialog.rs:119`). L'id `cflyout:` non collide con quelli del ribbon (il test lo verifica: l'overlay del
  ribbon lo ignora).

## 7. Rischi e punti da verificare

1. Chiusura del flyout dopo il clic su una riga e ancoraggio sotto il pulsante in barre verticali, a destra e
   flottanti (§6).
2. Il menu delle barre ha circa 30 voci: scroll con la rotella e chiusura al rilascio sulla barra di scorrimento.
3. Il numero di barre con almeno un pulsante dipende dal contenuto dei gruppi: il test del registro lo fissa; se un
   gruppo atteso non produce pulsanti non compare nell'elenco.
4. Stack di `rustc`: nessuna logica nuova in `view_main` (solo chiamate a funzioni `#[inline(never)]`).

## 8. Test e fasi

**Unitari (modello):** default (6 visibili, resto `Hidden`); `set_visible` mostra a cascata e nasconde ricordando
`home`; lane compattate dopo aver nascosto; round-trip serde con `Hidden`; lettura di un `settings.json` con le
sole 6 chiavi; `sanitized` scarta chiavi sconosciute. **Registro:** nessuna chiave duplicata; ogni barra offerta ha
almeno un pulsante; i titoli duplicati vengono disambiguati; le 6 chiavi storiche esistono. **Widget reale**
(`iced_test`, già usato nel repo): tasto destro su un pulsante apre il menu con le spunte giuste; clic su una
riga la applica; clic breve su un pulsante con varianti esegue il comando; clic tenuto apre il flyout e non lo
esegue. **Visivi:** screenshot dell'utente su Windows.

Fasi, ognuna consegnabile da sola:

1. Registro dinamico, `Hidden`, `set_visible` e persistenza; le barre nuove sono nascoste e non c'è ancora il menu.
2. Menu col tasto destro con le spunte (apre le barre flottanti).
3. Clic prolungato per le varianti, che sostituisce il flyout sul tasto destro.

## 9. File

Nuovi: `src/ui/toolbar_registry.rs`. Toccati: `src/ui/toolbar_layout.rs`, `src/ui/toolbar_dock.rs`,
`src/ui/classic_toolbar.rs` (pulsanti con varianti, `items_for`), `src/ui/classic_layers.rs` (nessuna modifica
attesa), `src/app/update/toolbar.rs` (Toggle, HoldTick), `src/app/mod.rs` (`tool_hold`),
`src/app/view/mod.rs` (aggancio dell'overlay del flyout e della sottoscrizione), `src/app/update/file.rs` e
`src/app/config.rs` (solo `sanitized` con il registro).
