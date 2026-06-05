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
- `crates/molscene-wasm` — wasm-bindgen bindings (stub today; the pure-web path).
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

> **In transition** — see ROADMAP "Architecture shift — structured model". The
> serialized `Scene` spec and the textual selection parser are being removed in
> favor of a code-as-source-of-truth model and typed `Expr` selections. The
> invariants below describe that target; some code (string selections, `to_json`)
> still reflects the previous model until the refactor lands.

- **One serialized contract: `GeometrySpec`.** The in-memory `Scene` (built via the
  fluent API) compiles to a low-level renderer-neutral `GeometrySpec` (`to_geometry`).
  Only the `GeometrySpec` is serialized; the `Scene` itself has no wire format — the
  building code is the source of truth. Add rendering features by extending the
  `GeometrySpec`, not by teaching the renderer about molecules.
- **Geometry is lazy.** `.sticks()` etc. only record intent; geometry is computed once
  at display time in `to_geometry`.
- **WASM-safe core.** `geometry` / `color` / `selection` / `structure` are pure compute
  and compile to WASM; only `parse` (pdbtbx, uses rayon/`std::fs`) is gated behind the
  `parse` feature and excluded from `molscene-wasm`. Keep new processing code in this
  pure set unless it genuinely needs parsing.
- **Network only in Python `load`.** RCSB fetch happens there; nothing else touches the
  network.

### Current limitations to keep in mind

`cartoon`/`surface` are recorded but not drawn (skipped with a warning). Bond inference
is O(n²) (the selection k-d tree only covers spatial queries, not bond inference). These
are tracked as milestones in `ROADMAP.md`.

Selections are typed `Expr` values, not strings: `selection.rs` holds the `Expr` tree
(boolean `and`/`or`/`not`, spatial `around`/`within`/`expand`/`beyond` via a `kiddo`
k-d tree, aggregation `byres`/`bychain`/`bymol`, numeric `b`/`q`) and its evaluator.
They are built and composed through the API (`ms.select` + `& | ~`), which constructs
`Expr` nodes directly — there is no string parser, so an `Expr` is valid by
construction. (Until the refactor lands, `ms.select` still builds a string the core
parses; see the transition note above.)

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
