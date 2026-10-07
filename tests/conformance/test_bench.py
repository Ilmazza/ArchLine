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


if __name__ == "__main__":
    unittest.main()
