# BIM parte 3: porte e finestre con soglia e architrave — progetto

Data: 2026-11-05. Branch previsto: `claude/bim-aperture` (da `lavoro`, **dopo** che il lavoro non
committato su `lavoro` — ancora dei locali, `src/bim/migrate.rs` — sarà committato: questa parte tocca
`rooms.rs`, `zone.rs`, `refresh.rs`, `walls.rs`). Segue la parte 1 (`2026-10-23-bim-stato-piante-design.md`,
registro R0…R69) e la parte 2 (`2026-10-28-bim-abachi-quantita-design.md`, registro R70…R95).

**Stato del documento: proposta scritta in autonomia da Claude nel loop del 2026-11-05, non rivista da
Mauro.** Ogni scelta non ricavata da una decisione già presa porta «(scelta mia)» e sta nel §9.

## 1. Contesto e scopo

Dopo le parti 1 e 2 il disegno ha zone demolite e nuove, locali e abachi. Una porta o una finestra oggi non
esiste come cosa: un vano porta nel rilievo è un buco tra le facce del muro, chiuso a mano con una linea di
soglia o con `HPGAPTOL` (parte 1 §5.2), e una demolizione che attraversa una porta conta il vano come
muratura demolita. Mancano tre cose che il computo di una ristrutturazione chiede ogni volta:

1. **Detrazione dei vani**: dal volume di un muro demolito, dalla superficie di un muro nuovo e dalla
   superficie delle pareti di un locale (intonaci, tinteggiature: il «non obiettivo» della parte 2 §2).
2. **Soglia e architrave** come elementi con la loro misura: una porta ha una soglia (lunghezza ×
   spessore del muro), un vano nuovo in un muro portante ha un architrave.
3. **Un abaco delle aperture** (porte e finestre per stato: esistente, demolita, nuova), nella forma delle
   altre tabelle, con fonte e stato di ogni misura.

Il flusso resta quello del CAD «Retino» (oggetti a intento, derivati automatici): Mauro indica **dove**
sta il vano, il modulo ricava la larghezza dal disegno, e tutto il resto (detrazioni, abaco) segue.

## 2. Obiettivi, non obiettivi, anti-obiettivi

**Obiettivi**

- Un oggetto **apertura** nel DWG (tipo, stato, posizione sull'asse del muro, larghezza dal disegno,
  altezza, davanzale, soglia, architrave), con round-trip DWG/DXF come le zone (XDATA).
- La larghezza **dal disegno** (due punti sull'asse alle spalle del vano); le altre misure **inserite**.
- Le detrazioni: volume dei vani nelle demolizioni (m³), superficie nelle nuove murature (m²), superficie
  delle pareti dei locali (m²) al netto dei vani sul loro perimetro.
- Il vano porta chiude il locale **da solo** (come un varco, parte 1 §5.3): niente linea di soglia a mano.
- Un abaco **Aperture** (`AL_ABACO`) e le sue righe nel CSV (`AL_ABACO_CSV`).
- Le aperture nei tre DWG estratti (`AL_ESTRAI`): demolite in giallo, nuove in rosso.

**Non obiettivi**

- Libreria di famiglie (infissi con telaio, ante, vetrate), simboli di apertura a norma (arco di rotazione
  dell'anta): l'apertura è un **vano con misure**, non un blocco. Il simbolo grafico è un possibile giro
  successivo.
- Prezzi, computo, import in ArchCme (come nella parte 2).
- Trasmittanza, prestazioni acustiche, rapporti aeroilluminanti (analisi: non qui).
- Aperture in muri curvi o con facce non parallele: avviso «apertura non supportata» (come il varco).
- Il 3D: la parte 5 userà l'oggetto, qui si lasciano solo i punti di aggancio (§4.4).

**Anti-obiettivi**

- Un'altezza, un davanzale o un appoggio d'architrave **di ripiego** (mai un 2,10 m «tipico»): se manca
  è `n.d.` con il motivo, e la riga non entra nei totali che lo richiedono.
- Una detrazione che sottrae un vano che il disegno non mostra o che Mauro non ha inserito.
- Modificare le entità del rilievo (resta valida la regola «originali intatti», parte 1 §5.4).

## 3. Vincoli

- Tutto in file propri sotto `src/bim/` (`aperture.rs`, `aperture_quantities.rs`, …), più comandi in
  `src/app/commands/` e pochi ganci di una riga nel codice di upstream (stesso elenco della parte 2).
- Dati nel DWG (XDATA), nessun file a parte. Nuovo `APPID` `ARCHLINE_APERTURA`, non copiato nei DWG estratti.
- Unità come le parti 1 e 2 (`INSUNITS`, `AL_UNITA`); misure inserite in unità del disegno con eco in metri.
- Nessuna chiamata di rete, nessun valore inventato, testo italiano (virgola decimale, m, m², m³).
- Aree e lunghezze con le tolleranze `tol` / `room_tol` già in uso; nessuna tolleranza nuova.

## 4. Modello

### 4.1 L'oggetto apertura

Una **apertura** è una `LWPOLYLINE` chiusa rettangolare (il vano in pianta: larghezza lungo l'asse del
muro × spessore del muro, da faccia a faccia) su `AL-APERTURE` (layer nuovo, creato con `ensure_layers`),
con il record `ARCHLINE_APERTURA`:

| Campo | Tipo | Note |
|---|---|---|
| `tipo` | `porta` / `finestra` / `portafinestra` | scelto dal comando |
| `stato` | `esistente` / `demolita` / `nuova` | come `ZoneState`, con un'aggiunta: `esistente` |
| `asse` | due `Position3D` | i due punti sull'asse alle spalle del vano; seguono MOVE/ROTATE/SCALE |
| `spessore` | `Distance` | dal disegno (facce parallele entro 1,50 m, come `rect_from_axis`) |
| `altezza` | `Real` + stato | inserita; assente = `n.d.` |
| `davanzale` | `Real` + stato | solo `finestra`; da terra al piano del davanzale; inserito |
| `soglia` | sì / no | default **sì** per `porta` e `portafinestra`, no per `finestra` (scelta mia) |
| `appoggio` | `Real` | appoggio dell'architrave per lato; vedi 4.3 |

Larghezza = distanza tra i due punti dell'asse: stato `dal disegno`. Le altre misure `inserita`. L'oggetto
non ha un'altezza ereditata: **non c'è un livello di ripiego** (a differenza dell'altezza dei locali):
un'altezza di porta non si deduce dall'altezza del locale.

Famiglia, ancora e migrazione: l'apertura segue lo schema dei locali (ancora per le copie, `renew_anchors`,
migrazione all'apertura); l'identità della famiglia è la polilinea del vano. I dettagli dipendono dal lavoro
non ancora committato su `lavoro` e vanno fissati nel piano **dopo** quel commit.

### 4.2 Geometria e locali

- La **faccia del muro** attraversata dal vano non si tocca: il rilievo resta intatto.
- Per il calcolo dei locali un'apertura `porta` o `portafinestra` con `soglia = sì` **riaggiunge i due lati
  lunghi del vano** (le due facce), come il varco (parte 1 §5.3): il vano diventa una faccia chiusa a sé, i
  due locali restano due e **l'area del vano non appartiene a nessun locale** (decisione di Mauro,
  2026-10-23, estesa alle aperture: scelta mia, §9). Una `finestra` non chiude nulla: nel rilievo il muro è
  continuo.
- Una porta **già chiusa** da una linea di soglia di Mauro non cambia il risultato: le due chiusure
  coincidono (`distinct`/tolleranza esistenti).
- Un'apertura che non cade in un muro (non trova due facce parallele) non si crea: lo chiede lo spessore,
  come `AL_DEMOLISCI` (mai indovinato).

### 4.3 Quantità

Tutte per riga di apertura, ciascuna con **fonte e stato** (`dal disegno`, `inserita`, `n.d.`):

- **Vano**: superficie = larghezza × altezza (m²), `n.d.` senza altezza. Volume del vano nel muro =
  superficie × spessore (m³), per le detrazioni.
- **Soglia**: lunghezza = larghezza (m); superficie in pianta = larghezza × spessore. Solo se `soglia = sì`.
- **Architrave**: lunghezza = larghezza + 2 × `appoggio` (m). L'appoggio è un dato del progetto
  (`ARCHLINE_PROGETTO`, voce nuova) o dell'apertura, **mai di ripiego**: senza, la lunghezza è `n.d.` con il
  motivo «appoggio non inserito». L'architrave si computa solo per le aperture `nuova` (un vano nuovo in
  muratura esistente) e per `portafinestra`/`porta` su richiesta: `architrave = sì/no` come `soglia`
  (default **no**, scelta mia: non tutti i vani lo richiedono).
- **Davanzale** (finestre): lunghezza = larghezza (+ eventuali sporgenze, fuori dal giro).

**Detrazioni** (righe «a detrarre» nel CSV, `detrai = sì`, come nel libretto di ArchCme):

1. *Demolizioni.* Una zona demolita con asse che contiene un'apertura `esistente` o `demolita` (il vano
   cade nel suo rettangolo, tolleranza `tol`): volume da detrarre = volume del vano; il volume della zona
   resta quello di parte 2 e la detrazione è una riga a parte, mai sottratta in silenzio. Senza altezza
   dell'apertura la detrazione è `n.d.` e la zona porta la nota «vano senza altezza: volume non detratto».
2. *Nuove murature.* Una zona nuova con asse che contiene un'apertura `nuova`: superficie da detrarre =
   superficie del vano.
3. *Pareti dei locali* (il «non obiettivo» della parte 2, ora risolto): superficie delle pareti =
   perimetro (anello esterno + isole) × altezza del locale − somma dei vani sul perimetro. Un'apertura è
   sul perimetro del locale se il suo asse interseca un lato dell'anello (non basta la vicinanza); un vano
   tra due locali si detrae da entrambi. Senza altezza del locale o di un'apertura incontrata: la riga
   pareti è `n.d.` con il motivo, e **non** si somma una parete parziale.

### 4.4 Punti di aggancio per il 3D (non implementati qui)

L'apertura porta tipo, larghezza, altezza, davanzale e spessore del muro: sono i dati che servono a
forare il muro in un modello 3D (parte 5). Il campo `asse` e il piano di lavoro sono già quelli delle zone.

## 5. Comportamento

### 5.1 `AL_PORTA` / `AL_FINESTRA`

`AL_PORTA` (anche `AL_PORTAFINESTRA` come opzione `[Porta/Finestra/Portafinestra]` del comando comune
`AL_APERTURA`): due punti sull'asse alle spalle del vano → il modulo trova le due facce parallele entro
1,50 m e ne ricava lo spessore; se non le trova, lo chiede. Poi chiede lo stato `[Esistente/Demolita/Nuova]`
(default **Esistente** per un rilievo, scelta mia), l'altezza (invio = lasciala `n.d.`), per una finestra il
davanzale. Eco in metri («210 cm = 2,10 m»), avviso fuori da 0,50–3,50 m per l'altezza e 0,60–3,00 m per la
larghezza (soglie mie, §9). Un passo di annullamento; layer e APPID in un passo a sé la prima volta.

### 5.2 `AL_APERTURA_DATI`

Sulle aperture selezionate: `Stato`, `Altezza`, `Davanzale`, `Soglia`, `Architrave`, `Appoggio`, `Rimuovi`.
Come `AL_ALTEZZA S`: un passo, layer bloccati saltati e contati, famiglie ambigue rifiutate.

### 5.3 Abaco e CSV

`AL_ABACO` scrive la quarta tabella **Aperture** (colonne: codice `P1`/`F1`/`PF1`, tipo, stato, larghezza,
altezza, davanzale, vano m², soglia m, architrave m, note); le tabelle dei locali ricevono la colonna
«Pareti» con il netto dei vani. `AL_ABACO_CSV`: righe `tipo = apertura` con `parti = 1`, i fattori
nell'ordine del libretto e `detrai` per le detrazioni; solo righe complete. Stesse regole di blocco `*T`,
layer bloccati e sostituzione sul posto della parte 2.

### 5.4 Estrazione

`AL_ESTRAI`: nel DWG di fatto le aperture `esistente`; nelle demolizioni e costruzioni `demolita` in giallo
(ACI 2) e `nuova` in rosso (ACI 1) con il contorno del vano; nello stato di progetto `esistente` e `nuova`.
Senza XDATA né APPID `ARCHLINE_*` (regola già in vigore). Il vano di una porta appare come contorno senza
simbolo.

## 6. Interfaccia

Menu **Tools > Ristrutturazione** e barra omonima: tre voci nuove (Porta, Finestra, Dati apertura) → dodici
comandi; icone proprie 24×24 con lo schema e la trappola dei colori della parte 1 (giallo = demolito, rosso =
nuovo, accenti del tema; niente esadecimali della lista di `recolor_semantic_svg`); scheda nel ribbon del
workspace OpenCAD Studio. Stringhe italiane, nessuna finestra nuova.

## 7. Verifica

Casi sintetici, numeri calcolati a mano, nelle due forme del rilievo (linee continue e polilinee chiuse);
mai disegni di clienti.

- **Caso A+P**: caso A con una porta di 0,90 m nel tramezzo (spessore 0,10): senza linea di soglia A e B
  restano due locali (12,00 e 11,70 m²); vano 0,90 × 0,10 = 0,09 m² in pianta non appartiene a nessuno.
- **Caso A+Pd**: con altezza 2,10: vano 0,90 × 2,10 = 1,89 m²; volume 0,189 m³; pareti di A = perimetro ×
  2,70 − vani sul perimetro (0,90 × 2,10 = 1,89 m²).
- **Caso demolizione**: demolizione del tramezzo di 3,00 m con la porta dentro: volume 3,00 × 0,10 × 2,70 =
  0,81 m³ (parte 2, D1), detrazione 0,189 m³ in una riga a parte, mai sottratta in silenzio.
- **Caso N**: muro nuovo con una finestra nuova 1,20 × 1,40 (davanzale 0,90): detrazione 1,68 m².
- **Caso n.d.**: porta senza altezza → vano, detrazione e pareti `n.d.` con il motivo; nessun numero di
  ripiego; la riga è fuori dal CSV e dai totali, con l'avviso per riga.
- **Caso fuori muro**: apertura senza due facce → lo spessore si chiede, mai indovinato.
- **Round-trip**: DXF e DWG (R2004–R2018), copia, MOVE, MIRROR (ancora e identità come i locali), apertura di un
  disegno con aperture e di uno senza.
- **Banco `bim_extract.py`**: stesso disegno sintetico, apertura aggiunta; controlli su `AL-APERTURE`, sul
  blocco `*T` della tabella Aperture e sui tre DWG estratti.

Test: `cargo test --locked --lib bim::`, `--lib app::bim_tests`, `--lib commands::bim_a`, banco BIM,
`unittest` del banco. Da vedere a schermo e in AutoCAD vero: elenco nel registro della parte 3.

## 8. Suddivisione prevista (passi piccoli, ognuno con test)

1. Modello: record XDATA, `aperture.rs`, layer e APPID, lettura/scrittura, migrazione e ancora.
2. Comando `AL_PORTA`/`AL_FINESTRA` (facce, spessore, larghezza, prompt, eco, avvisi).
3. Chiusura del locale con il vano (come il varco) e area del vano fuori dai locali.
4. `AL_APERTURA_DATI`.
5. Quantità delle aperture e detrazioni (demolizioni, nuove, pareti dei locali).
6. Tabella Aperture e colonna Pareti, CSV.
7. Estrazione nei tre DWG.
8. Menu, barra, ribbon, icone.
9. Banco BIM, documenti e registro (R96…).

## 9. Scelte mie e decisioni di Mauro

Da confermare con Mauro (nessuna è una decisione già presa):

1. **Le aperture sono un oggetto proprio** (non zone `varco`): il varco della parte 1 resta per i passaggi
   aperti demolendo muro; una porta/finestra ha misure di serramento.
2. **Il vano porta chiude il locale da solo** (soglia sì per porta e portafinestra), estendendo la decisione sui
   varchi (area del vano a nessun locale).
3. **Nessuna altezza di ripiego**, nemmeno la «standard» delle porte; appoggio dell'architrave mai di ripiego.
4. **Default**: stato `esistente`, soglia sì per le porte, architrave no.
5. **Soglie degli avvisi** (altezza 0,50–3,50 m, larghezza 0,60–3,00 m, solo avvisi).
6. **Pareti dei locali**: perimetro × altezza − vani; un'apertura conta se l'asse interseca un lato
   dell'anello.
7. **Nessun simbolo grafico** dell'anta nei DWG estratti, solo il contorno del vano.
8. **Dipendenza**: l'implementazione parte dopo il commit del lavoro sull'ancora dei locali su `lavoro`.
