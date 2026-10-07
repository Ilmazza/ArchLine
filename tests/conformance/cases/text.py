"""Testo: TEXT con codici %%, allineamenti, rotazione; MTEXT con formattazione e oltre 250 caratteri."""
import ezdxf
from ezdxf.enums import TextEntityAlignment

from ._common import save

NAME = "text"


def build(version, work):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    msp.add_text("Planimetria %%c120 %%d 45%%p1", height=2.5).set_placement((0, 0))
    msp.add_text("Sottotetto è più basso: 2,40 m ù à ò ì é", height=2).set_placement((0, 10))
    msp.add_text("Centrato", height=2).set_placement((50, 50), align=TextEntityAlignment.MIDDLE_CENTER)
    msp.add_text("Destra", height=2).set_placement((50, 60), align=TextEntityAlignment.BOTTOM_RIGHT)
    msp.add_text("Ruotato", height=2, rotation=30).set_placement((10, 20))
    msp.add_mtext("{\\fArial|b1|i0;Titolo}\\PSecondo riga\\P\\C1;rosso \\U+00E8 \\~ fine", dxfattribs={"insert": (0, 30), "char_height": 3})
    msp.add_mtext("A" * 300 + "\\PB", dxfattribs={"insert": (0, 40), "char_height": 1})  # > 250 caratteri: piu' blocchi gruppo 3
    return save(doc, work, NAME, version)
