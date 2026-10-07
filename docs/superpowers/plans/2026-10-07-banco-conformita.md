# Banco di conformità DXF/DWG — piano di implementazione

> **Per chi esegue:** SOTTO-SKILL OBBLIGATORIA: `superpowers:subagent-driven-development` (consigliata) oppure `superpowers:executing-plans`. I passi usano la sintassi a caselle (`- [ ]`).

**Obiettivo:** un harness Python che genera disegni sintetici con ezdxf, li passa da `OpenCADStudio --export` (DXF→DXF e DXF→DWG→DXF) e classifica ogni controllo come OK / Atteso / Regressione / Migliorato rispetto a `expected.json`.

**Architettura:** quattro moduli piccoli e una cartella di casi. `classify.py` (stati e `expected.json`, puro), `report.py` (Markdown, puro), `oracle.py` (tutto ciò che usa ezdxf), `run.py` (orchestrazione e CLI), `cases/` (un modulo per scenario). Il banco si prova con un convertitore finto che imita `--export` e può corrompere gli accenti, perdere un'entità o andare in crash.

**Stack:** Python 3 (3.13 qui), ezdxf 1.4.4 (`pip install ezdxf`), `unittest` della libreria standard. Nessun codice Rust, nessuna modifica a file upstream, tranne un passo nel workflow `archline-windows.yml`.

**Spec:** [docs/superpowers/specs/2026-10-07-banco-conformita-design.md](../specs/2026-10-07-banco-conformita-design.md)

## Vincoli globali

- Repo **pubblico**: solo disegni sintetici generati da codice. Nessun file di terzi, nessun disegno reale dell'utente (`Base_strutturali.dwg` fuori).
- Resta su `lavoro`, nessun worktree. **Nessun push** senza richiesta esplicita. `main` intoccabile. **Niente righe di attribuzione** nei messaggi di commit. Un commit per modifica logica.
- File nuovi in LF. Python con `# -*- coding` non necessario (UTF-8 di default); nel codice dei casi i caratteri accentati si scrivono come escape (`\u00e8`) così il file non dipende dall'editor.
- `expected.json` si genera dalla misura sull'eseguibile attuale, **mai da valori trascritti** dalla baseline del 03/10 (serve solo come controllo di coerenza). Nessuna voce «atteso» senza nota.
- Le stringhe del report sono ASCII + accenti italiani; lo stdout va riconfigurato a UTF-8 (la console Windows è cp1252).
- Il convertitore va lanciato leggendo l'output come **byte** (`capture_output=True` senza `text=True`): `--export` stampa `→` e su Windows la decodifica cp1252 può sollevare `UnicodeDecodeError`.
- `tests/conformance/` contiene solo `.py`/`.json`/`.md`: cargo scopre solo `tests/*.rs` e `tests/*/main.rs`, quindi la cartella non entra in `cargo test`.
- Comandi Python: `python` (Python 3.13 nel PATH). Test del banco: `python -m unittest discover -s tests/conformance -p "test_*.py" -v` dalla radice del repo.

## Review Focus

Modi di rompersi che la spec implica ma nessun compito elenca per primo. Ognuno ha un test nel compito indicato.

1. **Eseguibile assente o non eseguibile** → messaggio chiaro su stderr e exit 2, non un traceback (Compito 3, `test_missing_executable_is_a_bench_error`).
2. **Una conversione va in crash o in timeout su un caso** → quel controllo registra «conversione fallita (exit N)» e il banco continua con gli altri casi (Compito 3, `test_crash_is_recorded_and_bench_continues`).
3. **Console Windows cp1252 con testo accentato nel report** (`Ã¨`, `è`) → nessun `UnicodeEncodeError`, il report esce in UTF-8 (Compito 3, `test_report_survives_cp1252_console`).
4. **Input sbagliati dell'utente**: `--case` inesistente, `expected.json` con una voce senza nota, `--update-expected` senza `--note` davanti a divergenze nuove → exit 2 con spiegazione (Compiti 1 e 3).
5. **ezdxf non installato** → exit 77 (saltato) con messaggio, non un `ImportError` (Compito 3, `test_missing_ezdxf_skips_with_77`).

---

## Struttura dei file

Tutto sotto `tests/conformance/`:

| File | Responsabilità |
|---|---|
| `classify.py` | `CHECKS`, gli stati, `Result`, `classify()`, `key()`, `load_expected()`, `save_expected()`. Nessuna dipendenza esterna. |
| `report.py` | `render(meta, results, expected)` → testo Markdown. Nessuna dipendenza esterna. |
| `oracle.py` | `read_strict()`, `snapshot()`, `compare()`, `failed()`. Unico modulo (con `cases/` e il convertitore finto) che importa ezdxf. |
| `cases/_common.py`, `cases/*.py`, `cases/__init__.py` | Un modulo per scenario: `NAME` e `build(version, work) -> Path`; `ALL` elenca i casi. |
| `run.py` | `BenchError`, `convert()`, `measure()`, `run_bench()`, `updated_expected()`, `main()`. |
| `fake_converter.py` | Imita `--export IN OUT`; modo da `FAKE_CONVERTER_MODE`. Solo per i test. |
| `test_bench.py` | Test del banco. |
| `expected.json` | Esiti noti. |
| `README.md` | Uso. |

---

### Compito 1: stati, `expected.json` e report (moduli puri)

**Files:**
- Create: `tests/conformance/classify.py`
- Create: `tests/conformance/report.py`
- Create: `tests/conformance/test_bench.py`
- Modify: `.gitignore` (aggiungere `**/__pycache__/`)

**Interfaces:**
- Produces (`classify.py`):
  - costanti `OK="OK"`, `EXPECTED="ATTESO"`, `REGRESSION="REGRESSIONE"`, `IMPROVED="MIGLIORATO"`; `CHECKS = ("readable", "types", "extents", "strings")`
  - `class Result(NamedTuple)`: `case, version, path, check, ok, detail, state` (`path` è `"dxf"` o `"dwg"`)
  - `key(case, version, path, check) -> str` (formato `caso|versione|percorso|controllo`)
  - `classify(ok: bool, detail: str, entry: dict | None) -> str`
  - `load_expected(path) -> dict` (file assente → `{}`; voce senza `detail` o `note` non vuoti → `ValueError`)
  - `save_expected(path, data) -> None` (UTF-8, chiavi ordinate, LF)
- Produces (`report.py`): `render(meta: list[tuple[str, str]], results: list[Result], expected: dict) -> str`

- [ ] **Passo 1: aggiungere `__pycache__` a `.gitignore`**

Aggiungere in fondo a `.gitignore`:

```
# Python (banco di conformita)
**/__pycache__/
```

- [ ] **Passo 2: scrivere i test (devono fallire)**

Creare `tests/conformance/test_bench.py`:

```python
"""Test del banco di conformita. Da lanciare dalla radice del repo:
python -m unittest discover -s tests/conformance -p "test_*.py" -v
"""
import json
import tempfile
import unittest
from pathlib import Path

import classify
import report
from classify import EXPECTED, IMPROVED, OK, REGRESSION, Result


class ClassifyTests(unittest.TestCase):
    def test_ok_without_entry_is_ok(self):
        self.assertEqual(classify.classify(True, "", None), OK)

    def test_ok_with_entry_is_improved(self):
        entry = {"detail": "x", "note": "n"}
        self.assertEqual(classify.classify(True, "", entry), IMPROVED)

    def test_divergence_without_entry_is_regression(self):
        self.assertEqual(classify.classify(False, "x", None), REGRESSION)

    def test_divergence_with_same_detail_is_expected(self):
        entry = {"detail": "x", "note": "n"}
        self.assertEqual(classify.classify(False, "x", entry), EXPECTED)

    def test_divergence_with_different_detail_is_regression(self):
        entry = {"detail": "x", "note": "n"}
        self.assertEqual(classify.classify(False, "y", entry), REGRESSION)

    def test_key_format(self):
        self.assertEqual(classify.key("geometry", "R2000", "dwg", "types"), "geometry|R2000|dwg|types")


class ExpectedFileTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="conf_test_"))
        self.path = self.tmp / "expected.json"

    def test_missing_file_is_empty(self):
        self.assertEqual(classify.load_expected(self.path), {})

    def test_entry_without_note_is_rejected(self):
        self.path.write_text(json.dumps({"a|R2000|dxf|types": {"detail": "x", "note": ""}}), encoding="utf-8")
        with self.assertRaises(ValueError) as ctx:
            classify.load_expected(self.path)
        self.assertIn("a|R2000|dxf|types", str(ctx.exception))

    def test_entry_without_detail_is_rejected(self):
        self.path.write_text(json.dumps({"a|R2000|dxf|types": {"note": "n"}}), encoding="utf-8")
        with self.assertRaises(ValueError):
            classify.load_expected(self.path)

    def test_save_load_round_trip_keeps_accents_and_sorts_keys(self):
        data = {"b|R2000|dxf|strings": {"detail": "[('\u00e8', '\u00c3\u00a8')]", "note": "perch\u00e9"},
                "a|R2000|dxf|types": {"detail": "x", "note": "n"}}
        classify.save_expected(self.path, data)
        raw = self.path.read_bytes()
        self.assertNotIn(b"\r\n", raw)
        self.assertIn("perch\u00e9".encode("utf-8"), raw)
        self.assertLess(raw.index(b"a|R2000"), raw.index(b"b|R2000"))
        self.assertEqual(classify.load_expected(self.path), data)


def _res(case, version, path, check, ok, detail, state):
    return Result(case, version, path, check, ok, detail, state)


class ReportTests(unittest.TestCase):
    def setUp(self):
        self.expected = {
            "text|R2000|dxf|strings": {"detail": "stringhe x", "note": "accenti R2000"},
            "geometry|R2000|dxf|types": {"detail": "tipi y", "note": "vecchio"},
        }
        self.results = [
            _res("geometry", "R2000", "dxf", "readable", True, "", OK),
            _res("geometry", "R2000", "dxf", "types", True, "", IMPROVED),
            _res("text", "R2000", "dxf", "strings", False, "stringhe x", EXPECTED),
            _res("text", "R2000", "dwg", "extents", False, "estensioni z", REGRESSION),
        ]
        self.text = report.render([("Eseguibile", "OpenCADStudio test")], self.results, self.expected)

    def test_header_and_dwg_warning(self):
        self.assertIn("- Eseguibile: OpenCADStudio test", self.text)
        self.assertIn("DWG", self.text)
        self.assertIn("indiretto", self.text)

    def test_table_has_one_row_per_case_version_path(self):
        self.assertIn("| geometry | R2000 | DXF>DXF |", self.text)
        self.assertIn("| text | R2000 | DXF>DWG>DXF |", self.text)

    def test_sections_and_counts(self):
        self.assertIn("## Regressioni (1)", self.text)
        self.assertIn("## Migliorati (1)", self.text)
        self.assertIn("## Attesi (1)", self.text)
        self.assertIn("- REGRESSIONE: 1", self.text)
        self.assertIn("- OK: 1", self.text)

    def test_expected_section_shows_the_note(self):
        self.assertIn("nota: accenti R2000", self.text)

    def test_text_with_accents_encodes_as_utf8(self):
        res = [_res("t", "R2000", "dxf", "strings", False, "[('\u00e8', '\u00c3\u00a8')]", REGRESSION)]
        text = report.render([], res, {})
        self.assertEqual(text.encode("utf-8").decode("utf-8"), text)
        self.assertIn("\u00c3\u00a8", text)


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Passo 3: vedere i test fallire**

Run: `python -m unittest discover -s tests/conformance -p "test_*.py" -v`
Expected: errore di importazione `ModuleNotFoundError: No module named 'classify'` (rosso per modulo mancante: lo dichiaro come tale).

- [ ] **Passo 4: scrivere `classify.py`**

```python
"""Stati di un controllo e lettura/scrittura di expected.json."""
import json
from pathlib import Path
from typing import NamedTuple

OK = "OK"
EXPECTED = "ATTESO"
REGRESSION = "REGRESSIONE"
IMPROVED = "MIGLIORATO"

CHECKS = ("readable", "types", "extents", "strings")


class Result(NamedTuple):
    case: str
    version: str
    path: str  # "dxf" (DXF -> DXF) oppure "dwg" (DXF -> DWG -> DXF)
    check: str
    ok: bool
    detail: str
    state: str


def key(case, version, path, check):
    return f"{case}|{version}|{path}|{check}"


def classify(ok, detail, entry):
    """`entry` e' la voce di expected.json per questo controllo, o None.

    Un esito diverso da quello registrato conta come regressione: i dettagli non hanno un ordine,
    quindi non si puo' dire quale diverga di meno. Lo risolve una persona con --update-expected.
    """
    if ok:
        return IMPROVED if entry is not None else OK
    if entry is None:
        return REGRESSION
    return EXPECTED if entry["detail"] == detail else REGRESSION


def load_expected(path):
    p = Path(path)
    if not p.exists():
        return {}
    data = json.loads(p.read_text(encoding="utf-8"))
    for k, v in data.items():
        if not str(v.get("detail", "")).strip() or not str(v.get("note", "")).strip():
            raise ValueError(f"expected.json: la voce {k!r} deve avere 'detail' e 'note' non vuoti")
    return data


def save_expected(path, data):
    text = json.dumps(data, indent=2, ensure_ascii=False, sort_keys=True) + "\n"
    Path(path).write_bytes(text.encode("utf-8"))
```

- [ ] **Passo 5: scrivere `report.py`**

```python
"""Report Markdown del banco di conformita."""
from collections import Counter

from classify import CHECKS, EXPECTED, IMPROVED, OK, REGRESSION, key

PATH_LABEL = {"dxf": "DXF>DXF", "dwg": "DXF>DWG>DXF"}
STATES = (OK, EXPECTED, REGRESSION, IMPROVED)

DWG_WARNING = (
    "Avvertenza: il percorso DWG e' verificato solo in modo indiretto. ezdxf non legge DWG: il DWG scritto "
    "da ArchLine viene riletto da ArchLine stesso e confrontato dopo l'export in DXF."
)


def _where(r):
    return f"{r.case} {r.version} {PATH_LABEL[r.path]} / {r.check}"


def render(meta, results, expected):
    out = ["# Banco di conformita ArchLine", ""]
    out += [f"- {label}: {value}" for label, value in meta]
    out += ["", DWG_WARNING, ""]
    out.append("| Caso | Versione | Percorso | " + " | ".join(CHECKS) + " |")
    out.append("|---|---|---|" + "---|" * len(CHECKS))
    rows = {}
    for r in results:
        rows.setdefault((r.case, r.version, r.path), {})[r.check] = r.state
    for (case, version, path), states in rows.items():
        cells = " | ".join(states.get(c, "-") for c in CHECKS)
        out.append(f"| {case} | {version} | {PATH_LABEL[path]} | {cells} |")
    for title, state in (("Regressioni", REGRESSION), ("Migliorati", IMPROVED), ("Attesi", EXPECTED)):
        picked = [r for r in results if r.state == state]
        if not picked:
            continue
        out += ["", f"## {title} ({len(picked)})", ""]
        for r in picked:
            line = f"- {_where(r)}"
            entry = expected.get(key(r.case, r.version, r.path, r.check))
            if state == REGRESSION:
                line += f": {r.detail}"
                if entry:
                    line += f" (registrato: {entry['detail']})"
            elif state == IMPROVED:
                line += f" (era: {entry['detail']}; nota: {entry['note']})"
            else:
                line += f": {r.detail} - nota: {entry['note']}"
            out.append(line)
    count = Counter(r.state for r in results)
    out += ["", "## Totali", ""] + [f"- {s}: {count.get(s, 0)}" for s in STATES]
    return "\n".join(out) + "\n"
```

- [ ] **Passo 6: vedere i test passare**

Run: `python -m unittest discover -s tests/conformance -p "test_*.py" -v`
Expected: tutti i test di `ClassifyTests`, `ExpectedFileTests`, `ReportTests` passano (15 test).

- [ ] **Passo 7: commit**

```bash
git add .gitignore tests/conformance/classify.py tests/conformance/report.py tests/conformance/test_bench.py
git commit -m "Banco di conformita: stati, expected.json e report (moduli puri, con test)"
```

---

### Compito 2: oracolo ezdxf e casi

**Files:**
- Create: `tests/conformance/oracle.py`
- Create: `tests/conformance/cases/__init__.py`, `_common.py`, `geometry.py`, `colors.py`, `hatch.py`, `hatch_spline.py`, `text.py`, `text_accents.py`, `dims.py`, `attribs.py`, `paperspace.py`
- Modify: `tests/conformance/test_bench.py` (aggiungere test dell'oracolo e dei casi)

**Prerequisito:** `python -m pip install ezdxf` (versione 1.4.4 disponibile per Python 3.13).

**Interfaces:**
- Consumes: `classify.CHECKS`
- Produces (`oracle.py`):
  - `read_strict(path) -> (doc | None, errore: str | None)` (l'errore è solo il nome della classe dell'eccezione: deterministico)
  - `snapshot(doc) -> {"types": dict, "extents": tuple | None, "strings": list}`
  - `compare(src_snapshot, out_path) -> {check: (ok: bool, detail: str)}` per i quattro `CHECKS`
  - `failed(why: str) -> {check: (False, why)}` per i quattro `CHECKS`
- Produces (`cases`): `cases.ALL` (lista di moduli); ogni modulo ha `NAME: str` e `build(version: str, work: Path) -> Path`; `cases._common.save(doc, work, name, version) -> Path` scrive `work/{name}_{version}.dxf`.

- [ ] **Passo 1: installare ezdxf**

Run: `python -m pip install ezdxf`
Expected: `Successfully installed ezdxf-1.4.4` (o già presente).

- [ ] **Passo 2: aggiungere i test dell'oracolo (devono fallire)**

In `tests/conformance/test_bench.py`, aggiungere in cima agli import:

```python
import importlib.util

HAVE_EZDXF = importlib.util.find_spec("ezdxf") is not None
```

e prima di `if __name__ == "__main__":` aggiungere:

```python
@unittest.skipUnless(HAVE_EZDXF, "ezdxf non installato")
class OracleTests(unittest.TestCase):
    def setUp(self):
        import ezdxf
        import oracle
        self.ezdxf, self.oracle = ezdxf, oracle
        self.tmp = Path(tempfile.mkdtemp(prefix="conf_test_"))

    def _save(self, name, build):
        doc = self.ezdxf.new("R2018")
        build(doc.modelspace())
        path = self.tmp / name
        doc.saveas(path)
        return path

    def test_identical_files_pass_all_checks(self):
        a = self._save("a.dxf", lambda m: (m.add_line((0, 0), (10, 5)), m.add_text("ciao").set_placement((1, 1))))
        doc, err = self.oracle.read_strict(a)
        self.assertIsNone(err)
        res = self.oracle.compare(self.oracle.snapshot(doc), a)
        self.assertEqual(set(res), set(classify.CHECKS))
        self.assertTrue(all(ok for ok, _ in res.values()), res)

    def test_extents_difference_is_detected_beyond_tolerance_only(self):
        a = self._save("a.dxf", lambda m: m.add_line((0, 0), (10, 0)))
        near = self._save("near.dxf", lambda m: m.add_line((0, 0), (10.001, 0)))
        far = self._save("far.dxf", lambda m: m.add_line((0, 0), (10.1, 0)))
        snap = self.oracle.snapshot(self.oracle.read_strict(a)[0])
        self.assertTrue(self.oracle.compare(snap, near)["extents"][0])
        ok, detail = self.oracle.compare(snap, far)["extents"]
        self.assertFalse(ok)
        self.assertIn("10.10", detail)

    def test_type_difference_names_both_counts(self):
        a = self._save("a.dxf", lambda m: (m.add_line((0, 0), (1, 1)), m.add_circle((0, 0), 1)))
        b = self._save("b.dxf", lambda m: m.add_line((0, 0), (1, 1)))
        snap = self.oracle.snapshot(self.oracle.read_strict(a)[0])
        ok, detail = self.oracle.compare(snap, b)["types"]
        self.assertFalse(ok)
        self.assertIn("'CIRCLE': (1, 0)", detail)

    def test_string_difference_is_reported(self):
        a = self._save("a.dxf", lambda m: m.add_text("perch\u00e9").set_placement((0, 0)))
        b = self._save("b.dxf", lambda m: m.add_text("perch\u00c3\u00a9").set_placement((0, 0)))
        snap = self.oracle.snapshot(self.oracle.read_strict(a)[0])
        ok, detail = self.oracle.compare(snap, b)["strings"]
        self.assertFalse(ok)
        self.assertIn("perch\u00c3\u00a9", detail)

    def test_unreadable_file_fails_every_check(self):
        a = self._save("a.dxf", lambda m: m.add_line((0, 0), (1, 1)))
        junk = self.tmp / "junk.dxf"
        junk.write_bytes(b"questo non e' un DXF")
        snap = self.oracle.snapshot(self.oracle.read_strict(a)[0])
        res = self.oracle.compare(snap, junk)
        self.assertEqual(set(res), set(classify.CHECKS))
        self.assertFalse(any(ok for ok, _ in res.values()))
        self.assertTrue(res["readable"][1].startswith("non leggibile ("))

    def test_failed_covers_every_check(self):
        res = self.oracle.failed("conversione fallita (exit 1)")
        self.assertEqual(res, {c: (False, "conversione fallita (exit 1)") for c in classify.CHECKS})


@unittest.skipUnless(HAVE_EZDXF, "ezdxf non installato")
class CasesTests(unittest.TestCase):
    def test_every_case_builds_a_strict_readable_file_in_both_versions(self):
        import cases
        import oracle
        tmp = Path(tempfile.mkdtemp(prefix="conf_test_"))
        names = [c.NAME for c in cases.ALL]
        self.assertEqual(len(names), len(set(names)), "nomi di caso duplicati")
        self.assertEqual(
            sorted(names),
            sorted(["geometry", "colors", "hatch", "hatch_spline", "text", "text_accents", "dims", "attribs", "paperspace"]),
        )
        for case in cases.ALL:
            for version in ("R2000", "R2018"):
                path = case.build(version, tmp)
                self.assertEqual(path.name, f"{case.NAME}_{version}.dxf")
                doc, err = oracle.read_strict(path)
                self.assertIsNone(err, f"{case.NAME} {version}: {err}")
                self.assertGreater(len(list(doc.modelspace())), 0, f"{case.NAME} {version}: model space vuoto")

    def test_text_accents_contains_the_accented_strings_in_all_three_entity_kinds(self):
        import cases
        import oracle
        tmp = Path(tempfile.mkdtemp(prefix="conf_test_"))
        for version in ("R2000", "R2018"):
            doc, _ = oracle.read_strict(cases.text_accents.build(version, tmp))
            strings = oracle.snapshot(doc)["strings"]
            joined = " ".join(strings)
            self.assertEqual(len(strings), 3, strings)
            for ch in "\u00e8\u00e0\u00f2\u00f9\u00ec\u00e9":
                self.assertIn(ch, joined, f"{version}: manca {ch!r}")
```

- [ ] **Passo 3: vedere i test fallire**

Run: `python -m unittest discover -s tests/conformance -p "test_*.py" -v`
Expected: i test `OracleTests`/`CasesTests` falliscono con `ModuleNotFoundError: No module named 'oracle'` (rosso per modulo mancante).

- [ ] **Passo 4: scrivere `oracle.py`**

```python
"""Tutto cio' che usa ezdxf: legge un DXF e ne ricava i dati da confrontare."""
import collections

import ezdxf
from ezdxf import bbox

from classify import CHECKS

EXT_TOL = 2e-3


def read_strict(path):
    """ezdxf.readfile e' strict. Ritorna (doc, None) oppure (None, nome dell'eccezione)."""
    try:
        return ezdxf.readfile(path), None
    except Exception as ex:  # DXFError, IOError, UnicodeDecodeError...
        return None, type(ex).__name__


def _extents(doc):
    box = bbox.extents(doc.modelspace())
    if not box.has_data:
        return None
    return (box.extmin.x, box.extmin.y, box.extmax.x, box.extmax.y)


def _types(doc):
    return dict(sorted(collections.Counter(e.dxftype() for e in doc.modelspace()).items()))


def _strings(doc):
    out = []
    for e in doc.modelspace():
        kind = e.dxftype()
        if kind == "TEXT":
            out.append(e.plain_text())
        elif kind == "MTEXT":
            out.append(e.plain_text(split=False))
        elif kind == "INSERT":
            out += [a.dxf.text for a in e.attribs]
    return out


def snapshot(doc):
    return {"types": _types(doc), "extents": _extents(doc), "strings": _strings(doc)}


def _fmt_box(box):
    return "vuoto" if box is None else "(" + ", ".join(f"{v:.2f}" for v in box) + ")"


def _same_box(a, b):
    if a is None or b is None:
        return a is None and b is None
    return all(abs(x - y) <= EXT_TOL for x, y in zip(a, b))


def failed(why):
    return {c: (False, why) for c in CHECKS}


def compare(src, out_path):
    """Confronta lo snapshot della sorgente con il file `out_path`: {controllo: (ok, dettaglio)}."""
    doc, err = read_strict(out_path)
    if doc is None:
        res = failed("non leggibile")
        res["readable"] = (False, f"non leggibile ({err})")
        return res
    got = snapshot(doc)
    res = {"readable": (True, "")}

    if got["types"] == src["types"]:
        res["types"] = (True, "")
    else:
        keys = sorted(set(src["types"]) | set(got["types"]))
        diff = {
            k: (src["types"].get(k, 0), got["types"].get(k, 0))
            for k in keys
            if src["types"].get(k, 0) != got["types"].get(k, 0)
        }
        res["types"] = (False, f"tipi {diff}")

    if _same_box(src["extents"], got["extents"]):
        res["extents"] = (True, "")
    else:
        res["extents"] = (False, f"estensioni {_fmt_box(src['extents'])} -> {_fmt_box(got['extents'])}")

    a, b = src["strings"], got["strings"]
    if a == b:
        res["strings"] = (True, "")
    else:
        bad = [(x, y) for x, y in zip(a, b) if x != y][:2]
        res["strings"] = (False, f"stringhe {bad}" if bad else f"numero stringhe {len(a)} -> {len(b)}")
    return res
```

- [ ] **Passo 5: scrivere i casi**

`tests/conformance/cases/_common.py`:

```python
"""Utilita' comuni ai casi."""


def save(doc, work, name, version):
    path = work / f"{name}_{version}.dxf"
    doc.saveas(path)
    return path
```

`tests/conformance/cases/geometry.py`:

```python
"""Geometria: blocchi annidati, array ruotato, scale negative, estrusione -Z, bulge, polilinee, solido."""
import ezdxf

from ._common import save

NAME = "geometry"


def build(version, work):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    doc.layers.add("MURI", color=1)
    msp.add_line((0, 0), (10, 5), dxfattribs={"layer": "MURI"})
    msp.add_circle((20, 10), 3)
    msp.add_arc((30, 0), 5, 20, 200)
    msp.add_arc((30, 0), 3, 300, 60)  # passa per 0 gradi
    msp.add_lwpolyline([(0, 20, 0, 0, 0.5), (10, 20, 0, 0, -0.8), (15, 28, 0, 0, 0)], format="xyseb", close=True)
    msp.add_polyline2d([(40, 0, 0.4), (50, 0, 0), (55, 8, -0.6), (45, 12, 0)], format="xyb", close=True)
    msp.add_solid([(60, 0), (65, 0), (60, 5), (65, 5)])

    b1 = doc.blocks.new("B1", base_point=(1, 2))
    b1.add_line((0, 0), (4, 0))
    b1.add_arc((2, 2), 2, 0, 180)
    b1.add_circle((1, 1), 0.5)
    b1.add_lwpolyline([(0, 0), (3, 0), (3, 3)], format="xy")
    b2 = doc.blocks.new("B2")
    b2.add_blockref("B1", (5, 5), dxfattribs={"xscale": 2, "yscale": 0.5, "rotation": 30})
    b2.add_line((0, 0), (0, 10))
    msp.add_blockref("B1", (100, 0))
    msp.add_blockref("B2", (120, 10), dxfattribs={"rotation": 45, "xscale": -1.5, "yscale": 2})
    msp.add_blockref("B1", (150, 0), dxfattribs={"xscale": -1, "yscale": -1, "rotation": 10})
    arr = msp.add_blockref("B1", (0, 60), dxfattribs={"rotation": 15})
    arr.grid(size=(3, 4), spacing=(12, 9))
    # estrusione (0,0,-1): OCS speculare
    msp.add_circle((10, 40), 2, dxfattribs={"extrusion": (0, 0, -1)})
    msp.add_arc((20, 40), 4, 10, 120, dxfattribs={"extrusion": (0, 0, -1)})
    msp.add_lwpolyline([(30, 40, 0, 0, 0.5), (40, 45, 0, 0, 0), (45, 38, 0, 0, 0)], format="xyseb", dxfattribs={"extrusion": (0, 0, -1)})
    msp.add_blockref("B1", (60, 40), dxfattribs={"extrusion": (0, 0, -1), "rotation": 20, "xscale": 1.5})
    return save(doc, work, NAME, version)
```

`tests/conformance/cases/colors.py`:

```python
"""Colori e layer in blocchi annidati: BYBLOCK, BYLAYER, layer 0."""
import ezdxf

from ._common import save

NAME = "colors"


def build(version, work):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    doc.layers.add("L1", color=3)
    doc.layers.add("WALL", color=6)
    inner = doc.blocks.new("INNER")
    inner.add_line((0, 0), (1, 0), dxfattribs={"layer": "0", "color": 0})       # '0' + BYBLOCK
    inner.add_line((0, 1), (1, 1), dxfattribs={"layer": "0", "color": 256})     # '0' + BYLAYER
    inner.add_line((0, 2), (1, 2), dxfattribs={"layer": "WALL", "color": 256})  # layer proprio
    inner.add_line((0, 3), (1, 3), dxfattribs={"layer": "WALL", "color": 0})    # layer proprio + BYBLOCK
    inner.add_line((0, 4), (1, 4), dxfattribs={"layer": "0", "color": 5})       # esplicito
    outer = doc.blocks.new("OUTER")
    outer.add_blockref("INNER", (0, 0), dxfattribs={"layer": "0", "color": 0})
    outer.add_blockref("INNER", (3, 0), dxfattribs={"layer": "0", "color": 2})
    msp.add_blockref("INNER", (0, 0), dxfattribs={"layer": "L1", "color": 256})
    msp.add_blockref("INNER", (10, 0), dxfattribs={"layer": "L1", "color": 1})
    msp.add_blockref("OUTER", (20, 0), dxfattribs={"layer": "L1", "color": 256})
    msp.add_blockref("OUTER", (40, 0), dxfattribs={"layer": "WALL", "color": 4})
    return save(doc, work, NAME, version)
```

`tests/conformance/cases/hatch.py`:

```python
"""Hatch: solido con bulge, pattern ANSI31, isola, ellisse, cerchio."""
import ezdxf

from ._common import save

NAME = "hatch"


def build(version, work):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    h1 = msp.add_hatch(color=2)
    h1.paths.add_polyline_path([(0, 0), (10, 0, 0.5), (10, 10), (0, 10)], is_closed=True)
    h2 = msp.add_hatch()
    h2.set_pattern_fill("ANSI31", scale=2)
    ep = h2.paths.add_edge_path()
    ep.add_line((20, 0), (30, 0))
    ep.add_arc((30, 5), 5, -90, 90)
    ep.add_line((30, 10), (20, 10))
    ep.add_arc((20, 5), 5, 90, 270)
    h3 = msp.add_hatch(color=3)
    h3.paths.add_polyline_path([(40, 0), (60, 0), (60, 20), (40, 20)], is_closed=True)
    h3.paths.add_polyline_path([(45, 5), (55, 5), (55, 15), (45, 15)], is_closed=True, flags=0)
    h4 = msp.add_hatch()
    h4.paths.add_edge_path().add_ellipse((80, 10), major_axis=(10, 0), ratio=0.5, start_angle=0, end_angle=360, ccw=True)
    h5 = msp.add_hatch()
    h5.paths.add_edge_path().add_arc((100, 10), 5, 0, 360, ccw=True)
    return save(doc, work, NAME, version)
```

`tests/conformance/cases/hatch_spline.py`:

```python
"""Hatch con contorno a spline (la baseline del 03/10 ha visto i punti di fit passare da 3 a 0 in R2000).

Riserva: la spline sintetica ha solo punti di fit e nessun punto di controllo, cosa che AutoCAD di norma
non scrive.
"""
import ezdxf

from ._common import save

NAME = "hatch_spline"


def build(version, work):
    doc = ezdxf.new(version)
    h = doc.modelspace().add_hatch()
    h.paths.add_edge_path().add_spline(fit_points=[(0, 0), (5, 3), (10, 0)])
    h.paths[0].add_line((10, 0), (0, 0))
    return save(doc, work, NAME, version)
```

`tests/conformance/cases/text.py`:

```python
"""Testo: TEXT con codici %%, allineamenti, rotazione; MTEXT con formattazione e oltre 250 caratteri."""
import ezdxf
from ezdxf.enums import TextEntityAlignment

from ._common import save

NAME = "text"


def build(version, work):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    msp.add_text("Planimetria %%c120 %%d 45%%p1", height=2.5).set_placement((0, 0))
    msp.add_text("Sottotetto \u00e8 pi\u00f9 basso: 2,40 m \u00f9 \u00e0 \u00f2 \u00ec \u00e9", height=2).set_placement((0, 10))
    msp.add_text("Centrato", height=2).set_placement((50, 50), align=TextEntityAlignment.MIDDLE_CENTER)
    msp.add_text("Destra", height=2).set_placement((50, 60), align=TextEntityAlignment.BOTTOM_RIGHT)
    msp.add_text("Ruotato", height=2, rotation=30).set_placement((10, 20))
    msp.add_mtext("{\\fArial|b1|i0;Titolo}\\PSecondo riga\\P\\C1;rosso \\U+00E8 \\~ fine", dxfattribs={"insert": (0, 30), "char_height": 3})
    msp.add_mtext("A" * 300 + "\\PB", dxfattribs={"insert": (0, 40), "char_height": 1})  # > 250 caratteri: piu' blocchi gruppo 3
    return save(doc, work, NAME, version)
```

`tests/conformance/cases/text_accents.py`:

```python
"""Accenti italiani in TEXT, MTEXT e ATTRIB.

Difetto noto (baseline del 03/10): nei file R2000 ($DWGCODEPAGE = ANSI_1252) ArchLine scrive i caratteri
accentati come UTF-8, e chi rispetta il codepage legge 'e con accento' come due caratteri (mojibake).
"""
import ezdxf

from ._common import save

NAME = "text_accents"


def build(version, work):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    msp.add_text("Sottotetto \u00e8 pi\u00f9 basso: \u00f9 \u00e0 \u00f2 \u00ec \u00e9", height=2).set_placement((0, 0))
    msp.add_mtext("Cucina: perch\u00e9 gi\u00e0 cos\u00ec\\Pcitt\u00e0 \u00e8", dxfattribs={"insert": (0, 10), "char_height": 2})
    blk = doc.blocks.new("TARGA")
    blk.add_attdef("NOTE", (0, 0), "-", dxfattribs={"height": 0.5})
    ins = msp.add_blockref("TARGA", (20, 20))
    ins.add_auto_attribs({"NOTE": "porta \u00e8 blindata"})
    return save(doc, work, NAME, version)
```

`tests/conformance/cases/dims.py`:

```python
"""Quote: lineare, allineata, raggio, diametro, angolare."""
import ezdxf

from ._common import save

NAME = "dims"


def build(version, work):
    doc = ezdxf.new(version, setup=True)
    msp = doc.modelspace()
    msp.add_linear_dim(base=(5, 3), p1=(0, 0), p2=(10, 0)).render()
    msp.add_linear_dim(base=(-3, 5), p1=(0, 0), p2=(0, 10), angle=90).render()
    msp.add_aligned_dim(p1=(20, 0), p2=(30, 8), distance=2).render()
    msp.add_radius_dim(center=(50, 5), radius=4, angle=45).render()
    msp.add_diameter_dim(center=(70, 5), radius=4, angle=30).render()
    msp.add_angular_dim_3p(base=(95, 12), center=(90, 0), p1=(100, 0), p2=(90, 10)).render()
    msp.add_circle((50, 5), 4)
    return save(doc, work, NAME, version)
```

`tests/conformance/cases/attribs.py`:

```python
"""Attributi di blocco (ATTDEF + ATTRIB)."""
import ezdxf

from ._common import save

NAME = "attribs"


def build(version, work):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    blk = doc.blocks.new("TAG")
    blk.add_circle((0, 0), 1)
    blk.add_attdef("NUM", (0, 0), "0", dxfattribs={"height": 0.5})
    blk.add_attdef("NOTE", (0, -2), "-", dxfattribs={"height": 0.5})
    ins = msp.add_blockref("TAG", (10, 10))
    ins.add_auto_attribs({"NUM": "A1", "NOTE": "porta"})
    return save(doc, work, NAME, version)
```

`tests/conformance/cases/paperspace.py`:

```python
"""Paper space: le entita' del layout non devono entrare nelle estensioni del model space."""
import ezdxf

from ._common import save

NAME = "paperspace"


def build(version, work):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    msp.add_line((0, 0), (10, 10))
    doc.layout("Layout1").add_line((0, 0), (500, 500))
    return save(doc, work, NAME, version)
```

`tests/conformance/cases/__init__.py`:

```python
"""Casi del banco: ogni modulo espone NAME e build(version, work) -> Path."""
from . import attribs, colors, dims, geometry, hatch, hatch_spline, paperspace, text, text_accents

ALL = [geometry, colors, hatch, hatch_spline, text, text_accents, dims, attribs, paperspace]
```

- [ ] **Passo 6: vedere i test passare**

Run: `python -m unittest discover -s tests/conformance -p "test_*.py" -v`
Expected: tutti passano (15 del Compito 1 + 6 `OracleTests` + 2 `CasesTests`). Se un caso solleva un errore di API di ezdxf, correggere il caso (le chiamate sono quelle dello script `run_oracle.py` di Autocad_clone, provato con ezdxf 1.4.4); il test `test_every_case_builds…` indica quale.

- [ ] **Passo 7: commit**

```bash
git add tests/conformance/oracle.py tests/conformance/cases tests/conformance/test_bench.py
git commit -m "Banco di conformita: oracolo ezdxf e nove casi sintetici (R2000 e R2018)"
```

---

### Compito 3: orchestratore, CLI e convertitore finto

**Files:**
- Create: `tests/conformance/fake_converter.py`
- Create: `tests/conformance/run.py`
- Modify: `tests/conformance/test_bench.py`

**Interfaces:**
- Consumes: `classify.{CHECKS, Result, classify, key, load_expected, save_expected, OK…}`, `report.render`, `oracle.{read_strict, snapshot, compare, failed}`, `cases.ALL`
- Produces (`run.py`):
  - `class BenchError(Exception)`
  - `VERSIONS = ("R2000", "R2018")`
  - `converter_cmd(exe) -> list[str]` (un `.py` viene lanciato con `sys.executable`)
  - `convert(cmd, src, dst, timeout=120) -> (ok: bool, why: str)`; `FileNotFoundError`/`PermissionError` → `BenchError`
  - `measure(cmd, case, version, work) -> {"dxf": {...}, "dwg": {...}}` (ognuno `{check: (ok, detail)}`)
  - `run_bench(cmd, cases, versions, expected, work) -> list[Result]`
  - `updated_expected(expected, results, note) -> (new_dict, added, changed, removed)`; senza `note` davanti a divergenze nuove o cambiate → `BenchError`
  - `main(argv=None) -> int` (0, 1, 2, 77)
- Produces (`fake_converter.py`): script `fake_converter.py --export IN OUT` / `--version`; `FAKE_CONVERTER_MODE` ∈ `faithful` (predefinito), `corrupt-accents`, `drop-entity`, `crash`.

- [ ] **Passo 1: scrivere il convertitore finto**

`tests/conformance/fake_converter.py`:

```python
#!/usr/bin/env python3
"""Convertitore finto per i test del banco: imita `OpenCADStudio --export IN OUT` e `--version`.

Modo da FAKE_CONVERTER_MODE:
  faithful         rilegge e riscrive con ezdxf (nessuna divergenza)
  corrupt-accents  come faithful, ma i testi escono come UTF-8 letto in cp1252 (e' -> 'A tilde' + 'copyright'...)
  drop-entity      come faithful, ma perde l'ultima entita' del model space
  crash            esce con codice 1 senza scrivere nulla
"""
import os
import shutil
import sys
import tempfile
from pathlib import Path

import ezdxf


def load(src):
    src = Path(src)
    if src.suffix.lower() == ".dxf":
        return ezdxf.readfile(src)
    with tempfile.TemporaryDirectory() as d:  # un "DWG" finto e' un DXF con altra estensione
        tmp = Path(d) / "in.dxf"
        shutil.copyfile(src, tmp)
        return ezdxf.readfile(tmp)


def mojibake(s):
    return s.encode("utf-8").decode("cp1252", errors="replace")


def corrupt_accents(doc):
    for e in doc.modelspace():
        kind = e.dxftype()
        if kind == "TEXT":
            e.dxf.text = mojibake(e.dxf.text)
        elif kind == "MTEXT":
            e.text = mojibake(e.text)
        elif kind == "INSERT":
            for a in e.attribs:
                a.dxf.text = mojibake(a.dxf.text)


def main(argv):
    if argv[:1] == ["--version"]:
        print("fake-converter 0")
        return 0
    if len(argv) != 3 or argv[0] != "--export":
        print("uso: fake_converter.py --export IN OUT", file=sys.stderr)
        return 2
    mode = os.environ.get("FAKE_CONVERTER_MODE", "faithful")
    if mode == "crash":
        return 1
    doc = load(argv[1])
    if mode == "corrupt-accents":
        corrupt_accents(doc)
    elif mode == "drop-entity":
        msp = doc.modelspace()
        entities = list(msp)
        if entities:
            msp.delete_entity(entities[-1])
    doc.saveas(argv[2])
    print(f"Exported {argv[1]} -> {argv[2]}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```

- [ ] **Passo 2: scrivere i test dell'orchestratore (devono fallire)**

In `tests/conformance/test_bench.py`, aggiungere agli import in cima:

```python
import contextlib
import io
import os
import sys
from unittest import mock
```

e prima di `if __name__ == "__main__":` aggiungere:

```python
FAKE = str(Path(__file__).resolve().parent / "fake_converter.py")


class UpdatedExpectedTests(unittest.TestCase):
    def setUp(self):
        import run
        self.run = run

    def _res(self, check, ok, detail, state):
        return Result("c", "R2000", "dxf", check, ok, detail, state)

    def test_new_divergence_without_note_is_refused(self):
        results = [self._res("types", False, "tipi x", REGRESSION)]
        with self.assertRaises(self.run.BenchError) as ctx:
            self.run.updated_expected({}, results, None)
        self.assertIn("--note", str(ctx.exception))

    def test_new_divergence_with_note_is_added(self):
        results = [self._res("types", False, "tipi x", REGRESSION)]
        new, added, changed, removed = self.run.updated_expected({}, results, "motivo")
        self.assertEqual(new, {"c|R2000|dxf|types": {"detail": "tipi x", "note": "motivo"}})
        self.assertEqual((added, changed, removed), (1, 0, 0))

    def test_unchanged_expected_keeps_its_old_note_and_needs_no_note(self):
        old = {"c|R2000|dxf|types": {"detail": "tipi x", "note": "nota vecchia"}}
        results = [self._res("types", False, "tipi x", EXPECTED)]
        new, added, changed, removed = self.run.updated_expected(old, results, None)
        self.assertEqual(new, old)
        self.assertEqual((added, changed, removed), (0, 0, 0))

    def test_improved_entry_is_dropped(self):
        old = {"c|R2000|dxf|types": {"detail": "tipi x", "note": "n"}}
        results = [self._res("types", True, "", IMPROVED)]
        new, added, changed, removed = self.run.updated_expected(old, results, None)
        self.assertEqual(new, {})
        self.assertEqual((added, changed, removed), (0, 0, 1))

    def test_changed_detail_is_replaced_and_needs_a_note(self):
        old = {"c|R2000|dxf|types": {"detail": "tipi x", "note": "n"}}
        results = [self._res("types", False, "tipi y", REGRESSION)]
        with self.assertRaises(self.run.BenchError):
            self.run.updated_expected(old, results, None)
        new, added, changed, removed = self.run.updated_expected(old, results, "nuovo motivo")
        self.assertEqual(new["c|R2000|dxf|types"], {"detail": "tipi y", "note": "nuovo motivo"})
        self.assertEqual((added, changed, removed), (0, 1, 0))

    def test_entries_of_cases_not_run_are_preserved(self):
        old = {"altro|R2000|dxf|types": {"detail": "d", "note": "n"}}
        results = [self._res("types", True, "", OK)]
        new, *_ = self.run.updated_expected(old, results, None)
        self.assertEqual(new, old)


@unittest.skipUnless(HAVE_EZDXF, "ezdxf non installato")
class BenchEndToEndTests(unittest.TestCase):
    def setUp(self):
        import cases
        import run
        self.cases, self.run = cases, run
        self.tmp = Path(tempfile.mkdtemp(prefix="conf_test_"))
        self.expected_path = self.tmp / "expected.json"
        self.cmd = [sys.executable, FAKE]

    def _bench(self, mode, case_modules, expected=None):
        work = self.tmp / f"work_{mode}"
        work.mkdir(exist_ok=True)
        with mock.patch.dict(os.environ, {"FAKE_CONVERTER_MODE": mode}):
            return self.run.run_bench(self.cmd, case_modules, self.run.VERSIONS, expected or {}, work)

    def _main(self, mode, *args, exe=FAKE, stream=None):
        argv = [exe, "--expected", str(self.expected_path), *args]
        out = stream or io.StringIO()
        err = io.StringIO()
        with mock.patch.dict(os.environ, {"FAKE_CONVERTER_MODE": mode}), \
                contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            rc = self.run.main(argv)
        return rc, out, err.getvalue()

    def test_faithful_converter_gives_all_ok(self):
        results = self._bench("faithful", [self.cases.geometry, self.cases.text_accents])
        self.assertEqual(len(results), 2 * 2 * 2 * 4)  # casi x versioni x percorsi x controlli
        bad = [r for r in results if r.state != OK]
        self.assertEqual(bad, [], bad)

    def test_corrupted_accents_are_a_regression_only_in_strings(self):
        results = self._bench("corrupt-accents", [self.cases.text_accents])
        by_check = {}
        for r in results:
            by_check.setdefault(r.check, set()).add(r.state)
        self.assertEqual(by_check["strings"], {REGRESSION})
        self.assertEqual(by_check["types"], {OK})
        self.assertEqual(by_check["extents"], {OK})
        self.assertEqual(by_check["readable"], {OK})
        detail = next(r.detail for r in results if r.check == "strings")
        self.assertIn("\u00c3", detail)

    def test_dropped_entity_is_detected_in_types(self):
        results = self._bench("drop-entity", [self.cases.geometry])
        states = {r.state for r in results if r.check == "types"}
        self.assertEqual(states, {REGRESSION})

    def test_crash_is_recorded_and_bench_continues(self):
        results = self._bench("crash", [self.cases.geometry, self.cases.text_accents])
        self.assertEqual(len(results), 2 * 2 * 2 * 4)
        self.assertTrue(all(not r.ok for r in results))
        self.assertTrue(all(r.detail == "conversione fallita (exit 1)" for r in results), {r.detail for r in results})

    def test_recorded_divergence_becomes_expected_and_strict_passes(self):
        rc, out, _ = self._main("corrupt-accents", "--case", "text_accents", "--strict")
        self.assertEqual(rc, 1)
        self.assertIn("REGRESSIONE", out.getvalue())
        rc, out, _ = self._main("corrupt-accents", "--case", "text_accents", "--update-expected",
                                "--note", "accenti corrotti dal convertitore finto")
        self.assertEqual(rc, 0)
        saved = classify.load_expected(self.expected_path)
        self.assertEqual(len(saved), 4)  # strings x 2 versioni x 2 percorsi
        self.assertTrue(all(k.endswith("|strings") for k in saved))
        rc, out, _ = self._main("corrupt-accents", "--case", "text_accents", "--strict")
        self.assertEqual(rc, 0)
        self.assertIn("ATTESO", out.getvalue())
        self.assertNotIn("## Regressioni", out.getvalue())

    def test_fixed_divergence_is_reported_as_improved(self):
        self._main("corrupt-accents", "--case", "text_accents", "--update-expected", "--note", "n")
        rc, out, _ = self._main("faithful", "--case", "text_accents", "--strict")
        self.assertEqual(rc, 0)
        self.assertIn("## Migliorati (4)", out.getvalue())

    def test_report_survives_cp1252_console(self):
        raw = io.BytesIO()
        console = io.TextIOWrapper(raw, encoding="cp1252", errors="strict")
        rc, out, _ = self._main("corrupt-accents", "--case", "text_accents", stream=console)
        out.flush()
        self.assertEqual(rc, 0)
        text = raw.getvalue().decode("utf-8")
        self.assertIn("## Regressioni", text)
        self.assertIn("\u00c3", text)

    def test_out_file_is_written_as_utf8(self):
        out_file = self.tmp / "report.md"
        self._main("corrupt-accents", "--case", "text_accents", "--out", str(out_file))
        text = out_file.read_bytes().decode("utf-8")
        self.assertIn("# Banco di conformita ArchLine", text)

    def test_keep_leaves_the_generated_files(self):
        keep = self.tmp / "keep"
        self._main("faithful", "--case", "geometry", "--keep", str(keep))
        self.assertTrue((keep / "geometry_R2000.dxf").exists())
        self.assertTrue((keep / "geometry_R2018.dxf").exists())

    def test_missing_executable_is_a_bench_error(self):
        rc, _, err = self._main("faithful", exe=str(self.tmp / "non_esiste.exe"))
        self.assertEqual(rc, 2)
        self.assertIn("eseguibile non trovato", err)

    def test_unknown_case_is_a_bench_error(self):
        rc, _, err = self._main("faithful", "--case", "inesistente")
        self.assertEqual(rc, 2)
        self.assertIn("caso sconosciuto: inesistente", err)
        self.assertIn("geometry", err)

    def test_expected_entry_without_note_is_a_bench_error(self):
        self.expected_path.write_text(
            json.dumps({"geometry|R2000|dxf|types": {"detail": "x", "note": ""}}), encoding="utf-8")
        rc, _, err = self._main("faithful", "--case", "geometry")
        self.assertEqual(rc, 2)
        self.assertIn("geometry|R2000|dxf|types", err)

    def test_update_expected_without_note_is_a_bench_error(self):
        rc, _, err = self._main("corrupt-accents", "--case", "text_accents", "--update-expected")
        self.assertEqual(rc, 2)
        self.assertIn("--note", err)
        self.assertFalse(self.expected_path.exists())

    def test_missing_ezdxf_skips_with_77(self):
        with mock.patch("importlib.util.find_spec", return_value=None):
            rc, out, _ = self._main("faithful")
        self.assertEqual(rc, 77)
        self.assertIn("ezdxf", out.getvalue())
```

Nota: l'ultimo test usa `self._main`, che chiama `run.main`; `main` deve controllare `importlib.util.find_spec("ezdxf")` **prima** di ogni altra cosa (anche prima di verificare l'eseguibile).

- [ ] **Passo 3: vedere i test fallire**

Run: `python -m unittest discover -s tests/conformance -p "test_*.py" -v`
Expected: `UpdatedExpectedTests` e `BenchEndToEndTests` falliscono con `ModuleNotFoundError: No module named 'run'` (rosso per modulo mancante).

- [ ] **Passo 4: scrivere `run.py`**

```python
#!/usr/bin/env python3
"""Banco di conformita: genera disegni sintetici con ezdxf, li passa da `OpenCADStudio --export`
e confronta il risultato con la sorgente (ezdxf come oracolo). Misura, non blocca (salvo --strict).

Uso: run.py PATH_ESEGUIBILE [--out report.md] [--strict] [--update-expected --note "..."]
            [--case NOME ...] [--keep DIR] [--expected FILE]
Exit: 0 ok, 1 regressione (solo con --strict), 2 errore del banco, 77 ezdxf non installato.
"""
import argparse
import importlib.util
import shutil
import subprocess
import sys
import tempfile
from datetime import date
from pathlib import Path

from classify import CHECKS, Result, classify, key, load_expected, save_expected, REGRESSION
import report

HERE = Path(__file__).resolve().parent
VERSIONS = ("R2000", "R2018")
PATHS = ("dxf", "dwg")


class BenchError(Exception):
    """Errore del banco (non una divergenza dell'app): exit 2."""


def converter_cmd(exe):
    exe = str(exe)
    return [sys.executable, exe] if exe.endswith(".py") else [exe]


def convert(cmd, src, dst, timeout=120):
    """Lancia `--export src dst`. Ritorna (ok, motivo). L'output si legge come byte: l'app stampa una freccia
    e la decodifica cp1252 della console Windows puo' sollevare UnicodeDecodeError."""
    try:
        r = subprocess.run([*cmd, "--export", str(src), str(dst)], capture_output=True, timeout=timeout)
    except (FileNotFoundError, PermissionError) as ex:
        raise BenchError(f"convertitore non eseguibile: {cmd[0]} ({type(ex).__name__})") from ex
    except subprocess.TimeoutExpired:
        return False, f"conversione fallita (timeout {timeout} s)"
    if r.returncode != 0:
        return False, f"conversione fallita (exit {r.returncode})"
    if not dst.exists() or dst.stat().st_size == 0:
        return False, "conversione fallita (nessun file scritto)"
    return True, ""


def measure(cmd, case, version, work):
    import oracle

    src = case.build(version, work)
    src_doc, err = oracle.read_strict(src)
    if src_doc is None:
        raise BenchError(f"la sorgente {src.name} non e' leggibile da ezdxf ({err}): difetto del caso, non dell'app")
    snap = oracle.snapshot(src_doc)
    stem = src.stem

    dxf_out = work / f"{stem}.out.dxf"
    ok, why = convert(cmd, src, dxf_out)
    dxf = oracle.compare(snap, dxf_out) if ok else oracle.failed(why)

    dwg = work / f"{stem}.out.dwg"
    back = work / f"{stem}.back.dxf"
    ok, why = convert(cmd, src, dwg)
    if ok:
        ok, why = convert(cmd, dwg, back)
    dwg_res = oracle.compare(snap, back) if ok else oracle.failed(why)
    return {"dxf": dxf, "dwg": dwg_res}


def run_bench(cmd, cases, versions, expected, work):
    results = []
    for case in cases:
        for version in versions:
            measured = measure(cmd, case, version, work)
            for path in PATHS:
                for check in CHECKS:
                    ok, detail = measured[path][check]
                    entry = expected.get(key(case.NAME, version, path, check))
                    results.append(Result(case.NAME, version, path, check, ok, detail, classify(ok, detail, entry)))
    return results


def updated_expected(expected, results, note):
    """Nuovo expected.json dopo la misura. Ritorna (dict, aggiunte, cambiate, rimosse)."""
    new, added, changed, seen = {}, 0, 0, set()
    for r in results:
        k = key(r.case, r.version, r.path, r.check)
        seen.add(k)
        if r.ok:
            continue
        old = expected.get(k)
        if old is not None and old["detail"] == r.detail:
            new[k] = old
            continue
        if not note:
            raise BenchError('--update-expected: ci sono divergenze nuove o cambiate, serve --note "motivo"')
        new[k] = {"detail": r.detail, "note": note}
        if old is None:
            added += 1
        else:
            changed += 1
    for k, v in expected.items():
        if k not in seen:
            new[k] = v  # caso non eseguito in questo giro (--case): resta com'e'
    removed = sum(1 for k in expected if k in seen and k not in new)
    return new, added, changed, removed


def select_cases(names):
    import cases

    if not names:
        return list(cases.ALL)
    by_name = {c.NAME: c for c in cases.ALL}
    for n in names:
        if n not in by_name:
            raise BenchError(f"caso sconosciuto: {n} (disponibili: {', '.join(sorted(by_name))})")
    return [by_name[n] for n in names]


def exe_version(cmd):
    try:
        r = subprocess.run([*cmd, "--version"], capture_output=True, timeout=60)
        lines = r.stdout.decode("utf-8", errors="replace").strip().splitlines()
        return lines[0] if lines else "sconosciuta"
    except (OSError, subprocess.TimeoutExpired):
        return "sconosciuta"


def main(argv=None):
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")  # la console Windows e' cp1252
    ap = argparse.ArgumentParser(description="Banco di conformita DXF/DWG con oracolo ezdxf")
    ap.add_argument("exe", help="eseguibile OpenCADStudio (o uno script .py come convertitore di prova)")
    ap.add_argument("--out", help="scrive il report Markdown in questo file")
    ap.add_argument("--strict", action="store_true", help="exit 1 se c'e' una regressione")
    ap.add_argument("--update-expected", action="store_true", help="aggiorna expected.json con la misura")
    ap.add_argument("--note", help="nota per le divergenze nuove o cambiate (obbligatoria con --update-expected)")
    ap.add_argument("--case", action="append", help="esegue solo questo caso (ripetibile)")
    ap.add_argument("--keep", help="cartella dove tenere i file generati (altrimenti temporanea)")
    ap.add_argument("--expected", default=str(HERE / "expected.json"), help="percorso di expected.json")
    args = ap.parse_args(argv)

    if importlib.util.find_spec("ezdxf") is None:
        print("ezdxf non installato (pip install ezdxf): banco saltato")
        return 77
    try:
        if not (Path(args.exe).exists() or shutil.which(args.exe)):
            raise BenchError(f"eseguibile non trovato: {args.exe}")
        expected = load_expected(args.expected)
        selected = select_cases(args.case)
        cmd = converter_cmd(args.exe)
        work = Path(args.keep) if args.keep else Path(tempfile.mkdtemp(prefix="conformance_"))
        work.mkdir(parents=True, exist_ok=True)
        try:
            results = run_bench(cmd, selected, VERSIONS, expected, work)
        finally:
            if not args.keep:
                shutil.rmtree(work, ignore_errors=True)
        import ezdxf

        meta = [
            ("Eseguibile", exe_version(cmd)),
            ("ezdxf", ezdxf.__version__),
            ("Data", date.today().isoformat()),
            ("Casi", ", ".join(c.NAME for c in selected)),
        ]
        text = report.render(meta, results, expected)
        print(text)
        if args.out:
            Path(args.out).write_bytes(text.encode("utf-8"))
        if args.update_expected:
            new, added, changed, removed = updated_expected(expected, results, args.note)
            save_expected(args.expected, new)
            print(f"expected.json aggiornato: +{added} nuove, ~{changed} cambiate, -{removed} rimosse")
            return 0
    except (BenchError, ValueError) as ex:
        print(f"errore del banco: {ex}", file=sys.stderr)
        return 2
    return 1 if args.strict and any(r.state == REGRESSION for r in results) else 0


if __name__ == "__main__":
    sys.exit(main())
```

Nota sull'ordine: `--update-expected` senza `--note` deve fallire **senza** scrivere `expected.json` (`updated_expected` solleva prima di `save_expected`).

- [ ] **Passo 5: vedere i test passare**

Run: `python -m unittest discover -s tests/conformance -p "test_*.py" -v`
Expected: tutti passano. Tempo atteso: qualche decina di secondi (ogni conversione finta avvia Python + ezdxf). Se `test_report_survives_cp1252_console` fallisce con `UnicodeEncodeError`, la riconfigurazione dello stdout in `main` non sta funzionando: non indebolire il test.

- [ ] **Passo 6: verificare a mano che il banco possa davvero fallire**

Run:
```bash
FAKE_CONVERTER_MODE=corrupt-accents python tests/conformance/run.py tests/conformance/fake_converter.py --case text_accents --strict --expected "$TEMP/vuoto.json"; echo "exit=$?"
```
Expected: tabella con `REGRESSIONE` nella colonna `strings`, sezione «Regressioni (4)», `exit=1`.

- [ ] **Passo 7: commit**

```bash
git add tests/conformance/run.py tests/conformance/fake_converter.py tests/conformance/test_bench.py
git commit -m "Banco di conformita: orchestratore, CLI e convertitore finto che prova che il banco puo' fallire"
```

---

### Compito 4: prima misura sull'eseguibile reale e `expected.json`

**Files:**
- Create: `tests/conformance/expected.json`
- Create: `tests/conformance/README.md`

**Prerequisito:** `target\release\OpenCADStudio.exe` esiste (build di ffd82560 del 07/10/2026 15:39). Se manca o è più vecchio di `HEAD`, chiedere all'utente se vuole una build (`cargo build --release --locked --bin OpenCADStudio`, 4-5 min, fallisce se l'app è aperta: controllare con `tasklist | grep -i opencadstudio`, non chiuderla).

- [ ] **Passo 1: misura senza `expected.json`**

Run (da radice repo; ~54 avvii dell'eseguibile, può richiedere alcuni minuti):
```bash
python tests/conformance/run.py target/release/OpenCADStudio.exe --out "$TEMP/prima-misura.md" > "$TEMP/prima-misura.txt"; echo "exit=$?"
```
Expected: exit 0, tabella completa. Senza `expected.json` ogni divergenza è «REGRESSIONE»: è normale per la prima misura.

Se un caso risulta «conversione fallita» o «non leggibile» su **tutti** i controlli, non è una divergenza da registrare: è un problema da capire (percorso, versione dell'eseguibile, timeout). Riportarlo all'utente prima di andare avanti.

- [ ] **Passo 2: coerenza con la baseline del 03/10**

Aprire `$TEMP/prima-misura.md` e controllare che le divergenze note ricompaiano: accenti R2000 (`text`, `text_accents`, `attribs`: stringhe `è` → `Ã¨`), `POLYLINE`→`LWPOLYLINE` nel caso `geometry`, punti di fit della spline in `hatch_spline` R2000, e che gli stessi casi in R2018 non mostrino mojibake. **Qualsiasi divergenza che non sta in questo elenco** va riportata all'utente con il dettaglio, non registrata in silenzio: o è un difetto nuovo, o è un artefatto del banco.

- [ ] **Passo 3: registrare la misura**

Run:
```bash
python tests/conformance/run.py target/release/OpenCADStudio.exe --update-expected --note "prima misura 2026-10-07 (ffd82560): divergenza registrata, causa da classificare"
```
Expected: `expected.json aggiornato: +N nuove, ~0 cambiate, -0 rimosse`.

- [ ] **Passo 4: sostituire la nota generica con quella vera per le divergenze note**

Modificare a mano `tests/conformance/expected.json`: per le voci che corrispondono ai difetti noti, scrivere la nota vera, per esempio:
- accenti R2000: `Difetto noto: in R2000 ($DWGCODEPAGE=ANSI_1252) i caratteri accentati sono scritti come UTF-8 (docs: baseline-ocs-f0.md, punto aperto 4 di CLAUDE.md)`;
- `POLYLINE`→`LWPOLYLINE`: `Benigno: il codec converte le POLYLINE 2D in LWPOLYLINE (come il nostro lettore di Autocad_clone)`;
- spline: `Difetto da approfondire: contorno a spline di hatch R2000 perde i punti di fit (riserva: spline sintetica senza punti di controllo)`.

Lasciare la nota generica («da classificare») sulle voci che non corrispondono a nulla di noto, e riportarle all'utente.

- [ ] **Passo 5: la misura ripetuta è stabile**

Run:
```bash
python tests/conformance/run.py target/release/OpenCADStudio.exe --strict; echo "exit=$?"
```
Expected: `exit=0`, nessuna «REGRESSIONE», nessun «MIGLIORATO»; tutte le divergenze in «Attesi». Se compaiono voci che oscillano tra due esecuzioni, il dettaglio non è deterministico: trovare la fonte (di solito un valore con timestamp, un percorso o un ordine) e correggerla in `oracle.py` con un test, non nascondere la voce.

- [ ] **Passo 6: scrivere il README**

`tests/conformance/README.md`:

````markdown
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

## Come leggere il report

Per ogni caso, versione (R2000, R2018), percorso (DXF>DXF, DXF>DWG>DXF) e controllo (`readable`, `types`,
`extents`, `strings`):

| Stato | Significato |
|---|---|
| OK | coincide con la sorgente |
| ATTESO | diverge, e la divergenza e' in `expected.json` con una nota |
| REGRESSIONE | peggio di quanto registrato, o divergenza nuova. Un dettaglio diverso da quello registrato conta come regressione |
| MIGLIORATO | era una divergenza registrata e ora coincide |

**Limite:** il percorso DWG e' indiretto. ezdxf non legge DWG: il DWG scritto da ArchLine viene riletto da
ArchLine stesso e confrontato dopo l'export in DXF. Inoltre si misura il convertitore headless (`--export`),
non l'apertura/salvataggio interattivi.

## expected.json

Un'entry per divergenza: `"caso|versione|percorso|controllo": {"detail": "...", "note": "..."}`. Nessuna entry
senza nota (il banco la rifiuta). Per aggiornarlo dopo una misura: `--update-expected --note "motivo"`; le entry
invariate tengono la loro nota, quelle ora OK vengono tolte, le nuove o cambiate prendono la nota passata.
Dopo, rifinire a mano le note.

## Aggiungere un caso

Un modulo in `cases/` con `NAME` e `build(version, work) -> Path` (usare `cases/_common.save`), poi aggiungerlo a
`cases.ALL`. Solo disegni sintetici generati da codice: il repo e' pubblico.

## Test del banco

```
python -m unittest discover -s tests/conformance -p "test_*.py" -v
```
`fake_converter.py` imita `--export` e puo' corrompere gli accenti, perdere un'entita' o andare in crash
(`FAKE_CONVERTER_MODE`): prova che il banco *puo'* fallire, non solo passare.
````

- [ ] **Passo 7: commit**

```bash
git add tests/conformance/expected.json tests/conformance/README.md
git commit -m "Banco di conformita: prima misura sull'eseguibile (expected.json) e README"
```

---

### Compito 5: CI, CLAUDE.md e allineamento della spec

**Files:**
- Modify: `.github/workflows/archline-windows.yml`
- Modify: `CLAUDE.md`
- Modify: `docs/superpowers/specs/2026-10-07-banco-conformita-design.md`

- [ ] **Passo 1: aggiungere il passo al workflow**

In `.github/workflows/archline-windows.yml`, dopo il passo `Package` e prima di `actions/upload-artifact@v4`, inserire:

```yaml
      - uses: actions/setup-python@v5
        with:
          python-version: "3.12"

      # Misura, non blocca: il report entra nell'artefatto. Per promuoverlo a cancello, togliere continue-on-error.
      - name: Conformance (ezdxf come oracolo)
        continue-on-error: true
        shell: pwsh
        run: |
          python -m pip install ezdxf
          python -m unittest discover -s tests/conformance -p "test_*.py"
          python tests/conformance/run.py target\release\OpenCADStudio.exe --out dist\conformance-report.md
```

- [ ] **Passo 2: validare il YAML**

Run: `python -c "import yaml,sys; yaml.safe_load(open('.github/workflows/archline-windows.yml',encoding='utf-8')); print('yaml ok')"` (se `yaml` manca: `python -m pip install pyyaml`).
Expected: `yaml ok`. **Il passo non è verificato in CI finché l'utente non fa push**: dirlo nel riepilogo.

- [ ] **Passo 3: aggiornare CLAUDE.md**

In «Build e test», aggiungere dopo il blocco `cargo`:

```
python -m pip install ezdxf
python -m unittest discover -s tests/conformance -p "test_*.py"          # test del banco di conformità
python tests/conformance/run.py target\release\OpenCADStudio.exe          # misura DXF/DWG con oracolo ezdxf
```

In «Cosa è ArchLine» aggiungere una voce: `tests/conformance/`: banco di conformità (ezdxf come oracolo, `expected.json` con gli esiti noti, report `ATTESO/REGRESSIONE/MIGLIORATO`); misura e non blocca; spec `docs/superpowers/specs/2026-10-07-banco-conformita-design.md`; passo non bloccante nel workflow Windows. In «Aperti» togliere «banco di conformità (oracolo ezdxf; materiale in …)» e aggiungere, se serve, «banco: comandi pilotati dall'API di automazione, disegni reali, promozione a cancello».

- [ ] **Passo 4: allineare la spec a ciò che è stato fatto**

Nella spec, correggere questi punti (differenze emerse nel piano):
- §3 la struttura comprende anche `classify.py`, `report.py`, `fake_converter.py`, `test_bench.py`;
- §4 `text` e `hatch_spline` esistevano già nello script di partenza e sono stati portati; il caso davvero nuovo è `text_accents`;
- §5 «Migliorato» = era una divergenza registrata e ora il controllo è OK; un dettaglio diverso da quello registrato conta come **Regressione** (i dettagli non hanno un ordine, quindi non si può dire quale diverga di meno); `--update-expected` richiede `--note`, e non scrive nulla se manca;
- §8 i test usano `unittest` e `fake_converter.py` (modi `faithful`, `corrupt-accents`, `drop-entity`, `crash`), non un convertitore «script finto» generico.

- [ ] **Passo 5: test del banco e `cargo check` non toccato**

Run: `python -m unittest discover -s tests/conformance -p "test_*.py"`
Expected: tutti passano. Non serve `cargo` (nessun file Rust cambiato); `git status --short` deve mostrare solo i file di questo piano.

- [ ] **Passo 6: commit**

```bash
git add .github/workflows/archline-windows.yml CLAUDE.md docs/superpowers/specs/2026-10-07-banco-conformita-design.md
git commit -m "Banco di conformita: passo non bloccante nel workflow Windows, CLAUDE.md e spec allineati"
```

---

## Autorevisione del piano

- **Copertura della spec:** §1-2 scopo e origine → vincoli globali e Compito 2 (casi sintetici). §3 struttura → tabella dei file. §4 casi e percorsi → Compito 2 (nove casi, `text_accents` nuovo) e `measure` (Compito 3). §5 stati, `--update-expected`, exit code → `classify.py` (Compito 1) e `main` (Compito 3). §6 report → `report.py` (Compito 1). §7 CI → Compito 5. §8 test del banco → Compiti 1-3, con convertitore finto. §9 fuori giro: nessun compito. §10 rischi: la prima misura (Compito 4) tratta le divergenze sconosciute.
- **Differenze dalla spec, dichiarate:** `classify.py`/`report.py`/`fake_converter.py`/`test_bench.py` in più; «Migliorato» definito solo come «ora OK»; `--note` obbligatorio; il Compito 5 corregge la spec.
- **Segnaposto:** nessuno. Il Compito 4 non ha codice da scrivere ma comandi e criteri di decisione, perché dipende da valori misurati.
- **Coerenza dei nomi:** `Result(case, version, path, check, ok, detail, state)`, `key()`, `classify()`, `compare()`, `failed()`, `run_bench()`, `updated_expected()` e `BenchError` hanno le stesse firme in tutti i compiti. `PATHS = ("dxf", "dwg")` coincide con `PATH_LABEL` in `report.py`.
- **Non verificabile qui:** il passo CI (serve push su `claude/**`); il comportamento dell'eseguibile su disegni reali (fuori giro).
