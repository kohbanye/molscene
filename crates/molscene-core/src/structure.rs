//! Structure data model: a flat list of atoms with the residue/chain identity
//! each atom belongs to. Deliberately simple for v0.1 — the renderer parses
//! geometry itself; this model exists to back `ms.load()` introspection and the
//! Rust selection evaluator (v0.2).
//!
//! This module is parser-agnostic: `parse.rs` (pdbtbx) maps into these types so
//! the rest of the core never depends on pdbtbx directly.

use std::collections::HashMap;

/// A chemical element, as a type-safe enum instead of a per-atom `String`.
///
/// Covers the symbols molscene knows (those referenced by the radius/color
/// tables and the chemistry predicates); everything else falls into
/// [`Element::Other`], which carries the *normalized* symbol (trimmed,
/// uppercased) so arbitrary-symbol selections and display still work. Unknown
/// symbols are resolved once at parse time via [`Element::from_symbol`], so the
/// geometry/perception hot paths compare a cheap enum instead of re-parsing a
/// string on every lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Element {
    H,
    B,
    C,
    N,
    O,
    F,
    Na,
    Mg,
    P,
    S,
    Cl,
    K,
    Ca,
    Fe,
    Zn,
    Se,
    As,
    Br,
    I,
    /// Any symbol outside the known set, holding the normalized text.
    Other(String),
}

impl Element {
    /// Resolve an element symbol to a typed [`Element`]. This is the single
    /// normalization point: the input is trimmed and uppercased, then matched to
    /// a known variant or stored in [`Element::Other`]. An empty/missing symbol
    /// becomes `Other("")`.
    pub fn from_symbol(symbol: &str) -> Element {
        match symbol.trim().to_ascii_uppercase().as_str() {
            "H" => Element::H,
            "B" => Element::B,
            "C" => Element::C,
            "N" => Element::N,
            "O" => Element::O,
            "F" => Element::F,
            "NA" => Element::Na,
            "MG" => Element::Mg,
            "P" => Element::P,
            "S" => Element::S,
            "CL" => Element::Cl,
            "K" => Element::K,
            "CA" => Element::Ca,
            "FE" => Element::Fe,
            "ZN" => Element::Zn,
            "SE" => Element::Se,
            "AS" => Element::As,
            "BR" => Element::Br,
            "I" => Element::I,
            other => Element::Other(other.to_string()),
        }
    }

    /// The element's symbol (canonical capitalization for known variants, the
    /// stored normalized text for [`Element::Other`]).
    pub fn symbol(&self) -> &str {
        match self {
            Element::H => "H",
            Element::B => "B",
            Element::C => "C",
            Element::N => "N",
            Element::O => "O",
            Element::F => "F",
            Element::Na => "Na",
            Element::Mg => "Mg",
            Element::P => "P",
            Element::S => "S",
            Element::Cl => "Cl",
            Element::K => "K",
            Element::Ca => "Ca",
            Element::Fe => "Fe",
            Element::Zn => "Zn",
            Element::Se => "Se",
            Element::As => "As",
            Element::Br => "Br",
            Element::I => "I",
            Element::Other(s) => s,
        }
    }
}

impl std::fmt::Display for Element {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.symbol())
    }
}

/// A single atom.
#[derive(Debug, Clone, PartialEq)]
pub struct Atom {
    /// Serial number from the source file.
    pub serial: usize,
    /// Atom name, e.g. `"CA"`.
    pub name: String,
    /// Element, e.g. [`Element::C`].
    pub element: Element,
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
    /// When the structure carries explicit bonds (SDF/mol2), returns those
    /// pairs; otherwise infers covalent bonds from interatomic distances via a
    /// uniform cell grid ([`infer_bonds_grid`]) — O(n) at realistic molecular
    /// densities, with results identical to the naive all-pairs scan.
    pub fn bonds(&self) -> Vec<(usize, usize)> {
        if let Some(bonds) = &self.explicit_bonds {
            return bonds.iter().map(|b| (b.a, b.b)).collect();
        }
        infer_bonds_grid(&self.atoms)
    }
}

/// Tolerance (Å) added to the covalent-radius sum when testing for a bond.
const BOND_TOLERANCE: f64 = 0.45;
/// Squared minimum bond length (Å²); pairs closer than this are treated as
/// duplicate/overlapping atoms and rejected. (0.4 Å)².
const BOND_FLOOR_SQ: f64 = 0.16;

/// Distance-based covalent bond inference via a uniform cell grid.
///
/// Two atoms bond when their separation is below the sum of their covalent radii
/// plus [`BOND_TOLERANCE`] and above [`BOND_FLOOR_SQ`] (which rejects
/// duplicate/overlapping atoms) — exactly the predicate of the old O(n²) scan.
///
/// The cell edge equals the largest possible bond cutoff
/// (`2 * MAX_COVALENT_RADIUS + BOND_TOLERANCE`), so any bonded pair must lie
/// within the 27-cell neighborhood of a cell. Bucketing atoms by cell and only
/// scanning those neighbors makes this O(n) at realistic molecular densities
/// instead of the naive all-pairs O(n²). Uses only `Vec`/`HashMap`, so it stays
/// WASM-safe (no kiddo tree to rebuild per call).
fn infer_bonds_grid(atoms: &[Atom]) -> Vec<(usize, usize)> {
    if atoms.len() < 2 {
        return Vec::new();
    }
    // Cell edge = worst-case bond cutoff, so bonds only ever span adjacent cells.
    let cell = 2.0 * MAX_COVALENT_RADIUS + BOND_TOLERANCE;
    // Bounding-box minimum, so coordinates map onto a grid anchored at the data
    // (handles large/negative coordinates; i64 cells avoid overflow/negatives).
    let mut min = (atoms[0].x, atoms[0].y, atoms[0].z);
    for a in atoms {
        min.0 = min.0.min(a.x);
        min.1 = min.1.min(a.y);
        min.2 = min.2.min(a.z);
    }
    let cell_of = |a: &Atom| -> (i64, i64, i64) {
        (
            ((a.x - min.0) / cell).floor() as i64,
            ((a.y - min.1) / cell).floor() as i64,
            ((a.z - min.2) / cell).floor() as i64,
        )
    };
    // Bucket atom indices by cell.
    let mut grid: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (i, a) in atoms.iter().enumerate() {
        grid.entry(cell_of(a)).or_default().push(i);
    }
    let mut bonds = Vec::new();
    // Iterate atoms in index order; the map is only ever looked up (never
    // iterated), and the output is sorted below, so hasher order cannot leak
    // into the result. Do NOT start iterating `grid` here — it would.
    for i in 0..atoms.len() {
        let a = &atoms[i];
        let ra = covalent_radius(&a.element);
        let (cx, cy, cz) = cell_of(a);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(bucket) = grid.get(&(cx + dx, cy + dy, cz + dz)) else {
                        continue;
                    };
                    for &j in bucket {
                        // Visit each unordered pair once, keeping i < j.
                        if j <= i {
                            continue;
                        }
                        let b = &atoms[j];
                        let cutoff = ra + covalent_radius(&b.element) + BOND_TOLERANCE;
                        let ddx = a.x - b.x;
                        let ddy = a.y - b.y;
                        let ddz = a.z - b.z;
                        let d2 = ddx * ddx + ddy * ddy + ddz * ddz;
                        if d2 > BOND_FLOOR_SQ && d2 < cutoff * cutoff {
                            bonds.push((i, j));
                        }
                    }
                }
            }
        }
    }
    // Restore the exact (i, j) ascending order of the old all-pairs loop. The
    // stencil visits pairs out of order; downstream code (chem.rs adjacency /
    // SSSR, the insta geometry snapshots, strict-vector bond tests) depends on
    // this deterministic ordering.
    bonds.sort_unstable();
    bonds
}

/// Van der Waals radius (Å) for an element; falls back to carbon's.
pub fn vdw_radius(element: &Element) -> f32 {
    match element {
        Element::H => 1.10,
        Element::C => 1.70,
        Element::N => 1.55,
        Element::O => 1.52,
        Element::S => 1.80,
        Element::P => 1.80,
        Element::F => 1.47,
        Element::Cl => 1.75,
        Element::Br => 1.85,
        Element::I => 1.98,
        Element::Fe => 1.80,
        Element::Zn => 1.39,
        Element::Mg => 1.73,
        Element::Na => 2.27,
        Element::Ca => 2.31,
        Element::K => 2.75,
        _ => 1.70,
    }
}

/// Largest value in [`covalent_radius`]'s table (`Element::K`, 2.03 Å).
///
/// The cell-grid bond search ([`infer_bonds_grid`]) sizes its cells from this so
/// the 27-cell stencil is guaranteed to contain every possible bond. A test
/// (`max_covalent_radius_matches_table`) pins it to the actual table maximum, so
/// adding a larger element can't silently shrink the cells and drop bonds.
const MAX_COVALENT_RADIUS: f64 = 2.03;

/// Covalent radius (Å) for an element (Cordero 2008); fallback ~carbon.
pub fn covalent_radius(element: &Element) -> f64 {
    match element {
        Element::H => 0.31,
        Element::C => 0.76,
        Element::N => 0.71,
        Element::O => 0.66,
        Element::S => 1.05,
        Element::P => 1.07,
        Element::B => 0.84,
        Element::Se => 1.20,
        Element::As => 1.19,
        Element::F => 0.57,
        Element::Cl => 1.02,
        Element::Br => 1.20,
        Element::I => 1.39,
        Element::Fe => 1.32,
        Element::Zn => 1.22,
        Element::Mg => 1.41,
        Element::Na => 1.66,
        Element::Ca => 1.76,
        Element::K => 2.03,
        _ => 0.77,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carbon(serial: usize, x: f64, y: f64, z: f64) -> Atom {
        Atom {
            serial,
            name: "C".into(),
            element: Element::C,
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

    /// The original naive all-pairs scan, kept as a correctness oracle for the
    /// cell-grid implementation. Must stay byte-for-byte equivalent in result.
    #[allow(clippy::needless_range_loop)] // intentional all-pairs reference scan
    fn bonds_n2(atoms: &[Atom]) -> Vec<(usize, usize)> {
        let mut bonds = Vec::new();
        for i in 0..atoms.len() {
            let a = &atoms[i];
            let ra = covalent_radius(&a.element);
            for j in (i + 1)..atoms.len() {
                let b = &atoms[j];
                let cutoff = ra + covalent_radius(&b.element) + BOND_TOLERANCE;
                let dx = a.x - b.x;
                let dy = a.y - b.y;
                let dz = a.z - b.z;
                let d2 = dx * dx + dy * dy + dz * dz;
                if d2 > BOND_FLOOR_SQ && d2 < cutoff * cutoff {
                    bonds.push((i, j));
                }
            }
        }
        bonds
    }

    fn assert_grid_matches_n2(atoms: &[Atom]) {
        assert_eq!(infer_bonds_grid(atoms), bonds_n2(atoms));
    }

    #[test]
    fn grid_matches_n2_collinear_chain() {
        let atoms = vec![
            carbon(1, 0.0, 0.0, 0.0),
            carbon(2, 1.5, 0.0, 0.0),
            carbon(3, 3.0, 0.0, 0.0),
        ];
        assert_grid_matches_n2(&atoms);
    }

    #[test]
    fn grid_matches_n2_benzene_ring() {
        // Flat hexagon, ~1.39 Å C-C edges.
        let r = 1.39;
        let mut atoms = Vec::new();
        for k in 0..6 {
            let theta = std::f64::consts::PI / 3.0 * k as f64;
            atoms.push(carbon(k + 1, r * theta.cos(), r * theta.sin(), 0.0));
        }
        assert_grid_matches_n2(&atoms);
    }

    #[test]
    fn grid_matches_n2_random_cloud() {
        // Deterministic pseudo-random cloud via a tiny LCG (no rand dependency),
        // spread so some pairs bond and most don't, crossing many cell borders.
        let mut state: u64 = 0x1234_5678_9abc_def0;
        let mut next = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as f64 / (1u64 << 31) as f64 // [0, 1)
        };
        let mut atoms = Vec::new();
        for k in 0..400 {
            atoms.push(carbon(k + 1, next() * 20.0, next() * 20.0, next() * 20.0));
        }
        assert_grid_matches_n2(&atoms);
    }

    #[test]
    fn grid_matches_n2_across_cell_boundaries() {
        // A tight cluster placed so atoms straddle a cell boundary (cell edge is
        // ~4.51 Å); bonds must still be found across the seam.
        let mut atoms = Vec::new();
        let mut serial = 1;
        for gx in 0..3 {
            for gy in 0..3 {
                // Pairs ~1.5 Å apart, centered near multiples of the cell edge.
                let base = gx as f64 * 4.51;
                let y = gy as f64 * 4.51;
                atoms.push(carbon(serial, base - 0.75, y, 0.0));
                serial += 1;
                atoms.push(carbon(serial, base + 0.75, y, 0.0));
                serial += 1;
            }
        }
        assert_grid_matches_n2(&atoms);
    }

    #[test]
    fn grid_matches_n2_large_offset() {
        // Same as the benzene case but translated far from the origin and into
        // negative coordinates, exercising the bbox-relative i64 cell math.
        let r = 1.39;
        let mut atoms = Vec::new();
        for k in 0..6 {
            let theta = std::f64::consts::PI / 3.0 * k as f64;
            atoms.push(carbon(
                k + 1,
                r * theta.cos() - 1000.0,
                r * theta.sin() + 1000.0,
                -500.0,
            ));
        }
        assert_grid_matches_n2(&atoms);
    }

    #[test]
    fn grid_empty_and_single_atom() {
        assert!(infer_bonds_grid(&[]).is_empty());
        assert!(infer_bonds_grid(&[carbon(1, 0.0, 0.0, 0.0)]).is_empty());
    }

    #[test]
    fn max_covalent_radius_matches_table() {
        // Pin MAX_COVALENT_RADIUS to the true table maximum across every known
        // Element variant. If a larger element is added without updating the
        // constant, the cell grid would shrink and silently miss bonds.
        let elements = [
            Element::H,
            Element::B,
            Element::C,
            Element::N,
            Element::O,
            Element::F,
            Element::Na,
            Element::Mg,
            Element::P,
            Element::S,
            Element::Cl,
            Element::K,
            Element::Ca,
            Element::Fe,
            Element::Zn,
            Element::Se,
            Element::As,
            Element::Br,
            Element::I,
            Element::Other("Xx".into()),
        ];
        let table_max = elements
            .iter()
            .map(covalent_radius)
            .fold(f64::MIN, f64::max);
        assert_eq!(MAX_COVALENT_RADIUS, table_max);
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
        assert_eq!(vdw_radius(&Element::C), 1.70);
        assert_eq!(vdw_radius(&Element::from_symbol("o")), 1.52); // case-insensitive
        assert_eq!(vdw_radius(&Element::from_symbol("Xx")), 1.70); // fallback
        assert_eq!(covalent_radius(&Element::O), 0.66);
    }

    #[test]
    fn element_from_symbol_normalizes() {
        assert_eq!(Element::from_symbol(" c "), Element::C);
        assert_eq!(Element::from_symbol("FE"), Element::Fe);
        assert_eq!(Element::from_symbol("fe").symbol(), "Fe");
        assert_eq!(Element::from_symbol("Xx"), Element::Other("XX".into()));
        assert_eq!(Element::from_symbol("Xx").symbol(), "XX");
    }
}
