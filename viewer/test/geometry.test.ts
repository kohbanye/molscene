import { describe, expect, it } from "vitest";
import { buildInstances, quaternionFromYTo, type GeometrySpec } from "../src/geometry";

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
  camera: { center: [0, 0, 0], radius: 5 },
  background: [1, 1, 1],
};

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
