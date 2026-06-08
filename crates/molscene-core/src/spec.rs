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
    /// Radius scale (spheres/dots); font scale (labels).
    pub scale: Option<f32>,
    /// Cylinder radius (sticks).
    pub radius: Option<f32>,
    /// Label content mode (labels): `residue` (default) / `resn` / `resi` /
    /// `chain` / `atom` / `element`. Ignored by other representations.
    pub text: Option<String>,
}

/// Where a structure's coordinates come from (provenance only).
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// Fetched from RCSB by PDB id.
    Rcsb { id: String },
    /// Inline PDB text.
    InlinePdb { data: String },
    /// Inline SDF / V2000 molfile text.
    InlineSdf { data: String },
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
    /// Thin bond lines: like sticks (bond-order aware) but without the
    /// ball-and-stick atom caps — a cheap wireframe.
    Lines,
    /// Point cloud: a small sphere per atom (cheaper than full spheres).
    Dots,
    /// Text annotations: per-residue or per-atom labels drawn as camera-facing
    /// billboards.
    Labels,
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

/// Camera state. Auto zoom-to-fit, optionally centered on a selection and/or
/// oriented so a selection's principal axes align with the screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    pub auto: bool,
    /// Frame (and translate the view to) this selection instead of all atoms.
    pub center: Option<Expr>,
    /// Orient the view along this selection's principal axes (PCA): its longest
    /// dimension goes horizontal, the next vertical (PyMOL-style `orient`).
    pub orient: Option<Expr>,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            auto: true,
            center: None,
            orient: None,
        }
    }
}
