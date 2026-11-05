# BIM parte 3: porte e finestre — registro delle decisioni

Spec: `docs/superpowers/specs/2026-11-05-bim-porte-finestre-design.md`. Piano:
`docs/superpowers/plans/2026-11-05-bim-porte-finestre.md`. Le decisioni di Mauro sulle parti 1 e 2 sono nei
registri `2026-10-23-…` e `2026-10-28-…`; la numerazione continua da R96.

| ID | Decisione | Costo se sbagliata |
|---|---|---|
| R96 | L'apertura è una sola entità (polilinea del vano con il record), senza ancora di famiglia: una copia è un'apertura indipendente. Corregge la spec §4.1 («ancora e migrazione come i locali»). | Se un giorno l'apertura avrà una parte staccata (etichetta, simbolo), serve un'ancora come per i locali. |
| R97 | La chiusura del vano porta vale sia per lo stato di fatto sia per quello di progetto (`measure_fatto` e `project_geometry`), con le fasi: fatto = esistente + demolita, progetto = esistente + nuova. La spec §4.2 parlava solo del calcolo dei locali senza dire che il fatto non usa `room_entities`. | Una porta esistente senza linea di soglia lascerebbe «non chiuso» il locale di fatto. |
| R98 | Layer `AL-APERTURE` e APPID in `aperture::ensure_layer`, non in `zone::ensure_layers`. | Un passo di annullamento in più la prima volta. |

## Da confermare con Mauro

Le otto scelte del §9 della spec (aperture come oggetto proprio, chiusura automatica del vano, nessuna
altezza di ripiego, default stato/soglia/architrave, soglie degli avvisi, pareti dei locali, nessun simbolo
dell'anta, dipendenza).

## Da vedere a schermo / in AutoCAD

Da compilare a fine implementazione.
