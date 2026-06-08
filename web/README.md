# molscene — pure-web demo

A browser page where **JavaScript builds a scene and renders it, with zero
Python**. The Rust `molscene-core` engine is compiled to WebAssembly
(`molscene-wasm`); JS calls the same typed `Scene` / `Selection` API the Python
facade uses, compiles to a `GeometrySpec` in the browser, and hands that spec —
the one and only serialized contract — to the existing Three.js viewer.

```
web/index.html ──> ../python/molscene/_static/viewer.js  (renderGeometry)
              └──> ./main.js ──> ./pkg/molscene_wasm.js   (the WASM core)
```

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

This builds two artifacts:

- `python/molscene/_static/viewer.js` — the Three.js viewer IIFE
  (`window.molscene.renderGeometry`), reused unchanged from the notebook path.
- `web/pkg/` — the wasm-pack output (`molscene_wasm.js` + `molscene_wasm_bg.wasm`),
  an ES module the page imports. **Generated, gitignored.**

## Serve

ES modules and `.wasm` can't be loaded over `file://`, so serve over HTTP:

```sh
python3 -m http.server 8000
# open http://localhost:8000/web/index.html
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
