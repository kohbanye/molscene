//! Shared bench fixtures. Included via `#[path = "common.rs"] mod common;` from
//! each benchmark (so it is not itself a separate bench target).

use molscene_core::{Atom, Element, Structure};

/// A deterministic cubic carbon lattice of about `n` atoms, spaced ~1.5 Å so
/// each atom bonds to its axis-aligned neighbors (~3 bonds/atom) — a realistic
/// molecular density that is fully reproducible across runs.
pub fn lattice(n: usize) -> Structure {
    // Smallest cube side whose volume covers n atoms.
    let side = (n as f64).cbrt().ceil() as usize;
    const SPACING: f64 = 1.5;
    let mut atoms = Vec::with_capacity(n);
    let mut serial = 1usize;
    'outer: for ix in 0..side {
        for iy in 0..side {
            for iz in 0..side {
                atoms.push(Atom {
                    serial,
                    name: "C".into(),
                    element: Element::C,
                    residue_name: "LIG".into(),
                    residue_seq: 1,
                    chain_id: "A".into(),
                    hetero: false,
                    b_factor: 0.0,
                    occupancy: 1.0,
                    x: ix as f64 * SPACING,
                    y: iy as f64 * SPACING,
                    z: iz as f64 * SPACING,
                });
                serial += 1;
                if atoms.len() == n {
                    break 'outer;
                }
            }
        }
    }
    Structure::new(atoms)
}

/// Like [`lattice`], but with a tiny deterministic per-atom jitter so no two
/// atoms share an exact axis coordinate. A perfect grid puts hundreds of atoms
/// on the same plane, which overflows the k-d tree's leaf buckets at build time;
/// real PDB coordinates (floats) never collide like that. Use this for spatial
/// selection benches, which build the tree.
// `common.rs` is shared via `#[path]` by every bench; not all of them use this.
#[allow(dead_code)]
pub fn lattice_jittered(n: usize) -> Structure {
    let mut s = lattice(n);
    for (i, a) in s.atoms.iter_mut().enumerate() {
        // Distinct offsets per atom (sub-0.05 Å so neighbor relationships and
        // bond inference are unchanged), derived from the index — no RNG dep.
        let j = i as f64;
        a.x += (j * 0.0011).fract() * 0.04;
        a.y += (j * 0.0023).fract() * 0.04;
        a.z += (j * 0.0037).fract() * 0.04;
        // Sequential residue ids let a bench pick a small seed set (a short
        // `resi` range) without touching coordinates — so the spatial bench can
        // exercise the case where the one-time tree build dominates each call.
        a.residue_seq = i as i32 + 1;
    }
    s
}
