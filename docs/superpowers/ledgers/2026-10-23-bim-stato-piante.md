> **Registro delle decisioni** del piano BIM parte 1 (`docs/superpowers/plans/2026-10-23-bim-stato-piante.md`): le decisioni R0…R66 prese durante l'esecuzione del piano, ognuna con il costo se sbagliata («se sbagliato: …»); le righe `minor (deferred)` sono limiti noti, non difetti dimenticati.
> **Istantanea** del registro di lavoro (`.superpowers/sdd/2026-10-23-bim-stato-piante/progress.md`, locale e fuori da git) al commit `575b4a8f0` del branch `claude/bim-stato-piante` (HEAD alla copia, dopo il fix finale): non si aggiorna da sé.
> Gli ID R… (e i «Task N») sono citati da `CLAUDE.md` e dai commenti nel codice, per esempio «R30» o «Task 12»: si risolvono qui.
> Le ultime righe sono l'esito della revisione finale dell'intero branch e le decisioni R65/R66 che definiscono il fix finale.
> Controllato prima della copia: nessun dato di clienti né percorso personale (solo disegni sintetici e file del repo).

# SDD ledger — plan: docs/superpowers/plans/2026-10-23-bim-stato-piante.md

Spec: docs/superpowers/specs/2026-10-23-bim-stato-piante-design.md (approvata da Mauro; piano approvato da Mauro il 2026-10-23, "Si e usa subagent driven").
Branch: claude/bim-stato-piante (lavoro in place, nessun worktree). Start commit: 3c9e09504.

## Pre-flight scan

| Coppia / task | Produce → consuma | Esito |
|---|---|---|
| T1↔T2 | costanti `bim::mod` → zone.rs | ok |
| T1↔T6 | `units::resolve`, `record::write` → dispatch_bim | ok |
| T2↔T5 | `zone_hatch`, `zone_hatch_from_paths` → ZoneCommand | ok |
| T2↔T6 | `missing_layers`/`ensure_layers` → primo passo di annullamento | LACUNA: nessuno registra l'APPID `ARCHLINE_ZONA` (spec 4.1 vuole set_entity_xdata, che lo registra) → R2 |
| T2↔T13/T14 | layer `AL-LOCALI`, `AL-LOCALI-PROGETTO` (ACI 7) | LACUNA: nessun task li crea con handle → R2 |
| T2↔T7 | `read_zone`, `restyle` → AL_STATO | ok |
| T3↔T4 | fixtures, `wall_set` → `sources_for` | ok |
| T3↔T5 | `faces`, `region_at` → punto interno | ok |
| T3↔T13/T14 | `region_at`/`compute_at` con vuoto d'angolo | CONFLITTO: il contorno esterno del caso A racchiude sempre il punto, quindi con il vuoto la regione è più grande (19,26 m²), non None → R3 |
| T4↔T11 | `WallRect.long_sides/end_sides` → tagli e chiusure | ok |
| T5↔T6 | `ZoneSnapshot`, `ZoneCommand::new` → famiglia app | ok |
| T6↔T7 | rami di `dispatch_bim` | ok |
| T6↔T16 | registrazione nomi → `every_command_exists` | ok |
| T8↔T9 | `extract`, `write`, `WriteError` → AL_ESTRAI | AL_ESTRAI chiede 3 elaborati prima del T12 → R5 |
| T8↔T12 | ramo StatoDiProgetto | ok |
| T9↔T12 | `propose_new_walls_layer` (vuoto fino al T12) | ok (detto nel piano) |
| T10↔T11 | `kept_pieces` con indici di segmento | ok |
| T11↔T13 | `room_entities` con lati lunghi del varco | ok |
| T13↔T14↔T15 | `compute_at`, `project_rooms` | ok (con R3) |
| Spec↔piano | archi: spec 3 "corde, approssimato"; piano "archi esatti con ripiego" | piano approvato da Mauro dopo la spec: vale il piano (R0) |

| Task | Coerenza interna | Esito |
|---|---|---|
| T1 | `format_m2(11.905) == "11,91"` vs `{:.2}` che dà 11,90 | → R1 |
| T2 | test vs interfacce | ok |
| T3 | aree 12,00/11,70/11,91, fuori piano, doppie, bloccati | ok |
| T4 | NoFaces/OneFace con i numeri dati | ok (verificato a mano) |
| T5 | `validate` "attraversi o tocchi l'interno": l'asse sul tramezzo ha le linee SUL bordo; AL_NUOVO (2;0)-(2;3) ha la faccia y=0 sul bordo e il test si aspetta nessun avviso | → R4 |
| T6, T7 | test vs flusso | ok |
| T8, T9, T10 | test vs procedura | ok |
| T11 | `new_l_shape_has_no_seam` "6 linee": sul caso A il lato y=0 è collineare con la faccia di A → non aggiunto + Giunzione → 5 linee | → R6 |
| T12 | ok |
| T13 | "(senza nome)": punto interno del baricentro della faccia; la faccia del contorno esterno come anello cade dentro A | → R7 |
| T14 | `aggiorna_reports_open_room` togliendo (0;0)-(4;0): stesso problema di R3 | ok con R3 |
| T15, T16 | ok |
| T17 | `--serve` può riscrivere il vero settings.json (trappola 5 di CLAUDE.md) | → R8 |

## Rulings

- R0 Ruling: aree su archi esatte con `exact_hatch_paths` e ripiego dichiarato (`ContornoApprossimato`), come dice il piano, non corde come diceva la spec §3 — Mauro ha approvato il piano dopo la spec e il piano lo segnala come scelta — se sbagliato: costo basso, il ripiego a corde esiste già nel codice.
- R1 Ruling: `format_m2` arrotonda con `(m2 * 100.0).round()` (metà lontano da zero) e poi formatta; `format!("{:.2}")` darebbe "11,90" per 11.905 — il test del piano chiede "11,91" — se sbagliato: un centesimo sui casi a metà.
- R2 Ruling: `zone::ensure_layers` crea con handle tutti e quattro i layer `AL-*` (DEMOLIZIONI ACI 2, NUOVE ACI 1, LOCALI ACI 7, LOCALI-PROGETTO ACI 7) e registra con handle gli APPID `ARCHLINE_ZONA` e `ARCHLINE_LOCALE` (come `set_entity_xdata`); `missing_layers` è vero se manca uno qualunque — la spec 4.1 vuole l'APPID registrato e nessun task creava i layer dei locali — se sbagliato: due layer vuoti in più nel disegno di lavoro al primo AL_DEMOLISCI.
- R3 Ruling: `rooms::compute_at` (T13) restituisce None ("non chiuso") quando dentro la regione trovata c'è un gruppo di segmenti muro connessi che non tocca entro tol né l'anello esterno né i buchi della regione: è il segno di un vano aperto (vuoto d'angolo). I monconi collegati al bordo non contano. Così il vuoto di 2 mm dà "non chiuso" invece di un'area sbagliata più grande; i test di T13/T14 restano come scritti — senza, ogni vuoto nel caso A darebbe 19,26 m² invece del messaggio — se sbagliato: un arredo aperto lasciato acceso dentro un locale lo rende "non chiuso" (messaggio, non numero sbagliato).
- R4 Ruling: `validate` — Demolito: conta un segmento che tocca la zona chiusa, bordo compreso (le facce del muro stanno sul bordo della zona ad asse); Nuovo: conta solo un tratto con punto medio strettamente dentro (distanza > tol dal bordo) e fuori da zone Demolito — è l'unica lettura in cui passano i test di T5/T6/T8/T17 (asse sul tramezzo senza avviso; nuovo (2;0)-(2;3) senza avviso) — se sbagliato: un muro nuovo appoggiato con una faccia su un muro esistente non dà avviso.
- R5 Ruling: fino al T12 `extract` salta in silenzio `StatoDiProgetto` (nessun documento, nessun avviso); AL_ESTRAI lo chiede già, il T12 lo aggiunge — stato intermedio del branch, mai consegnato — se sbagliato: nulla, il T12 lo copre.
- R6 Ruling: `new_l_shape_has_no_seam` si fa su un disegno senza muri esistenti (entità vuote), così le 6 linee valgono; sul caso A il lato y=0 sarebbe collineare con la faccia di A (5 linee + Giunzione, regola 4 del T11) — se sbagliato: il test copre la cucitura ma non l'interazione con l'esistente, che copre già `new_against_existing_reports_junctions`.
- R7 Ruling: in `project_rooms` il punto interno di una faccia si prende sulla faccia come regione (anello meno le facce figlie dirette, stessa regola di `region_at`), non sull'anello grezzo: altrimenti la faccia del contorno esterno avrebbe il punto dentro A e diventerebbe un "(senza nome)" — se sbagliato: nessuno, la regola è più stretta.
- R8 Ruling: lo script del T17 avvia ArchLine con `APPDATA` (e `LOCALAPPDATA`) puntati a una cartella temporanea: `config_dir()` legge `APPDATA` su Windows, così il vero settings.json di Mauro non si tocca (trappola 5 di CLAUDE.md) — se sbagliato: nessuno.

## Progress
Task 1: dispatched (BASE 3c9e095, implementer sonnet)
Task 1: implementer DONE a60418b14; review dispatched (sonnet)
Task 1: review: spec ✅, quality Approved; ⚠️ coordinate negli avvisi col punto decimale → confermato come lacuna (il piano scrive "(4,05; 0)") → fix round 1
Task 1: minor (deferred): record::write no-op silenzioso se manca il dizionario radice (record.rs:116-119); commento dice che c'è sempre, il codec dice di no
Task 1: minor (deferred): test mancanti su from_keyword/metres/tolerance, write con None, riuso dello stesso handle al secondo write
Task 1: minor (deferred): dipendenza circolare units↔record; read ignora la versione (codice 90); commento di format_m2 "mezzo verso l'alto" impreciso sui negativi; record non marca il documento modificato (lo fa il chiamante)
Task 1: fix round 1/5 (1 addressed, 0 open — coordinate con virgola; commits a60418b..9e3f6ba)
Task 1: complete (commits 3c9e095..9e3f6ba, review clean)
Task 2: dispatched (BASE 9e3f6ba, implementer sonnet)
Task 2: implementer DONE_WITH_CONCERNS 6fef6b0 — restyle non tocca raw_dwg_eed (manca l'handle APPID): una zona riletta da DWG, ristilizzata e risalvata tiene lo stato vecchio nel record → da portare nel Task 7: dopo restyle chiamare set_entity_xdata(doc, h, APP_ZONA, Some(encode(z))), che scarta le voci raw dell'APPID
Task 2: review dispatched (sonnet)
Task 2: review: spec ✅, quality Approved
Task 2: minor (deferred): rings[0] = esterno non garantito in lettura (read_zone prende l'ordine dei path; zone_hatch_from_paths prende l'ordine del chiamante) → puntatore per Task 5 (punto interno: passare l'esterno per primo) e Task 11
Task 2: minor (deferred): CLOSE_EPS assoluto 1e-9 in open_ring (coordinate grandi tipo Gauss-Boaga); test DWG senza varco=true/altezza; decode last-wins su chiavi doppie; duplicazioni nei test; nomi key()/ACI_BIANCO locale; doc di zone_hatch non dice che ensure_layers va chiamato prima di add_entity (il Task 6 lo fa al passo 3)
Task 2: complete (commits 9e3f6ba..6fef6b0, review clean)
Task 3: dispatched (BASE 6fef6b0, implementer opus)
Task 3: implementer DONE_WITH_CONCERNS dd6bda9 — difetto upstream exact_hatch_paths (una "D" arco+corda in una sola polilinea diventa cerchio intero); mitigato con scarto d'area per anello 5%, ma una nicchia a "D" sul contorno di un locale grande può passare per esatta
- R9 Ruling: in net_area si aggiunge il controllo geometrico (rimedio 3 del rapporto): ogni punto campionato del percorso esatto deve stare entro la freccia delle corde (più tol) dall'anello delle facce, altrimenti si ripiega sul poligono con exact=false; nessuna modifica a upstream (edge_curve resta com'è) — un numero sbagliato che passa per esatto è l'anti-obiettivo della spec; il 5% sull'area non lo esclude — se sbagliato: qualche locale con archi risulta "approssimato" (avviso ContornoApprossimato) quando sarebbe esatto.
- R10 Ruling: accettati i due scostamenti dal brief: entity_curves_on_plane resta privata (inutilizzata, la riesportazione dava un warning); face_inside decide sul primo vertice che non tocca l'esterno (con test della rientranza a L) — se sbagliato: un task futuro che usi entity_curves_on_plane la renderà pub(crate) allora.
Task 3: correzione pre-review R9 inviata all'implementer
Task 3: correzione R9 b2c52c4 (controllo geometrico in net_area, test nicchia a D); review dispatched (opus)
Task 3: review (opus, ripresa dopo stallo): ❌ 2 Important — (1) "D"/linea+arco con arco ≤ ~10,5° passa il controllo R9 come esatta (bulge 0,04: 40,05 m² exact); (2) boundary_area somma rispetto all'origine: a coordinate Gauss-Boaga errore fino a 4e-3 m² con exact=true (plan-mandated: il brief impone boundary_area)
- R11 Ruling: si tiene boundary_area (piano) ma anelli e sorgenti si traslano in un'origine locale (-region.outer[0]) prima di exact_hatch_paths/boundary_area; test del caso A spostato a (2 300 000; 4 640 000) m e mm — il piano chiede boundary_area, non vieta la traslazione; la spec vieta valori sbagliati — se sbagliato: nessuno, la traslazione non cambia l'area in aritmetica esatta.
- R12 Ruling: rimedio (a) del revisore, locale in walls.rs: exact=false per ogni anello con un lato attribuibile a più di una curva sorgente (due curve distinte con gli stessi estremi del lato: "D" o linea+arco); nessuna modifica a edge_curve di upstream (rimedio b, va chiesto a Mauro se mai) — se sbagliato: absidi chiuse da soglia risultano "contorno approssimato" con area a corde.
Task 3: minor (deferred): totale esatto ricalcola l'annidamento con ring_nesting_depths invece di esterno − Σ buchi (walls.rs:256); off_plane conta anche curve degeneri (walls.rs:120, l'avviso non dovrà dire solo "fuori piano"); tassellazione upstream 7,5° contro la freccia ~0,1 mm della spec §3 (pesa sul ripiego a corde: r=100 m → 0,8 m²); allowed usa il raggio massimo per tutti i lati (walls.rs:292); WallSet senza Debug
Task 3: fix round 1/5 (2 addressed, 0 open — lati ambigui arco/corda, origine locale; commits b2c52c4..8b15d77)
Task 3: minor (deferred): has_ambiguous_edge duplica la scelta privata di edge_curve di upstream (al merge va ricontrollata); test georiferito con soglia 1e-9 vicina all'ulp; d_shaped_polyline_falls_back_to_chords confronta aree con uguaglianza esatta
Task 3: complete (commits 6fef6b0..8b15d77, review clean)
Task 4: dispatched (BASE 8b15d77, implementer sonnet)
Task 4: implementer DONE_WITH_CONCERNS d16c83b — 4 scelte non coperte dal brief
- R13 Ruling: se l'asse cade su una faccia (candidato con |s| ≤ tol), quella è un lato e l'altro è il candidato valido più vicino (|s| minimo) da una qualunque parte, invece del più vicino a sinistra/destra — con la regola del brief, un clic sulla faccia di un muro che dà su un corridoio < 1,50 m prenderebbe la faccia oltre il corridoio (spessore sbagliato) — se sbagliato: nessuno nei casi del piano (il test sulla faccia del tramezzo dà sempre 0,10).
- R14 Ruling: lo scostamento s di un candidato si misura nel punto medio del tratto in cui la sua proiezione copre (0, |p2−p1|), non all'inizio del segmento: con la tolleranza di 0,5° un segmento di 10 m darebbe fino a 8,7 cm di errore sugli angoli della zona — per segmenti paralleli il valore è identico — se sbagliato: nessuno.
- R15 Ruling: accettate le scelte 1 (asse sull'unica faccia senza faccia vera dall'altra parte → OneFace) e 2 (due facce valide ma spessore oltre il limite → NoFaces): portano entrambe alla richiesta dello spessore — se sbagliato: solo il testo del prompt.
Task 4: correzione pre-review R13+R14 inviata all'implementer
Task 4: correzione R13/R14 98136c0; review dispatched (sonnet)
Task 4: review: spec ✅, quality Approved; ⚠️ candidati solo Curve::Line (archi e lati con bulge non sono facce)
- R16 Ruling: confermato, sono facce solo i tratti dritti: un muro con una faccia ad arco dà OneFace e quindi la richiesta dello spessore (spec 5.1 "due linee parallele"; nessun valore inventato) — se sbagliato: su muri curvi si digita lo spessore.
Task 4: minor (deferred): doc di modulo di walls.rs da aggiornare (§5.1); confronti negati anti-NaN senza commento (walls.rs:606,656,662); doc di axis_direction (NaN a lunghezza 0); test ridondanti; controfaccia quasi doppia a poche tol → muro sottilissimo (nessuno spessore minimo): il prompt del varco mostra l'area della zona
Task 4: complete (commits 8b15d77..98136c0, review clean)
Task 5: dispatched (BASE 98136c0, implementer opus)
Task 5: implementer DONE_WITH_CONCERNS 668e218 — (1) con R4 un AL_NUOVO che ricalca esattamente un muro esistente non avvisa; (2) punto interno con net_area non esatta → campitura dagli anelli; (3) selezione non valida ripete il prompt senza messaggio; (4) via script "0,12" diventa il punto (0;12) (feed_active_cmd legge le coordinate prima); dalla riga di comando vera un passo SingleToken riceve il testo prima (update/command.rs:955-975), quindi "0,12" funziona per l'utente
- R17 Ruling: NuovoSuEsistente anche quando due lati paralleli e opposti dell'anello della zona nuova (a distanza > tol) sono coperti ciascuno per più di metà lunghezza da segmenti muro esistenti non dentro una zona Demolito (bordo compreso) — la spec 5.1 chiede l'avviso per il nuovo sopra un muro esistente non demolito, e R4 da solo lo perdeva — se sbagliato: un avviso in più su un muro nuovo stretto tra due muri esistenti paralleli (es. riempimento di una nicchia), che l'utente conferma.
- R18 Ruling: selezione non valida → il prompt si ripete con il prefisso "L'oggetto scelto non è una polilinea chiusa. " (come "Nessuna area chiusa attorno al punto. ") — spec: mai blocco silenzioso — se sbagliato: solo testo.
- R19 Ruling: accettati (2) campitura dagli anelli quando net_area non è esatta (i percorsi esatti potrebbero essere sbagliati) e (4) negli script lo spessore col punto ("0.12"), come già nei test del piano — se sbagliato: nessuno.
Task 5: correzione pre-review R17+R18 inviata all'implementer
- R17b Ruling: R17 alla lettera contraddiceva il suo stesso test (le estremità y=0/y=3 di un muro nuovo trasversale stanno sulle facce di A, parallele e coperte): due lati coperti contano come "muro esistente" solo se distano ≤ MAX_WALL_THICKNESS_M e meno della lunghezza di entrambi i lati (scostamento dell'implementer, accettato) — se sbagliato: un riempimento di nicchia stretta dà un avviso da confermare.
Task 5: correzione R17b/R18 6500db8; review dispatched (sonnet)
Task 5: review (opus): spec ✅; 1 Important (da decisioni R4+R17): muro nuovo più sottile sopra un muro esistente non demolito non avvisa (zona 4,02–4,08 sul tramezzo; muro sottile allineato a una faccia)
- R20 Ruling: controllo "a pinza" in validate per le zone Nuovo rettangolari (modo asse e modo rettangolo): rect_from_axis sulla mezzeria parallela al lato lungo, con le sorgenti meno i tratti dentro zone Demolito (bordo compreso); se trova un muro (Ok) → NuovoSuEsistente — è lo scenario per cui la convalida esiste (spec 5.1: ricostruire dimenticando AL_DEMOLISCI) — se sbagliato: un avviso da confermare per un muro nuovo lungo un corridoio ≤ 1,50 m.
Task 5: minor (deferred): prompt ripetuti senza motivo (punti coincidenti, rettangolo degenere, poligono < 3 punti, Invio senza last); motivo "non è una polilinea chiusa" anche per area nulla/normale inclinata; spessore senza controllo di plausibilità (scambio di unità); commands.rs 1899 righe (validate potrebbe andare in validate.rs, doppioni Bounds/edges con walls.rs); rami non testati (normale −Z, chiusura per vertice, UCS ruotato, georiferito, anteprima poligono); messaggio d'errore cita "0,12" che da script è una coordinata; Tolerance::new va in panic con tol ≤ 0; falso allarme R17 sulla tamponatura di un vano con linee di soglia (da documentare)
Task 5: fix round 1/5 (1 addressed, 0 open — tenaglia R20; commits 6500db8..dd52e63)
Task 5: minor (deferred): controllo degli angoli retti in rectangle_midline con soglia relativa 1e-9 (commands.rs:876-879): su disegni georiferiti con asse ruotato la tenaglia si spegne in silenzio (84% dei casi a Gauss-Boaga); usare |u·v|/|u| ≤ tol o coordinate locali; mancano test con asse ruotato e in mm — DA TRIARE NELLA REVISIONE FINALE
Task 5: minor (deferred): una faccia sul bordo di una zona demolita viene scartata anche se appartiene a un secondo muro in piedi (convenzione bordo compreso)
Task 5: complete (commits 98136c0..dd52e63, review clean)
Task 6: dispatched (BASE dd52e63, implementer opus)
Task 6: implementer DONE_WITH_CONCERNS edfaf02 — 9 righe di aggancio upstream; (1) messaggio INSUNITS doppio negli script (comportamento del runner di upstream); (2) nessun rifiuto nello spazio carta; (3) FuoriPiano conta anche curve degeneri; (4) snap.tol = u.tolerance(), AL_UNITA senza parola non rilancia, errore esplicito se il record non si scrive
- R21 Ruling: AL_DEMOLISCI/AL_NUOVO (e in seguito gli altri AL_*) rifiutano nello spazio carta con un messaggio in riga di comando, senza creare nulla — la spec vuole le zone nello spazio modello e zones_in/wall_set non vedrebbero una zona in carta: sarebbe un dato perso in silenzio — se sbagliato: l'utente deve passare al Modello per usare i comandi.
- R22 Ruling: accettati (1) (comportamento del runner di upstream, non nostro) e (4); (3) resta minor del Task 3 da triare nella revisione finale — se sbagliato: un avviso "fuori dal piano" che conta anche linee di lunghezza nulla.
Task 6: correzione pre-review R21 inviata all'implementer
- R23 Ruling: accettato il rifiuto anche nell'editor di blocchi (la zona finirebbe nella definizione del blocco, invisibile a zones_in) — se sbagliato: si toglie una condizione.
Task 6: correzione R21 ca44369; review dispatched (sonnet)
Task 6: review: spec ✅, quality Approved
Task 6: minor (deferred): bim_set_unit segna dirty prima del controllo di scrittura (passo vuoto se il record non si scrive, bim.rs:129-137); test di last_thickness non discrimina "handle più alto"; colori ACI 7 dei layer dei locali non asseriti; rami AL_UNITA senza parola / record non scrivibile senza test; predicato dello spazio modello duplicato da scene/entity.rs:383-393 (da ricontrollare ai merge)
Task 6: complete (commits dd52e63..ca44369, review clean)
Task 7: dispatched (BASE ca44369, implementer sonnet)
- R24 Ruling: AL_STATO N su una zona demolita con varco mette varco = false (una zona nuova non può essere un varco, spec 4.1 "varco solo per le zone demolito"); un solo passo di annullamento e solo se almeno una zona cambia; valori A/S con virgola o punto, > 0, altrimenti errore senza modifiche — se sbagliato: riportando la zona a Demolito il varco va rimesso a mano.
Task 7: implementer DONE b6ef175; review dispatched (sonnet)
Task 7: review: spec ✅; 1 Important — AL_STATO sposta una zona su AL-NUOVE/AL-DEMOLIZIONI senza ensure_layers (dopo PURGE il layer può mancare) → fix round 1
Task 7: ⚠️ AL_STATO S cambia solo il numero dello spessore, non gli anelli della zona (come dice il brief; il dato resta metadato per la parte 2) — risolto: segue il brief, nessuna correzione
Task 7: minor (deferred): parse_positive duplica parse_number di bim/commands.rs:472; testi "1 zone modificate"/"1 oggetti non sono zone" (plan-mandated, plurale con 1); HashMap invece di FxHashMap (bim.rs:222); test con conteggio 12 LINE fisso; manca un HATCH solido non-zona come oggetto ignorato; dopo un errore al prompt Altezza il comando è già finito (comportamento di SelectThenKeywordCommand)
Task 7: fix round 1/5 (1 addressed, 0 open — ensure_layers in AL_STATO; commits b6ef175..b9d4514)
Task 7: complete (commits ca44369..b9d4514, review clean)
PR 1 (Task 1-7) completa sul branch.
Task 8: dispatched (BASE b9d4514, implementer opus)
Task 8: implementer DONE_WITH_CONCERNS 5682c5b — NonCopiata scatta solo per handle fantasma: immagini senza definizione e INSERT di xref senza blocco passano in silenzio; layout non elencati; LTSCALE e altre variabili, layer 0 e stile Standard di src non copiati; HATCH con record ARCHLINE_ZONA illeggibile entra nel fatto
- R25 Ruling: in extract si escludono e si elencano con NonCopiata (kind = tipo) le entità che il percorso di copia non porta intere: immagini raster, underlay (PDF/DWF/DGN), OLE, INSERT di riferimenti esterni, e i tipi 3D (3DSOLID, REGION, BODY, SURFACE, MESH) — spec 5.5 dice che immagini, xref e 3D sono "elencati nel rapporto", e l'anti-obiettivo vieta perdite in silenzio — se sbagliato: le immagini di sfondo non arrivano nei DWG estratti (vanno reinserite a mano), ma lo dice il rapporto.
- R26 Ruling: un NonCopiata per ogni layout di spazio carta con entità (handle del blocco del layout, kind = "Layout <nome>") — spec 5.5 "layout ... elencato"; nessuna variante nuova di Notice — se sbagliato: righe in più nel rapporto.
- R27 Ruling: si copiano da src le variabili scalari LTSCALE, PSLTSCALE, CELTSCALE, MEASUREMENT, LUNITS, LUPREC, AUNITS, AUPREC, TEXTSIZE, DIMSCALE (niente handle), e le proprietà del layer 0 e dello stile di testo Standard (colore, tipo di linea, font, altezza) — spec 5.5 "tabelle dei layer, tipi di linea e stili di testo copiati"; LTSCALE cambia l'aspetto di ogni linea tratteggiata — se sbagliato: nessuno (valori di src). SORTENTS resta fuori (minor).
- R28 Ruling: un'entità con un record ARCHLINE_ZONA (leggibile o no) è una zona per l'esclusione dal fatto — nessun dato nostro deve entrare come tratteggio comune — se sbagliato: nessuno.
Task 8: correzione pre-review R25-R28 inviata all'implementer
Task 8: correzione f6105d7 (R25-R28 applicate) — nuovi dubbi: INSERT di blocchi con immagini/xref esclusi interi; stili MLEADER/MLINE/TABLE non copiati da WBLOCK → entità con riferimento a stile mancante, non elencate
- R29 Ruling: accettato che un INSERT di un blocco che contiene immagini/underlay/OLE/xref (anche annidati) sia escluso intero e elencato; un 3D dentro un blocco resta — se sbagliato: un cartiglio con logo raster sparisce dall'estratto, ma è elencato.
- R30 Ruling: per MLEADER, MLINE, TABLE (e ogni entità il cui oggetto stile manca in out dopo la copia) si copia lo stile da src in out come si fa per i tipi di linea (passo 5); se la copia dello stile richiede di toccare il codec o è sproporzionata, quelle entità si escludono e si elencano con NonCopiata — mai un riferimento a uno stile assente in silenzio — se sbagliato: annotazioni escluse dall'estratto ma elencate.
Task 8: correzione R30 inviata all'implementer
Task 8: correzione R30 ef1e528 — MLINE porta lo stile; MULTILEADER/ACAD_TABLE/TOLERANCE/SHAPE/SectionSymbol esclusi ed elencati (grafo di oggetti per handle)
Task 8: minor (deferred): handle comuni non controllati (plotstyle_handle con stili di stampa con nome, color_book_handle), stili di RTEXT/camera — possibile riferimento pendente nel DWG estratto
Task 8: review dispatched (opus)
Task 8: review (opus): ❌ 1 Important — stili di quota (DIMTXSTY, frecce DIMBLK*, DIMLTYPE/DIMLTEX*) e style_handle dei tipi di linea complessi arrivano in out con handle di src (orfani o sbagliati); frecce _ArchTick diventano piene in silenzio; dimstyle Standard di src non copiato; Layer.plotstyle_handle stessa famiglia
- R31 Ruling: si corregge (non si escludono le quote, che sono essenziali in pianta): passaggio dopo la copia sul modello di copy_mline_styles — per ogni dimstyle in out si risolvono per nome in src stile di testo, tipi di linea e blocchi delle frecce, si assicurano in out e si ripuntano gli handle; valori del dimstyle Standard copiati da src (come R27); style_handle dei tipi di linea complessi ripuntato per nome; plotstyle_handle dei layer ripuntato per nome se il placeholder esiste in out, altrimenti NULL — l'estrazione promette DWG puliti per AutoCAD — se sbagliato: lavoro in più, nessun rischio sui dati.
- R32 Ruling (⚠️ della review): restano nell'estratto 3DFACE/POLYLINE 3D/mesh ed entità con Z≠0 (si copiano bene, "solo 2D" vuol dire piante, non modello) e arrivano solo i layer usati (percorso WBLOCK, coerente col brief) — se sbagliato: un layer vuoto del rilievo non c'è nell'estratto.
Task 8: minor (deferred): scrittura parziale se il 2° file fallisce (DWG aperto in AutoCAD) e Io nomina solo il file fallito (extract.rs:642-645) — DA TRIARE; zone dentro un blocco finiscono nel fatto senza avviso; kernel chiamato due volte e filtro BLOCK/ENDBLK duplicato da wblock.rs; avvisi senza motivo e "Layout … non copiata" al femminile (report.rs:111); test mancanti (underlay, OLE, REGION…, blocchi annidati, 3D in blocco, layer 0 congelato con CLAYER); riferimenti "Task 12"/"R30" nel codice; struttura da dividere al Task 12; is_ours toglie anche APPID ARCHLINE_* di terzi (vincolo globale)
Task 8: minor (deferred): override per quota (XDATA ACAD/DSTYLE) con handle di src; quote dentro blocchi copiati per una quota non ripuntate; INSERT in *D con blocco assente anche in src non elencato — DA TRIARE
Task 8: fix round 1/5 (1 addressed, 0 open — stili di quota, tipi di linea complessi, stili di stampa, frecce nei *D; commits ef1e528..0f09f60)
Task 8: minor (deferred): dimtxsty non risolto → NULL invece dello Standard di out; ripieghi su NULL senza avviso; EntityCommon.plotstyle_handle/color_book_handle delle entità e xref_block_record_handle di layer/tipi di linea scritti tali e quali; quote dentro blocchi uniti per una quota non ripuntate
Task 8: complete (commits b9d4514..0f09f60, review clean)
Task 9: dispatched (BASE 0f09f60, implementer opus)
Task 9: implementer DONE_WITH_CONCERNS 4f6899e
- R33 Ruling: accettati gli scostamenti dal brief del Task 9: tab_id nella finestra, on_bim pub(in crate::app), cartella chiesta con FreeTextValuePromptCommand (spazi ammessi in un .scr), etichetta «Elaborati» tolta per stare in 300 px, AL_ESTRAI non rifiuta lo spazio carta (legge solo il modello, non crea nulla), messaggio Io con i file già scritti — se sbagliato: solo forma.
Task 9: minor (deferred): cartella con spazi non passa da run/--serve (il runner divide sugli spazi); estrazione sincrona (l'interfaccia si ferma su disegni grandi); Invio nella finestra non preme OK
Task 9: review dispatched (sonnet)
Task 9: review: spec ✅, Approved ma 1 Important — avvisi (Notice) non testati nell'app e persi quando write fallisce con Io dopo scritture parziali → fix round 1
Task 9: minor (deferred): nessun test del cambio tab con finestra aperta e di Picked(Some) che ritira la conferma; solo "Stato di progetto" spuntato → "scritti 0 file" e layer ricordato con passo di annullamento; layer ricordato anche se inesistente; TempDir con assert "senza spazi" per tutti i test; duplicazioni folder_error/unità; doppio clic veloce su OK sovrascrive senza leggere l'avviso (etichetta "Sovrascrivi" al secondo passo, plan-mandated) — DA TRIARE; pick_list accanto al text_input non dichiarato
- R34 Ruling: accettata la variante WriteError::Partial { written, message } (interfaccia nostra del Task 8; Io ora = niente scritto) — se sbagliato: solo forma.
Task 9: fix round 1/5 (1 addressed, 0 open — avvisi anche dopo scrittura parziale, test; commits 4f6899e..521cc05)
Task 9: minor (deferred): dopo una scrittura Partial il layer dei muri nuovi non viene ricordato
Task 9: complete (commits 0f09f60..521cc05, review clean)
PR 2 (Task 8-9) completa sul branch.
Task 10: dispatched (BASE 521cc05, implementer sonnet)
Task 10: implementer DONE_WITH_CONCERNS a1522db — trappola per il Task 11: per i segmenti con bulge negativo le curve del kernel sono archi antiorari con estremi scambiati, quindi il t di clip (verso dell'entità) va passato come [1-t1, 1-t0]
- R35 Ruling: il contratto di kept_pieces si allinea a quello che il Task 11 ha in mano: gli intervalli di `removed` sono nel parametro normalizzato 0..1 della curva BoundarySource.curves[k] nel suo verso e nella sua parametrizzazione (quella che restituisce kernel::geom2d::inside_spans, normalizzata); clip fa da sé l'inversione per i bulge negativi — una convenzione che il chiamante deve ricordarsi di invertire è un errore silenzioso in attesa — se sbagliato: nessuno, cambia solo dove sta l'inversione.
- R36 Ruling: accettati handle NULL anche senza tagli (il Task 11 tiene gli originali non toccati in kept_originals), azzeramento di xdictionary/reactors/raw_record/graphic_data dei pezzi, MIN_PIECE_REL = 1e-9 della lunghezza, POLYLINE 2D resta tale — se sbagliato: nessuno.
Task 10: correzione pre-review R35 inviata all'implementer
Task 10: correzione R35 67962e3 (intervalli di inside_spans passati così come sono); review dispatched (sonnet)
Task 10: review: spec ✅, quality Approved
Task 10: minor (deferred → portato nel Task 11): MIN_PIECE_REL 1e-9 molto sotto la tolleranza dei tagli: con il bordo di una zona su un vertice possono restare tratti di 0,1-1 µm; il Task 11 deve agganciare i t alla tolleranza e testare il bordo su un vertice
Task 10: minor (deferred → Task 12): i pezzi tengono layer_handle/linetype_handle/plotstyle_handle… dell'originale; in un documento nuovo vanno risolti per nome
Task 10: minor (deferred): mirrors_into_xy ricostruisce la curva intera; POLYLINE 2D curve/spline-fit tagliate sui vertici guida; test same_ends accetta entrambi i versi
Task 10: complete (commits 521cc05..67962e3, review clean)
- R37 Ruling: nel test point_and_uncuttable_entities la SPLINE che attraversa la zona demolita non è un'entità muro: non sta in drawn/kept_originals (che sono solo muri) né in excluded, e produce NonTagliata; la porta nello stato di progetto il Task 12 con le altre entità non muro — il piano scriveva "in kept/drawn intera", in contraddizione con la definizione di kept_originals (entità muro non toccate) — se sbagliato: nessuno, la SPLINE arriva comunque intera nel DWG di progetto.
Task 11: dispatched (BASE 67962e3, implementer opus)
Task 11: implementer DONE_WITH_CONCERNS d156fa9 — scelte oltre il brief S1-S8
- R38 Ruling: accettate S1 (zona con asse il cui contorno non è più il rettangolo dell'asse = zona senza asse), S2 (ConfiniNonRimossi anche quando un muro arriva al bordo di una zona senza asse o lo attraversa), S3 (zone doppie contano una volta), S4 (lato di estremità dentro un'altra zona demolita: niente linea né avviso), S5 (da un lato nuovo si toglie solo il tratto che coincide con un muro esistente), S7 (entità non muro tutta dentro una zona demolita → excluded + Esclusa), S8 (entità AL-* o con XDATA ARCHLINE_* fuori dalle regole 5-6), handle sintetici sopra il massimo del disegno — tutte vanno verso "mai in silenzio" o sono più precise della lettera del brief — se sbagliato: S7 toglie dal progetto un'entità (es. campitura del muro demolito) che Mauro voleva, ma lo dice il rapporto.
Task 11: review dispatched (opus)
Task 11: review (opus): spec ✅ alla lettera; 2 Important plan-mandated — (1) muro innestato a T sul lato lungo di una zona demolita resta aperto senza avviso (aree sbagliate in silenzio); (2) facce non parallele all'asse (entro 0,5°) lasciano mezza faccia del muro demolito nel progetto, senza avviso specifico
- R39 Ruling: sui lati lunghi di ogni zona demolita con asse, gli estremi delle curve muro rimaste che stanno entro tol dal lato e lontani dagli angoli si ordinano lungo il lato e si accoppiano in sequenza (1-2, 3-4…): ogni coppia riceve una LINE di chiusura con gli attributi della curva del primo estremo (è la testa del muro innestato rimasta scoperta); un estremo spaiato → TaglioNonChiuso — è la stessa logica delle chiusure della regola 2 (spec 5.4.2), non una pulizia di giunzioni — se sbagliato: una chiusura in più su un muro innestato, visibile nel DWG.
- R40 Ruling: (a) rect_from_axis (walls.rs) orienta il rettangolo sulla direzione media delle facce trovate (asse riproiettato sulla nuova mezzeria), così le facce stanno sui lati lunghi entro tol anche con un asse cliccato a occhio; i test del Task 4 si adeguano; (b) rete di sicurezza nella regola 1: un tratto rimasto parallelo entro FACE_PARALLEL_DEGREES a un lato lungo, nella sua fascia (distanza ≤ tan(0,5°)·|asse| + tol) e sovrapposto all'intervallo dell'asse → nuovo avviso Notice::FacciaNonTolta { at } "Faccia del muro demolito non tolta in (x; y): facce non parallele all'asse." (muri a spessore variabile) — mai mezza faccia in silenzio — se sbagliato: un avviso in più su muri a spessore variabile.
Task 11: minor (deferred): S4 troppo largo (salta la chiusura anche se il lato di estremità tocca solo il bordo di una zona demolita senza asse, project.rs:728-735); same_zone ignora axis/thickness/varco; test mancanti (S8 con campiture nello spazio modello, regola 5 su INSERT/MTEXT e normale −Z, disegni in mm/cm, demolizione parziale in ChiusaPerMuro, S5 parziale); model_space duplicata da walls.rs:59; regola 6 senza prefiltro a riquadri; pezzi con XDATA dell'originale e chiusure senza (il Task 12 li toglie)
Task 11: fix round 1 applicato 809ce66 (R39, R40a, R40b) — punti aperti D1-D3
- R41 Ruling (D1): rect_from_axis segue la direzione delle facce solo quando le due facce sono parallele tra loro (entro FACE_PARALLEL_DEGREES fra loro… più stretto: entro tol sullo spessore lungo l'asse); altrimenti resta sull'asse cliccato e FacciaNonTolta segnala le parti rimaste — il verso medio su un muro a spessore variabile lasciava mezza faccia per lato più tre TaglioNonChiuso — se sbagliato: su muri a spessore variabile resta l'avviso, mai un taglio sbagliato in silenzio.
- R42 Ruling (D2): una coppia di R39 si chiude solo se le due curve sono parallele tra loro e distano ≤ MAX_WALL_THICKNESS_M; altrimenti TaglioNonChiuso per entrambi gli estremi — evita di chiudere in silenzio il tratto fra due muri diversi — se sbagliato: un avviso al posto di una chiusura giusta su un muro innestato più spesso di 1,50 m.
- R43 Ruling (D3): accettata la fascia di FacciaNonTolta (2,6 cm su 3 m): un rivestimento vicino alla faccia dà un avviso in più — se sbagliato: solo un avviso.
Task 11: emendamento al fix round 1 (R41, R42) inviato all'implementer
Task 11: emendamento 6ebc02f (R41 spessore costante entro tol, R42 coppie parallele ≤ 1,50 m, teste ordinate indipendenti dal verso dell'asse)
- R44 Ruling: accettati i limiti residui di R42 (un muro con due facce buone accanto a uno con una sola faccia resta aperto con avvisi; due teste di muri diversi parallele entro 1,50 m si chiudono tra loro) — casi rari, il primo dà avvisi — se sbagliato: una chiusura tra due muri vicini da togliere a mano.
Task 11: re-review fix round 1 dispatched (opus)
Task 11: fix round 1/5 (2 addressed, 0 open — teste a T R39/R42, zona sulle facce R40/R41, FacciaNonTolta; commits d156fa9..6ebc02f)
Task 11: minor (deferred, ALTA PRIORITÀ PER LA REVISIONE FINALE): asse di AL_DEMOLISCI che sfora nei muri ortogonali (clic a occhio): la zona entra nella massa dei muri sud/nord, R39 accoppia le facce dei locali e, con L ≤ 1,50 m, ridisegna proprio le facce demolite; in ChiusaPerMuro taglia anche la faccia y=0 del muro sud; solo TaglioNonChiuso come avvisi. Rimedio proposto: in rect_from_axis accorciare il rettangolo all'intersezione dell'intervallo dell'asse con la copertura comune delle due facce
Task 11: minor (deferred): tramezzo fuori squadra rispetto ai muri d'ambito → 4 TaglioNonChiuso e nessuna chiusura (servirebbe chiudere lungo le facce ortogonali); prova di R41 molto stretta (3,3e-7 rad su 3 m) → facce rilevate indipendenti tornano all'asse cliccato + FacciaNonTolta; un FacciaNonTolta per ogni tratto; test della parità degli estremi assente; border_through_a_vertex non controlla avvisi/chiusure; ordine dei controlli R42 / zona demolita a 886-897; nessun test con due coppie sullo stesso lato
Task 11: complete (commits 67962e3..6ebc02f, review clean)
Task 12: dispatched (BASE 6ebc02f, implementer opus)
Task 12: implementer DONE_WITH_CONCERNS b7bf295
- R45 Ruling: accettati (1) da script il layer non si salva nel record (ordine record → proposta → «0» con riga), dalla finestra solo se il progetto è scritto; (2) rifiuto dei nomi di layer non validi per AutoCAD; (3) FuoriPiano nell'estrazione del progetto e riga quando il layer non esiste nel disegno — se sbagliato: solo forma.
- R46 Ruling (4): un layer dei muri nuovi assente dal disegno si crea con colore, tipo di linea e spessore del layer muri più frequente (propose_new_walls_layer), non con i predefiniti — spec 5.5 "con colore e tipo di linea dei muri esistenti" — se sbagliato: i muri nuovi hanno l'aspetto dei muri esistenti invece del bianco continuo.
Task 12: correzione pre-review R46 inviata all'implementer
Task 12: correzione R46 f8ffc6b; review dispatched (opus)
Task 12: review: spec ✅, quality Approved
Task 12: minor (deferred): R37 non coperto da un test di estrazione (entità non muro che attraversa una zona copiata intera + NonTagliata); add_entity(..).ok() e layers.add ignorati; riga "muri nuovi sul layer 0" stampata anche se poi Exists; layer non valido nel record blocca tutti gli elaborati da script; on_rooms_layer per prefisso vs nomi esatti nella comparativa; validazione del nome duplicata (e copia di validated_symbol_name di plugin_host); extract.rs 3108 righe (test da spostare in extract_tests.rs); propose_new_walls_layer conta anche i muri fuori piano; doc che linka item privati; layer creato senza plotstyle/stati/trasparenza
Task 12: complete (commits 6ebc02f..f8ffc6b, review clean)
PR 3 (Task 10-12) completa sul branch.
Task 13: dispatched (BASE f8ffc6b, implementer opus)
Task 13: implementer DONE_WITH_CONCERNS 2b97499 — vuoto chiuso da HPGAPTOL sulla faccia sud dà 11,997 dichiarato esatto (refined_boundary_ring di upstream sposta l'angolo saldato); seme di progetto in un varco saltato senza avviso; rooms_in dà ogni locale due volte (Contorno e Testo): ai Task 14/15 va passato un Room per locale
- R47 Ruling: compute_at, quando tol > unit.tolerance(), ricalcola anche alla tolleranza di base: se lì il locale non è chiuso (o l'area differisce), il risultato chiuso grazie a HPGAPTOL ha exact = false (→ ContornoApprossimato) — un vuoto del rilievo chiuso per tolleranza non è un contorno esatto, e 11,997 dichiarato esatto viola "mai un numero sbagliato come esatto"; walls::net_area e upstream non si toccano — se sbagliato: i locali chiusi con HPGAPTOL escono con l'avviso "contorno curvo approssimato" (testo improprio ma non silenzioso).
- R48 Ruling: un seme di progetto che cade in un varco dà un avviso nuovo Notice::SemeNelVarco { name } "Il locale di progetto {name} cade nel varco: non nomina nessun locale." — mai in silenzio — se sbagliato: un avviso in più.
- R49 Ruling: accettati walls::face_regions (stessa regola di region_at), il ripiego triangulate_rings, LocaleFuso anche con 2+ semi di progetto nella stessa faccia, seme senza regione → LocaleNonChiuso, escape di \ { } nell'MTEXT, seme di fatto in un muro nuovo senza avviso proprio (le due metà danno LocaleSenzaNome) — se sbagliato: solo avvisi.
Task 13: correzione pre-review R47+R48 inviata all'implementer
Task 13: correzione R47/R48 6798fb2 (compute_at_with_base); review dispatched (opus)
Task 13: review (opus): spec ✅; 1 Important (da R3) — vuoto d'angolo in un locale collegato a un altro locale chiuso (spallette e soglia): la catena aperta tocca un buco, R3 non la vede, 19,17 m² invece di 12,00 dichiarati esatti con HPGAPTOL = 0 → fix round 1
- R50 Ruling (sostituisce R3): un locale è "non chiuso" se i segmenti muro che stanno strettamente dentro la regione (punto medio dentro, distanza > tol da anello esterno e buchi, cioè con la regione da tutte e due le parti) formano un gruppo connesso lungo in totale più di 0,50 m (unit.metres(0.5)); i gruppi più corti (sbordi, monconi, battute) si ignorano; i gruppi staccati di R3 sono un caso particolare — copre spallette, soglie, ponti — se sbagliato: una linea muro libera più lunga di 50 cm dentro un locale lo rende "non chiuso" (messaggio, mai numero sbagliato).
- R51 Ruling (promossi nel giro perché violano gli anti-obiettivi della spec: dato sbagliato o perdita silenziosa): (a) un locale chiuso solo dalla tolleranza (R47) dà un avviso proprio Notice::ChiusoPerTolleranza { name } "Locale {name}: chiuso solo dalla tolleranza HPGAPTOL, superficie da verificare." invece di ContornoApprossimato ("curvo"); (b) le facce contenute in un buco qualsiasi di un locale calcolato non diventano "(senza nome)" (pilastri a doppio contorno, cavedi); (c) un seme in una faccia coperta da una zona Demolito senza varco dà Notice::SemeInDemolizione { name } "Il locale {name} cade in una zona demolita: non nomina nessun locale."; (d) project_rooms deduplica i semi per (nome, seme entro tol), così Contorno+Testo dello stesso locale non danno "A + A" — se sbagliato: avvisi in più.
Task 13: minor (deferred): LocaleFuso con 2+ semi di progetto dice "da confermare" ma l'etichetta no; "lasciato com'era" impreciso per un seme di progetto; test mancanti (LocaleNonChiuso "(senza nome)", mm/cm, accenti nel giro DWG); closure compute_at che nasconde la funzione pubblica; bounds/box_within/boxes_overlap/ring_curves doppioni di walls.rs; altezza del testo non valida → 1,0 senza avviso
- R52 Ruling: accettati without_spurs (le punte dei monconi attaccati al contorno si tolgono dagli anelli: l'area non cambia, il contorno disegnato non segue il moncone), measure_at al posto di compute_at_with_base, parete a una linea > 0,50 m dentro un locale → non chiuso (costo dichiarato di R50) — se sbagliato: il contorno del locale non disegna i monconi.
Task 13: fix round 1/5 (5 addressed, 0 open — R50 non chiuso con muri dentro > 0,50 m, R51 a-d; commits 6798fb2..4c9e364)
- R53 Ruling: confermato che un ramo muro attaccato al contorno e lungo più di 0,50 m (parete a una linea, muretto, fianco d'armadio, simbolo di porta su un layer muri acceso) rende il locale "non chiuso" — messaggio, mai numero sbagliato; i simboli di porta vanno su layer non muro (spec 5.0) — se sbagliato: Mauro spegne il layer o chiude con una soglia.
Task 13: minor (deferred, DA TRIARE): R51(b) zittisce anche un locale vero senza seme dentro un'isola (box bagno a doppia linea in mezzo a un locale): prima usciva "(senza nome)" con avviso, ora niente — proposta: elencarlo con LocaleSenzaNome senza disegnarlo
Task 13: minor (deferred): filtro delle isole dei locali senza nome solo al secondo passaggio (LocaleNonChiuso "(senza nome)" possibile prima); nessun test di R50 in cm/mm; has_walls_inside senza sweep (costo su rilievi grandi, raddoppiato da R47); avviso doppio LocaleNonChiuso{A} + LocaleNonChiuso{(senza nome)} per lo stesso spazio aperto
- R54 Ruling (dall'osservazione fuori giro della re-review, portata nel Task 14): un vuoto con i due lati attaccati ad altri anelli (tramezzo che si ferma 2 mm prima del muro nord) fonde A e B senza che R50 lo veda (≈ 23,70 m² esatti nel fatto); in AL_LOCALE e AL_AGGIORNA, se la regione trovata contiene il seme di un altro locale di fatto, si dà LocaleFuso con i due nomi — se sbagliato: un avviso in più quando due locali condividono davvero la stessa regione.
Task 13: complete (commits f8ffc6b..4c9e364, review clean)
Task 14: dispatched (BASE 4c9e364, implementer opus)
Task 14: implementer DONE_WITH_CONCERNS ed152f7 — R54 in AL_AGGIORNA: i locali fusi si aggiornano comunque sull'area unita (A e B scrivono entrambi 23,70)
- R55 Ruling: in AL_AGGIORNA, se la regione di un locale contiene il seme di un altro locale di fatto, NESSUNO dei due si aggiorna (restano com'erano, con l'etichetta precedente) e si dà LocaleFuso con i due nomi + riga "lasciati com'erano" — riscrivere 23,70 su entrambi sovrascrive un dato corretto con uno sbagliato (anti-obiettivo della spec); in AL_LOCALE (nuovo locale) resta l'avviso LocaleFuso e il prompt del nome — se sbagliato: un locale che davvero si è unito a un altro va ridisegnato a mano.
- R56 Ruling: accettati ContornoMancante e TestoMancante, i locali su layer bloccato saltati e contati, avvisi di AL_LOCALE nel prompt del numero, AL_LOCALE di progetto senza rifiuto per seme in varco o zona demolita (li segnala l'estrazione con SemeNelVarco/SemeInDemolizione), pari-dispari sui muri nuovi.
- R57 Ruling (per il Task 15): ExtractOptions guadagna `room_tol` = max(unit.tolerance(), HPGAPTOL) per i locali; `tol` resta per muri e geometria.
Task 14: correzione R55 inviata all'implementer
Task 14: correzione R55 6522dc5 (Notice::LocaliFusiLasciati); review dispatched (opus)
Task 14: review (opus): spec quasi ✅; 1 Important (lacuna del piano) — AL_AGGIORNA può riscrivere un'area cresciuta in uno spazio senza seme (soglia cancellata, layer spento) dicendo solo "n locali aggiornati"
- R58 Ruling: AL_AGGIORNA stampa una riga per ogni locale cambiato "Locale {nome}: {vecchia} → {nuova} m²." (area vecchia dal contorno precedente; omessa se non calcolabile) — tracciabilità di ogni numero riscritto — se sbagliato: righe in più.
- R59 Ruling (promossi nel giro): (a) "AL_AGGIORNA: nessun locale di fatto nel disegno." con test, senza chiedere l'unità; (b) il contorno si confronta alla tolleranza di base (non HPGAPTOL) per decidere se aggiornarlo — altrimenti etichetta nuova e contorno vecchio; (c) locale fuso in AL_LOCALE scritto con confirm = true → "(da confermare)" nell'etichetta; (d) Notice::Isole al singolare ("1 isola sottratta"); (e) refactor puro prima del Task 15: RoomCommand → src/bim/room_command.rs, refresh_rooms/RoomEdit/fatto_parts → src/bim/refresh.rs, copie ring_curves/region_holds/strictly_in rimosse rendendo pub(crate) quelle di rooms.rs — in un commit a parte, senza cambi di comportamento; l'estrazione non deve dipendere dal modulo dei comandi interattivi.
Task 14: minor (deferred): "Locali Nuovo + A fusi" quando si rifà AL_LOCALE in un locale già nominato (il testo manda a cercare un vuoto che non c'è, e AL_AGGIORNA poi li lascia fermi); room_text_height calcolata due volte; project_geometry ricalcolata a ogni AL_LOCALE anche in fase Fatto; TEXTSIZE 2,5 in un disegno in metri dà testi alti 2,5 m; contorno convertito in POLYLINE → "manca il contorno"; nessun test con muro ad arco in AL_AGGIORNA (aggiunto se facile)
Task 14: fix round 1/5 → re-review: R58/R59 a-e + spostamento ADDRESSED; 1 Important nuovo — l'area "di prima" della riga R58 è lorda (il contorno scritto è l'anello esterno, senza isole): con pilastro la riga dice "12,00 → 11,61" invece di "11,91 → 11,61", e con il layer del pilastro spento "12,00 → 12,00" mentre l'etichetta passa da 11,91 a 12,00
- R60 Ruling: l'area di prima della riga R58 si legge dall'etichetta di prima (il "= N,NN m²" scritto da noi con format_m2), mai dal contorno; se il testo non c'è o non si legge, la riga è "Locale {nome}: aggiornato a {nuova} m²."; se vecchio e nuovo testo coincidono ma il contorno è cambiato → "Locale {nome}: contorno aggiornato ({nuova} m²)."; test con pilastro (layer spento / spostamento del tramezzo) e test AL_AGGIORNA di un locale "(da confermare)" (resta invariato nel gruppo fuso, perde la nota quando il vuoto si chiude); l'uscita "nessun locale di fatto" azzera bim_pending.
Task 14: correzione R60 (fix round 2) inviata all'implementer
Task 14: fix round 2/5 (1 addressed, 0 open — area di prima dall'etichetta; commits 2747dba..ecdb17e). Totale Task 14: bd31b2e..ecdb17e
Task 14: minor (deferred): riga "23,70 → 23,70 m²" quando cambia solo la nota "(da confermare)" o il nome (userebbe ContornoAggiornato / "etichetta aggiornata"); il parser dell'etichetta non richiede il marcatore \PS (un'etichetta scritta a mano che finisce con "= 5,00 m²" verrebbe letta); la nota "(da confermare)" viene tolta anche quando resta la regione unita dopo la cancellazione dell'altro locale
Task 14: complete (commits 4c9e364..ecdb17e, review clean)
- R61 Ruling (per il Task 15): `extract` ricalcola i locali di fatto con `refresh::refresh_rooms` (e applica `RoomEdit` sul documento `work`), con tolleranza `room_tol` = max(unit.tolerance(), HPGAPTOL) per i locali; ExtractOptions guadagna `room_tol`; le app passano gap_tolerance(). I locali fusi (R55) e non chiusi restano com'erano e danno gli avvisi — l'estratto non contiene mai un numero che il disegno di lavoro non avrebbe dopo AL_AGGIORNA.
Task 15: dispatched (BASE ecdb17e, implementer opus)
Task 15: implementer DONE_WITH_CONCERNS e9e27b1
- R62 Ruling: accettato che l'estratto di fatto ricalcoli anche i locali su layer bloccato (AL_AGGIORNA li salta nel disegno di lavoro): "ricalcolati" della spec 5.5 riguarda l'estratto, e la differenza con l'etichetta scritta è riportata dalla riga EtichettaDaAggiornare — se sbagliato: l'estratto mostra un numero diverso dall'etichetta bloccata, con la riga che lo dice.
- R63 Ruling: accettati senza semi niente locali di progetto (niente riga Varco), i file toccati fuori elenco (refresh.rs, rooms.rs, room_command.rs, report.rs, app/commands/bim.rs), avvisi di fatto e progetto in un solo elenco senza doppioni.
Task 15: minor (deferred): il testo "lasciato com'era" di LocaleNonChiuso non è preciso per l'estratto di progetto; area del varco non riportata se il disegno non ha nessun seme di locale
Task 15: review dispatched (opus)
Task 15: review: spec ✅, Approved
Task 15: minor (deferred, DA TRIARE nella revisione finale): (1) esclusione dei locali per layer ma riconoscimento per XDATA (rooms_in): un locale di fatto spostato a mano su un altro layer finisce nel DWG di progetto come etichetta vecchia accanto a quella ricalcolata (due numeri per lo stesso locale) — rimedio economico: escludere anche gli handle di rooms_in(src) nei tre rami; (2) lo spec §5.4 dice che una regione senza seme esce "(senza nome)", ma project_rooms (Task 13) la produce solo vicino a una zona o a un locale di fatto: in un disegno con soli semi di progetto una regione senza nome non esce né è elencata; (3) misura delle regioni di fatto duplicata fra extract.rs:284-306 e refresh.rs:163-185, ensure_rooms_layer duplica ensure_new_walls_layer; (4) due fatto_rooms con semantiche diverse (per seme / per nome+seme); (5) extract.rs 3886 righe (test da spostare in extract/tests.rs); (6) LocaleNonChiuso fuso fatto/progetto con testo impreciso
Task 15: complete (commits ecdb17e..e9e27b1, review clean)
PR 4 (Task 13-15) completa sul branch.
Task 16: dispatched (BASE e9e27b1, implementer sonnet)
- R64 Ruling: accettati il Sep dopo il sottomenu (prima di UCS) e l'aggiornamento di NO_ICON con i sei AL_* — se sbagliato: una riga da togliere. Task 16 review dispatched (sonnet)
Task 16: review: spec ✅, Approved
Task 16: minor (deferred → CLAUDE.md/spec): spec workspace-design riga 72 ("Alto, riga 3 | Insert, Reference") e commento di autocad_classic() disallineati; chi ha già un layout salvato non vede la barra finché non riseleziona AutoCAD Classic (avviso "Toolbar changes were not saved…" innocuo); WorkspaceSet::first_run può mettere in "My Workspace" chi non ha mai avviato i workspace (non riguarda Mauro)
Task 16: complete (commits e9e27b1..32338fc, review clean)
Task 17: dispatched (BASE 32338fc, implementer sonnet)
Task 17: implementer DONE 952464b (32 controlli OK); review dispatched (sonnet)
Task 17: review: spec ✅, Approved (32/32 OK ripetibile; isolamento dei config verificato su ogni spawn; XDATA rilevato anche in blocchi e APPID)
Task 17: minor (deferred): isolamento non asserito a runtime (M1: verificare che <work>/config/Roaming/OpenCADStudio esista); macOS non isolato (HOME) e README non lo dice; solid_fill==1 debole (aggiungere pattern_name == "SOLID"); il record ARCHLINE_PROGETTO non è controllato nel rootdict; "solo spazio modello" vuoto (nessuna entità in Layout1; dichiarato); nessun caso positivo XDATA sul percorso DWG→DXF; test lasciano cartelle bim_test_* in TEMP; README dice ~40 s ma la suite ora ~55 s
Task 17: complete (commits 32338fc..952464b, review clean)
TUTTI I 17 TASK COMPLETI. Restano: aggiornamento CLAUDE.md, revisione finale dell'intero branch, fix finale unico.
CLAUDE.md aggiornato 8332a51 (+26 righe, trappola 9, Aperti)
=== REVISIONE FINALE (fable) 3c9e095..8332a51: With fixes - nessun Critical, 5 Important ===
Final: Important 1 (a) rect_from_axis: asse che sfora nei muri ortogonali -> pianta di progetto sbagliata (buchi di 10 cm nelle facce dei muri esterni, TaglioNonChiuso, R39 ridisegna le facce demolite, locale unito non chiuso)
Final: Important 2 (e) doppio OK sovrascrive senza leggere (cancella i ritocchi fatti in AutoCAD sul DWG di progetto)
Final: Important 3 (b) locale di fatto spostato su un altro layer: due etichette con numeri diversi nel DWG di progetto e nella comparativa; seme di progetto spostato finisce nel fatto
Final: Important 4 (f) rectangle_midline: soglia relativa 1e-9, su disegni georiferiti con asse ruotato la tenaglia (NuovoSuEsistente) si spegne in silenzio
Final: Important 5 il registro delle decisioni (R2..R64) e citato da CLAUDE.md e dai commenti ma sta in .superpowers/ (fuori da git)
- R65 Ruling: UN SOLO fix finale con i 5 Important e i Minor economici nei file toccati (read_zone ordina gli anelli per area; add_drawn/layers.add che falliscono danno NonCopiata; bim_set_unit con drop_unchanged_pending_history; room_text_height ripiega a 0,25 m se TEXTSIZE > 1 m; zone doppie con varco diverso tengono varco = true; CLAUDE.md limiti noti; spec 5.4 emendata sul senza nome). Il resto del triage (DOPO/LASCIARE) resta elencato come limiti noti - se sbagliato: difetti minori noti ma non corretti in questo giro.
- R66 Ruling: il registro delle decisioni viene copiato in docs/superpowers/ledgers/2026-10-23-bim-stato-piante.md (istantanea, senza dati di clienti) e CLAUDE.md punta li - se sbagliato: un file di documentazione in piu nel repo.
Fix finale: dispatched (opus, fresh implementer)
