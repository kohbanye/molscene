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
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// A parsed molecular structure.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Structure {
    pub atoms: Vec<Atom>,
}

impl Structure {
    pub fn new(atoms: Vec<Atom>) -> Self {
        Self { atoms }
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
}
