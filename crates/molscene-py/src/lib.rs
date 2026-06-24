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
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3_stub_gen::derive::{gen_stub_pyclass, gen_stub_pymethods};

fn kind_name(kind: RepresentationKind) -> &'static str {
    match kind {
        RepresentationKind::Cartoon => "cartoon",
        RepresentationKind::Surface => "surface",
        RepresentationKind::Sticks => "sticks",
        RepresentationKind::Spheres => "spheres",
        RepresentationKind::Lines => "lines",
        RepresentationKind::Dots => "dots",
        RepresentationKind::Labels => "labels",
    }
}

fn parse_kind(kind: &str) -> PyResult<RepresentationKind> {
    Ok(match kind {
        "cartoon" => RepresentationKind::Cartoon,
        "surface" => RepresentationKind::Surface,
        "sticks" => RepresentationKind::Sticks,
        "spheres" => RepresentationKind::Spheres,
        "lines" => RepresentationKind::Lines,
        "dots" => RepresentationKind::Dots,
        "labels" => RepresentationKind::Labels,
        other => {
            return Err(PyValueError::new_err(format!(
                "unknown representation kind: {other:?}"
            )))
        }
    })
}

/// A molecular scene. Wraps `molscene_core::Scene`.
#[gen_stub_pyclass]
#[pyclass(module = "molscene._core")]
pub struct Scene {
    inner: CoreScene,
}

#[gen_stub_pymethods]
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
    #[allow(clippy::too_many_arguments)] // one keyword per style field, by design
    #[pyo3(signature = (kind, selection, color=None, opacity=None, scale=None, radius=None, text=None))]
    fn representation(
        &mut self,
        kind: &str,
        selection: &Selection,
        color: Option<String>,
        opacity: Option<f64>,
        scale: Option<f64>,
        radius: Option<f64>,
        text: Option<String>,
    ) -> PyResult<()> {
        let kind = parse_kind(kind)?;
        let style = Style {
            color,
            opacity: opacity.map(|v| v as f32),
            scale: scale.map(|v| v as f32),
            radius: radius.map(|v| v as f32),
            text,
        };
        let sel = selection.expr.clone();
        match kind {
            RepresentationKind::Cartoon => self.inner.cartoon(sel, style),
            RepresentationKind::Surface => self.inner.surface(sel, style),
            RepresentationKind::Sticks => self.inner.sticks(sel, style),
            RepresentationKind::Spheres => self.inner.spheres(sel, style),
            RepresentationKind::Lines => self.inner.lines(sel, style),
            RepresentationKind::Dots => self.inner.dots(sel, style),
            RepresentationKind::Labels => self.inner.labels(sel, style),
        };
        Ok(())
    }

    /// Center the camera on a selection.
    fn set_center(&mut self, selection: &Selection) {
        self.inner.center(selection.expr.clone());
    }

    /// Orient the view along a selection's principal axes.
    fn set_orient(&mut self, selection: &Selection) {
        self.inner.orient(selection.expr.clone());
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

    // -- in-place representation editing ------------------------------------
    // The facade exposes these as a `scene.representations` sequence of editable
    // proxies; here they are flat index-keyed accessors over the core's
    // representation list (state stays in Rust).

    /// Number of representations added so far.
    fn num_representations(&self) -> usize {
        self.inner.representations().len()
    }

    /// The kind of representation `index` (e.g. `"cartoon"`).
    fn rep_kind(&self, index: usize) -> PyResult<&'static str> {
        Ok(kind_name(self.rep(index)?.kind))
    }

    /// The style of representation `index` as
    /// `(color, opacity, scale, radius, text)`.
    #[allow(clippy::type_complexity)]
    fn rep_style(
        &self,
        index: usize,
    ) -> PyResult<(
        Option<String>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<String>,
    )> {
        let s = &self.rep(index)?.style;
        Ok((
            s.color.clone(),
            s.opacity.map(|v| v as f64),
            s.scale.map(|v| v as f64),
            s.radius.map(|v| v as f64),
            s.text.clone(),
        ))
    }

    /// Replace the whole style of representation `index`. A `None` field clears
    /// that field (back to the representation's default).
    #[pyo3(signature = (index, color=None, opacity=None, scale=None, radius=None, text=None))]
    fn set_rep_style(
        &mut self,
        index: usize,
        color: Option<String>,
        opacity: Option<f64>,
        scale: Option<f64>,
        radius: Option<f64>,
        text: Option<String>,
    ) -> PyResult<()> {
        self.rep_mut(index)?.style = Style {
            color,
            opacity: opacity.map(|v| v as f32),
            scale: scale.map(|v| v as f32),
            radius: radius.map(|v| v as f32),
            text,
        };
        Ok(())
    }

    /// The selection of representation `index`.
    fn rep_selection(&self, index: usize) -> PyResult<Selection> {
        Ok(Selection::of(self.rep(index)?.selection.clone()))
    }

    /// Re-target representation `index` at a new selection.
    fn set_rep_selection(&mut self, index: usize, selection: &Selection) -> PyResult<()> {
        self.rep_mut(index)?.selection = selection.expr.clone();
        Ok(())
    }

    /// Compile to the JSON geometry spec (instanced draw list for the renderer).
    fn to_geometry_json(&self) -> String {
        self.inner.to_geometry_json()
    }

    /// Render the scene to PNG bytes with the native GPU rasterizer
    /// (`molscene-render`, via wgpu). Headless — no window or browser. Returns
    /// the PNG file contents as `bytes`. Raises `RuntimeError` if no GPU
    /// (or software fallback) is available.
    #[pyo3(signature = (width=800, height=600, ssaa=2))]
    fn to_png<'py>(
        &self,
        py: Python<'py>,
        width: u32,
        height: u32,
        ssaa: u32,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let spec = self.inner.to_geometry();
        let opts = molscene_render::RenderOptions {
            width,
            height,
            ssaa,
        };
        let bytes = molscene_render::render_png(&spec, &opts)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(PyBytes::new(py, &bytes))
    }
}

impl Scene {
    /// Borrow representation `index`, raising `IndexError` if out of range.
    fn rep(&self, index: usize) -> PyResult<&molscene_core::spec::Representation> {
        self.inner
            .representations()
            .get(index)
            .ok_or_else(|| index_error(index, self.inner.representations().len()))
    }

    fn rep_mut(&mut self, index: usize) -> PyResult<&mut molscene_core::spec::Representation> {
        let len = self.inner.representations().len();
        self.inner
            .representations_mut()
            .get_mut(index)
            .ok_or_else(|| index_error(index, len))
    }
}

fn index_error(index: usize, len: usize) -> PyErr {
    pyo3::exceptions::PyIndexError::new_err(format!(
        "representation index {index} out of range (have {len})"
    ))
}

/// A selection — a wrapper over a core [`Expr`]. Built through the constructor
/// staticmethods (the `ms.select` DSL) and composed with the boolean operators
/// (`& | ~`). Valid by construction; there is no selection string.
#[gen_stub_pyclass]
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

#[gen_stub_pymethods]
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
    fn solvent() -> Selection {
        Selection::of(Expr::Solvent)
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

/// Gathers the `#[gen_stub_*]`-registered signatures into a [`StubInfo`], using
/// the workspace-root `pyproject.toml` (two levels up from this crate) for the
/// maturin layout — `module-name = "molscene._core"` and `python-source =
/// "python"` — so the `.pyi` lands at `python/molscene/_core.pyi`. The
/// `stub_gen` binary calls this and writes the stubs.
pub fn stub_info() -> pyo3_stub_gen::Result<pyo3_stub_gen::generate::StubInfo> {
    let pyproject = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../pyproject.toml");
    pyo3_stub_gen::generate::StubInfo::from_pyproject_toml(pyproject)
}
