#!/usr/bin/env sh
# Build the pure-web demo: the Three.js viewer bundle + the molscene WASM core.
#
# Prerequisites (one-time):
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-pack          # or: cargo binstall wasm-pack
#   (Node.js, for the viewer bundle)
#
# Then serve the repo root over HTTP (ES modules + wasm can't load from file://):
#   python3 -m http.server 8000
#   open http://localhost:8000/web/index.html
set -e

repo_root=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo_root"

# 1. The Three.js viewer IIFE → python/molscene/_static/viewer.js (reused as-is).
(cd viewer && npm install && npm run build)

# 2. The WASM core (PDB/mmCIF/SDF parsing + Scene + to_geometry) → web/pkg/.
wasm-pack build crates/molscene-wasm --target web --out-dir ../../web/pkg

echo
echo "Demo ready. Serve the repo root and open the page:"
echo "  python3 -m http.server 8000"
echo "  http://localhost:8000/web/index.html"
