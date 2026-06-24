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


# Order of the style tuple exchanged with `_core` (rep_style / set_rep_style).
_STYLE_FIELDS = ("color", "opacity", "scale", "radius", "text")


class Representation:
    """An editable handle on one representation already added to a scene.

    Style fields can be read and reassigned in place after the fact, without
    rebuilding the scene — geometry is recomputed lazily at display time, so
    edits here are reflected by the next ``show`` / ``to_geometry``::

        scene = ms.load("1ubq").cartoon().surface()
        scene.representations[1].opacity = 0.3
        scene.representations[0].color = "spectrum"
        scene.representations[0].selection = ms.select.chain("A")

    ``kind`` is read-only (it picks the geometry path); ``selection`` and the
    style fields ``color`` / ``opacity`` / ``scale`` / ``radius`` / ``text`` are
    read/write. Setting a field to ``None`` clears it back to the default.
    """

    __slots__ = ("_core", "_index")

    def __init__(self, core: _core.Scene, index: int) -> None:
        self._core = core
        self._index = index

    @property
    def kind(self) -> str:
        return self._core.rep_kind(self._index)

    @property
    def selection(self) -> Selection:
        return self._core.rep_selection(self._index)

    @selection.setter
    def selection(self, value: Selection) -> None:
        self._core.set_rep_selection(self._index, _coerce(value))

    def _get(self, field: str):
        style = self._core.rep_style(self._index)
        return style[_STYLE_FIELDS.index(field)]

    def _set(self, field: str, value) -> None:
        style = list(self._core.rep_style(self._index))
        style[_STYLE_FIELDS.index(field)] = value
        self._core.set_rep_style(self._index, *style)

    @property
    def color(self) -> str | None:
        return self._get("color")

    @color.setter
    def color(self, value: str | None) -> None:
        self._set("color", value)

    @property
    def opacity(self) -> float | None:
        return self._get("opacity")

    @opacity.setter
    def opacity(self, value: float | None) -> None:
        self._set("opacity", value)

    @property
    def scale(self) -> float | None:
        return self._get("scale")

    @scale.setter
    def scale(self, value: float | None) -> None:
        self._set("scale", value)

    @property
    def radius(self) -> float | None:
        return self._get("radius")

    @radius.setter
    def radius(self, value: float | None) -> None:
        self._set("radius", value)

    @property
    def text(self) -> str | None:
        return self._get("text")

    @text.setter
    def text(self, value: str | None) -> None:
        self._set("text", value)

    def __repr__(self) -> str:
        return (
            f"<molscene.Representation {self.kind} "
            f"selection={self.selection!s} color={self.color!r} "
            f"opacity={self.opacity!r}>"
        )


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
    @property
    def representations(self) -> list[Representation]:
        """Editable handles on the representations added so far, in order.

        Indexable and iterable (``scene.representations[0]``, ``for rep in
        scene.representations``); each item's style fields and selection can be
        reassigned in place — see :class:`Representation`.
        """
        return [
            Representation(self._core, i)
            for i in range(self._core.num_representations())
        ]

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

    # -- image export -------------------------------------------------------
    def to_png(self, *, width: int = 800, height: int = 600, ssaa: int = 2) -> bytes:
        """Render the scene to PNG bytes with the native Rust GPU rasterizer.

        Unlike :meth:`show` (which hands the geometry to Three.js in the
        browser), this rasterizes the same ``GeometrySpec`` headlessly in Rust
        via ``wgpu`` — spheres and bonds are drawn as ray-traced impostors —
        so it works without a notebook or browser::

            png = ms.load("1ubq").cartoon().to_png(width=1200, height=900)

        ``ssaa`` is the supersampling factor for antialiasing (``1`` disables
        it). Raises ``RuntimeError`` if no GPU (or software fallback such as
        SwiftShader/llvmpipe) is available in the environment.
        """
        return self._core.to_png(width=width, height=height, ssaa=ssaa)

    def save_png(
        self, path: str, *, width: int = 800, height: int = 600, ssaa: int = 2
    ) -> str:
        """Render the scene and write it to ``path`` as a PNG. Returns ``path``::

        ms.load("1ubq").cartoon(color="spectrum").save_png("ubq.png")
        """
        with open(path, "wb") as fh:
            fh.write(self.to_png(width=width, height=height, ssaa=ssaa))
        return path

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
