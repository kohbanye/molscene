"""molscene — Python-first, notebook-native molecular scene builder.

    import molscene as ms

    scene = (
        ms.load("1ubq")
        .cartoon("protein", color="spectrum")
        .surface("protein", opacity=0.25)
        .sticks("ligand", color="element")
    )
    scene.show()
"""

from __future__ import annotations

import os

from . import colors
from .scene import Scene
from .selection import Selection, sel

__all__ = ["load", "Scene", "Selection", "sel", "colors", "__version__"]
__version__ = "0.1.0"


def _looks_like_pdb_id(source: str) -> bool:
    return len(source) == 4 and source[0].isdigit() and source.isalnum()


def load(source: str) -> Scene:
    """Load a structure into a new :class:`Scene`.

    ``source`` may be a 4-character PDB id (fetched from RCSB by the renderer),
    or a path to a local ``.pdb`` / ``.cif`` file (embedded inline).
    """
    if _looks_like_pdb_id(source):
        return Scene.from_rcsb(source.lower())
    if os.path.exists(source):
        with open(source, encoding="utf-8") as fh:
            return Scene.from_inline_pdb(fh.read())
    # Fall back to treating it as an RCSB id (lets short non-standard ids work).
    return Scene.from_rcsb(source.lower())
