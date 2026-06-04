// Pure translation from molscene selections/styles to 3Dmol.js specs.
//
// v0.1 supports single-clause selection strings. Composed expressions
// (`and` / `or` / `not`, produced by the ms.sel operators) are deferred to the
// v0.2 Rust evaluator, which will resolve them to explicit atom sets; until then
// they select everything and log a warning.

import type { Representation } from "./spec";

export type AtomSpec = Record<string, unknown>;

const WATER_RESN = ["HOH", "WAT", "H2O", "TIP3", "SOL"];

/** Translate a molscene selection string into a 3Dmol AtomSelectionSpec. */
export function selectionToSpec(selection: string): AtomSpec {
  const s = selection.trim();
  if (s === "" || s === "all") return {};
  if (s === "none") return { resi: -99999 };

  if (isComposed(s)) {
    // eslint-disable-next-line no-console
    console.warn(
      `molscene: composed selection ${JSON.stringify(s)} is not evaluated in ` +
        `v0.1; selecting all atoms. (Rust evaluator lands in v0.2.)`,
    );
    return {};
  }
  return clauseToSpec(stripParens(s));
}

function isComposed(s: string): boolean {
  return / and | or |^not\b/.test(s) || s.includes(") and (") || s.includes(") or (");
}

function stripParens(s: string): string {
  let out = s.trim();
  while (out.startsWith("(") && out.endsWith(")")) {
    out = out.slice(1, -1).trim();
  }
  return out;
}

function clauseToSpec(clause: string): AtomSpec {
  switch (clause) {
    case "protein":
    case "nucleic":
    case "polymer":
      return { hetflag: false };
    case "ligand":
    case "hetero":
      return { hetflag: true };
    case "water":
    case "solvent":
      return { resn: WATER_RESN };
    case "hydrogen":
      return { elem: "H" };
    case "backbone":
      return { atom: ["C", "CA", "N", "O"] };
  }

  const [kw, ...rest] = clause.split(/\s+/);
  const arg = rest.join(" ");
  switch (kw) {
    case "chain":
      return { chain: arg };
    case "resn":
      return { resn: arg };
    case "resi":
      return { resi: arg };
    case "element":
    case "elem":
      return { elem: arg };
  }

  // eslint-disable-next-line no-console
  console.warn(`molscene: unrecognized selection ${JSON.stringify(clause)}; selecting all.`);
  return {};
}

/** Coloring keywords that map to a 3Dmol colorscheme rather than a flat color. */
const COLORSCHEMES: Record<string, string> = {
  element: "default",
  chain: "chain",
  secondary_structure: "ssPyMOL",
};

function applyColor(target: Record<string, unknown>, style: Record<string, unknown>): void {
  const color = style.color as string | undefined;
  if (color === undefined) return;
  if (color === "spectrum") {
    target.color = "spectrum";
  } else if (color in COLORSCHEMES) {
    target.colorscheme = COLORSCHEMES[color];
  } else {
    target.color = color;
  }
}

/** 3Dmol style object for a non-surface representation (cartoon/sticks/spheres). */
export function styleForKind(rep: Representation): Record<string, unknown> {
  const style = rep.style ?? {};
  const inner: Record<string, unknown> = {};
  applyColor(inner, style);
  if (style.opacity !== undefined) inner.opacity = style.opacity;

  switch (rep.kind) {
    case "cartoon":
      return { cartoon: inner };
    case "sticks": {
      if (style.radius !== undefined) inner.radius = style.radius;
      return { stick: inner };
    }
    case "spheres": {
      if (style.scale !== undefined) inner.scale = style.scale;
      return { sphere: inner };
    }
    default:
      return {};
  }
}

/** 3Dmol surface style object. */
export function surfaceStyle(style: Record<string, unknown> | undefined): Record<string, unknown> {
  const s = style ?? {};
  const out: Record<string, unknown> = {};
  applyColor(out, s);
  out.opacity = s.opacity !== undefined ? s.opacity : 1.0;
  return out;
}
