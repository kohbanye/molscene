//! PyO3 bindings: thin wrappers over molscene-core, exposed as the
//! `molscene._core` extension module.
//!
//! State and serialization live in Rust (`core::Scene`); the Python facade in
//! `python/molscene/` adds the ergonomic keyword-argument API and notebook
//! display on top. `Selection` implements the boolean operators in Rust to keep
//! the `ms.select` DSL backed by the core.

use molscene_core::scene::Scene as CoreScene;
use molscene_core::spec::{RepresentationKind, Source, Style};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Validate a selection string against the core grammar, mapping a parse error
/// to a Python `ValueError`.
fn validate_selection(selection: &str) -> PyResult<()> {
    molscene_core::selection::parse(selection)
        .map(|_| ())
        .map_err(|e| PyValueError::new_err(format!("invalid selection {selection:?}: {e}")))
}

fn parse_kind(kind: &str) -> PyResult<RepresentationKind> {
    Ok(match kind {
        "cartoon" => RepresentationKind::Cartoon,
        "surface" => RepresentationKind::Surface,
        "sticks" => RepresentationKind::Sticks,
        "spheres" => RepresentationKind::Spheres,
        other => {
            return Err(PyValueError::new_err(format!(
                "unknown representation kind: {other:?}"
            )))
        }
    })
}

fn parse_style(style_json: &str) -> PyResult<Style> {
    if style_json.is_empty() {
        return Ok(Style::new());
    }
    let value: serde_json::Value = serde_json::from_str(style_json)
        .map_err(|e| PyValueError::new_err(format!("invalid style JSON: {e}")))?;
    match value {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err(PyValueError::new_err("style must be a JSON object")),
    }
}

/// A molecular scene. Wraps `molscene_core::Scene`.
#[pyclass(module = "molscene._core")]
pub struct Scene {
    inner: CoreScene,
}

#[pymethods]
impl Scene {
    /// Build a scene from PDB text fetched for an RCSB `id`. Parses the
    /// coordinates and records the id as the spec source.
    #[staticmethod]
    fn from_rcsb(id: &str, pdb_text: &str) -> PyResult<Self> {
        let inner = CoreScene::from_pdb(pdb_text, Source::Rcsb { id: id.to_string() })
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Scene { inner })
    }

    /// Build a scene from inline PDB text (parsed and embedded in the spec).
    #[staticmethod]
    fn from_inline_pdb(pdb_text: &str) -> PyResult<Self> {
        let inner = CoreScene::from_pdb(
            pdb_text,
            Source::InlinePdb {
                data: pdb_text.to_string(),
            },
        )
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Scene { inner })
    }

    /// Add a representation. `style_json` is a JSON object string (or "").
    fn representation(&mut self, kind: &str, selection: &str, style_json: &str) -> PyResult<()> {
        let kind = parse_kind(kind)?;
        validate_selection(selection)?;
        let style = parse_style(style_json)?;
        match kind {
            RepresentationKind::Cartoon => self.inner.cartoon(selection, style),
            RepresentationKind::Surface => self.inner.surface(selection, style),
            RepresentationKind::Sticks => self.inner.sticks(selection, style),
            RepresentationKind::Spheres => self.inner.spheres(selection, style),
        };
        Ok(())
    }

    /// Center the camera on a selection.
    fn set_center(&mut self, selection: &str) -> PyResult<()> {
        validate_selection(selection)?;
        self.inner.center(selection);
        Ok(())
    }

    /// Override the color of a sub-selection (applied on top of the
    /// representations' schemes, in call order).
    fn set_color(&mut self, selection: &str, color: &str) -> PyResult<()> {
        validate_selection(selection)?;
        self.inner.set_color(selection, color);
        Ok(())
    }

    /// Serialize to the JSON scene spec (declarative form).
    fn to_json(&self) -> String {
        self.inner.to_json()
    }

    /// Compile to the JSON geometry spec (instanced draw list for the renderer).
    fn to_geometry_json(&self) -> String {
        self.inner.to_geometry_json()
    }
}

/// A selection. Wraps a selection string; the boolean operators (`& | ~`)
/// compose it. The core parses and evaluates the string into an expression tree
/// (boolean / spatial / aggregation / numeric); invalid strings raise
/// `ValueError` when added to a scene.
#[pyclass(module = "molscene._core", skip_from_py_object)]
#[derive(Clone)]
pub struct Selection {
    value: String,
}

#[pymethods]
impl Selection {
    #[new]
    fn new(value: String) -> Self {
        Selection { value }
    }

    #[getter]
    fn value(&self) -> &str {
        &self.value
    }

    fn __and__(&self, other: &Selection) -> Selection {
        Selection {
            value: format!("({}) and ({})", self.value, other.value),
        }
    }

    fn __or__(&self, other: &Selection) -> Selection {
        Selection {
            value: format!("({}) or ({})", self.value, other.value),
        }
    }

    fn __invert__(&self) -> Selection {
        Selection {
            value: format!("not ({})", self.value),
        }
    }

    fn __str__(&self) -> &str {
        &self.value
    }

    fn __repr__(&self) -> String {
        format!("Selection({:?})", self.value)
    }
}

#[pymodule]
fn _core(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("SPEC_VERSION", molscene_core::spec::SPEC_VERSION)?;
    m.add_class::<Scene>()?;
    m.add_class::<Selection>()?;
    Ok(())
}
