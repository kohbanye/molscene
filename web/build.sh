#!/usr/bin/env sh
# Build the pure-web demo: the Three.js viewer bundle + the molscene WASM core.
#
# Prerequisites (one-time):
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-pack          # or: cargo binstall wasm-pack
#   (Node.js, for the viewer bundle)
#
# Then serve the web/ directory over HTTP (ES modules + wasm can't load
# from file://):
#   python3 -m http.server 8000 --directory web
#   open http://localhost:8000/
set -e

repo_root=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo_root"

# 1. The Three.js viewer IIFE → python/molscene/_static/viewer.js, then copy it
#    next to the demo page so index.html can load it with a self-contained path
#    (works the same when served locally or deployed to GitHub Pages).
(cd viewer && npm install && npm run build)
cp python/molscene/_static/viewer.js web/viewer.js

# 2. The WASM core (PDB/mmCIF/SDF parsing + Scene + to_geometry) → web/pkg/.
wasm-pack build crates/molscene-wasm --target web --out-dir ../../web/pkg

echo
echo "Demo ready. Serve the web/ directory and open the page:"
echo "  python3 -m http.server 8000 --directory web"
echo "  http://localhost:8000/"
