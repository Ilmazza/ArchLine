"""Quote: lineare, allineata, raggio, diametro, angolare."""
import ezdxf

from ._common import save

NAME = "dims"


def build(version, work):
    doc = ezdxf.new(version, setup=True)
    msp = doc.modelspace()
    msp.add_linear_dim(base=(5, 3), p1=(0, 0), p2=(10, 0)).render()
    msp.add_linear_dim(base=(-3, 5), p1=(0, 0), p2=(0, 10), angle=90).render()
    msp.add_aligned_dim(p1=(20, 0), p2=(30, 8), distance=2).render()
    msp.add_radius_dim(center=(50, 5), radius=4, angle=45).render()
    msp.add_diameter_dim(center=(70, 5), radius=4, angle=30).render()
    msp.add_angular_dim_3p(base=(95, 12), center=(90, 0), p1=(100, 0), p2=(90, 10)).render()
    msp.add_circle((50, 5), 4)
    return save(doc, work, NAME, version)
