//! Camera framing: turn a structure (and optional center / orient selections)
//! into an oriented bounding box the renderer fits to (`GeomCamera`).
//!
//! Pure compute — only `Vec`/arrays, no parsing or rendering — so it is
//! WASM-safe. The framing fits an axis-aligned box by default, or a box aligned
//! to a selection's principal axes (PCA) when an `orient` selection is given.

use crate::geometry::{pos, GeomCamera};
use crate::structure::Structure;

/// Minimum half-width and additive padding applied to each axis so tightly
/// clustered atoms (or a single atom) still get a sane, slightly padded frame.
/// Matches the spirit of the old bounding-sphere `radius.max(1.0) + 2.0`.
const MIN_EXTENT: f32 = 1.0;
const PADDING: f32 = 2.0;

/// Compute the camera framing.
///
/// - `center_idx` — if given (and non-empty), frame these atoms; else fall back
///   to the orient selection, else all atoms.
/// - `orient_idx` — if given with ≥ 2 atoms, orient the view along their
///   principal axes (longest spread horizontal); else keep the identity basis.
pub fn frame(
    structure: &Structure,
    center_idx: Option<&[usize]>,
    orient_idx: Option<&[usize]>,
) -> GeomCamera {
    // Atoms to frame: explicit center selection, else the orient selection,
    // else all atoms.
    let all: Vec<usize>;
    let frame_idx: &[usize] = match (center_idx, orient_idx) {
        (Some(c), _) if !c.is_empty() => c,
        (_, Some(o)) if !o.is_empty() => o,
        _ => {
            all = (0..structure.atoms.len()).collect();
            &all
        }
    };
    if frame_idx.is_empty() {
        return GeomCamera::default();
    }

    // Orientation basis: principal axes of the orient selection, else identity.
    let (right, up, forward) = match orient_idx {
        Some(o) if o.len() >= 2 => principal_axes(structure, o),
        _ => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    };

    // Project the framed atoms onto the basis and take the oriented AABB.
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for &i in frame_idx {
        let p = pos(&structure.atoms[i]);
        let proj = [dot(p, right), dot(p, up), dot(p, forward)];
        for k in 0..3 {
            min[k] = min[k].min(proj[k]);
            max[k] = max[k].max(proj[k]);
        }
    }
    // Box midpoint in basis coords, mapped back to world.
    let mid = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let center = add3(
        add3(scale3(right, mid[0]), scale3(up, mid[1])),
        scale3(forward, mid[2]),
    );
    let extent = [
        ((max[0] - min[0]) * 0.5).max(MIN_EXTENT) + PADDING,
        ((max[1] - min[1]) * 0.5).max(MIN_EXTENT) + PADDING,
        ((max[2] - min[2]) * 0.5).max(MIN_EXTENT) + PADDING,
    ];
    GeomCamera {
        center,
        right,
        up,
        extent,
    }
}

/// Principal axes of a point set via PCA, returned as `(right, up, forward)`
/// unit vectors ordered by descending variance: the longest spread becomes the
/// screen-x (`right`) axis, the next `up`, and the view direction `forward` is
/// `right × up` (kept right-handed).
fn principal_axes(structure: &Structure, idx: &[usize]) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let n = idx.len() as f64;
    let mut c = [0.0f64; 3];
    for &i in idx {
        let p = pos(&structure.atoms[i]);
        c = [c[0] + p[0] as f64, c[1] + p[1] as f64, c[2] + p[2] as f64];
    }
    c = [c[0] / n, c[1] / n, c[2] / n];

    // Symmetric covariance matrix (the 1/n factor is irrelevant to eigenvectors).
    let mut cov = [[0.0f64; 3]; 3];
    for &i in idx {
        let p = pos(&structure.atoms[i]);
        let d = [p[0] as f64 - c[0], p[1] as f64 - c[1], p[2] as f64 - c[2]];
        for (r, dr) in d.iter().enumerate() {
            for (col, dc) in d.iter().enumerate() {
                cov[r][col] += dr * dc;
            }
        }
    }

    let (vals, vecs) = jacobi_eigen(cov);
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| vals[b].partial_cmp(&vals[a]).unwrap());
    let axis = |k: usize| {
        let v = vecs[order[k]];
        normalize([v[0] as f32, v[1] as f32, v[2] as f32])
    };
    let right = axis(0);
    let mut up = axis(1);
    let forward = normalize(cross(right, up));
    // Re-orthogonalize up against (right, forward) to guarantee a clean frame.
    up = normalize(cross(forward, right));
    (right, up, forward)
}

/// Jacobi eigenvalue iteration for a symmetric 3×3 matrix. Returns the
/// eigenvalues and eigenvectors, where `vecs[k]` is the unit eigenvector for
/// `vals[k]`.
fn jacobi_eigen(mut a: [[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    // V accumulates the rotations; its columns become the eigenvectors.
    let mut v = [[0.0f64; 3]; 3];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for _ in 0..50 {
        let off = a[0][1].abs() + a[0][2].abs() + a[1][2].abs();
        if off < 1e-12 {
            break;
        }
        for (p, q) in [(0usize, 1usize), (0, 2), (1, 2)] {
            let apq = a[p][q];
            if apq.abs() < 1e-18 {
                continue;
            }
            // Rotation that zeroes a[p][q]. Treat theta == 0 (equal diagonal
            // entries) as positive so we still get the 45° rotation that zeroes
            // the off-diagonal — `signum` would hand back -1.0 for a -0.0 theta.
            let theta = (a[q][q] - a[p][p]) / (2.0 * apq);
            let sign = if theta >= 0.0 { 1.0 } else { -1.0 };
            let t = sign / (theta.abs() + (theta * theta + 1.0).sqrt());
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            let r = 3 - p - q; // the remaining index
            let (app, aqq, arp, arq) = (a[p][p], a[q][q], a[r][p], a[r][q]);
            a[p][p] = app - t * apq;
            a[q][q] = aqq + t * apq;
            a[p][q] = 0.0;
            a[q][p] = 0.0;
            a[r][p] = c * arp - s * arq;
            a[p][r] = a[r][p];
            a[r][q] = s * arp + c * arq;
            a[q][r] = a[r][q];
            for row in v.iter_mut() {
                let (vp, vq) = (row[p], row[q]);
                row[p] = c * vp - s * vq;
                row[q] = s * vp + c * vq;
            }
        }
    }
    let vals = [a[0][0], a[1][1], a[2][2]];
    // Columns of V are the eigenvectors; return them as rows for indexing.
    let vecs = [
        [v[0][0], v[1][0], v[2][0]],
        [v[0][1], v[1][1], v[2][1]],
        [v[0][2], v[1][2], v[2][2]],
    ];
    (vals, vecs)
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

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale3(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let len = dot(a, a).sqrt();
    if len < 1e-9 {
        [0.0, 0.0, 0.0]
    } else {
        [a[0] / len, a[1] / len, a[2] / len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::{Atom, Element, Structure};

    fn atom(x: f64, y: f64, z: f64) -> Atom {
        Atom {
            serial: 1,
            name: "C".into(),
            element: Element::from_symbol("C"),
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

    fn structure(points: &[[f64; 3]]) -> Structure {
        Structure::new(points.iter().map(|p| atom(p[0], p[1], p[2])).collect())
    }

    #[test]
    fn empty_structure_gives_default_camera() {
        let st = Structure::new(vec![]);
        assert_eq!(frame(&st, None, None), GeomCamera::default());
    }

    #[test]
    fn default_frame_is_axis_aligned_box_over_all_atoms() {
        // Two atoms 1.5 apart on x — mirrors the geometry snapshot.
        let st = structure(&[[0.0, 0.0, 0.0], [1.5, 0.0, 0.0]]);
        let cam = frame(&st, None, None);
        assert_eq!(cam.center, [0.75, 0.0, 0.0]);
        assert_eq!(cam.right, [1.0, 0.0, 0.0]);
        assert_eq!(cam.up, [0.0, 1.0, 0.0]);
        // half-width 0.75 -> max(1.0) + 2.0 = 3.0 on x; degenerate axes -> 3.0.
        assert_eq!(cam.extent, [3.0, 3.0, 3.0]);
    }

    #[test]
    fn center_selection_frames_only_those_atoms() {
        let st = structure(&[
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [20.0, 0.0, 0.0],
            [30.0, 0.0, 0.0],
        ]);
        let all = frame(&st, None, None);
        // Center on just the first two atoms.
        let centered = frame(&st, Some(&[0, 1]), None);
        assert_eq!(centered.center, [5.0, 0.0, 0.0]);
        // Framing a subset is tighter than framing everything.
        assert!(centered.extent[0] < all.extent[0]);
    }

    #[test]
    fn orient_handles_equal_variance_diagonal_cloud() {
        // Points along the (1,1,0) diagonal: the covariance has equal diagonal
        // entries (xx == yy) and a non-zero xy, so the Jacobi pivot hits
        // theta == 0 — the case the rotation must still resolve.
        let st = structure(&[
            [-3.0, -3.0, 0.0],
            [-1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [3.0, 3.0, 0.0],
        ]);
        let idx: Vec<usize> = (0..4).collect();
        let cam = frame(&st, None, Some(&idx));
        // The principal axis is the (1,1,0) diagonal; `right` aligns with it.
        let inv_sqrt2 = 1.0 / 2.0_f32.sqrt();
        assert!(
            (cam.right[0].abs() - inv_sqrt2).abs() < 1e-3
                && (cam.right[1].abs() - inv_sqrt2).abs() < 1e-3
                && cam.right[2].abs() < 1e-3,
            "right should lie on the (1,1,0) diagonal: {:?}",
            cam.right
        );
    }

    #[test]
    fn orient_aligns_longest_axis_to_screen_right() {
        // Cloud stretched far along +y, less along +x, least along +z.
        let st = structure(&[
            [0.0, -50.0, 0.0],
            [0.0, 50.0, 0.0],
            [-10.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, 0.0, -2.0],
            [0.0, 0.0, 2.0],
        ]);
        let idx: Vec<usize> = (0..6).collect();
        let cam = frame(&st, None, Some(&idx));
        // The longest spread (world +y) should map onto `right` (screen-x).
        assert!(
            cam.right[1].abs() > 0.99,
            "right should align with y: {:?}",
            cam.right
        );
        // And the box's largest extent is along right (after padding it dominates).
        assert!(cam.extent[0] > cam.extent[1]);
        assert!(cam.extent[1] > cam.extent[2]);
        // Orthonormal, right-handed basis.
        assert!((dot(cam.right, cam.up)).abs() < 1e-4);
        let fwd = cross(cam.right, cam.up);
        let recovered = cross(cam.right, cam.up);
        assert!((dot(fwd, recovered) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn jacobi_recovers_known_eigenvalues() {
        // Diagonal matrix: eigenvalues are the diagonal, vectors the axes.
        let (vals, _vecs) = jacobi_eigen([[3.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 2.0]]);
        let mut sorted = vals;
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((sorted[0] - 1.0).abs() < 1e-9);
        assert!((sorted[1] - 2.0).abs() < 1e-9);
        assert!((sorted[2] - 3.0).abs() < 1e-9);
    }
}
