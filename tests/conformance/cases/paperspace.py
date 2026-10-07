"""Paper space: le entita' del layout non devono entrare nelle estensioni del model space."""
import ezdxf

from ._common import save

NAME = "paperspace"


def build(version, work):
    doc = ezdxf.new(version)
    msp = doc.modelspace()
    msp.add_line((0, 0), (10, 10))
    doc.layout("Layout1").add_line((0, 0), (500, 500))
    return save(doc, work, NAME, version)
