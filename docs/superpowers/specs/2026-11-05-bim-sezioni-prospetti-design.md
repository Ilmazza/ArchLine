# BIM parte 4: sezioni e prospetti estratti in 2D — progetto

Data: 2026-11-05. Branch previsto: `claude/bim-sezioni` (da `lavoro`, dopo la parte 3). Segue le parti 1
(stato e piante), 2 (abachi e quantità) e 3 (porte e finestre, `2026-11-05-bim-porte-finestre-design.md`).

**Stato del documento: proposta scritta in autonomia da Claude nel loop del 2026-11-05, non rivista da
Mauro.** Le scelte non ricavate da decisioni già prese portano «(scelta mia)» e stanno nel §9. Questa parte
dipende dalla parte 3 (aperture) e non si implementa prima.

## 1. Contesto e scopo

Dal disegno in pianta di una ristrutturazione Mauro deve ricavare **sezioni** e **prospetti** per le
tavole (stato di fatto, demolizioni e costruzioni, stato di progetto). Nel video «Retino» le sezioni si
tracciano con una linea e si aggiornano da sole. Qui lo stesso flusso, ma **in 2D e solo con ciò che il
disegno sa**: un muro ha un tracciato, uno spessore e (dalla parte 2) un'altezza; un'apertura ha
larghezza, altezza e davanzale (parte 3). Quote di solai, coperture, scale e fondazioni **non esistono**
nel modello: una sezione di questa parte non le disegna e dice che mancano (anti-obiettivo: nessuna quota
inventata).

Prodotto: un **disegno 2D** nello spazio modello (vista ortogonale a cui si applicano i soliti comandi di
quotatura e testo di ArchLine), generato da `AL_SEZIONE` e `AL_PROSPETTO`, con le stesse tre fasi di
`AL_ESTRAI` (fatto, demolizioni e costruzioni, progetto).

## 2. Obiettivi, non obiettivi, anti-obiettivi

**Obiettivi**

- Un **taglio di sezione** (oggetto nel disegno: linea con verso di vista e nome) e un **prospetto**
  (direzione di vista con nome: Nord, Sud, Est, Ovest o libera), con record XDATA come le altre cose.
- La sezione: i muri tagliati dal piano diventano rettangoli pieni (poché) dal piano di spiccato fino
  all'altezza del muro; le aperture tagliate lasciano il vano (davanzale/architrave dalla parte 3); i muri
  visti oltre il taglio in proiezione (più chiari, linea sottile).
- Il prospetto: i fronti dei muri esterni in proiezione, con i vani delle aperture e il loro contorno.
- Le tre fasi: demolite in giallo, nuove in rosso, nelle tavole di demolizioni e costruzioni (decisione di
  Mauro del 2026-10-23, colori e campitura piena).
- Un'**etichetta di stato** su ogni elemento con altezza `n.d.` (è escluso dal disegno e **elencato**).

**Non obiettivi**

- Solai, coperture, scale, fondazioni, quote altimetriche del terreno, controsoffitti: nessuno di questi
  esiste come oggetto. La sezione ha un **solo livello** (spiccato 0,00 locale al disegno).
- Ombre, materiali, stratigrafie (parte 6), prospettive (parte 5).
- Quotatura automatica (si usa quella esistente di ArchLine).
- Sezioni curve a più segmenti; il primo giro è un taglio rettilineo.

**Anti-obiettivi**

- Disegnare un muro «alto» quanto un valore di ripiego: un muro senza altezza (zona, locale o
  predefinita, parte 2 §4.1) **non entra** nel disegno e compare nell'elenco degli esclusi.
- Modificare le entità del rilievo (regola «originali intatti»).
- Disegni che sembrano completi e non lo sono: ogni tavola riporta un riquadro di **limiti** (solai,
  coperture, quote non presenti nel modello).

## 3. Vincoli

- File propri sotto `src/bim/` (`section.rs`, `elevation.rs`, `projection.rs`), comandi in
  `src/app/commands/`, pochi ganci di una riga in upstream (come le parti precedenti).
- Solo logica 2D deterministica, riuso di `kernel::geom2d`; nessuna dipendenza nuova, nessuna rete.
- Il risultato è un insieme di entità normali (`LINE`, `LWPOLYLINE`, `HATCH`, `MTEXT`) su layer
  `AL-SEZIONI` / `AL-PROSPETTI`, senza XDATA nel disegno estratto (regola esistente) e **posizionato
  accanto** al modello (non sovrapposto alla pianta), come la tabella d'abaco.
- Unità e tolleranze come le parti 1 e 2.

## 4. Modello

### 4.1 Taglio e vista

- **Taglio di sezione**: `LINE` su `AL-TAGLI` con record `ARCHLINE_TAGLIO`: nome («A-A»), verso di vista
  (a sinistra o a destra del tracciato, `Position3D` per il punto di vista), profondità di vista (m,
  inserita; assente = fino all'ultimo muro; **scelta mia**: mai infinita se manca, vedi §5.1).
- **Prospetto**: record `ARCHLINE_PROSPETTO` su un simbolo (freccia di direzione) con nome e direzione.

### 4.2 Che cosa si proietta

Per ogni **elemento verticale** del disegno di una fase:

1. i **muri** (le zone `nuovo` e i muri del rilievo non demoliti), ciascuno con tracciato, spessore, altezza
   risolta (parte 2) e il suo stato di altezza;
2. le **aperture** (parte 3) con larghezza, altezza, davanzale e tipo;
3. i **locali** non si proiettano: servono solo a nominare le stanze attraversate dal taglio.

L'altezza risolta ha lo stato della parte 2 (`inserita`, `ereditata`, `n.d.`): in tavola il tratto di un
muro con altezza `ereditata` ha la stessa grafica (il dato esiste), ma il **riquadro di fonti** dichiara
quali altezze sono ereditate dalla predefinita.

### 4.3 Che cosa si calcola

- **Sezione**: per ogni muro il cui tracciato incrocia la retta del taglio, il tratto tagliato (larghezza
  = spessore proiettato sulla retta), alto fino all'altezza del muro; le aperture sul tracciato
  nell'intervallo del taglio bucano il rettangolo tra `davanzale` e `davanzale + altezza` (porte:
  davanzale 0). I muri oltre il taglio, entro la profondità di vista, in proiezione senza poché.
- **Prospetto**: per ogni muro il cui lato più vicino al punto di vista è visibile (nessun muro davanti,
  semplice ordinamento per profondità sull'asse di vista; **scelta mia**: niente occlusione parziale,
  il muro davanti nasconde tutto il muro dietro lungo la sua estensione), il fronte come rettangolo;
  aperture come rettangoli interni.
- **Elementi esclusi**: muri senza altezza, aperture senza altezza (il vano non si disegna, il muro sì,
  con la nota), muri curvi (avviso «non supportato»). Ogni esclusione è un avviso tipizzato
  (`report::Notice`), mai silenziosa.

### 4.4 Punti di aggancio per il 3D

Le stesse grandezze (tracciato, spessore, altezza, aperture) sono i dati che la parte 5 estruderà. La
proiezione di questa parte non conserva lo stato 3D: è un'uscita, non un modello intermedio.

## 5. Comportamento

### 5.1 `AL_TAGLIO` e `AL_SEZIONE`

`AL_TAGLIO`: due punti del tracciato, un punto sul lato di vista, nome; profondità di vista facoltativa
(invio = fino ai muri più lontani **nel rettangolo di ingombro del disegno**, detto in riga di comando).
`AL_SEZIONE [nome]` genera la sezione per i tagli selezionati o per tutti, per le fasi richieste
(`Fatto/Demolizioni/Progetto/Tutte`), la posa **accanto** al modello (a destra, margine = larghezza
della tavola più larga, come `AL_ABACO`) e riporta in riga di comando: n. di muri tagliati, n. in
proiezione, elenco degli esclusi. Un passo di annullamento per lancio; rigenerare **sostituisce** i
disegni dello stesso nome (stessa regola di `AL_ABACO`).

### 5.2 `AL_PROSPETTO`

`AL_PROSPETTO N|S|E|O|<angolo> [nome]` come sopra. Il punto di vista è fuori dall'ingombro; le facce
visibili sono quelle rivolte a esso.

### 5.3 Estrazione e tavole

`AL_ESTRAI` ha una opzione per includere sezioni e prospetti generati nei tre DWG (nello spazio modello,
layer propri, senza XDATA). Impaginazione su tavola/layout: **fuori da questa parte** (c'è la stampa di
ArchLine).

## 6. Interfaccia

Menu **Tools > Ristrutturazione**: Taglio, Sezione, Prospetto; icone proprie 24×24 (stessa regola dei
colori); scheda nel ribbon. Nessuna finestra nuova: prompt in riga di comando.

## 7. Verifica

Casi sintetici (caso A+P della parte 3): numeri calcolati a mano.

- **Sezione trasversale A-A** a x=2: muri nord e sud di A tagliati, spessore 0,30 (solo se il caso A lo
  dice; si calcola dal disegno), altezza 2,70: due rettangoli 0,30 × 2,70; muro est (tramezzo) in
  proiezione con la porta 0,90 × 2,10 forata.
- **Prospetto Sud**: il muro sud di A + B in fronte, lunghezza 8,40, altezza 2,70; nessuna apertura.
- **Muro senza altezza**: escluso e elencato; la sezione dice cosa manca.
- **Fasi**: demolizione del tramezzo: nel disegno di demolizioni il muro demolito in giallo con poché pieno;
  nel progetto non c'è; la nuova parete in rosso.
- **Mai nel rilievo**: l'esecuzione non modifica nessuna entità (handle, geometria, attributi).
- **Banco BIM**: nuovo controllo in `bim_extract.py` su layer `AL-SEZIONI`, numero di poligoni di poché.

Test: `cargo test --locked --lib bim::`, `--lib app::bim_tests`, `--lib commands::bim_s*`, banco BIM e
`unittest` del banco.

## 8. Suddivisione prevista

1. Proiezione 2D pura (retta di taglio × segmenti, intervalli, occlusione per profondità).
2. Modello del taglio e del prospetto (XDATA, ancora, migrazione).
3. Sezione: muri e aperture tagliati, proiezione oltre il taglio.
4. Prospetto: fronti e aperture.
5. Fasi (colori, poché pieno), avvisi tipizzati, elenco esclusi.
6. Comandi `AL_TAGLIO`, `AL_SEZIONE`, `AL_PROSPETTO`, posa accanto al modello e sostituzione.
7. `AL_ESTRAI` con sezioni e prospetti.
8. Menu, barra, ribbon, icone.
9. Banco BIM, documenti e registro delle decisioni.

## 9. Scelte mie e decisioni di Mauro

Da confermare con Mauro:

1. **Un solo livello**, nessun solaio, tetto o quota di terreno: la sezione non li disegna e lo dice.
2. **Muri senza altezza esclusi** (anche se la pianta li ha), con elenco.
3. **Occlusione semplice** nei prospetti (per profondità lungo l'asse di vista), non parziale.
4. **Poché pieno** della sezione e colori di fase come nelle piante.
5. **Profondità di vista**: facoltativa; senza, fino all'ingombro del disegno.
6. **Posa accanto al modello** e sostituzione dello stesso nome, come l'abaco.
7. **Dipendenza**: dopo la parte 3 (aperture) e il commit del lavoro sull'ancora dei locali.
