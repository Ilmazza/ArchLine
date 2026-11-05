# BIM parte 5: modello 3D, assonometrie e prospettive — progetto

Data: 2026-11-05. Branch: `claude/bim-3d` (da `claude/bim-sezioni`, cioè sopra le parti 3 e 4 non ancora unite a
`lavoro`). Segue le parti 1 (stato e piante), 2 (abachi), 3 (porte e finestre,
`2026-11-05-bim-porte-finestre-design.md`) e 4 (sezioni e prospetti, `2026-11-05-bim-sezioni-prospetti-design.md`).

**Stato del documento: proposta scritta in autonomia da Claude nel loop del 2026-11-05, non rivista da Mauro.**
Le scelte non ricavate da decisioni già prese portano «(scelta mia)» e stanno nel §9.

## 1. Contesto e scopo

ArchLine ha già un visualizzatore 3D (camera ortogonale e prospettica, stili visivi, viste con nome, `Face3D`),
ma il modello BIM è tutto 2D: zone, locali e aperture con altezze, senza solidi. Per la tavola di Mauro servono
**assonometrie** e **prospettive** dello stato di fatto e di progetto: qui il modo più corto e verificabile è
**generare il modello 3D dalla pianta** con le altezze che il disegno già ha, e lasciarlo guardare con gli
strumenti 3D che ArchLine ha. Le immagini finali (render, stampa) restano al visualizzatore e alla stampa
esistenti, non si reinventano qui.

Regola del progetto, qui come prima: **nessun valore inventato**. Una parte senza altezza non si estrude e si
elenca. Il modello è uno **spaccato**: i muri arrivano alla loro altezza e non c'è un tetto, perché la
copertura non esiste nel modello (parte 4, §2).

## 2. Obiettivi, non obiettivi, anti-obiettivi

**Obiettivi**

- `AL_MODELLO3D`: genera nello spazio modello due modelli 3D, **stato di fatto** e **stato di progetto**, come
  `3DFACE` su layer propri, accanto alla pianta, ognuno con le sue altezze e le sue aperture.
- Muri come prismi dalla massa muraria della pianta, con i **vani delle aperture** (porte da terra, finestre dal
  davanzale) davvero aperti; pavimento di ogni locale a quota 0.
- Le **viste con nome** «Assonometria SE», «Assonometria SO», «Assonometria NE», «Assonometria NO» e
  «Prospettiva» già pronte sul modello, usabili con `VIEW`.
- Sostituzione a ogni lancio (come `AL_ABACO`, `AL_SEZIONE`): rigenerare cancella e rifà i soli oggetti nostri.
- I due modelli nei DWG estratti (`AL_ESTRAI`): il fatto nel DWG di fatto, il progetto in quello di progetto.

**Non obiettivi**

- Tetti, solai, scale, balconi, finiture, materiali, ombre, render fotorealistici.
- Demolizioni e costruzioni in 3D (richiederebbe differenze booleane di poligoni: la parte 1 §5.4 le esclude).
- Una nuova pipeline di rendering: si usano il visualizzatore, gli stili visivi e la stampa di ArchLine.
- IFC o altri formati 3D.

**Anti-obiettivi**

- Estrudere una massa con un'altezza di ripiego, o disegnare un vano di cui non si conosce la quota.
- Modificare le entità del rilievo (resta valida la regola «originali intatti»).
- Un modello 3D che sembri completo: ogni modello porta un testo dei **limiti** (spaccato, niente tetto né solai).

## 3. Vincoli

- File propri sotto `src/bim/` (`mass3d.rs`, `model3d.rs`, …), comandi in `src/app/commands/`, nessun gancio
  nuovo in upstream previsto.
- Solo `3DFACE` (triangoli e quadrilateri piani): il formato che DXF e DWG portano senza perdite e che ezdxf
  legge, quindi verificabile dal banco. Nessun `MESH` né `3DSOLID` (non provati nel round-trip del codec).
- Geometria 2D di partenza = quella delle parti 1–4 (`walls::faces`, regioni, contorni dei locali), alle stesse
  tolleranze; triangolazione con `kernel::geom2d::triangulate_rings`.
- Unità del disegno (`INSUNITS`, `AL_UNITA`): le quote in metri × unità per metro, come le sezioni.
- Nessuna chiamata di rete, testo italiano.

## 4. Modello

### 4.1 La massa muraria come prismi

Le **facce** (cicli del grafo planare dei muri, `walls::faces`) si classificano come nelle sezioni:

- **locale**: la faccia coincide con il contorno di un locale misurato → vuoto; ha il suo pavimento;
- **vano**: la faccia è il rettangolo di un'apertura → vuoto con la sua testa e il suo davanzale (4.2);
- **massa**: ogni altra faccia → prisma da quota 0 alla sua altezza.

Perché i vani siano facce chiuse in entrambe le forme del rilievo, al grafo si aggiungono **tutti e quattro i
lati** del rettangolo di ogni apertura della fase (non solo i due lunghi come per i locali); le spallette di una
porta diventano così lati dei prismi accanto.

Il prisma di una faccia ha: il **coperchio** a quota `H` (triangolazione dell'anello esterno meno i figli diretti),
e per ogni lato di ogni anello (anche dei buchi) un **quadrilatero verticale** da 0 a `H`. Niente fondo.

Altezza `H` della massa: il massimo delle altezze dei locali il cui contorno tocca la faccia (stessa regola
della sezione, R112); senza locali accanto, o con un'altezza che manca, la massa **non si estrude** e un avviso
`ModelloEscluso` la elenca.

### 4.2 Aperture

Un vano (faccia-apertura) è vuoto per la sua altezza: sopra, la **testa** è un prisma da `bottom + h` a `H`
(coperchio a `H`, lati verticali); sotto, per una finestra, il **davanzale** è un prisma da 0 a `bottom` con il
coperchio a `bottom`. Porte e portefinestre partono da terra (`bottom = 0`, nessun davanzale). Le spallette sono
i lati dei prismi vicini (i buchi degli anelli della massa). Un'apertura senza altezza o senza davanzale
(finestra) **non si apre**: la massa resta piena e un avviso lo dice.

### 4.3 Pavimenti

Per ogni locale misurato del modello, il suo contorno (anello esterno meno le isole) triangolato a quota 0.
Colore diverso dalla massa.

### 4.4 Fasi

- **Stato di fatto**: muri del rilievo con le soglie delle porte esistenti e da demolire; locali di fatto;
  aperture esistenti e da demolire.
- **Stato di progetto**: geometria di progetto (`project::project_geometry`); locali di progetto; aperture
  esistenti e nuove (R116: le esistenti dentro una zona demolita non ci sono).

### 4.5 Oggetti nel disegno

Ogni `3DFACE` generata porta un record `ARCHLINE_MODELLO3D` (`Integer16(1)`, `"fase"`, `"fatto|progetto"`) e sta
sul layer `AL-3D-FATTO` o `AL-3D-PROGETTO`. Colori: massa grigio (ACI 8), pavimenti ACI 9, teste e davanzali
come la massa. I modelli stanno **accanto alla pianta** (a destra dell'estensione, con le sezioni e i prospetti
già posati ancora a sinistra), l'uno sopra l'altro non serve: sono due insiemi affiancati in X.

## 5. Comportamento

### 5.1 `AL_MODELLO3D`

Spazio modello, unità. Calcola i due modelli, li posa (primo lancio: a destra di tutto ciò che c'è; poi dove
sono), sostituisce quelli di prima in un passo di annullamento, riporta in riga di comando: n. di prismi, n.
di facce, esclusi (massa senza altezza, aperture senza dati). Layer e APPID in un passo a sé la prima volta.
Senza locali né altezze dice che non c'è nulla da estrudere e non cambia niente.

### 5.2 Viste

Dopo la posa salva, nel documento, cinque viste con nome puntate sul modello di progetto: le quattro
assonometrie (camera ortogonale a 45° di rotazione e 35,264° di elevazione, `SE`, `SO`, `NE`, `NO`) e una
«Prospettiva» (camera prospettica dall'angolo SE, 30° di elevazione, lunghezza focale di 50 mm; (scelta mia)).
`VIEW RESTORE <nome>` le richiama. Il comando non cambia la vista corrente se non dopo `VIEW`: nessun cambio
di camera a sorpresa.

### 5.3 Estrazione

`AL_ESTRAI`: il DWG di fatto riceve le `3DFACE` della fase `fatto`, quello di progetto le `progetto`, la
comparativa nessuna (la differenza non è modellata). Senza XDATA né APPID `ARCHLINE_*`.

## 6. Interfaccia

Menu **Tools > Ristrutturazione**, barra e ribbon: una voce, **Modello 3D** (sedici comandi), con un'icona
propria 24×24 (stesso schema dei colori delle altre). Nessuna finestra.

## 7. Verifica

Casi sintetici, numeri a mano, entrambe le forme del rilievo.

- **Caso A** (altezza 2,70, senza porta): la massa muraria ha area in pianta 30,96 − 12,00 − 11,70 = 7,26 m², quindi
  i coperchi dei prismi sommano 7,26 m² e il volume è 7,26 × 2,70 = 19,602 m³; i pavimenti 12,00 e 11,70 m².
- **Caso A+P**: la porta 0,90 × 2,10 apre il tramezzo: la testa è alta 0,60 (2,70 − 2,10); nessun lato del
  prisma attraversa il vano.
- **Chiusura**: ogni prisma è un solido chiuso in alto e sui lati: ogni spigolo verticale è condiviso da due
  facce (test topologico sul modello).
- **Senza altezza**: nessun prisma, avviso; le aperture senza dati lasciano la massa piena e dicono perché.
- **Round-trip**: DXF e DWG; `ezdxf` conta le `3DFACE` per layer nel banco; nessun `ARCHLINE_*` negli estratti.
- **Viste**: cinque viste con nome nel documento, nomi esatti, la prospettiva con camera prospettica.

Test: `cargo test --locked --lib bim::`, `--lib app::bim_tests`, banco BIM con un quinto giro, `unittest` del banco.

## 8. Suddivisione prevista

1. Classificazione delle facce e altezza della massa (puro).
2. Prismi (coperchio triangolato e lati) e teste/davanzali dei vani (puro).
3. Pavimenti e assemblaggio dei due modelli con avvisi (puro).
4. Posa nel disegno: `3DFACE`, layer, record, sostituzione (puro).
5. Le viste con nome.
6. `AL_MODELLO3D` (lato app).
7. Estrazione.
8. Menu, barra, ribbon, icona.
9. Banco, documenti, registro.

## 9. Scelte mie e decisioni di Mauro

Da confermare con Mauro:

1. **Spaccato**: muri all'altezza dei locali, senza tetto né solai.
2. **Solo `3DFACE`**, niente solidi né mesh.
3. **Due modelli** (fatto e progetto) e **nessun** modello 3D delle demolizioni e costruzioni.
4. **Quattro assonometrie e una prospettiva** come viste con nome; camera della prospettiva (SE, 30°, 50 mm).
5. **Colori**: massa grigia, pavimenti più chiari; nessun colore per stato.
6. **Nessuna immagine**: l'uscita è il modello e le viste; render e stampa sono quelli di ArchLine.
7. **Dipendenza**: sopra le parti 3 e 4, non ancora unite a `lavoro`.
