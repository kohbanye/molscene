# molscene roadmap

A living plan for where molscene is going. Versions are milestones, not promises
about dates. See `README.md` for the vision and `CLAUDE`/source for current code.

## Principles

These constrain every milestone below:

- **Rust owns the molecule.** Parsing, selection, neighbor search, geometry
  generation, and color resolution live in `molscene-core`. The renderer is a
  dumb 3D canvas (Three.js today) that knows nothing about molecules.
- **Scenes are declarative.** You describe *what* to show; molscene compiles it.
  No imperative `cmd.show(...)` clone — state lives in an in-memory `Scene` built
  through a typed, composable API. The *building code* is the source of truth;
  there is no serialized scene format to hand-edit or round-trip.
- **Structured selections.** Selections are typed `Expr` values, built and
  composed through the API (`ms.select` + `& | ~`) — not a query string to parse.
  An `Expr` is valid by construction; there is no textual selection language.
- **One serialized contract.** The `Scene` compiles to a low-level,
  renderer-neutral `GeometrySpec` (instanced primitives + meshes). The
  `GeometrySpec` is the *only* wire format — the `Scene` itself is never serialized.
- **Dual-core.** `geometry`/`color`/`selection`/`scene` are pure compute and
  WASM-safe; only `parse` (pdbtbx) is native-gated. A pure-browser/WASM product is
  a real goal: the same Rust core builds the `Scene` and emits a `GeometrySpec`
  from JS via wasm-bindgen — no Python, and still no Scene wire format.
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
- **Structured model** (architecture shift): the `Scene` is a pure in-memory model
  (no `to_json`/`to_dict`, no serde on scene types) and selections are typed `Expr`
  values built through the API — the string tokenizer/parser is gone. Only the
  compiled `GeometrySpec` is serialized.
- **Cartoon** (v0.4): native protein ribbons — Cα-geometric secondary-structure
  assignment (preferring file `HELIX`/`SHEET` annotations), a Catmull-Rom backbone
  spline extruded into helix ribbons, β-strand arrows, and loop tubes (a `meshes`
  channel on `GeometrySpec`), with `color="secondary_structure"`/`"spectrum"`.
- **Surface** (v0.5): native molecular surface — a fast grid-based approximate
  SES (accessible-solid rasterization + Felzenszwalb–Huttenlocher distance
  transform + `fast-surface-nets`), nearest-atom vertex coloring, and depth-sorted
  **mesh-group transparency** (`meshes` is a list of groups each with an `opacity`).
- **Chemistry & bond orders** (v0.6): bonds carry an order (single/double/triple/
  aromatic) — read explicitly from **SDF / V2000 molfile** imports
  (`ms.load("lig.sdf")`) or perceived from geometry for distance-only sources
  (`chem.rs`: SSSR ring finding + planarity-based aromatic detection + bond-length
  heuristics). Sticks render double/triple bonds as **parallel offset cylinders**
  (oriented by ring centroid or a neighbor-atom plane) and aromatic rings with the
  **inner-ring depiction**.

### Current limitations (these drive the milestones below)

- Transparency covers **mesh groups only** (surface / cartoon); instanced spheres
  and sticks are still opaque (per-instance alpha is a follow-up).
- Bond inference is **O(n²)**; large structures will be slow.
- Lighting is flat; single colors read muddy.
- No benchmarks yet → no "fast" claims yet.

---

## Architecture shift — structured model ✅ (shipped)

A foundational refactor that landed before the feature milestones. It removed
two pieces of the original design that turned out to be the wrong defaults:

1. **The serialized `Scene` spec.** The JSON scene spec was meant to be the
   hand-editable / cross-frontend contract, but in practice only the compiled
   `GeometrySpec` ever reaches a renderer, and the building code is what people
   actually read and edit. So the `Scene` becomes a pure in-memory model: **no
   `to_json` / `to_dict`, no serde on the scene types**. The fluent API call site
   is the source of truth; only the `GeometrySpec` is serialized (it still has to
   cross to the JS viewer).
2. **The textual selection language.** Selections become typed `Expr` values,
   built and composed through the API. The tokenizer + recursive-descent parser
   in `selection.rs` is **deleted**; an `Expr` is valid by construction, so a
   selection can no longer fail to parse.

Color values stay strings (`"bfactor"`, `"element:cyan"`) — that grammar is small
and unaffected; only *selections* and the *Scene serialization* change.

### Plan (by layer)

- **core / `selection.rs`** — keep `Expr` + the evaluator; delete the
  tokenizer/parser. Add `Expr` constructors/combinators (`Expr::chain`,
  `Expr::resi`, `.and`/`.or`/`.not`, `around`/`byres`/…). Keep a `Display` impl
  for debugging/error messages only (not a canonical form).
- **core / `scene.rs` + `spec.rs`** — drop `Serialize`/`Deserialize` and
  `to_json` / `to_json_pretty` / `to_value`. `Representation.selection`,
  `ColorAssignment.selection`, and `Camera.center` change from `String` to `Expr`.
  Replace the `serde_json::Map` `Style` with a typed struct (color stays a string
  parsed to `ColorScheme`; opacity/scale/radius typed). Drop `spec_version` and
  the "wire format" framing.
- **core / `geometry.rs`** — `evaluate(structure, &Expr)` instead of `&str`; no
  other change (`GeometrySpec` keeps its serde — it is the wire format).
- **bindings / `molscene-py`** — `Selection` wraps a Rust `Expr` (not a `String`);
  the `ms.select` builders and `& | ~` construct `Expr` in Rust. Remove
  `validate_selection` and `Scene::to_json`; keep `to_geometry_json`.
- **facade / `python/molscene`** — `ms.select.*` build `Expr`-backed `Selection`s;
  remove `_wrap` / string parsing. Representations and `set_color` take a
  `Selection` (no bare strings). Remove `Scene.to_json` / `to_dict`; keep
  `to_geometry`. Default selections (`"protein"`, `"all"`, …) become
  `ms.select.protein()` etc.
- **tests / examples** — remove the scene-JSON snapshot and parser tests; add
  `Expr`-builder + evaluator tests. Migrate the notebook / README / tests off
  string selections. The geometry snapshot stays.

This is a breaking API change (string selections and `to_json` / `to_dict` go
away); it predates the 1.0 surface freeze, so we take it now.

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

**Deliverable:** `ms.load("1ubq").spheres(ms.select.protein(), color="bfactor")` and explicit
per-selection colors (`.set_color(ms.select.resi(50), "red")`) evaluate and render.

## v0.4 — Cartoon (the hero representation) ✅ (shipped)

The iconic protein view, generated natively.

- **Secondary-structure assignment** in Rust: a Cα-only geometric heuristic
  (P-SEA-style virtual angle/torsion + Cα(i±n) distances, smoothed), preferring
  file `HELIX`/`SHEET` annotations when present (parsed from PDB; mmCIF falls back
  to the heuristic).
- **Backbone spline** (Catmull-Rom through Cα) + profile extrusion: helix ribbon,
  β-strand arrow, loop tube as a single morphing elliptical cross-section →
  triangle mesh with smooth vertex normals and end caps.
- Extended `GeometrySpec` with a **`meshes`** channel
  (positions / normals / indices / per-vertex colors).
- Three.js renderer draws meshes via `BufferGeometry` (per-vertex colors).
- `color="secondary_structure"` (helix/sheet/loop palette) and `color="spectrum"`
  along the chain.

**Deliverable:** `ms.load("1ubq").cartoon(ms.select.protein(), color="spectrum")`
renders a real cartoon.

Future polish (deferred): smoother profile transitions at SS boundaries, mmCIF
`HELIX`/`SHEET` annotation parsing, and tunable cartoon dimensions.

## v0.5 — Surface ✅ (shipped)

- Molecular **surface mesh**: a fast grid-based **approximate SES**. Rasterize the
  solvent-accessible solid onto a voxel grid, erode it by the probe radius via a
  separable Felzenszwalb–Huttenlocher Euclidean distance transform (this is what
  yields the concave re-entrant patches a per-sphere min-SDF cannot), and mesh the
  isosurface with `fast-surface-nets`. Grid spacing is auto-coarsened under a cell
  budget; vertices are colored by their nearest selected atom. `SurfaceParams`
  stays a swappable seam for an analytic / Gaussian backend later.
- **Mesh-group transparency** in the renderer (depth-sorted / OIT-lite): the
  `meshes` channel is now a list of groups, each carrying its own `opacity`, drawn
  with a transparent material (and `depthWrite` off) when `opacity < 1`. Surface
  and cartoon each emit one group, so a translucent surface sits over an opaque
  cartoon.
- Reuses the `meshes` channel from v0.4.

**Deliverable:** `surface("protein", opacity=0.3)` over a cartoon.

Follow-up (deferred to a later PR): **per-representation opacity for spheres /
sticks** via per-instance alpha (an instanced opacity attribute on the Three.js
`InstancedMesh`). v0.5 ships transparency for **mesh groups only** (surface /
cartoon); instanced primitives stay opaque.

## v0.6 — Chemistry & bond orders ✅ (shipped)

Correct chemistry for ligands and small molecules. (Proteins stay single-bond
sticks, as is conventional.)

- **Bond orders**: read explicitly from **SDF / V2000 molfile** imports (a
  hand-written parser, behind the `parse` feature, tagging atoms as a ligand
  residue); geometry-based perception as the fallback for distance-only sources
  (PDB/mmCIF). Order (single/double/triple/aromatic) is carried on `Structure`'s
  bonds as a `Bond { a, b, order }`. _(mmCIF / Chemical Component Dictionary and
  mol2 imports remain future work — pdbtbx surfaces no orders.)_
- **Perception** (`chem.rs`, pure-compute / WASM-safe): smallest-set-of-smallest-
  rings via per-edge BFS, aromatic-ring detection (size 5/6, aromatic elements,
  Newell-normal planarity), and bond-length heuristics for double/triple bonds —
  documented as a best-effort visual heuristic, never overriding explicit orders.
- **Double / triple bonds**: parallel offset cylinders, oriented by a bond
  reference plane (ring centroid, else a neighbor atom).
- **Aromatic rings**: inner-ring depiction (a full bond plus a shorter cylinder
  offset toward the ring centroid).

**Deliverable:** `ms.load("ligand.sdf").sticks()` renders correct double bonds
and aromatic rings (inner ring). ✅

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

- Flesh out `molscene-wasm` (wasm-bindgen): build a `Scene` through the same typed
  API and run `to_geometry` entirely in the browser, no Python.
- A **web demo page** consuming the WASM core + the existing `viewer/` renderer.
- Same `GeometrySpec` contract drives both Python-notebook and pure-web paths — and
  it remains the *only* serialized format on either path.

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
- **Typed `Element`**: replace `Atom.element: String` with a type-safe `Element`
  enum (no repeated trim/uppercase normalization, no `String` per atom, exhaustive
  `match`). A cross-cutting refactor touching `structure`/`parse`/`color`/`radii`/
  `selection`, so it lands on its own after the v0.6 surface settles.
- **Rigorous chemistry model**: carry **valence**, **formal charge**, and
  **(implicit/explicit) hydrogen counts** on atoms, with real perception/
  sanitization (valence checks, charge balancing, aromaticity by electron count
  rather than the v0.6 geometry heuristics). This is a proper cheminformatics
  layer — likely its **own crate** (e.g. `molscene-chem`) that `molscene-core`
  depends on — so the geometry/render path stays lean while molecule handling
  gets stricter. Enables radii/H placement, protonation states, and
  bond-order perception that doesn't rely on coordinates alone.

## Non-goals (for now)

- A textual selection language. Selections are a typed, composable API, not a
  query string molscene parses — so no PyMOL-style selection-string compatibility,
  and no imperative command language.
- A GUI, measurement tools, or a plugin system.
