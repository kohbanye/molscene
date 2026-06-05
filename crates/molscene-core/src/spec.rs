//! The in-memory scene model types.
//!
//! These are plain Rust values — there is no serialized scene format. The `Scene`
//! is built through the fluent API and compiled to a `GeometrySpec` (the only
//! wire format); the building code is the source of truth.

use crate::selection::Expr;

/// Per-representation style. Plain fields, not a free-form map: `color` is a
/// string in the color grammar (parsed to a `ColorScheme` at geometry time);
/// the rest are typed scalars.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Style {
    pub color: Option<String>,
    pub opacity: Option<f32>,
    /// Sphere radius scale (spheres).
    pub scale: Option<f32>,
    /// Cylinder radius (sticks).
    pub radius: Option<f32>,
}

/// Where a structure's coordinates come from (provenance only).
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// Fetched from RCSB by PDB id.
    Rcsb { id: String },
    /// Inline PDB text.
    InlinePdb { data: String },
    /// Fetched from an arbitrary URL.
    Url { href: String },
}

/// A structure entry in the scene, addressable by `id` from representations.
#[derive(Debug, Clone, PartialEq)]
pub struct StructureEntry {
    pub id: String,
    pub source: Source,
}

/// The kind of visual representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepresentationKind {
    Cartoon,
    Surface,
    Sticks,
    Spheres,
}

/// One representation: a selection of a structure drawn in a given style.
#[derive(Debug, Clone, PartialEq)]
pub struct Representation {
    /// Id of the structure this applies to.
    pub structure: String,
    pub kind: RepresentationKind,
    pub selection: Expr,
    pub style: Style,
}

/// An explicit color override: paint a sub-selection a given color, on top of
/// whatever scheme the representations use. Applied in order (last write wins),
/// PyMOL-style. The color is a string in the color grammar.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorAssignment {
    pub selection: Expr,
    pub color: String,
}

/// Camera state. Auto zoom-to-fit, optionally centered on a selection.
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    pub auto: bool,
    pub center: Option<Expr>,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            auto: true,
            center: None,
        }
    }
}
