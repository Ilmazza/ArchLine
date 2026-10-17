# Parità con AutoCAD: cosa manca ad ArchLine

Stato: 2026-10-17, su `lavoro` (cea29392), codec `opencadcodec` fe69506, kernel `opencadkernel` ae28f66. Riferimento:
AutoCAD 2026. Per Mauro Mazzarelli. Approfondisce [gap-ocs-vs-autocad.md](gap-ocs-vs-autocad.md), che confrontava i
**nomi** dei comandi con un elenco scritto a memoria: qui il confronto per nome è fatto sull'indice ufficiale Autodesk e,
soprattutto, si guarda la **profondità** (cosa fa davvero il codice). Le priorità del §6 sono una proposta: la scelta è tua.

## 1. In una pagina

**Verdetto.** ArchLine non è alla pari con AutoCAD, ma il distacco non sta dove lo fa pensare il conteggio dei comandi.
Il nucleo 2D (disegno, quote, MTEXT, scala annotativa, vincoli parametrici, PDF vettoriale) è profondo e in molti punti al
livello di AutoCAD. La parità si rompe su cinque fronti:
- **gesti quotidiani** che mancano o si comportano diversamente;
- **consegna delle tavole**: plotter in scala, pubblicazione, penne CTB;
- **dati per abachi e cartigli**: campi area, conteggi, estrazione degli attributi;
- **blocchi dinamici** di terzi;
- **risorse dello studio**: retini, tipi di linea, font, template, script.

**Per nome.** L'indice "Commands" della guida AutoCAD 2026 conta 988 voci, comprese 84 Express Tools e 87 forme "-":

| Comandi AutoCAD 2026 | Voci |
|---|---|
| registrati in ArchLine con lo stesso nome | 407 |
| presenti in ArchLine con un altro nome (§3.3) | 14 |
| assenti | 567 |

Come si dividono le 567 assenze:
- **58 fuori scopo per scelta**: servizi cloud e IA di Autodesk (41) e LISP/VBA/.NET/ARX (17).
- **156 tra 3D, rendering e geolocalizzazione.**
- **353 nelle altre aree**: interfaccia 77, riferimenti e import 52, annotazione 41, file 37, disegno 2D 33, stampa 29,
  blocchi 26, blocchi dinamici 24, viste e layer 21, utilità 13.

Registrato non vuol dire funzionante: dei 407, almeno una dozzina sono stub o rimandi (§3.2).

**Le dieci lacune che pesano di più su uno studio** (dettaglio al §4, ordine proposto al §6). «Ricontrollato» = verificato
da me nel codice, oltre che dall'analisi.
1. **Gli snap digitati al prompt non sono riconosciuti** (`end`, `mid`, `int`, `per`…). Funzionano solo da menu e barra.
   (ricontrollato)
2. **I campi Area e Lunghezza si calcolano solo su cerchio** (area) **e su linea, cerchio, arco** (lunghezza). Su polilinee
   e tratteggi il campo mostra il valore memorizzato nel file, senza avvisare. Nel dialogo non c'è il fattore di conversione
   cm²→m². (ricontrollato)
3. **HATCH ha la tolleranza di chiusura fissa a 1e-6 e non carica `.pat` dello studio.** (ricontrollato)
4. **La stampa su plotter passa sempre da un PDF temporaneo.** Se è installato un lettore PDF che gestisce la stampa diretta
   (`printto`), stampa lui e la scala la decide il lettore. Altrimenti ArchLine stampa un raster a 300 DPI, adattato all'area
   stampabile e solo della prima pagina. Niente PC3. (ricontrollato)
5. **Non c'è una pubblicazione vera**: PRINTALL fa un solo PDF del disegno attivo. Mancano un PDF per tavola, più disegni,
   un elenco salvabile.
6. **Estrazione dati e conteggi non servono per gli abachi.** DATAEXTRACTION non dà né il nome del blocco né gli attributi;
   COUNT conta per tipo o layer, non per blocco; SELECTSIMILAR non distingue i blocchi.
7. **Dei blocchi dinamici si cambia solo la visibilità**, e con un difetto (§5). Niente maniglie stretch, flip, rotate,
   lookup; le proprietà dinamiche si leggono ma non si modificano.
8. **Non ci sono percorsi di supporto.** `.pat` e `.lin` esterni non si caricano, gli SHX si cercano solo vicino al disegno,
   la GUI non offre template (NEW crea un disegno vuoto in metri).
9. **Gli xref annidati non si vedono nel disegno** (ricontrollato) e lo stato Unload non si salva.
10. **STRETCH lascia fermi tratteggi, spline, punti, MLine e wipeout** (ricontrollato). FILLET e CHAMFER non hanno
    Trim/No trim, e CHAMFER lavora solo su linee.

**Difetti** (funzioni presenti ma sbagliate), elenco completo al §5:
- LAYUNISO accende tutti i layer;
- le viste con nome si salvano quadrate;
- cambiare la visibilità di un blocco dinamico tocca tutte le copie, o la definizione;
- accenti corrotti nei DXF R2000;
- le polilinee con 100.000 vertici o più vengono scartate all'apertura e perse al salvataggio.

**Vantaggi verificati** (§7): automazione esterna (MCP, REST, uso senza interfaccia), blocco dei file interoperabile,
nessun traffico di rete all'avvio, vincoli parametrici completi, node graph.

## 2. Metodo e affidabilità

1. **Indice ufficiale dei comandi.** È l'indice "Commands" A–Z della guida AutoCAD 2026 (help.autodesk.com), raccolto il
   2026-10-17. Contiene 898 voci, di cui 815 comandi e 83 Express Tools. A queste si aggiungono 87 forme "-" con pagina
   propria e 3 voci con una pagina "(Command)" ma fuori indice, trovate controllando tutta la guida.
   Per confronto, l'indice delle variabili di sistema 2026 ne conta 1027.
2. **Comandi di ArchLine.** Sono tutti i `CommandRegistration { names }` del codice: 707 nomi. Ogni nome AutoCAD che non
   compare nel registro l'ho cercato come stringa letterale in tutto `src/`. I 19 trovati li ho controllati a mano: 5 sono
   funzioni presenti con altro nome o gestite nel dispatch, 14 sono stringhe che compaiono per altri motivi.
3. **Area di ogni comando assente.** La classificazione è mia, fatta a mano.
4. **Profondità.** Sette analisi in sola lettura del codice, una per area, su app, codec e kernel, con evidenze
   `file:riga`. Stati: Completo, Parziale, Assente. 12 affermazioni chiave le ho ricontrollate io sul codice: sono segnate
   «ricontrollato».
5. **Cosa questo documento non è.**
   - Non ho eseguito nessun comando né aperto DWG reali, e non avevo una GPU.
   - «Dedotto» = comportamento letto nel codice, non osservato.
   - Il riferimento AutoCAD per opzioni e comportamenti è conoscenza generale, non verificata pagina per pagina. L'elenco
     dei comandi, invece, è ufficiale.

I percorsi `src/…` sono relativi al repo. `codec:` sta per il sorgente di `opencadcodec` nella cache di Cargo
(`~/.cargo/git/checkouts/opencadcodec-*/fe69506/src/`).

## 3. I comandi per nome

### 3.1 Assenti, per area

La colonna di destra elenca i comandi che contano per uno studio di architettura (scelta mia). L'elenco completo è in
[parita-autocad-comandi.tsv](parita-autocad-comandi.tsv).

| Area | Assenti | Quelli che contano |
|---|---|---|
| Interfaccia e personalizzazione | 77 | `WORKSPACE`, `WSSAVE`, `CUSTOMIZE`, `ACTRECORD` (Action Recorder), `TOOLBAR`. Molti degli altri sono comandi di chiusura palette (`…CLOSE`): impatto basso |
| Riferimenti, immagini, import | 52 | `IMAGEADJUST`, `IMAGEQUALITY`, `PDFSHXTEXT`, `-XBIND`, `DGNIMPORT`/`DGNEXPORT`, `INSERTOBJ` e `OLE*`, hyperlink (`ATTACHURL`…), Express `XLIST` |
| Annotazione | 41 | `DIM` (quota unica), `DIMREASSOCIATE`, `DIMDISASSOCIATE`, `DIMINSPECT`, `DIMOVERRIDE`, `TEXTALIGN`, `SPELL`, `TABLEEXPORT`, `TXT2MTXT`, `TXTEXP`, `UPDATEFIELD`, la famiglia `COUNT*` (palette, tabella, campo), `HATCHSETORIGIN`, `HATCHGENERATEBOUNDARY` |
| File e qualità | 37 | `RECOVER`, `RECOVERALL`, `DRAWINGRECOVERY`, `COMPARE`, `XCOMPARE`, `QNEW`, `CLOSEALL`, `PARTIALOPEN`, `STANDARDS`/`CHECKSTANDARDS`, `EXPORTTOAUTOCAD`/`PURGEAECDATA` (file di AutoCAD Architecture), `DWGCONVERT` |
| Disegno e modifica 2D | 33 | `CHANGE`, `COPYTOLAYER`, `ARRAYEDIT`, `CUTBASE`, `PASTESPEC`, `PTYPE`; Express `BREAKLINE`, `COPYM`, `MPEDIT`, `SUPERHATCH`, `EXOFFSET` |
| Stampa e pubblicazione | 29 | `PUBLISH`, `PREVIEW`, `PLOTTERMANAGER`, `PLOTSTAMP`, `CONVERTCTB`/`CONVERTPSTYLES`, `EXPORTLAYOUT`, `PNGOUT`/`JPGOUT`, `NEWSHEETSET`/`OPENSHEETSET`, `MVSETUP`, `LAYOUTWIZARD` |
| Blocchi e attributi | 26 | `BCOUNT`, `BURST`, `BLOCKREPLACE`/`BREPLACE`, `GATTE`, `ATTIPEDIT`, `ATTOUT`/`ATTIN`, `TINSERT`, `GROUPEDIT`, `BLOCKICON` |
| Blocchi dinamici (authoring) | 24 | `BPARAMETER`, `BACTION`, `BVSTATE`, `BTABLE`, `BTESTBLOCK` |
| Viste, layer, viewport | 21 | `LAYWALK`, `LAYVPI`, `CHSPACE`, `VPMAX`/`VPMIN`, `UCSMAN`, `VIEWBACK`/`VIEWFORWARD` |
| Utilità | 13 | `STATUS`, `TIME`; per gli script `DELAY`, `RESUME`, `RSCRIPT` |
| 3D, rendering, geolocalizzazione | 156 | §4.10 |
| Cloud e IA di Autodesk | 41 | fuori scopo per scelta (§4.11) |
| LISP/VBA/.NET/ARX | 17 | fuori scopo per scelta |

**Express Tools.** L'indice ne elenca 84: 12 sono registrati in ArchLine (`ALIASEDIT`, `ARCTEXT`, `EXTRIM`, `FLATTEN`,
`SAVEALL`, `TCASE`, `TCOUNT`, `TEXTFIT`, `TEXTMASK`, `TORIENT`, `VPSYNC`, `XDATA`), 72 sono assenti.

### 3.2 Registrati ma non funzionanti

| Nome | Cosa fa davvero | Evidenza |
|---|---|---|
| `TOOLPALETTES` | dice «not yet implemented» | `src/app/commands/display.rs:591` |
| `SHEETSET`, `SSM` | dice «not yet implemented» | `src/app/commands/display.rs:596` |
| `ADCENTER`, `ADC`, `CONTENTBROWSER` | stampano in riga di comando i nomi di blocchi e layer del disegno corrente | `src/app/commands/blocks.rs:666-692` |
| `BATTMAN`, `ATTMAN` | aprono l'editor dell'istanza, non il gestore delle definizioni | `src/app/commands/blocks.rs:716-725` |
| `CUIIMPORT`, `CUILOAD` | leggono un file di testo «TASTO COMANDO», non un `.cuix` | `src/app/commands/view.rs:352-396` |
| `FILTER` | è un alias di QSELECT: niente AND/OR/NOT né filtri salvati | `src/app/commands/inquiry.rs:116` |
| `QLEADER` | è un alias di LEADER, senza finestra delle impostazioni | `src/app/commands/dim.rs:115` |
| `QTEXTMODE`, `PICKSTYLE` | il valore si salva ma non ha effetto | `src/app/commands/styleprops.rs:2375`, `:2451` |
| `ARCTEXT` | crea un TEXT per carattere: niente oggetto modificabile | `src/app/commands/dim.rs:1359-1407` |
| `DWGPROPS` | nessuna finestra | `src/app/commands/view.rs:84-92` |

### 3.3 Presenti con un altro nome: alias a costo quasi nullo

| AutoCAD | ArchLine |
|---|---|
| `GCPARALLEL` / `GCTANGENT` / `GCSYMMETRIC` / `GCCOLLINEAR` | `PCONSTRAINT` / `TCONSTRAINT` / `SYCONSTRAINT` / `LCONSTRAINT` |
| `BSAVE` / `BCLOSE` | `BEDIT_SAVE` / `BEDIT_DISCARD` |
| `MTEDIT` | `TEXTEDIT` |
| `LAYERSTATESAVE` | `LAYERSTATE` |
| `NEWVIEW` | `VIEW` |
| `XBIND` | `XREF` → Bind |
| `LAYOUT`, `MODEL` | schede e `LAYOUTMANAGER` (manca il comando `LAYOUT` con le sue opzioni) |
| `ULAYERS`, `PDFADJUST` | gestiti nel dispatch ma non registrati: non compaiono nell'autocompletamento |

Ci sono anche differenze negli alias di `assets/ocad.pgp`, da controllare una per una prima di cambiarle:
- `T` apre TEXT, mentre in AutoCAD apre MTEXT;
- mancano `LW` e `QSE`;
- Ctrl+F e Ctrl+H aprono FIND, mentre in AutoCAD commutano OSNAP e PICKSTYLE.

Un `acad.pgp` si può incollare a mano in `%APPDATA%\OpenCADStudio\ocad.pgp`: non c'è un import.

## 4. Le funzioni, area per area

Per ogni area: una riga con ciò che è al livello di AutoCAD, poi una tabella **solo con ciò che manca o è parziale**.

### 4.1 Disegno e modifica 2D

**Al livello di AutoCAD:**
- disegno: PLINE (anche in modo arco), CIRCLE (tutte le varianti), ARC (11 varianti più Continua), RECTANG, POLYGON, DONUT,
  XLINE, RAY, MLINE con MLEDIT, SKETCH, BOUNDARY, REGION, DIVIDE/MEASURE con blocchi, CENTERLINE;
- modifica: OFFSET, PEDIT, BREAK/BREAKATPOINT, LENGTHEN, MIRROR, ALIGN con scala, MOVE/COPY (con Array), JOIN, OVERKILL,
  BLEND, REVERSE, DRAWORDER.

| Funzione | Stato | Evidenza | Cosa manca |
|---|---|---|---|
| STRETCH | Parziale, difetto (ricontrollato) | `src/app/command_driver/transform.rs:527-530` | Restano fermi, anche se interamente dentro la finestra: tratteggi non associativi, spline, punti, wipeout, MLine, leader, tabelle. Archi e cerchi si spostano interi solo se il centro è dentro. Il commento in `src/modules/draw/modify/stretch.rs:16` dice il contrario |
| FILLET | Parziale | `src/modules/draw/modify/fillet.rs:1500-1516` | Trim/No trim, Shift per raggio 0, raccordo su cerchi, ellissi, spline |
| CHAMFER | Parziale | `fillet.rs:2075-2088` | Angle, Method, Trim, Polyline; oggi solo linee |
| TRIM/EXTEND | Parziale | `src/modules/draw/modify/trim.rs:2771-2815` | Project, Undo interno, modo Standard (c'è solo Quick). Non taglia i tratteggi; blocchi, testi e tratteggi non fanno da spigolo di taglio |
| ROTATE, SCALE | Parziale | `rotate.rs:172-177`, `scale.rs:90-114` | opzione Copy; Points nell'opzione Reference |
| ARRAY | Parziale | `array.rs:226`, `:396`, `:736` | non associativo; niente ARRAYEDIT |
| EXPLODE | Parziale | `explode.rs:87-118` | Tratteggi, regioni, MLeader e tabelle ignorati senza avviso. Niente XPLODE né BURST |
| MATCHPROP | Parziale | `src/entities/match_props.rs:484-494` | Settings: oggi copia sempre tutto |
| ELLIPSE | Parziale | `src/modules/draw/draw/ellipse.rs:44-46` | Parte dal centro (AutoCAD parte dall'estremo d'asse; «C» non è riconosciuto). Mancano Rotation e Isocircle |
| Grip | Parziale | `src/app/update/context_menu.rs:315-397` | ciclo dei modi con Spazio (oggi Spazio = Invio); copia multipla nei modi grip |
| FILLETRAD, CHAMFERA/B, OFFSETDIST | Parziale | `src/modules/draw/defaults.rs:9-12` | valgono solo per la sessione, non si salvano nel disegno |
| CHANGE, COPYTOLAYER, PTYPE | Assenti | — | — |

### 4.2 Aiuti al disegno, selezione, interfaccia

**Al livello di AutoCAD:**
- snap e puntamento: tutti gli snap principali da menu e barra (END, MID, CEN, NOD, QUA, INT, APPINT, EXT, INS, PER, TAN,
  NEA, PAR), FROM, M2P, TT, filtri `.X/.Y/.XY`, menu Shift+destro, ORTHO, OTRACK;
- input: coordinate `@ # <`, distanza diretta, unità architettoniche e angoli topografici, espressioni nei prompt;
- selezione: W, C, F, WP, CP, All, Last, Previous, QSELECT;
- interfaccia: Proprietà con *VARIES*, Quick Properties, cronologia, Spazio/Invio per ripetere il comando.

| Funzione | Stato | Evidenza | Cosa manca |
|---|---|---|---|
| Snap digitato (`end`, `mid`, `int`…) | **Assente** (ricontrollato) | `src/command.rs:982-995` | `PointModifier::from_token` riconosce solo MTP, FROM, TT e i filtri; il resto dà «Expected Cartesian…» (`src/app/update/command.rs:915`) |
| GCEN; TAN e PER differiti | Assente | `src/snap.rs` | — |
| AutoSnap | Parziale | `src/snap.rs:17` | apertura fissa a 15 px; nessun suggerimento col nome dello snap |
| POLAR | Parziale | `src/ui/popup/polar_popup.rs:12` | angoli aggiuntivi; misura relativa all'ultimo segmento |
| Input dinamico | Parziale | `src/app/drafting_settings.rs:51` | DYNMODE 1 e 2 non distinti; prefisso `*` |
| Comandi trasparenti | Parziale | `src/app/commands/mod.rs:382-384` | solo `'PAN`, `'ZOOM` e gli interruttori |
| Prompt «Select objects» | Parziale | `src/modules/draw/select.rs:226-237` | Undo, Group, Box, Auto, SIngle |
| Preselezione | Parziale | `src/app/commands/draw.rs:613-623` | LIST, CHPROP, DRAWORDER, ISOLATE, LAYISO e TCASE danno errore senza preselezione; PICKFIRST non c'è |
| SELECTSIMILAR | Parziale | `src/scene/selection.rs:352-382` | confronta solo tipo e layer: porte e finestre sullo stesso layer vengono prese insieme |
| Selezione di entità dentro i blocchi | Assente | — | — |
| UNDO | Parziale | `src/app/commands/fileops.rs:72-85` | Mark/Back/BEgin/End, MREDO |
| UCS | Parziale | `src/app/commands/layerprops.rs:179-560` | Previous, ZAxis, UCSMAN, UCS dinamico |
| CAL / QUICKCALC | Parziale | `src/app/commands/inquiry.rs:120-141` | solo numeri, niente punti |
| Palette e finestre di disegno flottanti | Assente | `src/app/config.rs:82` | palette solo ancorate a sinistra o a destra |
| Variabili di sistema | Parziale | `src/app/commands/styleprops.rs:1095-2725` | Circa 155 gestite, contro 1027 documentate nella guida 2026. Sono una catena di `match`, senza registro. Mancano FILEDIA, CMDDIA, EXPERT, PICKFIRST, PEDITACCEPT, TRIMEXTENDMODE, HPGAPTOL, MSLTSCALE. Otto si salvano ma non hanno effetto: PICKSTYLE, PLINEGEN, DRAGMODE, QTEXTMODE, SORTENTS, BLIPMODE, REGENMODE, TEXTQLTY |

### 4.3 Annotazione

**Al livello di AutoCAD:**
- quote: tutti i tipi (lineare, allineata, angolare, raggio, diametro, con jog, lunghezza d'arco, coordinata, linea di base,
  continua), QDIM completo, DIMASSOC 0/1/2 con aggiornamento quando la geometria cambia, circa 80 DIMVAR nella resa,
  dialogo DIMSTYLE a 7 schede, DIMBREAK, DIMSPACE, DIMJOGLINE;
- testi: TEXT con 15 giustificazioni, editor MTEXT con formattazione per carattere, colonne e frazioni sovrapposte;
- scala annotativa completa;
- MLEADER e stili;
- tratteggi associativi con isole, gradienti e trasparenza;
- REVCLOUD, CENTERMARK/CENTERLINE, WIPEOUT, TOLERANCE.

| Funzione | Stato | Evidenza | Cosa manca |
|---|---|---|---|
| Campi Area/Lunghezza | **Parziale, difetto** (ricontrollato) | `codec:fields.rs:1053-1082`; anteprima «----» in `src/app/commands/field.rs:112-115` | L'area si calcola solo sul cerchio, la lunghezza su linea, cerchio e arco. Polilinee e tratteggi mostrano il valore memorizzato. Il dialogo non offre formato, precisione né fattore di conversione (`src/ui/window/field_dialog.rs:245-249`), anche se il motore gestisce `%ct` |
| HATCH, tolleranza di chiusura | Assente (ricontrollato) | `src/app/commands/draw.rs:755` | fissa a 1e-6; esiste solo in BOUNDARY |
| HATCH, pattern | Parziale | `src/io/patterns.rs:11` | Un solo `.pat` incorporato: 106 pattern di derivazione QCAD. Non si caricano `.pat` dell'utente; `acad.pat` e `acadiso.pat` non sono completi |
| HATCH da riga di comando | Parziale | `src/modules/draw/draw/hatch.rs:538` | solo P/A/L |
| Stili di quota figli | Parziale | `src/entities/dimension.rs:3674-3678` | Non si applicano in creazione né in modifica; il dialogo non ha «Stili per». Esempio: un raggio con freccia sotto uno stile lineare con tick |
| ARCHTICK | Parziale | `src/scene/convert/tessellate.rs:2696` | disegnato come OBLIQUE, senza spessore |
| `DIM`, `DIMREASSOCIATE`, `DIMDISASSOCIATE`, `DIMINSPECT` | Assenti | — | — |
| Editor MTEXT | Parziale | `src/app/mtext_editor.rs` | Mancano elenchi puntati e numerati, tabulazioni, inserimento di campi e controllo ortografico. Il giustificato è reso allineato a sinistra; le colonne dinamiche non rifluiscono |
| Font SHX | Parziale | `src/scene/text/font_face.rs:80-97` | Nessun percorso di ricerca né FONTALT. Un font mancante viene sostituito in silenzio con LFF. Su richiesta si scarica da GitHub (`src/io/font_repo.rs`) |
| TrueType grassetto/corsivo | Parziale | `src/entities/text_support.rs:643-660` | simulati (obliquo, tratto più largo) |
| FIND/REPLACE | Parziale | `src/app/find_replace.rs:205-245` | Non cerca in quote, MLEADER e tabelle. Mancano maiuscole/minuscole, parola intera e caratteri jolly |
| SPELL | Assente nella GUI | `src/app/control/text_ops.rs:967` | solo dall'API (`text_audit`) |
| TXTEXP, TEXTALIGN; QTEXT | Assenti; QTEXT senza effetto | — | — |
| Tabelle | Parziale | `src/modules/annotate/table_cmd.rs:764`; `src/app/commands/display.rs:14-133` | Si modificano solo da riga di comando o da Proprietà: non si selezionano celle col mouse. L'interruzione su più parti non si gestisce. TABLEEXPORT assente; Excel solo in lettura |
| LEADER / QLEADER | Parziale | `src/modules/annotate/leader_cmd.rs:202-218` | annotazione solo MText o nessuna; QLEADER senza impostazioni |
| Bolle MLEADER con blocco | Parziale (dedotto) | `src/entities/multileader.rs:1703-1760` | Gli attributi del blocco vengono ignorati: probabilmente il numero nella bolla non si vede. Non ci sono blocchi a bolla predefiniti |
| MLEADERALIGN | Parziale | `src/modules/annotate/mleader_edit.rs:320-366` | Distribute, Parallel, Spacing |

### 4.4 Blocchi, attributi, blocchi dinamici, vincoli

**Al livello di AutoCAD:**
- blocchi: BLOCK, WBLOCK, palette Blocks (Current, Recent, Favorites, Libraries), ridefinizione, BASE, NCOPY, XCLIP,
  EXPLODE;
- attributi: ATTDEF completo, editor avanzato degli attributi (quello che AutoCAD chiama EATTEDIT), ATTDISP, ATTREQ;
- vincoli: geometrici e dimensionali completi, AUTOCONSTRAIN, inferenza, solutore che rileva vincoli ridondanti e in
  conflitto, vincoli nativi DWG (AcDbAssoc*) conservati.

| Funzione | Stato | Evidenza | Cosa manca |
|---|---|---|---|
| DATAEXTRACTION / EATTEXT | **Parziale, inutile per gli abachi** | `src/app/annotation_data.rs:391-436`, `:1371-1405` | Le colonne sono fisse. Il nome del blocco sta nel campo «dettagli» insieme alle coordinate, quindi «combina identici» non raggruppa. Mancano attributi e proprietà dinamiche. Esce solo CSV/TXT, e la tabella non è collegata |
| COUNT / BCOUNT | Assente per i blocchi | `src/app/commands/inquiry.rs:1220-1255` | conta per tipo o per layer |
| Blocchi dinamici, uso | Parziale | `src/app/visibility.rs:179-219`; `src/entities/object_data.rs:719-790` | Funziona solo la visibilità; le altre proprietà si vedono in sola lettura. Mancano le maniglie lineare, polare, XY, rotazione, flip, lookup e le loro azioni |
| Blocchi dinamici, authoring | Assente | — | BPARAMETER, BACTION, BVSTATE, BTABLE, BTESTBLOCK |
| BEDIT | Parziale | `src/app/commands/inquiry.rs:463-660` | Niente parametri. Si entra solo scegliendo un riferimento: un blocco mai inserito non si modifica. Su un blocco dinamico apre il blocco anonimo `*U` (`:498`) |
| REFEDIT | Parziale | `inquiry.rs:764-880` | ATTDEF esclusi; niente REFSET |
| ATTSYNC | Parziale, difetto | `src/app/commands/blocks.rs:565-660` | §5 |
| BATTMAN | Assente (rimanda all'editor) | `blocks.rs:716-725` | gestione delle definizioni |
| ATTDIA | Assente | `styleprops.rs:2091` | salvato, mai letto: niente finestra dei valori all'inserimento |
| -ATTEDIT globale, ATTIPEDIT, GATTE | Parziale / Assenti | `src/app/commands/draw.rs:82-180` | -ATTEDIT lavora solo su inserti già selezionati |
| Palette Blocks, librerie | Parziale | `src/app/update/blocks_palette.rs:192`, `:537` | I blocchi dentro un DWG-libreria non si sfogliano. La casella «Auto-Placement» non ha effetto |
| ADCENTER, TOOLPALETTES | Stub | §3.2 | — |
| MINSERT | Parziale | `src/modules/insert/minsert.rs` | non chiede gli attributi |
| BLOCK «scala uniforme» | Parziale | `src/scene/entity.rs:975` | salvata, non imposta all'inserimento |
| BURST, BLOCKREPLACE | Assente / surrogato | `src/app/update/command.rs:4028-4077` | Si può cambiare il nome nelle Proprietà, ma gli attributi non vengono riallineati |
| GROUP | Parziale | `src/app/commands/layers.rs:733` | descrizione, opzione «selezionabile», GROUPEDIT |
| Parameters Manager | Parziale | `src/scene/named_parameters.rs:115` | trigonometria, PI, arrotondamenti |

### 4.5 Layer e viste

**Al livello di AutoCAD:**
- gestore layer: le colonne principali;
- layer states nativi (ACAD_LAYERSTATES) con maschera delle proprietà;
- LAYTRANS e i comandi LAYOFF, LAYFRZ, LAYLCK, LAYON, LAYTHW, LAYMCUR, LAYMCH, LAYCUR, LAYERP, LAYMRG, LAYDEL;
- PAN.

| Funzione | Stato | Evidenza | Cosa manca |
|---|---|---|---|
| LAYUNISO | **Difetto** (ricontrollato) | `src/app/commands/layers.rs:690-708` | accende tutti i layer invece di ripristinare lo stato di prima |
| LAYISO | Parziale | `layers.rs:588-622` | solo Off; vuole una preselezione |
| Filtri layer | Assente | `src/ui/window/layers.rs:451` | per proprietà, di gruppo, invertiti (c'è solo la ricerca per nome) |
| Gestore layer | Parziale | `src/ui/window/layers.rs:386-420` | Mancano le colonne Descrizione, stile di stampa, New VP Freeze e override per viewport. Elimina un layer in uso con i suoi oggetti, previo avviso (`src/app/update/command.rs:1361`); AutoCAD invece rifiuta |
| Override per viewport | Parziale | `src/scene/view/render.rs:3455-3680` | Si leggono e si vedono, ma non si creano né si modificano, anche se il codec ha `set_layer_viewport_override` |
| LAYFRZ dentro una viewport | Parziale | `layers.rs` | congela ovunque |
| Layer states `.las` | Assente | — | import/export |
| LAYWALK, COPYTOLAYER, LAYVPI, LAYERNOTIFY, LAYLOCKFADECTL | Assenti | — | — |
| Viste con nome | **Parziale, difetto** (ricontrollato) | `src/scene/camera_ops.rs:222-251`; `src/app/commands/layerprops.rs:766-875` | Si salvano quadrate (larghezza = altezza), quindi l'area «View» in PLOT è sbagliata. Solo model space e solo da riga di comando; non conservano prospettiva, stile, layer |
| ZOOM | Parziale | `src/app/commands/dim.rs:829-938` | Center, nX/nXP: `ZOOM S 1/100xp` non fa nulla e non dà errore (`:914`) |
| VPORTS in model space | Parziale | `src/app/commands/view.rs:561-605` | solo SINGLE, 2H, 2V, 4 |

### 4.6 Layout, stampa, pubblicazione

**Al livello di AutoCAD:**
- viewport: MVIEW rettangolare, poligonale e da oggetto, VPCLIP, scala e blocco della viewport, PSLTSCALE, quote in paper
  space associate al model space (39 test);
- stampa: dialogo PLOT completo con i formati del driver, PAGESETUP con nome e PSETUPIN;
- PDF vettoriale su più pagine, con TTF incorporati e testo ricercabile (anche SHX); CTB letti e applicati.

| Funzione | Stato | Evidenza | Cosa manca |
|---|---|---|---|
| Stampante o plotter Windows | Parziale (ricontrollato) | `src/io/print_to_printer.rs:583-590`, `:1176-1205` | §1 punto 4: scala esatta, più pagine, uscita vettoriale |
| PC3, PLOTTERMANAGER | Assente | `src/io/plot_device.rs:56-81` | — |
| PUBLISH | Parziale | `src/ui/window/print_all.rs`; `src/app/update/file.rs:3724-3967` | PRINTALL fa solo il disegno attivo, in un unico PDF. Un PDF per foglio c'è solo dall'API (`src/app/control/actions.rs:688`). Niente `.dsd` |
| PREVIEW | Parziale | `src/app/update/file.rs:5383` | apre un PDF temporaneo nel lettore esterno |
| CTB | Parziale (dedotto) | `src/io/pdf_export.rs:744-770`; `src/app/update/file.rs:410-435` | Il tipo di linea del CTB è ignorato. Blocchi e xref prendono la penna dell'INSERT; true color e ByBlock restano senza penna |
| Stampa senza CTB | Difetto | `src/io/pdf_export.rs:772-791` | §5 |
| STB, CONVERTCTB | Assente | `src/io/plot_style.rs:277-281` | — |
| Viewport poligonali in stampa | Probabile difetto (dedotto) | `src/scene/project.rs:251` contro `src/scene/view/render.rs:4833` | in stampa le linee si tagliano sul rettangolo, non sul contorno |
| Shade plot / hide plot per viewport | Parziale | `src/scene/mod.rs:7184` | in stampa non hanno effetto |
| Defpoints | Parziale | — | Conta solo il flag Plot: le cornici delle viewport messe su Defpoints possono uscire in stampa |
| Timbro di stampa | Parziale | `src/io/pdf_export.rs:1318` | testo fisso «Open CAD Studio \| utente \| secondi Unix» |
| PDF | Parziale | `src/io/pdf_export.rs` | layer PDF (OCG), collegamenti, segnalibri; il gradiente è reso col colore medio |
| Export raster e DWF | Assente | — | PNGOUT, JPGOUT, EXPORTDWF |
| Layout | Parziale | `src/app/update/viewport.rs:6432` | copia, layout da template, comando LAYOUT, LAYOUTWIZARD, MVSETUP, CHSPACE, VPMAX/VPMIN |
| SHEETSET | Stub | §3.2 | — |

### 4.7 Riferimenti esterni e immagini

**Al livello di AutoCAD:**
- xref: XATTACH attach/overlay con percorso pieno, relativo o nessuno, palette con gli stati, XCLIP anche invertito,
  XDWGFADECTL, VISRETAIN;
- immagini: IMAGECLIP, ricarica delle immagini (fatta il 2026-10-07);
- underlay PDF, DWF e DGN con layer, osnap e clip; PDFIMPORT vettoriale.

| Funzione | Stato | Evidenza | Cosa manca |
|---|---|---|---|
| Xref annidati | **Non visualizzati** (ricontrollato) | `src/ui/window/xref_manager.rs:1797` | la palette li marca «nested — not rendered» |
| Unload | Parziale | `src/io/xref.rs:27-30` | non si salva nel file |
| REFEDIT su un xref | Rischio (dedotto) | `src/io/xref.rs:2044` | le modifiche non arrivano al file dell'xref |
| XOPEN | Parziale | `src/app/commands/blocks.rs:1671` | percorso non risolto: con un percorso relativo probabilmente fallisce |
| Bind | Parziale | `src/io/xref.rs:1708` | solo il tipo Bind: niente Insert né XBIND |
| Reload da riga di comando | Parziale | `src/app/commands/blocks.rs:1058` | rifiuta PDF, DWF, DGN (la palette invece li ricarica) |
| IMAGEATTACH | Parziale | `src/modules/draw/draw/raster_image.rs:81` | Nessuna finestra: niente scala, rotazione, percorso relativo. Formati: solo PNG, JPG, BMP, TIFF |
| IMAGEADJUST | Parziale | `src/app/commands/display.rs:1277`; `src/scene/model/image_model.rs:192` | luminosità e contrasto salvati ma non applicati; funziona solo Fade |
| IMAGEQUALITY, PDFSHXTEXT | Assenti | — | — |
| ETRANSMIT | Parziale | `src/app/commands/fileops.rs:150-205` | Fa una copia piatta in una cartella. Niente zip, font, CTB, underlay né xref annidati; i percorsi non vengono riscritti |
| Avviso di xref modificato | Parziale | `src/app/update/dialog.rs:769` | arriva solo con la palette aperta |

### 4.8 File, formati, round-trip

**Al livello di AutoCAD:**
- formati: lettura DWG R13–2018 (il formato 2018 è quello di AutoCAD 2018–2026), scrittura R14–2018, DXF ASCII,
  PDF import, miniature;
- conservazione: handle mantenuti; le entità non modificate si riscrivono byte per byte; proxy, dizionari, XRECORD, XDATA
  conservati;
- sicurezza del lavoro: SAVETIME, `.bak`, blocco del file interoperabile con impronta del file prima di salvare.

| Funzione | Stato | Evidenza | Cosa manca |
|---|---|---|---|
| Template `.dwt`, QNEW | Parziale | `src/app/commands/fileops.rs:6`; `src/app/control/sheets.rs:402` | Il template si applica solo dall'API. Apri rifiuta i `.dwt`; NEW crea un disegno vuoto |
| Disegno nuovo | Diverso (ricontrollato) | `src/scene/entity.rs:3053-3062` | Nasce con INSUNITS=6 (metri) e stile di quota ISO-25. Per acadiso.dwt ricordo i millimetri, da verificare. Effetto: cambia la scala dei blocchi inseriti |
| Polilinee con 100.000 vertici o più | **Difetto** (ricontrollato) | `src/io/mod.rs:2371-2400` | Scartate all'apertura come «corrotte» e perse al salvataggio. È un filtro contro le desincronizzazioni del parser, ma colpisce anche i rilievi veri |
| Accenti nei DXF prima del 2007 | **Difetto** (ricontrollato) | `codec:io/dxf/writer/text_writer.rs:144-175` | Lo scrittore DXF scrive sempre UTF-8; `encode_legacy_string` lo usa solo il DWG. È la causa del caso R2000 del banco di conformità |
| Oggetti AEC e custom (AutoCAD Architecture) | Parziale | `codec:io/dwg/…/objects.rs:1386-1390`; `src/io/mod.rs:1495-1543` | Salvando nella stessa versione DWG si conservano. Cambiando versione o passando a DXF vengono eliminati, con un avviso prima. Non sono modificabili; la grafica proxy è solo filo |
| DXF binario | Parziale | `src/io/mod.rs:1906` | si legge, non si scrive |
| DWG R12 e precedenti | Assente | `codec:io/dwg/dwg_version.rs:49` | — |
| RECOVER, AUDIT | Parziale | `src/app/commands/styleprops.rs:2827-2871`; `src/io/mod.rs:850-935` | AUDIT controlla solo layer e blocchi non definiti e non ripara. Il recupero parte da solo, e solo se l'apertura fallisce |
| Recupero dopo un crash | Parziale | `src/app/update/file.rs:3302` | all'avvio nessuno segnala i `.sv$` rimasti |
| PURGE | Parziale | `src/app/commands/styleprops.rs:163-518` | Copre blocchi, layer, tipi di linea, stili di testo. Mancano regapp, oggetti a lunghezza zero, testo vuoto e una finestra |
| DWG Compare, Xref Compare, CHECKSTANDARDS | Assenti | — | — |
| `.dwl` di AutoCAD | Parziale | `src/io/edit_lock.rs:137-200` | il blocco è proprio: non legge né scrive i `.dwl` |
| Appunti di sistema | Parziale | `src/app/mod.rs:1046` | passa solo testo: niente copia-incolla con AutoCAD o Office |
| Import OBJ | Parziale, perdita (dedotto) | `src/app/update/mod.rs:1700-1706` | la mesh vive solo in memoria: al salvataggio si perde |
| STEP | Parziale | `src/io/step.rs` | solo export, a triangoli; niente import |
| IFC, IGES, SAT, export DWF/DGN, export raster | Assenti | — | — |
| OLE | Parziale | `src/scene/model/ole_pres.rs` | si vede l'anteprima, l'oggetto non si apre |
| Formati delle immagini | Parziale | `Cargo.toml` (crate `image`) | GIF, JP2, ECW, MrSID |

### 4.9 Personalizzazione, script, automazione, lingua

**Al livello di AutoCAD, o meglio:**
- alias `.pgp` con ALIASEDIT, tasti funzione configurabili;
- MCP, REST, uso senza interfaccia;
- plugin Rust fuori processo, con comandi interattivi e anteprima (§7).

| Funzione | Stato | Evidenza | Cosa manca |
|---|---|---|---|
| Percorsi dei file di supporto | **Assente** | `src/ui/window/options.rs:1621-1624` | La finestra lo dice: «The application has no support-file search path». Retini e tipi di linea sono incorporati |
| Tipi di linea `.lin` | Parziale | `src/io/linetypes.rs:234` | solo i 50 interni più quelli già nel DWG |
| Script `.scr` | Parziale | `src/app/commands/fileops.rs:344-397` | Quasi tutte le forme «-» sono assenti: -LAYER, -PLOT, -PURGE, -HATCH, -STYLE. Mancano anche FILEDIA, CMDDIA, RSCRIPT, DELAY |
| LISP | Assente per scelta | `src/app/commands/view.rs:201` | Esiste un traduttore esterno `.lsp`→`.py` che produce una bozza. I LISP di terzi non girano |
| Python | Parziale | `plugins/opencad-python/PLUGIN.md`; `.github/workflows/archline-windows.yml` | **Non incluso nella build ArchLine.** RustPython senza libreria standard, input non sincrono, nessuna registrazione di comandi |
| Plugin nel workspace classico | Parziale (ricontrollato) | `src/plugin/registry.rs:15-31`; `src/ui/toolbar_registry.rs` | Le schede ribbon dei plugin non hanno equivalente nei menu e nelle barre classiche: i loro comandi si possono solo digitare. **Riguarda il plugin architettonico** |
| CUI | Parziale | `src/app/commands/view.rs:303-306` | Si personalizzano solo le scorciatoie: non menu, barre, doppio clic, Quick Properties. Niente macro `^C^C` né DIESEL |
| Workspace, profili | Assente | `src/workspace.rs`; `src/app/config.rs` | si sceglie solo con una variabile d'ambiente; un solo `settings.json` |
| ACTRECORD | Assente | — | — |
| OPTIONS | Parziale | `src/ui/window/options.rs` | Mancano le schede System e Profiles; Files è in sola lettura |
| Lingua | Parziale | `src/i18n.rs:22` | L'italiano c'è, ma all'avvio l'interfaccia è in inglese. Non ci sono nomi di comando italiani (`_LINE` è accettato; si possono creare alias come `LINEA`). 68 prompt non tradotti; i tasti delle opzioni restano inglesi |
| Express Tools | Parziale | — | Presenti TCOUNT, TCASE, TORIENT, TEXTFIT, TEXTMASK, JUSTIFYTEXT, EXTRIM, NCOPY, FLATTEN, OVERKILL, LAYMRG; ARCTEXT semplificato. Assenti BURST, TXTEXP, COPYM, MPEDIT, BREAKLINE, SUPERHATCH, BLOCKREPLACE, GATTE… |

### 4.10 3D e rendering (secondario per lo studio)

**Al livello di AutoCAD:**
- modellazione: primitive, POLYSOLID, EXTRUDE, REVOLVE, SWEEP e LOFT con B-rep esatto, SLICE, THICKEN, 3DMOVE, storia
  modificabile per primitive ed estrusioni;
- solidi ACIS dei DWG di AutoCAD: si leggono e si mostrano; se non toccati si riscrivono byte per byte;
- visualizzazione: mesh SubD, linee nascoste a schermo, ViewCube, materiali e luci letti dal DWG.

Mancano:
- **Tavole dal 3D.** In PDF i solidi escono solo come spigoli, anche in «Hidden» (dedotto da `src/io/pdf_export.rs:105-111`).
  FLATSHOT fa solo la vista dall'alto. SECTION taglia solo sui piani X/Y/Z e produce linee spezzate. La sezione live non
  ha retino e non genera il blocco 2D. VIEWBASE non c'è.
- **Booleane, raccordi e SHELL limitati**: un muro con finestra sottratta viene rifiutato da FILLETEDGE. SOLIDEDIT è
  quasi assente.
- **3DROTATE, 3DMIRROR, 3DALIGN** lavorano solo sugli assi del WCS e perdono layer e colore (dedotto).
  **INTERFERE** dà un falso negativo.
- **Presentazione**: niente RENDER, niente export immagine, niente stili Realistic e Conceptual.
- **Altro**: superfici NURBS, RULESURF/EDGESURF, geolocalizzazione e sole, IFC. STEP solo a triangoli.

### 4.11 Cloud e IA di Autodesk: fuori scopo per scelta

41 comandi:
- Autodesk Docs: Push to Docs, Connected Support Files;
- condivisione: Share, Shared Views;
- revisione: Trace, Markup Import/Assist, Activity Insights;
- intelligenza artificiale: Autodesk Assistant, la parte di riconoscimento degli Smart Blocks.

Per ArchLine non sono lacune. Due idee però non richiedono il cloud: il **confronto di disegni** (DWG Compare, dal 2019) e
**«Search and Convert»**, che trova la geometria ripetuta e la trasforma in blocchi.

## 5. Difetti: funzioni presenti ma sbagliate

«Analisi» = trovato da uno degli agenti nel codice; «ricontrollato» = verificato anche da me; «dedotto» = da confermare con
una prova a schermo.

| # | Difetto | Evidenza | Fonte | Costo |
|---|---|---|---|---|
| 1 | LAYUNISO accende tutti i layer | `src/app/commands/layers.rs:690-708` | ricontrollato | basso |
| 2 | Viste con nome salvate quadrate: l'area «View» in PLOT è sbagliata | `src/scene/camera_ops.rs:222-251` | ricontrollato | basso |
| 3 | STRETCH lascia fermi i tipi «generici», contro il suo stesso commento | `src/app/command_driver/transform.rs:527-530` | ricontrollato | basso-medio |
| 4 | Campi area/lunghezza su polilinee e tratteggi: valore vecchio, senza avviso | `codec:fields.rs:1053-1082` | ricontrollato | basso-medio (codec MPL: patch da proporre a upstream, oppure aggancio lato app) |
| 5 | Accenti nei DXF prima del 2007: sempre UTF-8 | `codec:io/dxf/writer/text_writer.rs:144-175` | ricontrollato | basso (codec) |
| 6 | Polilinee con 100.000 vertici o più scartate e perse | `src/io/mod.rs:2371-2400` | ricontrollato | basso (distinguere un dato valido da uno corrotto) |
| 7 | Cambiare la visibilità di un blocco dinamico tocca tutte le copie che condividono il `*U`, o la definizione se il riferimento è nello stato predefinito | `src/app/visibility.rs:179-219` | analisi | medio (il codec offre `copy_evaluated_block_metadata`) |
| 8 | ATTSYNC crea gli attributi nuovi all'origine WCS, altezza 2,5, stile Standard, senza la trasformazione del blocco | `src/app/commands/blocks.rs:649` | analisi | basso |
| 9 | BEDIT e REFEDIT su un blocco dinamico aprono il `*U` invece della definizione | `src/app/commands/inquiry.rs:498` | analisi | basso |
| 10 | REFEDIT su un xref: le modifiche non arrivano al file | `src/io/xref.rs:2044` | dedotto | basso per bloccarlo, alto per implementarlo |
| 11 | Stampa senza CTB: giallo diventa nero, ciano blu scuro; gli spessori fino a 0,25 mm escono a circa 0,26 mm | `src/io/pdf_export.rs:772-791`; `src/scene/view/render.rs:3604` | analisi | basso |
| 12 | Il timbro di stampa scrive «Open CAD Studio» e i secondi Unix | `src/io/pdf_export.rs:1318` | analisi | basso |
| 13 | `ZOOM S 1/100xp` non fa nulla e non dà errore | `src/app/commands/dim.rs:914` | analisi | basso |
| 14 | IMAGEADJUST salva luminosità e contrasto ma non li applica | `src/scene/model/image_model.rs:192` | analisi | basso |
| 15 | Impostazioni salvate senza effetto: PICKSTYLE, QTEXTMODE, PLINEGEN, DRAGMODE, SORTENTS, BLIPMODE, REGENMODE, TEXTQLTY, ATTDIA, «Verifica» di ATTDEF, «Auto-Placement», «scala uniforme» | vari, §4 | analisi | basso, ciascuna |
| 16 | CUIIMPORT e CUILOAD non leggono `.cuix` | `src/app/commands/view.rs:352-396` | analisi | basso (almeno un messaggio onesto) |
| 17 | INTERFERE riporta un rifiuto del kernel come «non si sovrappongono» | `src/app/model_ops.rs:1879-1904` | analisi | basso |
| 18 | 3DROTATE, 3DMIRROR, 3DALIGN creano un solido nuovo e perdono layer e colore | `src/app/model_ops.rs:1912-2058` | dedotto | basso |
| 19 | L'OBJ importato si perde al salvataggio | `src/app/update/mod.rs:1700-1706` | dedotto | basso |
| 20 | Viewport poligonali tagliate sul rettangolo in stampa | `src/scene/project.rs:251` | dedotto | medio |
| 21 | Defpoints stampato se il suo flag Plot è acceso | — | analisi; da confermare che AutoCAD non lo stampi mai | basso |
| 22 | Il gestore layer elimina un layer in uso insieme ai suoi oggetti | `src/app/update/command.rs:1361` | analisi | basso |
| 23 | Area Limits/View della PLOT su file: usa l'ultima finestra scelta | `src/app/update/file.rs:5363-5395` | dedotto | basso |

## 6. Priorità proposta

Criteri:
- **impatto** su uno studio di architettura: giudizio mio;
- **costo**: qualitativo, dalle analisi del codice; niente ore.

Ogni voce dei livelli 1–2 è una PR piccola, con test.

**Livello 1 — difetti e gesti quotidiani, costo basso**
1. Snap digitati al prompt (`PointModifier::from_token`).
2. LAYUNISO, viste con nome, `ZOOM nXP`.
3. Tolleranza di chiusura in HATCH (riusando la logica di BOUNDARY).
4. FILLET e CHAMFER: Trim/No trim, Shift per raggio 0, CHAMFER Angle e Polyline.
5. STRETCH sui tipi oggi saltati.
6. ROTATE e SCALE con Copy; Undo e Group nel prompt di selezione; preselezione per LIST, CHPROP, DRAWORDER e gli altri
   comandi del §4.2.
7. I 14 nomi AutoCAD del §3.3 come alias. Cambiare `T` in MTEXT è una scelta tua: rompe l'abitudine di chi usa già ArchLine.
8. ATTSYNC; BEDIT e REFEDIT che aprono la definizione e non il `*U`; REFEDIT bloccato sugli xref finché non salva.

**Livello 2 — dati per abachi e cartigli** (ponte verso il plugin architettonico)
9. Campi Area/Lunghezza su LWPOLYLINE, POLYLINE, HATCH, ELLIPSE, SPLINE, con formato e fattore di conversione nel dialogo.
10. DATAEXTRACTION con il nome del blocco e gli attributi come colonne; COUNT e SELECTSIMILAR per nome di blocco.
11. TABLEEXPORT (`table_to_csv` esiste già).

**Livello 3 — consegna delle tavole**
12. PUBLISH: un PDF per tavola, più disegni, elenco salvabile. I pezzi ci sono già: PRINTALL e `per_page`.
13. Fedeltà dei CTB su blocchi, xref e true color. Prima va misurata, estendendo il banco di conformità al PDF.
14. Stampa su plotter in scala 1:1: prima il raster GDI senza adattamento all'area stampabile, poi l'uscita vettoriale
    (costo alto).
15. Timbro di stampa ArchLine, Defpoints, viewport poligonali (queste ultime dopo una prova).

**Livello 4 — risorse dello studio**
16. Percorsi di supporto per `.pat`, `.lin` e SHX (per esempio la cartella Fonts di AutoCAD). Template `.dwt` dalla GUI e
    QNEW. Decidere l'unità del disegno nuovo: oggi metri.
17. Python nella build ArchLine; forme «-» e FILEDIA per gli script `.scr`.
18. Menu e barre classiche per i comandi dei plugin: **prerequisito del plugin architettonico**.

**Livello 5 — riferimenti**
19. Xref annidati visibili; stato Unload salvato.
20. IMAGEATTACH con finestra (scala, rotazione, percorso relativo); IMAGEADJUST applicato.
21. ETRANSMIT in zip, con font e CTB.

**Livello 6 — costo alto, da pesare**
22. Blocchi dinamici: maniglie e azioni (stretch, flip, rotate, lookup) e proprietà modificabili.
23. Layer: filtri, override per viewport modificabili, `.las`, LAYWALK.
24. Tabelle modificabili a griglia col mouse.
25. Array associativi.
26. Tool Palettes e DesignCenter.
27. 3D per le tavole: linee nascoste vettoriali, sezioni con retino. Solo se il 3D entra nel tuo flusso di lavoro.

**Fuori scopo per scelta:** cloud e IA di Autodesk, LISP/VBA/.NET. RENDER resta fuori salvo una tua decisione diversa.

## 7. Vantaggi di ArchLine verificati nel codice

- **Automazione esterna.**
  - MCP con 4 strumenti; REST con OpenAPI e token; avvio con `--serve`, `--http`, `--script`.
  - `save_verified` riapre il file salvato e ne calcola l'hash; `audit` dice cosa si perde nel formato di destinazione.
  - Per quanto risulta dalle analisi, AutoCAD desktop non ha un server REST o MCP locale nativo (non verificato su
    AutoCAD 2026).
- **Script Python completi, dall'esterno.** Uno script CPython con tutta la libreria standard può chiedere punti e
  selezioni all'utente via REST (`getpoint`, `user_select`).
- **Plugin isolati.** Girano fuori processo: un crash del plugin non chiude l'editor.
- **Vincoli.** Vincoli parametrici completi; i vincoli nativi DWG vengono conservati.
- **Node graph** integrato, con circa 100 nodi: AutoCAD base non ce l'ha.
- **Privacy.** Nessun traffico di rete all'avvio. Eccezione: il download dei font mancanti. Parte su conferma dell'utente ma
  non dipende da `ARCHLINE_ONLINE` (`src/io/font_repo.rs`): da valutare rispetto alla scelta offline del fork.
- **Robustezza.** Blocco del file con impronta prima di salvare, utile su un NAS; ripiego automatico se la GPU non va.

## 8. Documenti da correggere

- [gap-ocs-vs-autocad.md](gap-ocs-vs-autocad.md) §5: così com'è, DATAEXTRACTION non serve per gli abachi (colonne fisse,
  niente attributi).
- `docs/tessellation.md`: SWEEP e LOFT ora producono B-rep esatti, non solo mesh.
- `docs/cadcodec-reader-gaps.md`: in parte superato a fe69506. Sono stati corretti la rotazione degli underlay, i booleani
  290-299, DRAWINGVIEW e SECTIONLINE.
- `docs/cadkernel-body-path.md`: tempi e rifiuti misurati sulla rev 6f046af, da rimisurare su ae28f66.
- Diversi messaggi d'errore dicono «solid created this session», ma i solidi letti da file vengono ricostruiti.

## 9. Cosa non è verificato

- **Comportamento reale.** Non ho eseguito niente: i punti «dedotto» vanno confermati con una prova a schermo su Windows.
- **File reali.** Nel repo c'è un solo DWG reale. Il round-trip con AutoCAD e AutoCAD Architecture (blocchi dinamici,
  vincoli, oggetti AEC) non è provato.
- **Prestazioni** su file grandi.
- **Riferimento AutoCAD.**
  - L'elenco dei comandi è ufficiale.
  - Opzioni e comportamenti vengono da conoscenza generale. In particolare da confermare: l'INSUNITS di acadiso.dwt, che
    Defpoints non si stampi mai, come un CTB tratta i true color.
- **Classificazione per area** dei comandi assenti: è mia.

## Allegato

[parita-autocad-comandi.tsv](parita-autocad-comandi.tsv): 988 righe, una per voce dell'indice AutoCAD 2026. Colonne:
comando, stato in ArchLine (registrato / altro nome / assente), area se assente, Express Tool, pagina della guida Autodesk.
Le descrizioni di Autodesk non sono state copiate. Gli script che l'hanno generata sono nello scratchpad della sessione, non
nel repo.
