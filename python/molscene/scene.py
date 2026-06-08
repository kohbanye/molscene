"""The fluent ``Scene`` facade.

State lives in Rust (``_core.Scene``); this class adds the ergonomic
keyword-argument API and notebook display. Every styling method returns ``self``
so calls chain. Selections are ``ms.select`` values (never strings).
"""

from __future__ import annotations

import json

from . import _core
from ._viewer import DEFAULT_HEIGHT, render_html
from .selection import Selection, select


def _coerce(selection: Selection) -> Selection:
    """Reject string selections with a pointer to the DSL."""
    if isinstance(selection, str):
        raise TypeError(
            "selections are built with ms.select, not strings "
            f"(e.g. ms.select.chain({selection!r}) or ms.select.protein())"
        )
    return selection


class Scene:
    def __init__(self, core: _core.Scene) -> None:
        self._core = core
        self._n_reps = 0

    # -- constructors -------------------------------------------------------
    @classmethod
    def from_rcsb(cls, pdb_id: str, pdb_text: str) -> Scene:
        return cls(_core.Scene.from_rcsb(pdb_id, pdb_text))

    @classmethod
    def from_inline_pdb(cls, pdb_text: str) -> Scene:
        return cls(_core.Scene.from_inline_pdb(pdb_text))

    @classmethod
    def from_inline_sdf(cls, sdf_text: str) -> Scene:
        return cls(_core.Scene.from_inline_sdf(sdf_text))

    # -- representations ----------------------------------------------------
    def _add(
        self,
        kind: str,
        selection: Selection,
        *,
        color=None,
        opacity=None,
        scale=None,
        radius=None,
        text=None,
    ) -> Scene:
        self._core.representation(
            kind,
            _coerce(selection),
            color=color,
            opacity=opacity,
            scale=scale,
            radius=radius,
            text=text,
        )
        self._n_reps += 1
        return self

    def cartoon(
        self, selection: Selection | None = None, *, color=None, opacity=None
    ) -> Scene:
        """Protein/nucleic ribbons. Defaults to ``ms.select.protein()`` — crystal
        waters and solvent ions are never drawn as cartoon."""
        sel = selection if selection is not None else select.protein()
        return self._add("cartoon", sel, color=color, opacity=opacity)

    def surface(
        self, selection: Selection | None = None, *, color=None, opacity=None
    ) -> Scene:
        """Molecular surface. Defaults to ``ms.select.protein()`` — water/solvent
        is excluded."""
        sel = selection if selection is not None else select.protein()
        return self._add("surface", sel, color=color, opacity=opacity)

    def sticks(
        self,
        selection: Selection | None = None,
        *,
        color=None,
        opacity=None,
        radius=None,
    ) -> Scene:
        """Bond sticks. Defaults to ``ms.select.ligand()`` (hetero atoms minus
        water), so crystal waters aren't drawn by accident."""
        sel = selection if selection is not None else select.ligand()
        return self._add("sticks", sel, color=color, opacity=opacity, radius=radius)

    def spheres(
        self,
        selection: Selection | None = None,
        *,
        color=None,
        opacity=None,
        scale=None,
    ) -> Scene:
        """Atom spheres. Defaults to everything *except* solvent
        (``ms.select.all() & ~ms.select.solvent()``) so crystal waters and buffer
        ions aren't dumped into the view. Pass ``ms.select.all()`` (or
        ``ms.select.water()`` / ``ms.select.solvent()``) explicitly to show them."""
        sel = selection if selection is not None else select.all() & ~select.solvent()
        return self._add("spheres", sel, color=color, opacity=opacity, scale=scale)

    def lines(
        self,
        selection: Selection | None = None,
        *,
        color=None,
        opacity=None,
        radius=None,
    ) -> Scene:
        """Thin bond lines (a cheap wireframe). Like ``sticks`` but without the
        ball-and-stick atom caps; bond orders are still drawn::

            ms.load("1ubq").lines(ms.select.protein())
        """
        sel = selection if selection is not None else select.all()
        return self._add("lines", sel, color=color, opacity=opacity, radius=radius)

    def dots(
        self,
        selection: Selection | None = None,
        *,
        color=None,
        opacity=None,
        scale=None,
    ) -> Scene:
        """A point cloud: a small sphere per atom (cheaper than ``spheres``)::

        ms.load("1ubq").dots(ms.select.protein())
        """
        sel = selection if selection is not None else select.all()
        return self._add("dots", sel, color=color, opacity=opacity, scale=scale)

    def label(
        self,
        selection: Selection | None = None,
        *,
        text=None,
        color=None,
        size=None,
    ) -> Scene:
        """Text annotations drawn as camera-facing billboards. Defaults to
        ``ms.select.ligand()`` and one label per residue (``"ALA42"``)::

            ms.load("1ubq").cartoon().label(ms.select.resi(50))

        ``text`` picks the content: ``"residue"`` (default), ``"resn"``,
        ``"resi"``, ``"chain"`` (one per residue) or ``"atom"`` / ``"element"``
        (one per atom). Labels default to black; pass ``color`` to recolor and
        ``size`` to scale the font::

            ms.load("lig.sdf").sticks().label(text="atom", size=1.5)
        """
        sel = selection if selection is not None else select.ligand()
        return self._add("labels", sel, text=text, color=color, scale=size)

    def center(self, selection: Selection | None = None) -> Scene:
        """Frame the view on a selection (still auto-fits the zoom)::

        ms.load("1ubq").cartoon().center(ms.select.resi(50))
        """
        if selection is not None:
            self._core.set_center(_coerce(selection))
        return self

    def orient(self, selection: Selection | None = None) -> Scene:
        """Orient the view along a selection's principal axes (PyMOL-style
        ``orient``): its longest dimension goes horizontal, the next vertical::

            ms.load("1ubq").cartoon().orient(ms.select.protein())
        """
        if selection is not None:
            self._core.set_orient(_coerce(selection))
        return self

    def set_color(self, selection: Selection, color: str) -> Scene:
        """Override the color of a sub-selection, on top of the representations'
        schemes. Applied in call order (last write wins), so you can color
        everything one way and then repaint a few atoms::

            ms.load("1ubq").cartoon(color="grey").set_color(ms.select.resi(50), "red")
        """
        self._core.set_color(_coerce(selection), color)
        return self

    def background(self, color: str) -> Scene:
        """Set the scene background color (a named color or ``#rrggbb``).

        Defaults to white when never called::

            ms.load("1ubq").cartoon().background("black")
        """
        self._core.set_background(color)
        return self

    # -- serialization ------------------------------------------------------
    def to_geometry(self) -> dict:
        """The compiled geometry spec (instanced draw list) the renderer draws.

        This is the only serialized form — there is no declarative scene spec;
        the code that builds the scene is the source of truth.
        """
        return json.loads(self._core.to_geometry_json())

    # -- display ------------------------------------------------------------
    def _repr_html_(self) -> str:
        return render_html(self.to_geometry())

    def show(self, *, height: int = DEFAULT_HEIGHT, width: str = "100%"):
        from IPython.display import HTML

        return HTML(render_html(self.to_geometry(), height=height, width=width))

    def export_html(
        self, path: str, *, height: int = DEFAULT_HEIGHT, width: str = "100%"
    ) -> str:
        markup = render_html(self.to_geometry(), height=height, width=width)
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(markup)
        return path

    def __repr__(self) -> str:
        return f"<molscene.Scene: {self._n_reps} representation(s)>"
