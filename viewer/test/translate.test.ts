import { describe, expect, it, vi } from "vitest";
import { selectionToSpec, styleForKind, surfaceStyle } from "../src/translate";
import type { Representation } from "../src/spec";

describe("selectionToSpec", () => {
  it("maps macros", () => {
    expect(selectionToSpec("all")).toEqual({});
    expect(selectionToSpec("protein")).toEqual({ hetflag: false });
    expect(selectionToSpec("ligand")).toEqual({ hetflag: true });
    expect(selectionToSpec("water")).toEqual({
      resn: ["HOH", "WAT", "H2O", "TIP3", "SOL"],
    });
  });

  it("maps predicates", () => {
    expect(selectionToSpec("chain A")).toEqual({ chain: "A" });
    expect(selectionToSpec("resi 10-30")).toEqual({ resi: "10-30" });
    expect(selectionToSpec("element Fe")).toEqual({ elem: "Fe" });
  });

  it("defers composed selections to v0.2 and warns", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    expect(selectionToSpec("(chain A) and (ligand)")).toEqual({});
    expect(selectionToSpec("not (hydrogen)")).toEqual({});
    expect(warn).toHaveBeenCalledTimes(2);
    warn.mockRestore();
  });
});

describe("styleForKind", () => {
  const rep = (kind: Representation["kind"], style: Record<string, unknown>): Representation => ({
    structure: "s0",
    kind,
    selection: "all",
    style,
  });

  it("maps cartoon spectrum", () => {
    expect(styleForKind(rep("cartoon", { color: "spectrum" }))).toEqual({
      cartoon: { color: "spectrum" },
    });
  });

  it("maps element coloring to a colorscheme", () => {
    expect(styleForKind(rep("sticks", { color: "element" }))).toEqual({
      stick: { colorscheme: "default" },
    });
  });

  it("maps secondary_structure to ssPyMOL", () => {
    expect(styleForKind(rep("cartoon", { color: "secondary_structure" }))).toEqual({
      cartoon: { colorscheme: "ssPyMOL" },
    });
  });

  it("passes flat colors and opacity through", () => {
    expect(styleForKind(rep("spheres", { color: "red", opacity: 0.5 }))).toEqual({
      sphere: { color: "red", opacity: 0.5 },
    });
  });
});

describe("surfaceStyle", () => {
  it("defaults opacity to 1.0", () => {
    expect(surfaceStyle(undefined)).toEqual({ opacity: 1.0 });
  });
  it("carries opacity", () => {
    expect(surfaceStyle({ opacity: 0.25 })).toEqual({ opacity: 0.25 });
  });
});
