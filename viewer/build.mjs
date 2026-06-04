// Bundles the TS adapter into a single IIFE that the notebook iframe loads.
// 3Dmol.js itself is NOT bundled — it is loaded separately (CDN or vendored)
// and the adapter references the global `$3Dmol`.
import * as esbuild from "esbuild";

await esbuild.build({
  entryPoints: ["src/index.ts"],
  bundle: true,
  format: "iife",
  globalName: "molscene",
  outfile: "../python/molscene/_static/viewer.js",
  target: "es2018",
  minify: true,
  sourcemap: false,
});

console.log("built -> python/molscene/_static/viewer.js");
