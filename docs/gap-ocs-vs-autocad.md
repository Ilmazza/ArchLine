# Analisi di gap: ArchLine (Open CAD Studio) rispetto ad AutoCAD classico

Stato: prima stesura, 2026-10-07, su `lavoro` (efff3cfc). Per Mauro Mazzarelli: serve a decidere **cosa fare dopo**,
non è un elenco di difetti. Le priorità del §7 sono una proposta, la scelta è tua.

## 1. In una pagina

- **Comandi.** ArchLine registra **705 nomi di comando** (alias compresi) in 118 file. Di 97 voci della matrice AutoCAD
  che avevi scritto in Autocad_clone, **66 sono presenti per nome**, 6 sono funzioni e non comandi, 25 non coincidono
  per nome. Esaminate una per una (sono righe della matrice): 7 sono lo strato architettonico (assente di proposito,
  è il plugin), 2 sono scelte tue («non previsto»: AutoLISP, ViewCube), 5 esistono in altra forma, **11 sono lacune
  vere**.
- **Altri comandi d'uso comune** (un elenco mio, non verificato su documentazione Autodesk, §4): lacune vere di
  rilievo per uno studio sono una ventina, soprattutto `PUBLISH`/`PREVIEW`, `LAYWALK`/`LAYCUR`/`LAYERP`, `BCOUNT`,
  `TXTEXP`, `COPYTOLAYER`, `CHSPACE`, `IMAGEADJUST`/`IMAGEQUALITY`.
- **Lo strato architettonico non esiste**: nessuna occorrenza di IFC nel codice, né locali, né abachi con fonte e stato.
  È il plugin, non un gap da «colmare» in ArchLine.
- **Immagini: non c'è modo di ricaricarle** (§6). È un difetto vero e piccolo: la causa è una cache che vive per
  tutta la sessione.
- Il resto della copertura è ampia: il nucleo 2D, quote, testi, blocchi, xref, layout, stampa e 3D ci sono. Il rischio
  non è l'assenza di comandi ma la **fedeltà** (il banco di conformità ha già trovato gli accenti R2000 e le spline).

## 2. Metodo, e cosa vuol dire «presente»

1. **Comandi di ArchLine.** Estratti dal codice: ogni `CommandRegistration { names: &[...] }` (script
   `gap_extract.py`, scratchpad della sessione). Sono i nomi che compaiono nell'autocompletamento.
2. **Riferimento AutoCAD.** Due elenchi: la **matrice di Autocad_clone** (`docs/04-comandi-matrice.md`, tua, 97 righe,
   con priorità M/1/2) e un **elenco aggiuntivo mio** di comandi usati in uno studio (207 nomi, §4).
3. **Controllo.** Un nome è «registrato» se compare tra i `names`. Se non c'è, l'ho cercato come stringa letterale nel
   resto di `src/` (il dispatch può gestire un comando non registrato). Se non c'è nemmeno lì è «assente».

**Cosa non dice.** «Presente» è *registrato nel codice*, non «funziona come in AutoCAD»: non ho eseguito i comandi uno a
uno. Un'assenza *per nome* può nascondere una funzione equivalente con altro nome o nel menu: dove l'ho trovata la
indico. L'elenco AutoCAD aggiuntivo viene dalla mia conoscenza generale e può avere omissioni. L'app non è stata
guidata a schermo (qui niente GPU).

## 3. La matrice di Autocad_clone (97 righe)

Le 66 righe presenti per nome non sono elencate. Le altre:

### 3.1 Lacune vere (11)

| Comando | Pri. matrice | Nota |
|---|---|---|
| `CHANGE` | 1 | c'è `CHPROP`; `CHANGE` (punti, testo, quota) no |
| `STATUS` | M | stato del disegno (entità, limiti, memoria): il letterale compare solo come tag di un attributo |
| `RECOVER` | 1 | c'è `AUDIT`; `RECOVER` compare solo in un elenco del plugin host |
| `TIME` | 2 | |
| `LAYCUR` | 1 | (Express) porta gli oggetti sul layer corrente |
| `LAYWALK` | 1 | |
| `NAMEDVIEW` | 1 | c'è `VIEW`; la finestra delle viste con nome non è registrata |
| `EATTEDIT` | 1 | c'è `ATTEDIT` e `BATTMAN` |
| `PREVIEW` | M | anteprima di stampa |
| `PLOTTERMANAGER` | 1 | |
| `PUBLISH` | 2 | stampa di più fogli in un colpo: per uno studio pesa più di quanto dica la priorità 2 (mio giudizio) |

### 3.2 Esistono in altra forma (5 righe, più 1 voce dentro un'altra riga)

| Voce | Come c'è |
|---|---|
| `XBIND` | `XREF Bind` (op del comando XREF) |
| `LAYOUT` | schede di layout, `LAYOUTMANAGER`, `LAYOUTTAB` |
| `OTRACK` | interruttore F11 (`shortcuts.rs`) |
| `ANNOTATIVE` | famiglia `ANNOSCALE`, `ANNOAUTOSCALE`, `OBJECTSCALE`... |
| `SHEETSET` (stessa riga di `PUBLISH`) | gestito in `app/commands/display.rs`, non registrato |
| `LWEIGHT` | spessore di linea nel menu della barra Layers e nelle proprietà; il comando-finestra no |

### 3.3 Fuori scelta tua, o fuori da ArchLine

- Non previsti nella tua matrice: AutoLISP (sostituito da Python), ViewCube (la stessa riga cita `ORBIT`: c'è `3DORBIT`).
- Strato architettonico (7 voci: `MURO`, `PORTA`/`FINESTRA`, `LOCALE`/`LOCALIAUTO`, `ABACO`, `QUOTEAUTO`, `ESPORTAIFC`,
  `ESPORTAQUANTITA`): vedi §5.

### 3.4 Funzioni, non comandi (6 righe)

Riga di comando/alias/cronologia, Dynamic Input (`DYNMODE` è tra le variabili), grips (variabili `GRIP*` presenti),
alias editor (`ALIASEDIT` registrato), tool palette (`TOOLPALETTES` nel dispatch), blocchi dinamici. Per i blocchi
dinamici: `app/visibility.rs` gestisce il parametro di visibilità con la sua maniglia; **non c'è** la creazione di
parametri e azioni (`BPARAMETER`, `BACTION` assenti).

## 4. Altri comandi d'uso comune (elenco mio)

Per area: registrati / elenco, poi le assenze, già ripulite da ciò che non è un vero gap (nomi inventati da me,
equivalenti, funzioni fuori scopo). **Elenco non verificato su documentazione Autodesk.**

| Area | Registrati | Assenti che contano |
|---|---|---|
| Disegno | 20 / 21 | nessuna (l'unica «assente» era un nome sbagliato mio) |
| Modifica | 29 / 33 | `TXTEXP` (esplodi testo), `COPYTOLAYER`, `CHSPACE` (sposta tra model e paper space), `ARRAYEDIT` |
| Annotazione | 19 / 25 | `DIMREASSOCIATE`, `DIMDISASSOCIATE`, `DIMINSPECT`, `TEXTALIGN`, `TABLEEXPORT`, `SPELL` (controllo ortografico: non verificato a fondo) |
| Blocchi | 9 / 14 | `BCOUNT` (conteggio blocchi per abaco), `ATTIPEDIT`, `BLOCKICON` |
| Riferimenti | 11 / 18 | `IMAGEADJUST`, `IMAGEQUALITY`, `PDFSHXTEXT`, `OLELINKS`, `INSERTOBJ` |
| Layer | 15 / 20 | `LAYERP` e `LAYERPMODE` (layer precedente), `LAYWALK`, `LAYCUR`, `LAYVPI` |
| Vista | 11 / 14 | `NAMEDVIEW`, `UCSMAN`, `DVIEW` (prospettiva interattiva) |
| Layout e stampa | 14 / 21 | `PUBLISH`, `PREVIEW`, `VPMAX`, `VPMIN`, `SHEETSETHIDE` |
| Utilità | 24 / 34 | `STATUS`, `TIME`, `RECOVERALL`, `WORKSPACE` (cambio di workspace da comando: oggi solo con `ARCHLINE_WORKSPACE`) |
| Parametrico | 5 / 7 | `BPARAMETER`, `BACTION` (blocchi dinamici) |

Scartati: `MULTILINE` (il comando è `MLINE`, presente), `DESIGNCENTER` (c'è `ADCENTER`), `BCLOSE`/`-EXPORT`/
`PROPERTIESCLOSE` (equivalenti o banali), `CUIX`, `APPLOAD`, `VLIDE`, `NETLOAD` (LISP/.NET: scelta di sostituire con Python).

## 5. Lo strato architettonico (assente, è il piano a parte)

Verificato: nessuna occorrenza di `IFC` nel codice di `src/`. Mancano locali con area/perimetro da contorno, muri e
aperture come oggetti, abachi con fonte e stato (deterministico/bozza), quantità, esportazione IFC. Due ingredienti
esistono già e il plugin li può usare: l'estrazione dati (`DATAEXTRACTION`/`EATTEXT`, che scrive file esterni e
collegamenti dati in **CSV e testo**; Excel non verificato) e l'API dei plugin Python (documentata in
`docs/plugin-architecture.md`). È un lavoro architetturale: richiede una spec scritta e il tipo di intervento
(nuova costruzione, ristrutturazione, manutenzione, restauro).

## 6. Lacune trovate lavorando

### 6.1 Le immagini non si possono ricaricare (segnalato da te) — RISOLTO il 2026-10-07

**Stato attuale.** `XREF Reload`, il Reload della palette e «Reload All References» ricaricano ora anche le immagini: rileggono il file, applicano un percorso cambiato con `XREF Path` e tolgono lo stato «Unloaded». Implementato con `invalidate_image` (`image_model.rs`) e `Scene::reload_image_definitions` (`scene/entity.rs`); verificato da test, **non ancora visto a schermo**. Restano aperti il Reload dei PDF/DWF da riga di comando (rifiutato, mentre la palette li ricarica) e `IMAGEADJUST`/`IMAGEQUALITY`. Il testo sotto descrive il difetto com'era prima della correzione.

**Sintomo.** Se il file di un'immagine collegata cambia, o compare dopo essere mancato, il disegno non la aggiorna.

**Causa, verificata nel codice** (non l'ho provata a schermo):
- `resolve_image` (`src/scene/model/image_model.rs:468`) tiene i pixel in una cache **globale per percorso**,
  anche l'esito «file mancante» (un `None`). Si svuota solo in `clear_image_cache` (`:456`), chiamata una volta, quando si
  **apre** un disegno (`src/scene/mod.rs:1174`). Nessun altro punto la svuota.
- `XREF Reload`, sia da riga di comando sia dalla palette, **rifiuta** le immagini: «reload applies to drawing
  references only» (`src/app/commands/blocks.rs:1058`, `src/app/update/dialog.rs:987`). Lo fissa anche un test
  (`xref_reload_image_keeps_flag_no_spurious_match`, `blocks.rs:1951`), quindi è una scelta esplicita, non un caso.
- «Reload All References» considera solo gli xref di disegno (`dialog.rs:1200`).
- Per i PDF/DWF/DGN e le nuvole di punti il Reload nella palette funziona; **per le immagini no**. Inoltre da riga di
  comando `XREF Reload` rifiuta anche i PDF che dalla palette si ricaricano: incoerenza tra i due percorsi.

**Effetto collaterale.** `XREF Path` su una riga immagine dice «Path set … — Reload to apply» (`blocks.rs:1352`), ma per
le immagini il Reload non esiste: l'istruzione non si può eseguire. (Non ho verificato se la modifica del percorso, da
sola, rifà l'immagine: `set_ref_path`, `io/xref.rs:2254`, aggiorna la definizione ma non chiama le funzioni che
ripopolano le immagini.)

**Come si aggira oggi:** chiudere e riaprire il disegno.

**Correzione possibile, piccola:** far valere `Reload` per le righe `RefKind::Image`: togliere dalla cache il percorso
(servirebbe `invalidate_image(path)`; oggi c'è solo lo svuotamento totale), richiamare
`populate_images_from_document_unbumped` e incrementare l'epoca geometrica; stessa cosa per «Reload All». Con test sul
comportamento (immagine che cambia, immagine che compare dopo essere mancata). **Da decidere con te** prima di toccare.

### 6.2 Dal banco di conformità

Accenti corrotti nei file R2000 (TEXT, MTEXT, ATTRIB), spline di contorno degli hatch R2000 senza punti di fit,
`POLYLINE` riscritta come `LWPOLYLINE` (benigno). Il caso `colors` del banco non misura nulla (vedi
`tests/conformance/README.md`).

## 7. Priorità proposta

Criterio dichiarato: *impatto per uno studio di architettura* (mio giudizio) contro *costo* (qualitativo, non ho
stimato ore). Niente numeri inventati.

| # | Cosa | Impatto | Costo | Perché |
|---|---|---|---|---|
| 1 | ~~Ricarica delle immagini (§6.1)~~ fatto | alto: piante scansionate e catastali cambiano spesso | basso | difetto già segnalato, causa chiara |
| 2 | `LAYERP`, `LAYCUR`, `LAYWALK` | medio: lavoro quotidiano sui layer | basso | la barra Layers c'è, mancano i comandi |
| 3 | `PUBLISH` e `PREVIEW` | alto: consegna di più tavole | medio | la stampa singola c'è |
| 4 | `TXTEXP`, `COPYTOLAYER`, `CHSPACE`, `BCOUNT` | medio | basso | comandi di editing isolati |
| 5 | `IMAGEADJUST`, `IMAGEQUALITY` | medio per i raster | basso | si collega al punto 1 |
| 6 | `NAMEDVIEW`, `STATUS`, `TIME`, `RECOVER`, `EATTEDIT` | basso-medio | basso | completano la matrice |
| 7 | Creazione di blocchi dinamici (`BPARAMETER`/`BACTION`) | medio | alto | oggi si leggono (visibilità) ma non si creano |
| 8 | Strato architettonico (plugin) | il motivo del progetto | alto | serve spec scritta |

## 8. Cosa non ho verificato

- Nessun comando è stato eseguito: «registrato» non è «completo» (esiste un registro di copertura per le entità in
  `docs/plugin-host-model-coverage-ledger.md`, ma non per i comandi).
- Menu e barre possono esporre funzioni senza comando registrato: ho cercato i letterali nel sorgente, non ho
  percorso i menu.
- L'elenco AutoCAD aggiuntivo (§4) è mio e senza fonte esterna; la matrice di Autocad_clone è tua e anche lei una scelta
  di cosa serve, non il catalogo completo di AutoCAD.
- Prestazioni, stabilità su file grandi, rendering e stampa: fuori giro (e senza GPU qui).
