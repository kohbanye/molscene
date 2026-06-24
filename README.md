# molscene

molscene is a Rust-based molecular visualization library for AI-driven research. Scenes are written
declaratively, a short block of code an AI can generate. No native app
required.

```python
import molscene as ms

scene = (
    ms.load("1ubq")
    .sticks(ms.select.protein(), color="spectrum")
    .spheres(ms.select.hetero(), color="element")
)
scene.show()   # interactive 3D in Jupyter / Colab
scene.save_png("ubq.png")   # or render to a PNG natively in Rust — no browser
```

Representations pick sensible defaults: `cartoon()`/`surface()` show the protein,
`sticks()` the ligand, and `spheres()` everything *except* solvent — crystal waters
and buffer ions aren't dumped into the view by accident. Pass an explicit selection
(`ms.select.all()`, `ms.select.water()`, `ms.select.solvent()`) to show them.

## Architecture

```text
Python fluent API ─┐
                   ├─ molscene-py (PyO3)      ─┐
browser / JS API  ─┤                          ├─ molscene-core (pure Rust)
                   └─ molscene-wasm            ┘   Structure / Scene / Selection / geometry
                                                        │ to_geometry()
                                                  GeometrySpec  (the one wire format)
                                                        ↓
                                          molscene-render (wgpu, one renderer)
                                       ┌──────────────┼───────────────────────┐
                                  native PNG      browser canvas        notebook iframe
                                 .to_png()/        (web/ demo,           (show()/_repr_html_,
                                 .save_png()       WebGPU)               WASM + WebGPU)
```

molscene owns all molecular processing (parse, selection, **geometry generation**,
color) in Rust, and renders the resulting `GeometrySpec` with a single wgpu
renderer — natively to a PNG and, compiled to WebAssembly, straight to a browser
canvas (WebGPU). The renderer knows nothing about molecules; it only draws the
`GeometrySpec`.

- **`crates/molscene-core`** — renderer- and binding-agnostic engine (Rust): structure,
  selection, color, and geometry compilation to a `GeometrySpec`.
- **`crates/molscene-render`** — the wgpu rasterizer (impostor spheres/bonds + meshes);
  one renderer for native PNG and, compiled to wasm, the browser canvas.
- **`crates/molscene-py`** — PyO3 bindings → the `molscene._core` extension module.
- **`crates/molscene-wasm`** — wasm-bindgen bindings: the `Scene`/`Selection` API plus a
  WebGPU `Renderer` that draws the `GeometrySpec` to a canvas.
- **`python/molscene`** — thin fluent facade, notebook display.

## Pure-web / WASM

The same Rust core **and renderer** run in the browser — zero Python.
`molscene-wasm` exposes the identical `Scene` / `Selection` API via wasm-bindgen,
builds the scene and compiles `to_geometry` entirely in WebAssembly, and draws the
resulting `GeometrySpec` (the only wire format, byte-for-byte the same one the
notebook and native PNG paths use) with the wgpu `Renderer` straight to a `<canvas>`
via WebGPU — drag to orbit, scroll to zoom, and download a PNG. PDB / mmCIF / SDF all
parse in the browser. See [`web/README.md`](web/README.md):

```sh
./web/build.sh                              # wasm-pack (core + wgpu Renderer) → web/pkg
python3 -m http.server 8000 --directory web # then open http://localhost:8000/ (needs WebGPU)
```

A hosted build is published to GitHub Pages (`https://kohbanye.github.io/molscene/`).

## Development

```sh
# Rust core (the main TDD surface — no Python needed)
cargo test -p molscene-core

# GPU renderer (skips when no GPU/software-Vulkan adapter is available)
cargo test -p molscene-render

# Python facade (after building the extension into a venv)
maturin develop && pytest -m "not network"
```

## Roadmap

See [ROADMAP.md](ROADMAP.md). Today molscene renders **spheres**, **sticks**,
**cartoon**, and **surface** natively, evaluates a **typed, composable selection
API** (boolean, spatial, aggregation, numeric) in Rust, and runs the same core in
the browser via **WebAssembly** (a pure-web path, zero Python).

## License

Dual-licensed under MIT OR Apache-2.0.
