"""Notebook display: render a geometry spec in the browser with the molscene
wgpu renderer compiled to WebAssembly.

We return an ``<iframe srcdoc="...">`` — the only approach that renders reliably
across Jupyter Notebook 7, JupyterLab, Colab, and VS Code (own document context,
no script-injection sandboxing, no requirejs clashes, no global collisions).

The WASM bundle (the molscene Rust core + the wgpu renderer, built with
``wasm-pack --target no-modules``) and the geometry are inlined, so the output is
fully self-contained and works offline — no CDN. The same ``GeometrySpec`` wire
format and the same Rust renderer power ``Scene.to_png`` natively. Rendering uses
**WebGPU**; a browser without it shows a short message instead.
"""

from __future__ import annotations

import base64
import html
import json
from importlib import resources

DEFAULT_HEIGHT = 480

# Where `wasm-pack --target no-modules` writes the bundle (see web/build.sh).
_WASM_DIR = "_static/wasm"
_GLUE_JS = "molscene_wasm.js"
_WASM_BIN = "molscene_wasm_bg.wasm"


def _load_bundle() -> tuple[str, bytes] | None:
    """Read the no-modules JS glue and the wasm binary, or ``None`` if not built."""
    try:
        root = resources.files("molscene").joinpath(_WASM_DIR)
        glue = root.joinpath(_GLUE_JS).read_text(encoding="utf-8")
        wasm = root.joinpath(_WASM_BIN).read_bytes()
        return glue, wasm
    except (FileNotFoundError, ModuleNotFoundError, AttributeError, OSError):
        return None


# Bootstrap appended after the wasm-bindgen glue: initialize the module from the
# inlined bytes, create a WebGPU renderer on the canvas, load the geometry, and
# wire up drag-to-orbit / wheel-to-zoom. `wasm_bindgen` is the global the
# no-modules glue defines.
_BOOTSTRAP = """
(async function () {
  const msg = document.getElementById('msg');
  const canvas = document.getElementById('viewport');
  if (!('gpu' in navigator)) {
    msg.textContent =
      'This browser has no WebGPU support, so molscene cannot draw here. ' +
      'Use Scene.save_png(...) for an image, or open a recent Chrome/Edge.';
    msg.style.display = 'block';
    return;
  }
  function b64ToBytes(b64) {
    const bin = atob(b64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    return bytes;
  }
  let renderer;
  try {
    await wasm_bindgen(b64ToBytes(MOLSCENE_WASM_B64));
    renderer = await wasm_bindgen.Renderer.create(canvas);
  } catch (e) {
    msg.textContent = 'WebGPU init failed: ' + e;
    msg.style.display = 'block';
    return;
  }
  const geom = document.getElementById('molscene-geometry').textContent;
  renderer.loadSpecJson(geom);
  const cam = { yaw: 0, pitch: 0, zoom: 1 };
  function sizeAndDraw() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const w = Math.max(1, Math.floor(canvas.clientWidth * dpr));
    const h = Math.max(1, Math.floor(canvas.clientHeight * dpr));
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w; canvas.height = h; renderer.resize(w, h);
    }
    renderer.draw(cam.yaw, cam.pitch, cam.zoom);
  }
  let dragging = false, lastX = 0, lastY = 0;
  canvas.addEventListener('pointerdown', (e) => {
    dragging = true;
    lastX = e.clientX;
    lastY = e.clientY;
    canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener('pointermove', (e) => {
    if (!dragging) return;
    cam.yaw -= (e.clientX - lastX) * 0.01;
    const lim = Math.PI / 2 - 0.01;
    cam.pitch = Math.max(-lim, Math.min(lim, cam.pitch + (e.clientY - lastY) * 0.01));
    lastX = e.clientX; lastY = e.clientY;
    renderer.draw(cam.yaw, cam.pitch, cam.zoom);
  });
  const end = () => { dragging = false; };
  canvas.addEventListener('pointerup', end);
  canvas.addEventListener('pointercancel', end);
  canvas.addEventListener('wheel', (e) => {
    e.preventDefault();
    cam.zoom = Math.max(0.1, Math.min(10, cam.zoom * Math.exp(-e.deltaY * 0.001)));
    renderer.draw(cam.yaw, cam.pitch, cam.zoom);
  }, { passive: false });
  window.addEventListener('resize', sizeAndDraw);
  sizeAndDraw();
})();
"""

# Shown when the wasm bundle hasn't been built into the package.
_MISSING_BUNDLE = (
    "<div style='font:13px sans-serif;padding:12px;color:#555'>"
    "molscene viewer bundle not built — run <code>./web/build.sh</code> "
    "(or <code>wasm-pack build crates/molscene-wasm --target no-modules "
    "--out-dir ../../python/molscene/_static/wasm</code>) and reinstall."
    "</div>"
)


def _srcdoc(geometry: dict) -> str:
    """The self-contained HTML document loaded into the iframe."""
    bundle = _load_bundle()
    geometry_json = json.dumps(geometry)
    if bundle is None:
        return (
            "<!doctype html><html><head><meta charset='utf-8'></head><body>"
            f"{_MISSING_BUNDLE}</body></html>"
        )
    glue, wasm = bundle
    wasm_b64 = base64.b64encode(wasm).decode("ascii")
    return (
        "<!doctype html><html><head><meta charset='utf-8'>"
        "<style>html,body{margin:0;height:100%;overflow:hidden;}"
        "#viewport{display:block;width:100%;height:100vh;cursor:grab;touch-action:none;}"
        "#viewport:active{cursor:grabbing;}"
        "#msg{display:none;position:absolute;inset:0;padding:16px;font:13px sans-serif;"
        "color:#555;}</style></head><body>"
        "<canvas id='viewport'></canvas>"
        "<div id='msg'></div>"
        "<script type='application/json' id='molscene-geometry'>"
        f"{geometry_json}</script>"
        f"<script>{glue}</script>"
        f"<script>const MOLSCENE_WASM_B64='{wasm_b64}';{_BOOTSTRAP}</script>"
        "</body></html>"
    )


def render_html(
    geometry: dict, *, height: int = DEFAULT_HEIGHT, width: str = "100%"
) -> str:
    """Return an iframe (escaped srcdoc) that renders the geometry with WebGPU."""
    srcdoc = _srcdoc(geometry)
    escaped = html.escape(srcdoc, quote=True)
    style = f"border:0;width:{width};height:{height}px;"
    return (
        f'<iframe srcdoc="{escaped}" width="{width}" height="{height}" '
        f'frameborder="0" style="{style}"></iframe>'
    )
