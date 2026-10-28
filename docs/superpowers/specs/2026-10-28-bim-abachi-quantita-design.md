# BIM parte 2: abachi e quantità — progetto

Data: 2026-10-28. Branch: `claude/bim-quantita` (da `lavoro`). Segue la parte 1
(`2026-10-23-bim-stato-piante-design.md`; registro delle decisioni R0…R69 in
`docs/superpowers/ledgers/`). Chiude il «Dopo» lasciato dalla parte 1: l'altezza oggi non viene
mai riempita (spec parte 1 §4.1) e le quantità non escono dal disegno.

## 1. Contesto e scopo

Mauro lavora in ristrutturazione, parte da un rilievo DWG a due linee per muro e consegna anche il
computo. Dopo la parte 1 il disegno contiene zone demolite e nuove (con asse, spessore, origine nello
XDATA) e locali di fatto e di progetto con superficie netta. Mancano le **quantità**: da zone e locali
devono uscire numeri che si possano portare nel computo, ciascuno con la **fonte** (da dove viene) e lo
**stato** (quanto è sicuro). Regola del progetto: nessun valore inventato; dato deterministico del
disegno distinto da dato inserito, ereditato o mancante.

Il computo vero e proprio (prezzario, importi) **resta fuori**: è il lavoro di ArchCme
(`C:\Users\archi\Documents\Progetti IA\archcme_py`, libretto delle misure con `parti × L × W × H`, un
fattore `None` è «non applicabile» e viene saltato nel prodotto, flag «a detrarre»). Questa parte produce
le misure nella stessa forma del libretto, così un futuro import in ArchCme è semplice. L'import in
ArchCme **non** si fa qui e non si tocca quel repo.

Programma complessivo (parte 1, §1): 1 piante con stato (fatta) · **2 abachi e quantità (questa)** · 3 porte
e finestre · 4 sezioni e prospetti · 5 assonometrie e prospettive · 6 particolari. La direzione di fondo
(deciso con Mauro nel confronto del 2026-10-28): il disegno 2D del rilievo resta la fonte di verità per
ristrutturazione e quantità; muri a intento, livelli e aperture (parte 3) sono il nucleo comune con il 3D e
arrivano dopo. Questa parte non scrive codice per il 3D, ma lascia i punti di aggancio (§4.4, §9).

## 2. Obiettivi, non obiettivi, anti-obiettivi

**Obiettivi**

- Definire l'**altezza** a tre livelli (zona o locale → predefinita del livello → nessuna), con stato per
  ogni valore.
- Calcolare le quantità di **demolizioni** (m³ principali), **nuove murature** (m² principali, raggruppate
  per spessore) e **locali** (m² netti; perimetro e volume come informazione).
- Scrivere nel DWG **tre tabelle** (Demolizioni, Nuove murature, Locali) con `AL_ABACO`, aggiornabili.
- Esportare un **CSV italiano** (`;`, virgola decimale, BOM) con le colonne del libretto delle misure di
  ArchCme, per le sole righe complete.

**Non obiettivi**

- Prezzi, importi, computo, collegamento al prezzario; import in ArchCme; formato `.xlsx`.
- Superficie delle pareti dei locali (intonaci, tinte): richiede la detrazione dei vani, parte 3.
- Detrazione di porte e finestre; abachi di solai, coperture, scale, finiture, arredi.
- Livelli come oggetti (un solo piano per disegno): l'«altezza predefinita del livello» è un valore unico
  del progetto, nome scelto per non dover migrare i dati quando i livelli esisteranno.
- Modifiche al 3D, a IFC, alle sezioni.

**Anti-obiettivi** (sarebbe un fallimento anche se «funziona»)

- Un numero senza fonte e stato, o un'altezza di ripiego (come 2,70) usata senza che Mauro l'abbia scritta.
- Una riga con altezza mancante che entra nei totali o nel CSV (in ArchCme un fattore vuoto verrebbe
  **saltato**: un volume diventerebbe in silenzio una superficie).
- Cambiare in silenzio il rilievo o i dati: l'abaco **non modifica mai** le entità del rilievo, né zone e
  locali (le scrive solo `AL_ALTEZZA`, su richiesta); scrive solo le tabelle e il record di progetto.
- Un abaco che non torna con `AL_AGGIORNA` e `AL_ESTRAI`: stessa geometria, stesse tolleranze (R61).

## 3. Vincoli

- File propri sotto `src/bim/` (logica pura testabile senza interfaccia) e agganci minimi al codice di
  upstream, come nella parte 1. Nessuna dipendenza nuova.
- Verifica solo in locale (CI spenta). Disegni di prova **sintetici** nel repo. Mauro non ha un caso reale
  da dare (risposta del 2026-10-28): i numeri si controllano su casi sintetici con quantità calcolate a
  mano (§7) e questo è un **limite dichiarato della verifica**, non una prova su un lavoro vero.
- Le unità sono quelle della parte 1 (`INSUNITS` letto, mai scritto; mm, cm, m). Le quantità escono in
  m, m², m³. Le altezze si salvano **in unità del disegno**, come `spessore` della zona (scelta mia, §9).
- **Valori digitati.** Un'altezza digitata (`AL_ALTEZZA`, `AL_STATO Altezza`) è sempre in **unità del
  disegno**, come già oggi per lo spessore: una sola regola per tutti i comandi. Il messaggio di conferma
  la ripete con l'unità («altezza 270 cm = 2,70 m»), così un errore di scala si vede subito. Fuori
  dall'intervallo 0,50–10 m c'è un avviso, non un blocco. Il caso di un disegno in cm in cui si digita
  2,7 dà quindi 2,7 cm e l'avviso.
- Numeri nelle tabelle: due decimali, virgola. I totali si calcolano sui valori **non arrotondati** e poi
  si formattano, quindi gli addendi mostrati possono non dare il totale mostrato (una nota in tabella lo
  dice). Nel CSV quattro decimali.
- Stringhe nuove in italiano; trappole del progetto invariate (niente logica in `view_main`, righe di menu
  `mouse_area`, `RUST_MIN_STACK`).

## 4. Modello

### 4.1 Altezza

Tre posti, dal più specifico al più generale; vince il primo che c'è:

1. **Zona**: campo `altezza` già presente in `ARCHLINE_ZONA`; oggi lo scrive `AL_STATO Altezza`.
2. **Locale**: nuovo campo facoltativo `altezza` nell'XDATA `ARCHLINE_LOCALE` (per un locale di fatto sul
   contorno e sul testo, per uno di progetto sul seme). Chiave aggiunta in coda: la lettura va per chiave e
   salta le sconosciute, la versione del record resta 1.
3. **Predefinita del livello**: nuova voce del record `ARCHLINE_PROGETTO` (un reale). Il disegno ha un solo
   livello; la voce sarà quella del livello quando i livelli esisteranno.

Ogni valore usato ha uno **stato**: `inserita` (scritta da Mauro su zona o locale), `ereditata` (dalla
predefinita), `n.d.` (manca). **Eccezioni, per non inventare:**

- Una zona **varco** non eredita mai: l'altezza di un'apertura (porta 2,10 o taglio fino al solaio) la dice
  solo Mauro. Senza altezza propria il varco è `n.d.`, con il motivo «altezza del varco da inserire».
- Un locale di progetto risolve l'altezza così: (a) il suo seme di progetto, se ne ha una propria; (b)
  altrimenti i locali di fatto i cui semi cadono nella sua faccia: se sono tutti risolti allo **stesso
  valore** lo prende, con lo stato di quelli (`inserita` se inserita sul fatto, `ereditata` se ereditata) e
  la nota «dal locale di fatto»; se i valori **differiscono** (fusione «A + B» con 2,70 e 3,00) è `n.d.` con
  avviso «altezze discordi» che li elenca; (c) senza locali di fatto né seme propri, la predefinita. Così il
  volume di progetto non cambia in silenzio rispetto al fatto quando nessuno ha toccato l'altezza.

### 4.2 Riga di quantità

Struttura pura (`src/bim/quantities.rs`, nome da confermare in piano). Una riga ha: `tipo`
(demolizione | nuova muratura | varco | locale di fatto | locale di progetto), `fonte` (zona con handle,
locale con nome; in futuro oggetto 3D), le grandezze con ciascuna il proprio stato, la quantità
principale con unità, e le note. Stato di una grandezza: `dal disegno` (esatta dalla geometria),
`approssimata` (archi a corde o locale chiuso solo da `HPGAPTOL`, come `ContornoApprossimato` e
`ChiusoPerTolleranza` della parte 1), `inserita`, `ereditata`, `n.d.` (con il motivo). Una riga è
**completa** se la sua quantità principale è calcolata; solo le righe complete entrano nei totali e nel CSV.

### 4.3 Che cosa si calcola

**Zone** (da `zone::zones_in`): area in pianta = anello esterno meno i buchi, in m².

| Voce | Quantità principale | Altre colonne | Condizione |
|---|---|---|---|
| Demolizione | volume m³ = area in pianta × H | lunghezza e spessore (se c'è l'asse), superficie di parete m² = lunghezza × H | il volume serve solo area e H; la parete serve l'asse |
| Varco | volume m³ = lunghezza × spessore × H | — | H solo inserita (§4.1); sempre con asse (la parte 1 lo impone) |
| Nuova muratura | superficie m² = lunghezza × H | spessore, volume m³ = area × H | serve l'asse; una zona senza asse resta in tabella con l'area in pianta e quantità `n.d.` («zona senza asse») |

**Fonte dello spessore.** La geometria (gli anelli della zona) è la fonte del numero; il campo `spessore`
dell'XDATA è solo un metadato (parte 1: `AL_STATO Spessore` non cambia gli anelli). Per demolizioni e varchi
con asse lo **spessore effettivo** è area in pianta ÷ lunghezza dell'asse, e con questo si scrivono la
colonna «spessore» e il fattore `larghezza` del CSV: il prodotto `L × W × H` è così uguale al volume anche
per zone con buchi o con l'asse accorciato (`rect_from_axis`). Se il metadato diverge dal valore effettivo
oltre `tol`, la tabella mostra quello effettivo e c'è un avviso («spessore dichiarato 0,12, misurato
0,10»); nessuna riga diventa incompleta per questo. Il volume di una nuova muratura è area × H (stessa
geometria). Il raggruppamento per spessore usa quello effettivo, al millimetro.

Le nuove murature si raggruppano per **spessore** (al millimetro) con un totale per gruppo; senza
spessore, gruppo «spessore n.d.». Demolizioni: un totale generale (volume e parete) più, a parte, il totale
dei varchi. Una zona nuova sopra una demolita (ricostruzione) dà due righe indipendenti. **Sovrapposizioni.** Agli
angoli e negli incroci a T due zone nuove si sovrappongono per forza di (spessore × spessore): è normale,
**non si corregge** e **non dà avviso**; la nota in tabella dice che lunghezze e volumi agli incroci non
sono corretti (somma delle zone come disegnate). Una sovrapposizione **sostanziale** tra zone dello stesso
stato dà invece avviso e nota sulle due righe: area comune maggiore del 25% dell'area della più piccola
(soglia mia). Vale anche per un varco dentro una zona demolita dello stesso stato (conteggio doppio,
quasi certamente un errore). Il numero non si corregge da solo.

**Locali** (stessa fonte di `AL_AGGIORNA` e `AL_ESTRAI`: fatto ricalcolato dal rilievo, mai dall'etichetta;
progetto da `project_geometry`):

- superficie netta m² (stato `dal disegno` o `approssimata`, come il dato di parte 1);
- perimetro m = lunghezza dell'anello esterno della regione (le isole non contano; con archi a corde:
  `approssimata`);
- volume m³ = superficie netta × H, `n.d.` se manca H;
- un locale «non chiuso» resta in tabella con superficie `n.d.` e la nota.

Accoppiamento fatto/progetto per il confronto: un locale di progetto è accoppiato a un locale di fatto
quando il seme di quest'ultimo cade nella sua faccia e la corrispondenza è uno a uno; solo allora c'è la
colonna «variazione». Locali fusi o divisi: righe separate con nota («da A + B», «unito in C»), nessuna
somma implicita. Il dettaglio dell'impaginazione dei casi non uno-a-uno lo fissa il piano, con la regola
che nessun numero sommato resta senza dichiarazione.

### 4.4 Punti di aggancio per il 3D (non implementati qui)

- `fonte` ammette un oggetto 3D: quando i muri diventeranno oggetti, l'altezza verrà dall'oggetto con stato
  `dal disegno` e le righe resteranno quelle.
- Colonna «detrazione vani»: presente nelle nuove murature, valore «non calcolata» fino alla parte 3, **mai
  zero**. Nel CSV la detrazione sarà una riga con «a detrarre» (flag della `Misura` di ArchCme).
- Scale e coperture saranno nuovi `tipo` di riga con le loro grandezze.

## 5. Comportamento

### 5.1 `AL_ALTEZZA`

- `AL_ALTEZZA L <valore>`: imposta l'altezza predefinita del livello nel record di progetto (come
  `AL_UNITA`, in un passo di annullamento a sé).
- `AL_ALTEZZA S <valore>`: scrive l'altezza su zone e locali selezionati; `AL_ALTEZZA R`: la toglie, e
  tornano a ereditare. Un solo passo di annullamento per la selezione. Senza argomenti chiede la
  parola chiave `[Livello/Selezione/Rimuovi]` e poi il valore. Valori ≤ 0 o non numerici sono rifiutati
  con un messaggio. `AL_STATO Altezza` per le zone resta com'è.
- Rifiuta spazio carta ed editor di blocchi, come gli altri comandi che scrivono.

### 5.2 `AL_ABACO`

Calcola le righe (§4.3) e scrive le tre tabelle come entità `TABLE` nello spazio modello, layer `AL-ABACO`
(creato dal comando se manca, come `ensure_layers`), con un XDATA `ARCHLINE_ABACO` (`tabella` =
demolizioni | nuove | locali, versione) che le riconosce. Se esistono già, si **sostituiscono** dove si
trovano (stesso punto di inserimento, un passo di annullamento). Quelle mancanti si creano: se ne manca
solo una, sotto l'ultima esistente; se mancano tutte, in un punto che si clicca, oppure indicato come
argomento (`AL_ABACO <x,y>`), oppure, da script senza argomento, a destra dell'estensione dello spazio
modello con un margine pari alla larghezza della tabella. Mai una copia nuova a ogni lancio.

**Fattibilità della `TABLE`** (da provare per prima nel passo 4 del piano, non da dare per scontata): si
crea una `TABLE` con celle e contenuto come fa `table_cmd.rs`, con XDATA, si salva in DWG e si rilegge
(il contenuto delle celle e l'XDATA devono tornare); poi Mauro la apre in AutoCAD vero. Se la prova fallisce
(contenuto non scritto, XDATA perso, DWG che AutoCAD segnala), **ripiego**: linee e `MTEXT` raggruppati, con
lo stesso XDATA su ogni parte per riconoscerli e sostituirli. Il ripiego è lo stesso meccanismo della §5.4. Dopo il comando, in riga di comando: quante righe, quante
incomplete e perché, e gli avvisi (sovrapposizioni, altezze discordi, locali non chiusi, approssimazioni).
Se non ci sono zone né locali, nessuna tabella e un messaggio. Il rilievo non si tocca.

Aspetto (mockup approvato, opzione A del 2026-10-28): intestazione con tipo e unità principale; colonne
della §4.3; `n.d.` evidenziato; stato dell'altezza in ogni riga; totale in fondo a ciascuna tabella con la
dicitura «righe complete» e il numero di righe escluse; nota sull'arrotondamento. Colori e stile di
testo seguono lo stile tabella corrente del disegno (scelta mia, §9).

### 5.3 `AL_ABACO_CSV`

`AL_ABACO_CSV [percorso]`. Una riga per misura, **solo righe complete**, colonne:

`tipo;codice;descrizione;parti;lunghezza;larghezza;altezza;detrai;quantita;um;stato_altezza;fonte;note`

- `codice` è il numero progressivo della riga per tipo (D1, V1, N1, LF1, LP1), nell'ordine degli handle, lo
  stesso della prima colonna delle tabelle; `descrizione` = nome del locale, oppure «Demolizione muro
  spessore 0,100», «Varco», «Nuova muratura spessore 0,080». **Il formato non è vincolato a un importatore
  di ArchCme, che non esiste:** le colonne sono nostre, ricalcate sulla `Misura`. Il file mescola unità
  (m³, m², m²) e ha `um` per distinguerle; quando l'import in ArchCme si scriverà, il formato si
  riallinea a quello (e questa spec si aggiorna).
- `parti`, `lunghezza`, `larghezza`, `altezza`, `detrai` seguono la `Misura` di ArchCme: demolizione con
  asse = 1 × L × spessore effettivo × H (m³); varco = 1 × L × spessore effettivo × H (m³); nuova muratura = 1 × L × H (m²,
  `larghezza` vuota = non applicabile, valido solo qui perché la quantità è una superficie); locale = area
  netta come `parti = 1`, `lunghezza` = area, un fattore (m²). **Demolizione senza asse:** `lunghezza` =
  area in pianta e la `descrizione` dice «area in pianta (zona senza asse)»; il prodotto resta area × H.
  (scelta mia, §9)
- Separatore `;`, virgola decimale, quattro decimali, BOM UTF-8, fine riga CRLF: si apre in Excel italiano.
  (Il `TABLEEXPORT` di ArchLine scrive CSV con virgola e senza BOM: non si cambia.)
- Le righe incomplete **non** vanno nel file; ne resta traccia in riga di comando e nella tabella del DWG.
  Un'altezza mancante non diventa mai un campo vuoto.
- Senza percorso: finestra «Salva con nome» del sistema; da script e riga di comando il percorso è
  obbligatorio. **Non si sovrascrive mai** un file esistente senza conferma (la finestra di sistema chiede;
  da script, con file esistente, non si scrive nulla). Errori di scrittura: messaggio, nessun file a metà.

### 5.4 Estrazione

`AL_ESTRAI` oggi esclude le `TABLE` dai tre DWG e le elenca come `NonCopiata`. Per le tabelle
riconosciute dall'XDATA `ARCHLINE_ABACO` questo avviso non compare più (non sono una perdita). Se il disegno
di lavoro ha tabelle d'abaco, nel solo DWG **demolizioni e costruzioni** si scrivono le tre tabelle
**ricalcolate al momento dell'estrazione** (come i locali: l'estratto non contiene un numero che
`AL_ABACO` non darebbe adesso) e convertite in linee e testi ordinari, nello stesso punto del disegno di
lavoro, senza XDATA. Non si copia mai una tabella vecchia. Questo passo è l'ultimo del piano e
separabile (§8): se Mauro preferisce, si può rinviare e restare con il solo silenzio sull'avviso.

## 6. Interfaccia

- Sottomenu **Strumenti > Ristrutturazione** e barra omonima (generata dal registro): tre voci nuove,
  Altezza, Abaco, Abaco CSV. Icone SVG proprie da disegnare con lo schema delle sei esistenti (24×24,
  tabella `RIBBON` di `src/ui/menu_icons.rs`, nessun esadecimale della lista fissa di
  `recolor_semantic_svg`, niente `white`; test di colore e di pixel). Chi ha una disposizione salvata
  vede la barra nuova riselezionando il workspace.
- Nessuna finestra nuova: tutto da riga di comando, finestra di sistema solo per «Salva con nome».

## 7. Verifica

Casi sintetici nel repo (le quantità si ricalcolano a mano e stanno scritte nei test; fixture di parte 1
riusate, con in più altezze). Metri; altezza predefinita del livello 2,70.

- **Caso A, demolizione totale** (tramezzo 0,10 × 3,00, area in pianta 0,30 m²): volume 0,30 × 2,70 =
  **0,81 m³**; lunghezza 3,00; parete 3,00 × 2,70 = **8,10 m²**; altezza `ereditata`. I locali: A 12,00 m²,
  perimetro 14,00 m, volume 32,40 m³; B 11,70 m², perimetro 13,80 m, volume 31,59 m³. Progetto: locale
  unico 24,00 m² (nome «da confermare»).
- **Caso A', altezza inserita**: stessa zona con altezza 3,00 `inserita`: volume **0,90 m³**, parete 9,00 m².
- **Caso B, varco** (0,10 × 0,90, `varco = sì`): senza altezza propria il varco è `n.d.` (motivo
  «altezza del varco da inserire»), anche con la predefinita impostata; con altezza 2,10 `inserita`:
  0,90 × 0,10 × 2,10 = **0,189 m³** (0,19 in tabella). Fuori dai totali delle demolizioni, nel totale dei
  varchi.
- **Caso G, nuova muratura** (asse 2,00 m, spessore 0,08, altezza ereditata 2,70): superficie
  2,00 × 2,70 = **5,40 m²**, gruppo «spessore 0,080», area in pianta 0,16 m², volume 0,432 m³; detrazione
  vani «non calcolata». Due zone nuove stesso spessore: un gruppo, somma esatta. Zona nuova senza asse:
  riga con area in pianta e quantità `n.d.`.
- **Caso H, senza altezza** (nessuna predefinita, nessuna sulla zona): riga `n.d.`, fuori dai totali, fuori
  dal CSV, nota «da completare»; con la predefinita impostata dopo, `AL_ABACO` aggiorna e la riga entra.
- **Caso I, locali**: altezza sul locale di fatto che vince sulla predefinita; locale di progetto fuso da
  due locali di fatto con altezze 2,70 e 3,00 → volume `n.d.` e avviso «altezze discordi»; locale non
  chiuso → superficie `n.d.`; locale chiuso da `HPGAPTOL` → stato `approssimata`.
- **Caso J, unità**: caso A in centimetri con `INSUNITS` = cm dà le stesse quantità, con altezza digitata
  `270` (unità del disegno, §3); digitando `2,7` si ha l'avviso di intervallo e il messaggio «2,7 cm =
  0,03 m». `AL_STATO Altezza` e `AL_ALTEZZA` leggono lo stesso numero allo stesso modo.
- **Caso K, spessore e sovrapposizioni**: zona con metadato `spessore` 0,12 ma anelli di 0,10: tabella e CSV
  usano 0,10 con avviso, e `L × W × H` = volume. Due zone nuove ortogonali che si incontrano ad angolo: nessun
  avviso, nota sugli incroci. Due zone nuove sovrapposte per più del 25%: avviso. Un varco dentro una
  demolizione: avviso.
- **Caso L, altezza del locale di progetto**: fatto con altezza inserita 3,00 e locale di progetto senza
  seme proprio: volume di progetto con 3,00, stato `inserita` e nota «dal locale di fatto»; non 2,70.
- **Script**: `AL_ABACO` senza argomento da script mette le tabelle a destra dell'estensione; con una
  tabella cancellata ricrea solo quella, sotto l'ultima esistente.
- **Tabelle**: `AL_ABACO` due volte non crea copie; dopo una modifica del rilievo la seconda esecuzione
  sostituisce le tabelle; annullando si torna indietro in un passo; il rilievo ha stessi handle, geometria
  e attributi (come in parte 1).
- **CSV**: separatore, virgola, BOM e CRLF; nessuna riga incompleta; il prodotto
  `parti × L × W × H` delle colonne `Misura` uguale a `quantita` per ogni riga; file esistente non
  sovrascritto da script.
- **Coerenza**: per i locali, area e nome di `AL_ABACO` uguali a quelli di `AL_AGGIORNA` e di
  `AL_ESTRAI` sullo stesso disegno.
- **Estrazione**: nessun avviso `NonCopiata` per le tabelle d'abaco; nel DWG demolizioni e costruzioni le
  tabelle sono linee e testi senza XDATA e coi numeri di una `AL_ABACO` fresca.
- **Banco** (`tests/conformance`): il CSV si legge con `csv` standard; la tabella si controlla con
  `ezdxf` se la libreria la espone, altrimenti solo il CSV (limite dichiarato).
- **Limite della verifica**: nessun lavoro reale di Mauro; il banco è sintetico.

Test del progetto da eseguire: `cargo test --locked --lib bim::`, `--lib app::bim_tests`,
`python tests/conformance/bim_extract.py target\release\ArchLine.exe`, `cargo test --locked --lib` intero.

## 8. Suddivisione prevista (passi piccoli, ognuno con test)

1. Altezza: voce nel record di progetto, campo nel locale, risoluzione a tre livelli con stati (pura),
   `AL_ALTEZZA`.
2. Quantità delle zone: righe, gruppi per spessore, varchi, avvisi di sovrapposizione (pura).
3. Quantità dei locali: perimetro, volume, accoppiamento fatto/progetto, stati (pura, sulle stesse
   istantanee di `AL_AGGIORNA`).
4. `AL_ABACO`: tabelle nel DWG, layer, XDATA, sostituzione, riga di comando.
5. `AL_ABACO_CSV`.
6. Menu, barra, tre icone, caso nel banco BIM.
7. Estrazione (§5.4), separabile.

## 9. Scelte mie e decisioni di Mauro

**Decisioni di Mauro (2026-10-28):**

1. Altezza a tre livelli: zona o locale → predefinita del livello → nessuna, `n.d.` quando manca, mai un
   valore di ripiego.
2. Uscita doppia: tre tabelle nel DWG (layout A: una per tipo) e CSV piatto per Excel italiano.
3. Perimetro: solo quantità; computo e prezzario fuori. Demolizioni in m³, nuove murature in m².
4. Il 2D del rilievo resta la fonte di verità; il 3D (muri a intento, livelli, aperture) viene dopo e
   condivide nucleo e righe. Nessun caso reale da fornire.

**Scelte mie** (da controllare leggendo):

- varco **non eredita** l'altezza (§4.1): un'apertura non ha l'altezza del muro;
- fusione di locali: altezza solo se concorde, altrimenti `n.d.` con avviso;
- altezze digitate e salvate in unità del disegno, come `spessore`, con eco in metri e avviso fuori da
  0,50–10 m;
- spessore effettivo = area ÷ lunghezza (la geometria vince sul metadato, con avviso se divergono);
- sovrapposizioni: nessuna correzione agli angoli e agli incroci a T (dichiarato in nota); avviso solo oltre
  il 25% dell'area della zona più piccola;
- altezza di un locale di progetto: dal suo seme, altrimenti dai locali di fatto concordi, altrimenti la
  predefinita (§4.1);
- `codice` e `descrizione` del CSV (§5.3); formato non vincolato a ArchCme finché l'importatore non esiste;
- punto di inserimento delle tabelle (§5.2) e prova di fattibilità della `TABLE` con ripiego su linee e testi;
- campo `altezza` in `ARCHLINE_LOCALE` e voce nel record di progetto, aggiunti in modo compatibile in lettura;
- nomi `AL_ALTEZZA`, `AL_ABACO`, `AL_ABACO_CSV` e le parole chiave `L/S/R`; layer `AL-ABACO`; XDATA `ARCHLINE_ABACO`;
- sostituzione delle tabelle sul posto invece di una copia nuova a ogni lancio;
- nuove murature senza asse: quantità `n.d.` (nessuna lunghezza ricavata dall'area);
- demolizione senza asse nel CSV: `lunghezza` = area in pianta, dichiarato nella descrizione;
- perimetro = anello esterno, isole escluse;
- totali sui valori non arrotondati; due decimali nelle tabelle, quattro nel CSV;
- `TABLE` nello stile tabella corrente del disegno;
- estrazione: tabelle ricalcolate e convertite in linee e testi solo nel DWG demolizioni e costruzioni,
  separabile dal resto (§5.4).
