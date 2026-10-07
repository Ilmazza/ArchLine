# Banco di conformità DXF/DWG con oracolo ezdxf

Stato: design approvato dall'utente in chat il 2026-10-07; spec in attesa di revisione. Branch `lavoro`.

## 1. Scopo

Misurare nel tempo quanto fedelmente l'eseguibile ArchLine legge e scrive DXF/DWG, usando **ezdxf come oracolo
indipendente** (un codec che rilegge ciò che scrive non prova nulla sul formato). Serve come rete di sicurezza prima
del plugin architettonico e a ogni merge da upstream.

Ruolo scelto dall'utente: **misura + report, mai bloccante**. Quando i difetti noti saranno sistemati, il banco può
essere promosso a cancello in CI (si toglie `continue-on-error`), senza cambiare il resto.

Non è un test Rust: misura un eseguibile compilato, non una libreria, quindi resta fuori da `cargo test`
(2141 test) e non tocca lo stack di `rustc`.

## 2. Origine del materiale

Si parte da `Ilmazza/Autocad_clone` (repo privato dell'utente): `tests/oracle/run_oracle.py` (scenari sintetici
ezdxf), `tests/conformance/ocs_baseline.py` (confronto via `--export`), `docs/baseline-ocs-f0.md` (risultati del
2026-10-03). Si porta **solo codice che genera disegni sintetici**: nessun file di terzi, nessun disegno reale
dell'utente (`Base_strutturali.dwg` resta fuori dal repo, che è pubblico).

L'app ha già tutto il necessario: `OpenCADStudio --export IN OUT [--target-version V]` esce prima di creare la
finestra (`src/main.rs`, `app::export_headless`), quindi gira senza GPU.

## 3. Struttura (file tutti nuovi, nessun punto d'innesto in codice upstream)

```
tests/conformance/
  run.py            orchestratore e CLI: genera → converte → confronta → classifica → report
  classify.py       stati, Result, lettura e scrittura di expected.json (puro, senza ezdxf)
  report.py         report Markdown (puro, senza ezdxf)
  oracle.py         funzioni ezdxf: leggibilità strict, estensioni, tipi di entità, stringhe
  cases/            un modulo per scenario (vedi §4); ognuno espone NAME e build(version, work) -> Path
  expected.json     esito noto per (caso, versione, percorso, controllo)
  fake_converter.py convertitore finto che imita --export, solo per i test
  test_bench.py     test del banco
  README.md         uso, lettura del report, aggiornamento di expected.json
```

Uso: `python tests/conformance/run.py target/release/OpenCADStudio.exe [--out report.md] [--strict]
[--update-expected --note "..."] [--case NOME] [--keep DIR]`. Dipendenze: Python 3 e `pip install ezdxf`. Senza ezdxf esce con
codice 77 (saltato), come l'oracolo di partenza. I disegni sono generati in una cartella temporanea; il repo non viene
scritto, salvo `report.md` e, solo con `--update-expected`, `expected.json`.

## 4. Casi

Da `run_oracle.py`: geometria (blocchi annidati, array ruotato, scale negative, estrusione −Z, bulge, polilinee,
solido), hatch (solido con bulge, pattern ANSI31, isola, ellisse, cerchio), quote (lineare, allineata, raggio,
diametro, angolare), colori e layer in blocchi annidati (BYBLOCK/BYLAYER/layer 0), paper space e layout, attributi.
Ogni caso è generato in **R2000** (AC1015) e **R2018** (AC1032).

Anche gli scenari `text` (TEXT con codici `%%`, allineamenti, MTEXT con formattazione) e `hatch_spline` (hatch con
contorno a spline: la baseline del 03/10 ha visto i punti di fit passare da 3 a 0 in R2000; riserva già annotata: la
spline sintetica ha solo punti di fit, cosa che AutoCAD di norma non scrive) esistevano già nello script di partenza e
sono stati portati. Il caso davvero nuovo è `text_accents`: `è à ò ù ì é` in TEXT, MTEXT e ATTRIB (R2000 con
`$DWGCODEPAGE = ANSI_1252`, R2018).

Percorsi per ogni caso (senza `--target-version`, l'export conserva la versione del documento letto:
`export_headless` usa `doc.version`):
1. DXF → DXF (`--export`)
2. DXF → DWG → DXF (due `--export`)

Il percorso 2 verifica DWG **solo in modo indiretto**: ezdxf non legge DWG, quindi il DWG scritto da ArchLine viene
riletto da ArchLine stesso e confrontato dopo l'export in DXF. Il report lo dichiara in testa, ogni volta.

## 5. Controlli e stati

Per ogni (caso, versione, percorso) si eseguono quattro controlli, tutti con ezdxf come riferimento:
`readable` (ezdxf legge il file in modalità strict), `types` (conteggio dei tipi di entità del model space),
`extents` (estensioni, tolleranza 2e-3), `strings` (TEXT/MTEXT/ATTRIB, accenti inclusi).

Le **estensioni sono calcolate senza testi** (TEXT, MTEXT, ATTDEF e gli ATTRIB degli INSERT): ezdxf stima l'estensione
di un testo dalla lunghezza della stringa, che non è un dato geometrico, e senza questa scelta un difetto sugli accenti
compariva anche come difetto di estensione. I testi si confrontano solo nel controllo `strings`. Il dettaglio di
`strings` riporta **tutte** le stringhe diverse: un dettaglio parziale mascherava le regressioni nelle altre.

Ogni controllo ha uno di quattro stati, confrontando l'esito misurato con `expected.json`:

| Stato | Significato | Effetto |
|---|---|---|
| **OK** | coincide con la sorgente | nessuno |
| **Atteso** | diverge, ed è in `expected.json` con una nota (es. POLYLINE→LWPOLYLINE, benigno; accenti R2000, difetto) | segnalato in tabella, non è errore |
| **Regressione** | peggiore di quanto registrato: era OK e ora diverge, è una divergenza nuova, o il dettaglio è diverso da quello registrato (i dettagli non hanno un ordine, quindi non si può dire quale diverga di meno) | exit 1 con `--strict`; sempre evidenziato |
| **Migliorato** | era una divergenza registrata e ora il controllo è OK | segnalato; si aggiorna `expected.json` con `--update-expected` (mai in automatico) |

`expected.json` **nasce dalla prima esecuzione sull'eseguibile attuale**: i valori sono quelli misurati, non
trascritti dalla baseline. La baseline del 2026-10-03 serve solo da controllo di coerenza (le stesse divergenze note
devono ricomparire). Ogni voce «atteso» porta una nota di testo; una voce senza nota è rifiutata da `run.py`.
`--update-expected` richiede `--note "motivo"` davanti a divergenze nuove o cambiate e, senza, non scrive nulla; le
voci invariate tengono la loro nota, quelle ora OK vengono tolte, e le note si rifiniscono a mano.

Exit code: 0 (nessuna regressione, o senza `--strict`), 1 (regressione con `--strict`), 77 (ezdxf mancante), 2
(errore del banco: eseguibile assente, conversione andata in crash).

## 6. Report

Tabella Markdown su stdout e, con `--out`, in `report.md`: intestazione con versione dell'eseguibile
(`OpenCADStudio --version`), versione di ezdxf, data, avvertenza sul percorso DWG indiretto; poi una riga per caso con
gli stati dei quattro controlli nei due percorsi e nelle due versioni; in fondo l'elenco di regressioni e
miglioramenti, e il conteggio per stato.

## 7. CI

In `.github/workflows/archline-windows.yml`, dopo «Package» e prima del caricamento dell'artefatto: `setup-python`, poi un
passo che installa ezdxf, lancia i test del banco e il banco con `--out dist\conformance-report.md`, con
`continue-on-error: true`. Il report entra
nell'artefatto già pubblicato (`ArchLine-windows-N`). Nessun nuovo workflow, nessun segreto.

## 8. Test del banco

Il banco ha un test di sé stesso (`tests/conformance/test_bench.py`, `python -m unittest`), senza bisogno
dell'eseguibile di ArchLine:
- la classificazione dei quattro stati su coppie (misurato, atteso) costruite a mano;
- il rifiuto di una voce «atteso» senza nota, e le regole di `--update-expected`;
- il rilevamento di una divergenza reale, con `fake_converter.py` al posto dell'eseguibile: prova che il banco *può*
  fallire, non solo passare. Il convertitore finto ha quattro modi (`FAKE_CONVERTER_MODE`): `faithful`,
  `corrupt-accents`, `drop-entity`, `crash`;
- i casi limite: eseguibile assente, caso inesistente, ezdxf mancante (exit 77), console Windows cp1252, conversione in
  crash che non ferma gli altri casi.

## 9. Fuori da questo giro

Comandi di modifica pilotati dall'API di automazione, rendering e stampa, disegni reali dell'utente, prestazioni,
correzione dei difetti trovati, segnalazione a monte della codepage R2000 (è un punto a parte dell'elenco aperti).
Cosa misurare in più si decide guardando il primo report.

## 10. Rischi

- La build debug non basta a misurare la velocità, ma qui non serve: si misura la fedeltà, non le prestazioni.
- Il primo `expected.json` registra anche difetti che oggi non conosciamo: è voluto, ma ogni voce «atteso» che non
  corrisponde a un difetto già noto va guardata a mano prima di essere accettata.
- `--export` di ArchLine può avere comportamenti diversi dall'apertura interattiva (stesso codec, ma non lo stesso
  percorso di codice dell'app). Il report non va letto come «l'app salva giusto», ma come «il convertitore headless
  salva giusto».
