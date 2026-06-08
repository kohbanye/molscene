// Pure-web demo driver. Loads the molscene Rust core (compiled to WebAssembly),
// builds a Scene with the same typed API the Python facade uses, compiles it to
// a GeometrySpec entirely in the browser, and hands the spec to the existing
// Three.js viewer (window.molscene.renderGeometry). No Python anywhere.

import init, { Scene, Selection, version } from "./pkg/molscene_wasm.js";

const statusEl = document.getElementById("status");
const viewport = document.getElementById("viewport");
const fileInput = document.getElementById("file");
const setStatus = (msg) => {
  statusEl.textContent = msg;
};

// Offline fallback: a benzene ring as a V2000 molfile (aromatic bonds → the
// inner-ring depiction). Used when the RCSB fetch is unavailable.
const BENZENE_SDF = `benzene
  molscene
aromatic ring demo
 12 12  0  0  0  0  0  0  0  0999 V2000
    1.3900    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0
    0.6950    1.2038    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0
   -0.6950    1.2038    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0
   -1.3900    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0
   -0.6950   -1.2038    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0
    0.6950   -1.2038    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0
    2.4600    0.0000    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0
    1.2300    2.1304    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0
   -1.2300    2.1304    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0
   -2.4600    0.0000    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0
   -1.2300   -2.1304    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0
    1.2300   -2.1304    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0
  1  2  4  0  0  0  0
  2  3  4  0  0  0  0
  3  4  4  0  0  0  0
  4  5  4  0  0  0  0
  5  6  4  0  0  0  0
  6  1  4  0  0  0  0
  1  7  1  0  0  0  0
  2  8  1  0  0  0  0
  3  9  1  0  0  0  0
  4 10  1  0  0  0  0
  5 11  1  0  0  0  0
  6 12  1  0  0  0  0
M  END
`;

// The current Three.js renderer, kept so a re-render (e.g. an upload) can tear
// down the previous one — renderGeometry only appends a canvas and starts its
// own animation loop, so without this the old scene stays on screen.
let currentRenderer = null;

function render(scene) {
  const spec = JSON.parse(scene.toGeometryJson()); // the only wire format
  if (currentRenderer) {
    currentRenderer.setAnimationLoop(null);
    currentRenderer.dispose();
    currentRenderer.domElement.remove();
    currentRenderer = null;
  }
  viewport.replaceChildren();
  currentRenderer = window.molscene.renderGeometry(viewport, spec);
}

// Cartoon for the protein chains, sticks for everything else that isn't water.
// Works for full structures and bare small molecules alike.
function drawStructure(scene) {
  scene.representation("cartoon", Selection.protein(), "spectrum");
  const rest = Selection.protein().not().and(Selection.water().not());
  scene.representation("sticks", rest, "element");
  scene.setBackground("white");
  render(scene);
}

// Protein hero: fetch a PDB from RCSB (CORS-enabled) and draw a cartoon.
async function renderProtein(id) {
  const url = `https://files.rcsb.org/download/${id}.pdb`;
  const pdb = await (await fetch(url)).text();
  const scene = Scene.fromInlinePdb(pdb);
  scene.representation("cartoon", Selection.protein(), "spectrum");
  scene.representation("sticks", Selection.ligand(), "element");
  scene.setBackground("white");
  render(scene);
  setStatus(`${id}: cartoon (spectrum) + ligand sticks — built in WASM.`);
}

// Offline fallback: a small molecule from an embedded SDF, drawn as sticks.
function renderBenzene() {
  const scene = Scene.fromInlineSdf(BENZENE_SDF);
  scene.representation("sticks", Selection.all(), "element");
  scene.setBackground("white");
  render(scene);
  setStatus("benzene (embedded SDF) — aromatic sticks, built in WASM.");
}

// Build a scene from an uploaded file. The extension picks the parser: SDF /
// MOL molfiles carry explicit bond orders; everything else is treated as PDB.
function renderUpload(name, text) {
  const isSdf = /\.(sdf|mol)$/i.test(name);
  try {
    if (isSdf) {
      const scene = Scene.fromInlineSdf(text);
      scene.representation("sticks", Selection.all(), "element");
      scene.setBackground("white");
      render(scene);
      setStatus(`${name}: sticks (element) — built in WASM.`);
    } else {
      const scene = Scene.fromInlinePdb(text);
      drawStructure(scene);
      setStatus(`${name}: cartoon (spectrum) + sticks — built in WASM.`);
    }
  } catch (err) {
    console.error("failed to render upload:", err);
    setStatus(`Could not parse ${name}: ${err}`);
  }
}

fileInput.addEventListener("change", () => {
  const file = fileInput.files && fileInput.files[0];
  if (!file) return;
  setStatus(`reading ${file.name}…`);
  const reader = new FileReader();
  reader.onload = () => renderUpload(file.name, String(reader.result));
  reader.onerror = () => setStatus(`Could not read ${file.name}.`);
  reader.readAsText(file);
  fileInput.value = ""; // allow re-uploading the same file
});

async function main() {
  await init();
  console.log("molscene-wasm", version());
  try {
    await renderProtein("1ubq");
  } catch (err) {
    console.warn("RCSB fetch failed, falling back to embedded molecule:", err);
    renderBenzene();
  }
}

main();
