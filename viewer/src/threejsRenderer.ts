// Renders a molscene geometry spec with Three.js. Three.js is a general-purpose
// 3D library that knows nothing about molecules — molscene generates all the
// geometry (in Rust); this just draws instanced spheres and cylinders.

import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";

import {
  buildInstances,
  cross,
  fitDistance,
  flattenVec3,
  type GeometrySpec,
  type Instance,
  isTransparent,
  type Label,
  labelSpriteScale,
} from "./geometry";

/** Font size (px) the label text is rasterized at; world scaling is separate. */
const LABEL_FONT_PX = 64;

/** Rasterize `text` onto a tightly-sized canvas in the label's color. */
function makeTextCanvas(label: Label): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d")!;
  const font = `bold ${LABEL_FONT_PX}px sans-serif`;
  // Measure with the target font, then size the canvas to fit (with padding).
  ctx.font = font;
  const pad = LABEL_FONT_PX * 0.25;
  const textWidth = ctx.measureText(label.text).width;
  canvas.width = Math.ceil(textWidth + pad * 2);
  canvas.height = Math.ceil(LABEL_FONT_PX + pad * 2);
  // measureText/canvas size reset the context, so re-apply the font.
  ctx.font = font;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  const [r, g, b] = label.color;
  ctx.fillStyle = `rgb(${Math.round(r * 255)}, ${Math.round(g * 255)}, ${Math.round(b * 255)})`;
  ctx.fillText(label.text, canvas.width / 2, canvas.height / 2);
  return canvas;
}

/** A camera-facing text sprite for one label. */
function labelSprite(label: Label): THREE.Sprite {
  const canvas = makeTextCanvas(label);
  const texture = new THREE.CanvasTexture(canvas);
  texture.minFilter = THREE.LinearFilter; // non-power-of-two canvas
  const material = new THREE.SpriteMaterial({ map: texture, transparent: true });
  const sprite = new THREE.Sprite(material);
  const [sx, sy] = labelSpriteScale(canvas.width, canvas.height, label.size);
  sprite.scale.set(sx, sy, 1);
  sprite.position.set(label.position[0], label.position[1], label.position[2]);
  return sprite;
}

function instancedMesh(
  base: THREE.BufferGeometry,
  instances: Instance[],
): THREE.InstancedMesh {
  const material = new THREE.MeshStandardMaterial({
    roughness: 0.4,
    metalness: 0.0,
  });
  const mesh = new THREE.InstancedMesh(base, material, instances.length);
  const m = new THREE.Matrix4();
  const pos = new THREE.Vector3();
  const quat = new THREE.Quaternion();
  const scl = new THREE.Vector3();
  const color = new THREE.Color();
  instances.forEach((inst, i) => {
    pos.set(inst.position[0], inst.position[1], inst.position[2]);
    quat.set(
      inst.quaternion[0],
      inst.quaternion[1],
      inst.quaternion[2],
      inst.quaternion[3],
    );
    scl.set(inst.scale[0], inst.scale[1], inst.scale[2]);
    m.compose(pos, quat, scl);
    mesh.setMatrixAt(i, m);
    mesh.setColorAt(
      i,
      color.setRGB(inst.color[0], inst.color[1], inst.color[2]),
    );
  });
  mesh.instanceMatrix.needsUpdate = true;
  if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  return mesh;
}

/** Render a geometry spec into `element`. Returns the renderer for disposal. */
export function renderGeometry(
  element: HTMLElement,
  spec: GeometrySpec,
): THREE.WebGLRenderer {
  const width = element.clientWidth || 640;
  const height = element.clientHeight || 480;

  const scene = new THREE.Scene();
  scene.background = new THREE.Color().setRGB(
    spec.background[0],
    spec.background[1],
    spec.background[2],
  );

  const { spheres, cylinders } = buildInstances(spec);
  if (spheres.length) {
    scene.add(instancedMesh(new THREE.SphereGeometry(1, 24, 16), spheres));
  }
  if (cylinders.length) {
    // Unit cylinder along +Y, height 1 centered at the origin.
    scene.add(
      instancedMesh(new THREE.CylinderGeometry(1, 1, 1, 16), cylinders),
    );
  }

  // Triangle meshes (cartoon, surface). molscene tessellates these in Rust; we
  // just draw the buffers with per-vertex colors. Each group has its own
  // opacity, so a translucent surface can sit over an opaque cartoon — Three.js
  // renders opaque objects first, then depth-sorts the transparent ones.
  for (const mesh of spec.meshes ?? []) {
    if (!mesh.positions.length) continue;
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute(
      "position",
      new THREE.BufferAttribute(flattenVec3(mesh.positions), 3),
    );
    geometry.setAttribute(
      "normal",
      new THREE.BufferAttribute(flattenVec3(mesh.normals), 3),
    );
    geometry.setAttribute(
      "color",
      new THREE.BufferAttribute(flattenVec3(mesh.colors), 3),
    );
    geometry.setIndex(mesh.indices);
    const transparent = isTransparent(mesh.opacity);
    const material = new THREE.MeshStandardMaterial({
      vertexColors: true,
      roughness: 0.4,
      metalness: 0.0,
      side: THREE.DoubleSide,
      transparent,
      opacity: mesh.opacity,
      // Don't occlude geometry behind a translucent surface with depth writes.
      depthWrite: !transparent,
    });
    scene.add(new THREE.Mesh(geometry, material));
  }

  // Text labels: camera-facing sprites with the glyphs rasterized to a canvas
  // texture. Three.js sprites billboard automatically, so they stay readable as
  // the camera orbits.
  for (const label of spec.labels ?? []) {
    scene.add(labelSprite(label));
  }

  // Lighting: a hemisphere light gives a soft sky/ground gradient so undersides
  // aren't dead black (replacing flat ambient), plus a key + fill directional
  // rig for shape-revealing shading. Lights live in world space, so the rig
  // stays fixed relative to the molecule as the camera orbits.
  scene.add(new THREE.HemisphereLight(0xffffff, 0x444444, 0.6));
  const key = new THREE.DirectionalLight(0xffffff, 0.8);
  key.position.set(1, 1, 1);
  scene.add(key);
  const fill = new THREE.DirectionalLight(0xffffff, 0.3);
  fill.position.set(-1, 0.5, -0.5);
  scene.add(fill);

  // Camera fit to the oriented bounding box (aspect-aware, tight per axis).
  const { center, right, up, extent } = spec.camera;
  const fov = 45;
  const aspect = width / height;
  const distance = fitDistance(extent, aspect, fov);
  // View direction (from the box toward the camera) = right × up.
  const forward = cross(right, up);
  const far = distance + Math.hypot(extent[0], extent[1], extent[2]);
  const camera = new THREE.PerspectiveCamera(fov, aspect, 0.1, far * 2 + 100);
  const target = new THREE.Vector3(center[0], center[1], center[2]);
  camera.up.set(up[0], up[1], up[2]);
  camera.position.set(
    center[0] + forward[0] * distance,
    center[1] + forward[1] * distance,
    center[2] + forward[2] * distance,
  );
  camera.lookAt(target);

  const renderer = new THREE.WebGLRenderer({ antialias: true });
  // Cap the device pixel ratio: MSAA already smooths edges, so rendering at
  // 3x+ on hi-DPI displays just wastes fill rate.
  renderer.setPixelRatio(Math.min(globalThis.devicePixelRatio || 1, 2));
  renderer.setSize(width, height);
  element.appendChild(renderer.domElement);

  const controls = new OrbitControls(camera, renderer.domElement);
  controls.target.copy(target);
  controls.update();

  const animate = () => {
    requestAnimationFrame(animate);
    controls.update();
    renderer.render(scene, camera);
  };
  animate();

  return renderer;
}
