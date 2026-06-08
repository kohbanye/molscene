//! The Scene model: the in-memory model built fluently
//! (`.cartoon().surface().sticks()`) and compiled to a `GeometrySpec` via
//! [`Scene::to_geometry`]. The Scene itself is not serialized — the building code
//! is the source of truth.

use crate::selection::Expr;
use crate::spec::{
    Camera, ColorAssignment, Representation, RepresentationKind, Source, StructureEntry, Style,
};
use crate::structure::Structure;

pub use crate::spec::{Camera as CameraSpec, Representation as RepresentationSpec};
pub use crate::spec::{RepresentationKind as Kind, Source as StructureSource};

/// Id used for the (single, for now) structure.
const STRUCTURE_ID: &str = "s0";

/// A molecular scene: a set of structures, the representations drawn over them,
/// explicit color overrides, and the camera. An in-memory model only.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    structures: Vec<StructureEntry>,
    representations: Vec<Representation>,
    colors: Vec<ColorAssignment>,
    camera: Camera,
    /// Background color, parsed at set-time; `None` keeps the default (white).
    background: Option<crate::color::Rgb>,
    /// Parsed coordinates, kept in memory for native geometry generation.
    structure: Option<Structure>,
}

impl Scene {
    /// Create a scene from a structure source.
    pub fn new(source: Source) -> Self {
        Self {
            structures: vec![StructureEntry {
                id: STRUCTURE_ID.to_string(),
                source,
            }],
            representations: Vec::new(),
            colors: Vec::new(),
            camera: Camera::default(),
            background: None,
            structure: None,
        }
    }

    /// Create a scene that fetches `id` from RCSB (no coordinates loaded yet).
    pub fn from_rcsb(id: impl Into<String>) -> Self {
        Self::new(Source::Rcsb { id: id.into() })
    }

    /// Create a scene from inline PDB text (no coordinates loaded yet).
    pub fn from_inline_pdb(data: impl Into<String>) -> Self {
        Self::new(Source::InlinePdb { data: data.into() })
    }

    /// Parse `text` as an SDF / V2000 molfile and build a scene holding the
    /// resulting coordinates and explicit bond orders.
    #[cfg(feature = "parse")]
    pub fn from_sdf(text: &str, source: Source) -> Result<Self, crate::parse::ParseError> {
        let structure = crate::parse::parse_str(text, crate::parse::InputFormat::Sdf)?;
        let mut scene = Self::new(source);
        scene.structure = Some(structure);
        Ok(scene)
    }

    /// Parse `text` and build a scene that holds the resulting coordinates.
    /// `source` records provenance (e.g. an RCSB id).
    #[cfg(feature = "parse")]
    pub fn from_pdb(text: &str, source: Source) -> Result<Self, crate::parse::ParseError> {
        let structure = crate::parse::parse_str(text, crate::parse::InputFormat::Pdb)?;
        let mut scene = Self::new(source);
        scene.structure = Some(structure);
        Ok(scene)
    }

    /// Attach parsed coordinates to the scene.
    pub fn with_structure(mut self, structure: Structure) -> Self {
        self.structure = Some(structure);
        self
    }

    /// The loaded coordinates, if any.
    pub fn structure(&self) -> Option<&Structure> {
        self.structure.as_ref()
    }

    fn push(&mut self, kind: RepresentationKind, selection: Expr, style: Style) -> &mut Self {
        self.representations.push(Representation {
            structure: STRUCTURE_ID.to_string(),
            kind,
            selection,
            style,
        });
        self
    }

    pub fn cartoon(&mut self, selection: Expr, style: Style) -> &mut Self {
        self.push(RepresentationKind::Cartoon, selection, style)
    }

    pub fn surface(&mut self, selection: Expr, style: Style) -> &mut Self {
        self.push(RepresentationKind::Surface, selection, style)
    }

    pub fn sticks(&mut self, selection: Expr, style: Style) -> &mut Self {
        self.push(RepresentationKind::Sticks, selection, style)
    }

    pub fn spheres(&mut self, selection: Expr, style: Style) -> &mut Self {
        self.push(RepresentationKind::Spheres, selection, style)
    }

    /// Center the camera on a selection (still auto-fits the zoom).
    pub fn center(&mut self, selection: Expr) -> &mut Self {
        self.camera.center = Some(selection);
        self
    }

    /// Orient the view along a selection's principal axes (PyMOL-style
    /// `orient`): its longest dimension is laid out horizontally, the next
    /// vertically. Also frames the selection unless `center` overrides it.
    pub fn orient(&mut self, selection: Expr) -> &mut Self {
        self.camera.orient = Some(selection);
        self
    }

    /// Override the color of a sub-selection, on top of the representations'
    /// schemes. Applied in call order (last write wins) at geometry time.
    pub fn set_color(&mut self, selection: Expr, color: &str) -> &mut Self {
        self.colors.push(ColorAssignment {
            selection,
            color: color.to_string(),
        });
        self
    }

    /// Set the scene background color (a named color or `#rrggbb`). Parsed at
    /// set-time; an unrecognized color leaves the current value unchanged
    /// (the default is white until set).
    pub fn background(&mut self, color: &str) -> &mut Self {
        if let Some(rgb) =
            crate::color::named_color(color).or_else(|| crate::color::parse_hex(color))
        {
            self.background = Some(rgb);
        } else {
            eprintln!("molscene: unknown background color {color:?}; keeping previous value.");
        }
        self
    }

    /// The background color, if one was set.
    pub fn background_color(&self) -> Option<crate::color::Rgb> {
        self.background
    }

    /// Representations added so far.
    pub fn representations(&self) -> &[Representation] {
        &self.representations
    }

    /// Explicit color overrides added so far (in application order).
    pub fn color_assignments(&self) -> &[ColorAssignment] {
        &self.colors
    }

    /// The camera.
    pub fn camera(&self) -> &Camera {
        &self.camera
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_representations_in_order() {
        let mut scene = Scene::from_rcsb("1ubq");
        scene
            .cartoon(
                Expr::Protein,
                Style {
                    color: Some("spectrum".into()),
                    ..Default::default()
                },
            )
            .surface(
                Expr::Protein,
                Style {
                    opacity: Some(0.25),
                    ..Default::default()
                },
            )
            .sticks(
                Expr::Ligand,
                Style {
                    color: Some("element".into()),
                    ..Default::default()
                },
            );

        let reps = scene.representations();
        assert_eq!(reps.len(), 3);
        assert_eq!(reps[0].kind, RepresentationKind::Cartoon);
        assert_eq!(reps[0].selection, Expr::Protein);
        assert_eq!(reps[2].kind, RepresentationKind::Sticks);
        assert_eq!(reps[2].selection, Expr::Ligand);
    }

    #[test]
    fn camera_defaults_to_auto() {
        let scene = Scene::from_rcsb("1ubq");
        assert!(scene.camera().auto);
        assert_eq!(scene.camera().center, None);
        assert_eq!(scene.camera().orient, None);
    }

    #[test]
    fn center_sets_camera_target() {
        let mut scene = Scene::from_rcsb("1ubq");
        scene.center(Expr::Ligand);
        assert_eq!(scene.camera().center, Some(Expr::Ligand));
    }

    #[test]
    fn orient_sets_camera_axes() {
        let mut scene = Scene::from_rcsb("1ubq");
        scene.orient(Expr::Protein);
        assert_eq!(scene.camera().orient, Some(Expr::Protein));
    }

    #[test]
    fn background_parses_named_and_hex_and_keeps_default_on_unknown() {
        let mut scene = Scene::from_rcsb("1ubq");
        assert_eq!(scene.background_color(), None);
        scene.background("black");
        assert_eq!(scene.background_color(), Some([0.0, 0.0, 0.0]));
        scene.background("#ff0000");
        assert_eq!(scene.background_color(), Some([1.0, 0.0, 0.0]));
        // An unrecognized color leaves the previous value untouched.
        scene.background("definitely-not-a-color");
        assert_eq!(scene.background_color(), Some([1.0, 0.0, 0.0]));
    }

    #[test]
    fn set_color_records_overrides_in_order() {
        let mut scene = Scene::from_rcsb("1ubq");
        scene
            .set_color(Expr::Protein, "grey")
            .set_color(Expr::resi(50, 50), "red");
        let overrides = scene.color_assignments();
        assert_eq!(overrides.len(), 2);
        assert_eq!(overrides[0].selection, Expr::Protein);
        assert_eq!(overrides[0].color, "grey");
        assert_eq!(overrides[1].color, "red");
    }
}
