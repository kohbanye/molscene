"""Notebook display: render a geometry spec with the bundled Three.js viewer.

We return an ``<iframe srcdoc="...">`` — the only approach that renders reliably
across Jupyter Notebook 7, JupyterLab, Colab, and VS Code (own document context,
no script-injection sandboxing, no requirejs clashes, no global collisions).

The viewer bundle (Three.js + the molscene adapter) is inlined, so the output is
fully self-contained and works offline — no CDN.
"""

from __future__ import annotations

import html
import json
from importlib import resources

DEFAULT_HEIGHT = 480


def _load_viewer_js() -> str:
    """Read the bundled viewer adapter, or a placeholder if not yet built."""
    try:
        return (
            resources.files("molscene")
            .joinpath("_static/viewer.js")
            .read_text(encoding="utf-8")
        )
    except (FileNotFoundError, ModuleNotFoundError, AttributeError):
        return (
            "/* molscene viewer bundle not built; run `cd viewer && npm run build` */"
            "\nwindow.molscene = window.molscene || { renderGeometry: function(){ "
            "console.warn('molscene viewer bundle missing'); } };"
        )


def _srcdoc(geometry: dict) -> str:
    """The self-contained HTML document loaded into the iframe."""
    geometry_json = json.dumps(geometry)
    viewer_js = _load_viewer_js()
    return (
        "<!doctype html><html><head><meta charset='utf-8'>"
        "<style>html,body{margin:0;height:100%;overflow:hidden;}"
        "#viewport{position:relative;width:100%;height:100vh;}</style></head><body>"
        "<div id='viewport'></div>"
        "<script type='application/json' id='molscene-geometry'>"
        f"{geometry_json}</script>"
        f"<script>{viewer_js}</script>"
        "<script>(function(){var g=JSON.parse("
        "document.getElementById('molscene-geometry').textContent);"
        "if(window.molscene&&window.molscene.renderGeometry){"
        "window.molscene.renderGeometry(document.getElementById('viewport'),g);}})();"
        "</script></body></html>"
    )


def render_html(
    geometry: dict, *, height: int = DEFAULT_HEIGHT, width: str = "100%"
) -> str:
    """Return an iframe (escaped srcdoc) that renders the geometry with Three.js."""
    srcdoc = _srcdoc(geometry)
    escaped = html.escape(srcdoc, quote=True)
    style = f"border:0;width:{width};height:{height}px;"
    return (
        f'<iframe srcdoc="{escaped}" width="{width}" height="{height}" '
        f'frameborder="0" style="{style}"></iframe>'
    )
