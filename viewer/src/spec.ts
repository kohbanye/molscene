// The JSON scene spec — the contract emitted by molscene-core. Mirrors the
// serde types in crates/molscene-core/src/spec.rs.

export type Source =
  | { type: "rcsb"; id: string }
  | { type: "inline_pdb"; data: string }
  | { type: "url"; href: string };

export interface StructureEntry {
  id: string;
  source: Source;
}

export type RepresentationKind = "cartoon" | "surface" | "sticks" | "spheres";

export interface Representation {
  structure: string;
  kind: RepresentationKind;
  selection: string;
  style?: Record<string, unknown>;
}

export interface Camera {
  auto: boolean;
  center?: string;
}

export interface SceneSpec {
  spec_version: string;
  structures: StructureEntry[];
  representations: Representation[];
  camera: Camera;
}
