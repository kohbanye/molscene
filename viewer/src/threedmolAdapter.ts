// Drives 3Dmol.js from a molscene scene spec. References the global `$3Dmol`
// (loaded separately from a CDN in the notebook iframe), so it is NOT bundled.

import type { Representation, SceneSpec } from "./spec";
import { selectionToSpec, styleForKind, surfaceStyle } from "./translate";

// 3Dmol is provided globally by the page; typed loosely on purpose.
declare const $3Dmol: any;

export interface ViewerLike {
  addModel(data: string, format: string): unknown;
  setStyle(sel: unknown, style: unknown): unknown;
  addSurface(type: unknown, style: unknown, sel: unknown): unknown;
  zoomTo(sel?: unknown): unknown;
  render(): unknown;
}

function applyRepresentation(viewer: ViewerLike, rep: Representation): void {
  const sel = selectionToSpec(rep.selection);
  if (rep.kind === "surface") {
    viewer.addSurface($3Dmol.SurfaceType.VDW, surfaceStyle(rep.style), sel);
  } else {
    viewer.setStyle(sel, styleForKind(rep));
  }
}

function applyScene(viewer: ViewerLike, spec: SceneSpec): void {
  for (const rep of spec.representations) {
    applyRepresentation(viewer, rep);
  }
  if (spec.camera && spec.camera.center) {
    viewer.zoomTo(selectionToSpec(spec.camera.center));
  } else {
    viewer.zoomTo();
  }
  viewer.render();
}

/** Render a scene spec into a DOM element using 3Dmol.js. */
export function render(element: HTMLElement, spec: SceneSpec): ViewerLike {
  const viewer: ViewerLike = $3Dmol.createViewer(element, { backgroundColor: "white" });
  const source = spec.structures[0]?.source;

  if (!source) {
    applyScene(viewer, spec);
    return viewer;
  }

  if (source.type === "inline_pdb") {
    viewer.addModel(source.data, "pdb");
    applyScene(viewer, spec);
  } else if (source.type === "rcsb") {
    $3Dmol.download(`pdb:${source.id}`, viewer, {}, () => applyScene(viewer, spec));
  } else if (source.type === "url") {
    // v0.x: fetch then addModel. For now, treat as empty scene.
    applyScene(viewer, spec);
  }
  return viewer;
}
