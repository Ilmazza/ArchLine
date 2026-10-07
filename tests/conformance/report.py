"""Report Markdown del banco di conformita."""
from collections import Counter

from classify import CHECKS, EXPECTED, IMPROVED, OK, REGRESSION, key

PATH_LABEL = {"dxf": "DXF>DXF", "dwg": "DXF>DWG>DXF"}
STATES = (OK, EXPECTED, REGRESSION, IMPROVED)

DWG_WARNING = (
    "Avvertenza: il percorso DWG e' verificato solo in modo indiretto. ezdxf non legge DWG: il DWG scritto "
    "da ArchLine viene riletto da ArchLine stesso e confrontato dopo l'export in DXF."
)


def _where(r):
    return f"{r.case} {r.version} {PATH_LABEL[r.path]} / {r.check}"


def render(meta, results, expected):
    out = ["# Banco di conformita ArchLine", ""]
    out += [f"- {label}: {value}" for label, value in meta]
    out += ["", DWG_WARNING, ""]
    out.append("| Caso | Versione | Percorso | " + " | ".join(CHECKS) + " |")
    out.append("|---|---|---|" + "---|" * len(CHECKS))
    rows = {}
    for r in results:
        rows.setdefault((r.case, r.version, r.path), {})[r.check] = r.state
    for (case, version, path), states in rows.items():
        cells = " | ".join(states.get(c, "-") for c in CHECKS)
        out.append(f"| {case} | {version} | {PATH_LABEL[path]} | {cells} |")
    for title, state in (("Regressioni", REGRESSION), ("Migliorati", IMPROVED), ("Attesi", EXPECTED)):
        picked = [r for r in results if r.state == state]
        if not picked:
            continue
        out += ["", f"## {title} ({len(picked)})", ""]
        for r in picked:
            line = f"- {_where(r)}"
            entry = expected.get(key(r.case, r.version, r.path, r.check))
            if state == REGRESSION:
                line += f": {r.detail}"
                if entry:
                    line += f" (registrato: {entry['detail']})"
            elif state == IMPROVED:
                line += f" (era: {entry['detail']}; nota: {entry['note']})"
            else:
                line += f": {r.detail} - nota: {entry['note']}"
            out.append(line)
    count = Counter(r.state for r in results)
    out += ["", "## Totali", ""] + [f"- {s}: {count.get(s, 0)}" for s in STATES]
    return "\n".join(out) + "\n"
