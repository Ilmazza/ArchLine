"""Test delle funzioni di bim_extract.py (l'estrazione BIM del banco). Non lanciano ArchLine: il server
`--serve` e' imitato da un piccolo script Python. Da lanciare dalla radice del repo:
python -m unittest discover -s tests/conformance -p "test_*.py" -v
"""
import contextlib
import importlib.util
import io
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path
from unittest import mock

import bim_extract as be

HAVE_EZDXF = importlib.util.find_spec("ezdxf") is not None

FAKE_SERVER = textwrap.dedent(
    """
    import json, sys
    print("rumore di avvio, non JSON", flush=True)
    print(json.dumps({"ok": True, "ready": True, "version": "fake"}), flush=True)
    for line in sys.stdin:
        req = json.loads(line)
        if req.get("op") == "mute":
            continue  # non risponde mai
        print(json.dumps({"ok": True, "echo": req}), flush=True)
    """
)


class SegmentTests(unittest.TestCase):
    def test_equal_in_either_direction(self):
        a = ((0.0, 0.0), (4.0, 0.0))
        self.assertTrue(be.segments_equal(a, ((0.0, 0.0), (4.0, 0.0))))
        self.assertTrue(be.segments_equal(a, ((4.0, 0.0), (0.0, 0.0))))

    def test_tolerance_is_inclusive_and_per_coordinate(self):
        a = ((0.0, 0.0), (4.0, 0.0))
        self.assertTrue(be.segments_equal(a, ((0.0, 0.0), (4.0 + 0.9e-6, 0.0))))
        self.assertFalse(be.segments_equal(a, ((0.0, 0.0), (4.0 + 2e-6, 0.0))))
        self.assertFalse(be.segments_equal(a, ((0.0, 0.0), (4.0, 1e-3))))

    def test_a_point_swapped_across_segments_is_not_equal(self):
        self.assertFalse(be.segments_equal(((0, 0), (4, 0)), ((0, 0), (0, 4))))

    def test_match_reports_missing_and_extra(self):
        want = [((0, 0), (4, 0)), ((4, 0), (4, 3))]
        got = [((4, 3), (4, 0)), ((9, 9), (9, 10))]
        missing, extra = be.match_segments(want, got)
        self.assertEqual(missing, [((0, 0), (4, 0))])
        self.assertEqual(extra, [((9, 9), (9, 10))])

    def test_identical_sets_match_in_any_order(self):
        want = [((0, 0), (4, 0)), ((4, 0), (4, 3)), ((4, 3), (0, 3))]
        got = [((0, 3), (4, 3)), ((0, 0), (4, 0)), ((4, 3), (4, 0))]
        self.assertEqual(be.match_segments(want, got), ([], []))

    def test_one_found_segment_serves_one_expected_only(self):
        seg = ((0, 0), (4, 0))
        missing, extra = be.match_segments([seg, seg], [seg])
        self.assertEqual((len(missing), len(extra)), (1, 0))


class VerticalOverlapTests(unittest.TestCase):
    def test_finds_a_segment_along_the_line(self):
        segs = [((4.0, 0.0), (4.0, 3.0))]
        self.assertEqual(be.vertical_overlaps(segs, 4.0, 0.0, 3.0), segs)

    def test_partial_overlap_counts(self):
        segs = [((4.0, 2.0), (4.0, 5.0))]
        self.assertEqual(be.vertical_overlaps(segs, 4.0, 0.0, 3.0), segs)

    def test_touching_only_at_the_end_does_not_count(self):
        segs = [((4.0, 3.0), (4.0, 5.0)), ((4.0, -2.0), (4.0, 0.0))]
        self.assertEqual(be.vertical_overlaps(segs, 4.0, 0.0, 3.0), [])

    def test_other_x_or_slanted_segments_do_not_count(self):
        segs = [((4.001, 0.0), (4.001, 3.0)), ((4.0, 0.0), (4.1, 3.0)), ((4.0, 0.0), (4.1, 0.0))]
        self.assertEqual(be.vertical_overlaps(segs, 4.0, 0.0, 3.0), [])

    def test_segment_given_top_down_counts(self):
        segs = [((4.0, 3.0), (4.0, 0.0))]
        self.assertEqual(be.vertical_overlaps(segs, 4.0, 0.0, 3.0), segs)


class WorkBaseTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="bim_test_"))

    def test_has_space(self):
        self.assertTrue(be.has_space("C:\\Users\\Mauro Mazzarelli\\Temp"))
        self.assertFalse(be.has_space("C:\\ocs-bench"))

    def test_candidate_with_spaces_is_skipped(self):
        spaced = self.tmp / "con spazi"
        good = self.tmp / "buona"
        self.assertEqual(be.pick_work_base([spaced, good]), good)
        self.assertTrue(good.is_dir())
        self.assertFalse(spaced.exists())

    def test_missing_folder_is_created(self):
        wanted = self.tmp / "a" / "b"
        self.assertEqual(be.pick_work_base([wanted]), wanted)
        self.assertTrue(wanted.is_dir())

    def test_no_usable_candidate_is_a_clear_error(self):
        with self.assertRaises(be.BenchError) as ctx:
            be.pick_work_base([self.tmp / "con spazi"])
        self.assertIn("senza spazi", str(ctx.exception))
        self.assertIn("con spazi", str(ctx.exception))

    def test_uncreatable_candidate_is_skipped(self):
        blocker = self.tmp / "file"
        blocker.write_text("x", encoding="utf-8")
        good = self.tmp / "buona"
        self.assertEqual(be.pick_work_base([blocker / "sotto", good]), good)


class ReportTests(unittest.TestCase):
    def test_lines_and_counts(self):
        out = io.StringIO()
        rep = be.Report(out)
        self.assertTrue(rep.check(True, "uno", "bene"))
        self.assertFalse(rep.check(False, "due", "male"))
        rep.check(True, "tre")
        lines = out.getvalue().splitlines()
        self.assertTrue(lines[0].startswith("OK") and lines[0].endswith("uno: bene"))
        self.assertTrue(lines[1].startswith("FALLITO") and lines[1].endswith("due: male"))
        self.assertTrue(lines[2].startswith("OK") and lines[2].endswith("tre"))
        self.assertEqual((rep.total, rep.failed), (3, 1))
        self.assertIn("3 controlli, 2 OK, 1 FALLITI", rep.summary())

    def test_prefixed_checks_land_in_the_same_report(self):
        out = io.StringIO()
        rep = be.Report(out)
        rep.check(True, "open")
        past = be.Prefixed(rep, be.PAST_PREFIX)
        self.assertFalse(past.check(False, "open", "male"))
        lines = out.getvalue().splitlines()
        self.assertTrue(lines[0].endswith(" open"))
        self.assertTrue(lines[1].startswith("FALLITO") and lines[1].endswith("asse oltre le facce, open: male"))
        self.assertEqual((rep.total, rep.failed), (2, 1))


@unittest.skipUnless(HAVE_EZDXF, "ezdxf non installato")
class DrawingTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="bim_test_"))

    def test_case_a_is_twelve_lines_on_muri_in_metres(self):
        import ezdxf

        path = be.build_case_a(self.tmp / "casoA.dxf")
        doc = ezdxf.readfile(path)
        self.assertEqual(doc.dxfversion, "AC1032")
        self.assertEqual(doc.header["$INSUNITS"], 6)
        lines = be.lines_of(doc, "MURI")
        self.assertEqual(len(lines), 12)
        self.assertEqual(len(be.lines_of(doc)), 12)
        # il tramezzo: le due facce sono i lati dei due locali
        self.assertTrue(any(be.segments_equal(s, ((4.0, 0.0), (4.0, 3.0))) for s in lines))
        self.assertTrue(any(be.segments_equal(s, ((4.1, 0.0), (4.1, 3.0))) for s in lines))

    def test_arch_xdata_appids_sees_xdata_and_table_entries(self):
        import ezdxf

        doc = ezdxf.new("R2018")
        self.assertEqual(be.arch_xdata_appids(doc), ([], []))
        doc.appids.add("ARCHLINE_ZONA")
        line = doc.modelspace().add_line((0, 0), (1, 0))
        line.set_xdata("ARCHLINE_ZONA", [(1000, "x")])
        self.assertEqual(be.arch_xdata_appids(doc), (["ARCHLINE_ZONA"], ["ARCHLINE_ZONA"]))

    def test_other_xdata_is_not_ours(self):
        import ezdxf

        doc = ezdxf.new("R2018")
        doc.appids.add("ALTRO")
        doc.modelspace().add_line((0, 0), (1, 0)).set_xdata("ALTRO", [(1000, "x")])
        self.assertEqual(be.arch_xdata_appids(doc), ([], []))


GOOD_CSV = (
    "﻿" + ";".join(be.CSV_HEADER) + "\r\n"
    "demolizione;D1;Demolizione muro spessore 0,100;1,0000;3,0000;0,1000;2,7000;no;0,8100;m³;ereditata;zona 4A;\r\n"
    "nuova muratura;N1;Nuova muratura spessore 0,120;1,0000;2,0000;;2,7000;no;5,4000;m²;ereditata;zona 4B;\r\n"
)


class AbacoCsvTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="bim_test_"))

    def write(self, text, name="abaco.csv"):
        path = self.tmp / name
        path.write_bytes(text.encode("utf-8"))
        return path

    def report(self):
        return be.Report(io.StringIO())

    def test_italian_numbers(self):
        self.assertEqual(be.parse_it_number("0,8100"), 0.81)
        self.assertEqual(be.parse_it_number("-1,5000"), -1.5)
        self.assertIsNone(be.parse_it_number(""))
        for bad in ("0.8100", "0,81", "1.234,5000", "0,81000", "n.d.", "1e3"):
            with self.assertRaises(ValueError, msg=bad):
                be.parse_it_number(bad)

    def test_reads_bom_crlf_header_and_rows(self):
        bom, crlf, header, rows = be.read_abaco_csv(self.write(GOOD_CSV))
        self.assertTrue(bom and crlf)
        self.assertEqual(header, be.CSV_HEADER)
        self.assertEqual([r["codice"] for r in rows], ["D1", "N1"])
        self.assertEqual(rows[0]["um"], "m³")
        self.assertEqual(rows[1]["larghezza"], "")

    def test_without_bom_or_with_bare_lf_is_noticed(self):
        bom, crlf, _, _ = be.read_abaco_csv(self.write(GOOD_CSV.replace("﻿", "")))
        self.assertFalse(bom)
        self.assertTrue(crlf)
        bom, crlf, _, _ = be.read_abaco_csv(self.write(GOOD_CSV.replace("\r\n", "\n")))
        self.assertTrue(bom)
        self.assertFalse(crlf)

    def test_a_row_with_the_wrong_field_count_is_an_error(self):
        with self.assertRaises(ValueError):
            be.read_abaco_csv(self.write(GOOD_CSV + "demolizione;D2;x\r\n"))

    def test_product_skips_empty_factors(self):
        _, _, _, rows = be.read_abaco_csv(self.write(GOOD_CSV))
        self.assertAlmostEqual(be.csv_product(rows[0]), 0.81)
        self.assertAlmostEqual(be.csv_product(rows[1]), 5.4)  # larghezza vuota: non si applica
        self.assertEqual(be.csv_problems(rows), [])

    def test_quantity_off_by_more_than_the_tolerance_is_a_problem(self):
        _, _, _, rows = be.read_abaco_csv(self.write(GOOD_CSV.replace("0,8100;m³", "0,8200;m³")))
        problems = be.csv_problems(rows)
        self.assertEqual(len(problems), 1)
        self.assertIn("D1", problems[0])
        # il prodotto dei fattori scritti, 2,99967, e' scritto 2,9997: scarto 3e-5, entro la regola di to_csv
        row = dict(rows[0], lunghezza="3,3333", larghezza="0,3333", altezza="2,7000", quantita="2,9997")
        self.assertEqual(be.csv_problems([row]), [])

    def test_empty_height_is_a_problem_except_for_rooms(self):
        _, _, _, rows = be.read_abaco_csv(self.write(GOOD_CSV.replace("2,7000;no;5,4000", ";no;2,0000")))
        self.assertTrue(any("altezza vuota" in p for p in be.csv_problems(rows)))
        room = dict(rows[1], tipo="locale di fatto", codice="LF1", quantita="2,0000", altezza="")
        self.assertEqual(be.csv_problems([room]), [])

    def test_a_malformed_number_is_a_problem_not_a_crash(self):
        _, _, _, rows = be.read_abaco_csv(self.write(GOOD_CSV.replace("0,8100;m³", "0.81;m³")))
        self.assertTrue(any("D1" in p for p in be.csv_problems(rows)))

    def test_check_abaco_csv_passes_the_good_file(self):
        rep = self.report()
        rows = be.check_abaco_csv(rep, self.write(GOOD_CSV))
        self.assertEqual(rep.failed, 0, rep.out.getvalue())
        self.assertEqual(len(rows), 2)

    def test_check_abaco_csv_fails_on_a_wrong_quantity_and_a_missing_file(self):
        rep = self.report()
        be.check_abaco_csv(rep, self.write(GOOD_CSV.replace("0,8100;m³", "0,9000;m³")))
        self.assertGreaterEqual(rep.failed, 2, rep.out.getvalue())  # il valore atteso e il prodotto dei fattori
        rep = self.report()
        self.assertIsNone(be.check_abaco_csv(rep, self.tmp / "manca.csv"))
        self.assertEqual(rep.failed, 1)

    def test_check_abaco_csv_fails_on_a_wrong_header(self):
        rep = self.report()
        be.check_abaco_csv(rep, self.write(GOOD_CSV.replace("stato_altezza", "stato")))
        self.assertIn("FALLITO  abaco CSV: intestazione", rep.out.getvalue())

    def test_two_decimals_is_the_table_cell_text(self):
        self.assertEqual(be.two_decimals("0,8100"), "0,81")
        self.assertEqual(be.two_decimals("5,4000"), "5,40")

    @unittest.skipUnless(HAVE_EZDXF, "ezdxf non installato")
    def test_a_dxf_without_tables_fails(self):
        import ezdxf

        path = self.tmp / "senza.dxf"
        ezdxf.new("R2018").saveas(path)
        rep = self.report()
        with contextlib.redirect_stdout(io.StringIO()) as out:
            be.check_abaco_tables(rep, path, [])
        self.assertEqual(rep.failed, 1, rep.out.getvalue())
        self.assertIn("0 ACAD_TABLE invece di 2", rep.out.getvalue())
        self.assertNotIn(be.TABLE_NOT_VERIFIED, out.getvalue())
        self.assertEqual(be.table_texts(ezdxf.readfile(path)), [])

    @unittest.skipUnless(HAVE_EZDXF, "ezdxf non installato")
    def test_two_tables_with_an_empty_block_are_a_declared_limit_not_a_failure(self):
        import ezdxf

        path = self.tmp / "vuote.dxf"
        ezdxf.new("R2018").saveas(path)  # ezdxf non crea ACAD_TABLE: le due tabelle senza testi sono imitate
        rep = self.report()
        with mock.patch.object(be, "table_texts", return_value=[[], []]),                 contextlib.redirect_stdout(io.StringIO()) as out:
            be.check_abaco_tables(rep, path, [])
        self.assertEqual(rep.failed, 0, rep.out.getvalue())
        self.assertIn(be.TABLE_NOT_VERIFIED, out.getvalue())

    def test_a_missing_table_file_fails(self):
        rep = self.report()
        be.check_abaco_tables(rep, self.tmp / "manca.dxf", [])
        self.assertEqual(rep.failed, 1)


class TouchesTests(unittest.TestCase):
    def test_either_end_within_the_tolerance(self):
        seg = ((20.0, 5.0), (30.0, 5.0))
        self.assertTrue(be.touches((20.0, 5.0), seg))
        self.assertTrue(be.touches((30.0 + 0.9e-6, 5.0), seg))
        self.assertFalse(be.touches((25.0, 5.0), seg))  # dentro il segmento, non un estremo
        self.assertFalse(be.touches((20.0, 5.0 + 2e-6), seg))


@unittest.skipUnless(HAVE_EZDXF, "ezdxf non installato")
class AbacoExtractTests(unittest.TestCase):
    """L'abaco nell'estrazione: i documenti sono fatti con ezdxf (che non scrive ACAD_TABLE)."""

    CORNER = (20.0, 5.0)

    def docs(self, texts=("D1", "0,90", "N1", "6,00"), corner=CORNER):
        import ezdxf

        docs = {key: ezdxf.new("R2018") for key in be.SUFFIX}
        msp = docs["comparativa"].modelspace()
        msp.add_line(corner, (corner[0] + 10.0, corner[1]), dxfattribs={"layer": be.LAYER_ABACO})
        for text in texts:
            msp.add_mtext(text, dxfattribs={"layer": be.LAYER_ABACO})
        msp.add_mtext("0,81", dxfattribs={"layer": "ALTRO"})  # un numero vecchio su un altro layer non conta
        return docs

    def run_check(self, docs, corners=(CORNER,)):
        rep = be.Report(io.StringIO())
        be.check_abaco_extract(rep, docs, list(corners))
        return rep

    def test_abaco_texts_are_the_mtext_on_al_abaco(self):
        docs = self.docs()
        self.assertEqual(sorted(be.abaco_texts(docs["comparativa"])), ["0,90", "6,00", "D1", "N1"])
        self.assertEqual(be.abaco_texts(docs["fatto"]), [])
        self.assertEqual(be.table_corners(docs["comparativa"]), [])

    def test_fresh_numbers_at_the_table_corner_pass(self):
        rep = self.run_check(self.docs())
        self.assertEqual(rep.failed, 0, rep.out.getvalue())

    def test_a_stale_number_or_a_missing_fresh_one_fails(self):
        rep = self.run_check(self.docs(texts=("D1", "0,81", "N1", "6,00")))
        out = rep.out.getvalue()
        self.assertIn("FALLITO  comparativa: riga D1 ricalcolata", out)
        self.assertIn("FALLITO  comparativa: niente numeri della tabella del disegno di lavoro", out)
        self.assertEqual(rep.failed, 2, out)

    def test_a_grid_away_from_the_table_or_no_table_fails(self):
        rep = self.run_check(self.docs(corner=(21.0, 5.0)))
        self.assertIn("FALLITO  comparativa: tabelle nel punto del disegno di lavoro", rep.out.getvalue())
        rep = self.run_check(self.docs(), corners=())
        self.assertEqual(rep.failed, 1, rep.out.getvalue())

    def test_abaco_in_fatto_or_progetto_fails(self):
        docs = self.docs()
        docs["progetto"].modelspace().add_mtext("D1", dxfattribs={"layer": be.LAYER_ABACO})
        rep = self.run_check(docs)
        self.assertIn("FALLITO  progetto: niente abaco", rep.out.getvalue())
        self.assertEqual(rep.failed, 1, rep.out.getvalue())


class SessionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="bim_test_"))
        self.script = self.tmp / "fake_serve.py"
        self.script.write_text(FAKE_SERVER, encoding="utf-8")

    def _session(self, script=None):
        return be.Session([sys.executable, str(script or self.script)], None, self.tmp, self.tmp / "err.txt")

    def test_ready_line_is_found_after_noise_and_requests_are_answered(self):
        s = self._session()
        try:
            self.assertEqual(s.ready["version"], "fake")
            self.assertEqual(s.request({"op": "run", "cmd": "X"})["echo"], {"op": "run", "cmd": "X"})
            self.assertEqual(s.request({"op": "open", "path": "a"})["echo"]["path"], "a")
        finally:
            self.assertEqual(s.close(), 0)

    def test_exit_before_ready_is_a_bench_error(self):
        script = self.tmp / "dies.py"
        script.write_text("import sys; sys.stderr.write('boom'); sys.exit(3)", encoding="utf-8")
        with self.assertRaises(be.BenchError) as ctx:
            self._session(script)
        self.assertIn("uscita", str(ctx.exception))

    def test_unanswered_request_is_a_bench_error(self):
        s = self._session()
        try:
            with mock.patch.object(be, "REQUEST_TIMEOUT", 1):
                with self.assertRaises(be.BenchError) as ctx:
                    s.request({"op": "mute"})
            self.assertIn("nessuna risposta", str(ctx.exception))
        finally:
            s.close()

    def test_missing_executable_is_a_bench_error(self):
        with self.assertRaises(be.BenchError):
            be.Session([str(self.tmp / "non_esiste.exe"), "--serve"], None, self.tmp, self.tmp / "err.txt")


class MainTests(unittest.TestCase):
    def _main(self, *argv):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            rc = be.main(list(argv))
        return rc, out.getvalue(), err.getvalue()

    @unittest.skipUnless(HAVE_EZDXF, "ezdxf non installato")
    def test_missing_executable_is_a_bench_error(self):
        rc, _, err = self._main(str(Path(tempfile.gettempdir()) / "non_esiste_ArchLine.exe"))
        self.assertEqual(rc, 2)
        self.assertIn("eseguibile non trovato", err)

    @unittest.skipUnless(HAVE_EZDXF, "ezdxf non installato")
    def test_keep_folder_must_be_new_or_empty(self):
        tmp = Path(tempfile.mkdtemp(prefix="bim_test_"))
        (tmp / "avanzo.txt").write_text("x", encoding="utf-8")
        fake_exe = Path(sys.executable)  # esiste: la prova si ferma sulla cartella, prima di lanciarlo
        rc, _, err = self._main(str(fake_exe), "--keep", str(tmp))
        self.assertEqual(rc, 2)
        self.assertIn("nuova o vuota", err)

    @unittest.skipUnless(HAVE_EZDXF, "ezdxf non installato")
    def test_keep_folder_with_spaces_is_refused(self):
        rc, _, err = self._main(str(Path(sys.executable)), "--keep", str(Path(tempfile.gettempdir()) / "con spazi"))
        self.assertEqual(rc, 2)
        self.assertIn("spazi", err)

    def test_missing_ezdxf_skips_with_77(self):
        with mock.patch.object(importlib.util, "find_spec", return_value=None):
            rc, out, _ = self._main("qualunque.exe")
        self.assertEqual(rc, 77)
        self.assertIn("ezdxf non installato", out)


if __name__ == "__main__":
    unittest.main()
