"""Test del banco di conformita. Da lanciare dalla radice del repo:
python -m unittest discover -s tests/conformance -p "test_*.py" -v
"""
import contextlib
import importlib.util
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import classify
import report
from classify import EXPECTED, IMPROVED, OK, REGRESSION, Result

HAVE_EZDXF = importlib.util.find_spec("ezdxf") is not None


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
        data = {"b|R2000|dxf|strings": {"detail": "[('è', 'Ã¨')]", "note": "perché"},
                "a|R2000|dxf|types": {"detail": "x", "note": "n"}}
        classify.save_expected(self.path, data)
        raw = self.path.read_bytes()
        self.assertNotIn(b"\r\n", raw)
        self.assertIn("perché".encode("utf-8"), raw)
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
        res = [_res("t", "R2000", "dxf", "strings", False, "[('è', 'Ã¨')]", REGRESSION)]
        text = report.render([], res, {})
        self.assertEqual(text.encode("utf-8").decode("utf-8"), text)
        self.assertIn("Ã¨", text)


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

    def test_every_differing_string_appears_in_the_detail(self):
        # un dettaglio che ne mostra solo alcune maschera una regressione nelle altre
        def make(name, texts):
            doc = self.ezdxf.new("R2018")
            for i, t in enumerate(texts):
                doc.modelspace().add_text(t, height=2).set_placement((0, 10 * i))
            path = self.tmp / name
            doc.saveas(path)
            return path

        a = make("a.dxf", ["uno", "due", "tre", "quattro"])
        b = make("b.dxf", ["UNO", "DUE", "TRE", "quattro"])
        snap = self.oracle.snapshot(self.oracle.read_strict(a)[0])
        ok, detail = self.oracle.compare(snap, b)["strings"]
        self.assertFalse(ok)
        for wrong in ("UNO", "DUE", "TRE"):
            self.assertIn(wrong, detail)
        self.assertNotIn("quattro", detail)

    def test_text_length_does_not_move_the_extents(self):
        # ezdxf stima l'estensione di TEXT, MTEXT e ATTRIB dalla lunghezza della stringa: non e' un dato
        # geometrico, e un difetto sugli accenti non deve comparire anche come difetto di estensione.
        def make(name, text):
            doc = self.ezdxf.new("R2018")
            msp = doc.modelspace()
            msp.add_line((0, 0), (10, 0))
            msp.add_text(text, height=2).set_placement((20, 0))
            msp.add_mtext(text, dxfattribs={"insert": (20, 10), "char_height": 2})
            blk = doc.blocks.new("T")
            blk.add_attdef("N", (0, 0), "-", dxfattribs={"height": 1})
            msp.add_blockref("T", (40, 0)).add_auto_attribs({"N": text})
            path = self.tmp / name
            doc.saveas(path)
            return path

        short = make("short.dxf", "e")
        long_ = make("long.dxf", "e" * 40)
        snap = self.oracle.snapshot(self.oracle.read_strict(short)[0])
        res = self.oracle.compare(snap, long_)
        self.assertTrue(res["extents"][0], res["extents"])
        self.assertFalse(res["strings"][0])  # il testo e' comunque diverso

    def test_text_position_still_moves_the_extents(self):
        # senza testi le estensioni non vedrebbero piu' inserimento, altezza, rotazione, allineamento
        def make(name, x):
            doc = self.ezdxf.new("R2018")
            doc.modelspace().add_line((0, 0), (10, 0))
            doc.modelspace().add_text("ciao", height=2).set_placement((x, 0))
            path = self.tmp / name
            doc.saveas(path)
            return path

        here, moved = make("here.dxf", 20), make("moved.dxf", 50)
        snap = self.oracle.snapshot(self.oracle.read_strict(here)[0])
        self.assertTrue(self.oracle.compare(snap, here)["extents"][0])
        self.assertFalse(self.oracle.compare(snap, moved)["extents"][0])

    def test_negative_zero_is_not_printed_in_the_extents_detail(self):
        # -2.9e-16 (rumore numerico della spline) stampato come '-0.00' oscillerebbe tra build di ezdxf
        text = self.oracle._fmt_box((-2.9e-16, 0.0, 10.0, 3.0))
        self.assertNotIn("-0.00", text)
        self.assertEqual(text, "(0.00, 0.00, 10.00, 3.00)")

    def test_dxf_version_change_fails_the_readable_check(self):
        # se l'export riscrivesse sempre in R2018, lo scenario R2000 non verrebbe piu' esercitato
        def make(name, version):
            doc = self.ezdxf.new(version)
            doc.modelspace().add_line((0, 0), (1, 1))
            path = self.tmp / name
            doc.saveas(path)
            return path

        r2000, r2018 = make("a.dxf", "R2000"), make("b.dxf", "R2018")
        snap = self.oracle.snapshot(self.oracle.read_strict(r2000)[0])
        self.assertEqual(snap["version"], "AC1015")
        self.assertTrue(self.oracle.compare(snap, r2000)["readable"][0])
        ok, detail = self.oracle.compare(snap, r2018)["readable"]
        self.assertFalse(ok)
        self.assertEqual(detail, "versione AC1015 -> AC1032")

    def test_type_difference_names_both_counts(self):
        a = self._save("a.dxf", lambda m: (m.add_line((0, 0), (1, 1)), m.add_circle((0, 0), 1)))
        b = self._save("b.dxf", lambda m: m.add_line((0, 0), (1, 1)))
        snap = self.oracle.snapshot(self.oracle.read_strict(a)[0])
        ok, detail = self.oracle.compare(snap, b)["types"]
        self.assertFalse(ok)
        self.assertIn("'CIRCLE': (1, 0)", detail)

    def test_string_difference_is_reported(self):
        a = self._save("a.dxf", lambda m: m.add_text("perché").set_placement((0, 0)))
        b = self._save("b.dxf", lambda m: m.add_text("perchÃ©").set_placement((0, 0)))
        snap = self.oracle.snapshot(self.oracle.read_strict(a)[0])
        ok, detail = self.oracle.compare(snap, b)["strings"]
        self.assertFalse(ok)
        self.assertIn("perchÃ©", detail)

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
            for ch in "èàòùìé":
                self.assertIn(ch, joined, f"{version}: manca {ch!r}")


FAKE = str(Path(__file__).resolve().parent / "fake_converter.py")


class ConverterCmdTests(unittest.TestCase):
    def setUp(self):
        import run
        self.run = run

    def test_relative_path_with_forward_slashes_becomes_absolute(self):
        # su Windows subprocess non trova 'target/release/x.exe' (barre /) ma trova la forma assoluta
        rel = os.path.relpath(FAKE).replace("\\", "/")
        self.assertFalse(os.path.isabs(rel))
        exe = self.run.converter_cmd(rel)[-1]
        self.assertTrue(os.path.isabs(exe), exe)
        self.assertTrue(os.path.exists(exe), exe)

    def test_script_is_run_with_the_current_python(self):
        self.assertEqual(self.run.converter_cmd(FAKE)[0], sys.executable)

    def test_bare_name_is_left_for_the_path_lookup(self):
        self.assertEqual(self.run.converter_cmd("OpenCADStudio"), ["OpenCADStudio"])


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
        self.assertIn("Ã", detail)

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
        self.assertIn("Ã", text)

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

    def test_existing_file_that_is_not_an_executable_is_a_bench_error(self):
        # un .exe troncato da una build interrotta, o un file passato per sbaglio: OSError, non un traceback
        not_exe = self.tmp / "non_eseguibile.bin"
        not_exe.write_bytes(b"questo non e' un programma")
        rc, _, err = self._main("faithful", "--case", "paperspace", exe=str(not_exe))
        self.assertEqual(rc, 2)
        self.assertIn("convertitore non eseguibile", err)

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


if __name__ == "__main__":
    unittest.main()
