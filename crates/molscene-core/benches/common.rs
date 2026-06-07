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
