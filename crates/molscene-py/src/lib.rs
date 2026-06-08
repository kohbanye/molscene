//! PyO3 bindings: thin wrappers over molscene-core, exposed as the
//! `molscene._core` extension module.
//!
//! State lives in Rust (`core::Scene`); the Python facade in `python/molscene/`
//! adds the ergonomic keyword-argument API and notebook display on top.
//! `Selection` wraps a core [`Expr`] and is built through constructor
//! staticmethods (the `ms.select` DSL) and the boolean operators (`& | ~`) — there
//! is no selection string to parse, so an invalid selection cannot be expressed.

use molscene_core::scene::Scene as CoreScene;
use molscene_core::spec::{RepresentationKind, Source, Style};
use molscene_core::{CmpOp, Expr, NumField};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

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

/// A molecular scene. Wraps `molscene_core::Scene`.
#[pyclass(module = "molscene._core")]
pub struct Scene {
    inner: CoreScene,
}

#[pymethods]
impl Scene {
    /// Build a scene from PDB text fetched for an RCSB `id`.
    #[staticmethod]
    fn from_rcsb(id: &str, pdb_text: &str) -> PyResult<Self> {
        let inner = CoreScene::from_pdb(pdb_text, Source::Rcsb { id: id.to_string() })
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Scene { inner })
    }

    /// Build a scene from inline PDB text.
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

    /// Build a scene from inline SDF / V2000 molfile text (explicit bond orders).
    #[staticmethod]
    fn from_inline_sdf(sdf_text: &str) -> PyResult<Self> {
        let inner = CoreScene::from_sdf(
            sdf_text,
            Source::InlineSdf {
                data: sdf_text.to_string(),
            },
        )
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Scene { inner })
    }

    /// Add a representation over `selection` with optional typed style.
    #[pyo3(signature = (kind, selection, color=None, opacity=None, scale=None, radius=None))]
    fn representation(
        &mut self,
        kind: &str,
        selection: &Selection,
        color: Option<String>,
        opacity: Option<f64>,
        scale: Option<f64>,
        radius: Option<f64>,
    ) -> PyResult<()> {
        let kind = parse_kind(kind)?;
        let style = Style {
            color,
            opacity: opacity.map(|v| v as f32),
            scale: scale.map(|v| v as f32),
            radius: radius.map(|v| v as f32),
        };
        let sel = selection.expr.clone();
        match kind {
            RepresentationKind::Cartoon => self.inner.cartoon(sel, style),
            RepresentationKind::Surface => self.inner.surface(sel, style),
            RepresentationKind::Sticks => self.inner.sticks(sel, style),
            RepresentationKind::Spheres => self.inner.spheres(sel, style),
        };
        Ok(())
    }

    /// Center the camera on a selection.
    fn set_center(&mut self, selection: &Selection) {
        self.inner.center(selection.expr.clone());
    }

    /// Override the color of a sub-selection (applied on top of the
    /// representations' schemes, in call order).
    fn set_color(&mut self, selection: &Selection, color: &str) {
        self.inner.set_color(selection.expr.clone(), color);
    }

    /// Set the scene background color (a named color or `#rrggbb`).
    fn set_background(&mut self, color: &str) {
        self.inner.background(color);
    }

    /// Compile to the JSON geometry spec (instanced draw list for the renderer).
    fn to_geometry_json(&self) -> String {
        self.inner.to_geometry_json()
    }
}

/// A selection — a wrapper over a core [`Expr`]. Built through the constructor
/// staticmethods (the `ms.select` DSL) and composed with the boolean operators
/// (`& | ~`). Valid by construction; there is no selection string.
#[pyclass(module = "molscene._core", skip_from_py_object)]
#[derive(Clone)]
pub struct Selection {
    expr: Expr,
}

impl Selection {
    fn of(expr: Expr) -> Selection {
        Selection { expr }
    }
}

#[pymethods]
impl Selection {
    // -- classification macros ----------------------------------------------
    #[staticmethod]
    fn all() -> Selection {
        Selection::of(Expr::All)
    }
    #[staticmethod]
    fn none() -> Selection {
        Selection::of(Expr::None)
    }
    #[staticmethod]
    fn protein() -> Selection {
        Selection::of(Expr::Protein)
    }
    #[staticmethod]
    fn nucleic() -> Selection {
        Selection::of(Expr::Nucleic)
    }
    #[staticmethod]
    fn ligand() -> Selection {
        Selection::of(Expr::Ligand)
    }
    #[staticmethod]
    fn water() -> Selection {
        Selection::of(Expr::Water)
    }
    #[staticmethod]
    fn hetero() -> Selection {
        Selection::of(Expr::Hetero)
    }
    #[staticmethod]
    fn backbone() -> Selection {
        Selection::of(Expr::Backbone)
    }
    #[staticmethod]
    fn sidechain() -> Selection {
        Selection::of(Expr::Sidechain)
    }
    #[staticmethod]
    fn hydrogen() -> Selection {
        Selection::of(Expr::Hydrogen)
    }

    // -- single-clause predicates -------------------------------------------
    #[staticmethod]
    fn chain(id: &str) -> Selection {
        Selection::of(Expr::chain(id))
    }
    #[staticmethod]
    fn resn(name: &str) -> Selection {
        Selection::of(Expr::resn(name))
    }
    #[staticmethod]
    fn element(symbol: &str) -> Selection {
        Selection::of(Expr::element(symbol))
    }
    #[staticmethod]
    #[pyo3(signature = (start, end=None))]
    fn resi(start: i32, end: Option<i32>) -> Selection {
        Selection::of(Expr::resi(start, end.unwrap_or(start)))
    }
    #[staticmethod]
    fn b(op: &str, value: f64) -> PyResult<Selection> {
        Ok(Selection::of(Expr::numeric(
            NumField::BFactor,
            cmp(op)?,
            value,
        )))
    }
    #[staticmethod]
    fn q(op: &str, value: f64) -> PyResult<Selection> {
        Ok(Selection::of(Expr::numeric(
            NumField::Occupancy,
            cmp(op)?,
            value,
        )))
    }

    // -- aggregation --------------------------------------------------------
    #[staticmethod]
    fn byres(sel: &Selection) -> Selection {
        Selection::of(sel.expr.clone().byres())
    }
    #[staticmethod]
    fn bychain(sel: &Selection) -> Selection {
        Selection::of(sel.expr.clone().bychain())
    }
    #[staticmethod]
    fn bymol(sel: &Selection) -> Selection {
        Selection::of(sel.expr.clone().bymol())
    }

    // -- spatial (radius in Å of an operand selection) ----------------------
    #[staticmethod]
    fn around(sel: &Selection, radius: f64) -> PyResult<Selection> {
        Ok(Selection::of(sel.expr.clone().around(radius_of(radius)?)))
    }
    #[staticmethod]
    fn within(sel: &Selection, radius: f64) -> PyResult<Selection> {
        Ok(Selection::of(sel.expr.clone().within(radius_of(radius)?)))
    }
    #[staticmethod]
    fn expand(sel: &Selection, radius: f64) -> PyResult<Selection> {
        Ok(Selection::of(sel.expr.clone().expand(radius_of(radius)?)))
    }
    #[staticmethod]
    fn beyond(sel: &Selection, radius: f64) -> PyResult<Selection> {
        Ok(Selection::of(sel.expr.clone().beyond(radius_of(radius)?)))
    }

    // -- boolean composition ------------------------------------------------
    fn __and__(&self, other: &Selection) -> Selection {
        Selection::of(self.expr.clone().and(other.expr.clone()))
    }
    fn __or__(&self, other: &Selection) -> Selection {
        Selection::of(self.expr.clone().or(other.expr.clone()))
    }
    fn __invert__(&self) -> Selection {
        Selection::of(self.expr.clone().not())
    }

    fn __str__(&self) -> String {
        self.expr.to_string()
    }
    fn __repr__(&self) -> String {
        format!("Selection({:?})", self.expr.to_string())
    }
}

/// Validate a spatial radius (non-negative and finite), or raise `ValueError`.
fn radius_of(radius: f64) -> PyResult<f64> {
    if radius.is_finite() && radius >= 0.0 {
        Ok(radius)
    } else {
        Err(PyValueError::new_err(
            "radius must be a non-negative finite number",
        ))
    }
}

/// Map a comparison operator string to a `CmpOp`, or raise `ValueError`.
fn cmp(op: &str) -> PyResult<CmpOp> {
    CmpOp::parse(op)
        .ok_or_else(|| PyValueError::new_err(format!("invalid comparison operator {op:?}")))
}

#[pymodule]
fn _core(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Scene>()?;
    m.add_class::<Selection>()?;
    Ok(())
}
