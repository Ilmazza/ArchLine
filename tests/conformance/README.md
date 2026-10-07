# Banco di conformita DXF/DWG

Misura quanto fedelmente `OpenCADStudio --export` legge e scrive DXF/DWG, con **ezdxf come oracolo
indipendente**. Misura e riferisce: non blocca nulla (salvo `--strict`). Spec:
`docs/superpowers/specs/2026-10-07-banco-conformita-design.md`.

## Uso

```
pip install ezdxf
python tests/conformance/run.py target\release\OpenCADStudio.exe
python tests/conformance/run.py target\release\OpenCADStudio.exe --out report.md --strict
python tests/conformance/run.py target\release\OpenCADStudio.exe --case text_accents --keep C:\tmp\conf
```

Opzioni: `--out FILE` (report Markdown), `--strict` (exit 1 se c'e' una regressione), `--case NOME`
(ripetibile), `--keep DIR` (tiene i DXF generati e convertiti), `--update-expected --note "..."`.
Exit: 0 ok, 1 regressione con `--strict`, 2 errore del banco, 77 ezdxf non installato.

Una misura completa fa 54 conversioni (9 casi x 2 versioni x 3 `--export`) e impiega circa 35 secondi.

## Come leggere il report

Per ogni caso, versione (R2000, R2018), percorso (DXF>DXF, DXF>DWG>DXF) e controllo (`readable`, `types`,
`extents`, `strings`):

| Stato | Significato |
|---|---|
| OK | coincide con la sorgente |
| ATTESO | diverge, e la divergenza e' in `expected.json` con una nota |
| REGRESSIONE | peggio di quanto registrato, o divergenza nuova. Un dettaglio diverso da quello registrato conta come regressione |
| MIGLIORATO | era una divergenza registrata e ora coincide |

**Limiti**

- Il percorso DWG e' indiretto. ezdxf non legge DWG: il DWG scritto da ArchLine viene riletto da ArchLine
  stesso e confrontato dopo l'export in DXF.
- Si misura il convertitore headless (`--export`), non l'apertura e il salvataggio interattivi.
- Le **estensioni sono calcolate senza testi**: ezdxf stima l'estensione di un testo dalla lunghezza della
  stringa, che non e' un dato geometrico. I testi si confrontano solo nel controllo `strings`.
- Solo disegni sintetici generati da codice. Nessun disegno reale (il repo e' pubblico).

## expected.json

Un'entry per divergenza: `"caso|versione|percorso|controllo": {"detail": "...", "note": "..."}`. Nessuna entry
senza nota (il banco la rifiuta). Per aggiornarlo dopo una misura: `--update-expected --note "motivo"`; le entry
invariate tengono la loro nota, quelle ora OK vengono tolte, le nuove o cambiate prendono la nota passata.
Dopo, rifinire a mano le note.

## Aggiungere un caso

Un modulo in `cases/` con `NAME` e `build(version, work) -> Path` (usare `cases/_common.save`), poi aggiungerlo a
`cases.ALL`. Solo disegni sintetici generati da codice.

## Test del banco

```
python -m unittest discover -s tests/conformance -p "test_*.py" -v
```

`fake_converter.py` imita `--export` e puo' corrompere gli accenti, perdere un'entita' o andare in crash
(`FAKE_CONVERTER_MODE`): prova che il banco *puo'* fallire, non solo passare. La suite impiega circa 40 secondi.
