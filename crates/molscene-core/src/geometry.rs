//! Geometry compilation: turn a declarative [`Scene`] into a renderer-neutral
//! draw list (`GeometrySpec`) of instanced spheres and cylinders.
//!
//! This is the lower-level contract consumed by the molecule-agnostic renderer
//! (Three.js today; wgpu later). It is pure compute — no pdbtbx, no rendering —
//! so it is WASM-safe. Supports spheres, sticks, cartoon ribbons, and molecular
//! surfaces (all tessellated natively).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::color::{
    chain_color, colormap_color, element_color, spectrum_color, ColorScheme, PropertyField, Rgb,
};
use crate::scene::Scene;
use crate::selection::evaluate;
use crate::spec::RepresentationKind;
use crate::structure::{vdw_radius, Atom, Element, Structure};

const DEFAULT_STICK_RADIUS: f32 = 0.25;
const DEFAULT_SPHERE_SCALE: f32 = 1.0;
/// Default radius of the cylinders in the `lines` rep — much thinner than
/// sticks, so bonds read as cheap wireframe lines.
const DEFAULT_LINE_RADIUS: f32 = 0.05;
/// Default vdW scale for the `dots` rep — small points, one per atom.
const DEFAULT_DOT_SCALE: f32 = 0.2;
/// Default font scale for the `labels` rep (the renderer maps this to a
/// readable on-screen size).
const DEFAULT_LABEL_SIZE: f32 = 1.0;

/// What text a `labels` representation writes for each atom/residue. Parsed from
/// the `text=` style string, mirroring `ColorScheme::parse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LabelText {
    /// One label per residue: `"{resn}{resi}"`, e.g. `"ALA42"` (the default).
    Residue,
    /// One label per residue: the residue name, e.g. `"ALA"`.
    Resn,
    /// One label per residue: the residue sequence number, e.g. `"42"`.
    Resi,
    /// One label per residue: the chain id, e.g. `"A"`.
    Chain,
    /// One label per atom: the atom name, e.g. `"CA"`.
    Atom,
    /// One label per atom: the element symbol, e.g. `"C"`.
    Element,
}

impl LabelText {
    /// Parse a `text=` value; an unrecognized value falls back to `Residue`
    /// (the default granularity) with a warning.
    fn parse(value: Option<&str>) -> LabelText {
        match value {
            None => LabelText::Residue,
            Some(v) => match v.trim().to_ascii_lowercase().as_str() {
                "residue" | "" => LabelText::Residue,
                "resn" | "resname" => LabelText::Resn,
                "resi" | "resid" => LabelText::Resi,
                "chain" => LabelText::Chain,
                "atom" | "name" => LabelText::Atom,
                "element" => LabelText::Element,
                other => {
                    eprintln!("molscene: unknown label text {other:?}; using residue labels.");
                    LabelText::Residue
                }
            },
        }
    }

    /// Whether this mode emits one label per residue (vs. one per atom).
    fn per_residue(self) -> bool {
        matches!(
            self,
            LabelText::Residue | LabelText::Resn | LabelText::Resi | LabelText::Chain
        )
    }
}

/// Instanced spheres.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Spheres {
    pub centers: Vec<[f32; 3]>,
    pub radii: Vec<f32>,
    pub colors: Vec<Rgb>,
}

/// Instanced cylinders (each defined by its two endpoints).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Cylinders {
    pub starts: Vec<[f32; 3]>,
    pub ends: Vec<[f32; 3]>,
    pub radii: Vec<f32>,
    pub colors: Vec<Rgb>,
}

/// A triangle mesh with per-vertex normals and colors, drawn as one group with
/// a single `opacity` (cartoon and surface each emit their own `Mesh`).
/// `indices` are triangle-list (groups of three) into the vertex arrays, which
/// all share the same length.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub colors: Vec<Rgb>,
    /// 1.0 = opaque; < 1.0 is drawn with depth-sorted transparency.
    pub opacity: f32,
}

impl Default for Mesh {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            indices: Vec::new(),
            colors: Vec::new(),
            opacity: 1.0,
        }
    }
}

/// A text annotation drawn as a camera-facing billboard (Three.js sprite) at a
/// world-space `position`. molscene picks the position, text, and color in Rust;
/// the renderer only rasterizes the glyphs. `size` is a font scale the renderer
/// turns into an on-screen size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub position: [f32; 3],
    pub text: String,
    pub color: Rgb,
    pub size: f32,
}

/// Camera framing as an oriented box the renderer fits to. `right`/`up` are the
/// screen basis (unit vectors); the view direction is `right × up`. `extent` are
/// the half-widths along `(right, up, forward)`, letting the renderer fit tightly
/// per axis (aspect-aware) instead of to a loose bounding sphere.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeomCamera {
    pub center: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub extent: [f32; 3],
}

impl Default for GeomCamera {
    fn default() -> Self {
        Self {
            center: [0.0, 0.0, 0.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            extent: [1.0, 1.0, 1.0],
        }
    }
}

/// The full draw list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeometrySpec {
    pub spheres: Spheres,
    pub cylinders: Cylinders,
    pub meshes: Vec<Mesh>,
    pub labels: Vec<Label>,
    pub camera: GeomCamera,
    pub background: Rgb,
}

impl Default for GeometrySpec {
    fn default() -> Self {
        Self {
            spheres: Spheres::default(),
            cylinders: Cylinders::default(),
            meshes: Vec::new(),
            labels: Vec::new(),
            camera: GeomCamera::default(),
            background: [1.0, 1.0, 1.0],
        }
    }
}

/// Per-structure context for resolving spectrum/chain colors.
struct ColorCtx {
    chain_index: HashMap<String, usize>,
    residue_ordinal: HashMap<(String, i32, String), usize>,
    residue_count: usize,
}

impl ColorCtx {
    fn new(structure: &Structure) -> Self {
        let mut chain_index = HashMap::new();
        for (i, c) in structure.chain_ids().into_iter().enumerate() {
            chain_index.insert(c, i);
        }
        let mut residue_ordinal = HashMap::new();
        let mut residue_count = 0;
        for a in &structure.atoms {
            let key = (a.chain_id.clone(), a.residue_seq, a.residue_name.clone());
            residue_ordinal.entry(key).or_insert_with(|| {
                let n = residue_count;
                residue_count += 1;
                n
            });
        }
        Self {
            chain_index,
            residue_ordinal,
            residue_count,
        }
    }

    fn color(&self, scheme: ColorScheme, atom: &Atom) -> Rgb {
        match scheme {
            ColorScheme::Element => element_color(&atom.element),
            ColorScheme::ElementCarbon(carbon) => {
                if atom.element == Element::C {
                    carbon
                } else {
                    element_color(&atom.element)
                }
            }
            ColorScheme::Chain => chain_color(*self.chain_index.get(&atom.chain_id).unwrap_or(&0)),
            ColorScheme::Spectrum => {
                let key = (
                    atom.chain_id.clone(),
                    atom.residue_seq,
                    atom.residue_name.clone(),
                );
                let ordinal = *self.residue_ordinal.get(&key).unwrap_or(&0);
                let t = if self.residue_count <= 1 {
                    0.0
                } else {
                    ordinal as f32 / (self.residue_count - 1) as f32
                };
                spectrum_color(t)
            }
            // Secondary-structure coloring is resolved by the cartoon module
            // (it owns per-residue SS); other representations degrade to CPK.
            ColorScheme::SecondaryStructure => element_color(&atom.element),
            ColorScheme::ByProperty { field, map, range } => {
                // An unresolved (auto) range means we never saw the atom set;
                // fall back to a flat t=0 rather than panicking.
                let (lo, hi) = range.unwrap_or((0.0, 0.0));
                let v = property_value(field, atom);
                let t = if hi > lo { (v - lo) / (hi - lo) } else { 0.0 };
                colormap_color(map, t)
            }
            ColorScheme::Fixed(rgb) => rgb,
        }
    }
}

/// The numeric value of `field` on `atom`, as an f32.
fn property_value(field: PropertyField, atom: &Atom) -> f32 {
    match field {
        PropertyField::BFactor => atom.b_factor as f32,
        PropertyField::Occupancy => atom.occupancy as f32,
    }
}

/// Fill in an auto (`None`) property range from the atoms being colored. Other
/// schemes pass through unchanged.
fn resolve_scheme(scheme: ColorScheme, structure: &Structure, indices: &[usize]) -> ColorScheme {
    match scheme {
        ColorScheme::ByProperty {
            field,
            map,
            range: None,
        } => {
            let mut lo = f32::INFINITY;
            let mut hi = f32::NEG_INFINITY;
            for &i in indices {
                let v = property_value(field, &structure.atoms[i]);
                lo = lo.min(v);
                hi = hi.max(v);
            }
            // Empty selection or all-equal values → a degenerate range; `color`
            // maps that to t=0.0.
            let range = if lo.is_finite() && hi.is_finite() {
                Some((lo, hi))
            } else {
                Some((0.0, 0.0))
            };
            ColorScheme::ByProperty { field, map, range }
        }
        other => other,
    }
}

pub(crate) fn pos(a: &Atom) -> [f32; 3] {
    [a.x as f32, a.y as f32, a.z as f32]
}

fn midpoint(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        (a[0] + b[0]) * 0.5,
        (a[1] + b[1]) * 0.5,
        (a[2] + b[2]) * 0.5,
    ]
}

// -- multi-bond geometry ----------------------------------------------------

/// Half-separation between the two equal lines of a double bond, ×radius.
/// Aromatic ring doubles orient this perpendicular toward the ring centroid.
const DOUBLE_SEP: f32 = 0.7;
/// Half-separation for the two outer lines of a triple bond, ×radius.
const TRIPLE_SEP: f32 = 1.0;
/// Thickness of each line in a double/triple bond, ×radius — thinner than a
/// single bond, and equal across the lines so a double reads as two even sticks.
const LINE_RADIUS: f32 = 0.5;
/// Atom/joint sphere radius in the sticks rep, ×radius — slightly larger than
/// the bond so atoms read as rounded balls (ball-and-stick).
const CAP_SCALE: f32 = 1.25;

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn length(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}
fn normalize(a: [f32; 3]) -> [f32; 3] {
    let len = length(a);
    if len > 1e-6 {
        scale(a, 1.0 / len)
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// A unit vector perpendicular to the `pa→pb` axis, lying in the plane defined
/// by `reference` (a ring centroid or a neighbor atom). Falls back to an
/// arbitrary perpendicular when no usable reference exists.
fn offset_dir(pa: [f32; 3], pb: [f32; 3], reference: Option<[f32; 3]>) -> [f32; 3] {
    let axis = normalize(sub(pb, pa));
    if let Some(refp) = reference {
        let v = sub(refp, pa);
        let perp = sub(v, scale(axis, dot(v, axis)));
        if length(perp) > 1e-4 {
            return normalize(perp);
        }
    }
    // Any axis not parallel to the bond, projected to a perpendicular.
    let t = if axis[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    normalize(cross(axis, t))
}

/// Push one two-color cylinder (split at the midpoint, colored `ca`/`cb`).
fn push_segment(
    cyl: &mut Cylinders,
    start: [f32; 3],
    end: [f32; 3],
    radius: f32,
    ca: Rgb,
    cb: Rgb,
) {
    let mid = midpoint(start, end);
    cyl.starts.push(start);
    cyl.ends.push(mid);
    cyl.radii.push(radius);
    cyl.colors.push(ca);
    cyl.starts.push(mid);
    cyl.ends.push(end);
    cyl.radii.push(radius);
    cyl.colors.push(cb);
}

/// One thin line parallel to the bond, shifted by `o`, at the multi-bond
/// line radius (a two-color half-cylinder pair).
fn push_offset_line(
    cyl: &mut Cylinders,
    pa: [f32; 3],
    pb: [f32; 3],
    radius: f32,
    ca: Rgb,
    cb: Rgb,
    o: [f32; 3],
) {
    push_segment(cyl, add(pa, o), add(pb, o), radius * LINE_RADIUS, ca, cb);
}

/// Emit the cylinder(s) for one bond according to its order. `reference` orients
/// the perpendicular the multi-bond lines spread along (ring centroid or a
/// neighbor atom). Aromatic bonds are resolved to `Single`/`Double` (Kekulé)
/// before this point.
#[allow(clippy::too_many_arguments)]
fn emit_bond(
    cyl: &mut Cylinders,
    order: crate::structure::BondOrder,
    pa: [f32; 3],
    pb: [f32; 3],
    radius: f32,
    ca: Rgb,
    cb: Rgb,
    reference: Option<[f32; 3]>,
) {
    use crate::structure::BondOrder::*;
    match order {
        Single => push_segment(cyl, pa, pb, radius, ca, cb),
        // Aromatic shouldn't reach here (the sticks loop maps it to a Kekulé
        // single/double), but fall back to a double if it does.
        Double | Aromatic => {
            // Two equal thin lines straddling the bond axis.
            let o = scale(offset_dir(pa, pb, reference), radius * DOUBLE_SEP);
            push_offset_line(cyl, pa, pb, radius, ca, cb, o);
            push_offset_line(cyl, pa, pb, radius, ca, cb, scale(o, -1.0));
        }
        Triple => {
            // A central line plus two equal thin lines either side.
            let o = scale(offset_dir(pa, pb, reference), radius * TRIPLE_SEP);
            push_offset_line(cyl, pa, pb, radius, ca, cb, [0.0, 0.0, 0.0]);
            push_offset_line(cyl, pa, pb, radius, ca, cb, o);
            push_offset_line(cyl, pa, pb, radius, ca, cb, scale(o, -1.0));
        }
    }
}

/// Precomputed lookup for orienting/depicting bonds: ring centroids keyed by
/// edge, atom positions and adjacency for the neighbor fallback, and the Kekulé
/// single-bond positions of aromatic rings.
struct BondCtx {
    positions: Vec<[f32; 3]>,
    adj: Vec<Vec<usize>>,
    centroids: Vec<[f32; 3]>,
    edge_ring: HashMap<(usize, usize), usize>,
    /// Aromatic ring edges that the Kekulé alternation draws as a *single* bond;
    /// the remaining aromatic edges are drawn as doubles.
    kekule_single: std::collections::HashSet<(usize, usize)>,
}

impl BondCtx {
    fn new(structure: &Structure, perc: &crate::chem::Perception) -> Self {
        let positions: Vec<[f32; 3]> = structure.atoms.iter().map(pos).collect();
        let mut adj = vec![Vec::new(); structure.atoms.len()];
        let mut order_of = HashMap::new();
        for bond in &perc.bonds {
            adj[bond.a].push(bond.b);
            adj[bond.b].push(bond.a);
            order_of.insert((bond.a.min(bond.b), bond.a.max(bond.b)), bond.order);
        }
        let mut centroids = Vec::with_capacity(perc.rings.len());
        let mut edge_ring = HashMap::new();
        let mut kekule_single = std::collections::HashSet::new();
        for (ri, ring) in perc.rings.iter().enumerate() {
            let mut c = [0.0f32; 3];
            for &i in ring {
                c = add(c, positions[i]);
            }
            centroids.push(scale(c, 1.0 / ring.len() as f32));
            let m = ring.len();
            let edge = |k: usize| {
                let (a, b) = (ring[k], ring[(k + 1) % m]);
                (a.min(b), a.max(b))
            };
            for k in 0..m {
                edge_ring.entry(edge(k)).or_insert(ri);
            }
            // A fully-aromatic ring is drawn as alternating single/double bonds
            // (Kekulé): odd-position edges become singles, the rest doubles. This
            // is exact for even rings (e.g. benzene); odd rings get an asymmetric
            // split ((m-1)/2 singles vs (m+1)/2 doubles) — acceptable for the rare
            // odd aromatic ring (e.g. cyclopentadienyl).
            let all_aromatic = (0..m)
                .all(|k| order_of.get(&edge(k)) == Some(&crate::structure::BondOrder::Aromatic));
            if all_aromatic {
                for k in (1..m).step_by(2) {
                    kekule_single.insert(edge(k));
                }
            }
        }
        Self {
            positions,
            adj,
            centroids,
            edge_ring,
            kekule_single,
        }
    }

    /// The reference point used to orient bond `i–j`'s offset: its ring centroid
    /// if the bond is in a ring, otherwise a neighboring atom, else `None`.
    fn reference(&self, i: usize, j: usize) -> Option<[f32; 3]> {
        if let Some(&ri) = self.edge_ring.get(&(i.min(j), i.max(j))) {
            return Some(self.centroids[ri]);
        }
        if let Some(&k) = self.adj[i].iter().find(|&&k| k != j) {
            return Some(self.positions[k]);
        }
        if let Some(&k) = self.adj[j].iter().find(|&&k| k != i) {
            return Some(self.positions[k]);
        }
        None
    }

    /// Whether an aromatic edge is drawn as a single bond by the Kekulé
    /// alternation. Aromatic edges not on a detected ring default to a double.
    fn aromatic_is_single(&self, i: usize, j: usize) -> bool {
        self.kekule_single.contains(&(i.min(j), i.max(j)))
    }
}

fn scheme_of(style: &crate::spec::Style) -> ColorScheme {
    style
        .color
        .as_deref()
        .map(ColorScheme::parse)
        .unwrap_or(ColorScheme::Element)
}

impl Scene {
    /// Compile the scene into a renderer-neutral draw list.
    pub fn to_geometry(&self) -> GeometrySpec {
        let mut g = GeometrySpec::default();
        if let Some(bg) = self.background_color() {
            g.background = bg;
        }
        let Some(structure) = self.structure() else {
            eprintln!("molscene: scene has no loaded coordinates; nothing to render.");
            return g;
        };

        let ctx = ColorCtx::new(structure);
        let overrides = self.color_overrides(structure);
        // Perceived bonds + rings, computed lazily on the first sticks rep so
        // cartoon-/surface-only scenes don't pay for ring perception.
        let mut perception: Option<crate::chem::Perception> = None;

        for rep in self.representations() {
            let indices = evaluate(structure, &rep.selection);
            // The representation's base scheme, with any auto property range
            // resolved over the atoms it colors.
            let base = resolve_scheme(scheme_of(&rep.style), structure, &indices);
            // An explicit `set_color` override wins over the base scheme.
            // `overrides` is empty when no `set_color` was used.
            let color_at = |i: usize, a: &Atom| match overrides.get(i).copied().flatten() {
                Some(ov) => ctx.color(ov, a),
                None => ctx.color(base, a),
            };
            match rep.kind {
                RepresentationKind::Spheres => {
                    let scale = rep.style.scale.unwrap_or(DEFAULT_SPHERE_SCALE);
                    for &i in &indices {
                        let a = &structure.atoms[i];
                        g.spheres.centers.push(pos(a));
                        g.spheres.radii.push(vdw_radius(&a.element) * scale);
                        g.spheres.colors.push(color_at(i, a));
                    }
                }
                RepresentationKind::Sticks => {
                    let radius = rep.style.radius.unwrap_or(DEFAULT_STICK_RADIUS);
                    let selected: std::collections::HashSet<usize> =
                        indices.iter().copied().collect();
                    let perc: &crate::chem::Perception =
                        perception.get_or_insert_with(|| crate::chem::perceive(structure));
                    let bond_ctx = BondCtx::new(structure, perc);
                    // Each bond becomes half-cylinders (split at the midpoint,
                    // colored by each end atom): single → 1 line, double → 2,
                    // triple → 3. Aromatic rings are drawn Kekulé-style, each
                    // edge resolved to a single or an inner-offset double.
                    for bond in &perc.bonds {
                        let (i, j) = (bond.a, bond.b);
                        if !selected.contains(&i) || !selected.contains(&j) {
                            continue;
                        }
                        let order = match bond.order {
                            crate::structure::BondOrder::Aromatic => {
                                if bond_ctx.aromatic_is_single(i, j) {
                                    crate::structure::BondOrder::Single
                                } else {
                                    crate::structure::BondOrder::Double
                                }
                            }
                            other => other,
                        };
                        let a = &structure.atoms[i];
                        let b = &structure.atoms[j];
                        let (pa, pb) = (pos(a), pos(b));
                        let (ca, cb) = (color_at(i, a), color_at(j, b));
                        emit_bond(
                            &mut g.cylinders,
                            order,
                            pa,
                            pb,
                            radius,
                            ca,
                            cb,
                            bond_ctx.reference(i, j),
                        );
                    }
                    // Rounded joints / lone atoms: a sphere (slightly larger than
                    // the bond) at each selected atom — a ball-and-stick look.
                    for &i in &indices {
                        let a = &structure.atoms[i];
                        g.spheres.centers.push(pos(a));
                        g.spheres.radii.push(radius * CAP_SCALE);
                        g.spheres.colors.push(color_at(i, a));
                    }
                }
                RepresentationKind::Cartoon => {
                    // Resolve per-residue color with full precedence: an explicit
                    // `set_color` override wins, then the base scheme — where
                    // `secondary_structure` maps the residue's assigned SS to the
                    // cartoon palette (the only place SS is known).
                    let cartoon_color = |i: usize, a: &Atom, ss: crate::structure::Ss| -> Rgb {
                        match overrides.get(i).copied().flatten() {
                            Some(ColorScheme::SecondaryStructure) => crate::cartoon::ss_color(ss),
                            Some(ov) => ctx.color(ov, a),
                            None => match base {
                                ColorScheme::SecondaryStructure => crate::cartoon::ss_color(ss),
                                other => ctx.color(other, a),
                            },
                        }
                    };
                    let params = crate::cartoon::CartoonParams {
                        color_fn: &cartoon_color,
                    };
                    let mut mesh = crate::geometry::Mesh::default();
                    crate::cartoon::build_cartoon(
                        structure,
                        &indices,
                        rep.style.radius,
                        &params,
                        &mut mesh,
                    );
                    if !mesh.positions.is_empty() {
                        mesh.opacity = rep.style.opacity.unwrap_or(1.0);
                        g.meshes.push(mesh);
                    }
                }
                RepresentationKind::Surface => {
                    // Color each surface vertex by its nearest selected atom,
                    // with the same `set_color`-override-then-base precedence as
                    // the other representations.
                    let surface_color = |i: usize, a: &Atom| color_at(i, a);
                    let params = crate::surface::SurfaceParams {
                        color_fn: &surface_color,
                        probe: crate::surface::DEFAULT_PROBE,
                        resolution: rep.style.radius,
                        opacity: rep.style.opacity.unwrap_or(1.0),
                    };
                    crate::surface::build_surface(structure, &indices, &params, &mut g.meshes);
                }
                RepresentationKind::Lines => {
                    // Like sticks (bond-order aware, two-color split per bond),
                    // but thinner and without the ball-and-stick atom caps.
                    let radius = rep.style.radius.unwrap_or(DEFAULT_LINE_RADIUS);
                    let selected: std::collections::HashSet<usize> =
                        indices.iter().copied().collect();
                    let perc: &crate::chem::Perception =
                        perception.get_or_insert_with(|| crate::chem::perceive(structure));
                    let bond_ctx = BondCtx::new(structure, perc);
                    for bond in &perc.bonds {
                        let (i, j) = (bond.a, bond.b);
                        if !selected.contains(&i) || !selected.contains(&j) {
                            continue;
                        }
                        let order = match bond.order {
                            crate::structure::BondOrder::Aromatic => {
                                if bond_ctx.aromatic_is_single(i, j) {
                                    crate::structure::BondOrder::Single
                                } else {
                                    crate::structure::BondOrder::Double
                                }
                            }
                            other => other,
                        };
                        let a = &structure.atoms[i];
                        let b = &structure.atoms[j];
                        let (pa, pb) = (pos(a), pos(b));
                        let (ca, cb) = (color_at(i, a), color_at(j, b));
                        emit_bond(
                            &mut g.cylinders,
                            order,
                            pa,
                            pb,
                            radius,
                            ca,
                            cb,
                            bond_ctx.reference(i, j),
                        );
                    }
                    // No atom caps: that's what distinguishes lines from sticks.
                }
                RepresentationKind::Dots => {
                    // A small sphere per atom — like spheres, just scaled down.
                    let scale = rep.style.scale.unwrap_or(DEFAULT_DOT_SCALE);
                    for &i in &indices {
                        let a = &structure.atoms[i];
                        g.spheres.centers.push(pos(a));
                        g.spheres.radii.push(vdw_radius(&a.element) * scale);
                        g.spheres.colors.push(color_at(i, a));
                    }
                }
                RepresentationKind::Labels => {
                    let mode = LabelText::parse(rep.style.text.as_deref());
                    let size = rep.style.scale.unwrap_or(DEFAULT_LABEL_SIZE);
                    // Labels default to black for readability; an explicit
                    // `color=` runs through the normal scheme machinery, and a
                    // `set_color` override still wins (via `overrides`).
                    let label_base = match rep.style.color.as_deref().map(ColorScheme::parse) {
                        Some(scheme) => resolve_scheme(scheme, structure, &indices),
                        None => ColorScheme::Fixed([0.0, 0.0, 0.0]),
                    };
                    let label_color = |i: usize, a: &Atom| match overrides.get(i).copied().flatten()
                    {
                        Some(ov) => ctx.color(ov, a),
                        None => ctx.color(label_base, a),
                    };
                    if mode.per_residue() {
                        // Group selected atoms by residue, preserving first-seen
                        // order. The representative atom is the residue's Cα when
                        // selected, else its first selected atom.
                        let mut order: Vec<(String, i32, String)> = Vec::new();
                        let mut groups: HashMap<(String, i32, String), Vec<usize>> = HashMap::new();
                        for &i in &indices {
                            let a = &structure.atoms[i];
                            let key = (a.chain_id.clone(), a.residue_seq, a.residue_name.clone());
                            groups
                                .entry(key.clone())
                                .or_insert_with(|| {
                                    order.push(key.clone());
                                    Vec::new()
                                })
                                .push(i);
                        }
                        for key in &order {
                            let members = &groups[key];
                            let rep_atom = members
                                .iter()
                                .copied()
                                .find(|&i| structure.atoms[i].name.trim() == "CA")
                                .unwrap_or(members[0]);
                            let a = &structure.atoms[rep_atom];
                            let text = match mode {
                                LabelText::Residue => {
                                    format!("{}{}", a.residue_name.trim(), a.residue_seq)
                                }
                                LabelText::Resn => a.residue_name.trim().to_string(),
                                LabelText::Resi => a.residue_seq.to_string(),
                                LabelText::Chain => a.chain_id.trim().to_string(),
                                _ => unreachable!("per-residue mode"),
                            };
                            g.labels.push(Label {
                                position: pos(a),
                                text,
                                color: label_color(rep_atom, a),
                                size,
                            });
                        }
                    } else {
                        for &i in &indices {
                            let a = &structure.atoms[i];
                            let text = match mode {
                                LabelText::Atom => a.name.trim().to_string(),
                                LabelText::Element => a.element.symbol().to_string(),
                                _ => unreachable!("per-atom mode"),
                            };
                            g.labels.push(Label {
                                position: pos(a),
                                text,
                                color: label_color(i, a),
                                size,
                            });
                        }
                    }
                }
            }
        }

        // Camera framing: evaluate the optional center/orient selections, then
        // compute an oriented box the renderer fits to.
        let cam = self.camera();
        let center_idx = cam.center.as_ref().map(|e| evaluate(structure, e));
        let orient_idx = cam.orient.as_ref().map(|e| evaluate(structure, e));
        g.camera = crate::camera::frame(structure, center_idx.as_deref(), orient_idx.as_deref());
        g
    }

    /// Compile and serialize the draw list to JSON.
    pub fn to_geometry_json(&self) -> String {
        serde_json::to_string(&self.to_geometry()).expect("GeometrySpec serializes")
    }

    /// A per-atom explicit-color override map built from the scene's
    /// `set_color` assignments, applied in order (last write wins). `None` means
    /// the atom keeps whatever scheme its representation uses.
    fn color_overrides(&self, structure: &Structure) -> Vec<Option<ColorScheme>> {
        if self.color_assignments().is_empty() {
            return Vec::new();
        }
        let mut map = vec![None; structure.atoms.len()];
        for assignment in self.color_assignments() {
            let scheme = ColorScheme::parse(&assignment.color);
            let indices = evaluate(structure, &assignment.selection);
            let scheme = resolve_scheme(scheme, structure, &indices);
            for i in indices {
                map[i] = Some(scheme);
            }
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::Expr;
    use crate::spec::Style;

    /// A style with just a `color` set.
    fn colored(color: &str) -> Style {
        Style {
            color: Some(color.into()),
            ..Default::default()
        }
    }

    fn atom(serial: usize, name: &str, elem: &str, x: f64, y: f64, z: f64) -> Atom {
        Atom {
            serial,
            name: name.into(),
            element: Element::from_symbol(elem),
            residue_name: "LIG".into(),
            residue_seq: 1,
            chain_id: "A".into(),
            hetero: false,
            b_factor: 0.0,
            occupancy: 1.0,
            x,
            y,
            z,
        }
    }

    fn two_carbons() -> Scene {
        let st = Structure::new(vec![
            atom(1, "C1", "C", 0.0, 0.0, 0.0),
            atom(2, "C2", "C", 1.5, 0.0, 0.0),
        ]);
        Scene::from_rcsb("test").with_structure(st)
    }

    #[test]
    fn spheres_one_per_atom_with_vdw_radius() {
        let mut scene = two_carbons();
        scene.spheres(Expr::All, colored("element"));
        let g = scene.to_geometry();
        assert_eq!(g.spheres.centers.len(), 2);
        assert_eq!(g.spheres.radii, vec![1.70, 1.70]);
        assert_eq!(g.spheres.colors[0], [0.30, 0.85, 0.30]); // carbon CPK
    }

    #[test]
    fn sticks_make_two_half_cylinders_per_bond_plus_caps() {
        let mut scene = two_carbons();
        scene.sticks(Expr::All, colored("element"));
        let g = scene.to_geometry();
        // one bond -> two half cylinders
        assert_eq!(g.cylinders.starts.len(), 2);
        assert_eq!(g.cylinders.radii, vec![0.25, 0.25]);
        // cap sphere at each selected atom, slightly larger than the bond
        assert_eq!(g.spheres.centers.len(), 2);
        assert_eq!(g.spheres.radii, vec![0.3125, 0.3125]); // 0.25 * CAP_SCALE
                                                           // half cylinder meets at the midpoint (0.75, 0, 0)
        assert_eq!(g.cylinders.ends[0], [0.75, 0.0, 0.0]);
        assert_eq!(g.cylinders.starts[1], [0.75, 0.0, 0.0]);
    }

    #[test]
    fn lines_make_cylinders_per_bond_without_caps() {
        let mut scene = two_carbons();
        scene.lines(Expr::All, colored("element"));
        let g = scene.to_geometry();
        // one bond -> two half cylinders, thinner than sticks
        assert_eq!(g.cylinders.starts.len(), 2);
        assert_eq!(
            g.cylinders.radii,
            vec![DEFAULT_LINE_RADIUS, DEFAULT_LINE_RADIUS]
        );
        // unlike sticks, lines emit no ball-and-stick atom caps
        assert!(g.spheres.centers.is_empty());
    }

    #[test]
    fn lines_reflect_bond_order() {
        use crate::structure::BondOrder::*;
        let lines_cyl = |order| {
            let mut scene = Scene::from_rcsb("test").with_structure(diatomic(order));
            scene.lines(Expr::All, colored("element"));
            scene.to_geometry().cylinders.starts.len()
        };
        // single → 1 line (2 half-cylinders), double → 2 lines (4), triple → 3 (6)
        assert_eq!(lines_cyl(Single), 2);
        assert_eq!(lines_cyl(Double), 4);
        assert_eq!(lines_cyl(Triple), 6);
    }

    #[test]
    fn dots_one_small_sphere_per_atom() {
        let mut scene = two_carbons();
        scene.dots(Expr::All, colored("element"));
        let g = scene.to_geometry();
        let r = vdw_radius(&Element::C) * DEFAULT_DOT_SCALE;
        assert_eq!(g.spheres.centers.len(), 2);
        assert_eq!(g.spheres.radii, vec![r, r]);
        // dots are points only — no bond cylinders
        assert!(g.cylinders.starts.is_empty());
    }

    /// A style selecting the labels `text=` mode.
    fn labeled(text: &str) -> Style {
        Style {
            text: Some(text.into()),
            ..Default::default()
        }
    }

    #[test]
    fn labels_default_to_one_black_residue_label() {
        // backbone(3) is 3 residues (ALA 1..3) of N/CA/C/O.
        let mut scene = Scene::from_rcsb("test").with_structure(backbone(3));
        scene.labels(Expr::All, Style::default());
        let g = scene.to_geometry();
        assert_eq!(g.labels.len(), 3);
        // Default text = "{resn}{resi}" placed at the residue's Cα; default color
        // is black for readability.
        let l = &g.labels[0];
        assert_eq!(l.text, "ALA1");
        assert_eq!(l.color, [0.0, 0.0, 0.0]);
        assert_eq!(l.size, DEFAULT_LABEL_SIZE);
        // Cα of residue 1 sits at x = 1.0 (see the backbone fixture).
        assert_eq!(l.position, [1.0, 0.3, 0.0]);
        assert_eq!(g.labels[2].text, "ALA3");
    }

    #[test]
    fn labels_atom_mode_is_one_per_atom() {
        let mut scene = Scene::from_rcsb("test").with_structure(backbone(2));
        scene.labels(Expr::All, labeled("atom"));
        let g = scene.to_geometry();
        // 2 residues * 4 atoms (N/CA/C/O).
        assert_eq!(g.labels.len(), 8);
        let names: Vec<&str> = g.labels.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(&names[..4], &["N", "CA", "C", "O"]);
    }

    #[test]
    fn labels_element_mode_uses_symbols() {
        let mut scene = two_carbons();
        scene.labels(Expr::All, labeled("element"));
        let g = scene.to_geometry();
        assert_eq!(g.labels.len(), 2);
        assert!(g.labels.iter().all(|l| l.text == "C"));
    }

    #[test]
    fn labels_color_and_size_styles_apply() {
        let mut scene = Scene::from_rcsb("test").with_structure(backbone(1));
        scene.labels(
            Expr::All,
            Style {
                color: Some("red".into()),
                scale: Some(2.0),
                ..Default::default()
            },
        );
        let g = scene.to_geometry();
        assert_eq!(g.labels.len(), 1);
        assert_eq!(g.labels[0].color, [1.0, 0.0, 0.0]);
        assert_eq!(g.labels[0].size, 2.0);
    }

    fn cylinder_count_for(structure: Structure) -> usize {
        let mut scene = Scene::from_rcsb("test").with_structure(structure);
        scene.sticks(Expr::All, colored("element"));
        scene.to_geometry().cylinders.starts.len()
    }

    fn diatomic(order: crate::structure::BondOrder) -> Structure {
        Structure::new(vec![
            atom(1, "C1", "C", 0.0, 0.0, 0.0),
            atom(2, "C2", "C", 1.4, 0.0, 0.0),
        ])
        .with_bonds(vec![crate::structure::Bond { a: 0, b: 1, order }])
    }

    #[test]
    fn bond_order_drives_cylinder_count() {
        use crate::structure::BondOrder::*;
        // single → 1 line (2 half-cylinders), double → 2 lines (4), triple → 3
        // lines (6). A lone aromatic bond (no ring) defaults to a double (4).
        assert_eq!(cylinder_count_for(diatomic(Single)), 2);
        assert_eq!(cylinder_count_for(diatomic(Double)), 4);
        assert_eq!(cylinder_count_for(diatomic(Triple)), 6);
        assert_eq!(cylinder_count_for(diatomic(Aromatic)), 4);
    }

    #[test]
    fn aromatic_ring_is_drawn_kekule() {
        // A hexagon of carbons with explicit aromatic bonds is drawn as
        // alternating single/double (Kekulé): 3 singles (2 cyl) + 3 doubles
        // (4 cyl) = 6 + 12 = 18 cylinders.
        use crate::structure::{Bond, BondOrder};
        let r = 1.39;
        let mut atoms = Vec::new();
        let mut bonds = Vec::new();
        for k in 0..6 {
            let t = std::f64::consts::PI / 3.0 * k as f64;
            atoms.push(atom(k + 1, "C", "C", r * t.cos(), r * t.sin(), 0.0));
        }
        for k in 0..6 {
            bonds.push(Bond {
                a: k,
                b: (k + 1) % 6,
                order: BondOrder::Aromatic,
            });
        }
        let st = Structure::new(atoms).with_bonds(bonds);
        assert_eq!(cylinder_count_for(st), 18);
    }

    #[test]
    fn empty_when_no_structure() {
        let mut scene = Scene::from_rcsb("test");
        scene.spheres(Expr::All, Style::default());
        let g = scene.to_geometry();
        assert!(g.spheres.centers.is_empty());
    }

    #[test]
    fn cartoon_without_backbone_emits_nothing() {
        // The two-carbon fixture has no Cα backbone, so cartoon traces nothing
        // and no mesh group is pushed.
        let mut scene = two_carbons();
        scene.cartoon(Expr::All, Style::default());
        let g = scene.to_geometry();
        assert!(g.spheres.centers.is_empty());
        assert!(g.cylinders.starts.is_empty());
        assert!(g.meshes.is_empty());
    }

    #[test]
    fn surface_emits_a_mesh_group() {
        let mut scene = two_carbons();
        scene.surface(
            Expr::All,
            Style {
                opacity: Some(0.3),
                ..Default::default()
            },
        );
        let g = scene.to_geometry();
        assert_eq!(g.meshes.len(), 1);
        let m = &g.meshes[0];
        assert!(!m.positions.is_empty());
        assert_eq!(m.positions.len(), m.normals.len());
        assert_eq!(m.positions.len(), m.colors.len());
        assert_eq!(m.indices.len() % 3, 0);
        assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
        assert_eq!(m.opacity, 0.3);
    }

    /// A minimal protein backbone: `n` residues of N/CA/C/O laid out along x.
    fn backbone(n: usize) -> Structure {
        let mut atoms = Vec::new();
        let mut serial = 1;
        for k in 0..n {
            let base = k as f64 * 3.8;
            for (name, dx, dy) in [
                ("N", 0.0, 0.0),
                ("CA", 1.0, 0.3),
                ("C", 2.0, 0.0),
                ("O", 2.2, 0.6),
            ] {
                let mut a = atom(serial, name, "C", base + dx, dy, 0.0);
                a.residue_name = "ALA".into();
                a.residue_seq = k as i32 + 1;
                atoms.push(a);
                serial += 1;
            }
        }
        Structure::new(atoms)
    }

    #[test]
    fn cartoon_emits_a_mesh_for_a_backbone() {
        let scene = Scene::from_rcsb("test").with_structure(backbone(6));
        let mut scene = scene;
        scene.cartoon(Expr::All, colored("spectrum"));
        let g = scene.to_geometry();
        assert_eq!(g.meshes.len(), 1);
        let m = &g.meshes[0];
        assert!(!m.positions.is_empty());
        assert_eq!(m.positions.len(), m.colors.len());
        assert_eq!(m.indices.len() % 3, 0);
    }

    #[test]
    fn cartoon_set_color_overrides_ss_coloring() {
        // `set_color` must win over the cartoon's secondary-structure coloring
        // for the overridden residue (and only that residue).
        let mut scene = Scene::from_rcsb("test").with_structure(backbone(6));
        scene
            .cartoon(Expr::All, colored("secondary_structure"))
            .set_color(Expr::resi(2, 2), "red");
        let g = scene.to_geometry();
        let red = [1.0, 0.0, 0.0];
        let colors = &g.meshes[0].colors;
        assert!(colors.contains(&red), "override color must appear");
        // Other residues keep an SS palette color, not red everywhere.
        assert!(colors.iter().any(|c| *c != red));
    }

    #[test]
    fn geometry_snapshot() {
        let mut scene = two_carbons();
        scene
            .spheres(Expr::All, colored("element"))
            .sticks(Expr::All, colored("element"));
        insta::assert_json_snapshot!(scene.to_geometry());
    }

    fn atom_b(serial: usize, b: f64, x: f64) -> Atom {
        let mut a = atom(serial, "C", "C", x, 0.0, 0.0);
        a.b_factor = b;
        a
    }

    #[test]
    fn property_coloring_spans_colormap_over_selection() {
        use crate::color::{colormap_color, Colormap};
        let st = Structure::new(vec![
            atom_b(1, 10.0, 0.0),
            atom_b(2, 50.0, 1.5),
            atom_b(3, 90.0, 3.0),
        ]);
        let mut scene = Scene::from_rcsb("test").with_structure(st);
        scene.spheres(Expr::All, colored("bfactor"));
        let g = scene.to_geometry();
        // Auto range is [10, 90] over the colored atoms: ends hit the colormap
        // endpoints, the middle lands at t=0.5.
        assert_eq!(g.spheres.colors[0], colormap_color(Colormap::Viridis, 0.0));
        assert_eq!(g.spheres.colors[1], colormap_color(Colormap::Viridis, 0.5));
        assert_eq!(g.spheres.colors[2], colormap_color(Colormap::Viridis, 1.0));
        assert_ne!(g.spheres.colors[0], g.spheres.colors[2]);
    }

    #[test]
    fn element_carbon_recolors_only_carbons() {
        let st = Structure::new(vec![
            atom(1, "C1", "C", 0.0, 0.0, 0.0),
            atom(2, "O1", "O", 1.2, 0.0, 0.0),
        ]);
        let mut scene = Scene::from_rcsb("test").with_structure(st);
        scene.spheres(Expr::All, colored("element:cyan"));
        let g = scene.to_geometry();
        assert_eq!(g.spheres.colors[0], [0.0, 1.0, 1.0]); // carbon → cyan
        assert_eq!(g.spheres.colors[1], [0.90, 0.20, 0.20]); // oxygen → CPK
    }

    #[test]
    fn set_color_override_beats_base_scheme() {
        let mut a1 = atom(1, "C1", "C", 0.0, 0.0, 0.0);
        a1.residue_seq = 1;
        let mut a2 = atom(2, "C2", "C", 1.5, 0.0, 0.0);
        a2.residue_seq = 2;
        let st = Structure::new(vec![a1, a2]);
        let mut scene = Scene::from_rcsb("test").with_structure(st);
        scene.spheres(Expr::All, colored("grey"));
        scene.set_color(Expr::resi(1, 1), "red");
        let g = scene.to_geometry();
        assert_eq!(g.spheres.colors[0], [1.0, 0.0, 0.0]); // overridden
        assert_eq!(g.spheres.colors[1], [0.5, 0.5, 0.5]); // base grey
    }
}
