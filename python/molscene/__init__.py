"""molscene — notebook-native molecular visualization with a Rust core.

Example::

    import molscene as ms

    scene = (
        ms.load("1ubq")
        .cartoon(ms.select.protein(), color="spectrum")
        .surface(ms.select.protein(), opacity=0.25)
        .sticks(ms.select.ligand(), color="element")
    )
    scene.show()
"""

from __future__ import annotations

import os

from . import colors
from .scene import Representation, Scene
from .selection import Selection, select

__all__ = [
    "load",
    "Scene",
    "Representation",
    "Selection",
    "select",
    "colors",
    "__version__",
]
__version__ = "0.1.0"


def _fetch_rcsb_pdb(pdb_id: str) -> str:
    """Download a PDB file from RCSB (the only network access in molscene)."""
    import urllib.request

    url = f"https://files.rcsb.org/download/{pdb_id.upper()}.pdb"
    with urllib.request.urlopen(url) as response:  # noqa: S310 (trusted host)
        return response.read().decode("utf-8")


def load(source: str) -> Scene:
    """Load a structure into a new :class:`Scene`.

    ``source`` may be a 4-character PDB id (downloaded from RCSB and parsed), or
    a path to a local ``.pdb`` file, or a local ``.sdf`` / ``.mol`` molfile
    (read and parsed, with explicit bond orders). Coordinates are parsed in Rust
    so the geometry can be generated natively.
    """
    if os.path.exists(source):
        with open(source, encoding="utf-8") as fh:
            text = fh.read()
        if source.lower().endswith((".sdf", ".mol")):
            return Scene.from_inline_sdf(text)
        return Scene.from_inline_pdb(text)
    pdb_id = source.lower()
    return Scene.from_rcsb(pdb_id, _fetch_rcsb_pdb(pdb_id))
