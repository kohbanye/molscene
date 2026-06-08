// The geometry spec emitted by molscene-core, plus pure helpers that turn it
// into per-instance transforms. Kept free of any Three.js import so it is unit-
// testable without a WebGL context.

export type Vec3 = [number, number, number];
export type Quat = [number, number, number, number]; // x, y, z, w

export interface Spheres {
  centers: Vec3[];
  radii: number[];
  colors: Vec3[];
}

export interface Cylinders {
  starts: Vec3[];
  ends: Vec3[];
  radii: number[];
  colors: Vec3[];
}

/**
 * A triangle mesh with per-vertex normals and colors, drawn as one group with a
 * single opacity (cartoon and surface each emit their own mesh).
 */
export interface Mesh {
  positions: Vec3[];
  normals: Vec3[];
  indices: number[];
  colors: Vec3[];
  /** 1.0 = opaque; < 1.0 is drawn with depth-sorted transparency. */
  opacity: number;
}

/**
 * Camera framing as an oriented box. `right`/`up` are the unit screen basis; the
 * view direction is `right × up`. `extent` are the half-widths along
 * `(right, up, forward)`, so the renderer can fit tightly per axis.
 */
export interface GeomCamera {
  center: Vec3;
  right: Vec3;
  up: Vec3;
  extent: Vec3;
}

/**
 * Distance from the box center at which the oriented box just fills the frustum,
 * accounting for the viewport aspect ratio (so wide or tall viewports never
 * clip). `extent` is `[half-width, half-height, half-depth]`; `aspect` is
 * width/height; `fovDeg` is the vertical field of view.
 */
export function fitDistance(
  extent: Vec3,
  aspect: number,
  fovDeg: number,
): number {
  // Guard against a degenerate viewport/FOV (e.g. zero-height container) that
  // would make tan blow up to Infinity/NaN and fling the camera to nowhere.
  if (!(aspect > 0) || !(fovDeg > 0 && fovDeg < 180)) {
    return extent[2] + Math.max(extent[0], extent[1], 1);
  }
  const tanV = Math.tan((fovDeg * Math.PI) / 360);
  const tanH = tanV * aspect;
  const distV = extent[1] / tanV + extent[2];
  const distH = extent[0] / tanH + extent[2];
  return Math.max(distV, distH);
}

/** Right-handed cross product `a × b`. */
export function cross(a: Vec3, b: Vec3): Vec3 {
  return [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ];
}

export interface GeometrySpec {
  spheres: Spheres;
  cylinders: Cylinders;
  meshes: Mesh[];
  camera: GeomCamera;
  background: Vec3;
}

/** Whether a mesh group needs the transparent (depth-sorted blend) path. */
export function isTransparent(opacity: number): boolean {
  return opacity < 1.0;
}

/** Flatten an array of Vec3 into a packed Float32Array for a BufferAttribute. */
export function flattenVec3(v: Vec3[]): Float32Array {
  const out = new Float32Array(v.length * 3);
  for (let i = 0; i < v.length; i++) {
    out[i * 3] = v[i][0];
    out[i * 3 + 1] = v[i][1];
    out[i * 3 + 2] = v[i][2];
  }
  return out;
}

export interface Instance {
  position: Vec3;
  quaternion: Quat;
  scale: Vec3;
  color: Vec3;
}

const IDENTITY_QUAT: Quat = [0, 0, 0, 1];

/** Quaternion rotating the +Y axis (the axis of THREE.CylinderGeometry) onto `dir`. */
export function quaternionFromYTo(dir: Vec3): Quat {
  const len = Math.hypot(dir[0], dir[1], dir[2]);
  if (len === 0) return IDENTITY_QUAT;
  const d: Vec3 = [dir[0] / len, dir[1] / len, dir[2] / len];
  const dot = d[1]; // dot([0,1,0], d)
  if (dot > 0.999999) return IDENTITY_QUAT;
  if (dot < -0.999999) return [1, 0, 0, 0]; // 180° about X
  // axis = cross([0,1,0], d) = [d.z, 0, -d.x]
  const axis: Vec3 = [d[2], 0, -d[0]];
  const axisLen = Math.hypot(axis[0], axis[1], axis[2]);
  const angle = Math.acos(dot);
  const s = Math.sin(angle / 2) / axisLen;
  return [axis[0] * s, axis[1] * s, axis[2] * s, Math.cos(angle / 2)];
}

function midpoint(a: Vec3, b: Vec3): Vec3 {
  return [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2, (a[2] + b[2]) / 2];
}

/** Per-instance transforms for every sphere and cylinder in the spec. */
export function buildInstances(spec: GeometrySpec): {
  spheres: Instance[];
  cylinders: Instance[];
} {
  const spheres: Instance[] = spec.spheres.centers.map((center, i) => {
    const r = spec.spheres.radii[i];
    return {
      position: center,
      quaternion: IDENTITY_QUAT,
      scale: [r, r, r] as Vec3,
      color: spec.spheres.colors[i],
    };
  });

  const cylinders: Instance[] = spec.cylinders.starts.map((start, i) => {
    const end = spec.cylinders.ends[i];
    const r = spec.cylinders.radii[i];
    const dir: Vec3 = [end[0] - start[0], end[1] - start[1], end[2] - start[2]];
    const length = Math.hypot(dir[0], dir[1], dir[2]);
    return {
      position: midpoint(start, end),
      quaternion: quaternionFromYTo(dir),
      scale: [r, length, r] as Vec3,
      color: spec.cylinders.colors[i],
    };
  });

  return { spheres, cylinders };
}
