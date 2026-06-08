import { describe, expect, it } from "vitest";
import {
  buildInstances,
  cross,
  fitDistance,
  flattenVec3,
  isTransparent,
  quaternionFromYTo,
  type GeometrySpec,
} from "../src/geometry";

function approx(a: number[], b: number[], eps = 1e-4) {
  expect(a.length).toBe(b.length);
  a.forEach((v, i) => expect(Math.abs(v - b[i])).toBeLessThan(eps));
}

describe("quaternionFromYTo", () => {
  it("is identity for +Y", () => {
    approx(quaternionFromYTo([0, 1, 0]), [0, 0, 0, 1]);
  });

  it("is 180° about X for -Y", () => {
    approx(quaternionFromYTo([0, -1, 0]), [1, 0, 0, 0]);
  });

  it("rotates +Y onto +X (-90° about Z)", () => {
    const s = Math.SQRT1_2;
    approx(quaternionFromYTo([2, 0, 0]), [0, 0, -s, s]);
  });
});

const SPEC: GeometrySpec = {
  spheres: {
    centers: [[1, 2, 3]],
    radii: [1.7],
    colors: [[0.2, 1, 0.2]],
  },
  cylinders: {
    starts: [[0, 0, 0]],
    ends: [[0, 2, 0]],
    radii: [0.25],
    colors: [[1, 0, 0]],
  },
  meshes: [],
  camera: {
    center: [0, 0, 0],
    right: [1, 0, 0],
    up: [0, 1, 0],
    extent: [5, 5, 5],
  },
  background: [1, 1, 1],
};

describe("fitDistance", () => {
  it("fits the vertical extent on a square viewport", () => {
    // aspect 1: tanH == tanV, so the larger of width/height drives the fit.
    const tanV = Math.tan((45 * Math.PI) / 360);
    // extent [3, 5, 0]: height (5) dominates -> 5 / tanV.
    expect(fitDistance([3, 5, 0], 1, 45)).toBeCloseTo(5 / tanV, 4);
  });

  it("accounts for aspect so a wide box does not clip", () => {
    // A box wider than it is tall on a square viewport must back off for width.
    const square = fitDistance([10, 1, 0], 1, 45);
    // A wide viewport (aspect 2) needs less distance for the same width.
    const wide = fitDistance([10, 1, 0], 2, 45);
    expect(wide).toBeLessThan(square);
  });

  it("adds the depth half-extent so the near face clears the frustum", () => {
    const flat = fitDistance([2, 2, 0], 1, 45);
    const deep = fitDistance([2, 2, 3], 1, 45);
    expect(deep - flat).toBeCloseTo(3, 4);
  });
});

describe("cross", () => {
  it("computes a right-handed cross product", () => {
    approx(cross([1, 0, 0], [0, 1, 0]), [0, 0, 1]);
  });
});

describe("isTransparent", () => {
  it("treats opacity 1 as opaque and < 1 as transparent", () => {
    expect(isTransparent(1.0)).toBe(false);
    expect(isTransparent(0.3)).toBe(true);
  });
});

describe("flattenVec3", () => {
  it("packs Vec3[] into a flat Float32Array", () => {
    const out = flattenVec3([
      [1, 2, 3],
      [4, 5, 6],
    ]);
    expect(out).toBeInstanceOf(Float32Array);
    expect(Array.from(out)).toEqual([1, 2, 3, 4, 5, 6]);
  });

  it("returns an empty array for no vertices", () => {
    expect(flattenVec3([]).length).toBe(0);
  });
});

describe("buildInstances", () => {
  it("maps spheres to translate+uniform-scale instances", () => {
    const { spheres } = buildInstances(SPEC);
    expect(spheres).toHaveLength(1);
    approx(spheres[0].position, [1, 2, 3]);
    approx(spheres[0].scale, [1.7, 1.7, 1.7]);
    approx(spheres[0].quaternion, [0, 0, 0, 1]);
    approx(spheres[0].color, [0.2, 1, 0.2]);
  });

  it("places a cylinder at the midpoint, scaled to its length", () => {
    const { cylinders } = buildInstances(SPEC);
    expect(cylinders).toHaveLength(1);
    approx(cylinders[0].position, [0, 1, 0]); // midpoint of (0,0,0)-(0,2,0)
    approx(cylinders[0].scale, [0.25, 2, 0.25]); // radius, length, radius
    approx(cylinders[0].quaternion, [0, 0, 0, 1]); // already along +Y
  });
});
