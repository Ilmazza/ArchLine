# BIM parte 5: modello 3D — registro delle decisioni

Spec: `docs/superpowers/specs/2026-11-05-bim-modello-3d-design.md`. Piano:
`docs/superpowers/plans/2026-11-05-bim-modello-3d.md`. Numerazione da R123 (la parte 4 arriva a R122).

| ID | Decisione | Costo se sbagliata |
|---|---|---|
| R123 | Il modello 3D si ricava dalle **facce** dei muri (come le sezioni, R111), non da «muri con altezza»: ogni faccia che non è un locale né un vano è un prisma da 0 all'altezza della massa (massimo dei locali che la toccano). Il vano di un'apertura è una faccia chiusa dai suoi quattro lati aggiunti al grafo, vuota per l'altezza dell'apertura. | Un'altezza che varia lungo un muro lungo è la più alta. |
| R124 | Classificazione (task 1, `mass3d.rs`): le facce sono i cicli di `walls::faces` con in più i **quattro lati** del rettangolo di ogni apertura (il vano è una faccia chiusa nelle due forme del rilievo); una faccia è locale o vano se coincide con il loro contorno (stessa area entro `tol²` e 1e-6 relativo, vertici sul bordo entro `tol`), altrimenti massa. I **figli diretti** sono i buchi del prisma. L'altezza della massa è il massimo dei locali che toccano il suo anello **o uno dei suoi buchi** (un locale può essere un buco della massa che lo circonda); `n.d.` e `Notice::ModelloEscluso` se manca un'altezza o nessun locale la tocca. Caso A: massa 7,26 m² senza porta, 7,17 m² con la porta (il vano 0,09 m² esce dalla massa), uguale in entrambe le forme del rilievo. | Una massa che non tocca nessun locale (un pilastro isolato fuori dagli ambienti) non si estrude. |

## Da confermare con Mauro

Le scelte del §9 della spec.

## Da vedere a schermo / in AutoCAD

Da compilare a fine implementazione.
