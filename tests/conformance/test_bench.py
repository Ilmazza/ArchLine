"""Test del banco di conformita. Da lanciare dalla radice del repo:
python -m unittest discover -s tests/conformance -p "test_*.py" -v
"""
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

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


if __name__ == "__main__":
    unittest.main()
