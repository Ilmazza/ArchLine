# BIM parte 5: modello 3D — piano di implementazione

Spec: `docs/superpowers/specs/2026-11-05-bim-modello-3d-design.md` (proposta, non rivista da Mauro).
Branch: `claude/bim-3d`, da `claude/bim-sezioni`. Registro delle decisioni:
`docs/superpowers/ledgers/2026-11-05-bim-modello-3d.md` (R123…).

Regole del piano: come le parti 3 e 4 — un task, un commit, test sui numeri calcolati a mano nel caso A, `cargo
test --locked --lib bim::` dopo ogni task, il rilievo non si tocca mai, build e test in primo piano.

## Task

1. **Classificazione delle facce e altezza della massa** (`src/bim/mass3d.rs`, puro). Dalle facce di una fase,
   i contorni dei locali (con altezze) e i rettangoli delle aperture: `classify(...) -> Vec<Cell>` con
   `CellKind::{Room(i), Vano(k), Mass}`, la regione di ogni faccia (anello esterno e figli diretti, come
   `walls::region_at`) e per la massa l'altezza (massimo dei locali che ne toccano il contorno, stati
   `inserita`/`ereditata`, `n.d.` con avviso). Al grafo si aggiungono i quattro lati di ogni vano.
   Test: caso A (due locali, massa 7,26 m²), caso A+P (vano chiuso come faccia in entrambe le forme), locale senza
   altezza.
2. **Prismi** (`mass3d.rs`): coperchio triangolato (`kernel::geom2d::triangulate_rings`) e lati verticali per ogni
   anello; teste e davanzali dei vani. Test: area dei coperchi 7,26 m², volume 19,602 m³, ogni spigolo verticale
   condiviso da due facce, la porta lascia la testa alta 0,60.
3. **Pavimenti e assemblaggio** (`model3d.rs`): i due modelli (fatto, progetto) come liste di facce piane con
   colore, gli avvisi `ModelloEscluso`.
4. **Posa nel disegno** (`model3d_place.rs`): `3DFACE` con `ARCHLINE_MODELLO3D`, layer `AL-3D-FATTO` /
   `AL-3D-PROGETTO`, accanto alla pianta; sostituzione; round-trip DXF/DWG.
5. **Viste con nome**: le quattro assonometrie e la prospettiva nel documento (`View` di `codec::tables`),
   calcolate dall'estensione del modello di progetto.
6. **`AL_MODELLO3D`** (lato app): unità, calcolo, posa, sostituzione, riporto, un passo di annullamento.
7. **Estrazione**: il fatto nel DWG di fatto, il progetto in quello di progetto.
8. **Menu, barra, ribbon, icona** (sedici comandi).
9. **Banco, `CLAUDE.md` e registro.**

## Ganci in codice di upstream

Nessuno nuovo previsto; le viste usano `document.views` e `Scene::current_as_named_view` già esistenti.
