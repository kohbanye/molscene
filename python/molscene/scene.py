"""The fluent ``Scene`` facade.

State and serialization live in Rust (``_core.Scene``); this class adds the
ergonomic keyword-argument API and notebook display. Every styling method
returns ``self`` so calls chain.
"""

from __future__ import annotations

import json
from typing import Any

from . import _core
from ._viewer import DEFAULT_HEIGHT, render_html


class Scene:
    def __init__(self, core: "_core.Scene") -> None:
        self._core = core

    # -- constructors -------------------------------------------------------
    @classmethod
    def from_rcsb(cls, pdb_id: str, pdb_text: str) -> "Scene":
        return cls(_core.Scene.from_rcsb(pdb_id, pdb_text))

    @classmethod
    def from_inline_pdb(cls, pdb_text: str) -> "Scene":
        return cls(_core.Scene.from_inline_pdb(pdb_text))

    # -- representations ----------------------------------------------------
    def _add(self, kind: str, selection: Any, color, opacity, style: dict) -> "Scene":
        if color is not None:
            style["color"] = color
        if opacity is not None:
            style["opacity"] = opacity
        self._core.representation(kind, str(selection), json.dumps(style) if style else "")
        return self

    def cartoon(self, selection: Any = "protein", *, color=None, opacity=None,
                **style: Any) -> "Scene":
        return self._add("cartoon", selection, color, opacity, style)

    def surface(self, selection: Any = "protein", *, color=None, opacity=None,
                **style: Any) -> "Scene":
        return self._add("surface", selection, color, opacity, style)

    def sticks(self, selection: Any = "ligand", *, color=None, opacity=None,
               **style: Any) -> "Scene":
        return self._add("sticks", selection, color, opacity, style)

    def spheres(self, selection: Any = "all", *, color=None, opacity=None,
                **style: Any) -> "Scene":
        return self._add("spheres", selection, color, opacity, style)

    def center(self, selection: Any = None) -> "Scene":
        if selection is not None:
            self._core.set_center(str(selection))
        return self

    # -- serialization ------------------------------------------------------
    def to_json(self) -> str:
        """The declarative scene spec (for inspection/reproducibility)."""
        return self._core.to_json()

    def to_dict(self) -> dict:
        return json.loads(self._core.to_json())

    def to_geometry(self) -> dict:
        """The compiled geometry spec (instanced draw list) the renderer draws."""
        return json.loads(self._core.to_geometry_json())

    # -- display ------------------------------------------------------------
    def _repr_html_(self) -> str:
        return render_html(self.to_geometry())

    def show(self, *, height: int = DEFAULT_HEIGHT, width: str = "100%"):
        from IPython.display import HTML

        return HTML(render_html(self.to_geometry(), height=height, width=width))

    def export_html(self, path: str, *, height: int = DEFAULT_HEIGHT,
                    width: str = "100%") -> str:
        markup = render_html(self.to_geometry(), height=height, width=width)
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(markup)
        return path

    def __repr__(self) -> str:
        spec = self.to_dict()
        n = len(spec.get("representations", []))
        return f"<molscene.Scene: {n} representation(s)>"
