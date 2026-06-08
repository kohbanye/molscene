# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What molscene is

A molecular visualization library built for AI-driven research. **molscene owns all
molecular processing in Rust** (parse, selection, neighbor search, geometry
generation, color) and treats the renderer as a dumb 3D canvas that knows nothing
about molecules. Scenes are declarative — you describe what to show; molscene
compiles it. See `README.md` for the vision and `ROADMAP.md` for direction.

## Layout & data flow

Three Rust crates + a Python facade + a TypeScript viewer:

- `crates/molscene-core` — the engine. Renderer- and binding-agnostic; **must never
  depend on PyO3 or wasm-bindgen**. Modules: `structure` (atom model + radii + bond
  inference), `parse` (pdbtbx adapter, behind the `parse` feature), `selection`
  (typed `Expr` → atom indices), `color` (palettes + scheme resolution), `scene`
  (in-memory `Scene` model — built fluently, not serialized), `geometry` (compiles
  a `Scene` into a `GeometrySpec` draw list).
- `crates/molscene-py` — thin PyO3 bindings → the `molscene._core` extension module.
- `crates/molscene-wasm` — wasm-bindgen bindings → the browser scene engine
  (mirrors the PyO3 `Scene`/`Selection`; `and`/`or`/`not` methods replace `& | ~`).
  Drives the `web/` demo: JS builds a `Scene`, compiles `to_geometry` in WASM, and
  the existing `viewer/` bundle renders the resulting `GeometrySpec` — zero Python.
- `python/molscene` — fluent facade (`load`, `Scene`, `ms.select` DSL, notebook display).
- `viewer/` — Three.js adapter that draws a `GeometrySpec` (instanced meshes).

The pipeline from `ms.load()` to pixels:

```
ms.load(id)           # __init__.py: fetch RCSB / read file → PDB text
  → _core.Scene.from_pdb(text, source)   # parse in Rust → Scene holds a Structure
.cartoon()/.sticks()/... # scene.py: record representations (no geometry yet)
scene.show()/_repr_html_ # scene.py → _core.to_geometry_json()
  → Scene::to_geometry() # geometry.rs: eval selections, infer bonds, tessellate,
                         #   resolve colors → GeometrySpec (spheres+cylinders+camera)
  → _viewer.render_html  # embed GeometrySpec + bundled viewer.js in an <iframe srcdoc>
  → molscene.renderGeometry(el, spec)  # threejsRenderer.ts: InstancedMesh + draw
```

### Architectural invariants (read before changing things)

- **One serialized contract: `GeometrySpec`.** The in-memory `Scene` (built via the
  fluent API) compiles to a low-level renderer-neutral `GeometrySpec` (`to_geometry`).
  Only the `GeometrySpec` is serialized; the `Scene` itself has no wire format — the
  building code is the source of truth. Add rendering features by extending the
  `GeometrySpec`, not by teaching the renderer about molecules.
- **Geometry is lazy.** `.sticks()` etc. only record intent; geometry is computed once
  at display time in `to_geometry`.
- **WASM-safe core.** `geometry` / `color` / `selection` / `structure` are pure compute
  and compile to WASM. The whole `parse` path compiles to WASM too: `molscene-core`
  pulls pdbtbx with `default-features = false` (dropping its optional `rayon` — the one
  thread-dependent blocker — plus `rstar`/`serde`, all unused; `compression`/miniz_oxide
  is kept and is WASM-safe), and we only call `ReadOptions::read_raw` (in-memory, no
  `std::fs`). `kiddo` is WASM-safe because it gates its native `generator` dep to
  x86_64/aarch64. Don't reintroduce a hard dependency on rayon or the rayon `par_iter`
  pdbtbx methods, or call pdbtbx's file-path `read` — that would break the wasm build
  (the CI `wasm` smoke job guards this).
- **Network only in Python `load`.** RCSB fetch happens there; nothing else touches the
  network.

### Current limitations to keep in mind

`cartoon` and `surface` are drawn natively now (surface is a grid-based approximate SES
in `surface.rs`; `GeometrySpec.meshes` is a list of `Mesh` groups, each with its own
`opacity` for depth-sorted transparency). Transparency covers mesh groups only —
instanced spheres/sticks are still opaque. Bond inference uses a uniform cell grid
(`structure.rs`: `infer_bonds_grid`) — O(n) at realistic densities — separate from the
selection k-d tree, which only covers spatial queries. These are tracked as milestones
in `ROADMAP.md`.

Selections are typed `Expr` values, not strings: `selection.rs` holds the `Expr` tree
(boolean `and`/`or`/`not`, spatial `around`/`within`/`expand`/`beyond` via a `kiddo`
k-d tree, aggregation `byres`/`bychain`/`bymol`, numeric `b`/`q`) and its evaluator.
They are built and composed through the API (`ms.select` + `& | ~`), which constructs
`Expr` nodes directly — there is no string parser, so an `Expr` is valid by
construction.

## Commands

Source the Rust toolchain first: `source "$HOME/.cargo/env"`. Python work uses the
project venv: `source .venv/bin/activate`.

```sh
# Rust core — the main TDD surface (no Python/libpython needed)
cargo test -p molscene-core
cargo test -p molscene-core selection          # one module
cargo test -p molscene-core geometry::tests::geometry_snapshot   # one test
cargo test --workspace                          # all crates
cargo fmt --all && cargo clippy -p molscene-core -- -D warnings

# insta snapshots (geometry JSON): regenerate, then review the .snap diff
INSTA_UPDATE=always cargo test -p molscene-core

# Viewer (TypeScript)
cd viewer && npm install && npm test            # vitest (pure geometry math)
npm run typecheck
npm run build                                   # bundles Three.js → python/molscene/_static/viewer.js

# Python facade — build the extension into the venv, then test
maturin develop                                 # build molscene._core + install editable
pytest -m "not network"                         # unit tests (offline, use fixtures)
pytest -m network                               # exercises the RCSB fetch path
pytest tests/test_selection.py::test_and_operator

# Type stubs (python/molscene/_core/__init__.pyi) are pyo3-stub-gen output, not
# hand-written. After changing the PyO3 API surface, regenerate + commit them
# (CI fails on drift). Run from the repo root:
cargo run -p molscene-py --features stub-gen --bin stub_gen

# WASM / pure-web (browser scene engine + demo)
rustup target add wasm32-unknown-unknown        # one-time
cargo build -p molscene-wasm --target wasm32-unknown-unknown   # quick compile check
./web/build.sh                                  # viewer bundle + wasm-pack → web/pkg
python3 -m http.server 8000                     # open http://localhost:8000/web/index.html
```

### Gotchas

- `cargo test --workspace` works because `molscene-py` sets `test = false` (its
  `extension-module` feature can't link libpython during a plain `cargo test`). Put all
  testable logic in `molscene-core`.
- After changing Rust exposed to Python, re-run `maturin develop`. After changing
  `viewer/src`, re-run `npm run build` — the bundle is gitignored and the wheel
  force-includes it via `[tool.maturin].include`.
- Tests must stay offline: use `tests/fixtures/*.pdb`; mark anything hitting RCSB with
  `@pytest.mark.network`. Bond-count tests need deterministic geometry (see
  `tests/fixtures/triatomic.pdb`).

## Conventions

- Dual-licensed MIT OR Apache-2.0; PyMOL-derived constants (VDW/covalent radii, CPK and
  spectrum colors) come from the reference clone at `/tmp/pymol-ref` and are cited in
  comments.
- TDD across all three layers; keep `cargo test` / `vitest` / `pytest` green. Commit in
  focused, logical units.
