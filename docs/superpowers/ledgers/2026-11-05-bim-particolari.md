# BIM parte 6: stratigrafie e particolari — registro delle decisioni

Spec: `docs/superpowers/specs/2026-11-05-bim-particolari-design.md`. Piano:
`docs/superpowers/plans/2026-11-05-bim-particolari.md`. Numerazione da R132 (la parte 5 arriva a R131).

| ID | Decisione | Costo se sbagliata |
|---|---|---|
| R132 | Modello (task 1, `strata.rs`): tutte le stratigrafie in **un** XRecord `ARCHLINE_STRATIGRAFIE` del dizionario radice, in sequenza (1 nome, 70 tipo, poi 3 materiale + 40 spessore per strato), letto per codice; uno spessore non finito o ≤ 0 letto da file si scarta. La scrittura di un XRecord con nome è ora comune (`record::write_named_xrecord`, usata anche da `ARCHLINE_PROGETTO`). Nome = un token (niente spazi né `;`), unico senza distinzione di maiuscole; strati scritti `materiale spessore; …` con l'ultimo token numero (punto o virgola), materiali con spazi. Spessori in unità del disegno, eco in cm con un decimale. Round-trip DXF e DWG provato. | Un nome con spazi non si può dare; uno strato scartato in lettura sparisce senza avviso (solo da file modificati altrove). |
