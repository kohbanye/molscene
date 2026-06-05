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
  local file → parsed in Rust), `ms.select` DSL with `& | ~`, fluent API,
  `_repr_html_` / `show` / `export_html` via iframe srcdoc.
- **Viewer** (`viewer/`): Three.js renderer — instanced spheres + cylinders,
  bounding-sphere camera, OrbitControls; bundled offline (no CDN).
- **Selections** (v0.2): a fully-evaluated selection language in Rust — boolean
  `and`/`or`/`not`, spatial `around`/`within`/`expand`/`beyond` (kiddo k-d tree),
  aggregation `byres`/`bychain`/`bymol`, numeric `b`/`q`, parsed from a string and
  validated (invalid selections raise `ValueError`).
- **Coloring** (v0.3): property colormaps (`bfactor`/`occupancy` → viridis/plasma/
  rdylgn, auto-ranged), color-by-element-keeping-carbon (`element:cyan`), and
  explicit per-selection overrides via `scene.set_color(selection, color)`.

### Current limitations (these drive the milestones below)

- **cartoon / surface** are recorded but **not drawn** yet.
- Bond inference is **O(n²)**; large structures will be slow.
- Lighting is flat; single colors read muddy.
- No benchmarks yet → no "fast" claims yet.

---

## v0.2 — Real selections ✅ (shipped)

Selection is a first-class, fully-evaluated language in Rust.

- Selection **expression tree** in `selection.rs` (tokenizer → recursive-descent
  parser → `Expr` AST → evaluator, replacing the string fallback): boolean
  `and`/`or`/`not`, grouping.
- **Spatial** operators: `around` / `within` / `expand` / `beyond`, backed by a
  `kiddo` k-d tree (behind a `neighbors_within` seam).
- **Aggregation**: `byres` / `bychain` / `bymol` (`bymol` via union-find over
  inferred bonds).
- `ms.select` builds the string; the `& | ~` operators compose it, and the core
  parses + evaluates it. Invalid selections raise `ValueError`.
- Numeric predicates: `b` / `q` comparisons (`b_factor`/`occupancy` now stored on
  `Atom`).

**Deliverable:** `ms.select.chain("A") & ms.select.around(ms.select.ligand(), 5) & ~ms.select.water()`
evaluates to the right atoms and renders.

## v0.3 — Coloring ✅ (shipped)

Per-instance color already existed in `GeometrySpec`; v0.3 exposes richer ways to
drive it. The `color` string stays the canonical, hand-editable form; the grammar
is `<base>[:<modifier>]` and `ColorScheme::parse` remains the single source of truth.

- **Color by property**: `color="bfactor"` / `"occupancy"` (aliases `b`/`q`) maps
  the per-atom field — already stored on `Atom` since v0.2 — through a colormap,
  auto-ranged over the colored atoms. Colormaps: `viridis` (default), `plasma`,
  `rdylgn`, picked via the modifier (`bfactor:plasma`).
- **Explicit / per-selection color**: `scene.set_color(selection, color)` overrides
  a sub-selection on top of the representations' schemes (PyMOL-style, last write
  wins) without stacking reps. Stored as a `colors` list on the scene spec.
- **Color-by-element keeping carbon**: `color="element:cyan"` — CPK with carbons in
  the chosen color.

**Deliverable:** `ms.load("1ubq").spheres("protein", color="bfactor")` and explicit
per-selection colors (`.set_color("resi 50", "red")`) evaluate and render.

## v0.4 — Cartoon (the hero representation)

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

## v0.5 — Surface

- Molecular **surface mesh** (SES / Gaussian) via a density grid +
  marching cubes / surface-nets (Rust crate, e.g. `fast-surface-nets`).
- **Transparency** in the renderer (depth-sorted / OIT-lite) for `opacity`.
- Reuse the `meshes` channel from v0.4.

**Deliverable:** `surface("protein", opacity=0.3)` over a cartoon.

## v0.6 — Chemistry & bond orders

Correct chemistry for ligands and small molecules. (Proteins stay single-bond
sticks, as is conventional.)

- **Bond orders**: read from mmCIF / the Chemical Component Dictionary and from
  SDF / mol2 imports; geometry-based perception (ring SSSR + heuristics) as a
  fallback. Carry order (and aromaticity) on `Structure`'s bonds.
- **Double / triple bonds**: parallel offset cylinders, oriented by a bond
  reference plane (neighbor atoms / ring plane).
- **Aromatic rings**: inner-ring depiction (or alternating doubles).
- Ligand-focused: pairs with better default handling of `organic` / `ligand`.

**Deliverable:** a ligand rendered with correct double bonds and aromatic rings.

## v0.7 — Rendering quality & performance

- **Lighting/material polish**: hemisphere + fill lights, sensible defaults,
  truer CPK colors; configurable background; antialiasing.
- **Camera**: better initial framing, fit modes, `center`/`orient`.
- **Performance**: replace O(n²) bonds with a **cell grid**; handle large
  structures (10⁵+ atoms) smoothly; instancing budget checks.
- **Benchmarks** (parse + geometry timings) → then "fast" is earned and goes in
  the README with numbers.
- Cheap reps: `lines`, `dots`, `labels`.
- Water/solvent sensible defaults (don't dump crystal waters by accident).

## v0.8 — WASM / pure-web

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
