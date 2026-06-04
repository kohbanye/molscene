//! molscene-core
//!
//! The renderer- and binding-agnostic engine for molscene. Owns the structure
//! data model, the scene graph (representations + camera), the selection
//! expression tree, and serde-based serialization to the versioned JSON
//! "scene spec" that all renderers and frontends consume.
//!
//! This crate must never depend on PyO3 or wasm-bindgen — the `molscene-py`
//! and `molscene-wasm` crates are thin translation shells over this API.

pub mod color;
pub mod scene;
pub mod selection;
pub mod spec;
pub mod structure;

#[cfg(feature = "parse")]
pub mod parse;

pub use scene::Scene;
pub use selection::evaluate;
pub use spec::{Camera, Representation, RepresentationKind, Source, StructureEntry, Style};
pub use structure::{covalent_radius, vdw_radius, Atom, Structure};
