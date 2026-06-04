//! The Scene model: the single in-memory model that serializes directly to the
//! JSON scene spec. Built fluently (`.cartoon().surface().sticks()`), then
//! handed to a renderer via [`Scene::to_json`].

use serde::{Deserialize, Serialize};

use crate::spec::{
    default_spec_version, Camera, Representation, RepresentationKind, Source, StructureEntry, Style,
};

pub use crate::spec::{Camera as CameraSpec, Representation as RepresentationSpec};
pub use crate::spec::{RepresentationKind as Kind, Source as StructureSource};

/// Id used for the (single, in v0.1) structure.
const STRUCTURE_ID: &str = "s0";

/// A molecular scene: a set of structures, the representations drawn over them,
/// and the camera. Serializes directly to the versioned scene spec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    #[serde(rename = "spec_version", default = "default_spec_version")]
    spec_version: String,
    structures: Vec<StructureEntry>,
    representations: Vec<Representation>,
    camera: Camera,
}

impl Scene {
    /// Create a scene from a structure source.
    pub fn new(source: Source) -> Self {
        Self {
            spec_version: default_spec_version(),
            structures: vec![StructureEntry {
                id: STRUCTURE_ID.to_string(),
                source,
            }],
            representations: Vec::new(),
            camera: Camera::default(),
        }
    }

    /// Create a scene that fetches `id` from RCSB.
    pub fn from_rcsb(id: impl Into<String>) -> Self {
        Self::new(Source::Rcsb { id: id.into() })
    }

    /// Create a scene from inline PDB text.
    pub fn from_inline_pdb(data: impl Into<String>) -> Self {
        Self::new(Source::InlinePdb { data: data.into() })
    }

    fn push(&mut self, kind: RepresentationKind, selection: &str, style: Style) -> &mut Self {
        self.representations.push(Representation {
            structure: STRUCTURE_ID.to_string(),
            kind,
            selection: selection.to_string(),
            style,
        });
        self
    }

    pub fn cartoon(&mut self, selection: &str, style: Style) -> &mut Self {
        self.push(RepresentationKind::Cartoon, selection, style)
    }

    pub fn surface(&mut self, selection: &str, style: Style) -> &mut Self {
        self.push(RepresentationKind::Surface, selection, style)
    }

    pub fn sticks(&mut self, selection: &str, style: Style) -> &mut Self {
        self.push(RepresentationKind::Sticks, selection, style)
    }

    pub fn spheres(&mut self, selection: &str, style: Style) -> &mut Self {
        self.push(RepresentationKind::Spheres, selection, style)
    }

    /// Center the camera on a selection (still auto-fits the zoom in v0.1).
    pub fn center(&mut self, selection: &str) -> &mut Self {
        self.camera.center = Some(selection.to_string());
        self
    }

    /// Representations added so far.
    pub fn representations(&self) -> &[Representation] {
        &self.representations
    }

    /// The camera.
    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    /// Serialize to the JSON scene spec.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Scene serializes")
    }

    /// Serialize to pretty JSON.
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("Scene serializes")
    }

    /// Serialize to a `serde_json::Value`.
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("Scene serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn style(v: serde_json::Value) -> Style {
        v.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn builds_representations_in_order() {
        let mut scene = Scene::from_rcsb("1ubq");
        scene
            .cartoon("protein", style(json!({"color": "spectrum"})))
            .surface("protein", style(json!({"opacity": 0.25})))
            .sticks("ligand", style(json!({"color": "element"})));

        let reps = scene.representations();
        assert_eq!(reps.len(), 3);
        assert_eq!(reps[0].kind, RepresentationKind::Cartoon);
        assert_eq!(reps[0].selection, "protein");
        assert_eq!(reps[2].kind, RepresentationKind::Sticks);
        assert_eq!(reps[2].selection, "ligand");
    }

    #[test]
    fn camera_defaults_to_auto() {
        let scene = Scene::from_rcsb("1ubq");
        assert!(scene.camera().auto);
        assert_eq!(scene.camera().center, None);
    }

    #[test]
    fn center_sets_camera_target() {
        let mut scene = Scene::from_rcsb("1ubq");
        scene.center("ligand");
        assert_eq!(scene.camera().center.as_deref(), Some("ligand"));
    }

    #[test]
    fn roundtrips_through_json() {
        let mut scene = Scene::from_rcsb("1ubq");
        scene.cartoon("protein", style(json!({"color": "spectrum"})));
        let json = scene.to_json();
        let back: Scene = serde_json::from_str(&json).unwrap();
        assert_eq!(scene, back);
    }

    #[test]
    fn matches_spec_snapshot() {
        let mut scene = Scene::from_rcsb("1ubq");
        scene
            .cartoon("protein", style(json!({"color": "spectrum"})))
            .surface("protein", style(json!({"opacity": 0.25})))
            .sticks("ligand", style(json!({"color": "element"})));

        insta::assert_json_snapshot!(scene.to_value());
    }
}
