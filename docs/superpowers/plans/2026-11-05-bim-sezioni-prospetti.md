# BIM parte 4: sezioni e prospetti — piano di implementazione

Spec: `docs/superpowers/specs/2026-11-05-bim-sezioni-prospetti-design.md` (proposta, non rivista da Mauro).
Branch: `claude/bim-sezioni`, da `claude/bim-aperture` (parte 3 non ancora unita a `lavoro`).
Registro delle decisioni: `docs/superpowers/ledgers/2026-11-05-bim-sezioni-prospetti.md` (R111…).

Regole del piano: come la parte 3 — un task, un commit, test sui numeri calcolati a mano nel caso A (con la
porta), `cargo test --locked --lib bim::` dopo ogni task, il rilievo non si tocca mai. Build e test in primo
piano, nessun altro `cargo` nello stesso worktree.

## Correzione alla spec emersa dal codice (R111)

La spec §4.2 immagina i muri come oggetti con tracciato, spessore e altezza. Nel rilievo **non lo sono**: sono
linee sulle facce, e l'altezza esiste solo per zone, locali e predefinita del livello. Quindi la sezione non parte
da «muri» ma dal **profilo lungo il taglio**, come lo si ricava dalla pianta:

- le **facce** dei muri (`walls::faces`) dividono il piano in regioni; il taglio le incrocia e il tratto tra due
  incroci consecutivi è di tre tipi: **fuori** dall'edificio (nessuna faccia), **locale** (dentro il contorno di un
  locale misurato, fatto o progetto) o **massa muraria** (dentro una faccia che non è un locale: il poché);
- l'altezza di un locale è la sua (parte 2); quella di una massa è il **massimo delle altezze dei locali che la
  toccano** ai due lati (`inserita`, `ereditata`) e `n.d.` se ne manca una: una massa senza altezza non si disegna
  e si elenca (R111, mai un'altezza di ripiego);
- le aperture (parte 3) bucano la massa dove il taglio le attraversa: porte da 0 all'altezza, finestre da
  davanzale a davanzale + altezza, `n.d.` se mancano.

## Task

1. **Profilo lungo il taglio** (`src/bim/section.rs`, puro): `profile(faces, rooms, from, to, tol) -> Vec<Span>`
   con `SpanKind::{Outside, Room(i), Mass}`; incroci della retta con gli anelli delle facce, classificazione del
   punto medio, tratti adiacenti dello stesso tipo uniti. Test: caso A a y = 1,5 (fuori / muro 0,30 / A 4,00 /
   tramezzo 0,10 / B 3,90 / muro 0,30 / fuori), entrambe le forme del rilievo, taglio che non incrocia nulla, taglio
   che parte dentro un locale, taglio lungo un muro.
2. **Altezze e aperture sul profilo**: `SectionElement` (poché con altezza e stato, locale con altezza, buchi delle
   aperture con quota di partenza e altezza) e gli avvisi tipizzati per ciò che manca.
3. **Il disegno della sezione**: entità 2D (`LINE`, `LWPOLYLINE` chiuse, `HATCH` solide per il poché, `MTEXT`) sul
   layer `AL-SEZIONI`, con il piano di spiccato a quota 0 e un riquadro dei limiti; colori per fase; posa accanto al
   modello.
4. **Oggetto taglio** (`ARCHLINE_TAGLIO`, `AL_TAGLIO`): linea con nome, verso di vista e profondità facoltativa;
   migrazione e copia come le aperture (entità unica, R96).
5. **`AL_SEZIONE`**: legge i tagli, calcola i tre profili (fatto, demolizioni e costruzioni, progetto), posa i
   disegni e li **sostituisce** a ogni lancio (stessa regola di `AL_ABACO`); riga di comando con i conteggi e gli
   esclusi.
6. **Prospetti** (`AL_PROSPETTO`): i fronti dei contorni esterni rivolti al punto di vista, con le aperture; per
   profondità, senza occlusione parziale (R112).
7. **Estrazione**: sezioni e prospetti generati nei tre DWG (opzione di `AL_ESTRAI`).
8. **Menu, barra, ribbon, icone** (comandi nuovi: Taglio, Sezione, Prospetto).
9. **Banco, `CLAUDE.md` e registro.**

## Ganci in codice di upstream

Come le parti 1–3: `mod` nei file nostri e le voci di menu; nessun gancio nuovo previsto.
