//! Selection evaluation.
//!
//! v0.1: a selection is an opaque string. This module resolves a **single-clause**
//! selection against a [`Structure`] into atom indices. Composed expressions
//! (`(A) and (B)`, `not (A)`) produced by the `ms.sel` operators are NOT yet
//! evaluated — they fall back to selecting all atoms with a warning. The real
//! expression-tree evaluator lands in v0.2.

use crate::structure::Structure;

const WATER_RESNAMES: [&str; 6] = ["HOH", "WAT", "H2O", "TIP3", "TIP", "SOL"];
const BACKBONE_NAMES: [&str; 4] = ["N", "CA", "C", "O"];

fn is_water(residue_name: &str) -> bool {
    let r = residue_name.trim().to_ascii_uppercase();
    WATER_RESNAMES.contains(&r.as_str())
}

/// Resolve a selection string to the matching atom indices (`i` into
/// `structure.atoms`).
pub fn evaluate(structure: &Structure, selection: &str) -> Vec<usize> {
    let all = || (0..structure.atoms.len()).collect::<Vec<_>>();
    let s = selection.trim();

    if s.is_empty() || s == "all" {
        return all();
    }
    if s == "none" {
        return Vec::new();
    }
    if is_composed(s) {
        eprintln!(
            "molscene: composed selection {s:?} is not evaluated in v0.1; \
             selecting all atoms. (Rust evaluator lands in v0.2.)"
        );
        return all();
    }

    let clause = strip_parens(s);
    let (keyword, arg) = split_clause(&clause);
    let pred: Box<dyn Fn(&crate::structure::Atom) -> bool> = match (keyword, arg.as_deref()) {
        ("protein", None) | ("polymer", None) | ("nucleic", None) => Box::new(|a| !a.hetero),
        ("hetero", None) | ("hetatm", None) => Box::new(|a| a.hetero),
        ("ligand", None) => Box::new(|a| a.hetero && !is_water(&a.residue_name)),
        ("water", None) | ("solvent", None) => Box::new(|a| is_water(&a.residue_name)),
        ("hydrogen", None) | ("hydrogens", None) => {
            Box::new(|a| a.element.eq_ignore_ascii_case("H"))
        }
        ("backbone", None) => Box::new(|a| {
            !a.hetero
                && BACKBONE_NAMES
                    .iter()
                    .any(|n| a.name.eq_ignore_ascii_case(n))
        }),
        ("chain", Some(c)) => {
            let c = c.to_string();
            Box::new(move |a| a.chain_id == c)
        }
        ("resn", Some(name)) | ("resname", Some(name)) => {
            let name = name.to_ascii_uppercase();
            Box::new(move |a| a.residue_name.eq_ignore_ascii_case(&name))
        }
        ("element", Some(e)) | ("elem", Some(e)) => {
            let e = e.to_string();
            Box::new(move |a| a.element.eq_ignore_ascii_case(&e))
        }
        ("resi", Some(spec)) | ("resid", Some(spec)) => match parse_resi(spec) {
            Some((lo, hi)) => Box::new(move |a| a.residue_seq >= lo && a.residue_seq <= hi),
            None => {
                eprintln!("molscene: invalid resi range {spec:?}; selecting nothing.");
                return Vec::new();
            }
        },
        _ => {
            eprintln!("molscene: unrecognized selection {clause:?}; selecting all atoms.");
            return all();
        }
    };

    structure
        .atoms
        .iter()
        .enumerate()
        .filter(|(_, a)| pred(a))
        .map(|(i, _)| i)
        .collect()
}

fn is_composed(s: &str) -> bool {
    s.contains(" and ") || s.contains(" or ") || s.starts_with("not ") || s.starts_with("not(")
}

fn strip_parens(s: &str) -> String {
    let mut out = s.trim().to_string();
    while out.starts_with('(') && out.ends_with(')') {
        out = out[1..out.len() - 1].trim().to_string();
    }
    out
}

fn split_clause(clause: &str) -> (&str, Option<String>) {
    match clause.split_once(char::is_whitespace) {
        Some((kw, rest)) => (kw, Some(rest.trim().to_string())),
        None => (clause, None),
    }
}

/// Parse "N" or "N-M" into an inclusive (lo, hi) range.
fn parse_resi(spec: &str) -> Option<(i32, i32)> {
    if let Some((lo, hi)) = spec.split_once('-') {
        Some((lo.trim().parse().ok()?, hi.trim().parse().ok()?))
    } else {
        let n: i32 = spec.trim().parse().ok()?;
        Some((n, n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::Atom;

    fn atom(
        i: usize,
        name: &str,
        elem: &str,
        resn: &str,
        resi: i32,
        chain: &str,
        het: bool,
    ) -> Atom {
        Atom {
            serial: i,
            name: name.into(),
            element: elem.into(),
            residue_name: resn.into(),
            residue_seq: resi,
            chain_id: chain.into(),
            hetero: het,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    fn fixture() -> Structure {
        Structure::new(vec![
            atom(0, "N", "N", "ALA", 1, "A", false),
            atom(1, "CA", "C", "ALA", 1, "A", false),
            atom(2, "C", "C", "ALA", 1, "A", false),
            atom(3, "CB", "C", "ALA", 1, "B", false),
            atom(4, "O", "O", "HOH", 101, "A", true),
            atom(5, "FE", "FE", "FE", 201, "A", true),
        ])
    }

    #[test]
    fn all_and_none() {
        let s = fixture();
        assert_eq!(evaluate(&s, "all"), vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(evaluate(&s, ""), vec![0, 1, 2, 3, 4, 5]);
        assert!(evaluate(&s, "none").is_empty());
    }

    #[test]
    fn classification_macros() {
        let s = fixture();
        assert_eq!(evaluate(&s, "protein"), vec![0, 1, 2, 3]);
        assert_eq!(evaluate(&s, "hetero"), vec![4, 5]);
        assert_eq!(evaluate(&s, "water"), vec![4]);
        assert_eq!(evaluate(&s, "ligand"), vec![5]);
        assert_eq!(evaluate(&s, "backbone"), vec![0, 1, 2]);
    }

    #[test]
    fn predicates() {
        let s = fixture();
        assert_eq!(evaluate(&s, "chain A"), vec![0, 1, 2, 4, 5]);
        assert_eq!(evaluate(&s, "element C"), vec![1, 2, 3]);
        assert_eq!(evaluate(&s, "resn HOH"), vec![4]);
        assert_eq!(evaluate(&s, "resi 1"), vec![0, 1, 2, 3]);
        assert_eq!(evaluate(&s, "resi 100-200"), vec![4]);
    }

    #[test]
    fn composed_falls_back_to_all() {
        let s = fixture();
        assert_eq!(
            evaluate(&s, "(chain A) and (water)"),
            vec![0, 1, 2, 3, 4, 5]
        );
        assert_eq!(evaluate(&s, "not (hydrogen)"), vec![0, 1, 2, 3, 4, 5]);
    }
}
