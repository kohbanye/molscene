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
use crate::structure::{vdw_radius, Atom, Structure};

const DEFAULT_STICK_RADIUS: f32 = 0.25;
const DEFAULT_SPHERE_SCALE: f32 = 1.0;

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

/// Camera framing as a bounding sphere the renderer fits to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeomCamera {
    pub center: [f32; 3],
    pub radius: f32,
}

impl Default for GeomCamera {
    fn default() -> Self {
        Self {
            center: [0.0, 0.0, 0.0],
            radius: 1.0,
        }
    }
}

/// The full draw list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeometrySpec {
    pub spheres: Spheres,
    pub cylinders: Cylinders,
    pub meshes: Vec<Mesh>,
    pub camera: GeomCamera,
    pub background: Rgb,
}

impl Default for GeometrySpec {
    fn default() -> Self {
        Self {
            spheres: Spheres::default(),
            cylinders: Cylinders::default(),
            meshes: Vec::new(),
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
                if atom.element.trim().eq_ignore_ascii_case("C") {
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

fn pos(a: &Atom) -> [f32; 3] {
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

/// Half-distance between adjacent parallel cylinders of a double bond, ×radius.
const DOUBLE_SEP: f32 = 1.0;
/// Half-distance for the outer cylinders of a triple bond, ×radius.
const TRIPLE_SEP: f32 = 1.5;
/// Radius of the sub-cylinders in a multi/aromatic bond, ×radius (so a double
/// isn't visually fatter than a single).
const MULTI_RADIUS: f32 = 0.6;
/// Inward offset of the aromatic inner-ring cylinder, ×radius.
const AROMATIC_INNER: f32 = 1.6;

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
fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    add(scale(a, 1.0 - t), scale(b, t))
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

/// Emit the cylinder(s) for one bond according to its order. `reference` orients
/// the offset of multi/aromatic bonds (ring centroid or a neighbor atom).
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
        Double => {
            let o = scale(offset_dir(pa, pb, reference), radius * DOUBLE_SEP);
            let rm = radius * MULTI_RADIUS;
            push_segment(cyl, add(pa, o), add(pb, o), rm, ca, cb);
            push_segment(cyl, sub(pa, o), sub(pb, o), rm, ca, cb);
        }
        Triple => {
            let o = scale(offset_dir(pa, pb, reference), radius * TRIPLE_SEP);
            let rm = radius * MULTI_RADIUS;
            push_segment(cyl, pa, pb, rm, ca, cb);
            push_segment(cyl, add(pa, o), add(pb, o), rm, ca, cb);
            push_segment(cyl, sub(pa, o), sub(pb, o), rm, ca, cb);
        }
        Aromatic => {
            // A full single bond plus a shorter cylinder offset toward the ring
            // interior — the classic inner-ring depiction.
            push_segment(cyl, pa, pb, radius, ca, cb);
            let o = scale(offset_dir(pa, pb, reference), radius * AROMATIC_INNER);
            let inner_start = add(lerp(pa, pb, 0.2), o);
            let inner_end = add(lerp(pa, pb, 0.8), o);
            push_segment(cyl, inner_start, inner_end, radius * MULTI_RADIUS, ca, cb);
        }
    }
}

/// Precomputed lookup for orienting multi/aromatic bond offsets: ring centroids
/// keyed by edge, plus atom positions and adjacency for the neighbor fallback.
struct BondCtx {
    positions: Vec<[f32; 3]>,
    adj: Vec<Vec<usize>>,
    centroids: Vec<[f32; 3]>,
    edge_ring: HashMap<(usize, usize), usize>,
}

impl BondCtx {
    fn new(structure: &Structure, perc: &crate::chem::Perception) -> Self {
        let positions: Vec<[f32; 3]> = structure.atoms.iter().map(pos).collect();
        let mut adj = vec![Vec::new(); structure.atoms.len()];
        for bond in &perc.bonds {
            adj[bond.a].push(bond.b);
            adj[bond.b].push(bond.a);
        }
        let mut centroids = Vec::with_capacity(perc.rings.len());
        let mut edge_ring = HashMap::new();
        for (ri, ring) in perc.rings.iter().enumerate() {
            let mut c = [0.0f32; 3];
            for &i in ring {
                c = add(c, positions[i]);
            }
            centroids.push(scale(c, 1.0 / ring.len() as f32));
            let m = ring.len();
            for k in 0..m {
                let a = ring[k];
                let b = ring[(k + 1) % m];
                edge_ring.entry((a.min(b), a.max(b))).or_insert(ri);
            }
        }
        Self {
            positions,
            adj,
            centroids,
            edge_ring,
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
                    // Each bond becomes one or more half-cylinders (split at the
                    // bond midpoint, colored by each end atom) per its order:
                    // single → 1 line, double → 2, triple → 3, aromatic → a full
                    // line plus an inner ring-ward line.
                    for bond in &perc.bonds {
                        let (i, j) = (bond.a, bond.b);
                        if !selected.contains(&i) || !selected.contains(&j) {
                            continue;
                        }
                        let a = &structure.atoms[i];
                        let b = &structure.atoms[j];
                        let (pa, pb) = (pos(a), pos(b));
                        let (ca, cb) = (color_at(i, a), color_at(j, b));
                        emit_bond(
                            &mut g.cylinders,
                            bond.order,
                            pa,
                            pb,
                            radius,
                            ca,
                            cb,
                            bond_ctx.reference(i, j),
                        );
                    }
                    // Rounded joints / lone atoms: a sphere at each selected atom.
                    for &i in &indices {
                        let a = &structure.atoms[i];
                        g.spheres.centers.push(pos(a));
                        g.spheres.radii.push(radius);
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
            }
        }

        g.camera = camera_for(structure);
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

/// Bounding-sphere camera over all atoms.
fn camera_for(structure: &Structure) -> GeomCamera {
    if structure.atoms.is_empty() {
        return GeomCamera::default();
    }
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for a in &structure.atoms {
        let p = pos(a);
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let mut radius = 0.0f32;
    for a in &structure.atoms {
        let p = pos(a);
        let d =
            ((p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2) + (p[2] - center[2]).powi(2))
                .sqrt();
        radius = radius.max(d);
    }
    GeomCamera {
        center,
        radius: radius.max(1.0) + 2.0,
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
            element: elem.into(),
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
        assert_eq!(g.spheres.colors[0], [0.2, 1.0, 0.2]); // carbon CPK
    }

    #[test]
    fn sticks_make_two_half_cylinders_per_bond_plus_caps() {
        let mut scene = two_carbons();
        scene.sticks(Expr::All, colored("element"));
        let g = scene.to_geometry();
        // one bond -> two half cylinders
        assert_eq!(g.cylinders.starts.len(), 2);
        assert_eq!(g.cylinders.radii, vec![0.25, 0.25]);
        // cap sphere at each selected atom
        assert_eq!(g.spheres.centers.len(), 2);
        assert_eq!(g.spheres.radii, vec![0.25, 0.25]);
        // half cylinder meets at the midpoint (0.75, 0, 0)
        assert_eq!(g.cylinders.ends[0], [0.75, 0.0, 0.0]);
        assert_eq!(g.cylinders.starts[1], [0.75, 0.0, 0.0]);
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
        // single → 1 line (2 half-cylinders), double → 2 lines (4),
        // triple → 3 lines (6), aromatic → full line + inner line (4).
        assert_eq!(cylinder_count_for(diatomic(Single)), 2);
        assert_eq!(cylinder_count_for(diatomic(Double)), 4);
        assert_eq!(cylinder_count_for(diatomic(Triple)), 6);
        assert_eq!(cylinder_count_for(diatomic(Aromatic)), 4);
    }

    #[test]
    fn aromatic_ring_emits_inner_ring_cylinders() {
        // A planar hexagon of carbons with explicit aromatic bonds: 6 bonds ×
        // (full line + inner line) × 2 half-cylinders = 24 cylinders.
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
        assert_eq!(cylinder_count_for(st), 24);
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
        assert_eq!(g.spheres.colors[1], [1.0, 0.3, 0.3]); // oxygen → CPK
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
