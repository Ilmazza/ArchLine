"""Accenti italiani in TEXT, MTEXT e ATTRIB.

Difetto noto (baseline del 03/10): nei file R2000 ($DWGCODEPAGE = ANSI_1252) ArchLine scrive i caratteri
accentati come UTF-8, e chi rispetta il codepage legge 'e con accento' come due caratteri (mojibake).
"""
import ezdxf

from ._common import save

NAME = "text_accents"


def build(version, work):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    msp.add_text("Sottotetto è più basso: ù à ò ì é", height=2).set_placement((0, 0))
    msp.add_mtext("Cucina: perché già così\\Pcittà è", dxfattribs={"insert": (0, 10), "char_height": 2})
    blk = doc.blocks.new("TARGA")
    blk.add_attdef("NOTE", (0, 0), "-", dxfattribs={"height": 0.5})
    ins = msp.add_blockref("TARGA", (20, 20))
    ins.add_auto_attribs({"NOTE": "porta è blindata"})
    return save(doc, work, NAME, version)
