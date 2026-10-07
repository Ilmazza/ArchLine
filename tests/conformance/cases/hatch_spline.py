"""Hatch con contorno a spline (la baseline del 03/10 ha visto i punti di fit passare da 3 a 0 in R2000).

Riserva: la spline sintetica ha solo punti di fit e nessun punto di controllo, cosa che AutoCAD di norma
non scrive.
"""
import ezdxf

from ._common import save

NAME = "hatch_spline"


def build(version, work):
    doc = ezdxf.new(version)
    h = doc.modelspace().add_hatch()
    h.paths.add_edge_path().add_spline(fit_points=[(0, 0), (5, 3), (10, 0)])
    h.paths[0].add_line((10, 0), (0, 0))
    return save(doc, work, NAME, version)
