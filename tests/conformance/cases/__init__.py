"""Casi del banco: ogni modulo espone NAME e build(version, work) -> Path."""
from . import attribs, colors, dims, geometry, hatch, hatch_spline, paperspace, text, text_accents

ALL = [geometry, colors, hatch, hatch_spline, text, text_accents, dims, attribs, paperspace]
