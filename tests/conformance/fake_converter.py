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
