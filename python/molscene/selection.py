"""The ``ms.sel`` selection DSL.

Selections wrap an opaque selection string in v0.1 (the renderer interprets it);
the boolean operators ``&`` / ``|`` / ``~`` compose them, backed by the Rust
``_core.Selection`` type. A real expression tree evaluated in Rust lands in v0.2.

Anything that takes a selection also accepts a plain string.
"""

from __future__ import annotations

from . import _core

# Re-export the Rust-backed type so ``isinstance(x, ms.Selection)`` works and the
# operators (__and__/__or__/__invert__) come from the core.
Selection = _core.Selection

SelectionLike = "Selection | str"


def _wrap(value: str) -> "Selection":
    return Selection(value)


class _Sel:
    """Namespace of selection constructors, exposed as ``molscene.sel``."""

    # Zero-argument macros.
    def all(self) -> "Selection":
        return _wrap("all")

    def none(self) -> "Selection":
        return _wrap("none")

    def protein(self) -> "Selection":
        return _wrap("protein")

    def nucleic(self) -> "Selection":
        return _wrap("nucleic")

    def ligand(self) -> "Selection":
        return _wrap("ligand")

    def water(self) -> "Selection":
        return _wrap("water")

    def hetero(self) -> "Selection":
        return _wrap("hetero")

    def backbone(self) -> "Selection":
        return _wrap("backbone")

    def sidechain(self) -> "Selection":
        return _wrap("sidechain")

    def hydrogen(self) -> "Selection":
        return _wrap("hydrogen")

    # One-argument predicates.
    def chain(self, chain_id: str) -> "Selection":
        return _wrap(f"chain {chain_id}")

    def resn(self, name: str) -> "Selection":
        return _wrap(f"resn {name}")

    def resi(self, start: int, end: int | None = None) -> "Selection":
        return _wrap(f"resi {start}-{end}" if end is not None else f"resi {start}")

    def element(self, symbol: str) -> "Selection":
        return _wrap(f"element {symbol}")

    # Spatial operators (evaluated in v0.2; the strings are recorded now).
    def around(self, selection: SelectionLike, radius: float) -> "Selection":
        return _wrap(f"around {radius} of ({selection})")

    def within(self, selection: SelectionLike, radius: float) -> "Selection":
        return _wrap(f"within {radius} of ({selection})")


#: The selection DSL namespace.
sel = _Sel()
