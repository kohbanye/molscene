"""Color palettes, with PyMOL-derived defaults.

These are reference constants the renderer adapters and future Rust coloring can
consume. Values are RGB floats in 0–1, matching PyMOL's ``layer1/Color.cpp``.
"""

from __future__ import annotations

# CPK / element colors (PyMOL Color.cpp:1050).
ELEMENT_COLORS: dict[str, tuple[float, float, float]] = {
    "C": (0.2, 1.0, 0.2),
    "N": (0.2, 0.2, 1.0),
    "O": (1.0, 0.3, 0.3),
    "H": (0.9, 0.9, 0.9),
    "S": (0.9, 0.775, 0.25),
}

# Secondary-structure colors. PyMOL has no hard-coded SS palette; these are
# molscene's own sensible defaults, applied when ``color="secondary_structure"``.
SS_COLORS: dict[str, tuple[float, float, float]] = {
    "helix": (1.0, 0.3, 0.3),   # red
    "sheet": (1.0, 0.9, 0.2),   # yellow
    "loop": (0.9, 0.9, 0.9),    # light grey
}

# Cycling palette for by-chain coloring (PyMOL AutoColor, Color.cpp:35).
CHAIN_PALETTE: list[str] = [
    "cyan", "magenta", "yellow", "salmon", "slate",
    "orange", "lime", "deepteal", "hotpink", "wheat",
]

#: Named color schemes accepted by the ``color=`` argument.
COLOR_SCHEMES = frozenset(
    {"element", "spectrum", "secondary_structure", "chain"}
)
