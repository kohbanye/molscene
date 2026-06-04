// Public entry point. esbuild bundles this (with Three.js) into an IIFE exposed
// as `window.molscene`, so the notebook iframe can call
// `molscene.renderGeometry(element, spec)`.

export { renderGeometry } from "./threejsRenderer";
export { buildInstances, quaternionFromYTo } from "./geometry";
export type { GeometrySpec } from "./geometry";
