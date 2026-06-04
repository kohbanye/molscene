# molscene roadmap

A living plan for where molscene is going. Versions are milestones, not promises
about dates. See `README.md` for the vision and `CLAUDE`/source for current code.

## Principles

These constrain every milestone below:

- **Rust owns the molecule.** Parsing, selection, neighbor search, geometry
  generation, and color resolution live in `molscene-core`. The renderer is a
  dumb 3D canvas (Three.js today) that knows nothing about molecules.
- **Scenes are declarative.** You describe *what* to show; molscene compiles it.
  No imperative `cmd.show(...)` clone — state lives in a `Scene` an AI can
  generate and a human can edit.
- **Two specs.** A high-level declarative `Scene` (serde JSON) compiles to a
  low-level renderer-neutral `GeometrySpec` (instanced primitives + meshes).
- **Dual-core.** `geometry`/`color`/`selection` are pure compute and WASM-safe;
  only `parse` (pdbtbx) is native-gated. A pure-browser/WASM product is a real
  goal, not a someday.
- **TDD, every layer.** `cargo test` (incl. insta snapshots), `vitest`, `pytest`
  stay green; the geometry pipeline is the main test surface.

## Status (done)

- **Workspace & tooling**: 3-crate Cargo workspace, maturin packaging, CI
  (cargo/vitest/pytest), TDD across layers.
- **Core engine** (`molscene-core`): `Structure` model, pdbtbx PDB/mmCIF parser
  (`parse` feature, native-only), declarative `Scene` + JSON spec, distance-based
  bond inference, single-clause selection evaluator, color resolution
  (CPK / spectrum / chain / named / hex), `GeometrySpec` + `Scene::to_geometry`
  for **spheres** and **sticks**.
- **Python** (`molscene`): PyO3 `Scene`/`Selection`, `ms.load` (RCSB fetch or
  local file → parsed in Rust), `ms.sel` DSL with `& | ~`, fluent API,
  `_repr_html_` / `show` / `export_html` via iframe srcdoc.
- **Viewer** (`viewer/`): Three.js renderer — instanced spheres + cylinders,
  bounding-sphere camera, OrbitControls; bundled offline (no CDN).

### Current limitations (these drive the milestones below)

- Composed selections (`& | ~`, spatial) are **recorded but not evaluated** —
  they fall back to selecting all.
- **cartoon / surface** are recorded but **not drawn** yet.
- Bond inference is **O(n²)**; large structures will be slow.
- Lighting is flat; single colors read muddy.
- No benchmarks yet → no "fast" claims yet.

---

## v0.2 — Real selections

Make selection a first-class, fully-evaluated language in Rust.

- Selection **expression tree** in `selection.rs` (replace the string fallback):
  boolean `and`/`or`/`not`, grouping.
- **Spatial** operators: `around` / `within` / `expand` / `beyond`, backed by a
  neighbor search (introduce `kiddo` k-d tree).
- **Aggregation**: `byres` / `bychain` / `bymol`.
- `ms.sel` builds/serializes the tree; the `& | ~` operators become real.
- Numeric predicates: `b` / `q` comparisons.

**Deliverable:** `ms.sel.chain("A") & ms.sel.around(ms.sel.ligand(), 5) & ~ms.sel.water()`
evaluates to the right atoms and renders.

## v0.3 — Cartoon (the hero representation)

The iconic protein view, generated natively.

- **Secondary-structure assignment** in Rust (DSSP-style H-bond/geometry).
- **Backbone spline** (Catmull-Rom through Cα) + profile extrusion:
  helix ribbon, sheet arrow, loop tube → triangle mesh.
- Extend `GeometrySpec` with a **`meshes`** channel
  (positions / normals / indices / per-vertex colors).
- Three.js renderer draws meshes via `BufferGeometry`.
- `color="secondary_structure"` and `color="spectrum"` along the chain.

**Deliverable:** `ms.load("1ubq").cartoon("protein", color="spectrum")` renders a
real cartoon.

## v0.4 — Surface

- Molecular **surface mesh** (SES / Gaussian) via a density grid +
  marching cubes / surface-nets (Rust crate, e.g. `fast-surface-nets`).
- **Transparency** in the renderer (depth-sorted / OIT-lite) for `opacity`.
- Reuse the `meshes` channel from v0.3.

**Deliverable:** `surface("protein", opacity=0.3)` over a cartoon.

## v0.5 — Rendering quality & performance

- **Lighting/material polish**: hemisphere + fill lights, sensible defaults,
  truer CPK colors; configurable background; antialiasing.
- **Camera**: better initial framing, fit modes, `center`/`orient`.
- **Performance**: replace O(n²) bonds with a **cell grid**; handle large
  structures (10⁵+ atoms) smoothly; instancing budget checks.
- **Benchmarks** (parse + geometry timings) → then "fast" is earned and goes in
  the README with numbers.
- Cheap reps: `lines`, `dots`, `labels`.
- Water/solvent sensible defaults (don't dump crystal waters by accident).

## v0.6 — WASM / pure-web

Prove the dual-core thesis.

- Flesh out `molscene-wasm` (wasm-bindgen): build a `Scene` and run
  `to_geometry` entirely in the browser, no Python.
- A **web demo page** consuming the WASM core + the existing `viewer/` renderer.
- Same `GeometrySpec` contract drives both Python-notebook and pure-web paths.

**Deliverable:** a browser page where JS builds a scene and renders it — zero
Python.

## v1.0 — Distribution & ergonomics

- **PyPI wheels**: maturin CI matrix (manylinux/macOS/Windows), versioned
  releases.
- **Type stubs** via `pyo3-stub-gen` (accurate, auto-generated) for IDE + AI.
- Richer **style options** and defaults; in-place **scene editing**
  (`scene.representations[i].opacity = ...`).
- **Docs site** + a gallery of declarative recipes.

---

## Backlog / longer term

- **Trajectories** and multiple models/states; animation; image/video export.
- **Biological assemblies**, symmetry mates, multi-structure scenes & alignment.
- **Measurements**: distances / angles / dihedrals as scene objects.
- **Density / volumetric** maps (cryo-EM, electron density).
- **Alternative renderer**: a `wgpu` path for native, headless, offscreen PNG
  rendering (screenshots without a browser) — reuses the same `GeometrySpec`.
- Richer mmCIF support; entity/chain metadata.

## Non-goals (for now)

- Full PyMOL command/selection-language compatibility.
- Ray tracing / publication-grade offline rendering.
- A GUI, measurement tools, or a plugin system.
