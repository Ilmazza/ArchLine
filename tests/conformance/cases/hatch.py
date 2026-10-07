"""Hatch: solido con bulge, pattern ANSI31, isola, ellisse, cerchio."""
import ezdxf

from ._common import save

NAME = "hatch"


def build(version, work):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    h1 = msp.add_hatch(color=2)
    h1.paths.add_polyline_path([(0, 0), (10, 0, 0.5), (10, 10), (0, 10)], is_closed=True)
    h2 = msp.add_hatch()
    h2.set_pattern_fill("ANSI31", scale=2)
    ep = h2.paths.add_edge_path()
    ep.add_line((20, 0), (30, 0))
    ep.add_arc((30, 5), 5, -90, 90)
    ep.add_line((30, 10), (20, 10))
    ep.add_arc((20, 5), 5, 90, 270)
    h3 = msp.add_hatch(color=3)
    h3.paths.add_polyline_path([(40, 0), (60, 0), (60, 20), (40, 20)], is_closed=True)
    h3.paths.add_polyline_path([(45, 5), (55, 5), (55, 15), (45, 15)], is_closed=True, flags=0)
    h4 = msp.add_hatch()
    h4.paths.add_edge_path().add_ellipse((80, 10), major_axis=(10, 0), ratio=0.5, start_angle=0, end_angle=360, ccw=True)
    h5 = msp.add_hatch()
    h5.paths.add_edge_path().add_arc((100, 10), 5, 0, 360, ccw=True)
    return save(doc, work, NAME, version)
