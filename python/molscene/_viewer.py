"""Notebook display: turn a scene spec into self-contained HTML.

We return an ``<iframe srcdoc="...">`` because that is the only approach that
renders reliably across Jupyter Notebook 7, JupyterLab, Colab, and VS Code — it
gets its own document context, avoiding script-injection sandboxing, requirejs
collisions, and global-namespace clashes between multiple viewers.
"""

from __future__ import annotations

import html
import json
from importlib import resources

# 3Dmol.js is loaded from a pinned CDN version (jsDelivr lags npm; never use
# @latest). Bump deliberately.
THREEDMOL_VERSION = "2.4.2"
_CDN_URL = f"https://cdn.jsdelivr.net/npm/3dmol@{THREEDMOL_VERSION}/build/3Dmol-min.js"

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
            "\nwindow.molscene = window.molscene || { render: function(){ "
            "console.warn('molscene viewer bundle missing'); } };"
        )


def _srcdoc(spec: dict, *, use_cdn: bool = True) -> str:
    """The HTML document loaded into the iframe."""
    spec_json = json.dumps(spec)
    viewer_js = _load_viewer_js()
    threedmol_tag = (
        f'<script src="{_CDN_URL}"></script>'
        if use_cdn
        # Offline export inlines nothing extra here; the bundle is expected to
        # carry its own renderer when use_cdn is False (handled in v0.x export).
        else f'<script src="{_CDN_URL}"></script>'
    )
    return (
        "<!doctype html><html><head><meta charset='utf-8'>"
        f"{threedmol_tag}"
        "<style>html,body{margin:0;height:100%;}#viewport{position:relative;"
        "width:100%;height:100vh;}</style></head><body>"
        "<div id='viewport'></div>"
        f"<script type='application/json' id='molscene-spec'>{spec_json}</script>"
        f"<script>{viewer_js}</script>"
        "<script>(function(){var spec=JSON.parse("
        "document.getElementById('molscene-spec').textContent);"
        "if(window.molscene&&window.molscene.render){"
        "window.molscene.render(document.getElementById('viewport'),spec);}})();"
        "</script></body></html>"
    )


def render_html(spec: dict, *, height: int = DEFAULT_HEIGHT, width: str = "100%",
                use_cdn: bool = True) -> str:
    """Return an iframe (with escaped srcdoc) that renders the scene."""
    srcdoc = _srcdoc(spec, use_cdn=use_cdn)
    escaped = html.escape(srcdoc, quote=True)
    style = f"border:0;width:{width};height:{height}px;"
    return (
        f'<iframe srcdoc="{escaped}" width="{width}" height="{height}" '
        f'frameborder="0" style="{style}"></iframe>'
    )
