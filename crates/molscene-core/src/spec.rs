//! The versioned JSON scene spec — the single contract between the core and
//! every renderer/frontend (the TS adapter today; Mol* and the WASM web product
//! later). These serde types ARE the wire format; there is no second model.

use serde::{Deserialize, Serialize};

/// Schema version of the JSON scene spec. Bumped on any breaking change so the
/// TS adapter (and future Mol*/WASM consumers) can negotiate.
pub const SPEC_VERSION: &str = "0.1";

pub(crate) fn default_spec_version() -> String {
    SPEC_VERSION.to_string()
}

/// Free-form style map (matches Python keyword args, e.g. `color`, `opacity`).
pub type Style = serde_json::Map<String, serde_json::Value>;

/// Where a structure's coordinates come from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    /// Fetch from RCSB by PDB id (the renderer fetches it).
    Rcsb { id: String },
    /// Inline PDB text embedded in the spec.
    InlinePdb { data: String },
    /// Fetch from an arbitrary URL.
    Url { href: String },
}

/// A structure entry in the scene, addressable by `id` from representations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructureEntry {
    pub id: String,
    pub source: Source,
}

/// The kind of visual representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepresentationKind {
    Cartoon,
    Surface,
    Sticks,
    Spheres,
}

/// One representation: a selection of a structure drawn in a given style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Representation {
    /// Id of the structure this applies to.
    pub structure: String,
    pub kind: RepresentationKind,
    /// Opaque selection string in v0.1 (passed through to the renderer); becomes
    /// a tagged string-or-tree in v0.2.
    pub selection: String,
    #[serde(default, skip_serializing_if = "Style::is_empty")]
    pub style: Style,
}

/// An explicit color override: paint a sub-selection a given color, on top of
/// whatever scheme the representations use. Applied in order (last write wins),
/// PyMOL-style. The color is a string in the same grammar as a representation's
/// `color` style, keeping the spec hand-editable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorAssignment {
    pub selection: String,
    pub color: String,
}

/// Camera state. v0.1 only supports auto zoom-to-fit, optionally centered on a
/// selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub auto: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<String>,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            auto: true,
            center: None,
        }
    }
}
