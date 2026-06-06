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
```

## Architecture

```text
Python fluent API ─┐
                   ├─ molscene-py (PyO3)      ─┐
browser / JS API  ─┤                          ├─ molscene-core (pure Rust)
                   └─ molscene-wasm            ┘   Structure / Scene / Selection / geometry
                                                        │ to_geometry()
                                              GeometrySpec (instanced spheres + cylinders)
                                                        ↓
                                              viewer/ (Three.js — knows nothing about molecules)
                                                        ↓
                                                  notebook / browser
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

See [ROADMAP.md](ROADMAP.md). Today molscene renders **spheres** and **sticks**
natively and evaluates a **typed, composable selection API** (boolean, spatial,
aggregation, numeric) in Rust; cartoon, surface, and a pure-web WASM path are next.

## License

Dual-licensed under MIT OR Apache-2.0.
