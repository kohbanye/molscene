// Public entry point. esbuild bundles this into an IIFE exposed as
// `window.molscene`, so the notebook iframe can call `molscene.render(...)`.

export { render } from "./threedmolAdapter";
export { selectionToSpec, styleForKind, surfaceStyle } from "./translate";
export type { SceneSpec } from "./spec";
