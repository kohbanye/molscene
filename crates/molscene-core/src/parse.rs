//! PDB/mmCIF parsing via pdbtbx, behind the `parse` feature (native only —
//! excluded from WASM builds). This is a thin adapter: it maps pdbtbx's types
//! into our own [`Structure`] so the rest of the core never depends on pdbtbx.

use std::io::BufReader;

use pdbtbx::{
    ContainsAtomConformer, ContainsAtomConformerResidue, ContainsAtomConformerResidueChain, Format,
    ReadOptions, StrictnessLevel,
};

use crate::structure::{Atom, Bond, BondOrder, Element, Ss, Structure};

/// Errors that can occur while parsing a structure.
#[derive(Debug)]
pub enum ParseError {
    /// pdbtbx reported a breaking error; the joined messages are included.
    Pdbtbx(String),
    /// A malformed SDF/molfile (bad counts line, unsupported version, …).
    Sdf(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Pdbtbx(msg) => write!(f, "failed to parse structure: {msg}"),
            ParseError::Sdf(msg) => write!(f, "failed to parse SDF: {msg}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Supported input formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Pdb,
    Mmcif,
    /// SDF / V2000 molfile (small molecules, with explicit bond orders).
    Sdf,
}

impl TryFrom<InputFormat> for Format {
    type Error = ParseError;
    fn try_from(f: InputFormat) -> Result<Self, Self::Error> {
        match f {
            InputFormat::Pdb => Ok(Format::Pdb),
            InputFormat::Mmcif => Ok(Format::Mmcif),
            // SDF never reaches pdbtbx — `parse_str` branches to `parse_sdf`.
            InputFormat::Sdf => Err(ParseError::Sdf("SDF is not a pdbtbx format".into())),
        }
    }
}

/// Parse a structure from in-memory text.
pub fn parse_str(text: &str, format: InputFormat) -> Result<Structure, ParseError> {
    if format == InputFormat::Sdf {
        return parse_sdf(text);
    }
    let pdbtbx_format: Format = format.try_into()?;
    let reader = BufReader::new(text.as_bytes());
    let (pdb, _errors) = ReadOptions::new()
        .set_format(pdbtbx_format)
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
                .map(|e| Element::from_symbol(e.symbol()))
                .unwrap_or(Element::Other(String::new())),
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

/// Parse a V2000 SDF / molfile into a [`Structure`] with explicit bond orders.
///
/// Layout: 3 header lines, then a counts line (atom count cols 1-3, bond count
/// cols 4-6), the atom block (`x`/`y`/`z` + element), and the bond block
/// (atom1, atom2, bond type). Atoms are tagged as a single ligand residue
/// (`LIG`, chain `A`, `HETATM`-like) since SDF carries no residue identity. Only
/// the first record of a multi-record file is read; `V3000` is rejected.
fn parse_sdf(text: &str) -> Result<Structure, ParseError> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 4 {
        return Err(ParseError::Sdf("file too short for a molfile".into()));
    }
    // Header is lines 0..3 (title, program, comment); line 3 is the counts line.
    let counts = lines[3];
    if counts.contains("V3000") {
        return Err(ParseError::Sdf(
            "V3000 molfiles are not supported (use V2000)".into(),
        ));
    }
    let n_atoms = col_usize(counts, 0..3)
        .ok_or_else(|| ParseError::Sdf("unparseable atom count in counts line".into()))?;
    let n_bonds = col_usize(counts, 3..6)
        .ok_or_else(|| ParseError::Sdf("unparseable bond count in counts line".into()))?;

    let atom_start = 4;
    let bond_start = atom_start + n_atoms;
    if lines.len() < bond_start + n_bonds {
        return Err(ParseError::Sdf(
            "atom/bond block shorter than the declared counts".into(),
        ));
    }

    let mut atoms = Vec::with_capacity(n_atoms);
    for (k, line) in lines[atom_start..bond_start].iter().enumerate() {
        let (x, y, z, element) = parse_atom_line(line)
            .ok_or_else(|| ParseError::Sdf(format!("malformed atom line {}", k + 1)))?;
        atoms.push(Atom {
            serial: k + 1,
            element: Element::from_symbol(&element),
            name: element,
            residue_name: "LIG".into(),
            residue_seq: 1,
            chain_id: "A".into(),
            hetero: true,
            b_factor: 0.0,
            occupancy: 1.0,
            x,
            y,
            z,
        });
    }

    let mut bonds = Vec::with_capacity(n_bonds);
    for (k, line) in lines[bond_start..bond_start + n_bonds].iter().enumerate() {
        let (a1, a2, kind) = parse_bond_line(line)
            .ok_or_else(|| ParseError::Sdf(format!("malformed bond line {}", k + 1)))?;
        // Molfile atom indices are 1-based.
        if a1 == 0 || a2 == 0 || a1 > n_atoms || a2 > n_atoms {
            return Err(ParseError::Sdf(format!(
                "bond {} references an out-of-range atom",
                k + 1
            )));
        }
        bonds.push(Bond {
            a: a1 - 1,
            b: a2 - 1,
            order: bond_order(kind),
        });
    }

    Ok(Structure::new(atoms).with_bonds(bonds))
}

/// Map a V2000 bond-type code to a [`BondOrder`] (1=single, 2=double, 3=triple,
/// 4=aromatic; query types 5-8 degrade to single).
fn bond_order(kind: u32) -> BondOrder {
    match kind {
        2 => BondOrder::Double,
        3 => BondOrder::Triple,
        4 => BondOrder::Aromatic,
        _ => BondOrder::Single,
    }
}

/// Parse an SDF atom line: `x`/`y`/`z` (cols 1-10/11-20/21-30) and the element
/// (cols 32-34). Falls back to whitespace splitting when the fixed columns don't
/// parse (hand-written fixtures are often not column-perfect).
fn parse_atom_line(line: &str) -> Option<(f64, f64, f64, String)> {
    let fixed = (|| {
        let x = col_f64(line, 0..10)?;
        let y = col_f64(line, 10..20)?;
        let z = col_f64(line, 20..30)?;
        let element = line.get(31..34)?.trim().to_string();
        if element.is_empty() {
            return None;
        }
        Some((x, y, z, element))
    })();
    if fixed.is_some() {
        return fixed;
    }
    let mut it = line.split_whitespace();
    let x = it.next()?.parse().ok()?;
    let y = it.next()?.parse().ok()?;
    let z = it.next()?.parse().ok()?;
    let element = it.next()?.to_string();
    Some((x, y, z, element))
}

/// Parse an SDF bond line: atom1/atom2 (cols 1-3/4-6, 1-based) and the bond type
/// (cols 7-9). Falls back to whitespace splitting like [`parse_atom_line`].
fn parse_bond_line(line: &str) -> Option<(usize, usize, u32)> {
    let fixed = (|| {
        let a1 = col_usize(line, 0..3)?;
        let a2 = col_usize(line, 3..6)?;
        let kind = col_usize(line, 6..9)? as u32;
        Some((a1, a2, kind))
    })();
    if fixed.is_some() {
        return fixed;
    }
    let mut it = line.split_whitespace();
    let a1 = it.next()?.parse().ok()?;
    let a2 = it.next()?.parse().ok()?;
    let kind = it.next()?.parse().ok()?;
    Some((a1, a2, kind))
}

/// Parse the `usize` in the 0-indexed half-open column `range`, trimming spaces.
fn col_usize(line: &str, range: std::ops::Range<usize>) -> Option<usize> {
    line.get(range)?.trim().parse().ok()
}

/// Parse the `f64` in the 0-indexed half-open column `range`, trimming spaces.
fn col_f64(line: &str, range: std::ops::Range<usize>) -> Option<f64> {
    line.get(range)?.trim().parse().ok()
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
        // A blank chain ID maps to the empty-string chain key atoms use, so
        // single-chain PDBs that leave the column blank still get annotated.
        let chain = if ic.is_whitespace() {
            String::new()
        } else {
            ic.to_string()
        };
        for seq in is.min(es)..=is.max(es) {
            structure.set_ss(chain.clone(), seq, kind);
        }
    }
}

/// The character at 0-indexed column `col`, if the line is long enough. May be a
/// space (a blank chain ID) — the caller normalizes that to the empty chain key.
fn col_char(line: &str, col: usize) -> Option<char> {
    line.as_bytes().get(col).map(|&b| b as char)
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
        assert_eq!(water.element, Element::O);
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
    fn blank_chain_helix_annotates_empty_chain() {
        // A HELIX record with a blank chain ID maps to the empty-string chain
        // key, so single-chain PDBs that leave the column blank still annotate.
        let mut helix = vec![b' '; 6];
        helix[..6].copy_from_slice(b"HELIX ");
        // Leave chain columns (20, 32) blank; only the sequence numbers are set.
        put(&mut helix, 25, "1");
        put(&mut helix, 37, "2");
        // A CA atom (also blank chain) so pdbtbx has something to parse.
        let mut atom =
            b"ATOM      1  CA  ALA A   1      11.804   5.123   6.034  1.00 20.00           C"
                .to_vec();
        atom[21] = b' '; // blank the chain ID column
        let text = format!(
            "{}\n{}\n",
            String::from_utf8(helix).unwrap(),
            String::from_utf8(atom).unwrap(),
        );
        let s = parse_str(&text, InputFormat::Pdb).unwrap();
        assert_eq!(s.ss_at("", 1), Some(Ss::Helix));
        assert_eq!(s.ss_at("", 2), Some(Ss::Helix));
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
        assert_eq!(first.element, Element::N);
        assert_eq!(first.residue_name, "ALA");
        assert_eq!(first.residue_seq, 1);
        assert_eq!(first.chain_id, "A");
        assert!(!first.hetero);
        assert!((first.x - 11.104).abs() < 1e-3);
        assert!((first.b_factor - 20.0).abs() < 1e-3);
        assert!((first.occupancy - 1.0).abs() < 1e-3);
    }

    // -- SDF parsing --------------------------------------------------------

    /// A V2000 atom line with the element in the standard fixed columns.
    fn mol_atom(x: f64, y: f64, z: f64, sym: &str) -> String {
        format!("{x:>10.4}{y:>10.4}{z:>10.4} {sym:<3} 0  0  0  0  0  0  0  0  0  0  0  0")
    }

    /// A V2000 bond line (1-based atom indices, fixed columns).
    fn mol_bond(a: usize, b: usize, order: u32) -> String {
        format!("{a:>3}{b:>3}{order:>3}  0  0  0  0")
    }

    /// Build a minimal V2000 molfile from atom and bond lines.
    fn molfile(atoms: &[String], bonds: &[String]) -> String {
        let mut s = String::from("title\n  prog\ncomment\n");
        s.push_str(&format!(
            "{:>3}{:>3}  0  0  0  0  0  0  0  0999 V2000\n",
            atoms.len(),
            bonds.len()
        ));
        for a in atoms {
            s.push_str(a);
            s.push('\n');
        }
        for b in bonds {
            s.push_str(b);
            s.push('\n');
        }
        s.push_str("M  END\n");
        s
    }

    #[test]
    fn parses_sdf_ethylene_with_explicit_orders() {
        // C=C double, four C–H singles.
        let atoms = vec![
            mol_atom(0.0, 0.0, 0.0, "C"),
            mol_atom(1.33, 0.0, 0.0, "C"),
            mol_atom(-0.5, 0.93, 0.0, "H"),
            mol_atom(-0.5, -0.93, 0.0, "H"),
            mol_atom(1.83, 0.93, 0.0, "H"),
            mol_atom(1.83, -0.93, 0.0, "H"),
        ];
        let bonds = vec![
            mol_bond(1, 2, 2),
            mol_bond(1, 3, 1),
            mol_bond(1, 4, 1),
            mol_bond(2, 5, 1),
            mol_bond(2, 6, 1),
        ];
        let s = parse_str(&molfile(&atoms, &bonds), InputFormat::Sdf).unwrap();
        assert_eq!(s.num_atoms(), 6);
        let explicit = s.explicit_bonds().expect("explicit bonds from SDF");
        assert_eq!(explicit.len(), 5);
        // The C=C bond (atoms 0-1) is double; the rest single.
        let cc = explicit.iter().find(|b| b.a == 0 && b.b == 1).unwrap();
        assert_eq!(cc.order, BondOrder::Double);
        assert!(explicit
            .iter()
            .filter(|b| !(b.a == 0 && b.b == 1))
            .all(|b| b.order == BondOrder::Single));
        // SDF atoms are tagged as a single ligand residue.
        assert!(s.atoms.iter().all(|a| a.hetero && a.residue_name == "LIG"));
    }

    #[test]
    fn parses_sdf_aromatic_bond_type() {
        let atoms = vec![mol_atom(0.0, 0.0, 0.0, "C"), mol_atom(1.39, 0.0, 0.0, "C")];
        let bonds = vec![mol_bond(1, 2, 4)];
        let s = parse_str(&molfile(&atoms, &bonds), InputFormat::Sdf).unwrap();
        assert_eq!(s.explicit_bonds().unwrap()[0].order, BondOrder::Aromatic);
    }

    #[test]
    fn rejects_v3000_molfile() {
        let text = "title\n  prog\ncomment\n  0  0  0developer  0  0  0  0  0  0999 V3000\n";
        let err = parse_str(text, InputFormat::Sdf).unwrap_err();
        assert!(matches!(err, ParseError::Sdf(_)));
    }

    #[test]
    fn rejects_truncated_sdf() {
        // Counts say 3 atoms but only one is present.
        let atom = mol_atom(0.0, 0.0, 0.0, "C");
        let mut text =
            String::from("title\n  prog\ncomment\n  3  0  0  0  0  0  0  0  0  0999 V2000\n");
        text.push_str(&atom);
        text.push('\n');
        let err = parse_str(&text, InputFormat::Sdf).unwrap_err();
        assert!(matches!(err, ParseError::Sdf(_)));
    }
}
