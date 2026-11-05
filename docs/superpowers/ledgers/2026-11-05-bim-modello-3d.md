# BIM parte 5: modello 3D — registro delle decisioni

Spec: `docs/superpowers/specs/2026-11-05-bim-modello-3d-design.md`. Piano:
`docs/superpowers/plans/2026-11-05-bim-modello-3d.md`. Numerazione da R123 (la parte 4 arriva a R122).

| ID | Decisione | Costo se sbagliata |
|---|---|---|
| R123 | Il modello 3D si ricava dalle **facce** dei muri (come le sezioni, R111), non da «muri con altezza»: ogni faccia che non è un locale né un vano è un prisma da 0 all'altezza della massa (massimo dei locali che la toccano). Il vano di un'apertura è una faccia chiusa dai suoi quattro lati aggiunti al grafo, vuota per l'altezza dell'apertura. | Un'altezza che varia lungo un muro lungo è la più alta. |

## Da confermare con Mauro

Le scelte del §9 della spec.

## Da vedere a schermo / in AutoCAD

Da compilare a fine implementazione.
