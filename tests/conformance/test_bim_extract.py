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
