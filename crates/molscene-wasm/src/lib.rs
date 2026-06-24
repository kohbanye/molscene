//! wasm-bindgen bindings over molscene-core for the browser scene engine.
//!
//! These mirror the PyO3 bindings in `molscene-py`: a `Scene` wrapping
//! `molscene_core::Scene` and a `Selection` wrapping a core [`Expr`], built
//! through constructor statics and composed with `and`/`or`/`not` methods (JS
//! has no operator overloading, so these replace Python's `& | ~`). The compiled
//! `GeometrySpec` is rendered in the browser by the shared wgpu renderer
//! ([`Renderer`], in `render`), the same one that powers `Scene.to_png`
//! natively — `toGeometryJson()` → `Renderer.loadSpecJson` → WebGPU.
//!
//! The entire `parse` path (PDB / mmCIF / SDF) is available here: pdbtbx is
//! WASM-safe in molscene-core (no rayon), so the browser builds a `Scene` from
//! inline PDB/SDF text with zero Python.

use molscene_core::scene::Scene as CoreScene;
use molscene_core::spec::{RepresentationKind, Source, Style};
use molscene_core::{CmpOp, Expr, NumField};
use wasm_bindgen::prelude::*;

// The GPU renderer uses browser-only APIs (canvas surface, WebGPU); it exists
// only in the wasm build. A plain `cargo build --workspace` (native) skips it.
#[cfg(target_arch = "wasm32")]
mod render;
#[cfg(target_arch = "wasm32")]
pub use render::Renderer;

/// Install a panic hook so Rust panics surface in the browser console. Runs once
/// when the wasm module is instantiated.
#[wasm_bindgen(start)]
pub fn start() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// The crate version (smoke export, also useful for cache-busting in demos).
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn parse_kind(kind: &str) -> Result<RepresentationKind, JsValue> {
    Ok(match kind {
        "cartoon" => RepresentationKind::Cartoon,
        "surface" => RepresentationKind::Surface,
        "sticks" => RepresentationKind::Sticks,
        "spheres" => RepresentationKind::Spheres,
        "lines" => RepresentationKind::Lines,
        "dots" => RepresentationKind::Dots,
        "labels" => RepresentationKind::Labels,
        other => {
            return Err(JsValue::from_str(&format!(
                "unknown representation kind: {other:?}"
            )))
        }
    })
}

/// A molecular scene. Wraps `molscene_core::Scene`.
#[wasm_bindgen]
pub struct Scene {
    inner: CoreScene,
}

#[wasm_bindgen]
impl Scene {
    /// Build a scene from PDB text fetched for an RCSB `id`.
    #[wasm_bindgen(js_name = fromRcsb)]
    pub fn from_rcsb(id: &str, pdb_text: &str) -> Result<Scene, JsValue> {
        let inner = CoreScene::from_pdb(pdb_text, Source::Rcsb { id: id.to_string() })
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Scene { inner })
    }

    /// Build a scene from inline PDB text.
    #[wasm_bindgen(js_name = fromInlinePdb)]
    pub fn from_inline_pdb(pdb_text: &str) -> Result<Scene, JsValue> {
        let inner = CoreScene::from_pdb(
            pdb_text,
            Source::InlinePdb {
                data: pdb_text.to_string(),
            },
        )
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Scene { inner })
    }

    /// Build a scene from inline SDF / V2000 molfile text (explicit bond orders).
    #[wasm_bindgen(js_name = fromInlineSdf)]
    pub fn from_inline_sdf(sdf_text: &str) -> Result<Scene, JsValue> {
        let inner = CoreScene::from_sdf(
            sdf_text,
            Source::InlineSdf {
                data: sdf_text.to_string(),
            },
        )
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Scene { inner })
    }

    /// Add a representation over `selection` with optional typed style. Omitted
    /// style fields are passed as `undefined` from JS.
    #[allow(clippy::too_many_arguments)] // one argument per style field, by design
    #[wasm_bindgen]
    pub fn representation(
        &mut self,
        kind: &str,
        selection: &Selection,
        color: Option<String>,
        opacity: Option<f64>,
        scale: Option<f64>,
        radius: Option<f64>,
        text: Option<String>,
    ) -> Result<(), JsValue> {
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
    #[wasm_bindgen(js_name = setCenter)]
    pub fn set_center(&mut self, selection: &Selection) {
        self.inner.center(selection.expr.clone());
    }

    /// Orient the view along a selection's principal axes.
    #[wasm_bindgen(js_name = setOrient)]
    pub fn set_orient(&mut self, selection: &Selection) {
        self.inner.orient(selection.expr.clone());
    }

    /// Override the color of a sub-selection (applied on top of the
    /// representations' schemes, in call order).
    #[wasm_bindgen(js_name = setColor)]
    pub fn set_color(&mut self, selection: &Selection, color: &str) {
        self.inner.set_color(selection.expr.clone(), color);
    }

    /// Set the scene background color (a named color or `#rrggbb`).
    #[wasm_bindgen(js_name = setBackground)]
    pub fn set_background(&mut self, color: &str) {
        self.inner.background(color);
    }

    /// Compile to the JSON geometry spec (the instanced draw list the renderer
    /// consumes — the same wire format the Python path serializes).
    #[wasm_bindgen(js_name = toGeometryJson)]
    pub fn to_geometry_json(&self) -> String {
        self.inner.to_geometry_json()
    }
}

/// A selection — a wrapper over a core [`Expr`]. Built through the constructor
/// statics (mirroring the `ms.select` DSL) and composed with the `and`/`or`/`not`
/// methods. Valid by construction; there is no selection string.
#[wasm_bindgen]
#[derive(Clone)]
pub struct Selection {
    expr: Expr,
}

impl Selection {
    fn of(expr: Expr) -> Selection {
        Selection { expr }
    }
}

#[wasm_bindgen]
impl Selection {
    // -- classification macros ----------------------------------------------
    #[wasm_bindgen]
    pub fn all() -> Selection {
        Selection::of(Expr::All)
    }
    #[wasm_bindgen]
    pub fn none() -> Selection {
        Selection::of(Expr::None)
    }
    #[wasm_bindgen]
    pub fn protein() -> Selection {
        Selection::of(Expr::Protein)
    }
    #[wasm_bindgen]
    pub fn nucleic() -> Selection {
        Selection::of(Expr::Nucleic)
    }
    #[wasm_bindgen]
    pub fn ligand() -> Selection {
        Selection::of(Expr::Ligand)
    }
    #[wasm_bindgen]
    pub fn water() -> Selection {
        Selection::of(Expr::Water)
    }
    #[wasm_bindgen]
    pub fn solvent() -> Selection {
        Selection::of(Expr::Solvent)
    }
    #[wasm_bindgen]
    pub fn hetero() -> Selection {
        Selection::of(Expr::Hetero)
    }
    #[wasm_bindgen]
    pub fn backbone() -> Selection {
        Selection::of(Expr::Backbone)
    }
    #[wasm_bindgen]
    pub fn sidechain() -> Selection {
        Selection::of(Expr::Sidechain)
    }
    #[wasm_bindgen]
    pub fn hydrogen() -> Selection {
        Selection::of(Expr::Hydrogen)
    }

    // -- single-clause predicates -------------------------------------------
    #[wasm_bindgen]
    pub fn chain(id: &str) -> Selection {
        Selection::of(Expr::chain(id))
    }
    #[wasm_bindgen]
    pub fn resn(name: &str) -> Selection {
        Selection::of(Expr::resn(name))
    }
    #[wasm_bindgen]
    pub fn element(symbol: &str) -> Selection {
        Selection::of(Expr::element(symbol))
    }
    #[wasm_bindgen]
    pub fn resi(start: i32, end: Option<i32>) -> Selection {
        Selection::of(Expr::resi(start, end.unwrap_or(start)))
    }
    #[wasm_bindgen]
    pub fn b(op: &str, value: f64) -> Result<Selection, JsValue> {
        Ok(Selection::of(Expr::numeric(
            NumField::BFactor,
            cmp(op)?,
            value,
        )))
    }
    #[wasm_bindgen]
    pub fn q(op: &str, value: f64) -> Result<Selection, JsValue> {
        Ok(Selection::of(Expr::numeric(
            NumField::Occupancy,
            cmp(op)?,
            value,
        )))
    }

    // -- aggregation --------------------------------------------------------
    #[wasm_bindgen(js_name = byRes)]
    pub fn byres(sel: &Selection) -> Selection {
        Selection::of(sel.expr.clone().byres())
    }
    #[wasm_bindgen(js_name = byChain)]
    pub fn bychain(sel: &Selection) -> Selection {
        Selection::of(sel.expr.clone().bychain())
    }
    #[wasm_bindgen(js_name = byMol)]
    pub fn bymol(sel: &Selection) -> Selection {
        Selection::of(sel.expr.clone().bymol())
    }

    // -- spatial (radius in Å of an operand selection) ----------------------
    #[wasm_bindgen]
    pub fn around(sel: &Selection, radius: f64) -> Result<Selection, JsValue> {
        Ok(Selection::of(sel.expr.clone().around(radius_of(radius)?)))
    }
    #[wasm_bindgen]
    pub fn within(sel: &Selection, radius: f64) -> Result<Selection, JsValue> {
        Ok(Selection::of(sel.expr.clone().within(radius_of(radius)?)))
    }
    #[wasm_bindgen]
    pub fn expand(sel: &Selection, radius: f64) -> Result<Selection, JsValue> {
        Ok(Selection::of(sel.expr.clone().expand(radius_of(radius)?)))
    }
    #[wasm_bindgen]
    pub fn beyond(sel: &Selection, radius: f64) -> Result<Selection, JsValue> {
        Ok(Selection::of(sel.expr.clone().beyond(radius_of(radius)?)))
    }

    // -- boolean composition (JS has no operator overloading) ---------------
    #[wasm_bindgen]
    pub fn and(&self, other: &Selection) -> Selection {
        Selection::of(self.expr.clone().and(other.expr.clone()))
    }
    #[wasm_bindgen]
    pub fn or(&self, other: &Selection) -> Selection {
        Selection::of(self.expr.clone().or(other.expr.clone()))
    }
    #[wasm_bindgen]
    pub fn not(&self) -> Selection {
        Selection::of(self.expr.clone().not())
    }

    #[wasm_bindgen(js_name = toString)]
    pub fn to_js_string(&self) -> String {
        self.expr.to_string()
    }
}

/// Validate a spatial radius (non-negative and finite), or return a JS error.
fn radius_of(radius: f64) -> Result<f64, JsValue> {
    if radius.is_finite() && radius >= 0.0 {
        Ok(radius)
    } else {
        Err(JsValue::from_str(
            "radius must be a non-negative finite number",
        ))
    }
}

/// Map a comparison operator string to a `CmpOp`, or return a JS error.
fn cmp(op: &str) -> Result<CmpOp, JsValue> {
    CmpOp::parse(op)
        .ok_or_else(|| JsValue::from_str(&format!("invalid comparison operator {op:?}")))
}
