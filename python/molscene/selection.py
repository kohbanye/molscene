"""The ``ms.select`` selection DSL.

Selections wrap a selection string; the boolean operators ``&`` / ``|`` / ``~``
compose them, backed by the Rust ``_core.Selection`` type. The core parses the
string into an expression tree and evaluates it natively — boolean composition,
spatial operators (``around`` / ``within`` / ``expand`` / ``beyond``),
aggregation (``byres`` / ``bychain`` / ``bymol``) and numeric ``b`` / ``q``
predicates. Invalid selections raise ``ValueError`` when added to a scene.

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


class _Select:
    """Namespace of selection constructors, exposed as ``molscene.select``."""

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

    # Numeric predicates: b-factor / occupancy comparisons.
    def b(self, op: str, value: float) -> "Selection":
        return _wrap(f"b {op} {value}")

    def q(self, op: str, value: float) -> "Selection":
        return _wrap(f"q {op} {value}")

    # Spatial operators (radius in Å of an operand selection).
    def around(self, selection: SelectionLike, radius: float) -> "Selection":
        return _wrap(f"around {radius} of ({selection})")

    def within(self, selection: SelectionLike, radius: float) -> "Selection":
        return _wrap(f"within {radius} of ({selection})")

    def expand(self, selection: SelectionLike, radius: float) -> "Selection":
        return _wrap(f"expand {radius} of ({selection})")

    def beyond(self, selection: SelectionLike, radius: float) -> "Selection":
        return _wrap(f"beyond {radius} of ({selection})")

    # Aggregation: expand to whole residue / chain / bonded molecule.
    def byres(self, selection: SelectionLike) -> "Selection":
        return _wrap(f"byres ({selection})")

    def bychain(self, selection: SelectionLike) -> "Selection":
        return _wrap(f"bychain ({selection})")

    def bymol(self, selection: SelectionLike) -> "Selection":
        return _wrap(f"bymol ({selection})")


#: The selection DSL namespace.
select = _Select()
