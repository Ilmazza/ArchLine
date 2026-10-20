# Parità con AutoCAD: cosa manca ad ArchLine

Stato: 2026-10-17, su `lavoro` (cea29392), codec `opencadcodec` fe69506, kernel `opencadkernel` ae28f66. **Confronto per nome
rimisurato il 2026-10-19 sulla v2026.42** (§1bis, §3); l'analisi di profondità (§4, §5) resta quella del 17/10, salvo le
righe segnate. Riferimento:
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
| registrati in ArchLine con lo stesso nome | 456 (il 17/10: 408 nel TSV) |
| presenti in ArchLine con un altro nome (§3.3) | 13 (14) |
| assenti | 519 (567) |

I conteggi sono rifatti sul codice della v2026.42: 48 voci sono diventate «registrate» (47 erano assenti, BCLOSE era «altro nome»; §1bis).

Come si dividono le 519 assenze:
- **58 fuori scopo per scelta**: servizi cloud e IA di Autodesk (41) e LISP/VBA/.NET/ARX (17).
- **154 tra 3D, rendering e geolocalizzazione.**
- **307 nelle altre aree**: interfaccia 74, riferimenti e import 42, annotazione 31, file 35, disegno 2D 30, stampa 24,
  blocchi 25, blocchi dinamici 13, viste e layer 20, utilità 13.

Registrato non vuol dire funzionante: dei 456, ne restano alcuni che sono stub o rimandi (§3.2, rivisto il 19/10).

**Le dieci lacune che pesano di più su uno studio** (dettaglio al §4, ordine proposto al §6). «Ricontrollato» = verificato
da me nel codice, oltre che dall'analisi.
1. **Gli snap digitati al prompt non sono riconosciuti** (`end`, `mid`, `int`, `per`…). Funzionano solo da menu e barra.
   (ricontrollato)
2. ~~**I campi Area e Lunghezza si calcolano solo su cerchio e su linea, cerchio, arco.**~~ **Corretto lato app** (voce 9):
   polilinee, tratteggi, ellissi e spline si misurano in ArchLine con la geometria del comando AREA e il campo si aggiorna;
   il dialogo ha formato, precisione e fattore di conversione (cm²→m²). Il codec resta com'è: difetto da segnalare a monte
   (§5, voce 4).
3. **HATCH non carica `.pat` dello studio.** La tolleranza di chiusura ora c'è (`HPGAPTOL`, fatto in `claude/parita-l1-04-hatch-gap`). (ricontrollato)
4. **La stampa su plotter passa sempre da un PDF temporaneo.** Se è installato un lettore PDF che gestisce la stampa diretta
   (`printto`), stampa lui e la scala la decide il lettore. Altrimenti ArchLine stampa un raster a 300 DPI, adattato all'area
   stampabile e solo della prima pagina. Niente PC3. (ricontrollato)
5. **Pubblicazione solo parziale** (aggiornato il 19/10): `PUBLISH` c'è come sinonimo di «Print all» e il dialogo dà un PDF
   per tavola (nostro). Mancano più disegni e l'elenco salvabile (DSD).
6. ~~**Estrazione dati e conteggi non servono per gli abachi.**~~ **Corretto per i blocchi** (voce 10): DATAEXTRACTION ha le
   colonne «Block name» e una per ogni attributo, `COUNT BLOCK` conta per blocco, SELECTSIMILAR non mescola più i blocchi.
   Restano fuori le proprietà dinamiche, i MINSERT espansi e i blocchi annidati (§4.4).
7. ~~**Dei blocchi dinamici si cambia solo la visibilità**~~ **Superato in parte da upstream v2026.42**: ci sono
   BPARAMETER, BACTION, BVSTATE, BLOOKUPTABLE, BTESTBLOCK, BVHIDE/BVSHOW con test (`src/app/block_editor_tests.rs`).
   La profondità (maniglie, flip, lookup su blocchi di terzi) **non è rimisurata** e va provata a schermo; il difetto
   della visibilità che tocca tutte le copie (§5) non è ricontrollato.
8. **Non ci sono percorsi di supporto.** `.pat` e `.lin` esterni non si caricano, gli SHX si cercano solo vicino al disegno,
   la GUI non offre template (NEW crea un disegno vuoto in metri).
9. **Gli xref annidati non si vedono nel disegno** (ricontrollato) e lo stato Unload non si salva.
10. ~~STRETCH lascia fermi tratteggi, spline, punti, MLine e wipeout~~ **Corretto** in `claude/parita-l1-06-stretch` (tranne tabelle e leader multiplo). FILLET e CHAMFER ora hanno
    Trim/No trim (`claude/parita-l1-05-fillet-chamfer`); CHAMFER lavora ancora solo su linee e polilinee.

**Difetti** (funzioni presenti ma sbagliate), elenco completo al §5:
- ~~LAYUNISO accende tutti i layer~~ **Fatto** (`claude/parita-l1-02-layuniso`): riaccende solo i layer che LAYISO ha spento;
- le viste con nome si salvano quadrate;
- cambiare la visibilità di un blocco dinamico tocca tutte le copie, o la definizione;
- accenti corrotti nei DXF R2000;
- le polilinee con 100.000 vertici o più vengono scartate all'apertura e perse al salvataggio.

**Vantaggi verificati** (§7): automazione esterna (MCP, REST, uso senza interfaccia), blocco dei file interoperabile,
nessun traffico di rete all'avvio, vincoli parametrici completi, node graph.

## 1bis. Aggiornamento del 2026-10-19: upstream v2026.42

L'analisi di questo documento è del 2026-10-17 e misurava la v2026.40.1. Il 18/10 upstream ha rilasciato la **v2026.42** (250 commit dalla .41; `Cargo.toml` 2026.42.0, codec `acadrust` rev 0dcd5aa) e ArchLine l'ha unita nel branch `claude/merge-upstream-2026.42` (16 file in conflitto con il fork risolti a mano; il lavoro di parità è poi stato riportato commit per commit, scartando o riducendo ciò che upstream fa già; tutti i test passano). Le voci che seguono hanno cambiato stato; le altre di questo documento **non sono state rimisurate**.

**Verificato nel codice o con un test dopo l'unione**
- Fatti da upstream: snap digitati, viste con nome e `ZOOM nXP`, FILLET/CHAMFER senza taglio e su polilinea, STRETCH di punti e spline, ATTSYNC, BEDIT/REFEDIT (REFEDIT scrive sugli xref), UPDATEFIELD e il modello dei campi aggiornati su eventi (apertura, salvataggio, rigenera, plot), `COUNT` come palette per blocco, `PUBLISH` come sinonimo di «Print all» (un PDF con tutti i layout, o stampante).
- Restano nostri: LAYUNISO, `HPGAPTOL`, ROTATE/SCALE Copy, Undo/Group e preselezione, i sette alias, STRETCH di leader/MLine/wipeout/tratteggi, DATAEXTRACTION con blocco e attributi, SELECTSIMILAR per blocco, TABLEEXPORT, campi Area/Lunghezza su polilinee, tratteggi, ellissi e spline (ora agganciati a `update_fields`: i campi si aggiornano su apertura, salvataggio, rigenera e UPDATEFIELD, non più a ogni ridisegno), un PDF per tavola da «Print all».
- Il difetto del codec sui campi (area solo del cerchio, lunghezza di linea, cerchio, arco) c'è ancora nel rev 0dcd5aa.
- **Corretto dal codec nuovo**: gli accenti nei DXF prima del 2007 (§5, voce 5) escono giusti; lo ha misurato il banco di conformità il 2026-10-19 (`text_accents` R2000 da ATTESO a OK).
- **Riaperti** (il lavoro non è sopravvissuto all'unione): FILLET con Shift per il raggio 0, CHAMFER Angle/Method, CHAMFER su tutta la polilinea.
- **Campi in una cella di tabella**: l'aggiornamento misurato riguarda solo i testi (MTEXT, TEXT, attributi); una cella con un campo Area di polilinea mostra il valore memorizzato (non coperto, non verificato).

**Rimisura per nome (2026-10-19).** Ho estratto dal codice i nomi di tutti i `CommandRegistration { names }` (819) e li ho confrontati con le 988 voci dell'indice: nessuna voce registrata il 17/10 è sparita; 47 assenti ora sono registrate e BCLOSE era un «altro nome» e ora è un comando. Per ognuna ho controllato che ci sia un gestore (non un messaggio «non implementato»):
- **blocchi dinamici (authoring)**: BPARAMETER, BACTION, BACTIONSET, BACTIONTOOL, BAUTHORPALETTE(CLOSE), BLOOKUPTABLE, BTESTBLOCK, BVSTATE, BVHIDE, BVSHOW, BSAVEAS;
- **annotazione**: COUNTAREA, COUNTFIELD, COUNTLIST(CLOSE), COUNTTABLE, COUNTCLOSE, UPDATEFIELD, TXT2MTXT, TXTEXP;
- **stampa e fogli**: PUBLISH, MVSETUP, NEWSHEETSET, OPENSHEETSET, SHEETSETHIDE (e SHEETSET/SSM ora aprono il Sheet Set Manager);
- **import e riferimenti**: IMPORT, DGNIMPORT (e `-DGNIMPORT`), PDFSHXTEXT, WMFIN, ACISIN, IMAGEADJUST (e `-IMAGEADJUST`), DWFADJUST, DGNADJUST, REFSET;
- **modifica e viste**: ARRAYEDIT, PASTESPEC, PTYPE, CHSPACE, DVIEW, COMPARE, QNEW; interfaccia: RIBBON, RIBBONCLOSE, EXTERNALREFERENCESCLOSE.

Il confronto è per nome: dice che il comando esiste e ha un gestore, **non** quanto sia profondo rispetto ad AutoCAD. La classificazione per area delle assenti resta quella del 17/10. Ho tenuto per assenti le 33 voci che compaiono nel codice solo come stringa (alias `pgp.rs`, catalogo delle traduzioni, parole comuni come `SELECT`, `LOAD`, `FILL`): nessuna è un comando con quel nome. Il TSV è aggiornato; la procedura (estrazione con una regex sui `CommandRegistration`, controllo a mano dei gestori) non è uno strumento nel repo.

**Dalle note di rilascio della v2026.42, non verificato nel codice** (da ricontrollare prima di contare sui nomi in §3.1 e sulle funzioni in §4): blocchi dinamici con parametri e azioni, tabelle di ricerca e di proprietà; `IMPORT`, `DGNIMPORT`, `PDFIMPORT`, `PDFSHXTEXT`; `EXPORTSVG`, `FBXEXPORT`; `MVSETUP`, `-VIEW`, `DVIEW`, `PASTESPEC`, `FILTER`, `TXT2MTXT`, `TXTEXP`, `ATTMAN`/`BATTMAN`, `HPASSOC`, `SELECTIONCYCLING`, `ARRAYRECT`/`ARRAYPOLAR` associativi, `ADJUST`/`-IMAGEADJUST`, `LAYCUR`, `MEASUREGEOM` con raggio e angolo; dock e palette riscritti, Graphic Attributes, menu a tendina classico (in ArchLine compare solo fuori dal workspace classico); PDF con i layer del disegno.

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
| Interfaccia e personalizzazione | 74 | `WORKSPACE`, `WSSAVE`, `CUSTOMIZE`, `ACTRECORD` (Action Recorder), `TOOLBAR`. Molti degli altri sono comandi di chiusura palette (`…CLOSE`): impatto basso |
| Riferimenti, immagini, import | 42 | `IMAGEQUALITY`, `-XBIND`, `DGNEXPORT`, `INSERTOBJ` e `OLE*`, hyperlink (`ATTACHURL`…), Express `XLIST` |
| Annotazione | 31 | `DIM` (quota unica), `DIMREASSOCIATE`, `DIMDISASSOCIATE`, `DIMINSPECT`, `DIMOVERRIDE`, `TEXTALIGN`, `SPELL`, `HATCHSETORIGIN`, `HATCHGENERATEBOUNDARY` |
| File e qualità | 35 | `RECOVER`, `RECOVERALL`, `DRAWINGRECOVERY`, `XCOMPARE`, `CLOSEALL`, `PARTIALOPEN`, `STANDARDS`/`CHECKSTANDARDS`, `EXPORTTOAUTOCAD`/`PURGEAECDATA` (file di AutoCAD Architecture), `DWGCONVERT` |
| Disegno e modifica 2D | 30 | `CHANGE`, `COPYTOLAYER`, `CUTBASE`; Express `BREAKLINE`, `COPYM`, `MPEDIT`, `SUPERHATCH`, `EXOFFSET` |
| Stampa e pubblicazione | 24 | `PREVIEW`, `PLOTTERMANAGER`, `PLOTSTAMP`, `CONVERTCTB`/`CONVERTPSTYLES`, `EXPORTLAYOUT`, `PNGOUT`/`JPGOUT`, `LAYOUTWIZARD` |
| Blocchi e attributi | 25 | `BCOUNT`, `BURST`, `BLOCKREPLACE`/`BREPLACE`, `GATTE`, `ATTIPEDIT`, `ATTOUT`/`ATTIN`, `TINSERT`, `GROUPEDIT`, `BLOCKICON` |
| Blocchi dinamici (authoring) | 13 | `BTABLE`, `BCPARAMETER`, `BGRIPSET`, `BCONSTRUCTION`, `BASSOCIATE`, `RESETBLOCK` (il nucleo BPARAMETER/BACTION/BVSTATE/BTESTBLOCK ora c'è) |
| Viste, layer, viewport | 20 | `LAYWALK`, `LAYVPI`, `VPMAX`/`VPMIN`, `UCSMAN`, `VIEWBACK`/`VIEWFORWARD` |
| Utilità | 13 | `STATUS`, `TIME`; per gli script `DELAY`, `RESUME`, `RSCRIPT` |
| 3D, rendering, geolocalizzazione | 154 | §4.10 |
| Cloud e IA di Autodesk | 41 | fuori scopo per scelta (§4.11) |
| LISP/VBA/.NET/ARX | 17 | fuori scopo per scelta |

**Express Tools.** L'indice ne elenca 84: 13 sono registrati in ArchLine (`ALIASEDIT`, `ARCTEXT`, `EXTRIM`, `FLATTEN`,
`SAVEALL`, `TCASE`, `TCOUNT`, `TEXTFIT`, `TEXTMASK`, `TORIENT`, `TXTEXP`, `VPSYNC`, `XDATA`), 71 sono assenti.

### 3.2 Registrati ma non funzionanti

| Nome | Cosa fa davvero | Evidenza |
|---|---|---|
| `TOOLPALETTES` | dice «not yet implemented» (ricontrollato 19/10) | `src/app/commands/display.rs:642-645` |
| ~~`SHEETSET`, `SSM`~~ | **Superato da upstream**: aprono il Sheet Set Manager (19/10) | `src/app/commands/sheet_set.rs:535` |
| `ADCENTER`, `ADC`, `CONTENTBROWSER` | stampano in riga di comando i nomi di blocchi e layer del disegno corrente (ricontrollato 19/10) | `src/app/commands/blocks.rs:695-715` |
| ~~`BATTMAN`, `ATTMAN`~~ | **Superato da upstream**: aprono il gestore degli attributi (finestra con modifica delle definizioni e, a scelta, dei riferimenti); non provato a schermo (19/10) | `src/app/commands/attman.rs` |
| `CUIIMPORT`, `CUILOAD` | leggono un file di testo «TASTO COMANDO», non un `.cuix` | `src/app/commands/view.rs:352-396` |
| `FILTER` | è un alias di QSELECT: niente AND/OR/NOT né filtri salvati (la riga è ancora nel dispatch, `inquiry.rs:118`; confronto con le note della v2026.42 non fatto) | `src/app/commands/inquiry.rs:118` |
| `QLEADER` | è un alias di LEADER, senza finestra delle impostazioni | `src/app/commands/dim.rs:115` |
| `QTEXTMODE`, `PICKSTYLE` | il valore si salva ma non ha effetto | `src/app/commands/styleprops.rs:2375`, `:2451` |
| `ARCTEXT` | crea un TEXT per carattere: niente oggetto modificabile (ricontrollato 19/10) | `src/app/commands/dim.rs:1495-1530` |
| `DWGPROPS` | nessuna finestra: stampa le proprietà in riga di comando (ricontrollato 19/10) | `src/app/commands/view.rs:100-125` |

### 3.3 Presenti con un altro nome: alias a costo quasi nullo

| AutoCAD | ArchLine |
|---|---|
| `GCPARALLEL` / `GCTANGENT` / `GCSYMMETRIC` / `GCCOLLINEAR` | `PCONSTRAINT` / `TCONSTRAINT` / `SYCONSTRAINT` / `LCONSTRAINT` |
| `BSAVE` / `BCLOSE` | `BEDIT_SAVE` (**alias fatto**) / `BCLOSE` (**ora è un comando di upstream**, 19/10: non verificato se chieda prima di scartare) |
| `MTEDIT` | `TEXTEDIT` |
| `LAYERSTATESAVE` | `LAYERSTATE` |
| `NEWVIEW` | `VIEW` |
| `XBIND` | `XREF` → Bind (**non fatto**: XBIND lega simboli scelti, `XREF Bind` l'intero riferimento) |
| `LAYOUT`, `MODEL` | schede e `LAYOUTMANAGER` (manca il comando `LAYOUT` con le sue opzioni) |
| `ULAYERS`, `PDFADJUST` | **registrati** (compaiono nell'autocompletamento) |

**Fatti** in `claude/parita-l1-08-alias-attsync` (`src/app/commands/autocad_names.rs`, verificati da test: stesso esito del comando di destinazione): `GCPARALLEL`, `GCTANGENT`, `GCSYMMETRIC`, `GCCOLLINEAR`, `BSAVE`, `MTEDIT`, `LAYERSTATESAVE` (→ `LAYERSTATE SAVE`), `NEWVIEW` (→ `VIEW`, che offre Save). Funzionano anche con il tuo `ocad.pgp` già copiato, perché sono nel codice. **Non fatti**, per i motivi sopra o perché il comando di destinazione non fa la stessa cosa: `XBIND`, `LAYOUT`, `MODEL`.

Ci sono anche differenze negli alias di `assets/ocad.pgp`, da controllare una per una prima di cambiarle:
- ~~`T` apre TEXT, mentre in AutoCAD apre MTEXT~~ **Cambiato su tua decisione** (`claude/parita-l1-09-alias-t`): `T` apre MTEXT. Chi ha già l'`ocad.pgp` con `T` su TEXT viene portato su MTEXT alla prima apertura (migrazione v6); un `T` personalizzato non si tocca. `DT` resta TEXT;
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
| STRETCH | Parziale | `src/app/command_driver/stretch_grips.rs`, `transform.rs` | **Ora si spostano** (verificato da test, non a schermo) i vertici dentro la finestra di punti, spline, leader, MLine, wipeout e tratteggi non associativi, con la stessa logica dei grip. Un tratteggio associativo segue il suo contorno e non si stira da solo. Restano fermi: tabelle, MULTILEADER, immagini, solidi. Archi e cerchi si spostano interi solo se il centro è dentro. Limite: nel leader l'annotazione (MTEXT) non segue la finestra; il gomito trascina il tratto finale come col grip. Commento di `stretch.rs` corretto |
| FILLET | Parziale | `src/modules/draw/modify/fillet.rs` | **Fatti** (verificati da test, non a schermo): opzione Trim (T) con Trim/No trim, ricordata per la sessione e condivisa con CHAMFER; Shift sul secondo oggetto = raggio 0. Limiti: con No trim un raccordo su segmenti di polilinea resta tagliato (la polilinea si ricostruisce in un pezzo); `TRIMMODE` non è digitabile come variabile. Mancano: raccordo su cerchi, ellissi, spline |
| CHAMFER | Parziale | `fillet.rs` | **Fatti** (verificati da test, non a schermo): Angle (lunghezza sulla prima linea e angolo da essa; default sessione 10 e 45°, scelti da me, non da AutoCAD che parte da 0), mEthod (Distance/Angle; D e A lo impostano), Trim/No trim, Polyline (tutti i vertici; i vertici accanto a un arco restano; se le distanze non stanno nei lati, rifiuta tutta l'operazione come fa FILLET). Limiti: ancora solo linee e polilinee (niente archi); le larghezze di vertice delle polilinee vengono copiate dal vertice originale |
| TRIM/EXTEND | Parziale | `src/modules/draw/modify/trim.rs:2771-2815` | Project, Undo interno, modo Standard (c'è solo Quick). Non taglia i tratteggi; blocchi, testi e tratteggi non fanno da spigolo di taglio |
| ROTATE, SCALE | Parziale | `rotate.rs`, `scale.rs` | **Copy fatto** (verificato da test e da prova end-to-end con `ROTATE 0,0 C 90` e `SCALE 0,0 C 2`: l'originale resta, nasce la copia, il comando finisce; resta attiva anche con Reference). Manca: Points nell'opzione Reference |
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
| Prompt «Select objects» | Parziale | `src/modules/draw/select.rs`, `command_driver/select_sets.rs` | **Undo (U) e Group (G) fatti** (verificati da test): U toglie l'ultima selezione, G chiede il nome del gruppo (senza distinzione di maiuscole) e ne aggiunge i membri; un gruppo chiamato come una parola chiave (ALL, P, L…) viene preso dalla parola chiave. Mancano: Box, Auto, SIngle |
| Preselezione | Parziale | `src/app/commands/inquiry.rs`, `layers.rs` | **Corretto** (verificato da test): LIST, ISOLATEOBJECTS, HIDEOBJECTS e LAYISO senza selezione ora chiedono gli oggetti. **Correzione all'analisi**: CHPROP, DRAWORDER e TCASE lo facevano già. PICKFIRST non c'è |
| SELECTSIMILAR | ~~Parziale~~ **Corretto per i blocchi** | `src/scene/selection.rs:352-382` | confronta tipo, layer e, per i riferimenti a blocco, il nome della definizione (senza distinguere maiuscole; un blocco dinamico conta col nome della definizione, non della copia `*U`) |
| Selezione di entità dentro i blocchi | Assente | — | — |
| UNDO | Parziale | `src/app/commands/fileops.rs:72-85` | Mark/Back/BEgin/End, MREDO |
| UCS | Parziale | `src/app/commands/layerprops.rs:179-560` | Previous, ZAxis, UCSMAN, UCS dinamico |
| CAL / QUICKCALC | Parziale | `src/app/commands/inquiry.rs:120-141` | solo numeri, niente punti |
| Palette e finestre di disegno flottanti | Assente | `src/app/config.rs:82` | palette solo ancorate a sinistra o a destra |
| Variabili di sistema | Parziale | `src/app/commands/styleprops.rs:1095-2725` | Circa 155 gestite, contro 1027 documentate nella guida 2026. Sono una catena di `match`, senza registro. Mancano FILEDIA, CMDDIA, EXPERT, PICKFIRST, PEDITACCEPT, TRIMEXTENDMODE, MSLTSCALE. HPGAPTOL è stata aggiunta (`src/app/commands/hatchvars.rs`). Otto si salvano ma non hanno effetto: PICKSTYLE, PLINEGEN, DRAGMODE, QTEXTMODE, SORTENTS, BLIPMODE, REGENMODE, TEXTQLTY |

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
| Campi Area/Lunghezza | **Corretto lato app** | `src/entities/field_measure.rs`; `src/entities/field.rs` (aggancio); `src/ui/window/field_dialog.rs` | Il codec valuta l'area solo del cerchio e la lunghezza di linea, cerchio e arco (`codec:fields.rs`, rev 0dcd5aa: `area` a `:987`, `length` a `:995`); per il resto ArchLine misura LWPOLYLINE, POLYLINE 2D e 3D, HATCH (solo area), ELLIPSE e SPLINE con la geometria del comando AREA e passa il numero al formattatore del codec, quindi anteprima, testo con campo e celle di tabella si aggiornano. Il dialogo Oggetto ha Formato, Precisione e Fattore di conversione (`%lu`, `%pr`, `%ct8[f]`, `%qf1` per le aree). Limiti: l'area di una polilinea aperta è quella chiusa dalla corda (come AREA; per AutoCAD è dedotto, non provato); l'area dell'arco di ellisse idem; un tratteggio non ha lunghezza; manca il prefisso/suffisso (`%ps`, per esempio «m²»); UPDATEFIELD resta assente (il testo con campo si rivaluta a ogni ridisegno, `src/entities/mtext.rs:88`; non verificato che il valore salvato nel file sia quello nuovo) |
| HATCH, tolleranza di chiusura | **Presente** (fatto; verificato da test, non a schermo) | `src/app/commands/hatchvars.rs`, `draw.rs` (HATCH e GRADIENT) | `HPGAPTOL n` (0–5000, 0 = chiusura esatta, come AutoCAD) chiude lacune fino a n unità nel contorno, con la stessa `boundary_faces` di BOUNDARY. Vale per la sessione: l'header del codec non ha il campo, quindi non si salva nel disegno. Manca l'opzione nella finestra/prompt di HATCH. Da verificare a schermo: come si vede il tratteggio sulla lacuna chiusa |
| HATCH, pattern | Parziale | `src/io/patterns.rs:11` | Un solo `.pat` incorporato: 106 pattern di derivazione QCAD. Non si caricano `.pat` dell'utente; `acad.pat` e `acadiso.pat` non sono completi |
| HATCH da riga di comando | Parziale | `src/modules/draw/draw/hatch.rs:538` | solo P/A/L |
| Stili di quota figli | Parziale | `src/entities/dimension.rs:3674-3678` | Non si applicano in creazione né in modifica; il dialogo non ha «Stili per». Esempio: un raggio con freccia sotto uno stile lineare con tick |
| ARCHTICK | Parziale | `src/scene/convert/tessellate.rs:2696` | disegnato come OBLIQUE, senza spessore |
| `DIM`, `DIMREASSOCIATE`, `DIMDISASSOCIATE`, `DIMINSPECT` | Assenti | — | — |
| Editor MTEXT | Parziale | `src/app/mtext_editor.rs` | Mancano elenchi puntati e numerati, tabulazioni, inserimento di campi e controllo ortografico. Il giustificato è reso allineato a sinistra; le colonne dinamiche non rifluiscono |
| Font SHX | Parziale | `src/scene/text/font_face.rs:80-97`; `src/font_alt.rs` | Nessun percorso di ricerca. Un font mancante si disegna con un sostituto LFF; la finestra "Missing fonts" fa scegliere il sostituto (LFF o font di Windows) e lo ricorda in `settings.json` (equivalente di FONTALT/`acad.fmp`, ma per utente e non per disegno). Larghezze misurate contro gli SHX di AutoCAD 2025: identiche per romans, simplex, txt; approssimate per gli altri (isoct −28%, monotxt −25%, isocp +10%). Il download da GitHub (`src/io/font_repo.rs`) resta solo con `ARCHLINE_ONLINE=1` |
| TrueType grassetto/corsivo | Parziale | `src/entities/text_support.rs:643-660` | simulati (obliquo, tratto più largo) |
| FIND/REPLACE | Parziale | `src/app/find_replace.rs:205-245` | Non cerca in quote, MLEADER e tabelle. Mancano maiuscole/minuscole, parola intera e caratteri jolly |
| SPELL | Assente nella GUI | `src/app/control/text_ops.rs:967` | solo dall'API (`text_audit`) |
| TXTEXP, TEXTALIGN; QTEXT | Assenti; QTEXT senza effetto | — | — |
| Tabelle | Parziale | `src/modules/annotate/table_cmd.rs:764`; `src/app/commands/display.rs:14-133` | Si modificano solo da riga di comando o da Proprietà: non si selezionano celle col mouse. L'interruzione su più parti non si gestisce. **TABLEEXPORT** (`src/app/commands/table_export.rs`): scrive la tabella scelta (selezionata o indicata) in un CSV con virgola, lo stesso testo di `DATALINKUPDATE WRITE`, quindi rileggibile con DATALINK; senza BOM (con il BOM la rilettura da DATALINK metterebbe U+FEFF nella prima cella), quindi Excel in italiano può richiedere «Dati → Da testo/CSV» e la codifica UTF-8; le celle unite non sono espanse (dedotto dal codice). Excel solo in lettura |
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
| DATAEXTRACTION / EATTEXT | **Parziale; utile per gli abachi di blocchi** | `src/app/annotation_data.rs`, `src/app/extraction_blocks.rs` | Nuove proprietà: «Block name» (nome della definizione, anche per i blocchi dinamici) e una colonna per ogni tag di attributo trovato nella sorgente (categoria «Attributes», spente di default; si ricordano nelle impostazioni salvate). Con «combina identici» e senza Handle/Details le righe si raggruppano per blocco e attributi. Gli oggetti estratti sono solo quelli di spazio modello e layout, non la geometria dentro le definizioni di blocco (prima si contava anche quella). Mancano le proprietà dinamiche, l'espansione dei blocchi annidati e dei MINSERT (un MINSERT è un riferimento). Esce solo CSV/TXT, e la tabella non è collegata |
| COUNT / BCOUNT | ~~Assente per i blocchi~~ **`COUNT BLOCK`** | `src/app/commands/inquiry.rs:1244-1290` | Conta i riferimenti a blocco di spazio modello e layout per nome di definizione (non quelli annidati nelle definizioni; un MINSERT conta 1). `COUNT` per tipo o layer conta ancora tutto il documento, definizioni di blocco comprese: stesso difetto di prima, non toccato |
| Blocchi dinamici, uso | Parziale (misura del 17/10: **superata in parte** dalla v2026.42, non rimisurata) | `src/app/visibility.rs:179-219`; `src/entities/object_data.rs:719-790` | Funziona solo la visibilità; le altre proprietà si vedono in sola lettura. Mancano le maniglie lineare, polare, XY, rotazione, flip, lookup e le loro azioni |
| Blocchi dinamici, authoring | **Registrato con gestori e test dal 19/10; profondità non rimisurata** | `src/app/commands/bvisibility.rs`, `src/modules/block_editor/`, `src/app/block_editor_tests.rs` | Restano assenti BTABLE, BCPARAMETER, BGRIPSET, BCONSTRUCTION, BASSOCIATE, RESETBLOCK. Da provare a schermo con un blocco di terzi |
| BEDIT | Parziale | `src/app/commands/inquiry.rs:463-660` | Niente parametri. Si entra solo scegliendo un riferimento: un blocco mai inserito non si modifica. **Corretto**: su un riferimento dinamico apre la definizione, non il `*U` (verificato da test con una rappresentazione sintetica, non con un blocco dinamico reale: da provare a schermo con un blocco della libreria) |
| REFEDIT | Parziale | `inquiry.rs`, `blockedit_target.rs` | ATTDEF esclusi; niente REFSET. Su un riferimento dinamico apre la definizione; su un xref rifiuta con un messaggio (verificato da test) |
| ATTSYNC | Parziale | `src/app/commands/blocks.rs` | **Difetto corretto** (verificato da test): l'attributo nuovo prende posizione, altezza, rotazione, stile e layer dalla definizione, con la trasformazione dell'inserimento; i valori degli attributi già presenti non cambiano |
| BATTMAN | ~~Assente~~ **Fatto da upstream** (19/10, non provato a schermo) | `src/app/commands/attman.rs` | — |
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
| LAYUNISO | **Corretto** (verificato da test) | `src/app/commands/layers.rs` (`layiso_off` in `DocumentTab`) | riaccende solo i layer spenti da LAYISO; senza LAYISO precedente avvisa e non tocca nulla. Limite: lo stato è per sessione e non entra nell'undo; dopo un UNDO di LAYISO, LAYUNISO riaccende comunque quei layer |
| LAYISO | Parziale | `layers.rs:588-622` | solo Off; vuole una preselezione |
| Filtri layer | Assente | `src/ui/window/layers.rs:451` | per proprietà, di gruppo, invertiti (c'è solo la ricerca per nome) |
| Gestore layer | Parziale | `src/ui/window/layers.rs:386-420` | Mancano le colonne Descrizione, stile di stampa, New VP Freeze e override per viewport. Elimina un layer in uso con i suoi oggetti, previo avviso (`src/app/update/command.rs:1361`); AutoCAD invece rifiuta |
| Override per viewport | Parziale | `src/scene/view/render.rs:3455-3680` | Si leggono e si vedono, ma non si creano né si modificano, anche se il codec ha `set_layer_viewport_override` |
| LAYFRZ dentro una viewport | Parziale | `layers.rs` | congela ovunque |
| Layer states `.las` | Assente | — | import/export |
| LAYWALK, COPYTOLAYER, LAYVPI, LAYERNOTIFY, LAYLOCKFADECTL | Assenti | — | — |
| Viste con nome | **Parziale** | `src/scene/camera_ops.rs:222-251`; `src/app/commands/layerprops.rs:766-875` | **Larghezza ora corretta** (aspetto della finestra attiva; verificato da test, non a schermo: le viste già salvate restano quadrate). Solo model space e solo da riga di comando; non conservano prospettiva, stile, layer |
| ZOOM | Parziale | `src/app/commands/dim.rs:829-938` | Center. `ZOOM S` capisce `n`, `nX` e `nXP` (anche frazioni, `1/100XP`): `nXP` imposta la scala del viewport di layout attivo; senza viewport attivo o con viewport bloccato dice perché non cambia nulla; un valore illeggibile dà errore. In model space `nXP` non ha effetto (errore esplicito). Il nome in minuscolo funziona dalla GUI, non da `run_command_line` |
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
| PUBLISH | Parziale | `src/ui/window/print_all.rs`; `src/app/update/file.rs:3724-3967`; `src/io/publish.rs` | PRINTALL fa solo il disegno attivo. **Un PDF per tavola** ora c'è anche dal dialogo (casella «One PDF per sheet», file `nome-Layout.pdf`, nomi che collidono numerati `-2`, `-3`; API e dialogo usano lo stesso codice). Mancano più disegni in un colpo e l'elenco salvabile (niente `.dsd`) |
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
| REFEDIT su un xref | **Bloccato** (verificato da test) | `inquiry.rs` | rifiuta con un messaggio; scrivere sul file dell'xref non è implementato |
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
| 1 | ~~LAYUNISO accende tutti i layer~~ **corretto** | `src/app/commands/layers.rs` | ricontrollato | basso |
| 2 | ~~Viste con nome salvate quadrate~~ **corretto per le nuove** (le già salvate restano quadrate) | `src/scene/camera_ops.rs:222-251` | ricontrollato | basso |
| 3 | ~~STRETCH lascia fermi i tipi «generici»~~ **corretto** per punti, spline, leader, MLine, wipeout e tratteggi liberi | `src/app/command_driver/stretch_grips.rs` | ricontrollato | basso-medio |
| 4 | Campi area/lunghezza su polilinee e tratteggi: valore vecchio, senza avviso | `codec:fields.rs:1053-1082` | ricontrollato | basso-medio (codec MPL: patch da proporre a upstream, oppure aggancio lato app) |
| 5 | Accenti nei DXF prima del 2007: sempre UTF-8 | `codec:io/dxf/writer/text_writer.rs:144-175` | ricontrollato | basso (codec) |
| 6 | Polilinee con 100.000 vertici o più scartate e perse | `src/io/mod.rs:2371-2400` | ricontrollato | basso (distinguere un dato valido da uno corrotto) |
| 7 | Cambiare la visibilità di un blocco dinamico tocca tutte le copie che condividono il `*U`, o la definizione se il riferimento è nello stato predefinito | `src/app/visibility.rs:179-219` | analisi | medio (il codec offre `copy_evaluated_block_metadata`) |
| 8 | ~~ATTSYNC crea gli attributi nuovi all'origine WCS, altezza 2,5, stile Standard, senza la trasformazione del blocco~~ **corretto** | `src/app/commands/blocks.rs:649` | analisi | basso |
| 9 | ~~BEDIT e REFEDIT su un blocco dinamico aprono il `*U` invece della definizione~~ **corretto** | `src/app/commands/inquiry.rs:498` | analisi | basso |
| 10 | REFEDIT su un xref: ~~le modifiche non arrivano al file~~ **ora è bloccato con un messaggio** | `src/io/xref.rs:2044` | dedotto | basso per bloccarlo, alto per implementarlo |
| 11 | Stampa senza CTB: giallo diventa nero, ciano blu scuro; gli spessori fino a 0,25 mm escono a circa 0,26 mm | `src/io/pdf_export.rs:772-791`; `src/scene/view/render.rs:3604` | analisi | basso |
| 12 | Il timbro di stampa scrive «Open CAD Studio» e i secondi Unix | `src/io/pdf_export.rs:1318` | analisi | basso |
| 13 | ~~`ZOOM S 1/100xp` non fa nulla e non dà errore~~ **corretto** | `src/app/commands/dim.rs:914` | analisi | basso |
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

**Livello 1 — difetti e gesti quotidiani, costo basso** (stato dopo l'unione con la v2026.42; «upstream» = lo fa il motore di Open CAD Studio)
1. Snap digitati al prompt: **upstream** (`typed_snap_override`, più completo del mio; il mio commit è stato scartato).
2. LAYUNISO: **fatto** (nostro). Viste con nome e `ZOOM nXP`: **upstream** (`last_render_aspect`, `set_viewport_scale_factor`); restano nostri i messaggi di `ZOOM nXP` (valore illeggibile, viewport bloccata o assente: upstream zooma la vista in silenzio) e i test.
3. Tolleranza di chiusura in HATCH (`HPGAPTOL`): **fatta** (nostra), per `-HATCH` e per la finestra Hatch.
4. FILLET e CHAMFER: **upstream** ha Trim/No trim (TRIMMODE), Polyline per FILLET e CHAMFER su un angolo, Multiple. **Riaperti, da rifare sulla struttura nuova** (il mio commit è stato scartato): Shift per il raggio 0 nel FILLET, CHAMFER con metodo Angle e opzione Method, CHAMFER su tutta la polilinea.
5. STRETCH: **upstream** muove punti e spline (`stretch_entity`); **nostri** leader, MLine, wipeout e tratteggi non associativi (`stretch_grips.rs`, ripiego dopo `stretch_entity`). Restano fuori tabelle, MULTILEADER, immagini.
6. ROTATE e SCALE con Copy; Undo e Group nel prompt di selezione; preselezione per LIST, ISOLATEOBJECTS, HIDEOBJECTS, LAYISO: **fatti** (nostri). CHPROP, DRAWORDER, TCASE non servivano: lo facevano già.
7. Alias dei nomi AutoCAD: **sette nostri** (GCPARALLEL, GCTANGENT, GCSYMMETRIC, GCCOLLINEAR, MTEDIT, LAYERSTATESAVE, NEWVIEW) in `autocad_names.rs`; `BSAVE` è ora un comando di upstream; ULAYERS e PDFADJUST sono di upstream e noi li abbiamo solo registrati per l'autocompletamento; 4 non fatti con motivo (§3.3). `T` apre MTEXT (scelta di Mauro; migrazione dei profili alla versione 11 degli alias).
8. ATTSYNC, BEDIT e REFEDIT: **upstream** (ATTSYNC rifà gli attributi dalla definizione con la trasformazione del blocco, come la mia correzione: verificato con `attsync_gives_a_new_attribute_the_definitions_place_and_height`; BEDIT/REFEDIT riscritti, con il write-back sugli xref). Il nostro `blockedit_target.rs` resta solo come helper per il nome della definizione di un blocco dinamico.

**Livello 2 — dati per abachi e cartigli** (ponte verso il plugin architettonico)
9. ~~Campi Area/Lunghezza su LWPOLYLINE, POLYLINE, HATCH, ELLIPSE, SPLINE, con formato e fattore di conversione nel dialogo.~~ **Fatto lato app**
    (scelta di Mauro; test in `src/entities/field_measure.rs` e `src/ui/window/field_dialog.rs`; da verificare a schermo: il dialogo Campo su una polilinea e su un tratteggio, e il testo che segue la polilinea quando si sposta un vertice).
10. DATAEXTRACTION con il nome del blocco e gli attributi come colonne, SELECTSIMILAR per nome di blocco: **fatti** (nostri; test in `src/app/annotation_data.rs`, `src/app/extraction_blocks.rs`, `src/app/dialog_flow_tests.rs`; da verificare a schermo: il wizard con attributi reali). Il conteggio per blocco è ora il `COUNT` di upstream (palette): il mio `COUNT BLOCK` è stato scartato.
11. ~~TABLEEXPORT (`table_to_csv` esiste già).~~ **Fatto** (test in `src/app/commands/table_export.rs`; da verificare a schermo: finestra di salvataggio e file aperto in Excel).

**Livello 3 — consegna delle tavole**
12. PUBLISH: un PDF per tavola, più disegni, elenco salvabile. **Fatto: un PDF per tavola dal dialogo «Print all»** (test in `src/io/publish.rs`; da verificare a schermo: casella e file scritti). **Aperti: più disegni e elenco salvabile.**
13. Fedeltà dei CTB su blocchi, xref e true color. Prima va misurata, estendendo il banco di conformità al PDF.
14. Stampa su plotter in scala 1:1: prima il raster GDI senza adattamento all'area stampabile, poi l'uscita vettoriale
    (costo alto).
15. Timbro di stampa ArchLine, Defpoints, viewport poligonali (queste ultime dopo una prova).

**Livello 4 — risorse dello studio**
16. Percorsi di supporto per `.pat`, `.lin` e SHX (per esempio la cartella Fonts di AutoCAD). Template `.dwt` dalla GUI e
    QNEW. Unità del disegno nuovo: **decisione: restano i metri** (nessuna modifica).
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
  non dipende da `ARCHLINE_ONLINE` (`src/io/font_repo.rs`). **Decisione di Mauro: resta com'è** (finestra «Missing fonts» con download solo su clic).
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
