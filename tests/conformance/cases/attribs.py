"""Attributi di blocco (ATTDEF + ATTRIB)."""
import ezdxf

from ._common import save

NAME = "attribs"


def build(version, work):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    blk = doc.blocks.new("TAG")
    blk.add_circle((0, 0), 1)
    blk.add_attdef("NUM", (0, 0), "0", dxfattribs={"height": 0.5})
    blk.add_attdef("NOTE", (0, -2), "-", dxfattribs={"height": 0.5})
    ins = msp.add_blockref("TAG", (10, 10))
    ins.add_auto_attribs({"NUM": "A1", "NOTE": "porta"})
    return save(doc, work, NAME, version)
