//! PDB/mmCIF parsing via pdbtbx, behind the `parse` feature (native only —
//! excluded from WASM builds). This is a thin adapter: it maps pdbtbx's types
//! into our own [`Structure`] so the rest of the core never depends on pdbtbx.

use std::io::BufReader;

use pdbtbx::{
    ContainsAtomConformer, ContainsAtomConformerResidue, ContainsAtomConformerResidueChain, Format,
    ReadOptions, StrictnessLevel,
};

use crate::structure::{Atom, Ss, Structure};

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

    let mut structure = Structure::new(atoms);
    // pdbtbx does not surface HELIX/SHEET secondary-structure records, so scan
    // the raw PDB text for them ourselves (mmCIF annotations are not read yet —
    // those structures fall back to geometric assignment in the cartoon builder).
    if format == InputFormat::Pdb {
        annotate_secondary_structure(text, &mut structure);
    }
    Ok(structure)
}

/// Read `HELIX`/`SHEET` records from PDB text and record per-residue secondary
/// structure on `structure`. Uses the fixed-column layout of the PDB spec; lines
/// too short or with unparseable sequence numbers are skipped.
fn annotate_secondary_structure(text: &str, structure: &mut Structure) {
    for line in text.lines() {
        // HELIX: initChain col 20, initSeq 22-25, endChain col 32, endSeq 34-37.
        // SHEET: initChain col 22, initSeq 23-26, endChain col 33, endSeq 34-37.
        // (1-indexed, inclusive; we use 0-indexed half-open ranges below.)
        let (kind, init_chain, init_seq, end_chain, end_seq) = if line.starts_with("HELIX ") {
            (Ss::Helix, 19, 21..25, 31, 33..37)
        } else if line.starts_with("SHEET ") {
            (Ss::Sheet, 21, 22..26, 32, 33..37)
        } else {
            continue;
        };
        let (Some(ic), Some(ec)) = (col_char(line, init_chain), col_char(line, end_chain)) else {
            continue;
        };
        let (Some(is), Some(es)) = (col_int(line, init_seq), col_int(line, end_seq)) else {
            continue;
        };
        // A range that spans two chains is malformed; only annotate when the
        // start and end chains agree.
        if ic != ec {
            continue;
        }
        let chain = ic.to_string();
        for seq in is.min(es)..=is.max(es) {
            structure.set_ss(chain.clone(), seq, kind);
        }
    }
}

/// The non-space character at 0-indexed column `col`, if the line is long enough.
fn col_char(line: &str, col: usize) -> Option<char> {
    line.as_bytes()
        .get(col)
        .map(|&b| b as char)
        .filter(|c| !c.is_whitespace())
}

/// Parse the integer in the 0-indexed half-open column `range`, trimming spaces.
fn col_int(line: &str, range: std::ops::Range<usize>) -> Option<i32> {
    line.get(range)?.trim().parse().ok()
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

    /// Write `s` into a PDB line buffer starting at 1-indexed column `col`.
    fn put(buf: &mut Vec<u8>, col: usize, s: &str) {
        let start = col - 1;
        if buf.len() < start + s.len() {
            buf.resize(start + s.len(), b' ');
        }
        buf[start..start + s.len()].copy_from_slice(s.as_bytes());
    }

    #[test]
    fn reads_helix_and_sheet_records() {
        // HELIX over A1..A3, SHEET over A6..A7, plus a minimal CA so parsing
        // succeeds. Columns follow the PDB spec.
        let mut helix = vec![b' '; 6];
        helix[..6].copy_from_slice(b"HELIX ");
        put(&mut helix, 20, "A"); // initChainID
        put(&mut helix, 25, "1"); // initSeqNum (right-justified in 22-25)
        put(&mut helix, 32, "A"); // endChainID
        put(&mut helix, 37, "3"); // endSeqNum (right-justified in 34-37)

        let mut sheet = vec![b' '; 6];
        sheet[..6].copy_from_slice(b"SHEET ");
        put(&mut sheet, 22, "A"); // initChainID
        put(&mut sheet, 26, "6"); // initSeqNum (right-justified in 23-26)
        put(&mut sheet, 33, "A"); // endChainID
        put(&mut sheet, 37, "7"); // endSeqNum (right-justified in 34-37)

        let text = format!(
            "{}\n{}\nATOM      1  CA  ALA A   1      11.804   5.123   6.034  1.00 20.00           C\n",
            String::from_utf8(helix).unwrap(),
            String::from_utf8(sheet).unwrap(),
        );
        let s = parse_str(&text, InputFormat::Pdb).unwrap();
        assert!(s.has_ss_annotations());
        assert_eq!(s.ss_at("A", 1), Some(Ss::Helix));
        assert_eq!(s.ss_at("A", 2), Some(Ss::Helix)); // inside the range
        assert_eq!(s.ss_at("A", 3), Some(Ss::Helix));
        assert_eq!(s.ss_at("A", 6), Some(Ss::Sheet));
        assert_eq!(s.ss_at("A", 7), Some(Ss::Sheet));
        assert_eq!(s.ss_at("A", 4), None); // gap between → Loop
    }

    #[test]
    fn no_annotations_when_no_records() {
        let s = parse_str(DIPEPTIDE, InputFormat::Pdb).unwrap();
        assert!(!s.has_ss_annotations());
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
