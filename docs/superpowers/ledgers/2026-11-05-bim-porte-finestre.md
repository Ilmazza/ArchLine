# BIM parte 3: porte e finestre — registro delle decisioni

Spec: `docs/superpowers/specs/2026-11-05-bim-porte-finestre-design.md`. Piano:
`docs/superpowers/plans/2026-11-05-bim-porte-finestre.md`. Le decisioni di Mauro sulle parti 1 e 2 sono nei
registri `2026-10-23-…` e `2026-10-28-…`; la numerazione continua da R96.

| ID | Decisione | Costo se sbagliata |
|---|---|---|
| R96 | L'apertura è una sola entità (polilinea del vano con il record), senza ancora di famiglia: una copia è un'apertura indipendente. Corregge la spec §4.1 («ancora e migrazione come i locali»). | Se un giorno l'apertura avrà una parte staccata (etichetta, simbolo), serve un'ancora come per i locali. |
| R97 | La chiusura del vano porta vale sia per lo stato di fatto sia per quello di progetto (`measure_fatto` e `project_geometry`), con le fasi: fatto = esistente + demolita, progetto = esistente + nuova. La spec §4.2 parlava solo del calcolo dei locali senza dire che il fatto non usa `room_entities`. | Una porta esistente senza linea di soglia lascerebbe «non chiuso» il locale di fatto. |
| R98 | Layer `AL-APERTURE` e APPID in `aperture::ensure_layer`, non in `zone::ensure_layers`. | Un passo di annullamento in più la prima volta. |
| R99 | Il vano porta non ha facce (è il buco), quindi `AL_PORTA` estende l'asse di 5 cm (`REACH_M`) oltre le due spalle per toccare le facce che lo fiancheggiano e ricavare lo spessore con `rect_from_axis`; l'asse dell'apertura è la proiezione dei due clic sulla mezzeria trovata. Se le facce non ci sono, lo spessore si chiede. | Con un vano largo vicino a un angolo di muro più corto di 5 cm le facce possono non toccarsi: si digita lo spessore. |
| R100 | Due clic su spalle diverse danno un asse inclinato di qualche grado, e `rect_from_axis` vuole facce parallele all'asse entro 0,5°: la direzione di prova si allinea alla retta di muro più vicina al centro del vano (entro 1,50 m) se si discosta al più di 15° da quella dei clic (`SNAP_DEGREES`). La larghezza è misurata lungo il muro, non lungo il clic. | Un vano a 10° da un muro obliquo vicino potrebbe prendere la direzione sbagliata: sceglie la retta più vicina, mai una media. |
| R101 | Fuori da 0,60–3,00 m (larghezza) o 0,50–3,50 m (altezza) il comando chiede conferma («Sì» di default), come le zone con avvisi; il comando non stampa l'eco in metri dei valori digitati (la regola di `AL_ALTEZZA` sta lato app): da aggiungere con `AL_APERTURA_DATI` (task 4). | Nessun eco: Mauro non vede «210 cm = 2,10 m» alla creazione. |
| R102 | La chiusura del vano (task 3) passa da `aperture::threshold_segments` (due lati lunghi del vano, per fase) in quattro punti: `measure_fatto`, `room_snapshot` (solo le facce di fatto: alla geometria di progetto non si passa un `WallSet` con linee sintetiche), `project_geometry` (accanto alle soglie dei varchi) e le regioni di fatto di `computed_project_rooms`. Il vano come faccia chiusa a polilinee (con le spallette nel rilievo) non è un locale «(senza nome)»: `project_rooms` riceve gli anelli dei vani (`aperture::vano_rings`) e li salta, come fa per i varchi (parametro nuovo `vani`). Nessun avviso per il vano: la sua area non è di nessun locale. | Un vano largo più di una spalletta con un seme di progetto dentro darebbe un locale di troppo; non c'è un avviso `Varco` per le aperture. |
| R103 | `AL_APERTURA_DATI` (task 4): `Stato`, `Altezza`, `Davanzale`, `Soglia`, `Architrave`, `Appoggio`, `Rimuovi Altezza|Davanzale|Appoggio` sulle aperture selezionate, un passo di annullamento (nessuno se nulla cambia), eco in metri dei valori digitati e avviso fuori da 0,50–3,50 m per l'altezza; il davanzale è solo delle finestre e la soglia solo di porte e portefinestre (altrimenti «Non si applica a N aperture»). L'**appoggio dell'architrave è solo dell'apertura**: la voce di progetto nel record `ARCHLINE_PROGETTO` prevista dal piano non c'è (R101 resta aperta sull'eco alla creazione). | Un appoggio uguale per tutto il progetto va digitato su ogni apertura; l'eco in metri manca ancora in `AL_PORTA`. |

## Da confermare con Mauro

Le otto scelte del §9 della spec (aperture come oggetto proprio, chiusura automatica del vano, nessuna
altezza di ripiego, default stato/soglia/architrave, soglie degli avvisi, pareti dei locali, nessun simbolo
dell'anta, dipendenza).

## Da vedere a schermo / in AutoCAD

Da compilare a fine implementazione.
