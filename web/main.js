// Pure-web demo driver. Loads the molscene Rust core (compiled to WebAssembly),
// builds a Scene with the same typed API the Python facade uses, and renders it
// with the Rust wgpu renderer (molscene-render, via WebGPU) straight onto the
// canvas — the same renderer that produces .to_png() natively. No Three.js, no
// Python.

import init, { Renderer, Scene, Selection, version } from "./pkg/molscene_wasm.js";

const statusEl = document.getElementById("status");
const canvas = document.getElementById("viewport");
const fileInput = document.getElementById("file");
const downloadBtn = document.getElementById("download");
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

// The wgpu renderer (created once) and the current scene + camera orbit state.
let renderer = null;
let currentScene = null;
const camera = { yaw: 0, pitch: 0, zoom: 1 };

/** Size the canvas backing store to its CSS box (device pixels) and the surface. */
function resizeToDisplay() {
  const dpr = Math.min(globalThis.devicePixelRatio || 1, 2);
  const w = Math.max(1, Math.floor(canvas.clientWidth * dpr));
  const h = Math.max(1, Math.floor(canvas.clientHeight * dpr));
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
    if (renderer) renderer.resize(w, h);
  }
}

/** Draw the loaded geometry with the current camera, if ready. */
function draw() {
  if (!renderer || !currentScene) return;
  renderer.draw(camera.yaw, camera.pitch, camera.zoom);
}

/** Swap in a new scene (resetting the orbit) and draw it. The renderer consumes
 *  the compiled GeometrySpec JSON — the one wire format — not the Scene. */
function setScene(scene) {
  currentScene = scene;
  camera.yaw = 0;
  camera.pitch = 0;
  camera.zoom = 1;
  renderer.loadSpecJson(scene.toGeometryJson());
  downloadBtn.disabled = false;
  resizeToDisplay();
  draw();
}

// Cartoon for the protein chains, sticks for everything else that isn't water.
function drawStructure(scene) {
  scene.representation("cartoon", Selection.protein(), "spectrum");
  const rest = Selection.protein().not().and(Selection.water().not());
  scene.representation("sticks", rest, "element");
  scene.setBackground("white");
  setScene(scene);
}

async function renderProtein(id) {
  const url = `https://files.rcsb.org/download/${id}.pdb`;
  const pdb = await (await fetch(url)).text();
  const scene = Scene.fromInlinePdb(pdb);
  scene.representation("cartoon", Selection.protein(), "spectrum");
  scene.representation("sticks", Selection.ligand(), "element");
  scene.setBackground("white");
  setScene(scene);
  setStatus(`${id}: cartoon (spectrum) + ligand sticks — rendered in WASM/wgpu.`);
}

function renderBenzene() {
  const scene = Scene.fromInlineSdf(BENZENE_SDF);
  scene.representation("sticks", Selection.all(), "element");
  scene.setBackground("white");
  setScene(scene);
  setStatus("benzene (embedded SDF) — aromatic sticks, rendered in WASM/wgpu.");
}

function renderUpload(name, text) {
  const isSdf = /\.(sdf|mol)$/i.test(name);
  try {
    if (isSdf) {
      const scene = Scene.fromInlineSdf(text);
      scene.representation("sticks", Selection.all(), "element");
      scene.setBackground("white");
      setScene(scene);
      setStatus(`${name}: sticks (element) — rendered in WASM/wgpu.`);
    } else {
      const scene = Scene.fromInlinePdb(text);
      drawStructure(scene);
      setStatus(`${name}: cartoon (spectrum) + sticks — rendered in WASM/wgpu.`);
    }
  } catch (err) {
    console.error("failed to render upload:", err);
    setStatus(`Could not parse ${name}: ${err}`);
  }
}

// -- interaction: drag to orbit, wheel to zoom ------------------------------
let dragging = false;
let lastX = 0;
let lastY = 0;
canvas.addEventListener("pointerdown", (e) => {
  dragging = true;
  lastX = e.clientX;
  lastY = e.clientY;
  canvas.setPointerCapture(e.pointerId);
});
canvas.addEventListener("pointermove", (e) => {
  if (!dragging) return;
  const dx = e.clientX - lastX;
  const dy = e.clientY - lastY;
  lastX = e.clientX;
  lastY = e.clientY;
  camera.yaw -= dx * 0.01;
  // Clamp pitch so the view can't flip past the poles.
  const limit = Math.PI / 2 - 0.01;
  camera.pitch = Math.max(-limit, Math.min(limit, camera.pitch + dy * 0.01));
  draw();
});
const endDrag = () => {
  dragging = false;
};
canvas.addEventListener("pointerup", endDrag);
canvas.addEventListener("pointercancel", endDrag);
canvas.addEventListener(
  "wheel",
  (e) => {
    e.preventDefault();
    camera.zoom *= Math.exp(-e.deltaY * 0.001);
    camera.zoom = Math.max(0.1, Math.min(10, camera.zoom));
    draw();
  },
  { passive: false },
);

globalThis.addEventListener("resize", () => {
  resizeToDisplay();
  draw();
});

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

// Download a high-res PNG rendered offscreen by the same Rust renderer.
downloadBtn.addEventListener("click", async () => {
  if (!renderer || !currentScene) return;
  downloadBtn.disabled = true;
  setStatus("rendering PNG…");
  try {
    const bytes = await renderer.toPng(1600, 1200, 2);
    const blob = new Blob([bytes], { type: "image/png" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "molscene.png";
    a.click();
    URL.revokeObjectURL(url);
    setStatus("PNG downloaded.");
  } catch (err) {
    console.error("PNG export failed:", err);
    setStatus(`PNG export failed: ${err}`);
  } finally {
    downloadBtn.disabled = false;
  }
});

async function main() {
  await init();
  console.log("molscene-wasm", version());

  if (!("gpu" in navigator)) {
    setStatus(
      "This browser has no WebGPU support. Try a recent Chrome/Edge, or Firefox/Safari with WebGPU enabled.",
    );
    return;
  }
  try {
    renderer = await Renderer.create(canvas);
  } catch (err) {
    console.error("WebGPU init failed:", err);
    setStatus(`Could not initialize WebGPU: ${err}`);
    return;
  }

  try {
    await renderProtein("1ubq");
  } catch (err) {
    console.warn("RCSB fetch failed, falling back to embedded molecule:", err);
    renderBenzene();
  }
}

main();
