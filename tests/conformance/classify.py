"""Stati di un controllo e lettura/scrittura di expected.json."""
import json
from pathlib import Path
from typing import NamedTuple

OK = "OK"
EXPECTED = "ATTESO"
REGRESSION = "REGRESSIONE"
IMPROVED = "MIGLIORATO"

CHECKS = ("readable", "types", "extents", "strings")


class Result(NamedTuple):
    case: str
    version: str
    path: str  # "dxf" (DXF -> DXF) oppure "dwg" (DXF -> DWG -> DXF)
    check: str
    ok: bool
    detail: str
    state: str


def key(case, version, path, check):
    return f"{case}|{version}|{path}|{check}"


def classify(ok, detail, entry):
    """`entry` e' la voce di expected.json per questo controllo, o None.

    Un esito diverso da quello registrato conta come regressione: i dettagli non hanno un ordine,
    quindi non si puo' dire quale diverga di meno. Lo risolve una persona con --update-expected.
    """
    if ok:
        return IMPROVED if entry is not None else OK
    if entry is None:
        return REGRESSION
    return EXPECTED if entry["detail"] == detail else REGRESSION


def load_expected(path):
    p = Path(path)
    if not p.exists():
        return {}
    data = json.loads(p.read_text(encoding="utf-8"))
    for k, v in data.items():
        if not str(v.get("detail", "")).strip() or not str(v.get("note", "")).strip():
            raise ValueError(f"expected.json: la voce {k!r} deve avere 'detail' e 'note' non vuoti")
    return data


def save_expected(path, data):
    text = json.dumps(data, indent=2, ensure_ascii=False, sort_keys=True) + "\n"
    Path(path).write_bytes(text.encode("utf-8"))
