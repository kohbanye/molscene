//! molscene-core
//!
//! The renderer- and binding-agnostic engine for molscene. Owns the structure
//! data model, the scene graph (representations + camera), and the selection
//! expression tree. The `Scene` is an in-memory model compiled to a
//! `GeometrySpec` — the only serialized form all renderers and frontends consume.
//!
//! This crate must never depend on PyO3 or wasm-bindgen — the `molscene-py`
//! and `molscene-wasm` crates are thin translation shells over this API.

pub mod color;
pub mod geometry;
pub mod scene;
pub mod selection;
pub mod spec;
pub mod structure;

#[cfg(feature = "parse")]
pub mod parse;

pub use geometry::{Cylinders, GeometrySpec, Spheres};
pub use scene::Scene;
pub use selection::{evaluate, CmpOp, Expr, NumField};
pub use spec::{Camera, Representation, RepresentationKind, Source, StructureEntry, Style};
pub use structure::{covalent_radius, vdw_radius, Atom, Structure};
