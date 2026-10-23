# BIM, parte 1: stato (demolito / nuovo) e piante di ristrutturazione

Stato: pronta per l'approvazione di Mauro (risposte alle tre domande ricevute il 2026-10-23, §9). Branch `claude/bim-stato-piante`. Data: 2026-10-23.

## 1. Contesto e scopo

ArchLine deve avere una parte BIM che lavori come il CAD personale «Retino» mostrato nel video
https://www.youtube.com/watch?v=t5xkc4mNXiY: comandi a intento, derivati automatici, tavole che si compongono
da sole. Quel video è solo il riferimento di flusso (non c'è codice da riusare). Un confronto con i progetti
aperti su GitHub (ottobre 2026) ha dato un solo riferimento utile, `IPedrax/BIMCraft` (Rust, MIT/Apache-2.0),
provato in locale: nucleo e test funzionano, ma ha un modello proprio senza DWG, misura i locali sugli assi dei
muri e i suoi locali non seguono le fasi. Resta un riferimento da leggere; non si usa come dipendenza né come
programma esterno.

Mauro lavora in ristrutturazione. Parte da un **rilievo in DWG fatto di linee e polilinee**, con **due linee
per muro** (le due facce, quindi lo spessore). Gli elaborati che consegna sono: piante, prospetti, sezioni,
assonometrie, prospettive, abachi e particolari costruttivi. L'intero programma è troppo grande per una spec
sola e si divide in parti indipendenti:

1. **Piante con stato** (questa spec): zone demolite e nuove, locali con superficie netta per fase, estrazione
   in DWG 2D.
2. Abachi: locali e quantità di demolizioni e nuove murature, con fonte e stato di ogni valore.
3. Porte e finestre con soglia e architrave.
4. Sezioni e prospetti estratti in 2D.
5. Assonometrie e prospettive dal modello 3D di ArchLine.
6. Particolari costruttivi (stratigrafie e dettagli).

L'ordine è quello proposto a Mauro e accettato con «procedi». Ogni parte avrà la sua spec; questa deve solo
lasciare il modello dati pronto per le successive (§4.1: altezza, asse e spessore).

## 2. Obiettivi, non obiettivi, anti-obiettivi

**Obiettivi**

- Assegnare lo stato **demolito** o **nuovo** a parti di muro, lavorando sul rilievo esistente in linee e
  polilinee, senza ridisegnarlo.
- Vedere lo stato nel disegno di lavoro: demolizioni in **giallo**, nuove costruzioni in **rosso**, con
  **campitura piena** (decisione di Mauro).
- Calcolare i **locali** con **superficie netta** (alle facce dei muri) nello stato di fatto e nello stato di
  progetto, dove i locali seguono le demolizioni e le nuove costruzioni.
- **Estrarre in DWG 2D ordinari** gli elaborati in pianta: stato di fatto, demolizioni e costruzioni, stato di
  progetto. Il file estratto si apre in AutoCAD senza ArchLine e senza i nostri dati.

**Non obiettivi (in questa parte)**

- Porte e finestre, sezioni, prospetti, 3D, scale, tetti, abachi, quantità, IFC, particolari.
- Riconoscere i muri da raster o PDF.
- Muri parametrici che sostituiscano le linee del rilievo.
- Stati diversi da demolito e nuovo (niente «temporaneo», nessuna fase numerata). «Esistente» è l'assenza di
  zona.
- Impostazioni di colore o di layer modificabili da finestra: nel primo giro sono costanti nel codice.
- Pulizia automatica delle giunzioni con i muri adiacenti (decisione di Mauro, §5.4 punto 4: si lasciano
  com'è e si elencano).
- Un plugin: il modulo sta dentro ArchLine, in file propri.

**Anti-obiettivi** (sarebbe un fallimento anche se tutto il resto funziona)

- Modificare, spostare o cancellare le linee del rilievo dell'utente. Gli originali non si toccano mai.
- Inventare un valore: unità, altezze, superfici. Un dato mancante si chiede o si segnala, non si stima.
- Un file estratto che richieda ArchLine, o che contenga dati nostri che altri programmi vedono come errore.
- Perdere elementi in silenzio nell'estrazione: ogni elemento escluso, non tagliato o non gestito è elencato.
- Un formato privato che blocchi il lavoro dentro ArchLine.

## 3. Vincoli

- **Nessuna dipendenza da altri software** (decisione di Mauro): niente BIMCraft, niente programmi esterni da
  avviare, niente servizi in rete. Le librerie Rust pure compilate dentro ArchLine (come `glam`, `kurbo`,
  `lyon_tessellation`, già presenti) non contano come software esterno: scelta mia (§9).
- File propri sotto `src/bim/` (nome da confermare in piano), con agganci minimi al codice di upstream
  (registro comandi, registro barre, menu), per ridurre i conflitti nel merge da upstream.
- Trappole del progetto: `view_main` e `start_page_content` sono enormi, quindi niente logica lì; nei menu a
  tasto destro righe `mouse_area`, mai `button`; `RUST_MIN_STACK` a 64 MiB resta.
- Verifica **solo in locale** (CI spenta): `cargo check --locked`, `cargo test --locked --lib <filtro>`, build
  release, banco di conformità. La verifica visiva la fa Mauro su Windows.
- Disegni di prova **sintetici** nel repo (pubblico), mai disegni dei clienti.
- **Unità.** Si leggono da `INSUNITS`; sono supportati millimetri, centimetri e metri, le altre unità sono
  rifiutate con un messaggio. Se `INSUNITS` vale 0 (non specificato) il comando **chiede** l'unità e non assume.
  La risposta si salva in un record del modulo nel dizionario degli oggetti con nome del disegno
  (`ARCHLINE_PROGETTO`), **senza scrivere `INSUNITS`**, così il disegno di Mauro non cambia in silenzio e la
  domanda non si ripete a ogni avvio. Le aree si mostrano in m².
- **Curve.** La ricerca dei contorni (`boundary_faces`) lavora su segmenti: gli archi diventano corde. Il modulo
  usa una suddivisione fine (freccia massima da fissare in piano, dell'ordine del decimo di millimetro) e il
  rapporto dichiara che il contorno curvo è approssimato. Un tracciatore esatto per gli archi è fuori dal giro.

## 4. Modello

### 4.1 Zona

Una **zona** è una `HATCH` a riempimento solido, **non associativa**, nello spazio modello, con un record
XDATA dell'applicazione `ARCHLINE_ZONA` scritto con `set_entity_xdata` (che conserva i record di altre
applicazioni e registra l'APPID). Campi:

- `stato`: `demolito` | `nuovo`.
- `asse`: i due punti sull'asse del muro, **facoltativo** (c'è quando la zona nasce con «muro tra due punti»,
  §5.1). Serve alle regole di taglio (§5.4) e alla parte 2 (lunghezza).
- `spessore`: in unità del disegno, facoltativo; ricavato dalle linee o digitato. Serve alla parte 2.
- `altezza`: facoltativo, **mai riempito da noi** (serve alle parti 2, 4 e 5).
- `origine`: come è nata la zona (`muro-due-punti`, `linea-spessore`, `punto-interno`, `rettangolo`,
  `poligono`, `polilinea-esistente`), per la tracciabilità e per la parte 2 (fonte del dato).
- `varco`: sì/no, solo per le zone `demolito` (§5.3).
- `versione` del record.

Layer e colori (costanti nel primo giro): zone demolite su `AL-DEMOLIZIONI`, colore ACI 2 (giallo); zone nuove
su `AL-NUOVE`, colore ACI 1 (rosso). Uso gli indici ACI e non il colore vero così in stampa un CTB può
rimapparli. Scelta mia (§9).

Una zona è una `HATCH` comune: in AutoCAD si vede gialla o rossa anche senza ArchLine, e `CANCELLA` la elimina.

### 4.2 Locale

I locali sono di due tipi, perché nello stato di progetto la forma cambia ogni volta che cambiano le zone.

- **Locale di fatto**: una polilinea chiusa (`LWPOLYLINE`) su `AL-LOCALI` più un testo (`MTEXT`) con nome e
  superficie netta, con XDATA `ARCHLINE_LOCALE` (`nome`, `numero`, `punto` interno da cui è nato, `fase` =
  `fatto`, `origine`).
- **Locale di progetto**: nel disegno di lavoro si salva **solo il seme**: un testo con il nome sul punto
  interno, layer `AL-LOCALI-PROGETTO`, XDATA con `fase` = `progetto`. **Forma e superficie non si
  salvano**: si calcolano ogni volta dalla geometria di progetto (§5.4), all'estrazione e su richiesta. Così
  non esistono dati vecchi da aggiornare.

La superficie è l'area della regione trovata attorno al punto interno, **meno le isole** (pilastri o vani
tecnici chiusi dentro il locale, già gestiti da `resolve_hatch_rings`). Con due linee per muro quella
regione è già netta; per questo non serve nessuna correzione sullo spessore.

Il testo e la polilinea di un locale di fatto possono diventare vecchi se Mauro modifica il rilievo:
`AL_AGGIORNA` li ricalcola tutti; l'estrazione ricalcola sempre da capo e non si fida mai di ciò che è scritto.

## 5. Comportamento

Nomi dei comandi: prefisso `AL_` e verbi italiani (scelta mia, §9). Tutti funzionano anche da riga di comando,
con barra dedicata nel workspace classico.

### 5.0 Quali entità formano la geometria dei muri

Nei rilievi di Mauro i muri stanno su layer propri, separati da arredi, quote e testi (risposta del
2026-10-23), quindi spegnere i layer che disturbano è un gesto già abituale. La ricerca di locali, zone da punto
interno e geometria di progetto usa **solo `LINE`, `ARC`, `LWPOLYLINE` e `POLYLINE` dei layer acceso e non
congelati**, **escludendo i layer `AL-*`**. Testi, quote, blocchi, campiture e
immagini non contano come muri. Ne segue che ciò che Mauro vuole fuori (per esempio gli arredi) si ottiene come
in AutoCAD, spegnendo o congelando quei layer; un arredo chiuso lasciato acceso dentro un locale conta come
isola e viene sottratto dall'area. Il rapporto elenca le isole trovate in ogni locale, così si vede.
`boundary_sources_on_plane` oggi prende tutti i fili visibili: il modulo ne usa una versione filtrata per
tipo e layer, senza modificare quella di upstream. Le fixture di prova contengono arredi, testi e quote di
disturbo.

### 5.1 Creare una zona

`AL_DEMOLISCI` e `AL_NUOVO` chiedono come indicare l'area. Modo principale:

- **Muro tra due punti** (per `AL_DEMOLISCI`): si indicano due punti sull'asse del muro. Il modulo cerca le
  due linee parallele più vicine ai lati dell'asse (entro uno spessore massimo ragionevole, da fissare in piano)
  e ne ricava lo spessore; la zona è il rettangolo lungo l'asse, con le estremità perpendicolari. Se non trova
  due linee chiede lo spessore, non lo indovina.
- **Linea + spessore** (per `AL_NUOVO`): due punti sull'asse e lo spessore digitato.

Modi di ripiego (la zona non ha `asse`: valgono le regole prudenti di §5.4):

- **Punto interno**: riusa la ricerca dei contorni di HATCH/BOUNDARY con la tolleranza `HPGAPTOL`. Funziona
  quando l'area è già chiusa da linee.
- **Rettangolo** a due punti, **polilinea** di punti, **polilinea chiusa esistente** (selezionandola).

Limite da gestire, non da nascondere: in un rilievo a due linee i muri collegati formano una sola regione.
Un punto interno dentro un muro collegato ad altri non isola un muro solo. Per questo il modo principale è
l'asse.

Convalide (avviso in riga di comando, mai blocco silenzioso): zona `demolito` che non incrocia nessuna linea
del rilievo; zona `nuovo` che si sovrappone a un muro esistente non demolito; zone di segno opposto sovrapposte
(il `nuovo` si disegna sopra il `demolito`: è una ricostruzione, ed è consentita); zone dello stesso segno
sovrapposte solo in parte.

`AL_STATO` cambia lo stato di zone esistenti (demolito ↔ nuovo, `varco`, `altezza`, `spessore`).

### 5.2 Locale

`AL_LOCALE`: punto interno + nome (+ numero) + fase. In fase `fatto` trova la regione sul rilievo e scrive
polilinea e testo. In fase `progetto` calcola la geometria di progetto in memoria, mostra la superficie in riga
di comando e salva il seme col nome (§4.2). Un locale di progetto che cade dentro un muro nuovo è segnalato.

Vani porta: nel rilievo un locale è chiuso solo se qualcosa chiude il vano. Si chiude con una **linea di
soglia** (qualunque linea sui layer dei muri) o con la tolleranza `HPGAPTOL`. Il modulo non chiude i vani da solo.

### 5.3 Varco

Se si demolisce solo una parte di muro per aprire un passaggio, la regione dei due locali si unirebbe senza che
Mauro l'abbia voluto. Per una zona `demolito` con `varco = sì`, nella geometria di progetto si **riaggiungono i
due lati lunghi della zona** (quelli che coincidevano con le facce del muro e che la demolizione toglie). Il
varco diventa così una faccia chiusa a sé: i due locali restano due e **l'area del varco non appartiene a nessun
locale**; viene riportata a parte nel rapporto. Nel primo giro vale solo per un rettangolo che attraversa il
muro da faccia a faccia; varchi a L, d'angolo o più larghi dello spessore generano l'avviso «varco non
supportato». Decisione di Mauro (2026-10-23): l'area del passaggio **non** si conta in nessun locale.

### 5.4 Stato di progetto

La geometria di progetto si calcola dall'esistente, senza unioni booleane di poligoni:

1. **Tagli.** Linee e tratti di polilinea (anche con archi) **strettamente dentro** una zona `demolito` si
   tolgono. Per una zona con `asse`, si tolgono anche i tratti che stanno sui **lati lunghi** (paralleli
   all'asse), che sono le facce del muro tolto. Per una zona senza `asse` i tratti sul bordo **non** si toccano
   e un avviso dice che i confini non sono stati rimossi.
2. **Chiusure.** I lati di estremità di una zona con `asse` (perpendicolari all'asse) si aggiungono come linee
   solo se non coincidono con una linea esistente e hanno entrambi gli estremi su linee esistenti, cioè
   tagliano il muro da faccia a faccia. Un lato con un estremo nel vuoto non si aggiunge e genera l'avviso
   «taglio non chiuso». Una polilinea chiusa tagliata dalla zona diventa pezzi aperti più la linea di chiusura,
   mantenendo tipo di entità e attributi (larghezza, tipo di linea) dove possibile.
3. **Nuove zone.** Il contorno di ogni zona `nuovo` si aggiunge come linee. I segmenti coincidenti (due zone
   nuove adiacenti che condividono un lato, un nuovo costruito esattamente sul demolito) si **cancellano a
   coppie** e i segmenti collineari contigui si fondono, così non restano cuciture né linee doppie. Zone nuove
   sovrapposte solo in parte non si risolvono e generano un avviso.
4. **Giunzioni.** Il modulo **non pulisce** le giunzioni con muri adiacenti: dove un lato di estremità
   coincide con una linea esistente (per esempio un tramezzo che si appoggia a un muro) il segmento resta com'è
   e **il rapporto elenca la posizione** («giunzione da verificare»). Decisione di Mauro (2026-10-23): le
   giunzioni si lasciano com'è e si ritoccano in AutoCAD; la pulizia automatica sarebbe un lavoro a parte.
5. **Elementi puntuali** (blocchi, testi) con punto d'inserimento dentro una zona `demolito` non entrano nel
   progetto; sono elencati.
6. **Entità non tagliabili** (spline, ellissi, quote, immagini, riferimenti esterni, campiture) restano intere
   se fuori dalle zone; se ne attraversano una restano intere, con avviso «non tagliata».

I **locali di progetto** si calcolano rilanciando la ricerca dal seme di ognuno, sulla geometria di progetto.
Le regole sul nome sono meccaniche e dichiarate:

- se in una regione cade un seme di progetto, ne vale il nome;
- altrimenti, se cadono uno o più semi di fatto, il nome è l'unione meccanica dei nomi di fatto con « + »,
  marcato «da confermare» (il modulo non inventa nessun nome nuovo: unisce quelli di Mauro);
- una regione senza nessun seme esce nel DWG come polilinea con la superficie e la dicitura «(senza nome)», ed
  è elencata nel rapporto.

### 5.5 Estrazione

`AL_ESTRAI`: sceglie una cartella e i tre elaborati. Il modulo chiede una volta il **layer dei muri nuovi** per
lo stato di progetto, con proposta il layer più frequente tra le linee esistenti dei muri, e lo ricorda nel
record del modulo. Per ogni elaborato scrive un **DWG nuovo, solo 2D**:

- **Stato di fatto**: il rilievo originale, senza zone, con i locali di fatto ricalcolati.
- **Demolizioni e costruzioni**: il rilievo originale con le campiture gialle e rosse delle zone.
- **Stato di progetto**: la geometria di §5.4, con i locali di progetto di §5.4 e il loro contorno; le linee dei
  muri nuovi sul layer scelto, con colore e tipo di linea dei muri esistenti.

Perimetro: **solo spazio modello**, e solo ciò che il percorso di copia già usato da `WBLOCK`
(`extract_entities_into`, `copy_to_clipboard_kernel`) sa copiare; il resto (layout, 3D, immagini, riferimenti
esterni, blocchi con attributi non copiabili) è elencato nel rapporto. Nel DWG estratto: tabelle dei layer, tipi
di linea e stili di testo copiati dal disegno di partenza, **nessun XDATA nostro**, nessuna dipendenza da
ArchLine. I nomi dei file derivano dal disegno (`<nome>_stato-di-fatto.dwg`,
`<nome>_demolizioni-costruzioni.dwg`, `<nome>_stato-di-progetto.dwg`); un file esistente non si sovrascrive
senza conferma. La versione DWG è quella del disegno di partenza. Un rapporto finale in riga di comando elenca
tutto ciò che è stato escluso, non tagliato, approssimato o segnalato.

## 6. Interfaccia

- Barra **Ristrutturazione** nel workspace classico, registrata in `src/ui/toolbar_registry.rs`
  (`COMMAND_BARS`), con i pulsanti Demolisci, Nuovo, Stato, Locale, Aggiorna, Estrai. Un pulsante per comando.
- Gli stessi comandi da riga di comando e da menu.
- La finestra di estrazione è minima: cartella, tre caselle, layer dei muri nuovi, OK. Il resto da riga di
  comando.
- Anteprima: mentre si traccia una zona, il contorno si vede nel colore dello stato.

## 7. Verifica

Disegni sintetici nel repo, ognuno nelle **due forme** del rilievo (una forma chiusa per muro, e linee continue
che attraversano gli incroci), con arredi, testi e quote di disturbo sui loro layer.

- **Caso A, locali e demolizione totale.** Due locali affiancati con muri a due linee, in metri. Locale A
  4,00 × 3,00 m (facce interne): 12,00 m². Tramezzo da 0,10 m: 0,30 m² in pianta. Locale B 3,90 × 3,00 m:
  11,70 m². Demolito l'intero tramezzo (zona 0,10 × 3,00 m): la zona misura 0,30 m². In progetto i due locali
  diventano uno, 4,00 + 0,10 + 3,90 = 8,00 × 3,00 m = **24,00 m²** (12,00 + 11,70 + 0,30), con nome composto
  «da confermare».
- **Caso B, varco.** Stesso tramezzo, demolito per 0,90 m con `varco = sì`: zona 0,10 × 0,90 = 0,09 m². I
  locali restano due, A = 12,00 m² e B = 11,70 m²; l'area del varco, 0,09 m², è riportata a parte.
- **Caso C, unità.** Il caso A in centimetri con `INSUNITS` = cm dà le stesse aree in m². Con `INSUNITS` = 0 il
  comando chiede l'unità e non calcola finché non la ottiene; `INSUNITS` non viene mai modificato.
- **Caso D, isola.** Un pilastro chiuso di 0,30 × 0,30 m dentro il locale A: area 12,00 − 0,09 = 11,91 m². Con il
  layer del pilastro spento: 12,00 m².
- **Caso E, giunzione.** Un tramezzo che si appoggia a un muro lungo e viene demolito: il rapporto elenca la
  giunzione da verificare, la linea del muro lungo non viene rimossa.
- **Caso F, disturbo.** Testi, quote e blocchi sul locale non cambiano l'area e il risultato è lo stesso se il
  comando si rilancia dopo aver scritto il testo del locale.
- **Estrazione.** I tre DWG si rileggono (nei test con l'oracolo `ezdxf` del banco di conformità, che non legge
  il DWG: DXF dello stesso contenuto) senza XDATA nostro; le campiture sono `HATCH` solide di colore ACI 2 e
  ACI 1 sui layer `AL-DEMOLIZIONI` e `AL-NUOVE`; le linee del rilievo fuori dalle zone sono invariate al
  micron.
- **Originali intatti.** Dopo qualunque sequenza di comandi le entità del rilievo hanno lo stesso handle, la
  stessa geometria e gli stessi attributi di prima.
- **Rapporti.** Un caso con entità non tagliabile, uno con taglio non chiuso e uno con zone nuove sovrapposte in
  parte producono gli avvisi previsti.

## 8. Suddivisione prevista (PR piccole, ognuna con test)

1. Modello dati e `AL_DEMOLISCI` / `AL_NUOVO` / `AL_STATO` con zone, layer, colori, XDATA, filtro delle entità
   di §5.0, unità.
2. Estrazione «Demolizioni e costruzioni» e «Stato di fatto».
3. Geometria di progetto (§5.4) ed estrazione «Stato di progetto».
4. Locali di fatto e di progetto (§4.2, §5.2, §5.3), `AL_AGGIORNA`.
5. Barra Ristrutturazione, rapporto finale, caso nel banco di conformità.

## 9. Scelte mie e domande aperte

**Scelte mie** (non le hai dette tu; da controllare leggendo):

- librerie Rust pure ammesse dentro ArchLine;
- layer `AL-…` e colori ACI 2 e 1 fissi nel primo giro;
- prefisso `AL_` e verbi italiani;
- zona come `HATCH` non associativa, con `asse` e `spessore` nei dati per la parte 2;
- le entità che contano come muri (§5.0) e la gestione dei layer per spegnere ciò che disturba;
- i locali di progetto salvano solo il seme; forma e superficie si ricalcolano sempre;
- il nome dei locali di progetto: seme di progetto, altrimenti unione meccanica dei nomi di fatto;
- regola del varco con i due lati lunghi riaggiunti;
- nessuna unione booleana: cancellazione di segmenti coincidenti a coppie;
- unità: solo mm, cm, m; la risposta si salva in `ARCHLINE_PROGETTO` e non in `INSUNITS`;
- archi approssimati con suddivisione fine e dichiarati nel rapporto;
- layer dei muri nuovi scelto all'estrazione, con proposta;
- un DWG per elaborato, versione come il disegno di partenza, solo spazio modello.

**Decisioni di Mauro (2026-10-23)**, risposte alle domande della verifica:

1. I muri del rilievo stanno su layer propri, separati da arredi, quote e testi (§5.0).
2. L'area del varco si **esclude** dai locali (§5.3).
3. Le giunzioni con i muri adiacenti si **lasciano com'è**, con l'elenco nel rapporto (§5.4 punto 4).
