// Renders a molscene geometry spec with Three.js. Three.js is a general-purpose
// 3D library that knows nothing about molecules — molscene generates all the
// geometry (in Rust); this just draws instanced spheres and cylinders.

import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";

import { buildInstances, type GeometrySpec, type Instance } from "./geometry";

function instancedMesh(
  base: THREE.BufferGeometry,
  instances: Instance[],
): THREE.InstancedMesh {
  const material = new THREE.MeshStandardMaterial({ roughness: 0.4, metalness: 0.0 });
  const mesh = new THREE.InstancedMesh(base, material, instances.length);
  const m = new THREE.Matrix4();
  const pos = new THREE.Vector3();
  const quat = new THREE.Quaternion();
  const scl = new THREE.Vector3();
  const color = new THREE.Color();
  instances.forEach((inst, i) => {
    pos.set(inst.position[0], inst.position[1], inst.position[2]);
    quat.set(inst.quaternion[0], inst.quaternion[1], inst.quaternion[2], inst.quaternion[3]);
    scl.set(inst.scale[0], inst.scale[1], inst.scale[2]);
    m.compose(pos, quat, scl);
    mesh.setMatrixAt(i, m);
    mesh.setColorAt(i, color.setRGB(inst.color[0], inst.color[1], inst.color[2]));
  });
  mesh.instanceMatrix.needsUpdate = true;
  if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  return mesh;
}

/** Render a geometry spec into `element`. Returns the renderer for disposal. */
export function renderGeometry(element: HTMLElement, spec: GeometrySpec): THREE.WebGLRenderer {
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
    scene.add(instancedMesh(new THREE.CylinderGeometry(1, 1, 1, 16), cylinders));
  }

  // Lighting.
  scene.add(new THREE.AmbientLight(0xffffff, 0.6));
  const key = new THREE.DirectionalLight(0xffffff, 0.8);
  key.position.set(1, 1, 1);
  scene.add(key);

  // Camera fit to the bounding sphere.
  const { center, radius } = spec.camera;
  const fov = 45;
  const camera = new THREE.PerspectiveCamera(fov, width / height, 0.1, radius * 100 + 100);
  const target = new THREE.Vector3(center[0], center[1], center[2]);
  const distance = radius / Math.sin((fov * Math.PI) / 360);
  camera.position.set(center[0], center[1], center[2] + distance);
  camera.lookAt(target);

  const renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setPixelRatio(globalThis.devicePixelRatio || 1);
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
