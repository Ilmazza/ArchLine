"""Colori e layer in blocchi annidati: BYBLOCK, BYLAYER, layer 0."""
import ezdxf

from ._common import save

NAME = "colors"


def build(version, work):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    doc.layers.add("L1", color=3)
    doc.layers.add("WALL", color=6)
    inner = doc.blocks.new("INNER")
    inner.add_line((0, 0), (1, 0), dxfattribs={"layer": "0", "color": 0})       # '0' + BYBLOCK
    inner.add_line((0, 1), (1, 1), dxfattribs={"layer": "0", "color": 256})     # '0' + BYLAYER
    inner.add_line((0, 2), (1, 2), dxfattribs={"layer": "WALL", "color": 256})  # layer proprio
    inner.add_line((0, 3), (1, 3), dxfattribs={"layer": "WALL", "color": 0})    # layer proprio + BYBLOCK
    inner.add_line((0, 4), (1, 4), dxfattribs={"layer": "0", "color": 5})       # esplicito
    outer = doc.blocks.new("OUTER")
    outer.add_blockref("INNER", (0, 0), dxfattribs={"layer": "0", "color": 0})
    outer.add_blockref("INNER", (3, 0), dxfattribs={"layer": "0", "color": 2})
    msp.add_blockref("INNER", (0, 0), dxfattribs={"layer": "L1", "color": 256})
    msp.add_blockref("INNER", (10, 0), dxfattribs={"layer": "L1", "color": 1})
    msp.add_blockref("OUTER", (20, 0), dxfattribs={"layer": "L1", "color": 256})
    msp.add_blockref("OUTER", (40, 0), dxfattribs={"layer": "WALL", "color": 4})
    return save(doc, work, NAME, version)
