# molscene web

A browser page where **JavaScript builds a scene and the Rust wgpu renderer
draws it**. The `molscene-core` engine *and* the `molscene-render` renderer are
compiled to WebAssembly (`molscene-wasm`); JS calls the same typed `Scene` /
`Selection` API the Python facade uses, compiles to a `GeometrySpec` in the
browser, and hands that spec — the one and only serialized contract — to the
wgpu `Renderer`, which draws straight to a `<canvas>` via **WebGPU**. The same
renderer powers `Scene.to_png()` natively; there is no JavaScript renderer.

Drag to orbit, scroll to zoom, and **Download PNG** to save a high-res image
(rendered offscreen by the same renderer). You can also **upload a PDB or SDF
file**; it is parsed in WASM and never leaves the browser.

```text
web/index.html ──> ./main.js ──> ./pkg/molscene_wasm.js   (core + wgpu Renderer, WASM)
                                  Renderer.create(canvas) → loadSpecJson → draw()/toPng()
```

Requires a **WebGPU-capable browser** (recent Chrome/Edge; Firefox/Safari with
WebGPU enabled). Without WebGPU the page shows a short message.

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
```

Then:

```sh
./web/build.sh
```

This builds `web/pkg/` — the wasm-pack output (`molscene_wasm.js` +
`molscene_wasm_bg.wasm`), an ES module the page imports (**generated,
gitignored**).

## Serve

ES modules and `.wasm` can't be loaded over `file://`, so serve the `web/`
directory over HTTP:

```sh
python3 -m http.server 8000 --directory web
# open http://localhost:8000/
```

The demo fetches `1UBQ` from RCSB (CORS-enabled) and draws a **cartoon** in the
browser. If the network is unavailable it falls back to an embedded benzene SDF
drawn as aromatic **sticks** — both built and rendered entirely in WASM/WebGPU.
Use the **Upload PDB or SDF…** button to load your own file (extension picks the
parser: `.sdf` / `.mol` → sticks, anything else → PDB cartoon + sticks).

## Notes

- The full parse path (PDB / mmCIF / SDF) works in the browser: `pdbtbx` is
  pulled into `molscene-core` with `default-features = false` (no `rayon`, the
  one thread-dependent blocker for `wasm32`), and `kiddo` drops its native-only
  dependency on wasm. No hand-written parser.
- `wasm-opt` is disabled (`Cargo.toml` package metadata) so the build needs no
  network download of binaryen; the `.wasm` is unoptimized. Re-enabling it is a
  size-only follow-up.
