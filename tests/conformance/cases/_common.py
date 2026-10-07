"""Utilita' comuni ai casi."""


def save(doc, work, name, version):
    path = work / f"{name}_{version}.dxf"
    doc.saveas(path)
    return path
