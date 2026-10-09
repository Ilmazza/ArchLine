# Finestra "Hatch and Gradient" (HATCH come in AutoCAD)

Stato: implementata secondo il piano docs/superpowers/plans/2026-10-08-finestra-hatch.md; verifica visiva da parte
dell'utente su Windows ancora da fare.

## 1. Scopo

Lanciando HATCH si apre una finestra come quella di AutoCAD (tab Hatch, Boundaries, Options, Islands, Boundary
retention, Inherit options) per scegliere le opzioni del retino, indicare le aree e creare il retino con OK.
Oggi tutto si fa da riga di comando (`P`, `A`, `L`, `O/S/B/N/D/Y`), senza anteprima del pattern.

Scelte dell'utente: **perimetro "solo ciò che il motore già sa fare"** (le altre opzioni compaiono grigie) e
**flusso fedele ad AutoCAD (B)**: dopo "Add: Pick points" si torna alla finestra e il retino nasce solo con OK.

## 2. Flusso

- HATCH (digitato, menu, barra) apre la finestra. `-HATCH` esegue il comando senza finestra, come oggi (§7).
- **Add: Pick points** / **Add: Select objects**: la finestra si nasconde (`active_modal = None`, lo stato resta in
  `app.hatch_dialog`) e parte un comando raccoglitore. Con Invio consegna le aree alla finestra, che si riapre; con Esc
  la riapre senza aggiungere nulla. Le aree si accumulano tra un "Add" e l'altro.
- **OK** crea un `HatchCommand` con impostazioni e aree e chiama `on_enter()`: i percorsi di commit esistenti
  (`handle_commit_hatch*` in `command_driver/entity_commit.rs`: associativo, separati, contorni, undo) restano quelli di
  oggi. Disabilitato finché non c'è almeno un'area valida. **Cancel**, X ed Esc a finestra visibile scartano lo stato.
- **Preview**: nasconde la finestra, mostra il retino con le impostazioni correnti (§4); Invio o Esc riaprono la finestra.
  Disabilitato senza aree.
- **Click to set new origin** (con "Specified origin"): nasconde la finestra, prende un punto, riapre.
- Meccanismo di ritorno: quello di Wblock/BlockDef (`WblockPickBasePointCommand` + `Dispatch("…")` + riapertura in
  `src/app/commands/blocks.rs`), con in più il payload tipizzato di §3.
- Gli Invio/Esc dei quattro flussi nascosti passano dal comando raccoglitore; Esc a finestra visibile segue Wblock
  (`update/mod.rs` ~406-436): chiude e scarta; Invio equivale a OK, ma solo se OK è abilitato.

## 3. Stato e contratto delle aree

```
HatchRing   = Vec<[f64; 2]>                  // coordinate locali del piano di lavoro
HatchRegion { rings: Vec<HatchRing> }        // rings[0] = esterno; rings[1..] = anelli interni (buchi)
RegionOrigin = Points | Objects              // quale "Add" l'ha prodotta

CmdResult::HatchBoundariesPicked {
    regions: Vec<(HatchRegion, RegionOrigin)>,
    objects: Vec<Handle>,                    // oggetti dell'ultimo "Select objects" (vuoto per Points)
}
```

`HatchDialogState` conserva:
- `owner_tab_id: u64` (l'`id` stabile del tab, non l'indice: gli indici slittano alla chiusura di un tab);
- `plane: WorkingPlane`, calcolato all'apertura come fa oggi l'arm `"HATCH"` (UCS corrente, o piano di default fuori dallo
  spazio modello);
- `boundary_sources` e `outlines`, ricalcolati a ogni "Add" (costo uguale a un avvio di HATCH di oggi) per non lavorare
  su uno snapshot vecchio;
- `regions: Vec<(HatchRegion, RegionOrigin)>` accumulate, `taken_objects: Vec<Handle>`;
- `saved_selection: Option<Vec<Handle>>` (vedi sotto);
- le impostazioni correnti (`HatchSettings`) e il testo grezzo di Angle/Scale.

**Deduplica.** Una regione ha una chiave canonica: ogni anello è ruotato a partire dal vertice minimo (ordine
lessicografico dopo quantizzazione a 1e-6), orientato in senso antiorario e privato del vertice di chiusura duplicato;
la chiave della regione è l'anello esterno canonico più l'insieme ordinato dei suoi buchi canonici. Due regioni con la
stessa chiave sono la stessa regione, **anche se una viene da Pick points e l'altra da Select objects**, o hanno
ordine/orientamento/vertice iniziale diversi. Resta la deduplica dei singoli anelli di `combined_rings()` per il
commit non separato. Per "Select objects" le regioni sono quelle di `bounded_faces` come oggi (un anello ciascuna;
le isole le ricava `make_hatch` dall'annidamento): è un comportamento esistente, non cambia.

**Selezione.** All'apertura gli oggetti già selezionati che contribuiscono con segmenti al contorno (anche oggetti aperti
che insieme chiudono un'area: è ciò che fa `boundary_sources.contains_key` + `bounded_faces`) alimentano la prima
raccolta di aree; se non chiudono nulla la finestra si apre vuota e la riga di comando lo dice. Un retino selezionato
non viene ereditato (Inherit Properties è grigio). Da lì in poi:
- ogni "Add: Select objects" **parte con selezione vuota**, indipendentemente da quella globale, quindi un secondo "Add"
  non riacquisisce gli stessi oggetti;
- prima di partire la selezione globale è salvata in `saved_selection` e **ripristinata** con Invio, con Esc e
  quando il flusso viene annullato per cambio/chiusura tab;
- ogni Add elabora **tutti** gli oggetti scelti in quell'Add, anche se già raccolti prima: due aree che condividono un
  oggetto di contorno devono potersi formare entrambe, quindi la geometria non si filtra mai per handle. Il solo
  filtro è la deduplica delle regioni risultanti (chiave canonica sopra). `taken_objects` è solo informativo (messaggi
  alla riga di comando, conteggi) e non entra in `bounded_faces`.

## 4. Anteprima fedele (motore)

L'attuale `HatchCommand::hatch_preview_models` unisce tutte le regioni in **un** modello, ignora `separate_hatches` e
non filtra gli anelli per stile isole; il filtro oggi avviene solo quando il modello viene ricostruito dall'entità DXF
(`scene/entity.rs`, `keep = match style { Normal => true, Outer => depth <= 1, Ignore => depth == 0 }`). Quindi la
Preview non attraversa il percorso del commit. Correzione:

- la regola `keep` diventa una funzione condivisa (`island_ring_kept(style, depth)`) usata sia da `entity.rs` sia dalla
  generazione dei modelli di anteprima, così non può divergere;
- la generazione dei modelli di anteprima per la finestra produce **un modello per regione** quando
  `separate_hatches` è attivo (altrimenti uno solo), e per ognuno filtra gli anelli *renderizzati* con `island_ring_kept`
  sulla profondità di annidamento (`ring_nesting_depths`) **calcolata sugli anelli di quel modello**;
- i buffer renderizzati sono **tre, e il clone di anteprima li filtra e li mantiene sincronizzati anello per anello**:
  `boundary` (usato da `HatchModel::pattern_segments()` e dallo swatch), `fill_plane_boundary` (la GPU lo preferisce a
  `boundary`, `scene/pipeline/wipeout_gpu.rs:206`) e `boundary_exterior` (un flag per anello). Filtrarne uno solo darebbe
  un'anteprima diversa a seconda del consumatore: un anello presente in un buffer e assente nell'altro è un difetto;
- `boundary_wcs`, `boundary_paths` e `boundary_sources` restano **completi** (tutti i percorsi originali): il filtro
  riguarda solo ciò che si disegna, così associazione e persistenza non cambiano;
- Retain boundaries esclude Separate, quindi con Retain l'anteprima è un modello solo;
- `HatchCommand::hatch_preview_models` (anteprima da riga di comando) usa la stessa funzione, così `-HATCH` migliora allo
  stesso modo e non esistono due implementazioni.

Nota di correzione alla spec precedente: "lo stile isole è un campo del modello, quindi cambiarlo dopo è sicuro" è vero
per il **commit** (che ricostruisce dall'entità), non per l'anteprima; ora è coperto qui.

## 5. Finestra

Tre colonne come nello screenshot dell'utente; widget iced, stile di `src/ui/window/drawing_units.rs`
(`group`, `drop_row`, `dialog_button`).

| Gruppo | Controlli attivi | Controlli grigi (visibili) |
|---|---|---|
| Type and pattern | Type = Predefined, Pattern (lista da `hatch_patterns::catalog()`), swatch | "…" palette, Color, Custom pattern |
| Angle and scale | Angle, Scale (testo con valori predefiniti) | Double, Relative to paper space, Spacing, ISO pen width |
| Hatch origin | Use current origin, Specified origin + Click to set new origin | Default to boundary extents, Bottom left, Store as default origin |
| Boundaries | Add: Pick points, Add: Select objects | Remove, Recreate, View selections |
| Options | Associative, Create separate hatches | Annotative, Draw order, Layer, Transparency |
| Islands | Island detection, stile Normal/Outer/Ignore | — |
| Boundary retention | Retain boundaries | Object type (solo Polyline) |
| Boundary set / Gap tolerance | — | tutto grigio |
| Inherit options, Inherit Properties | — | tutto grigio |

- Tab **Gradient** visibile ma disattivata (secondo giro).
- **Island detection** spenta equivale a stile Ignore; accesa, il radio sceglie Normal/Outer/Ignore.
- **Retain boundaries** e **Create separate hatches** si escludono a vicenda, come nel motore (in AutoCAD sono
  indipendenti: differenza dichiarata).
- **Validazione.** Angle: numero finito (virgola o punto). Scale: numero finito e `> 0`. Se un campo non è valido
  riceve il bordo di errore e una riga di messaggio sotto di sé; **OK, Preview, Add: Pick points, Add: Select objects e
  Invio sono disabilitati** finché non lo è (nessun errore a posteriori). Il testo grezzo si conserva mentre si digita.
- **Swatch.** Si riusa il renderer già presente in `src/ui/properties.rs` (`HatchPatternPreview` + `hatch_preview_scale`),
  reso `pub(crate)` **sul posto** (nessuno spostamento di file, meno conflitti) ed esteso con angolo e scala
  dell'utente (`angle_offset`, `scale` del modello). Normalizzazione: la scala di disegno parte da
  `hatch_preview_scale(pattern)` (passo ≈ 8 px) moltiplicata per la scala dell'utente, **limitata** perché il passo sullo
  schermo resti tra 2 e 64 px; sotto 2 px si disegna un riempimento tinto invece di centinaia di segmenti, sopra 64 px
  il passo viene fissato a 64 px, e i segmenti disegnati sono al massimo 400. Mai uno swatch vuoto.

## 6. Impostazioni e origine

- `hatch_last: HatchSettings` (sessione, non nel disegno) contiene **solo preferenze valide**: pattern, angolo, scala,
  associativo, separati, contorni, island detection e stile, modalità origine (`Current | Specified`). Niente aree, handle,
  riferimenti al documento o al tab.
- Si aggiorna **solo con Add (Pick points o Select objects) e con OK**, e solo con valori validi; **Preview non lo
  aggiorna**. Conseguenza dichiarata: un Cancel dopo un Add **conserva** le modifiche fatte prima di quell'Add; un Cancel
  senza Add né OK (anche dopo una o più Preview) non cambia nulla.
- **Origine.** "Use current origin" non conserva coordinate: le rilegge da `document.hatch_origin()` a ogni Add e a OK.
  "Specified origin" conserva un punto **nelle coordinate locali del piano di lavoro** (lo stesso spazio di
  `HatchCommand::default_origin`, che oggi è confrontato con l'ancora del contorno in coordinate locali): il punto
  scelto in WCS è convertito con `plane.to_local` al momento della scelta. Se non ne è stato scelto nessuno, vale
  l'origine corrente. Il punto non entra in `hatch_last`.

## 7. Tab, script e registrazione comandi

**Proprietà del tab.** I gestori di commit usano `active_tab` (`handle_commit_hatch*`); il comando raccoglitore vive nel
tab proprietario; `TabSwitch` cambia `active_tab` senza chiudere i modali nascosti. Decisione: **come per l'editor
attributi (`cancel_attr_editor` in `TabSwitch`), lasciare il tab proprietario annulla il flusso**. Cioè:
- `TabSwitch` verso un altro tab, e `TabClose` del tab proprietario (cercato per `owner_tab_id`), svuotano
  `hatch_dialog`, riportano `active_cmd = None` nel tab proprietario se è un raccoglitore HATCH, ripristinano la
  selezione salvata e chiudono la modale;
- ogni `Dispatch` di ritorno cerca il tab per `owner_tab_id`: se manca, o non è il tab attivo, lo stato è scartato e il
  risultato ignorato (mai applicato a un altro documento);
- OK può partire solo con `active_tab == owner` (garantito dalla regola precedente, ricontrollato con un guard).

**Script e automazione.** Decisione di progetto, non ipotesi: un campo esplicito `scripted_dispatch: bool` su `App`,
distinto da `suppress_plugin_dispatch` (che significa solo "evita ricorsione nel plugin"). Quando è vero, l'arm `"HATCH"`
esegue il corpo di `-HATCH` e non apre mai una finestra. Lo impostano tutti i confini programmatici:
`plugin_host::run_command` (`Run` e `Start`), `plugin_host::execute_command`, i canali control/MCP/REST/automazione
(`src/app/control`, `src/app/automation.rs`) e l'esecuzione di file di script (`.scr`). Ribbon, barre e riga di comando
interattiva non lo impostano e aprono la finestra. Il piano elenca per `grep` ogni chiamante di `run_command_line*` e
`dispatch_command` e assegna a ciascuno la sua origine.

**Registrazione.** `-HATCH` entra in `inventory::submit!(CommandRegistration { names: &["HATCH", "-HATCH"] })` e nell'elenco
dei comandi noti di `src/app/commands/mod.rs` (come `-INSERT`).

## 8. Modifiche al codice

Nuovi (file propri, per ridurre i conflitti col merge da upstream):
- `src/ui/window/hatch_dialog.rs`: `State`, `Field`, `view_window`.
- `src/app/commands/hatch_dialog.rs`: apertura, gestione del risultato tipizzato `CmdResult::HatchBoundariesPicked`
  (Pick/Select riusciti: **nessun `Dispatch`**), e dei `Dispatch` per i soli esiti senza payload di aree
  (`HATCH_PICK_CANCELLED`, `HATCH_ORIGIN_PICKED x y z`, `HATCH_PREVIEW_DONE`), annullamento per tab, deduplica.
- In `src/modules/draw/draw/hatch.rs`: `HatchSettings`, `HatchRegion`, i comandi raccoglitori (aree, punto, anteprima),
  `with_settings`, `with_regions`, la funzione di anteprima di §4. `make_hatch`/`on_enter` non cambiano.

Punti d'aggancio in codice upstream (minimi, ognuno con test):
- `src/ui/window/mod.rs` e `src/app/commands/mod.rs`: `pub mod` dei nuovi file.
- `src/app/mod.rs`: `ModalKind::Hatch`, campi `hatch_dialog`, `hatch_last`, `scripted_dispatch` (inizializzati accanto a
  `drawing_units: None`), varianti `Message::HatchDialog…`.
- `src/app/commands/draw.rs`: l'arm `"HATCH"` sceglie tra finestra e corpo di `-HATCH`.
- `src/app/view/modal.rs`, `src/app/update/mod.rs`: titolo, contenuto, `CloseModal`, `TabSwitch`, `TabClose`, Invio/Esc.
- `src/command.rs`, `src/app/command_driver/mod.rs`: `CmdResult::HatchBoundariesPicked`.
- `src/scene/entity.rs`: la regola `keep` diventa `island_ring_kept` (comportamento invariato).
- `src/ui/properties.rs`: swatch `pub(crate)` + angolo/scala.
- Localizzazione: stringhe nuove con `t!`; nessuna modifica a `locale_catalog.rs` (scelta del progetto); il piano verifica
  il fallback all'inglese per le chiavi mancanti.

## 9. Rischi

1. **Stack di `rustc`** (trappola n. 1): vista e logica in funzioni `#[inline(never)]` in file separati; poche righe
   nei chiamanti enormi.
2. **Nessun `button` in menu contestuali** (trappola n. 2) non applicabile: la finestra non ha menu contestuali.
3. **Verifica visiva**: solo dall'utente su Windows (screenshot o artefatto CI).

## 10. Test

Unitari (`cargo test --locked --lib hatch`):
- `HatchSettings` + regioni → stesso `CmdResult` di `on_enter` di oggi, per tutte le combinazioni associativo/separati/contorni;
- mappatura Island detection → stile; esclusione separati/contorni;
- parsing e validazione di Angle/Scale: virgola, valori non finiti, scala 0 e negativa, OK/Preview/Add/Invio disabilitati;
- deduplica: Pick points + Select objects sullo stesso contorno; stessa regione con vertice iniziale, ordine e
  orientamento diversi; regioni con buchi diversi non confuse;
- **Preview**: con `separate_hatches` un modello per regione; con tre livelli annidati e stili Normal/Outer/Ignore il
  numero di anelli disegnati coincide con quello che dà il percorso entità (`entity.rs`); `boundary_paths`,
  `boundary_sources` e `boundary_wcs` completi; Retain → un modello;
- **Preview, buffer sincronizzati**: per ogni stile, `boundary`, `fill_plane_boundary` e `boundary_exterior` contengono gli
  stessi anelli nello stesso ordine; il test gira anche su **piano di lavoro ruotato** (dove `boundary` e
  `fill_plane_boundary` hanno coordinate diverse) e verifica `pattern_segments()` sul primo e il bounding box GPU sul secondo;
- **aree che condividono un oggetto di contorno**: due Add successivi con un oggetto in comune formano entrambe le aree
  (nessun filtro per handle), e la regione identica ripetuta non si duplica;
- swatch: scala estrema piccola, grande e normale producono segmenti entro il limite e mai vuoto;
- origine specificata con **UCS ruotato**: il punto locale e la fase del pattern coincidono con quelli di `HatchCommand`;
- preselezione composta da **più segmenti aperti** che chiudono un'area.

Di flusso (`app.update(Message::…)`, nello stile dei test di Drawing Units), per **tutti e quattro** i flussi nascosti
(Pick points, Select objects, Preview, origine):
- ritorno con Invio e con Esc, stato intatto;
- cambio tab e chiusura del tab proprietario durante il flusso: stato scartato, `active_cmd` pulito, selezione ripristinata,
  nessun retino creato nel documento sbagliato;
- Esc durante Select objects ripristina la selezione precedente; un secondo Add parte con selezione vuota (non eredita
  quella globale), ma elabora tutti gli oggetti che sceglie, anche se già usati in un Add precedente;
- Invio con zero aree o campi non validi non fa nulla;
- persistenza: Add poi Cancel conserva le impostazioni; nessuna area né handle in `hatch_last`;
- OK crea il retino con undo a un passo, nei casi associativo, separati e contorni.

Regressione e canali:
- `-HATCH` e le opzioni da riga di comando invariati;
- `HATCH` non apre finestra da `plugin_host::run_command` (`Run` e `Start`), `execute_command`, modalità Start e Run, canale
  control/MCP/REST e `.scr`; `HATCH` da ribbon e da riga interattiva la apre.

## 11. Fuori da questo giro

Tab Gradient attiva; Color, Transparency, Layer, Draw order; palette "…" dei pattern; Inherit Properties; Gap tolerance e
Boundary set; Annotative; Double/Spacing/ISO pen width e i tipi User defined/Custom; HATCHEDIT con la stessa finestra (in
AutoCAD la riusa); Remove/Recreate boundaries e View selections.
