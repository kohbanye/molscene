# molscene

Python-first, notebook-native molecular scene builder. A Pythonic reimagining of
PyMOL's convenience — designed to be easy for AI to generate and for humans to
tweak, with no native app required.

```python
import molscene as ms

scene = (
    ms.load("1ubq")
    .cartoon("protein", color="spectrum")
    .surface("protein", opacity=0.25)
    .sticks("ligand", color="element")
)
scene.show()   # interactive 3D in Jupyter / Colab / VS Code
```

## Architecture

```
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

## License

Dual-licensed under MIT OR Apache-2.0.
