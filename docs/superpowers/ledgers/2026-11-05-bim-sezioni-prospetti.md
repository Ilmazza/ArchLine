# BIM parte 4: sezioni e prospetti — registro delle decisioni

Spec: `docs/superpowers/specs/2026-11-05-bim-sezioni-prospetti-design.md`. Piano:
`docs/superpowers/plans/2026-11-05-bim-sezioni-prospetti.md`. Numerazione da R111 (la parte 3 arriva a R110).

| ID | Decisione | Costo se sbagliata |
|---|---|---|
| R111 | La sezione non parte da «muri con altezza» (la spec §4.2 lo dava per scontato) ma dal **profilo lungo il taglio** ricavato dalle facce: tratti fuori, locale o massa muraria. L'altezza della massa è il massimo di quelle dei locali che la toccano; senza, la massa non si disegna e si elenca. | Un muro che separa un locale con altezza e uno senza viene escluso invece di prendere l'altezza dell'unico noto. |
| R112 | Elementi della sezione (task 2): un locale è vuoto da terra al suo soffitto; la massa ha il **massimo** delle altezze dei locali del tratto prima e del tratto dopo (stato `inserita` solo se tutti lo sono, altrimenti `ereditata`); un tratto di massa senza locali accanto, o accanto a un locale senza altezza, è `n.d.` e **non si disegna**, con un avviso `SezioneEsclusa`. Un'apertura buca la massa dove il taglio attraversa il suo rettangolo (Cyrus-Beck), da terra per porte e portefinestre, dal davanzale per le finestre; con altezza o davanzale che mancano il vano è `n.d.` e non si disegna (la massa resta piena in quel punto, e lo dice l'avviso). Nuovo avviso `Notice::SezioneEsclusa { what, reason }`. | Un muro tra un locale alto e uno senza altezza sparisce dalla sezione invece di prendere l'altezza dell'unico noto. |
| R113 | Disegno della sezione (task 3, `section_draw.rs`): distanza lungo il taglio = X, quota × unità per metro = Y, dall'origine data; poché = contorno `LWPOLYLINE` + `HATCH` solida per ogni pezzo, diviso dai vani delle aperture (sopra, e sotto se c'è un davanzale); locale = linea del soffitto; linea del pavimento a quota 0; titolo e riquadro dei **limiti** (un solo livello, niente solai, tetti, scale, quote del terreno). Layer `AL-SEZIONI`, nessun XDATA, `Tint` per stato (giallo demolito, rosso nuovo, `ByLayer` altrimenti). Un vano che non si disegna lascia la massa intera. | Un vano `n.d.` si vede pieno nel disegno: lo dice solo l'avviso. |

## Da confermare con Mauro

Le scelte del §9 della spec, più R111.

## Da vedere a schermo / in AutoCAD

Da compilare a fine implementazione.
