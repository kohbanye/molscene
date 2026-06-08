# molscene — pure-web demo

A browser page where **JavaScript builds a scene and renders it, with zero
Python**. The Rust `molscene-core` engine is compiled to WebAssembly
(`molscene-wasm`); JS calls the same typed `Scene` / `Selection` API the Python
facade uses, compiles to a `GeometrySpec` in the browser, and hands that spec —
the one and only serialized contract — to the existing Three.js viewer.

```
web/index.html ──> ./viewer.js               (renderGeometry; copied by build.sh)
              └──> ./main.js ──> ./pkg/molscene_wasm.js   (the WASM core)
```

## Hosted demo

Pushed to `main`, the demo is published to **GitHub Pages** by
[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) at
`https://kohbanye.github.io/molscene/`. (One-time: enable Pages in the repo
settings with **Source: GitHub Actions**. To preview from a branch before
merging, run the *Deploy web demo* workflow manually from the Actions tab.)

## Build

Prerequisites (one-time):

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack        # or: cargo binstall wasm-pack
# Node.js is needed for the viewer bundle.
```

Then:

```sh
./web/build.sh
```

This builds the demo's artifacts (all **generated, gitignored**):

- `web/viewer.js` — the Three.js viewer IIFE (`window.molscene.renderGeometry`),
  built by esbuild and copied next to the page (reused unchanged from the
  notebook path).
- `web/pkg/` — the wasm-pack output (`molscene_wasm.js` + `molscene_wasm_bg.wasm`),
  an ES module the page imports.

## Serve

ES modules and `.wasm` can't be loaded over `file://`, so serve the `web/`
directory over HTTP:

```sh
python3 -m http.server 8000 --directory web
# open http://localhost:8000/
```

The demo fetches `1UBQ` from RCSB (CORS-enabled) and draws a **cartoon** in the
browser. If the network is unavailable it falls back to an embedded benzene SDF
drawn as aromatic **sticks** — both built entirely in WASM.

## Notes

- The full parse path (PDB / mmCIF / SDF) works in the browser: `pdbtbx` is
  pulled into `molscene-core` with `default-features = false` (no `rayon`, the
  one thread-dependent blocker for `wasm32`), and `kiddo` drops its native-only
  dependency on wasm. No hand-written parser.
- `wasm-opt` is disabled (`Cargo.toml` package metadata) so the build needs no
  network download of binaryen; the `.wasm` is unoptimized. Re-enabling it is a
  size-only follow-up.
