#!/usr/bin/env python3
"""Banco di conformita, estrazione BIM: lancia `ArchLine --serve`, esegue sul caso A (due locali affiancati,
tramezzo da 0,10 m) AL_DEMOLISCI, AL_NUOVO e AL_ESTRAI, poi legge i tre DWG estratti con ezdxf (via
`ArchLine --export x.dwg x.dxf`, perche' ezdxf non legge DWG) e controlla il contenuto. Due giri, con gli stessi
controlli: l'asse del tramezzo dato esatto (4.05,0 4.05,3) e dato a occhio, oltre le facce (4.05,-0.1 4.05,3.1;
righe con il prefisso "asse oltre le facce,"): la zona deve fermarsi dove finiscono le facce.

Uso: bim_extract.py [PATH_ESEGUIBILE] [--keep DIR]
     (default: target\\release\\ArchLine.exe nella radice del repo)
Exit: 0 tutti i controlli OK, 1 almeno un controllo FALLITO, 2 errore del banco (eseguibile mancante,
nessuna cartella senza spazi, l'app non risponde), 77 ezdxf non installato.

Isolamento: l'app parte con APPDATA e LOCALAPPDATA puntati a una cartella temporanea, cosi' il vero
settings.json dell'utente non si legge e non si riscrive. Il disegno di prova e' sintetico (mai un disegno
dell'utente) e la sua copia su disco non deve cambiare nemmeno di un byte (SHA-256 prima e dopo).
"""
import argparse
import hashlib
import importlib.util
import json
import os
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
DEFAULT_EXE = REPO / "target" / "release" / "ArchLine.exe"

TOL = 1e-6  # metri: la tolleranza geometrica del modulo BIM
BASE = "casoA"  # nome del disegno di prova, quindi base dei nomi dei DWG estratti
LAYER_MURI = "MURI"
LAYER_DEMOLIZIONI = "AL-DEMOLIZIONI"
LAYER_NUOVE = "AL-NUOVE"
ACI_ROSSO, ACI_GIALLO = 1, 2

# Caso A, forma Continua, in metri: contorno esterno e due locali, quattro LINE ciascuno.
OUTER = [(-0.30, -0.30), (8.30, -0.30), (8.30, 3.30), (-0.30, 3.30)]
ROOM_A = [(0.0, 0.0), (4.00, 0.0), (4.00, 3.00), (0.0, 3.00)]
ROOM_B = [(4.10, 0.0), (8.00, 0.0), (8.00, 3.00), (4.10, 3.00)]

SUFFIX = {"fatto": "stato-di-fatto", "comparativa": "demolizioni-costruzioni", "progetto": "stato-di-progetto"}

# I due assi del tramezzo per AL_DEMOLISCI: esatto, e a occhio (sfora di 0,10 m nei muri d'ambito).
AXIS_EXACT = "4.05,0 4.05,3"
AXIS_PAST = "4.05,-0.1 4.05,3.1"
PAST_PREFIX = "asse oltre le facce, "

READY_TIMEOUT = 180  # s: avvio a freddo dell'app
REQUEST_TIMEOUT = 300  # s: una singola richiesta
EXPORT_TIMEOUT = 180  # s: una conversione


class BenchError(Exception):
    """Errore del banco (non un controllo fallito): exit 2."""


# ---------------------------------------------------------------- funzioni pure (provate in test_bim_extract)


def has_space(path):
    return " " in str(path)


def segments_equal(a, b, tol=TOL):
    """Due segmenti ((x1, y1), (x2, y2)) coincidono entro `tol` su ogni coordinata, in uno dei due versi."""

    def close(p, q):
        return abs(p[0] - q[0]) <= tol and abs(p[1] - q[1]) <= tol

    return (close(a[0], b[0]) and close(a[1], b[1])) or (close(a[0], b[1]) and close(a[1], b[0]))


def match_segments(expected, actual, tol=TOL):
    """Confronta due insiemi di segmenti uno a uno. Ritorna (mancanti, in_piu): i segmenti attesi senza
    corrispondente e quelli trovati senza corrispondente. Un segmento trovato serve a una sola attesa."""
    left = list(actual)
    missing = []
    for seg in expected:
        for k, cand in enumerate(left):
            if segments_equal(seg, cand, tol):
                del left[k]
                break
        else:
            missing.append(seg)
    return missing, left


def vertical_overlaps(segments, x, y0, y1, tol=TOL):
    """I segmenti verticali sulla retta `x` (entrambi gli estremi entro `tol`) la cui parte comune con
    l'intervallo aperto (y0, y1) e' piu' lunga di `tol`."""
    found = []
    for seg in segments:
        (ax, ay), (bx, by) = seg
        if abs(ax - x) > tol or abs(bx - x) > tol:
            continue
        low, high = min(ay, by), max(ay, by)
        if min(high, y1) - max(low, y0) > tol:
            found.append(seg)
    return found


def fmt_seg(seg):
    (ax, ay), (bx, by) = seg
    return f"({ax:.3f}; {ay:.3f})-({bx:.3f}; {by:.3f})"


def sha256_of(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def pick_work_base(candidates=None):
    """La prima cartella base il cui percorso non ha spazi (la riga di `run` si divide sugli spazi): la si
    crea se manca. Con `candidates` None: la cartella temporanea di sistema, poi, su Windows, la radice
    del disco di sistema (`C:\\ocs-bench`); su altri sistemi /tmp. Errore chiaro se nessuna va bene."""
    if candidates is None:
        candidates = [tempfile.gettempdir()]
        if os.name == "nt":
            candidates.append(os.path.join(os.environ.get("SystemDrive", "C:") + os.sep, "ocs-bench"))
        else:
            candidates.append("/tmp")
    tried = []
    for cand in candidates:
        path = Path(cand)
        if has_space(path):
            tried.append(f"{path} (ha spazi)")
            continue
        try:
            path.mkdir(parents=True, exist_ok=True)
        except OSError as ex:
            tried.append(f"{path} ({type(ex).__name__})")
            continue
        return path
    raise BenchError(
        "nessuna cartella di lavoro senza spazi nel percorso (il comando `run` divide la riga sugli spazi). "
        "Provate: " + "; ".join(tried) + ". Create una cartella senza spazi, per esempio C:\\ocs-bench."
    )


class Report:
    """Una riga per controllo: `OK` o `FALLITO` e una descrizione breve."""

    def __init__(self, out=None):
        self.out = out or sys.stdout
        self.total = 0
        self.failed = 0

    def check(self, ok, name, detail=""):
        self.total += 1
        if not ok:
            self.failed += 1
        text = f"{name}: {detail}" if detail else name
        print(f"{'OK      ' if ok else 'FALLITO '} {text}", file=self.out)
        return ok

    def summary(self):
        line = f"Riepilogo: {self.total} controlli, {self.total - self.failed} OK, {self.failed} FALLITI"
        print(line, file=self.out)
        return line


class Prefixed:
    """Lo stesso `Report`, con un prefisso davanti al nome di ogni controllo (per il secondo giro)."""

    def __init__(self, rep, prefix):
        self.rep = rep
        self.prefix = prefix

    def check(self, ok, name, detail=""):
        return self.rep.check(ok, self.prefix + name, detail)


# ---------------------------------------------------------------- disegno di prova


def build_case_a(path):
    """Il caso A, forma Continua, in metri, con $INSUNITS = 6 e i muri su MURI; DXF R2018."""
    import ezdxf

    doc = ezdxf.new("R2018")
    doc.header["$INSUNITS"] = 6
    doc.layers.add(LAYER_MURI)
    msp = doc.modelspace()
    for corners in (OUTER, ROOM_A, ROOM_B):
        for k, start in enumerate(corners):
            end = corners[(k + 1) % len(corners)]
            msp.add_line((start[0], start[1], 0.0), (end[0], end[1], 0.0), dxfattribs={"layer": LAYER_MURI})
    doc.saveas(path)
    return Path(path)


# ---------------------------------------------------------------- l'app

def isolated_env(config_dir):
    """Ambiente con le cartelle di configurazione dentro `config_dir`: il vero settings.json non si tocca."""
    roaming = Path(config_dir) / "Roaming"
    local = Path(config_dir) / "Local"
    roaming.mkdir(parents=True, exist_ok=True)
    local.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    env["APPDATA"] = str(roaming)
    env["LOCALAPPDATA"] = str(local)
    env["XDG_CONFIG_HOME"] = str(roaming)
    return env


def run_export(exe, src, dst, env, cwd):
    """`ArchLine --export src dst`. Ritorna (ok, motivo)."""
    try:
        r = subprocess.run([str(exe), "--export", str(src), str(dst)], capture_output=True, timeout=EXPORT_TIMEOUT,
                           env=env, cwd=str(cwd))
    except OSError as ex:
        raise BenchError(f"eseguibile non lanciabile: {exe} ({type(ex).__name__})") from ex
    except subprocess.TimeoutExpired:
        return False, f"timeout {EXPORT_TIMEOUT} s"
    if r.returncode != 0:
        tail = r.stderr.decode("utf-8", errors="replace").strip().splitlines()[-1:] or [""]
        return False, f"exit {r.returncode} {tail[0]}".strip()
    if not Path(dst).exists() or Path(dst).stat().st_size == 0:
        return False, "nessun file scritto"
    return True, ""


class Session:
    """Un server a righe JSON su stdin/stdout (`ArchLine --serve`): una riga per richiesta, una per risposta."""

    def __init__(self, cmd, env, cwd, stderr_path):
        self.stderr_path = Path(stderr_path)
        self._err = open(self.stderr_path, "wb")
        try:
            self.proc = subprocess.Popen([str(c) for c in cmd], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                         stderr=self._err, env=env, cwd=str(cwd))
        except OSError as ex:
            self._err.close()
            raise BenchError(f"eseguibile non lanciabile: {cmd[0]} ({type(ex).__name__})") from ex
        self._lines = queue.Queue()
        self._thread = threading.Thread(target=self._pump, daemon=True)
        self._thread.start()
        try:
            self.ready = self._wait_ready()
        except BaseException:
            self.kill()
            raise

    def _pump(self):
        for raw in self.proc.stdout:
            self._lines.put(raw)
        self._lines.put(None)

    def _next(self, timeout, what):
        try:
            raw = self._lines.get(timeout=timeout)
        except queue.Empty:
            raise BenchError(f"{what}: nessuna risposta in {timeout} s") from None
        if raw is None:
            raise BenchError(f"{what}: l'app e' uscita (exit {self.proc.poll()}). {self.stderr_tail()}")
        return raw.decode("utf-8", errors="replace").strip()

    def _wait_ready(self):
        for _ in range(50):  # righe che non sono JSON (rumore di avvio) si saltano
            line = self._next(READY_TIMEOUT, "avvio")
            try:
                msg = json.loads(line)
            except ValueError:
                continue
            if isinstance(msg, dict) and msg.get("ready"):
                return msg
        raise BenchError("avvio: la riga di pronto non e' arrivata")

    def request(self, obj):
        """Manda una richiesta e ritorna la risposta (dict)."""
        what = obj.get("op", "?") + (f" {obj['cmd']}" if "cmd" in obj else "")
        try:
            self.proc.stdin.write((json.dumps(obj) + "\n").encode("utf-8"))
            self.proc.stdin.flush()
        except OSError as ex:
            raise BenchError(f"{what}: l'app non legge piu' ({type(ex).__name__}). {self.stderr_tail()}") from ex
        for _ in range(50):
            line = self._next(REQUEST_TIMEOUT, what)
            try:
                msg = json.loads(line)
            except ValueError:
                continue
            if isinstance(msg, dict):
                return msg
        raise BenchError(f"{what}: nessuna risposta JSON")

    def stderr_tail(self, n=5):
        try:  # scrive il processo figlio sul suo handle: da qui basta rileggere il file
            lines = self.stderr_path.read_bytes().decode("utf-8", errors="replace").strip().splitlines()
        except OSError:
            return ""
        return ("stderr: " + " | ".join(lines[-n:])) if lines else ""

    def close(self):
        """Chiude stdin (la sessione finisce a fine file). Ritorna il codice di uscita (None se ucciso)."""
        try:
            self.proc.stdin.close()
        except OSError:
            pass
        try:
            code = self.proc.wait(timeout=60)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()
            code = None
        self._release()
        return code

    def kill(self):
        if self.proc.poll() is None:
            self.proc.kill()
            self.proc.wait()
        self._release()

    def _release(self):
        """Chiude i canali dopo la fine del processo (il lettore finisce da solo a fine file)."""
        self._thread.join(timeout=10)
        for stream in (self.proc.stdin, self.proc.stdout, self._err):
            try:
                stream.close()
            except OSError:
                pass


# ---------------------------------------------------------------- lettura con ezdxf


def lines_of(doc, layer=None):
    """I segmenti delle LINE dello spazio modello (sul layer dato, se indicato), in 2D."""
    out = []
    for e in doc.modelspace().query("LINE"):
        if layer is not None and e.dxf.layer != layer:
            continue
        out.append(((e.dxf.start.x, e.dxf.start.y), (e.dxf.end.x, e.dxf.end.y)))
    return out


def arch_xdata_appids(doc):
    """Gli APPID `ARCHLINE*` presenti nell'XDATA di una qualunque entita' del disegno (anche degli oggetti
    e delle voci di tabella), e quelli nella tabella APPID."""
    in_xdata = set()
    for e in doc.entitydb.values():
        data = getattr(getattr(e, "xdata", None), "data", None) or {}
        in_xdata.update(a for a in data if str(a).upper().startswith("ARCHLINE"))
    in_table = {str(a.dxf.name) for a in doc.appids if str(a.dxf.name).upper().startswith("ARCHLINE")}
    return sorted(in_xdata), sorted(in_table)


def read_dxf(path):
    import ezdxf

    try:
        return ezdxf.readfile(path), None
    except Exception as ex:  # DXFError, IOError, UnicodeDecodeError...
        return None, f"{type(ex).__name__}: {ex}"


# ---------------------------------------------------------------- il banco


def checks_session(rep, session, src, out_dir, axis=AXIS_EXACT):
    """Esegue i comandi via --serve (AL_DEMOLISCI sull'asse `axis`) e ne controlla le risposte."""
    rep.check(bool(session.ready.get("ok")), "sessione", f"app pronta (versione {session.ready.get('version', '?')})")
    resp = session.request({"op": "open", "path": str(src)})
    by_type = resp.get("by_type", {})
    rep.check(bool(resp.get("ok")) and by_type.get("Line") == 12 and resp.get("total") == 12,
              "open", f"disegno aperto, entita': {resp.get('total')} {by_type}" if resp.get("ok") else str(resp))
    for cmd in (f"AL_DEMOLISCI {axis} N", "AL_NUOVO 2,0.5 2,2.5 0.12", f"AL_ESTRAI {out_dir}"):
        resp = session.request({"op": "run", "cmd": cmd})
        ok = bool(resp.get("ok")) and resp.get("status") == "completed" and not resp.get("unconsumed")
        detail = (f"completato, entita' {resp.get('entities')} ({resp.get('added'):+d})" if resp.get("ok")
                  and isinstance(resp.get("added"), int) else str(resp))
        if resp.get("ok") and resp.get("unconsumed"):
            detail += f"; token non consumati: {resp.get('unconsumed')}"
        rep.check(ok, f"run {cmd.split()[0]}", detail)


def checks_files(rep, exe, env, work, out_dir, src_doc):
    """Riconverte i tre DWG in DXF e controlla ognuno con ezdxf."""
    docs = {}
    for key, suffix in SUFFIX.items():
        dwg = Path(out_dir) / f"{BASE}_{suffix}.dwg"
        if not dwg.is_file():
            rep.check(False, f"{key}: file", f"{dwg.name} non scritto")
            continue
        dxf = Path(work) / f"{BASE}_{suffix}.dxf"
        ok, why = run_export(exe, dwg, dxf, env, work)
        if not rep.check(ok, f"{key}: ricarica", f"{dwg.name} -> DXF" if ok else f"--export fallito ({why})"):
            continue
        doc, err = read_dxf(dxf)
        if not rep.check(doc is not None, f"{key}: ezdxf", "letto" if doc is not None else err):
            continue
        docs[key] = doc

    src_lines = lines_of(src_doc, LAYER_MURI)
    if "fatto" in docs:
        check_fatto(rep, docs["fatto"], src_lines)
    if "comparativa" in docs:
        check_comparativa(rep, docs["comparativa"], src_lines)
    if "progetto" in docs:
        check_progetto(rep, docs["progetto"])
    for key, doc in docs.items():
        xdata, table = arch_xdata_appids(doc)
        rep.check(not xdata and not table, f"{key}: niente ARCHLINE",
                  "nessun XDATA ne' APPID ARCHLINE*" if not xdata and not table
                  else f"XDATA {xdata}, tabella APPID {table}")
        extra = [layout.name for layout in doc.layouts if layout.name != "Model" and len(layout) > 0]
        rep.check(not extra, f"{key}: solo spazio modello",
                  "nessuna entita' nei layout" if not extra else f"entita' nei layout {extra}")


def check_fatto(rep, doc, src_lines):
    found = lines_of(doc, LAYER_MURI)
    missing, extra = match_segments(src_lines, found)
    detail = f"{len(found)} linee {LAYER_MURI}, estremi come nel DXF di partenza"
    if missing or extra:
        detail = (f"{len(found)} linee {LAYER_MURI} contro {len(src_lines)}; mancano "
                  f"{[fmt_seg(s) for s in missing]}; in piu' {[fmt_seg(s) for s in extra]}")
    rep.check(not missing and not extra, "fatto: muri", detail)
    zones = [e for e in doc.modelspace().query("HATCH")]
    rep.check(not zones, "fatto: senza zone", "nessuna campitura" if not zones else f"{len(zones)} campiture")


def check_comparativa(rep, doc, src_lines):
    hatches = list(doc.modelspace().query("HATCH"))
    by_layer = {h.dxf.layer: h for h in hatches}
    ok = (len(hatches) == 2 and set(by_layer) == {LAYER_DEMOLIZIONI, LAYER_NUOVE}
          and by_layer[LAYER_DEMOLIZIONI].dxf.color == ACI_GIALLO and by_layer[LAYER_NUOVE].dxf.color == ACI_ROSSO)
    desc = ", ".join(f"{h.dxf.layer} colore {h.dxf.color}" for h in hatches)
    rep.check(ok, "comparativa: campiture", f"2 HATCH ({desc})" if ok else f"{len(hatches)} HATCH ({desc})")
    solid = all(h.dxf.solid_fill == 1 for h in hatches) and bool(hatches)
    rep.check(solid, "comparativa: riempimento solido",
              "solid_fill = 1 su tutte" if solid else f"solid_fill {[h.dxf.solid_fill for h in hatches]}")
    found = lines_of(doc, LAYER_MURI)
    missing, extra = match_segments(src_lines, found)
    rep.check(not missing and not extra, "comparativa: rilievo",
              f"{len(found)} linee {LAYER_MURI} come nel DXF di partenza" if not missing and not extra
              else f"mancano {[fmt_seg(s) for s in missing]}; in piu' {[fmt_seg(s) for s in extra]}")


def check_progetto(rep, doc):
    segs = lines_of(doc)
    for y, label in ((0.0, "(4,00; 0)-(4,10; 0)"), (3.0, "(4,00; 3,00)-(4,10; 3,00)")):
        want = ((4.00, y), (4.10, y))
        hit = [s for s in segs if segments_equal(s, want)]
        rep.check(bool(hit), f"progetto: chiusura {label}", "LINE trovata" if hit else "nessuna LINE")
    for x in (4.00, 4.10):
        left = vertical_overlaps(segs, x, 0.0, 3.0)
        rep.check(not left, f"progetto: facce del tramezzo su x = {x:.2f}",
                  "nessuna LINE lungo la faccia demolita" if not left else f"restano {[fmt_seg(s) for s in left]}")
    for x in (1.94, 2.06):
        want = ((x, 0.5), (x, 2.5))
        hit = [e for e in doc.modelspace().query("LINE")
               if segments_equal(((e.dxf.start.x, e.dxf.start.y), (e.dxf.end.x, e.dxf.end.y)), want)]
        rep.check(bool(hit), f"progetto: faccia del muro nuovo su x = {x:.2f}",
                  f"LINE (x; 0,50)-(x; 2,50) sul layer {hit[0].dxf.layer}" if hit else "nessuna LINE")
        if hit:
            rep.check(hit[0].dxf.layer == LAYER_MURI, f"progetto: layer della faccia x = {x:.2f}",
                      f"layer {hit[0].dxf.layer}" + ("" if hit[0].dxf.layer == LAYER_MURI else f", atteso {LAYER_MURI}"))


def run_case(rep, exe, env, work, src, src_doc, before, axis):
    """Un giro: sessione --serve con AL_DEMOLISCI sull'asse `axis`, estratti in `work/out`, DXF riconvertiti in
    `work`, controlli su `rep`."""
    work = Path(work)
    out_dir = work / "out"
    out_dir.mkdir(parents=True, exist_ok=True)
    session = Session([exe, "--serve"], env, work, work / "serve.stderr.txt")
    try:
        checks_session(rep, session, src, out_dir, axis)
    except BaseException:
        session.kill()
        raise
    code = session.close()
    rep.check(code == 0, "sessione: chiusura", "exit 0" if code == 0 else f"exit {code}. {session.stderr_tail()}")

    after = sha256_of(src)
    rep.check(after == before, "sorgente intatta",
              "DXF di partenza identico byte per byte (SHA-256)" if after == before
              else f"il DXF di partenza e' cambiato (SHA-256 {before[:12]} -> {after[:12]})")
    checks_files(rep, exe, env, work, out_dir, src_doc)


def run_bim_extract(exe, work, rep):
    """Tutto il banco in `work` (percorso senza spazi): il giro con l'asse esatto in `work`, quello con l'asse
    oltre le facce in `work/asse-oltre` (stessi controlli, nomi col prefisso). Ritorna l'oggetto Report."""
    work = Path(work)
    env = isolated_env(work / "config")
    src = build_case_a(work / f"{BASE}.dxf")
    src_doc, err = read_dxf(src)
    if src_doc is None:
        raise BenchError(f"la sorgente non e' leggibile da ezdxf ({err}): difetto del banco, non dell'app")
    before = sha256_of(src)
    run_case(rep, exe, env, work, src, src_doc, before, AXIS_EXACT)
    run_case(Prefixed(rep, PAST_PREFIX), exe, env, work / "asse-oltre", src, src_doc, before, AXIS_PAST)
    return rep


def main(argv=None):
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")  # la console Windows e' cp1252
    ap = argparse.ArgumentParser(description="Banco di conformita: estrazione BIM controllata con ezdxf")
    ap.add_argument("exe", nargs="?", default=str(DEFAULT_EXE), help=r"eseguibile ArchLine (default target\release\ArchLine.exe)")
    ap.add_argument("--keep", metavar="DIR", help="cartella di lavoro da tenere (senza spazi); altrimenti temporanea")
    args = ap.parse_args(argv)

    if importlib.util.find_spec("ezdxf") is None:
        print("ezdxf non installato (pip install ezdxf): banco saltato")
        return 77
    work = None
    rep = Report()
    try:
        exe = Path(args.exe)
        if not exe.is_file():
            raise BenchError(f"eseguibile non trovato: {args.exe}")
        exe = exe.resolve()
        if args.keep:
            keep_dir = Path(args.keep).resolve()
            if has_space(keep_dir):
                raise BenchError(f"--keep: il percorso ha spazi ({keep_dir})")
            if keep_dir.exists() and (not keep_dir.is_dir() or any(keep_dir.iterdir())):
                # AL_ESTRAI non sovrascrive: una cartella con i file di un giro precedente farebbe fallire il banco
                raise BenchError(f"--keep: la cartella deve essere nuova o vuota ({keep_dir})")
            keep_dir.mkdir(parents=True, exist_ok=True)
            work = keep_dir
        else:
            work = Path(tempfile.mkdtemp(prefix="archline_bim_", dir=pick_work_base()))
        print(f"Eseguibile: {exe}")
        print(f"Cartella di lavoro: {work} (APPDATA isolato in {work / 'config'})")
        run_bim_extract(exe, work, rep)
    except BenchError as ex:
        print(f"errore del banco: {ex}", file=sys.stderr)
        if work is not None:
            print(f"cartella di lavoro tenuta per la diagnosi: {work}", file=sys.stderr)
        return 2
    rep.summary()
    keep = args.keep or rep.failed
    if work is not None and not keep:
        shutil.rmtree(work, ignore_errors=True)
    elif work is not None and rep.failed:
        print(f"cartella di lavoro tenuta per la diagnosi: {work}")
    return 1 if rep.failed else 0


if __name__ == "__main__":
    sys.exit(main())
