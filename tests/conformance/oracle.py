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
