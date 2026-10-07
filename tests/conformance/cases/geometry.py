"""Geometria: blocchi annidati, array ruotato, scale negative, estrusione -Z, bulge, polilinee, solido."""
import ezdxf

from ._common import save

NAME = "geometry"


def build(version, work):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    doc.layers.add("MURI", color=1)
    msp.add_line((0, 0), (10, 5), dxfattribs={"layer": "MURI"})
    msp.add_circle((20, 10), 3)
    msp.add_arc((30, 0), 5, 20, 200)
    msp.add_arc((30, 0), 3, 300, 60)  # passa per 0 gradi
    msp.add_lwpolyline([(0, 20, 0, 0, 0.5), (10, 20, 0, 0, -0.8), (15, 28, 0, 0, 0)], format="xyseb", close=True)
    msp.add_polyline2d([(40, 0, 0.4), (50, 0, 0), (55, 8, -0.6), (45, 12, 0)], format="xyb", close=True)
    msp.add_solid([(60, 0), (65, 0), (60, 5), (65, 5)])

    b1 = doc.blocks.new("B1", base_point=(1, 2))
    b1.add_line((0, 0), (4, 0))
    b1.add_arc((2, 2), 2, 0, 180)
    b1.add_circle((1, 1), 0.5)
    b1.add_lwpolyline([(0, 0), (3, 0), (3, 3)], format="xy")
    b2 = doc.blocks.new("B2")
    b2.add_blockref("B1", (5, 5), dxfattribs={"xscale": 2, "yscale": 0.5, "rotation": 30})
    b2.add_line((0, 0), (0, 10))
    msp.add_blockref("B1", (100, 0))
    msp.add_blockref("B2", (120, 10), dxfattribs={"rotation": 45, "xscale": -1.5, "yscale": 2})
    msp.add_blockref("B1", (150, 0), dxfattribs={"xscale": -1, "yscale": -1, "rotation": 10})
    arr = msp.add_blockref("B1", (0, 60), dxfattribs={"rotation": 15})
    arr.grid(size=(3, 4), spacing=(12, 9))
    # estrusione (0,0,-1): OCS speculare
    msp.add_circle((10, 40), 2, dxfattribs={"extrusion": (0, 0, -1)})
    msp.add_arc((20, 40), 4, 10, 120, dxfattribs={"extrusion": (0, 0, -1)})
    msp.add_lwpolyline([(30, 40, 0, 0, 0.5), (40, 45, 0, 0, 0), (45, 38, 0, 0, 0)], format="xyseb", dxfattribs={"extrusion": (0, 0, -1)})
    msp.add_blockref("B1", (60, 40), dxfattribs={"extrusion": (0, 0, -1), "rotation": 20, "xscale": 1.5})
    return save(doc, work, NAME, version)
