# Banco di conformita DXF/DWG

Misura quanto fedelmente `ArchLine --export` legge e scrive DXF/DWG, con **ezdxf come oracolo
indipendente**. Misura e riferisce: non blocca nulla (salvo `--strict`). Spec:
`docs/superpowers/specs/2026-10-07-banco-conformita-design.md`.

## Uso

```
pip install ezdxf
python tests/conformance/run.py target\release\ArchLine.exe
python tests/conformance/run.py target\release\ArchLine.exe --out report.md --strict
python tests/conformance/run.py target\release\ArchLine.exe --case text_accents --keep C:\tmp\conf
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
- Le **estensioni sono calcolate con ogni testo sostituito da un segnaposto di una lettera**: ezdxf stima
  l'estensione di un testo dalla lunghezza della stringa, che non e' un dato geometrico. Inserimento, altezza,
  rotazione e allineamento restano misurati; il contenuto dei testi si confronta solo nel controllo `strings`.
- `readable` controlla anche che la **versione DXF** del file in uscita sia quella della sorgente: un export che
  riscrivesse sempre in R2018 smetterebbe di esercitare gli scenari R2000.
- **Il caso `colors` non e' misurato da nessun controllo**: i quattro controlli non leggono colore ne layer, e
  `types` conta solo le entita' di primo livello del model space. Il caso genera BYBLOCK/BYLAYER/layer 0 in blocchi
  annidati ma un codec che li perdesse darebbe comunque OK. Serve un controllo su colore e layer (da decidere).
- Il confronto delle stringhe e' posizionale: un riordino delle entita' in uscita produce coppie disallineate.
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
`test_bim_extract.py` prova le funzioni di `bim_extract.py` (tolleranze sui segmenti, scelta della cartella senza
spazi, protocollo `--serve` con un server imitato da uno script Python); non lancia ArchLine.

## Estrazione BIM

`bim_extract.py` prova i comandi BIM di ArchLine (`AL_DEMOLISCI`, `AL_NUOVO`, `AL_ESTRAI`) su un disegno sintetico e
legge i tre DWG estratti con ezdxf come oracolo. Non usa `run.py` ne' `expected.json`: non misura una fedelta' in
percentuale ma verifica fatti precisi, e ogni controllo e' `OK` o `FALLITO` (niente `ATTESO`).

```
python tests/conformance/bim_extract.py target\release\ArchLine.exe
python tests/conformance/bim_extract.py target\release\ArchLine.exe --keep C:\ocs-bench\prova
```

Senza argomento prova `target\release\ArchLine.exe`. Exit: 0 tutti OK, 1 almeno un controllo FALLITO, 2 errore del
banco (eseguibile mancante, nessuna cartella senza spazi, l'app non risponde: la cartella di lavoro resta per la
diagnosi), 77 ezdxf non installato. Impiega pochi secondi. L'eseguibile va costruito prima
(`cargo build --release --bin ArchLine`); se e' in uso la build dice "Accesso negato".

**Cosa fa.** ezdxf costruisce il caso A (forma Continua, in metri, `$INSUNITS = 6`, DXF R2018): contorno esterno e
due locali, A di 4,00 x 3,00 m e B di 3,90 x 3,00 m, con un tramezzo da 0,10 m in mezzo; dodici `LINE` sul layer
`MURI`. `ArchLine --serve` apre il disegno ed esegue `AL_DEMOLISCI 4.05,0 4.05,3 N` (demolisce il tramezzo),
`AL_NUOVO 2,0.5 2,2.5 0.12` (muro nuovo da 0,12 m) e `AL_ESTRAI <cartella>`, che scrive
`casoA_stato-di-fatto.dwg`, `casoA_demolizioni-costruzioni.dwg` e `casoA_stato-di-progetto.dwg`. Ogni DWG si
riconverte con `ArchLine --export x.dwg x.dxf` (ezdxf non legge DWG) e si legge con ezdxf. Poi un secondo giro,
in `asse-oltre\` e con gli stessi controlli (righe con il prefisso "asse oltre le facce,"), con l'asse dato a occhio
che sfora di 0,10 m nei muri d'ambito: `AL_DEMOLISCI 4.05,-0.1 4.05,3.1 N`. La zona deve fermarsi dove finiscono
le facce del tramezzo (y da 0 a 3), quindi lo stato di progetto e' lo stesso del primo giro.

**Cosa controlla** (una riga per controllo):

- la sessione: comandi completati senza errori ne' token non consumati, uscita pulita (exit 0);
- il DXF di partenza resta identico byte per byte (SHA-256 prima e dopo): i comandi non scrivono il disegno di lavoro;
- i tre DWG si riconvertono con `--export` e ezdxf li legge;
- stato di fatto: le dodici `LINE` `MURI` hanno gli stessi estremi del DXF di partenza (entro 1e-6 m), nessuna campitura;
- demolizioni e costruzioni: lo stesso rilievo, piu' due `HATCH` solide (`solid_fill = 1`), una di colore 2 su
  `AL-DEMOLIZIONI` e una di colore 1 su `AL-NUOVE`;
- stato di progetto: le due `LINE` di chiusura (4,00; 0)-(4,10; 0) e (4,00; 3,00)-(4,10; 3,00); nessuna `LINE` lungo
  le facce demolite del tramezzo (x = 4,00 e x = 4,10, con 0 < y < 3,00); le due facce del muro nuovo, x = 1,94 e
  x = 2,06 per y da 0,50 a 2,50, sul layer `MURI` (il piu' frequente tra i muri, quello che `AL_ESTRAI` propone);
- nei tre file nessuna entita' (oggetti e voci di tabella compresi) con XDATA il cui APPID inizia con `ARCHLINE` e
  nessun APPID `ARCHLINE*` in tabella; nessuna entita' fuori dallo spazio modello.

**Isolamento.** L'app parte con `APPDATA` e `LOCALAPPDATA` puntati a una cartella temporanea (`config\Roaming` e
`config\Local` nella cartella di lavoro), cosi' il vero `settings.json` in `%APPDATA%\OpenCADStudio` non si legge e
non si riscrive (elenco dei recenti, dialoghi "una tantum"). Il disegno di prova e' sintetico: non usare mai un
disegno dell'utente.

**Percorsi senza spazi.** La riga di `run` si divide sugli spazi, quindi la cartella di lavoro (disegno, cartella
`out` degli estratti, configurazione) deve avere un percorso senza spazi. Lo script usa `tempfile.mkdtemp()` nella
cartella temporanea di sistema; se il suo percorso ha spazi (un utente "Mario Rossi"), ripiega su `C:\ocs-bench`
(radice del disco di sistema), che crea; se nemmeno quella va bene si ferma con exit 2 e un messaggio che elenca i
tentativi. `--keep DIR` usa una cartella a scelta (senza spazi, nuova o vuota: `AL_ESTRAI` non sovrascrive) e la lascia com'e'. Se un controllo fallisce la
cartella temporanea resta, con i DXF riconvertiti e `serve.stderr.txt`.

**Limiti.** Un solo disegno sintetico (due assi). Lo "spazio modello" e' controllato solo nel senso che i layout non hanno
entita': il caso non ha contenuto nello spazio carta, quindi quel controllo non distingue un'estrazione che
copiasse anche i layout. I DWG si verificano in modo indiretto (riletti da ArchLine stesso). Aree, nomi e testi dei
locali non si controllano qui: stanno nei test Rust (`cargo test --locked --lib bim::`, `app::bim_tests`).
