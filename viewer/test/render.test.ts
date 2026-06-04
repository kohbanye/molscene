import { beforeEach, describe, expect, it, vi } from "vitest";
import { render } from "../src/threedmolAdapter";
import type { SceneSpec } from "../src/spec";

// A recording mock of the 3Dmol viewer and global.
function makeMock() {
  const calls: Array<{ method: string; args: unknown[] }> = [];
  const rec = (method: string) => (...args: unknown[]) => {
    calls.push({ method, args });
  };
  const viewer = {
    addModel: rec("addModel"),
    setStyle: rec("setStyle"),
    addSurface: rec("addSurface"),
    zoomTo: rec("zoomTo"),
    render: rec("render"),
  };
  const $3Dmol = {
    SurfaceType: { VDW: "VDW" },
    createViewer: vi.fn(() => viewer),
    download: vi.fn((_q: string, _v: unknown, _o: unknown, cb: () => void) => cb()),
  };
  return { calls, viewer, $3Dmol };
}

const SPEC: SceneSpec = {
  spec_version: "0.1",
  structures: [{ id: "s0", source: { type: "rcsb", id: "1ubq" } }],
  representations: [
    { structure: "s0", kind: "cartoon", selection: "protein", style: { color: "spectrum" } },
    { structure: "s0", kind: "surface", selection: "protein", style: { opacity: 0.25 } },
    { structure: "s0", kind: "sticks", selection: "ligand", style: { color: "element" } },
  ],
  camera: { auto: true },
};

describe("render", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("downloads the rcsb structure and applies each representation", () => {
    const { calls, $3Dmol } = makeMock();
    (globalThis as any).$3Dmol = $3Dmol;

    render({} as HTMLElement, SPEC);

    expect($3Dmol.createViewer).toHaveBeenCalledOnce();
    expect($3Dmol.download).toHaveBeenCalledWith(
      "pdb:1ubq",
      expect.anything(),
      expect.anything(),
      expect.any(Function),
    );

    const sequence = calls.map((c) => c.method);
    expect(sequence).toEqual(["setStyle", "addSurface", "setStyle", "zoomTo", "render"]);

    // cartoon spectrum
    expect(calls[0].args).toEqual([{ hetflag: false }, { cartoon: { color: "spectrum" } }]);
    // surface opacity
    expect(calls[1].args).toEqual(["VDW", { opacity: 0.25 }, { hetflag: false }]);
    // sticks element -> colorscheme
    expect(calls[2].args).toEqual([{ hetflag: true }, { stick: { colorscheme: "default" } }]);
  });

  it("adds an inline model directly (no download)", () => {
    const { calls, $3Dmol } = makeMock();
    (globalThis as any).$3Dmol = $3Dmol;

    const spec: SceneSpec = {
      ...SPEC,
      structures: [{ id: "s0", source: { type: "inline_pdb", data: "ATOM ..." } }],
      representations: [{ structure: "s0", kind: "cartoon", selection: "all" }],
    };
    render({} as HTMLElement, spec);

    expect($3Dmol.download).not.toHaveBeenCalled();
    expect(calls[0]).toEqual({ method: "addModel", args: ["ATOM ...", "pdb"] });
    expect(calls.map((c) => c.method)).toEqual(["addModel", "setStyle", "zoomTo", "render"]);
  });

  it("zooms to the camera center when set", () => {
    const { calls, $3Dmol } = makeMock();
    (globalThis as any).$3Dmol = $3Dmol;

    render({} as HTMLElement, { ...SPEC, camera: { auto: true, center: "ligand" } });
    const zoom = calls.find((c) => c.method === "zoomTo")!;
    expect(zoom.args).toEqual([{ hetflag: true }]);
  });
});
