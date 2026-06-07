//! Geometry-based bond-order perception.
//!
//! This is a best-effort heuristic for *visualization*, not a chemistry engine.
//! Explicit bond orders from a file (SDF/mol2, carried on the [`Structure`])
//! always take precedence; perception only fills in orders for distance-only
//! sources (PDB/mmCIF), where no orders are available.
//!
//! The pipeline is: distance connectivity → smallest-set-of-smallest-rings
//! (SSSR) → aromatic-ring detection (size, elements, planarity) → bond orders
//! from bond length for the remaining edges. It is pure compute (no parsing, no
//! rendering) and so compiles to WASM.

use std::collections::{HashSet, VecDeque};

use crate::structure::{covalent_radius, Bond, BondOrder, Structure};

/// The perceived chemistry of a structure: ordered bonds plus the rings used to
/// orient multi-bond and aromatic depictions.
#[derive(Debug, Clone, PartialEq)]
pub struct Perception {
    /// Bonds with orders — explicit when the source carried them, otherwise
    /// perceived. Pairs are normalized to `a < b` and listed deterministically.
    pub bonds: Vec<Bond>,
    /// Rings (as ordered atom-index cycles) found in the connectivity graph,
    /// used by the geometry layer to offset bonds toward the ring interior.
    pub rings: Vec<Vec<usize>>,
}

/// Maximum out-of-plane deviation (Å) for a ring to count as planar/aromatic.
/// Ideal aromatic rings are planar to < 0.05 Å; sp3 rings deviate > 0.3 Å.
const PLANARITY_TOLERANCE: f64 = 0.10;
/// Largest ring size considered (bounds SSSR cost; ignores macrocycles).
const MAX_RING: usize = 7;

/// Resolve the ordered bonds and rings for `structure`.
///
/// Uses explicit bonds when present; otherwise perceives orders from geometry.
/// Rings are computed in both cases (the geometry layer needs ring centroids to
/// draw the aromatic inner ring).
pub fn perceive(structure: &Structure) -> Perception {
    let n = structure.atoms.len();
    let pairs = structure.bonds();
    let adj = adjacency(n, &pairs);
    let rings = sssr(n, &adj);

    let bonds = if let Some(explicit) = structure.explicit_bonds() {
        explicit.to_vec()
    } else {
        perceive_orders(structure, &pairs, &rings)
    };

    Perception { bonds, rings }
}

/// Build an undirected adjacency list from index pairs.
fn adjacency(n: usize, pairs: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut adj = vec![Vec::new(); n];
    for &(i, j) in pairs {
        adj[i].push(j);
        adj[j].push(i);
    }
    adj
}

/// Smallest-set-of-smallest-rings (approximate): for each edge, the smallest
/// cycle through it found by shortest-path-excluding-that-edge. Rings are
/// deduplicated by their vertex set and capped at [`MAX_RING`]. Returned as
/// ordered cycles so callers can compute a plane/centroid.
fn sssr(n: usize, adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut rings: Vec<Vec<usize>> = Vec::new();
    let mut seen: HashSet<Vec<usize>> = HashSet::new();
    for u in 0..n {
        for &v in &adj[u] {
            if u >= v {
                continue;
            }
            if let Some(path) = shortest_cycle(adj, u, v) {
                let size = path.len();
                if (3..=MAX_RING).contains(&size) {
                    let mut key = path.clone();
                    key.sort_unstable();
                    if seen.insert(key) {
                        rings.push(path);
                    }
                }
            }
        }
    }
    rings
}

/// Shortest path from `u` to `v` that does *not* use the direct `u–v` edge,
/// returned as the ordered vertex list `[u, …, v]` (the cycle through edge
/// `u–v`). `None` if the edge is not part of any cycle.
fn shortest_cycle(adj: &[Vec<usize>], u: usize, v: usize) -> Option<Vec<usize>> {
    let n = adj.len();
    let mut prev = vec![usize::MAX; n];
    let mut visited = vec![false; n];
    let mut queue = VecDeque::new();
    visited[u] = true;
    queue.push_back(u);
    while let Some(x) = queue.pop_front() {
        for &y in &adj[x] {
            // Forbid traversing the direct edge we're trying to close around.
            if (x == u && y == v) || (x == v && y == u) {
                continue;
            }
            if !visited[y] {
                visited[y] = true;
                prev[y] = x;
                if y == v {
                    // Reconstruct the path v -> ... -> u, then reverse.
                    let mut path = vec![v];
                    let mut cur = v;
                    while cur != u {
                        cur = prev[cur];
                        path.push(cur);
                    }
                    path.reverse();
                    return Some(path);
                }
                queue.push_back(y);
            }
        }
    }
    None
}

/// Perceive bond orders for distance-only structures: aromatic-ring edges first,
/// then bond-length heuristics for the rest. `pairs` is the connectivity
/// (already `i < j`, in deterministic order).
fn perceive_orders(
    structure: &Structure,
    pairs: &[(usize, usize)],
    rings: &[Vec<usize>],
) -> Vec<Bond> {
    let aromatic = aromatic_edges(structure, rings);
    pairs
        .iter()
        .map(|&(i, j)| {
            let order = if aromatic.contains(&(i, j)) {
                BondOrder::Aromatic
            } else {
                order_by_length(structure, i, j)
            };
            Bond { a: i, b: j, order }
        })
        .collect()
}

/// The set of edges (as `(min, max)` pairs) belonging to a perceived aromatic
/// ring.
fn aromatic_edges(structure: &Structure, rings: &[Vec<usize>]) -> HashSet<(usize, usize)> {
    let mut set = HashSet::new();
    for ring in rings {
        if is_aromatic(ring, structure) {
            let m = ring.len();
            for k in 0..m {
                let a = ring[k];
                let b = ring[(k + 1) % m];
                set.insert((a.min(b), a.max(b)));
            }
        }
    }
    set
}

/// Whether a ring is aromatic: size 5 or 6, only typical aromatic-ring elements
/// (C/N/O/S), and planar to within [`PLANARITY_TOLERANCE`].
fn is_aromatic(ring: &[usize], structure: &Structure) -> bool {
    let m = ring.len();
    if m != 5 && m != 6 {
        return false;
    }
    for &i in ring {
        if !is_aromatic_element(&structure.atoms[i].element) {
            return false;
        }
    }
    ring_max_deviation(ring, structure) < PLANARITY_TOLERANCE
}

/// Maximum out-of-plane deviation of a ring's atoms from their best-fit plane,
/// using Newell's method for a robust normal. Returns `INFINITY` for a
/// degenerate (collinear) ring so it never counts as planar.
fn ring_max_deviation(ring: &[usize], structure: &Structure) -> f64 {
    let m = ring.len();
    let p = |k: usize| {
        let a = &structure.atoms[ring[k]];
        [a.x, a.y, a.z]
    };
    // Newell normal: sum of edge cross-product contributions around the loop.
    let mut nrm = [0.0f64; 3];
    let mut centroid = [0.0f64; 3];
    for k in 0..m {
        let cur = p(k);
        let nxt = p((k + 1) % m);
        nrm[0] += (cur[1] - nxt[1]) * (cur[2] + nxt[2]);
        nrm[1] += (cur[2] - nxt[2]) * (cur[0] + nxt[0]);
        nrm[2] += (cur[0] - nxt[0]) * (cur[1] + nxt[1]);
        centroid[0] += cur[0];
        centroid[1] += cur[1];
        centroid[2] += cur[2];
    }
    let len = (nrm[0] * nrm[0] + nrm[1] * nrm[1] + nrm[2] * nrm[2]).sqrt();
    if len < 1e-9 {
        return f64::INFINITY;
    }
    let n_hat = [nrm[0] / len, nrm[1] / len, nrm[2] / len];
    let c = [
        centroid[0] / m as f64,
        centroid[1] / m as f64,
        centroid[2] / m as f64,
    ];
    let mut max_dev = 0.0f64;
    for k in 0..m {
        let q = p(k);
        let dev =
            ((q[0] - c[0]) * n_hat[0] + (q[1] - c[1]) * n_hat[1] + (q[2] - c[2]) * n_hat[2]).abs();
        max_dev = max_dev.max(dev);
    }
    max_dev
}

/// Classify a non-aromatic bond by how short it is relative to the expected
/// single-bond length (sum of covalent radii). Only [`is_multi_capable`] pairs
/// can be multiple; anything involving H (or other elements) stays single.
fn order_by_length(structure: &Structure, i: usize, j: usize) -> BondOrder {
    let a = &structure.atoms[i];
    let b = &structure.atoms[j];
    if !is_multi_capable(&a.element) || !is_multi_capable(&b.element) {
        return BondOrder::Single;
    }
    let expected = covalent_radius(&a.element) + covalent_radius(&b.element);
    if expected <= 0.0 {
        return BondOrder::Single;
    }
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let dz = a.z - b.z;
    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
    let ratio = dist / expected;
    // Tuned against typical lengths: C–C 1.54 (r≈1.01), C=C 1.34 (r≈0.88),
    // C≡C 1.20 (r≈0.79), C=O 1.23 (r≈0.87), C≡N 1.16 (r≈0.79).
    if ratio < 0.81 {
        BondOrder::Triple
    } else if ratio < 0.91 {
        BondOrder::Double
    } else {
        BondOrder::Single
    }
}

fn is_aromatic_element(element: &str) -> bool {
    matches!(
        element.trim().to_ascii_uppercase().as_str(),
        "C" | "N" | "O" | "S"
    )
}

/// Elements that can form a double/triple bond by the length heuristic. Beyond
/// the organic C/N/O/S, this includes P (phosphoryl P=O in phosphates) and the
/// other common π-forming p-block elements B/Se/As. H and metals stay single.
fn is_multi_capable(element: &str) -> bool {
    matches!(
        element.trim().to_ascii_uppercase().as_str(),
        "C" | "N" | "O" | "S" | "P" | "B" | "SE" | "AS"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::Atom;

    fn atom(element: &str, x: f64, y: f64, z: f64) -> Atom {
        Atom {
            serial: 0,
            name: element.into(),
            element: element.into(),
            residue_name: "LIG".into(),
            residue_seq: 1,
            chain_id: "A".into(),
            hetero: true,
            b_factor: 0.0,
            occupancy: 1.0,
            x,
            y,
            z,
        }
    }

    /// A planar regular hexagon of carbons with aromatic (1.39 Å) bond lengths.
    fn benzene_ring() -> Structure {
        let r = 1.39; // C–C aromatic, gives ~1.39 Å edges in a regular hexagon
        let mut atoms = Vec::new();
        for k in 0..6 {
            let t = std::f64::consts::PI / 3.0 * k as f64;
            atoms.push(atom("C", r * t.cos(), r * t.sin(), 0.0));
        }
        Structure::new(atoms)
    }

    #[test]
    fn sssr_finds_one_six_ring_for_benzene() {
        let s = benzene_ring();
        let adj = adjacency(s.atoms.len(), &s.bonds());
        let rings = sssr(s.atoms.len(), &adj);
        assert_eq!(rings.len(), 1);
        assert_eq!(rings[0].len(), 6);
    }

    #[test]
    fn benzene_perceived_aromatic() {
        let p = perceive(&benzene_ring());
        assert_eq!(p.bonds.len(), 6);
        assert!(p.bonds.iter().all(|b| b.order == BondOrder::Aromatic));
        assert_eq!(p.rings.len(), 1);
    }

    #[test]
    fn puckered_ring_is_not_aromatic() {
        // A cyclohexane-like chair: alternate atoms above/below the mean plane,
        // well beyond the planarity tolerance.
        let r = 1.45;
        let mut atoms = Vec::new();
        for k in 0..6 {
            let t = std::f64::consts::PI / 3.0 * k as f64;
            let z = if k % 2 == 0 { 0.25 } else { -0.25 };
            atoms.push(atom("C", r * t.cos(), r * t.sin(), z));
        }
        let s = Structure::new(atoms);
        let p = perceive(&s);
        assert!(p.bonds.iter().all(|b| b.order != BondOrder::Aromatic));
    }

    #[test]
    fn bond_length_classifies_double_and_triple() {
        // C–C single (1.54), C=C double (1.34), C≡C triple (1.20).
        let single = Structure::new(vec![atom("C", 0.0, 0.0, 0.0), atom("C", 1.54, 0.0, 0.0)]);
        let double = Structure::new(vec![atom("C", 0.0, 0.0, 0.0), atom("C", 1.34, 0.0, 0.0)]);
        let triple = Structure::new(vec![atom("C", 0.0, 0.0, 0.0), atom("C", 1.20, 0.0, 0.0)]);
        assert_eq!(perceive(&single).bonds[0].order, BondOrder::Single);
        assert_eq!(perceive(&double).bonds[0].order, BondOrder::Double);
        assert_eq!(perceive(&triple).bonds[0].order, BondOrder::Triple);
    }

    #[test]
    fn carbonyl_perceived_double() {
        // A C=O at 1.23 Å is well under the expected single-bond length (1.42).
        let s = Structure::new(vec![atom("C", 0.0, 0.0, 0.0), atom("O", 1.23, 0.0, 0.0)]);
        assert_eq!(perceive(&s).bonds[0].order, BondOrder::Double);
    }

    #[test]
    fn phosphoryl_perceived_double() {
        // A phosphate P=O at ~1.48 Å is under the expected single bond (~1.73),
        // so P must be multi-capable (a P–O ester at ~1.6 Å stays single).
        let dbl = Structure::new(vec![atom("P", 0.0, 0.0, 0.0), atom("O", 1.48, 0.0, 0.0)]);
        assert_eq!(perceive(&dbl).bonds[0].order, BondOrder::Double);
        let single = Structure::new(vec![atom("P", 0.0, 0.0, 0.0), atom("O", 1.6, 0.0, 0.0)]);
        assert_eq!(perceive(&single).bonds[0].order, BondOrder::Single);
    }

    #[test]
    fn hydrogen_bonds_stay_single() {
        // A short C–H must never be promoted to double/triple.
        let s = Structure::new(vec![atom("C", 0.0, 0.0, 0.0), atom("H", 0.95, 0.0, 0.0)]);
        assert_eq!(perceive(&s).bonds[0].order, BondOrder::Single);
    }

    #[test]
    fn explicit_bonds_are_used_verbatim() {
        let s = Structure::new(vec![atom("C", 0.0, 0.0, 0.0), atom("C", 1.34, 0.0, 0.0)])
            .with_bonds(vec![Bond {
                a: 0,
                b: 1,
                order: BondOrder::Aromatic,
            }]);
        // Geometry would perceive Double from the length, but the explicit
        // order wins.
        assert_eq!(perceive(&s).bonds[0].order, BondOrder::Aromatic);
    }
}
