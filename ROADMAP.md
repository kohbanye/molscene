# molscene roadmap

A living plan for where molscene is going. Versions are milestones, not promises
about dates. See `README.md` for the vision and `CLAUDE`/source for current code.

## Principles

These constrain every milestone below:

- **Rust owns the molecule *and* the renderer.** Parsing, selection, neighbor
  search, geometry generation, and color resolution live in `molscene-core`;
  rendering lives in `molscene-render` (wgpu). The renderer is a dumb 3D canvas
  that knows nothing about molecules — it only draws a `GeometrySpec`. One
  renderer serves every frontend: native PNG, browser canvas, notebook.
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
- **TDD, every layer.** `cargo test` (incl. insta snapshots and the
  `molscene-render` GPU test) and `pytest` stay green; the geometry pipeline is
  the main test surface.

## Status (done)

- **Workspace & tooling**: 4-crate Cargo workspace, maturin packaging, CI
  (cargo/pytest + wasm smoke build), TDD across layers.
- **Core engine** (`molscene-core`): `Structure` model, pdbtbx PDB/mmCIF parser
  (`parse` feature, native-only), declarative `Scene` + JSON spec, distance-based
  bond inference, single-clause selection evaluator, color resolution
  (CPK / spectrum / chain / named / hex), `GeometrySpec` + `Scene::to_geometry`
  for **spheres** and **sticks**.
- **Python** (`molscene`): PyO3 `Scene`/`Selection`, `ms.load` (RCSB fetch or
  local file → parsed in Rust), `ms.select` DSL with `& | ~`, fluent API,
  `_repr_html_` / `show` / `export_html` via iframe srcdoc.
- **Renderer** (`molscene-render`, v0.9): a single wgpu rasterizer — impostor
  spheres/bonds + meshes, oriented-box auto-framing camera — for native PNG and,
  compiled to wasm, the browser canvas + notebook. (The original Three.js
  `viewer/` was removed.)
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

## v0.6.1 — Typed `Element` ✅ (shipped)

A small, focused follow-up to v0.6: replace `Atom.element: String` with a
type-safe `Element` enum. No per-atom `String` and no repeated `trim`/uppercase
normalization on every radius/color/predicate lookup — element comparisons become
a cheap enum `match`, unknown symbols are resolved once at parse time, and the
geometry/perception hot paths stop re-parsing strings.

- **core**: an `Element` enum (the symbols molscene knows + an `Other` catch-all
  for the rest); `covalent_radius` / `vdw_radius` / `color::element_color` and the
  `chem` predicates (`is_multi_capable`, aromatic elements) key off it instead of
  `&str`.
- Cross-cutting but mechanical — touches `structure` / `parse` / `color` /
  `selection`; the public string-based color grammar is unaffected.
- The precursor to the **rigorous chemistry model** (backlog).

**Deliverable:** the element field is a typed `Element`; no behavior change — just
type safety and fewer allocations/normalizations.

## v0.7 — Performance: cell-grid bonds + benchmarks ✅ (shipped)

The original v0.7 ("Rendering quality & performance") bundled several unrelated
efforts into one grab-bag. It is split so each piece is its own ~1-PR milestone
(roughly ≤800 lines of diff); this first one tackles performance so "fast" can be
earned before the polish work lands. The remaining pieces are **v0.7.1–v0.7.5**
below.

- **Performance**: replaced the O(n²) bond inference with a **uniform cell grid**
  (`structure.rs`: `infer_bonds_grid`). Cells are sized to the worst-case bond
  cutoff, so every bond lies within a 27-cell stencil — O(n) at realistic
  molecular densities, with results byte-for-byte identical to the old all-pairs
  scan (a kept `bonds_n2` oracle test pins this). Uses only `Vec`/`HashMap`, so it
  stays WASM-safe.
- **Benchmarks**: criterion benches (`benches/bond_inference.rs`,
  `benches/geometry.rs`) over a deterministic 100 → 100k-atom lattice; run on
  demand (`cargo bench`), not in CI. These earn the "fast" claim for the README.

## v0.7.1 — Lighting & materials ✅ (shipped)

- **Lighting** (renderer-local, `viewer/`): a `HemisphereLight` (soft sky/ground
  gradient so undersides aren't dead black) plus a **key + fill** directional rig
  replaces the old flat ambient + single key, giving shape-revealing shading under
  the `MeshStandardMaterial` surfaces.
- **Truer CPK colors**: `color::element_color` desaturated toward Jmol/PyMOL-family
  values (carbon keeps its green, just softened) so single colors read cleaner under
  the new lighting.
- **Configurable background**: `scene.background("black")` (named color or
  `#rrggbb`), threaded Python → PyO3 → `Scene` → `GeometrySpec.background` (the only
  spec-side lighting/background channel; lighting itself stays renderer-local).
  Defaults to white.
- **Antialiasing**: MSAA stays on; the device-pixel-ratio is capped at 2 so hi-DPI
  displays don't over-render.

## v0.7.2 — Camera framing ✅ (shipped)

Better initial framing plus `center` / `orient`, scoped to the camera. The
compiled camera (`GeomCamera`) went from a loose bounding **sphere** to an
**oriented box** — `center` + a `right`/`up` screen basis + per-axis `extent`
half-widths — so the renderer fits tightly and aspect-aware.

- **`center` now actually frames a selection.** The `Camera.center` `Expr` was
  declared but never evaluated; `geometry.rs` now evaluates it (and the new
  `orient` selection) and a dedicated, WASM-safe `camera.rs` computes the box
  over just those atoms (falls back to all atoms).
- **`orient` (PyMOL-style).** A selection's principal axes (PCA via a self-
  contained symmetric-3×3 Jacobi eigensolver in `camera.rs`) set the screen
  basis: longest spread horizontal, next vertical. Exposed as `scene.orient(sel)`
  through PyO3 and the facade, mirroring `center`.
- **Aspect-aware fit in the viewer.** `fitDistance(extent, aspect, fov)` derives
  the camera distance from the oriented box and the viewport aspect (horizontal
  *and* vertical), so wide/tall viewports no longer clip; the camera positions
  along `right × up` with `up` as its up-vector.

**Deliverable:** `ms.load("1ubq").cartoon().center(ms.select.resi(50))` frames
that residue, and `.orient(ms.select.protein())` lays the long axis horizontal.

## v0.7.3 — Cheap representations: lines & dots ✅ (shipped)

- `lines` (thin bond lines) and `dots` (point clouds), reusing the existing
  sticks/spheres geometry path and `GeometrySpec`. Grouped together since they
  are the same kind of cheap rep and share the implementation.
- **`lines`** reuses the sticks bond path (bond-order aware: double/triple draw
  as parallel offset cylinders, aromatic Kekulé) but with thinner cylinders and
  **without** the ball-and-stick atom caps. **`dots`** reuses the spheres path
  with a small vdW scale — a sphere per atom. Both emit into the existing
  `cylinders`/`spheres` channels, so the renderer is unchanged.

**Deliverable:** `ms.load("1ubq").lines()` draws a cheap wireframe and
`ms.load("1ubq").dots()` a point cloud. ✅

## v0.7.4 — Water/solvent defaults ✅ (shipped)

- Sensible defaults so crystal waters and buffer ions aren't dumped into the view
  by accident. A new **`solvent`** classification macro (`selection.rs`:
  `Expr::Solvent`) matches **water OR common crystallographic ions** (a curated,
  best-effort `ION_RESNAMES` het-code list — Na/K/Mg/Ca/Cl/Zn/SO4/… alongside the
  existing water names), exposed as `ms.select.solvent()`.
- **`spheres()` now defaults to "everything but solvent"**
  (`ms.select.all() & ~ms.select.solvent()`) instead of `all()`, matching the
  water-excluding defaults the other reps already had (`cartoon`/`surface` →
  `protein`, `sticks` → `ligand`). Explicit selections always win, so
  `spheres(ms.select.all())` (or `water()` / `solvent()`) still shows them.
- `Expr::Water` (water only) and `Expr::Ligand` (hetero minus water) keep their
  semantics — `Solvent` is an orthogonal lens layered on top, not a redefinition.

## v0.7.5 — Labels & text annotations ✅ (shipped)

- Atom / residue labels drawn as Three.js **sprites with a `CanvasTexture`**
  (auto-billboarding so text stays readable as the camera orbits), driven by a
  new **`labels` channel** on `GeometrySpec` (`Label { position, text, color,
  size }`). The renderer only rasterizes glyphs; molscene picks position/text/
  color in Rust (`geometry.rs`), keeping the molecule-agnostic renderer split.
- **`scene.label(selection, text=…, color=…, size=…)`** through PyO3 + the
  facade, defaulting to `ms.select.ligand()`. `text` selects the content:
  `residue` (default → `"ALA42"` at the residue's Cα), `resn` / `resi` / `chain`
  (one per residue) or `atom` / `element` (one per atom). Labels default to
  black; `color` runs through the normal scheme machinery (and `set_color`
  overrides still win); `size` is a font scale carried on `Style.scale`.
- Split out from the cheap-rep line because it needs real text-rendering support
  in the renderer; sequenced last in the v0.7.x line.

## v0.8 — WASM / pure-web ✅ (shipped)

Proves the dual-core thesis: the same Rust core drives both Python and the browser.

- **`molscene-wasm` fleshed out** (wasm-bindgen): a `Scene` + `Selection` mirroring
  the PyO3 bindings exactly — the same typed API (representation builders, camera
  `center`/`orient`, `set_color`, `background`, the full `ms.select` constructor set)
  with `and`/`or`/`not` methods standing in for Python's `& | ~`. `to_geometry`
  runs entirely in the browser; the compiled `GeometrySpec` crosses to JS as the
  **same JSON string** the Python iframe uses (`toGeometryJson` → `JSON.parse`).
- **Full parse path in the browser — no hand-written parser.** The blocker was
  pdbtbx pulling `rayon` (threads, unavailable on `wasm32-unknown-unknown`). But
  `rayon` is an *optional* pdbtbx dependency behind cfg-gated parallel methods we
  never call, so `molscene-core` now pulls pdbtbx with `default-features = false`
  (keeping only the WASM-safe `compression`/miniz_oxide). This is a no-op natively
  (we only use `ReadOptions::read_raw` + the base hierarchy API) and lets
  **PDB / mmCIF / SDF** parse in the browser. `kiddo` is already WASM-safe (it gates
  its native `generator` dep to x86_64/aarch64).
- A **web demo page** (`web/`) consuming the WASM core + the existing `viewer/`
  renderer (the prebuilt IIFE bundle, reused as-is — no second bundler). It fetches
  a PDB from RCSB in-browser and draws a **cartoon**, falling back to an embedded
  benzene SDF (aromatic sticks) offline.
- A **CI smoke job** builds the wasm crate (`wasm-pack --target web`), guarding the
  WASM-safe core against a future dependency bump. `wasm-opt` is disabled so the
  build needs no network fetch of binaryen.
- Same `GeometrySpec` contract drives both Python-notebook and pure-web paths — and
  it remains the *only* serialized format on either path.

**Deliverable:** a browser page where JS builds a scene and renders it — zero
Python. ✅

## v0.9 — One wgpu renderer everywhere (Three.js removed) ✅ (shipped)

Rust owns rendering too, not just the molecule. A single wgpu rasterizer for the
`GeometrySpec` contract replaces the Three.js `viewer/` entirely — it produces a
PNG headlessly **and**, compiled to WebAssembly, draws straight to a browser
canvas. This is the long-signposted "wgpu later" path.

- **`molscene-render` crate** (wgpu): consumes the serialized `GeometrySpec` only —
  molecule-agnostic. The shared core (`gpu.rs`: pipelines + per-frame buffers + draw
  recording) **compiles for both native and `wasm32`**; only the platform glue is
  cfg-gated. The crate's `render_png(spec, opts)` is the native headless path
  (request adapter → render-to-texture → read back → encode PNG, supersampled AA).
  Depends on `molscene-core`, never the reverse; wgpu stays out of core.
- **Impostor primitives.** Spheres and cylinders are GPU impostors: a camera-facing
  quad per instance, and the fragment shader ray-traces the exact sphere / finite
  cylinder (lateral + flat caps), writing per-pixel depth so they interpenetrate
  meshes and each other correctly and stay perfectly smooth at any zoom. Cartoon /
  surface meshes are drawn directly (double-sided, depth-sorted transparency for
  translucent groups). Camera framing (45° FOV, aspect-aware oriented-box fit) and
  the hemisphere + key/fill lighting rig are shared, so every frontend matches.
- **Browser renderer** (`molscene-wasm`): a wasm-bindgen `Renderer` bound to a
  `<canvas>` via WebGPU — `loadSpecJson` then `draw(yaw, pitch, zoom)` (drag-orbit +
  zoom) for live display, and async `toPng(w, h, ssaa)` for a downloadable image.
  The `web/` demo and the notebook both drive it; the old Three.js `viewer/` crate,
  its vitest job, and the JS renderer are deleted.
- **Notebook display**: `show()` / `_repr_html_` now inline the WASM bundle (core +
  wgpu `Renderer`, built `--target no-modules`) and the `GeometrySpec` into the
  iframe `srcdoc`, and render on a canvas via WebGPU — still fully self-contained and
  offline. A browser without WebGPU shows a short message (use `save_png`).
- **Python API**: `scene.to_png(width, height, ssaa) -> bytes` and
  `scene.save_png(path, ...)` (a thin PyO3 wrapper over `render_png`; stubs
  regenerated).
- **Graceful degradation**: native, with no GPU/Vulkan/Metal/DX12/GL driver (and no
  software fallback like SwiftShader/llvmpipe), `render_png` returns `NoAdapter` and
  the Rust + Python tests skip rather than fail.

**Deliverables:** `ms.load("1ubq").cartoon(color="spectrum").save_png("ubq.png")`
writes a rendered image with zero browser involvement; the same scene renders in a
notebook and in the `web/` demo via the *same* wgpu renderer compiled to wasm. ✅

## v1.0 — Distribution & ergonomics

- **PyPI wheels** ✅: a maturin `Release` workflow (`.github/workflows/
  release.yml`) builds wheels across Linux (x86_64/aarch64), macOS (Intel/Apple
  Silicon), and Windows, plus an sdist, and publishes to PyPI on a `vX.Y.Z`
  tag. Built against pyo3's stable ABI (`abi3-py310`), so one wheel per platform
  covers every supported CPython; the publish job guards the tag against the
  pyproject version and uploads from a protected `pypi` environment.
- **Type stubs** ✅: `python/molscene/_core/__init__.pyi` is generated by
  `pyo3-stub-gen` from the live PyO3 signatures (a feature-gated `stub_gen`
  binary), not hand-written, so it can't drift — CI regenerates and fails on a
  diff.
- **In-place scene editing** ✅: representations added to a `Scene` are read
  back and edited after the fact via `scene.representations[i]` — style fields
  (`color`/`opacity`/`scale`/`radius`/`text`) and the selection are reassignable
  in place (a `Representation` proxy over Rust-owned state); geometry stays lazy,
  so edits land on the next compile.
- **Docs site** + a gallery of declarative recipes. _(remaining)_

---

## Backlog / longer term

- **Trajectories** and multiple models/states; animation; image/video export.
- **Biological assemblies**, symmetry mates, multi-structure scenes & alignment.
- **Measurements**: distances / angles / dihedrals as scene objects.
- **Density / volumetric** maps (cryo-EM, electron density).
- **Alternative renderer**: a `wgpu` path for native, headless, offscreen PNG
  rendering (screenshots without a browser) — reuses the same `GeometrySpec`.
- Richer mmCIF support; entity/chain metadata.
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
