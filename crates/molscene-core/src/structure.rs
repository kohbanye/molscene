//! Structure data model: a flat list of atoms with the residue/chain identity
//! each atom belongs to. Deliberately simple for v0.1 — the renderer parses
//! geometry itself; this model exists to back `ms.load()` introspection and the
//! Rust selection evaluator (v0.2).
//!
//! This module is parser-agnostic: `parse.rs` (pdbtbx) maps into these types so
//! the rest of the core never depends on pdbtbx directly.

/// A single atom.
#[derive(Debug, Clone, PartialEq)]
pub struct Atom {
    /// Serial number from the source file.
    pub serial: usize,
    /// Atom name, e.g. `"CA"`.
    pub name: String,
    /// Element symbol, e.g. `"C"`.
    pub element: String,
    /// Residue (component) name, e.g. `"ALA"` / `"HOH"`.
    pub residue_name: String,
    /// Residue sequence number.
    pub residue_seq: i32,
    /// Chain identifier, e.g. `"A"`.
    pub chain_id: String,
    /// `true` for `HETATM` records (ligands, water, ions).
    pub hetero: bool,
    /// Temperature (B) factor; drives `b` predicates and B-factor coloring.
    pub b_factor: f64,
    /// Occupancy; drives `q` predicates.
    pub occupancy: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Secondary-structure class for a residue. Filled either from a file's
/// `HELIX`/`SHEET` annotations (preferred) or computed geometrically by the
/// cartoon module when no annotation is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ss {
    Helix,
    Sheet,
    Loop,
}

/// Chemical bond order, carried on a [`Bond`]. `Aromatic` is a distinct class
/// (not a Kekulé double) so the renderer can draw the inner-ring depiction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BondOrder {
    Single,
    Double,
    Triple,
    Aromatic,
}

/// A bond between two atoms (by index, `a < b`) with its chemical order.
///
/// Bond orders come either from an explicit source (SDF/mol2, stored on the
/// [`Structure`]) or from geometry-based perception (`crate::chem`). They never
/// enter the serialized `GeometrySpec` — the geometry layer turns each order
/// into the right set of cylinders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bond {
    pub a: usize,
    pub b: usize,
    pub order: BondOrder,
}

/// A parsed molecular structure.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Structure {
    pub atoms: Vec<Atom>,
    /// Per-residue secondary structure from file annotations, keyed by
    /// `(chain_id, residue_seq)`. Empty when the source had no `HELIX`/`SHEET`
    /// records — in that case the cartoon builder computes SS geometrically.
    /// Only `Helix`/`Sheet` residues are stored; anything absent is `Loop`.
    ss: std::collections::HashMap<(String, i32), Ss>,
    /// Explicit bonds with orders, present only when the source carried them
    /// (SDF/mol2). `None` for distance-only sources (PDB/mmCIF), where bond
    /// orders are perceived from geometry at compile time instead.
    explicit_bonds: Option<Vec<Bond>>,
}

impl Structure {
    pub fn new(atoms: Vec<Atom>) -> Self {
        Self {
            atoms,
            ss: std::collections::HashMap::new(),
            explicit_bonds: None,
        }
    }

    /// Attach explicit bonds with orders (from an SDF/mol2 import). Normalizes
    /// each pair to `a < b` so connectivity matches the distance-inferred order.
    ///
    /// # Panics
    /// Panics if any bond is degenerate (`a == b`) or references an atom index
    /// out of range — these would otherwise survive into the geometry/adjacency
    /// pass and panic far from the cause.
    pub fn with_bonds(mut self, bonds: Vec<Bond>) -> Self {
        let n = self.atoms.len();
        self.explicit_bonds = Some(
            bonds
                .into_iter()
                .map(|b| {
                    assert!(
                        b.a != b.b && b.a < n && b.b < n,
                        "with_bonds: invalid Bond {{ a: {}, b: {} }} for a structure with {n} atoms",
                        b.a,
                        b.b,
                    );
                    Bond {
                        a: b.a.min(b.b),
                        b: b.a.max(b.b),
                        order: b.order,
                    }
                })
                .collect(),
        );
        self
    }

    /// The structure's explicit bonds, if the source provided them.
    pub fn explicit_bonds(&self) -> Option<&[Bond]> {
        self.explicit_bonds.as_deref()
    }

    /// Whether the structure carries file-provided secondary-structure
    /// annotations (i.e. the source had `HELIX`/`SHEET` records).
    pub fn has_ss_annotations(&self) -> bool {
        !self.ss.is_empty()
    }

    /// The annotated secondary structure for a residue, if any. Residues that
    /// fall outside every annotated range return `None` (treated as `Loop`).
    pub fn ss_at(&self, chain: &str, seq: i32) -> Option<Ss> {
        self.ss.get(&(chain.to_string(), seq)).copied()
    }

    /// Record an annotated secondary structure for a residue. Called by the
    /// parser when reading `HELIX`/`SHEET` records.
    pub fn set_ss(&mut self, chain: impl Into<String>, seq: i32, ss: Ss) {
        self.ss.insert((chain.into(), seq), ss);
    }

    /// Number of atoms.
    pub fn num_atoms(&self) -> usize {
        self.atoms.len()
    }

    /// Distinct chain identifiers, in first-seen order.
    pub fn chain_ids(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for a in &self.atoms {
            if !seen.contains(&a.chain_id) {
                seen.push(a.chain_id.clone());
            }
        }
        seen
    }

    /// Number of distinct chains.
    pub fn num_chains(&self) -> usize {
        self.chain_ids().len()
    }

    /// Number of distinct residues, keyed by (chain, residue_seq, residue_name).
    pub fn num_residues(&self) -> usize {
        let mut seen: Vec<(String, i32, String)> = Vec::new();
        for a in &self.atoms {
            let key = (a.chain_id.clone(), a.residue_seq, a.residue_name.clone());
            if !seen.contains(&key) {
                seen.push(key);
            }
        }
        seen.len()
    }

    /// Number of hetero (`HETATM`) atoms.
    pub fn num_hetero(&self) -> usize {
        self.atoms.iter().filter(|a| a.hetero).count()
    }

    /// Connectivity as index pairs with `i < j`.
    ///
    /// When the structure carries explicit bonds (SDF/mol2), returns those
    /// pairs. Otherwise infers covalent bonds from interatomic distances: two
    /// atoms are bonded when their distance is below the sum of their covalent
    /// radii plus a tolerance, and above a small floor (to reject
    /// duplicate/overlapping atoms).
    ///
    /// O(n²) for now — fine for typical single structures (~hundreds to a few
    /// thousand atoms); a spatial grid can replace this for very large inputs.
    pub fn bonds(&self) -> Vec<(usize, usize)> {
        if let Some(bonds) = &self.explicit_bonds {
            return bonds.iter().map(|b| (b.a, b.b)).collect();
        }
        const TOLERANCE: f64 = 0.45;
        const FLOOR_SQ: f64 = 0.16; // (0.4 Å)²
        let mut bonds = Vec::new();
        for i in 0..self.atoms.len() {
            let a = &self.atoms[i];
            let ra = covalent_radius(&a.element);
            for j in (i + 1)..self.atoms.len() {
                let b = &self.atoms[j];
                let cutoff = ra + covalent_radius(&b.element) + TOLERANCE;
                let dx = a.x - b.x;
                let dy = a.y - b.y;
                let dz = a.z - b.z;
                let d2 = dx * dx + dy * dy + dz * dz;
                if d2 > FLOOR_SQ && d2 < cutoff * cutoff {
                    bonds.push((i, j));
                }
            }
        }
        bonds
    }
}

/// Van der Waals radius (Å) for an element symbol; falls back to carbon's.
pub fn vdw_radius(element: &str) -> f32 {
    match normalize(element).as_str() {
        "H" => 1.10,
        "C" => 1.70,
        "N" => 1.55,
        "O" => 1.52,
        "S" => 1.80,
        "P" => 1.80,
        "F" => 1.47,
        "CL" => 1.75,
        "BR" => 1.85,
        "I" => 1.98,
        "FE" => 1.80,
        "ZN" => 1.39,
        "MG" => 1.73,
        "NA" => 2.27,
        "CA" => 2.31,
        "K" => 2.75,
        _ => 1.70,
    }
}

/// Covalent radius (Å) for an element symbol (Cordero 2008); fallback ~carbon.
pub fn covalent_radius(element: &str) -> f64 {
    match normalize(element).as_str() {
        "H" => 0.31,
        "C" => 0.76,
        "N" => 0.71,
        "O" => 0.66,
        "S" => 1.05,
        "P" => 1.07,
        "F" => 0.57,
        "CL" => 1.02,
        "BR" => 1.20,
        "I" => 1.39,
        "FE" => 1.32,
        "ZN" => 1.22,
        "MG" => 1.41,
        "NA" => 1.66,
        "CA" => 1.76,
        "K" => 2.03,
        _ => 0.77,
    }
}

fn normalize(element: &str) -> String {
    element.trim().to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carbon(serial: usize, x: f64, y: f64, z: f64) -> Atom {
        Atom {
            serial,
            name: "C".into(),
            element: "C".into(),
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

    #[test]
    fn bonds_by_distance() {
        // C1-C2 = 1.5 Å (bond), C2-C3 = 1.5 Å (bond), C1-C3 = 3.0 Å (no bond).
        let s = Structure::new(vec![
            carbon(1, 0.0, 0.0, 0.0),
            carbon(2, 1.5, 0.0, 0.0),
            carbon(3, 3.0, 0.0, 0.0),
        ]);
        assert_eq!(s.bonds(), vec![(0, 1), (1, 2)]);
    }

    #[test]
    fn explicit_bonds_drive_connectivity() {
        // Three collinear carbons would distance-infer C0-C1 and C1-C2, but an
        // explicit bond list (here only C0-C2) overrides that connectivity.
        let s = Structure::new(vec![
            carbon(1, 0.0, 0.0, 0.0),
            carbon(2, 1.5, 0.0, 0.0),
            carbon(3, 3.0, 0.0, 0.0),
        ])
        .with_bonds(vec![Bond {
            a: 2,
            b: 0,
            order: BondOrder::Double,
        }]);
        // Pair is normalized to (a < b) and replaces distance inference.
        assert_eq!(s.bonds(), vec![(0, 2)]);
        assert_eq!(s.explicit_bonds().unwrap()[0].order, BondOrder::Double);
    }

    #[test]
    #[should_panic(expected = "invalid Bond")]
    fn with_bonds_rejects_out_of_range_endpoint() {
        Structure::new(vec![carbon(1, 0.0, 0.0, 0.0)]).with_bonds(vec![Bond {
            a: 0,
            b: 5,
            order: BondOrder::Single,
        }]);
    }

    #[test]
    #[should_panic(expected = "invalid Bond")]
    fn with_bonds_rejects_self_bond() {
        Structure::new(vec![carbon(1, 0.0, 0.0, 0.0), carbon(2, 1.5, 0.0, 0.0)]).with_bonds(vec![
            Bond {
                a: 1,
                b: 1,
                order: BondOrder::Single,
            },
        ]);
    }

    #[test]
    fn overlapping_atoms_do_not_bond() {
        let s = Structure::new(vec![carbon(1, 0.0, 0.0, 0.0), carbon(2, 0.05, 0.0, 0.0)]);
        assert!(s.bonds().is_empty());
    }

    #[test]
    fn ss_annotations_default_empty_and_round_trip() {
        let mut s = Structure::new(vec![carbon(1, 0.0, 0.0, 0.0)]);
        assert!(!s.has_ss_annotations());
        assert_eq!(s.ss_at("A", 1), None);
        s.set_ss("A", 1, Ss::Helix);
        s.set_ss("A", 2, Ss::Sheet);
        assert!(s.has_ss_annotations());
        assert_eq!(s.ss_at("A", 1), Some(Ss::Helix));
        assert_eq!(s.ss_at("A", 2), Some(Ss::Sheet));
        assert_eq!(s.ss_at("A", 3), None); // unannotated → Loop
        assert_eq!(s.ss_at("B", 1), None); // wrong chain
    }

    #[test]
    fn radii_lookup_with_fallback() {
        assert_eq!(vdw_radius("C"), 1.70);
        assert_eq!(vdw_radius("o"), 1.52); // case-insensitive
        assert_eq!(vdw_radius("Xx"), 1.70); // fallback
        assert_eq!(covalent_radius("O"), 0.66);
    }
}
