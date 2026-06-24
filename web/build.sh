#!/usr/bin/env sh
# Build the pure-web demo: the molscene Rust core + the wgpu renderer, compiled
# to WebAssembly. There is no separate JS renderer anymore — the same Rust wgpu
# renderer that powers .to_png() natively draws straight to the canvas (WebGPU).
#
# Prerequisites (one-time):
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-pack          # or: cargo binstall wasm-pack
#
# Then serve the web/ directory over HTTP (ES modules + wasm can't load
# from file://):
#   python3 -m http.server 8000 --directory web
#   open http://localhost:8000/
set -e

repo_root=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo_root"

# The WASM bundle (PDB/mmCIF/SDF parsing + Scene + to_geometry + the wgpu
# Renderer) → web/pkg/. The demo loads it as an ES module.
wasm-pack build crates/molscene-wasm --target web --out-dir ../../web/pkg

echo
echo "Demo ready. Serve the web/ directory and open the page (WebGPU required):"
echo "  python3 -m http.server 8000 --directory web"
echo "  http://localhost:8000/"
