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

from classify import CHECKS, REGRESSION, Result, classify, key, load_expected, save_expected
import report

HERE = Path(__file__).resolve().parent
VERSIONS = ("R2000", "R2018")
PATHS = ("dxf", "dwg")


class BenchError(Exception):
    """Errore del banco (non una divergenza dell'app): exit 2."""


def converter_cmd(exe):
    exe = str(exe)
    if Path(exe).exists():
        exe = str(Path(exe).resolve())  # su Windows subprocess non trova 'target/release/x.exe' (barre /)
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
