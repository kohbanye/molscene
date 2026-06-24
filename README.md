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
                                              GeometrySpec (instanced spheres + cylinders)
                                                        ↓
                                  ┌─ viewer/ (Three.js)     → notebook / browser
                                  └─ molscene-render (wgpu) → PNG  (.to_png / .save_png)
                                     both consume the GeometrySpec; neither knows about molecules
```

molscene owns all molecular processing (parse, selection, **geometry generation**,
color) in Rust and treats the renderer as a dumb 3D canvas. v0.1 renders
**spheres** and **sticks**; cartoon/surface are a follow-up.

- **`crates/molscene-core`** — renderer- and binding-agnostic engine (Rust): structure,
  selection, color, and geometry compilation to a `GeometrySpec`.
- **`crates/molscene-py`** — PyO3 bindings → the `molscene._core` extension module.
- **`crates/molscene-wasm`** — wasm-bindgen bindings for the future browser product.
- **`python/molscene`** — thin fluent facade, notebook display.
- **`viewer/`** — Three.js adapter that draws a `GeometrySpec` (instanced meshes).

## Pure-web / WASM

The same Rust core runs in the browser — zero Python. `molscene-wasm` exposes the
identical `Scene` / `Selection` API via wasm-bindgen, builds the scene and compiles
`to_geometry` entirely in WebAssembly, and hands the resulting `GeometrySpec` (the
only wire format, byte-for-byte the same one the notebook path uses) to the existing
Three.js viewer. PDB / mmCIF / SDF all parse in the browser. See
[`web/README.md`](web/README.md):

```sh
./web/build.sh                              # viewer bundle + WASM core → web/
python3 -m http.server 8000 --directory web # then open http://localhost:8000/
```

A hosted build is published to GitHub Pages (`https://kohbanye.github.io/molscene/`).

## Development

```sh
# Rust core (the main TDD surface — no Python needed)
cargo test -p molscene-core

# Viewer adapter
cd viewer && npm install && npm test

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
