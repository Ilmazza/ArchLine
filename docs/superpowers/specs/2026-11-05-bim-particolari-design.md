# BIM parte 6: stratigrafie e particolari costruttivi — progetto

Data: 2026-11-05. Branch: `claude/bim-particolari` (da `claude/bim-3d`, cioè sopra le parti 3, 4 e 5 non ancora
unite a `lavoro`). Segue le parti 1 (stato e piante), 2 (abachi), 3 (porte e finestre), 4 (sezioni e prospetti) e
5 (modello 3D).

**Stato del documento: proposta scritta in autonomia da Claude nel loop del 2026-11-05, non rivista da Mauro.**
Le scelte non ricavate da decisioni già prese portano «(scelta mia)» e stanno nel §9.

## 1. Contesto e scopo

Le parti 1–5 dicono *dove* e *quanto* si demolisce e si costruisce; non dicono *di che cosa* è fatto ciò che si
costruisce. Nel computo di una ristrutturazione la nuova muratura e il nuovo pavimento sono pacchetti di strati
(intonaco, laterizio, massetto, isolante, finitura), e la tavola dei particolari costruttivi li disegna. Questa
parte aggiunge:

- le **stratigrafie** del disegno: pacchetti con nome, ciascuno di tipo *parete* o *solaio*, fatti di strati
  (materiale e spessore) **inseriti da chi progetta**;
- l'**assegnazione** di una stratigrafia a una nuova muratura (zona `AL_NUOVO`) o a un locale di progetto;
- il **particolare costruttivo** di ogni stratigrafia, disegnato in 2D nello spazio modello (strati, richiami con
  materiale e spessore, spessore totale);
- le **quantità per strato** nell'abaco e nel CSV, con la fonte e lo stato della riga a cui la stratigrafia è
  assegnata.

Regola del progetto, qui come prima: **nessun valore inventato**. Non c'è un catalogo di stratigrafie con spessori
«tipici»: ogni spessore lo scrive chi progetta. Un pacchetto che non torna con il disegno (somma degli spessori
diversa dallo spessore della muratura) si segnala, non si corregge.

## 2. Obiettivi, non obiettivi, anti-obiettivi

**Obiettivi**

- `AL_STRATIGRAFIA`: crea, elenca, rimuove le stratigrafie del disegno e le assegna.
- `AL_PARTICOLARE`: disegna i particolari di tutte le stratigrafie, li sostituisce a ogni lancio.
- Abaco: una tabella **Strati** e righe `strato parete` / `strato solaio` nel CSV.
- Estrazione: i particolari nei DWG di progetto e di demolizioni e costruzioni, non nel fatto (scelta mia).

**Non obiettivi**

- Retini dei materiali nei particolari (la scala di un retino è una scelta grafica che qui non ha una fonte: primo
  giro solo contorni e richiami, scelta mia).
- Catalogo di materiali o di stratigrafie, prezzi, trasmittanza, verifiche termoigrometriche.
- Stratigrafie delle demolizioni (che cosa si toglie di un solaio) e dei locali di fatto.
- Particolari di nodi (attacco a terra, davanzale, architrave): solo i pacchetti piani.

**Anti-obiettivi**

- Spessori di ripiego, materiali proposti, un pacchetto «standard» che compare da solo.
- Cambiare la geometria della pianta per farla tornare con il pacchetto.

## 3. Modello

### 3.1 Stratigrafie nel disegno

Un XRecord `ARCHLINE_STRATIGRAFIE` nel dizionario radice (come `ARCHLINE_PROGETTO`), con tutte le stratigrafie in
sequenza: 90 (Int32) versione; per ogni stratigrafia 1 (String) nome, 70 (Int16) tipo (0 parete, 1 solaio); per
ogni strato 3 (String) materiale e 40 (Double) spessore in **unità del disegno** (come altezze e spessori delle
parti 1–3). La lettura va per codice e salta ciò che non conosce. Nomi unici senza distinzione di maiuscole, non
vuoti; materiali non vuoti; spessori finiti e positivi. Per una parete gli strati vanno da una faccia all'altra,
per un solaio dall'alto (finitura) in basso.

### 3.2 Assegnazione

Un record XDATA a sé, APPID `ARCHLINE_STRATIGRAFIA` (versione, nome della stratigrafia), sull'entità che porta i
dati dell'elemento: la campitura della **zona nuova** per una parete, il **seme** (`MTEXT` su
`AL-LOCALI-PROGETTO`) del **locale di progetto** per un solaio. Un record separato non tocca `ARCHLINE_ZONA` né
`ARCHLINE_LOCALE`: `AL_STATO`, `AL_ALTEZZA` e la migrazione restano come sono, e una copia della zona o del seme
porta con sé l'assegnazione.

### 3.3 Controlli

- Parete su zona nuova: la **somma degli spessori** si confronta con lo **spessore effettivo** della muratura
  (area ÷ asse, parte 2); se differiscono oltre `tol`, avviso `StratigrafiaDiversa` (codice, somma, spessore).
  Senza asse lo spessore effettivo non c'è e il confronto non si fa (lo dice l'avviso della riga N).
- Assegnazione a una stratigrafia che non esiste più: `StratigrafiaMancante`, nessuna riga di strato.
- Tipo sbagliato (una parete su un locale, un solaio su una zona) si rifiuta all'assegnazione; letto da un
  disegno modificato altrove dà `StratigrafiaTipoDiverso` e nessuna riga.

## 4. Particolare costruttivo

Un particolare per stratigrafia, in scala 1:1 nello spazio modello su `AL-PARTICOLARI`, con il record
`ARCHLINE_PARTICOLARE` (nome della stratigrafia) su ogni entità:

- **parete**: strati come bande verticali affiancate (larghe quanto lo spessore), alte 0,50 m (scelta mia);
- **solaio**: strati come bande orizzontali impilate dall'alto, lunghe 0,50 m;
- ogni banda è una `LWPOLYLINE` chiusa; un **richiamo** (`LINE`) dal centro della banda a una colonna di testi a
  destra; il testo «n. Materiale — sp. X,X cm» (`MTEXT`), in ordine di strato;
- un titolo sopra: «Particolare <nome> — parete|solaio, spessore totale X,X cm»;
- testi alti 0,025 m (2,5 mm in una stampa 1:10, scelta mia), richiami a passo di 1,6 altezze.

I particolari stanno affiancati in X con 0,50 m di stacco, la prima volta a destra dell'estensione dello spazio
modello (5 m di stacco), poi all'angolo di quelli di prima (come sezioni e modelli 3D). Ogni `AL_PARTICOLARE`
cancella e rifà tutti i particolari, in un passo di annullamento; layer e APPID in un passo a sé.

## 5. Comportamento

### 5.1 `AL_STRATIGRAFIA`

Prende la riga intera (come `AL_ABACO_CSV`: i nomi dei materiali hanno spazi), con le opzioni:

- `Nuova <nome> <Parete|Solaio> <materiale> <spessore>; <materiale> <spessore>; …` — crea; l'ultimo token di ogni
  strato è lo spessore in unità del disegno (punto o virgola decimale); l'eco riporta ogni strato in cm. Un nome
  già usato si rifiuta (per cambiarla: `Rimuovi` e di nuovo `Nuova`).
- `Elenco` — elenca le stratigrafie con strati, spessore totale e quante assegnazioni hanno.
- `Rimuovi <nome>` — la toglie; le assegnazioni restano e danno `StratigrafiaMancante` finché non si riassegnano.
- `Assegna <nome> [x,y]` — assegna alla selezione (zone nuove o semi di progetto) o, con un punto, alla zona nuova
  che lo contiene oppure al locale di progetto la cui regione lo contiene. Un elemento di tipo sbagliato si salta e
  si conta. Un passo di annullamento; layer bloccati saltati e contati.
- `Togli [x,y]` — toglie l'assegnazione (selezione o punto).

Unità: `AL_UNITA` se manca. Scrittura del record in un passo a sé, come `AL_ALTEZZA L`.

### 5.2 `AL_PARTICOLARE`

Spazio modello, unità; senza stratigrafie lo dice e non cambia nulla; altrimenti posa e sostituisce (§4).

### 5.3 Quantità

Per ogni elemento con stratigrafia valida, una riga per strato, codice `<codice dell'elemento>-S<n>` (N1-S1,
LP1-S2…), descrizione = materiale, unità m², quantità = quella della riga dell'elemento (superficie della nuova
muratura L × H, superficie netta del locale di progetto), **con gli stessi fattori e lo stesso stato**; nella
tabella anche lo spessore dello strato e il volume m² × spessore (solo informativo). Una riga dell'elemento
incompleta dà righe di strato incomplete (fuori dal CSV, `n.d.` con lo stesso motivo).

CSV: tipo `strato parete` o `strato solaio`, fattori dell'elemento, nota «sp. X,X cm».

### 5.4 Estrazione

I particolari (record `ARCHLINE_PARTICOLARE`) vanno nei DWG di progetto e di demolizioni e costruzioni, non nel
fatto; l'XRecord `ARCHLINE_STRATIGRAFIE` e gli XDATA non escono (come ogni `ARCHLINE_*`). La tabella Strati
dell'abaco segue le regole delle altre tabelle d'abaco (parte 2, R91).

## 6. Interfaccia

Menu **Tools > Ristrutturazione**, barra e ribbon: **Stratigrafia** e **Particolare** (diciotto comandi), con icone
proprie 24×24 (stesso schema dei colori). Nessuna finestra nel primo giro (scelta mia).

## 7. Verifica

Casi sintetici, numeri a mano.

- Caso A con un tramezzo nuovo `AL_NUOVO 2,0 2,3 0.12` (asse 3,00 m, spessore 0,12) e la stratigrafia «T12»
  (intonaco 0,015, laterizio forato 0,09, intonaco 0,015: somma 0,12), altezza 2,70: tre righe N1-S1…S3 da
  3,00 × 2,70 = 8,10 m² ciascuna, nessun avviso; con laterizio 0,10 la somma è 0,13 e c'è `StratigrafiaDiversa`.
- Un locale di progetto con «P1» (ceramica 0,01, massetto 0,05): due righe con la superficie netta del locale.
- Particolare della parete: tre bande larghe 0,015 / 0,09 / 0,015 e alte 0,50; tre richiami e tre testi; titolo
  con «12,0 cm».
- Round-trip DXF e DWG del record e delle assegnazioni; nessun `ARCHLINE_*` negli estratti.

Test: `cargo test --locked --lib bim::`, `--lib app::bim_tests`, banco BIM con un sesto giro, `unittest` del banco.

## 8. Suddivisione prevista

1. Modello delle stratigrafie e XRecord (puro).
2. Assegnazione: record, lettura, controlli (puro).
3. `AL_STRATIGRAFIA` Nuova/Elenco/Rimuovi (lato app, con il gancio della riga intera).
4. `AL_STRATIGRAFIA` Assegna/Togli.
5. Particolare: disegno (puro) e posa.
6. `AL_PARTICOLARE` (lato app).
7. Quantità: righe di strato, tabella Strati, CSV.
8. Estrazione, menu, barra, ribbon, icone.
9. Banco, documenti, registro.

## 9. Scelte mie e decisioni di Mauro

Da confermare con Mauro:

1. **Nessun catalogo**: stratigrafie solo inserite; nessun materiale né spessore proposto.
2. **Spessori in unità del disegno** (come le altezze), eco in cm.
3. **Parete solo su zone nuove, solaio solo su locali di progetto**: niente stratigrafie delle demolizioni né dei
   locali di fatto.
4. **Particolare senza retini**, bande alte/lunghe 0,50 m, testi 0,025 m (stampa 1:10).
5. **Quantità per strato = quantità dell'elemento** (m²), volume solo informativo.
6. **Particolari nei DWG di progetto e di demolizioni e costruzioni**, non nel fatto.
7. **Riga di comando, nessuna finestra**; modifica = Rimuovi + Nuova.
8. **Dipendenza**: sopra le parti 3, 4 e 5, non ancora unite.
