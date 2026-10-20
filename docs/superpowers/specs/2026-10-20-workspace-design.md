# Workspace: pulsante, menu e disposizioni salvate

Stato: implementata (piano `2026-10-20-workspace.md`); da verificare a schermo dall'utente. Branch `claude/merge-upstream-2026.42`. Data: 2026-10-20.

## 1. Contesto e scopo

In AutoCAD un *workspace* è una disposizione dell'interfaccia (barre, palette, ribbon) che si cambia da un
pulsante nella barra di stato. ArchLine oggi ha due interfacce, scelte all'avvio con la variabile d'ambiente
`ARCHLINE_WORKSPACE` (classica con barre, o ribbon di upstream), e una disposizione delle barre salvata in
`settings.json` (`toolbars`, `ToolbarLayout`). Non si può cambiare a metà sessione e non si possono
salvare più disposizioni.

Obiettivo: il pulsante **Workspace Switching** nella barra di stato, con il menu di AutoCAD, e workspace che
cambiano l'interfaccia al volo, senza riavviare.

Decisioni prese con l'utente (2026-10-20):

- Un workspace è **la disposizione dei menu e delle barre** (non i colori, non il contenuto del disegno).
- Due workspace predefiniti: **AutoCAD Classic** (barre come in AutoCAD, §4) e **OpenCAD Studio** (il ribbon
  di upstream).
- Le voci del menu sono quelle di AutoCAD: l'elenco dei workspace con la spunta su quello attivo, poi
  *Save Current As…*, *Workspace Settings…*, *Customize…*, *Display Workspace Label*. Le quattro voci
  sotto l'elenco **funzionano subito**.
- Drafting & Annotation, 3D Basics e 3D Modeling **non** ci sono.
- Le barre mancanti (Standard, Styles, Draw Order) sono già state aggiunte e vanno usate da AutoCAD Classic.
- ArchLine Dark è il tema predefinito (già fatto); un workspace **non** cambia il tema.

## 2. Fuori portata

Editor completo di menu e barre come il CUI di AutoCAD (creare barre nuove, scegliere i comandi dei
pulsanti); workspace con palette, barra di stato o tema propri; esportare e importare workspace; comandi
a riga di comando `WSCURRENT`/`WSSAVE`; i workspace 3D di AutoCAD; il passaggio da classico a ribbon
*per scheda* (vale per l'intera finestra).

## 3. Modello

Nuovo file proprio `src/workspaces.rs`, puro (senza iced), serializzato in `settings.json` come sezione
`workspaces` con `#[serde(default)]`, così un `settings.json` vecchio si legge ancora.

- `Interface`: `Classic` | `Ribbon`.
- `Workspace { name, interface, toolbars: ToolbarLayout }`. In `Ribbon` il layout delle barre è conservato
  ma non mostrato.
- `WorkspaceSet { current: String, saved: Vec<Workspace> (solo quelli dell'utente), auto_save: bool,
  show_label: bool }`.
- I due predefiniti non stanno nel file: si costruiscono dal codice (`builtin()`), così un aggiornamento
  dell'app li può correggere. Non si possono eliminare né rinominare; si possono modificare solo salvando
  con un altro nome.
- Nomi unici senza distinzione di maiuscole, non vuoti, al massimo 40 caratteri.
- `ToolbarLayout` di `settings.json` resta la disposizione *di lavoro* (quella che l'utente sta
  modificando); il workspace attivo dice da quale disposizione salvata si è partiti.

**Cambio workspace** (`apply(name)`): carica `interface` e `toolbars` del workspace scelto nella
disposizione di lavoro. Se `auto_save` è acceso, prima scrive la disposizione di lavoro nel workspace che
si lascia, ma solo se è un workspace dell'utente: le modifiche a un predefinito si perdono al cambio, come
con *Do not save*, e per tenerle si usa *Save Current As…*. `auto_save` è spento di default, come in
AutoCAD ("Do not save changes to workspace").

**Interfaccia dal vivo.** `workspace::is_classic()` oggi legge la variabile d'ambiente una volta
(`OnceLock`) ed è chiamata in una decina di punti (`workspace.rs`, `app/view/mod.rs`, i test del dock). Diventa uno stato modificabile
(`AtomicBool`) con `set_classic(bool)`, inizializzato dalla variabile d'ambiente, poi dal workspace
salvato. Le funzioni `ribbon_shown`, `menu_bar_shown`, `classic_active`, `bottom_chrome_shown` restano con
la stessa firma.

## 4. AutoCAD Classic: disposizione predefinita

Dal confronto con lo screenshot di AutoCAD 2025 dell'utente (barre che esistono già in ArchLine):

| Posizione | Barre, da sinistra a destra |
|---|---|
| Alto, riga 1 | Standard, Dimension, Inquiry |
| Alto, riga 2 | Layers, Styles |
| Alto, riga 3 | Insert, Reference |
| Sinistra | Draw |
| Destra | Modify, Draw Order |
| Basso | Visual Styles, Preset Views, Modeling, Solid Editing |

Le altre barre restano nascoste. È una scelta dell'implementatore se qualche barra non entra nella
larghezza: la riga si divide con le regole già esistenti di `ToolbarLayout`.

**OpenCAD Studio**: `interface = Ribbon`, nessun layout di barre.

## 5. Interfaccia

**Pulsante.** Nella barra di stato, prima dell'ultima maniglia (la personalizzazione), con l'icona a
ingranaggio di AutoCAD e la descrizione "Workspace Switching". Apre il menu con `status_menu::menu_bar`
(lo stesso componente della personalizzazione), che chiude il menu dopo la scelta.

**Menu**, in quest'ordine: i workspace (predefiniti poi quelli dell'utente per nome; spunta su quello
attivo); separatore; *Save Current As…*; *Workspace Settings…*; *Customize…*; *Display Workspace
Label* (con spunta). Un solo separatore, come nello screenshot dell'utente.

**Etichetta.** Con *Display Workspace Label* acceso, il nome del workspace attivo compare nella barra di
stato accanto al pulsante. Di default spenta, come in AutoCAD.

**Save Current As…** Piccola finestra modale (stile `block_save_as`): campo nome, OK/Annulla, errore se
vuoto, troppo lungo o uguale a un predefinito. Se il nome è di un workspace dell'utente chiede di
sostituirlo. Salva interfaccia e disposizione di lavoro e rende quel workspace l'attivo.

**Workspace Settings…** e **Customize…** sono due schede della stessa finestra modale "Workspaces":

- *Settings*: elenco dei workspace (predefiniti in sola lettura) con Imposta come corrente, Rinomina,
  Elimina, Su/Giù (ordine nel menu); interruttore "Salva automaticamente le modifiche al cambio di
  workspace" (spento di default).
- *Customize*: elenco di tutte le barre (le stesse del tasto destro) con casella per mostrarle o
  nasconderle, più "Ripristina workspace" (ricarica la disposizione salvata del workspace attivo). Vale
  solo con l'interfaccia classica; con il ribbon la scheda spiega che le barre non sono mostrate.

Stato nell'app come per le altre finestre (`ModalKind`, campo in `App`), un solo `Message::Workspace(...)`
per ridurre i punti di contatto con il codice di upstream. Logica in `src/app/workspaces.rs`, finestra in
`src/ui/window/workspaces.rs`, voci del menu in `src/ui/statusbar/workspace_menu.rs`.

## 6. Comportamento da rispettare

- Al primo avvio dopo l'aggiornamento: `current = "AutoCAD Classic"` (o "OpenCAD Studio" se
  `ARCHLINE_WORKSPACE=ribbon`) e la disposizione di lavoro esistente **non viene toccata**.
- La variabile `ARCHLINE_WORKSPACE` decide l'interfaccia solo all'avvio e solo se non c'è un workspace
  salvato; dopo, vince il workspace.
- Cambiando workspace non si perde mai una modifica del disegno e non si chiudono documenti; i comandi in
  corso si annullano come per un cambio di scheda; menu aperti e trascinamenti di barra si chiudono.
- Se si lascia un workspace predefinito con modifiche alle barre non salvate (e il salvataggio automatico è
  spento), la riga di comando avvisa che sono state scartate e suggerisce *Save Current As…*.
- **All'avvio la disposizione di lavoro non viene sostituita**: si imposta solo l'interfaccia del workspace
  corrente. Il layout di un workspace si carica solo quando l'utente lo sceglie.
- Con la pagina iniziale e con schermo pulito (clean screen) valgono le regole di `workspace.rs` di oggi.
- Il tema resta quello scelto.

## 7. Scelte dell'implementatore (da controllare)

Chiuse senza chiedere all'utente; se una non va bene si cambia qui.

1. **Primo avvio con un `settings.json` esistente.** Se il file ha già la sezione `toolbars`, la
   disposizione di lavoro dell'utente viene salvata come workspace dell'utente "My Workspace" e resta
   quella corrente: non si perde nulla. Senza sezione (installazione nuova) il corrente è AutoCAD Classic.
   Con `ARCHLINE_WORKSPACE=ribbon` e nessun workspace salvato, il corrente è OpenCAD Studio.
2. **Installazione nuova.** `AppConfig::default()` e TOOLBARRESET usano la disposizione di AutoCAD Classic
   (§4), costruita da una funzione nuova `ToolbarLayout::autocad_classic()`, unica fonte di `builtin()`.
   `ToolbarLayout::default()` **non cambia** (resta il default storico, su cui poggiano una ventina di test
   e le funzioni che non devono cambiare comportamento). Un `settings.json` senza la sezione `toolbars` dà ora il layout di `AppConfig::default()`, cioè `autocad_classic()`; il test `toolbars_default_when_absent_and_round_trip` si aggiorna.
   Primo avvio dopo l'aggiornamento (`current` vuoto): se la disposizione letta è uguale a
   `autocad_classic()` il corrente è AutoCAD Classic; altrimenti diventa "My Workspace" (punto 1).

3. **Elimina il workspace corrente, o `current` che nomina un workspace inesistente.** Si torna ad AutoCAD
   Classic e lo si applica (l'interfaccia cambia davvero).
4. **Cambio dal vivo classico/ribbon.** Il piano comincia con una verifica di tutto ciò che dipende dalla
   modalità scelta all'avvio (stato del ribbon, geometria di dock e palette, stime di `TOP_CHROME` e
   `BOTTOM_CHROME` per le barre). Se un pezzo non regge il cambio dal vivo, si corregge; il ripiego
   "serve riavviare" è ammesso solo per quel pezzo e va detto all'utente, non assunto.
5. **Ripristina workspace.** Per un predefinito ricarica `builtin()`, per uno dell'utente l'ultima versione
   salvata. Nessun indicatore di "modificato" nel menu.
6. **Save As su un nome già di un workspace dell'utente** (anche il corrente) chiede sempre conferma di
   sostituzione.

## 8. Rischi noti

- `view_main` e `start_page_content` sono enormi: niente annidamento lì (trappola 1 del CLAUDE.md); le
  parti nuove stanno in funzioni `#[inline(never)]`.
- Il menu usa `status_menu` (non `iced_aw::ContextMenu`): se si scelgono altri componenti, le righe devono
  essere `mouse_area` (trappola 2).
- Due test del dock disegnano il ribbon con `ARCHLINE_WORKSPACE=ribbon`: devono continuare a passare
  (l'ambiente deve ancora inizializzare lo stato).
- La firma di `statusbar::view` è già lunga: il pulsante riceve i dati con una sola struct nuova.

## 9. Verifica

Test unitari del modello (nomi, predefiniti, round trip serde, `settings.json` vecchio senza sezione,
auto-save acceso e spento); test dei flussi con i messaggi dell'interfaccia (cambio workspace dal menu,
Save As con nome valido e non valido, Elimina del corrente, Ripristina); test sul layout di AutoCAD Classic
(ogni barra citata esiste nel registro). La verifica visiva è dell'utente su Windows (screenshot).
