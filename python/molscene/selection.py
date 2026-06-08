"""The ``ms.select`` selection DSL.

Selections are typed values backed by the Rust ``_core.Selection`` (an ``Expr``
tree), built through these constructors and composed with the boolean operators
``&`` / ``|`` / ``~``. There is no selection string to parse — a selection is
valid by construction.
"""

from __future__ import annotations

from . import _core

# Re-export the Rust-backed type so ``isinstance(x, ms.Selection)`` works and the
# operators (__and__/__or__/__invert__) come from the core.
Selection = _core.Selection


class _Select:
    """Namespace of selection constructors, exposed as ``molscene.select``."""

    # Zero-argument macros.
    def all(self) -> Selection:
        return _core.Selection.all()

    def none(self) -> Selection:
        return _core.Selection.none()

    def protein(self) -> Selection:
        return _core.Selection.protein()

    def nucleic(self) -> Selection:
        return _core.Selection.nucleic()

    def ligand(self) -> Selection:
        return _core.Selection.ligand()

    def water(self) -> Selection:
        return _core.Selection.water()

    def solvent(self) -> Selection:
        """Water *and* common crystallographic ions (Na, Cl, Mg, SO4, …) — the
        buffer/solvent that shouldn't land in the default view by accident."""
        return _core.Selection.solvent()

    def hetero(self) -> Selection:
        return _core.Selection.hetero()

    def backbone(self) -> Selection:
        return _core.Selection.backbone()

    def sidechain(self) -> Selection:
        return _core.Selection.sidechain()

    def hydrogen(self) -> Selection:
        return _core.Selection.hydrogen()

    # One-argument predicates.
    def chain(self, chain_id: str) -> Selection:
        return _core.Selection.chain(chain_id)

    def resn(self, name: str) -> Selection:
        return _core.Selection.resn(name)

    def resi(self, start: int, end: int | None = None) -> Selection:
        return _core.Selection.resi(start, end)

    def element(self, symbol: str) -> Selection:
        return _core.Selection.element(symbol)

    # Numeric predicates: b-factor / occupancy comparisons.
    def b(self, op: str, value: float) -> Selection:
        return _core.Selection.b(op, value)

    def q(self, op: str, value: float) -> Selection:
        return _core.Selection.q(op, value)

    # Spatial operators (radius in Å of an operand selection).
    def around(self, selection: Selection, radius: float) -> Selection:
        return _core.Selection.around(selection, radius)

    def within(self, selection: Selection, radius: float) -> Selection:
        return _core.Selection.within(selection, radius)

    def expand(self, selection: Selection, radius: float) -> Selection:
        return _core.Selection.expand(selection, radius)

    def beyond(self, selection: Selection, radius: float) -> Selection:
        return _core.Selection.beyond(selection, radius)

    # Aggregation: expand to whole residue / chain / bonded molecule.
    def byres(self, selection: Selection) -> Selection:
        return _core.Selection.byres(selection)

    def bychain(self, selection: Selection) -> Selection:
        return _core.Selection.bychain(selection)

    def bymol(self, selection: Selection) -> Selection:
        return _core.Selection.bymol(selection)


#: The selection DSL namespace.
select = _Select()
