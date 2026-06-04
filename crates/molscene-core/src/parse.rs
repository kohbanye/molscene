//! PDB/mmCIF parsing via pdbtbx, behind the `parse` feature (native only —
//! excluded from WASM builds). This is a thin adapter: it maps pdbtbx's types
//! into our own [`Structure`] so the rest of the core never depends on pdbtbx.

use std::io::BufReader;

use pdbtbx::{
    ContainsAtomConformer, ContainsAtomConformerResidue, ContainsAtomConformerResidueChain, Format,
    ReadOptions, StrictnessLevel,
};

use crate::structure::{Atom, Structure};

/// Errors that can occur while parsing a structure.
#[derive(Debug)]
pub enum ParseError {
    /// pdbtbx reported a breaking error; the joined messages are included.
    Pdbtbx(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Pdbtbx(msg) => write!(f, "failed to parse structure: {msg}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Supported input formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Pdb,
    Mmcif,
}

impl From<InputFormat> for Format {
    fn from(f: InputFormat) -> Self {
        match f {
            InputFormat::Pdb => Format::Pdb,
            InputFormat::Mmcif => Format::Mmcif,
        }
    }
}

/// Parse a structure from in-memory text.
pub fn parse_str(text: &str, format: InputFormat) -> Result<Structure, ParseError> {
    let reader = BufReader::new(text.as_bytes());
    let (pdb, _errors) = ReadOptions::new()
        .set_format(format.into())
        .set_level(StrictnessLevel::Loose)
        .read_raw(reader)
        .map_err(|errs| {
            ParseError::Pdbtbx(
                errs.iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;

    let mut atoms = Vec::with_capacity(pdb.atom_count());
    for hier in pdb.atoms_with_hierarchy() {
        let atom = hier.atom();
        let (x, y, z) = atom.pos();
        atoms.push(Atom {
            serial: atom.serial_number(),
            name: atom.name().to_string(),
            element: atom
                .element()
                .map(|e| e.symbol().to_string())
                .unwrap_or_default(),
            residue_name: hier.conformer().name().to_string(),
            residue_seq: hier.residue().id().0 as i32,
            chain_id: hier.chain().id().to_string(),
            hetero: atom.hetero(),
            b_factor: atom.b_factor(),
            occupancy: atom.occupancy(),
            x,
            y,
            z,
        });
    }

    Ok(Structure::new(atoms))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIPEPTIDE: &str = include_str!("../../../tests/fixtures/dipeptide.pdb");

    #[test]
    fn parses_atom_count() {
        let s = parse_str(DIPEPTIDE, InputFormat::Pdb).unwrap();
        assert_eq!(s.num_atoms(), 10);
    }

    #[test]
    fn parses_chains_and_residues() {
        let s = parse_str(DIPEPTIDE, InputFormat::Pdb).unwrap();
        assert_eq!(s.chain_ids(), vec!["A".to_string()]);
        assert_eq!(s.num_chains(), 1);
        // ALA 1, GLY 2, HOH 101
        assert_eq!(s.num_residues(), 3);
    }

    #[test]
    fn distinguishes_hetero_atoms() {
        let s = parse_str(DIPEPTIDE, InputFormat::Pdb).unwrap();
        // Only the water oxygen is a HETATM.
        assert_eq!(s.num_hetero(), 1);
        let water = s.atoms.iter().find(|a| a.residue_name == "HOH").unwrap();
        assert!(water.hetero);
        assert_eq!(water.element, "O");
    }

    #[test]
    fn reads_atom_fields() {
        let s = parse_str(DIPEPTIDE, InputFormat::Pdb).unwrap();
        let first = &s.atoms[0];
        assert_eq!(first.name, "N");
        assert_eq!(first.element, "N");
        assert_eq!(first.residue_name, "ALA");
        assert_eq!(first.residue_seq, 1);
        assert_eq!(first.chain_id, "A");
        assert!(!first.hetero);
        assert!((first.x - 11.104).abs() < 1e-3);
        assert!((first.b_factor - 20.0).abs() < 1e-3);
        assert!((first.occupancy - 1.0).abs() < 1e-3);
    }
}
