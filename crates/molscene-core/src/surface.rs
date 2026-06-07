//! Approximate solvent-excluded surface (SES) via a voxel grid + Euclidean
//! distance transform + surface nets. Pure compute, WASM-safe (no parsing, no
//! filesystem, no network), like the rest of the geometry pipeline.
//!
//! The surface is built in three steps:
//!
//! 1. Rasterize the solvent-**accessible** solid onto a grid: a voxel is "inside"
//!    if it lies within `r_i + probe` of any selected atom (`r_i` = vdW radius).
//! 2. Erode that solid by the probe radius. We compute, for every accessible
//!    voxel, the Euclidean distance `D` to the nearest non-accessible voxel (a
//!    separable Felzenszwalb–Huttenlocher distance transform), then take the
//!    signed field `φ = probe − D` (negative deep inside; the isosurface sits
//!    where `D = probe`). The eroded solid `{φ ≤ 0}` is the SES.
//! 3. Mesh `φ = 0` with `fast-surface-nets` and color each vertex by its nearest
//!    selected atom.
//!
//! Why the distance transform is required (and a per-sphere min-distance SDF is
//! not enough): eroding `min_i(|p − c_i| − (r_i + probe))` by the probe collapses
//! exactly back to the union of vdW spheres — a per-sphere minimum can only ever
//! produce locally convex isosurfaces, so it cannot represent the concave
//! *re-entrant* patches in the grooves between atoms. Those patches are a
//! non-local feature of where the probe *cannot* fit, i.e. of the complement of
//! the accessible solid. The grid EDT measures distance into that complement and
//! recovers them. `SurfaceParams` is the seam where an analytic/Gaussian backend
//! could later swap in.

use fast_surface_nets::ndshape::RuntimeShape;
use fast_surface_nets::{surface_nets, SurfaceNetsBuffer};
use kiddo::{KdTree, SquaredEuclidean};

use crate::color::Rgb;
use crate::geometry::Mesh;
use crate::structure::{vdw_radius, Atom, Structure};

/// Default solvent probe radius (Å), the conventional water radius.
pub const DEFAULT_PROBE: f32 = 1.4;
/// Default grid spacing (Å) when the style does not override it.
const DEFAULT_H: f32 = 0.7;
/// Upper bound on total grid cells; the spacing is coarsened to fit within it so
/// large structures stay bounded in time and memory.
const MAX_CELLS: usize = 12_000_000;
/// Empty-voxel border on every side so the isosurface never touches (and so the
/// EDT always has a background seed at) the grid boundary. Must be ≥ 1.
const PAD_VOXELS: usize = 2;

/// A sentinel "very large" squared distance used to seed the EDT.
const INF: f32 = 1.0e20;

/// Tunables for the surface, and the seam for a future analytic backend.
pub struct SurfaceParams<'a> {
    /// Color a vertex given its nearest selected atom (index + atom).
    pub color_fn: &'a dyn Fn(usize, &Atom) -> Rgb,
    /// Solvent probe radius in Å.
    pub probe: f32,
    /// Grid spacing in Å; `None` uses [`DEFAULT_H`].
    pub resolution: Option<f32>,
    /// Mesh opacity (1.0 = opaque).
    pub opacity: f32,
}

/// A uniform voxel grid: `dim` cells, spacing `h`, with cell `(0,0,0)`'s center
/// at `origin`.
struct Grid {
    origin: [f32; 3],
    h: f32,
    dim: [usize; 3],
}

impl Grid {
    #[inline]
    fn index(&self, i: usize, j: usize, k: usize) -> usize {
        // Matches `RuntimeShape::<u32,3>`: x fastest, then y, then z.
        (k * self.dim[1] + j) * self.dim[0] + i
    }

    #[inline]
    fn world(&self, i: f32, j: f32, k: f32) -> [f32; 3] {
        [
            self.origin[0] + i * self.h,
            self.origin[1] + j * self.h,
            self.origin[2] + k * self.h,
        ]
    }

    fn n_cells(&self) -> usize {
        self.dim[0] * self.dim[1] * self.dim[2]
    }
}

/// Build a grid covering the selected atoms (plus their radii and a padding
/// border), coarsening the spacing to stay within the cell budget. Returns
/// `None` for an empty selection.
fn build_grid(
    structure: &Structure,
    selected: &[usize],
    probe: f32,
    resolution: Option<f32>,
) -> Option<Grid> {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    let mut any = false;
    for &ai in selected {
        let a = &structure.atoms[ai];
        let p = [a.x as f32, a.y as f32, a.z as f32];
        let r = vdw_radius(&a.element) + probe;
        for d in 0..3 {
            lo[d] = lo[d].min(p[d] - r);
            hi[d] = hi[d].max(p[d] + r);
        }
        any = true;
    }
    if !any {
        return None;
    }

    let mut h = resolution.unwrap_or(DEFAULT_H).max(0.1);
    loop {
        let dim = [
            ((hi[0] - lo[0]) / h).ceil() as usize + 1 + 2 * PAD_VOXELS,
            ((hi[1] - lo[1]) / h).ceil() as usize + 1 + 2 * PAD_VOXELS,
            ((hi[2] - lo[2]) / h).ceil() as usize + 1 + 2 * PAD_VOXELS,
        ];
        let cells = dim[0] * dim[1] * dim[2];
        if cells <= MAX_CELLS {
            let origin = [
                lo[0] - PAD_VOXELS as f32 * h,
                lo[1] - PAD_VOXELS as f32 * h,
                lo[2] - PAD_VOXELS as f32 * h,
            ];
            return Some(Grid { origin, h, dim });
        }
        // Coarsen so the cell count drops under budget (cbrt of the overshoot),
        // with a margin to converge despite the per-axis `ceil`.
        h *= (cells as f32 / MAX_CELLS as f32).cbrt() * 1.05;
    }
}

/// Mark voxels inside the union of `(r_i + probe)` spheres of the selected atoms.
/// Each atom only touches the local cell box within its own radius.
fn rasterize_accessible(
    structure: &Structure,
    selected: &[usize],
    probe: f32,
    grid: &Grid,
) -> Vec<bool> {
    let mut acc = vec![false; grid.n_cells()];
    for &ai in selected {
        let a = &structure.atoms[ai];
        let c = [a.x as f32, a.y as f32, a.z as f32];
        let r = vdw_radius(&a.element) + probe;
        let r2 = r * r;
        let mut cell_lo = [0usize; 3];
        let mut cell_hi = [0usize; 3];
        for d in 0..3 {
            let lo = ((c[d] - r - grid.origin[d]) / grid.h).floor();
            let hi = ((c[d] + r - grid.origin[d]) / grid.h).ceil();
            cell_lo[d] = lo.max(0.0) as usize;
            cell_hi[d] = (hi.max(0.0) as usize).min(grid.dim[d] - 1);
        }
        for k in cell_lo[2]..=cell_hi[2] {
            for j in cell_lo[1]..=cell_hi[1] {
                for i in cell_lo[0]..=cell_hi[0] {
                    let p = grid.world(i as f32, j as f32, k as f32);
                    let d2 = (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2);
                    if d2 <= r2 {
                        acc[grid.index(i, j, k)] = true;
                    }
                }
            }
        }
    }
    acc
}

/// 1-D squared Euclidean distance transform of a sampled function `f` (the
/// Felzenszwalb–Huttenlocher lower-envelope algorithm). Returns, for each `q`,
/// `min_p (q − p)² + f[p]`.
fn edt_1d(f: &[f32]) -> Vec<f32> {
    let n = f.len();
    let mut d = vec![0.0f32; n];
    if n == 0 {
        return d;
    }
    let mut v = vec![0usize; n]; // locations of parabolas in the lower envelope
    let mut z = vec![0.0f32; n + 1]; // boundaries between parabolas
    let mut k = 0usize;
    v[0] = 0;
    z[0] = -INF;
    z[1] = INF;
    for q in 1..n {
        // Intersection of the parabola from q with the rightmost one in the hull.
        let mut s = ((f[q] + (q * q) as f32) - (f[v[k]] + (v[k] * v[k]) as f32))
            / (2.0 * (q as f32 - v[k] as f32));
        while s <= z[k] {
            k -= 1; // z[0] = -INF guarantees this never underflows
            s = ((f[q] + (q * q) as f32) - (f[v[k]] + (v[k] * v[k]) as f32))
                / (2.0 * (q as f32 - v[k] as f32));
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = INF;
    }
    k = 0;
    for (q, dq_out) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f32 - p as f32;
        *dq_out = dq * dq + f[p];
    }
    d
}

/// Squared Euclidean distance (in voxel units) from each accessible voxel to the
/// nearest non-accessible voxel, via three separable 1-D passes.
fn edt_squared(acc: &[bool], dim: [usize; 3]) -> Vec<f32> {
    let (nx, ny, nz) = (dim[0], dim[1], dim[2]);
    let idx = |i: usize, j: usize, k: usize| (k * ny + j) * nx + i;
    // Seed: background (non-accessible) voxels are at distance 0; accessible
    // voxels start at +∞ and are pulled down toward the nearest background.
    let mut g: Vec<f32> = acc
        .iter()
        .map(|&inside| if inside { INF } else { 0.0 })
        .collect();

    let mut line_x = vec![0.0f32; nx];
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                line_x[i] = g[idx(i, j, k)];
            }
            let d = edt_1d(&line_x);
            for i in 0..nx {
                g[idx(i, j, k)] = d[i];
            }
        }
    }
    let mut line_y = vec![0.0f32; ny];
    for k in 0..nz {
        for i in 0..nx {
            for j in 0..ny {
                line_y[j] = g[idx(i, j, k)];
            }
            let d = edt_1d(&line_y);
            for j in 0..ny {
                g[idx(i, j, k)] = d[j];
            }
        }
    }
    let mut line_z = vec![0.0f32; nz];
    for j in 0..ny {
        for i in 0..nx {
            for k in 0..nz {
                line_z[k] = g[idx(i, j, k)];
            }
            let d = edt_1d(&line_z);
            for k in 0..nz {
                g[idx(i, j, k)] = d[k];
            }
        }
    }
    g
}

/// Build an approximate SES mesh for `selected` atoms and push it (as one group
/// carrying `params.opacity`) onto `out`. Pushes nothing for an empty selection
/// or when the isosurface does not intersect the grid.
pub fn build_surface(
    structure: &Structure,
    selected: &[usize],
    params: &SurfaceParams,
    out: &mut Vec<Mesh>,
) {
    let Some(grid) = build_grid(structure, selected, params.probe, params.resolution) else {
        return;
    };

    let acc = rasterize_accessible(structure, selected, params.probe, &grid);
    let d2 = edt_squared(&acc, grid.dim);

    // Signed field: φ = probe − D·h. Negative inside the eroded (SES) solid; the
    // isosurface is where the distance into the accessible solid equals one
    // probe radius. Non-accessible voxels have D = 0, so φ = probe > 0 (outside).
    let phi: Vec<f32> = d2
        .iter()
        .map(|&dd| params.probe - dd.sqrt() * grid.h)
        .collect();

    let [nx, ny, nz] = grid.dim;
    let shape = RuntimeShape::<u32, 3>::new([nx as u32, ny as u32, nz as u32]);
    let mut buf = SurfaceNetsBuffer::default();
    surface_nets(
        &phi,
        &shape,
        [0, 0, 0],
        [nx as u32 - 1, ny as u32 - 1, nz as u32 - 1],
        &mut buf,
    );
    if buf.positions.is_empty() {
        return;
    }

    // Voxel coordinates → world; normalize surface-nets' (unnormalized) normals.
    let mut positions = Vec::with_capacity(buf.positions.len());
    let mut normals = Vec::with_capacity(buf.positions.len());
    for (p, n) in buf.positions.iter().zip(&buf.normals) {
        positions.push(grid.world(p[0], p[1], p[2]));
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        normals.push(if len > 1.0e-6 {
            [n[0] / len, n[1] / len, n[2] / len]
        } else {
            [0.0, 0.0, 1.0]
        });
    }

    // Color each vertex by its nearest selected atom.
    let mut tree: KdTree<f32, 3> = KdTree::with_capacity(selected.len());
    for &ai in selected {
        let a = &structure.atoms[ai];
        tree.add(&[a.x as f32, a.y as f32, a.z as f32], ai as u64);
    }
    let colors = positions
        .iter()
        .map(|p| {
            let nn = tree.nearest_one::<SquaredEuclidean>(p);
            let ai = nn.item as usize;
            (params.color_fn)(ai, &structure.atoms[ai])
        })
        .collect();

    out.push(Mesh {
        positions,
        normals,
        indices: buf.indices,
        colors,
        opacity: params.opacity,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carbon(serial: usize, x: f64, y: f64, z: f64) -> Atom {
        Atom {
            serial,
            name: "C".into(),
            element: crate::structure::Element::C,
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

    fn grey(_i: usize, _a: &Atom) -> Rgb {
        [0.5, 0.5, 0.5]
    }

    fn params<'a>(color_fn: &'a dyn Fn(usize, &Atom) -> Rgb, opacity: f32) -> SurfaceParams<'a> {
        SurfaceParams {
            color_fn,
            probe: DEFAULT_PROBE,
            // A coarse grid keeps the test fast while still producing a mesh.
            resolution: Some(1.0),
            opacity,
        }
    }

    #[test]
    fn two_atoms_produce_a_well_formed_mesh() {
        let st = Structure::new(vec![carbon(1, 0.0, 0.0, 0.0), carbon(2, 1.4, 0.0, 0.0)]);
        let mut out = Vec::new();
        build_surface(&st, &[0, 1], &params(&grey, 1.0), &mut out);

        assert_eq!(out.len(), 1);
        let m = &out[0];
        assert!(!m.positions.is_empty());
        assert_eq!(m.positions.len(), m.normals.len());
        assert_eq!(m.positions.len(), m.colors.len());
        assert_eq!(m.indices.len() % 3, 0);
        assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
        // Normals are normalized.
        for n in &m.normals {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((len - 1.0).abs() < 1.0e-4);
        }
    }

    #[test]
    fn empty_selection_emits_nothing() {
        let st = Structure::new(vec![carbon(1, 0.0, 0.0, 0.0)]);
        let mut out = Vec::new();
        build_surface(&st, &[], &params(&grey, 1.0), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn opacity_is_carried_onto_the_mesh() {
        let st = Structure::new(vec![carbon(1, 0.0, 0.0, 0.0)]);
        let mut out = Vec::new();
        build_surface(&st, &[0], &params(&grey, 0.3), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].opacity, 0.3);
    }

    #[test]
    fn vertices_are_colored_by_nearest_atom() {
        // Atom 0 (left) red, atom 1 (right) blue: both colors should appear on a
        // surface that wraps both atoms.
        let st = Structure::new(vec![carbon(1, -2.0, 0.0, 0.0), carbon(2, 2.0, 0.0, 0.0)]);
        let color_fn = |i: usize, _a: &Atom| {
            if i == 0 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 0.0, 1.0]
            }
        };
        let mut out = Vec::new();
        build_surface(&st, &[0, 1], &params(&color_fn, 1.0), &mut out);
        let colors = &out[0].colors;
        assert!(
            colors.contains(&[1.0, 0.0, 0.0]),
            "left atom's color must appear"
        );
        assert!(
            colors.contains(&[0.0, 0.0, 1.0]),
            "right atom's color must appear"
        );
    }
}
