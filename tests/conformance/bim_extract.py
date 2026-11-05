#!/usr/bin/env python3
"""Banco di conformita, estrazione BIM: lancia `ArchLine --serve`, esegue sul caso A (due locali affiancati,
tramezzo da 0,10 m) AL_DEMOLISCI, AL_NUOVO e AL_ESTRAI, poi legge i tre DWG estratti con ezdxf (via
`ArchLine --export x.dwg x.dxf`, perche' ezdxf non legge DWG) e controlla il contenuto. Due giri, con gli stessi
controlli: l'asse del tramezzo dato esatto (4.05,0 4.05,3) e dato a occhio, oltre le facce (4.05,-0.1 4.05,3.1;
righe con il prefisso "asse oltre le facce,"): la zona deve fermarsi dove finiscono le facce.

Abachi e quantita' (BIM parte 2), in ogni giro dopo AL_ESTRAI: `AL_ALTEZZA L 2.70`, `AL_ABACO_CSV abaco.csv`,
`AL_ABACO` e il salvataggio del disegno di lavoro in un DXF a parte (`abaco-lavoro.dxf`, mai sul disegno di
partenza). Il CSV si legge come lo legge Excel italiano (UTF-8 con BOM, `;`, CRLF): intestazione, righe D1
(demolizione, 0,8100 m3 = 3,00 x 0,10 x 2,70) e N1 (nuova muratura, 5,4000 m2 = 2,00 x 2,70, larghezza vuota),
`quantita` = parti x lunghezza x larghezza x altezza dei fattori scritti (entro 5e-5), altezza mai vuota. Le tabelle
(ACAD_TABLE) ezdxf le vede ma non ne legge le celle (n_rows = 0): si controlla il loro blocco di visualizzazione `*T`
(codice e quantita' a due decimali di ogni riga del CSV). E' un limite dichiarato (riga NOTA), non un controllo
fallito; se le due tabelle ci sono ma il blocco e' senza testi, la riga NOTA dice "TABLE non verificata (ezdxf):
solo CSV". Nessuna ACAD_TABLE (o diverse da due) e' invece FALLITO.

L'abaco nell'estrazione (parte 2, spec 5.4), in ogni giro dopo AL_ABACO: `AL_ALTEZZA L 3.00` senza rilanciare
AL_ABACO, poi `AL_ESTRAI` in `out-abaco` (righe con il prefisso "abaco estratto, "). Nel DWG di demolizioni e
costruzioni le tabelle sono LINE e MTEXT sul layer AL-ABACO con i numeri ricalcolati (D1 0,90 m3 = 3,00 x 0,10 x
3,00; N1 6,00 m2 = 2,00 x 3,00), nessuno dei numeri della tabella del disegno di lavoro (0,81 e 5,40, con 2,70), e
la griglia parte dal punto d'inserimento di ogni ACAD_TABLE del disegno di lavoro (letto da `abaco-lavoro.dxf`);
nei tre DWG nessuna ACAD_TABLE, nel fatto e nel progetto niente su AL-ABACO.

La tabella riscritta (revisione finale), in ogni giro dopo la seconda estrazione: `AL_ABACO` di nuovo e il salvataggio
in `abaco-rifatto.dxf` (righe con il prefisso "abaco riscritto"): ancora due ACAD_TABLE, e nel loro blocco `*T` (cio'
che disegnano ArchLine, AutoCAD ed ezdxf) i numeri a 3,00 (D1 0,90, N1 6,00), nessuno di quelli a 2,70 (0,81 e 5,40).

Porte (BIM parte 3), terzo giro (righe con il prefisso "porta, "): il caso A con un vano porta di 0,90 m nel
tramezzo (y da 1,00 a 1,90, nessuna linea di soglia), `AL_PORTA 4.05,1 4.05,1.9 E 2.1`, i locali A e B, l'altezza
del livello, `AL_ESTRAI` e `AL_ABACO_CSV`. Nei tre DWG: una polilinea 0,90 x 0,10 su AL-APERTURE, nessun XDATA ne'
APPID ARCHLINE; nel fatto e nel progetto i locali A 12,00 e B 11,70 m2 (la porta chiude il vano da sola) e nessun
locale in piu'. Nel CSV la porta (P1, 1,8900 m2), le pareti lorde (LF1-PA, 37,8000) e le detrazioni a detrarre
(LF1-PA-P1 e LF2-PA-P1, 1,8900), con `quantita` = prodotto dei fattori.

Sezioni e prospetti (BIM parte 4), quarto giro (righe con il prefisso "sezione, "): il caso A con la porta, l'altezza
del livello, i locali A e B, `AL_TAGLIO -1,1.5 9,1.5 4,3 A-A`, `AL_SEZIONE` (due volte: i disegni si sostituiscono, non
si duplicano), `AL_PROSPETTO S` e `AL_ESTRAI`. Nei tre DWG: due titoli (sezione A-A e prospetto Sud) con la fase
dell'elaborato, il poche' del muro d'ambito 0,30 x 2,70 su AL-SEZIONI, campiture solide, il taglio su AL-TAGLI, nessun
XDATA ne' APPID ARCHLINE.

Uso: bim_extract.py [PATH_ESEGUIBILE] [--keep DIR]
     (default: target\\release\\ArchLine.exe nella radice del repo)
Exit: 0 tutti i controlli OK, 1 almeno un controllo FALLITO, 2 errore del banco (eseguibile mancante,
nessuna cartella senza spazi, l'app non risponde), 77 ezdxf non installato.

Isolamento: l'app parte con APPDATA e LOCALAPPDATA puntati a una cartella temporanea, cosi' il vero
settings.json dell'utente non si legge e non si riscrive. Il disegno di prova e' sintetico (mai un disegno
dell'utente) e la sua copia su disco non deve cambiare nemmeno di un byte (SHA-256 prima e dopo).
"""
import argparse
import csv
import hashlib
import importlib.util
import io
import json
import os
import queue
import re
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

# Abaco (parte 2): altezza predefinita del livello, intestazione e tolleranza del CSV di `AL_ABACO_CSV`.
HEIGHT_CMD = "AL_ALTEZZA L 2.70"
CSV_HEADER = ["tipo", "codice", "descrizione", "parti", "lunghezza", "larghezza", "altezza", "detrai", "quantita",
              "um", "stato_altezza", "fonte", "note"]
CSV_QUANTITY_TOL = 5e-5 + 1e-9  # `quantita` e il prodotto dei fattori scritti: sempre entro 5e-5 (to_csv)
TABLE_NOT_VERIFIED = "TABLE non verificata (ezdxf): solo CSV"

# L'abaco nell'estrazione: l'altezza cambiata dopo AL_ABACO, la cartella del secondo AL_ESTRAI, il layer, i numeri
# attesi nell'estratto (altezza 3,00) e quelli della tabella del disegno di lavoro (altezza 2,70) che non ci devono essere.
HEIGHT_AFTER_CMD = "AL_ALTEZZA L 3.00"
ABACO_OUT = "out-abaco"
ABACO_PREFIX = "abaco estratto, "
LAYER_ABACO = "AL-ABACO"
ABACO_FRESH = {"D1": "0,90", "N1": "6,00"}
ABACO_STALE = ("0,81", "5,40")

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


def touches(point, segment, tol=TOL):
    """Il punto (x, y) e' un estremo del segmento ((x1, y1), (x2, y2)), entro `tol` su ogni coordinata."""
    return any(abs(point[0] - end[0]) <= tol and abs(point[1] - end[1]) <= tol for end in segment)


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


def parse_it_number(text):
    """Un numero del CSV dell'abaco: vuoto = fattore che non si applica (None); altrimenti quattro decimali con la
    virgola (`0,8100`), eventualmente negativo. Altro (punto decimale, migliaia, decimali diversi) e' ValueError."""
    if text == "":
        return None
    if not re.fullmatch(r"-?\d+,\d{4}", text):
        raise ValueError(f"numero non nella forma N,NNNN: {text!r}")
    return float(text.replace(",", "."))


def read_abaco_csv(path):
    """Legge il CSV di `AL_ABACO_CSV` come lo legge Excel italiano: UTF-8 con BOM, `;`, righe CRLF. Ritorna
    (bom, crlf, intestazione, righe): `righe` = lista di dict per colonna. Una riga con un numero di campi diverso
    dall'intestazione e' ValueError."""
    raw = Path(path).read_bytes()
    bom = raw.startswith(b"\xef\xbb\xbf")
    text = raw.decode("utf-8-sig")
    crlf = "\r\n" in text and "\n" not in text.replace("\r\n", "")
    parsed = list(csv.reader(io.StringIO(text, newline=""), delimiter=";"))
    if not parsed:
        raise ValueError("file vuoto")
    header, rows = parsed[0], []
    for n, fields in enumerate(parsed[1:], start=2):
        if len(fields) != len(header):
            raise ValueError(f"riga {n}: {len(fields)} campi contro {len(header)} dell'intestazione")
        rows.append(dict(zip(header, fields)))
    return bom, crlf, header, rows


def csv_product(row):
    """`parti x lunghezza x larghezza x altezza` dei fattori scritti, i vuoti saltati (come la `Misura` di ArchCme)."""
    product = 1.0
    for column in ("parti", "lunghezza", "larghezza", "altezza"):
        value = parse_it_number(row[column])
        if value is not None:
            product *= value
    return product


def csv_problems(rows):
    """Cio' che non torna nelle righe del CSV: `quantita` diversa dal prodotto dei fattori scritti oltre
    CSV_QUANTITY_TOL (la regola di `to_csv`: sempre entro 5e-5), numeri non nella forma N,NNNN, altezza vuota (un
    locale no: e' completo con la sola superficie e l'altezza `n.d.` sta in `stato_altezza`). Elenco vuoto = tutto
    torna."""
    problems = []
    for n, row in enumerate(rows, start=1):
        who = f"riga {n} ({row.get('tipo', '?')} {row.get('codice', '?')})"
        try:
            quantity = parse_it_number(row["quantita"])
            product = csv_product(row)
        except (KeyError, ValueError) as ex:
            problems.append(f"{who}: {ex}")
            continue
        if quantity is None or abs(quantity - product) > CSV_QUANTITY_TOL:
            problems.append(f"{who}: quantita {row['quantita']} contro il prodotto dei fattori {product:.6f}")
        if not row["tipo"].startswith("locale") and row["altezza"] == "":
            problems.append(f"{who}: altezza vuota")
    return problems


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


def checks_session(rep, session, src, out_dir, axis=AXIS_EXACT, csv_path=None, table_dxf=None, abaco_dir=None,
                   rewritten_dxf=None):
    """Esegue i comandi via --serve (AL_DEMOLISCI sull'asse `axis`) e ne controlla le risposte; con `csv_path`
    poi `AL_ALTEZZA L 2.70` e `AL_ABACO_CSV` (e, con `table_dxf`, `AL_ABACO` e il salvataggio in DXF; con
    `abaco_dir` anche, dopo, `AL_ALTEZZA L 3.00` e `AL_ESTRAI` in `abaco_dir`; con `rewritten_dxf`, per ultimi,
    `AL_ABACO` di nuovo e il salvataggio in quel DXF)."""
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
    if csv_path is not None:
        abaco_commands(rep, session, csv_path, table_dxf, abaco_dir, rewritten_dxf)


def run_ok(rep, session, cmd):
    """Un comando via `run`: completato, senza errori ne' token non consumati. Ritorna la risposta."""
    resp = session.request({"op": "run", "cmd": cmd})
    ok = bool(resp.get("ok")) and resp.get("status") == "completed" and not resp.get("unconsumed")
    detail = "completato" if ok else str(resp)
    rep.check(ok, f"run {cmd.split()[0]}", detail)
    return resp


def abaco_commands(rep, session, csv_path, table_dxf, abaco_dir=None, rewritten_dxf=None):
    """Parte 2 (abachi e quantita'): altezza del livello, CSV dell'abaco, tabelle; con `abaco_dir`, l'altezza
    cambiata senza rilanciare AL_ABACO e una seconda estrazione; con `rewritten_dxf` anche, dopo, AL_ABACO di nuovo e
    il salvataggio in quel DXF. I file si leggono dopo."""
    run_ok(rep, session, HEIGHT_CMD)
    run_ok(rep, session, f"AL_ABACO_CSV {csv_path}")
    if table_dxf is not None:
        run_ok(rep, session, "AL_ABACO")  # senza punto, da script: le tabelle a destra dell'estensione
        # Salvataggio in DXF in un altro file (mai sul disegno di partenza): ezdxf legge il DXF, non il DWG.
        resp = session.request({"op": "save", "path": str(table_dxf)})
        rep.check(bool(resp.get("ok")) and resp.get("target_format") == "dxf", "abaco: salvataggio DXF",
                  f"{Path(table_dxf).name} (DXF)" if resp.get("ok") else str(resp))
        if abaco_dir is not None:
            # Le tabelle del disegno restano quelle a 2,70: l'estratto deve ricalcolarle a 3,00.
            run_ok(rep, session, HEIGHT_AFTER_CMD)
            run_ok(rep, session, f"AL_ESTRAI {abaco_dir}")
            if rewritten_dxf is not None:
                # Poi AL_ABACO di nuovo: le tabelle si riscrivono dove sono, e il loro blocco *T con loro.
                run_ok(rep, session, "AL_ABACO")
                resp = session.request({"op": "save", "path": str(rewritten_dxf)})
                rep.check(bool(resp.get("ok")) and resp.get("target_format") == "dxf", "abaco riscritto: salvataggio DXF",
                          f"{Path(rewritten_dxf).name} (DXF)" if resp.get("ok") else str(resp))


def read_extracts(rep, exe, env, work, out_dir, dxf_dir, base=BASE):
    """Riconverte i tre DWG `<base>_<elaborato>.dwg` di `out_dir` in DXF (in `dxf_dir`) e li legge con ezdxf:
    {chiave: documento}, con un controllo FALLITO per ogni file che manca o non si legge."""
    docs = {}
    for key, suffix in SUFFIX.items():
        dwg = Path(out_dir) / f"{base}_{suffix}.dwg"
        if not dwg.is_file():
            rep.check(False, f"{key}: file", f"{dwg.name} non scritto")
            continue
        dxf = Path(dxf_dir) / f"{base}_{suffix}.dxf"
        ok, why = run_export(exe, dwg, dxf, env, work)
        if not rep.check(ok, f"{key}: ricarica", f"{dwg.name} -> DXF" if ok else f"--export fallito ({why})"):
            continue
        doc, err = read_dxf(dxf)
        if not rep.check(doc is not None, f"{key}: ezdxf", "letto" if doc is not None else err):
            continue
        docs[key] = doc
    return docs


def checks_files(rep, exe, env, work, out_dir, src_doc):
    """Riconverte i tre DWG in DXF e controlla ognuno con ezdxf."""
    docs = read_extracts(rep, exe, env, work, out_dir, work)

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


def table_texts(doc):
    """Per ogni ACAD_TABLE dello spazio modello, i testi (MTEXT) del suo blocco di visualizzazione `*T…`: ezdxf non
    legge le celle della TABLE (`n_rows` = 0, i dati AcDbTable non si interpretano), ma legge il blocco che
    AutoCAD disegna. Ritorna la lista dei testi per tabella (vuota se il blocco manca o non ha testi)."""
    out = []
    for table in doc.modelspace().query("ACAD_TABLE"):
        name = getattr(table.dxf, "geometry", None)
        try:
            block = doc.blocks.get(name)
        except Exception:  # blocco mancante: nessun testo
            block = None
        out.append([e.plain_text() for e in block if e.dxftype() == "MTEXT"] if block is not None else [])
    return out


def two_decimals(text):
    """`0,8100` -> `0,81`: come una cella dell'abaco scrive la stessa quantita' (due decimali, virgola)."""
    return f"{parse_it_number(text):.2f}".replace(".", ",")


def check_abaco_csv(rep, csv_path):
    """Il CSV dell'abaco del caso (D1 demolizione ad asse esatto, N1 muro nuovo, altezza 2,70 m) e le sue regole.
    Ritorna le righe lette (o None se il file non c'e' o non si legge), per il controllo delle tabelle."""
    if not Path(csv_path).is_file():
        rep.check(False, "abaco CSV: file", f"{Path(csv_path).name} non scritto")
        return None
    try:
        bom, crlf, header, rows = read_abaco_csv(csv_path)
    except (OSError, ValueError, UnicodeDecodeError) as ex:
        rep.check(False, "abaco CSV: lettura", f"{type(ex).__name__}: {ex}")
        return None
    rep.check(bom and crlf, "abaco CSV: BOM UTF-8 e CRLF", "BOM e CRLF" if bom and crlf else f"BOM {bom}, CRLF {crlf}")
    rep.check(header == CSV_HEADER, "abaco CSV: intestazione",
              ";".join(header) if header == CSV_HEADER else f"{header} invece di {CSV_HEADER}")
    codes = [r["codice"] for r in rows]
    rep.check(codes == ["D1", "N1"], "abaco CSV: righe", f"righe {codes}" if codes != ["D1", "N1"] else "D1 e N1")
    by_code = {r["codice"]: r for r in rows}
    dem, new = by_code.get("D1"), by_code.get("N1")
    # 3,00 x 0,10 x 2,70 = 0,81 m3 (tramezzo del caso A, altezza del livello); 2,00 x 2,70 = 5,40 m2 (muro nuovo)
    if dem is not None:
        got = (dem["tipo"], dem["quantita"], dem["um"])
        rep.check(got == ("demolizione", "0,8100", "m³"), "abaco CSV: demolizione D1",
                  "demolizione, 0,8100 m³" if got == ("demolizione", "0,8100", "m³")
                  else f"{got} invece di ('demolizione', '0,8100', 'm³')")
    if new is not None:
        got = (new["tipo"], new["quantita"], new["um"], new["larghezza"])
        want = ("nuova muratura", "5,4000", "m²", "")
        rep.check(got == want, "abaco CSV: nuova muratura N1",
                  "nuova muratura, 5,4000 m², larghezza vuota" if got == want else f"{got} invece di {want}")
    problems = csv_problems(rows)
    rep.check(not problems, "abaco CSV: quantita = parti x lunghezza x larghezza x altezza, altezza sempre scritta",
              f"{len(rows)} righe" if not problems else "; ".join(problems))
    return rows


def check_abaco_tables(rep, table_dxf, rows):
    """Le tabelle `AL_ABACO` riconvertite in DXF: ezdxf vede le due ACAD_TABLE e, nel loro blocco di
    visualizzazione, il codice e la quantita' di ogni riga del CSV (a due decimali). Le celle AcDbTable vere e proprie
    ezdxf non le interpreta: e' un limite dichiarato (riga NOTA), non un controllo fallito. Una tabella che manca
    (zero ACAD_TABLE, o diverse da due) invece e' FALLITO: l'app non ha scritto quello che doveva."""
    if not Path(table_dxf).is_file():
        rep.check(False, "abaco TABLE: file", f"{Path(table_dxf).name} non scritto")
        return
    doc, err = read_dxf(table_dxf)
    if not rep.check(doc is not None, "abaco TABLE: ezdxf", "letto" if doc is not None else err):
        return
    tables = table_texts(doc)
    # Nessuna tabella (o diversa da due) e' una regressione dell'app: AL_ABACO e' "completato" ma non ha scritto
    # cio' che doveva. Il limite dichiarato di ezdxf vale solo se le tabelle ci sono ma il loro blocco e' senza testi.
    rep.check(len(tables) == 2, "abaco TABLE: due ACAD_TABLE",
              "2 ACAD_TABLE nello spazio modello" if len(tables) == 2 else f"{len(tables)} ACAD_TABLE invece di 2")
    if not tables:
        return
    if not any(tables):
        print(f"NOTA     {TABLE_NOT_VERIFIED}")
        return
    rep.check(all(tables), "abaco TABLE: blocchi con testi",
              "ogni tabella ha il blocco di visualizzazione con testi" if all(tables)
              else f"testi per tabella {[len(t) for t in tables]}")
    for code in ("D1", "N1"):
        row = next((r for r in rows or [] if r["codice"] == code), None)
        if row is None:
            continue
        want = two_decimals(row["quantita"])
        hit = [t for t in tables if code in t and want in t]
        rep.check(bool(hit), f"abaco TABLE: riga {code}",
                  f"codice {code} e quantita' {want} nella stessa tabella" if hit
                  else f"codice {code} e quantita' {want} non trovati nello stesso blocco")
    print("NOTA     le celle AcDbTable non si leggono con ezdxf (n_rows = 0): controllato il blocco di "
          "visualizzazione *T, non la struttura a celle (da vedere in AutoCAD)")


def check_abaco_rewritten(rep, path):
    """AL_ABACO rilanciato dopo `AL_ALTEZZA L 3.00` (Critical della revisione finale): le due ACAD_TABLE si riscrivono
    dove sono e il loro blocco di visualizzazione `*T`, cio' che disegnano ArchLine, AutoCAD ed ezdxf, ha i numeri nuovi
    (ABACO_FRESH: D1 0,90 m3 = 3,00 x 0,10 x 3,00; N1 6,00 m2 = 2,00 x 3,00) e nessuno di quelli a 2,70 (ABACO_STALE).
    Qui un blocco senza testi e' FALLITO: dopo il primo AL_ABACO i testi c'erano, e sono proprio cio' che si controlla."""
    if not Path(path).is_file():
        rep.check(False, "abaco riscritto: file", f"{Path(path).name} non scritto")
        return
    doc, err = read_dxf(path)
    if not rep.check(doc is not None, "abaco riscritto: ezdxf", "letto" if doc is not None else err):
        return
    tables = table_texts(doc)
    rep.check(len(tables) == 2, "abaco riscritto: due ACAD_TABLE",
              "2 ACAD_TABLE, nessuna copia" if len(tables) == 2 else f"{len(tables)} ACAD_TABLE invece di 2")
    rep.check(bool(tables) and all(tables), "abaco riscritto: blocchi con testi",
              "ogni tabella ha il blocco *T con testi" if tables and all(tables)
              else f"testi per tabella {[len(t) for t in tables]}")
    for code, value in ABACO_FRESH.items():
        hit = [t for t in tables if code in t and value in t]
        rep.check(bool(hit), f"abaco riscritto: riga {code} nel blocco *T",
                  f"codice {code} e {value} nello stesso blocco" if hit
                  else f"codice {code} e {value} non trovati nello stesso blocco")
    stale = sorted({v for t in tables for v in ABACO_STALE if v in t})
    rep.check(not stale, "abaco riscritto: niente numeri vecchi nel blocco *T",
              "nessuno di " + ", ".join(ABACO_STALE) if not stale else f"trovati {stale}")


def abaco_texts(doc):
    """I testi (MTEXT) dello spazio modello sul layer AL-ABACO: le tabelle d'abaco di un estratto, che sono linee
    e testi ordinari."""
    return [e.plain_text() for e in doc.modelspace().query("MTEXT") if e.dxf.layer == LAYER_ABACO]


def table_corners(doc):
    """Il punto d'inserimento (x, y) di ogni ACAD_TABLE dello spazio modello: l'angolo in alto a sinistra."""
    return [(t.dxf.insert.x, t.dxf.insert.y) for t in doc.modelspace().query("ACAD_TABLE")]


def check_abaco_extract(rep, docs, corners):
    """L'abaco nell'estrazione fatta dopo AL_ABACO e `AL_ALTEZZA L 3.00` (spec parte 2, 5.4): nei tre DWG nessuna
    ACAD_TABLE e niente ARCHLINE; nel fatto e nel progetto niente su AL-ABACO; nella comparativa LINE e MTEXT su
    AL-ABACO con i numeri ricalcolati (ABACO_FRESH), nessuno di quelli della tabella del disegno di lavoro
    (ABACO_STALE), e una LINE che parte da ogni angolo `corners` (le ACAD_TABLE del disegno di lavoro)."""
    for key, doc in docs.items():
        tables = len(doc.modelspace().query("ACAD_TABLE"))
        rep.check(tables == 0, f"{key}: nessuna ACAD_TABLE",
                  "nessuna" if tables == 0 else f"{tables} ACAD_TABLE")
        xdata, table = arch_xdata_appids(doc)
        rep.check(not xdata and not table, f"{key}: niente ARCHLINE",
                  "nessun XDATA ne' APPID ARCHLINE*" if not xdata and not table
                  else f"XDATA {xdata}, tabella APPID {table}")
    for key in ("fatto", "progetto"):
        if key in docs:
            on = [e.dxftype() for e in docs[key].modelspace() if e.dxf.layer == LAYER_ABACO]
            rep.check(not on, f"{key}: niente abaco", f"niente su {LAYER_ABACO}" if not on else f"{on}")
    doc = docs.get("comparativa")
    if doc is None:
        return
    texts = abaco_texts(doc)
    lines = lines_of(doc, LAYER_ABACO)
    rep.check(bool(texts) and bool(lines), "comparativa: abaco in linee e testi",
              f"{len(lines)} LINE e {len(texts)} MTEXT su {LAYER_ABACO}")
    for code, value in ABACO_FRESH.items():
        hit = code in texts and value in texts
        rep.check(hit, f"comparativa: riga {code} ricalcolata",
                  f"codice {code} e {value}" if hit else f"codice {code} o {value} mancanti")
    stale = [v for v in ABACO_STALE if v in texts]
    rep.check(not stale, "comparativa: niente numeri della tabella del disegno di lavoro",
              "nessuno di " + ", ".join(ABACO_STALE) if not stale else f"trovati {stale}")
    missing = [c for c in corners if not any(touches(c, seg) for seg in lines)]
    rep.check(bool(corners) and not missing, "comparativa: tabelle nel punto del disegno di lavoro",
              f"{len(corners)} tabelle, la griglia parte dal loro punto d'inserimento" if corners and not missing
              else f"angoli senza griglia {missing} (tabelle del disegno di lavoro: {len(corners)})")


def run_case(rep, exe, env, work, src, src_doc, before, axis):
    """Un giro: sessione --serve con AL_DEMOLISCI sull'asse `axis`, estratti in `work/out`, DXF riconvertiti in
    `work`, controlli su `rep`; poi l'estrazione con l'abaco, in `work/out-abaco` (DXF accanto ai DWG)."""
    work = Path(work)
    out_dir = work / "out"
    out_dir.mkdir(parents=True, exist_ok=True)
    csv_path = work / "abaco.csv"
    table_dxf = work / "abaco-lavoro.dxf"
    rewritten_dxf = work / "abaco-rifatto.dxf"
    abaco_dir = work / ABACO_OUT
    abaco_dir.mkdir(parents=True, exist_ok=True)
    session = Session([exe, "--serve"], env, work, work / "serve.stderr.txt")
    try:
        checks_session(rep, session, src, out_dir, axis, csv_path, table_dxf, abaco_dir, rewritten_dxf)
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
    rows = check_abaco_csv(rep, csv_path)
    check_abaco_tables(rep, table_dxf, rows)
    lavoro, _ = read_dxf(table_dxf) if Path(table_dxf).is_file() else (None, None)
    abaco_rep = Prefixed(rep, ABACO_PREFIX)
    # Dopo il salvataggio il disegno di lavoro si chiama come `abaco-lavoro.dxf`: e' il nome dei DWG estratti.
    docs = read_extracts(abaco_rep, exe, env, work, abaco_dir, abaco_dir, base=Path(table_dxf).stem)
    check_abaco_extract(abaco_rep, docs, table_corners(lavoro) if lavoro is not None else [])
    check_abaco_rewritten(rep, rewritten_dxf)


# ---------------------------------------------------------------- porte e finestre (BIM parte 3)

LAYER_APERTURE = "AL-APERTURE"
DOOR_PREFIX = "porta, "
DOOR_CMDS = ("AL_PORTA 4.05,1 4.05,1.9 E 2.1", "AL_LOCALE 2,1.5 A", "AL_LOCALE 6,1.5 B")
DOOR_BASE = "casoAporta"


def build_case_a_door(path):
    """Il caso A, forma Continua, con un vano porta nel tramezzo da y = 1,00 a y = 1,90 (0,90 m): le facce del
    tramezzo (x = 4,00 e x = 4,10) si interrompono, nessuna linea di soglia. $INSUNITS = 6, muri su MURI."""
    import ezdxf

    doc = ezdxf.new("R2018")
    doc.header["$INSUNITS"] = 6
    doc.layers.add(LAYER_MURI)
    msp = doc.modelspace()
    attribs = {"layer": LAYER_MURI}

    def line(a, b):
        msp.add_line((a[0], a[1], 0.0), (b[0], b[1], 0.0), dxfattribs=attribs)

    for k, start in enumerate(OUTER):
        line(start, OUTER[(k + 1) % 4])
    # A senza il lato est, B senza il lato ovest, poi le quattro mezze facce del tramezzo.
    for a, b in (((4.00, 0.0), (0.0, 0.0)), ((0.0, 0.0), (0.0, 3.00)), ((0.0, 3.00), (4.00, 3.00)),
                 ((4.10, 0.0), (8.00, 0.0)), ((8.00, 0.0), (8.00, 3.00)), ((8.00, 3.00), (4.10, 3.00)),
                 ((4.00, 0.0), (4.00, 1.00)), ((4.00, 1.90), (4.00, 3.00)),
                 ((4.10, 0.0), (4.10, 1.00)), ((4.10, 1.90), (4.10, 3.00))):
        line(a, b)
    doc.saveas(path)
    return Path(path)


def apertures_of(doc):
    """Le polilinee del layer AL-APERTURE dello spazio modello: lista di liste di vertici (x, y)."""
    out = []
    for e in doc.modelspace().query("LWPOLYLINE"):
        if e.dxf.layer == LAYER_APERTURE:
            out.append([(p[0], p[1]) for p in e.get_points("xy")])
    return out


def room_labels(doc):
    """I testi (MTEXT) dei locali, sul layer AL-LOCALI, con il formato a capo di MTEXT reso in spazio."""
    return sorted(e.plain_text().replace("\n", " ") for e in doc.modelspace().query("MTEXT")
                  if e.dxf.layer == "AL-LOCALI")


def check_door_extract(rep, docs):
    """I tre DWG del caso con la porta: il vano e' una polilinea sola di 0,90 x 0,10 su AL-APERTURE in ognuno
    (la porta e' esistente), i locali A e B misurano 12,00 e 11,70 m2 anche senza una linea di soglia nel
    rilievo, e non resta nulla di nostro nell'XDATA."""
    for key, doc in docs.items():
        found = apertures_of(doc)
        want = {(4.00, 1.00), (4.10, 1.00), (4.10, 1.90), (4.00, 1.90)}
        ok = len(found) == 1 and len(found[0]) == 4 and all(
            any(abs(x - wx) < 1e-6 and abs(y - wy) < 1e-6 for (wx, wy) in want) for (x, y) in found[0])
        rep.check(ok, f"{key}: vano della porta",
                  "una polilinea 0,90 x 0,10 su AL-APERTURE" if ok else f"{len(found)} polilinee: {found}")
        xdata, table = arch_xdata_appids(doc)
        rep.check(not xdata and not table, f"{key}: niente ARCHLINE",
                  "nessun XDATA ne' APPID ARCHLINE*" if not xdata and not table else f"XDATA {xdata}, APPID {table}")
    for key in ("fatto", "progetto"):
        if key not in docs:
            continue
        labels = room_labels(docs[key])
        good = (len(labels) == 2 and "12,00" in labels[0] and labels[0].startswith("A")
                and "11,70" in labels[1] and labels[1].startswith("B"))
        rep.check(good, f"{key}: locali con la porta",
                  "A 12,00 m2 e B 11,70 m2, nessun locale in piu'" if good else f"etichette {labels}")


def check_door_csv(rep, csv_path):
    """Il CSV del caso con la porta: la porta (1,8900 m2), la detrazione dalle pareti di A e di B (a detrarre) e le
    pareti lorde; ogni `quantita` e' il prodotto dei fattori scritti."""
    try:
        bom, crlf, header, rows = read_abaco_csv(csv_path)
    except (OSError, ValueError) as ex:
        rep.check(False, "csv", f"non si legge: {ex}")
        return
    rep.check(bom and crlf and header == CSV_HEADER, "csv: forma", "BOM, CRLF e intestazione come attesi")
    problems = csv_problems(rows)
    rep.check(not problems, "csv: quantita = prodotto dei fattori", f"{len(rows)} righe" if not problems else "; ".join(problems))
    by_code = {row["codice"]: row for row in rows}
    want = {"P1": ("porta", "no", "1,8900"), "LF1-PA": ("pareti locale di fatto", "no", "37,8000"),
            "LF1-PA-P1": ("detrazione pareti", "sì", "1,8900"), "LF2-PA-P1": ("detrazione pareti", "sì", "1,8900")}
    for code, (tipo, detrai, quantity) in want.items():
        row = by_code.get(code)
        ok = row is not None and (row["tipo"], row["detrai"], row["quantita"]) == (tipo, detrai, quantity)
        rep.check(ok, f"csv: riga {code}", f"{tipo}, detrai {detrai}, quantita {quantity}" if ok else str(row))


def run_door_case(rep, exe, env, work):
    """Il caso A con il vano porta: `AL_PORTA` dal vano, i locali A e B (senza linea di soglia), l'altezza del
    livello, `AL_ESTRAI` e `AL_ABACO_CSV`; poi i tre DWG riconvertiti si controllano con ezdxf."""
    work = Path(work)
    work.mkdir(parents=True, exist_ok=True)
    out_dir = work / "out"
    out_dir.mkdir(parents=True, exist_ok=True)
    csv_path = work / "abaco.csv"
    src = build_case_a_door(work / f"{DOOR_BASE}.dxf")
    before = sha256_of(src)
    session = Session([exe, "--serve"], env, work, work / "serve.stderr.txt")
    try:
        resp = session.request({"op": "open", "path": str(src)})
        rep.check(bool(resp.get("ok")), "open", f"disegno aperto, entita': {resp.get('total')}" if resp.get("ok") else str(resp))
        for cmd in (*DOOR_CMDS, HEIGHT_CMD, f"AL_ESTRAI {out_dir}", f"AL_ABACO_CSV {csv_path}"):
            run_ok(rep, session, cmd)
    except BaseException:
        session.kill()
        raise
    code = session.close()
    rep.check(code == 0, "sessione: chiusura", "exit 0" if code == 0 else f"exit {code}. {session.stderr_tail()}")
    after = sha256_of(src)
    rep.check(after == before, "sorgente intatta", "DXF di partenza identico byte per byte (SHA-256)"
              if after == before else "il DXF di partenza e' cambiato")
    docs = read_extracts(rep, exe, env, work, out_dir, work, base=DOOR_BASE)
    check_door_extract(rep, docs)
    check_door_csv(rep, csv_path)


# ---------------------------------------------------------------- sezioni e prospetti (BIM parte 4)

LAYER_SEZIONI = "AL-SEZIONI"
LAYER_TAGLI = "AL-TAGLI"
SECTION_PREFIX = "sezione, "
SECTION_BASE = "casoAsezione"
SECTION_CMDS = (
    "AL_ALTEZZA L 2.70",
    "AL_PORTA 4.05,1 4.05,1.9 E 2.1",
    "AL_LOCALE 2,1.5 A",
    "AL_LOCALE 6,1.5 B",
    "AL_TAGLIO -1,1.5 9,1.5 4,3 A-A",
    "AL_SEZIONE",
    "AL_PROSPETTO S",
)
PHASE_TEXT = {"fatto": "stato di fatto", "comparativa": "demolizioni e costruzioni", "progetto": "stato di progetto"}


def section_texts(doc):
    """I testi (MTEXT) sul layer AL-SEZIONI, con il formato a capo di MTEXT reso in spazio."""
    return sorted(e.plain_text().replace("\n", " ") for e in doc.modelspace().query("MTEXT")
                  if e.dxf.layer == LAYER_SEZIONI)


def has_wall_outline(doc, width, height):
    """C'e' una polilinea chiusa su AL-SEZIONI larga `width` e alta `height` (il poche' di un muro)?"""
    for e in doc.modelspace().query("LWPOLYLINE"):
        if e.dxf.layer != LAYER_SEZIONI:
            continue
        pts = [(p[0], p[1]) for p in e.get_points("xy")]
        xs, ys = [p[0] for p in pts], [p[1] for p in pts]
        if abs((max(xs) - min(xs)) - width) < 1e-6 and abs((max(ys) - min(ys)) - height) < 1e-6:
            return True
    return False


def check_section_extract(rep, docs):
    """I tre DWG con il taglio A-A e il prospetto Sud: ognuno ha solo i disegni della sua fase (il titolo lo dice),
    una sezione e un prospetto, il poche' del muro d'ambito (0,30 x 2,70) e un campitura solida; il taglio c'e' in
    tutti; nessun XDATA ne' APPID ARCHLINE."""
    for key, doc in docs.items():
        texts = section_texts(doc)
        mine = PHASE_TEXT[key]
        titles = [t for t in texts if t.startswith("Sezione A-A") or t.startswith("Prospetto Sud")]
        good = (len(titles) == 2 and all(t.endswith(mine) for t in titles)
                and any(t.startswith("Sezione A-A") for t in titles) and any(t.startswith("Prospetto Sud") for t in titles))
        rep.check(good, f"{key}: titoli", f"{titles}" if good else f"attesi sezione e prospetto «{mine}», trovati {titles}")
        rep.check(has_wall_outline(doc, 0.30, 2.70), f"{key}: poche' del muro 0,30 x 2,70", "trovato")
        hatches = [e for e in doc.modelspace().query("HATCH") if e.dxf.layer == LAYER_SEZIONI]
        rep.check(bool(hatches) and all(h.dxf.solid_fill == 1 for h in hatches), f"{key}: campiture solide",
                  f"{len(hatches)} campiture" if hatches else "nessuna campitura")
        cuts = [e for e in doc.modelspace().query("LINE") if e.dxf.layer == LAYER_TAGLI]
        rep.check(len(cuts) == 1, f"{key}: taglio", f"{len(cuts)} linee su {LAYER_TAGLI}")
        xdata, table = arch_xdata_appids(doc)
        rep.check(not xdata and not table, f"{key}: niente ARCHLINE",
                  "nessun XDATA ne' APPID ARCHLINE*" if not xdata and not table else f"XDATA {xdata}, APPID {table}")


def run_section_case(rep, exe, env, work):
    """Il caso A con la porta: altezza, locali, taglio A-A, `AL_SEZIONE`, `AL_PROSPETTO S` e `AL_ESTRAI`; poi i tre
    DWG riconvertiti si controllano con ezdxf. Rilanciare AL_SEZIONE non deve duplicare i disegni."""
    work = Path(work)
    work.mkdir(parents=True, exist_ok=True)
    out_dir = work / "out"
    out_dir.mkdir(parents=True, exist_ok=True)
    src = build_case_a_door(work / f"{SECTION_BASE}.dxf")
    before = sha256_of(src)
    session = Session([exe, "--serve"], env, work, work / "serve.stderr.txt")
    try:
        resp = session.request({"op": "open", "path": str(src)})
        rep.check(bool(resp.get("ok")), "open", f"disegno aperto, entita': {resp.get('total')}" if resp.get("ok") else str(resp))
        for cmd in (*SECTION_CMDS, "AL_SEZIONE", f"AL_ESTRAI {out_dir}"):
            run_ok(rep, session, cmd)
    except BaseException:
        session.kill()
        raise
    code = session.close()
    rep.check(code == 0, "sessione: chiusura", "exit 0" if code == 0 else f"exit {code}. {session.stderr_tail()}")
    after = sha256_of(src)
    rep.check(after == before, "sorgente intatta", "DXF di partenza identico byte per byte (SHA-256)"
              if after == before else "il DXF di partenza e' cambiato")
    docs = read_extracts(rep, exe, env, work, out_dir, work, base=SECTION_BASE)
    check_section_extract(rep, docs)


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
    run_door_case(Prefixed(rep, DOOR_PREFIX), exe, env, work / "porta")
    run_section_case(Prefixed(rep, SECTION_PREFIX), exe, env, work / "sezione")
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
