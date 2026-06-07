//! Selection as a typed expression tree.
//!
//! A selection is an [`Expr`] value — built and composed through the API, never
//! parsed from a string. This module owns the tree and its evaluator, which
//! resolves an `Expr` against a [`Structure`] into atom indices.
//!
//! The tree has boolean composition (`and` / `or` / `not`), spatial operators
//! (`around` / `within` / `expand` / `beyond`, backed by a k-d tree), aggregation
//! (`byres` / `bychain` / `bymol`), numeric predicates (`b` / `q` comparisons),
//! plus the classification macros and single-clause predicates (`chain`, `resn`,
//! `element`, `resi`, …). Build it with [`Expr`]'s constructors and combinators
//! (`Expr::chain("A").and(Expr::Protein)`); a `Display` impl renders a readable
//! form for debugging only (it is not a canonical, re-parseable representation).

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;

use kiddo::{KdTree, SquaredEuclidean};

use crate::structure::{Atom, Element, Structure};

const WATER_RESNAMES: [&str; 6] = ["HOH", "WAT", "H2O", "TIP3", "TIP", "SOL"];
const BACKBONE_NAMES: [&str; 4] = ["N", "CA", "C", "O"];
/// Standard DNA/RNA residue names (PDB), including the `D*` deoxy forms.
const NUCLEIC_RESNAMES: [&str; 12] = [
    "DA", "DC", "DG", "DT", "DU", "DI", "A", "C", "G", "U", "T", "I",
];

fn is_water(residue_name: &str) -> bool {
    let r = residue_name.trim().to_ascii_uppercase();
    WATER_RESNAMES.contains(&r.as_str())
}

fn is_nucleic(residue_name: &str) -> bool {
    let r = residue_name.trim().to_ascii_uppercase();
    NUCLEIC_RESNAMES.contains(&r.as_str())
}

fn is_backbone(name: &str) -> bool {
    BACKBONE_NAMES.iter().any(|n| name.eq_ignore_ascii_case(n))
}

/// Numeric per-atom field a `b`/`q` predicate compares against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumField {
    /// Temperature (B) factor.
    BFactor,
    /// Occupancy.
    Occupancy,
}

/// Comparison operator for numeric predicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl CmpOp {
    fn apply(self, lhs: f64, rhs: f64) -> bool {
        match self {
            CmpOp::Lt => lhs < rhs,
            CmpOp::Le => lhs <= rhs,
            CmpOp::Gt => lhs > rhs,
            CmpOp::Ge => lhs >= rhs,
            CmpOp::Eq => lhs == rhs,
            CmpOp::Ne => lhs != rhs,
        }
    }

    /// Parse a comparison operator (`<`, `<=`, `>`, `>=`, `=`/`==`, `!=`).
    pub fn parse(s: &str) -> Option<CmpOp> {
        Some(match s.trim() {
            "<" => CmpOp::Lt,
            "<=" => CmpOp::Le,
            ">" => CmpOp::Gt,
            ">=" => CmpOp::Ge,
            "=" | "==" => CmpOp::Eq,
            "!=" => CmpOp::Ne,
            _ => return None,
        })
    }

    fn symbol(self) -> &'static str {
        match self {
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
            CmpOp::Eq => "=",
            CmpOp::Ne => "!=",
        }
    }
}

/// A selection expression tree. Build it with the constructors and combinators
/// below, or the bare unit variants (`Expr::Protein`, `Expr::All`, …).
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    All,
    None,
    // Classification macros.
    Protein, // protein | polymer  (non-hetero)
    Nucleic, // DNA/RNA by residue name (non-hetero)
    Hetero,  // hetero | hetatm
    Ligand,
    Water, // water | solvent
    Hydrogen,
    Backbone,
    Sidechain,
    // Single-clause predicates.
    Chain(String),
    ResName(String),
    Element(Element),
    ResId(i32, i32), // inclusive [lo, hi]
    Numeric {
        field: NumField,
        op: CmpOp,
        value: f64,
    },
    // Boolean composition.
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    // Aggregation: expand to whole residue / chain / bonded molecule.
    ByRes(Box<Expr>),
    ByChain(Box<Expr>),
    ByMol(Box<Expr>),
    // Spatial: radius (Å) of an operand selection.
    Within(f64, Box<Expr>),
    Around(f64, Box<Expr>),
    Expand(f64, Box<Expr>),
    Beyond(f64, Box<Expr>),
}

impl Expr {
    // Single-clause predicate constructors.
    pub fn chain(id: impl Into<String>) -> Expr {
        Expr::Chain(id.into())
    }
    pub fn resn(name: impl Into<String>) -> Expr {
        Expr::ResName(name.into())
    }
    pub fn element(symbol: &str) -> Expr {
        Expr::Element(Element::from_symbol(symbol))
    }
    /// Inclusive residue-number range `[lo, hi]` (use `lo == hi` for one residue).
    pub fn resi(lo: i32, hi: i32) -> Expr {
        Expr::ResId(lo, hi)
    }
    pub fn numeric(field: NumField, op: CmpOp, value: f64) -> Expr {
        Expr::Numeric { field, op, value }
    }

    // Boolean / aggregation / spatial combinators.
    pub fn and(self, other: Expr) -> Expr {
        Expr::And(Box::new(self), Box::new(other))
    }
    pub fn or(self, other: Expr) -> Expr {
        Expr::Or(Box::new(self), Box::new(other))
    }
    #[allow(clippy::should_implement_trait)]
    pub fn not(self) -> Expr {
        Expr::Not(Box::new(self))
    }
    pub fn byres(self) -> Expr {
        Expr::ByRes(Box::new(self))
    }
    pub fn bychain(self) -> Expr {
        Expr::ByChain(Box::new(self))
    }
    pub fn bymol(self) -> Expr {
        Expr::ByMol(Box::new(self))
    }
    /// Atoms within `radius` Å of `self`, excluding `self` (a shell).
    pub fn around(self, radius: f64) -> Expr {
        Expr::Around(radius, Box::new(self))
    }
    /// Atoms within `radius` Å of `self`, `self` included.
    pub fn within(self, radius: f64) -> Expr {
        Expr::Within(radius, Box::new(self))
    }
    /// `self` grown by `radius` Å (alias of `within` today).
    pub fn expand(self, radius: f64) -> Expr {
        Expr::Expand(radius, Box::new(self))
    }
    /// Atoms farther than `radius` Å from `self`.
    pub fn beyond(self, radius: f64) -> Expr {
        Expr::Beyond(radius, Box::new(self))
    }
}

/// A readable rendering for debugging / error messages. **Not** a canonical or
/// re-parseable form — selections are values, not strings.
impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::All => write!(f, "all"),
            Expr::None => write!(f, "none"),
            Expr::Protein => write!(f, "protein"),
            Expr::Nucleic => write!(f, "nucleic"),
            Expr::Hetero => write!(f, "hetero"),
            Expr::Ligand => write!(f, "ligand"),
            Expr::Water => write!(f, "water"),
            Expr::Hydrogen => write!(f, "hydrogen"),
            Expr::Backbone => write!(f, "backbone"),
            Expr::Sidechain => write!(f, "sidechain"),
            Expr::Chain(c) => write!(f, "chain {c}"),
            Expr::ResName(n) => write!(f, "resn {n}"),
            Expr::Element(e) => write!(f, "element {e}"),
            Expr::ResId(lo, hi) if lo == hi => write!(f, "resi {lo}"),
            Expr::ResId(lo, hi) => write!(f, "resi {lo}-{hi}"),
            Expr::Numeric { field, op, value } => {
                let name = match field {
                    NumField::BFactor => "b",
                    NumField::Occupancy => "q",
                };
                write!(f, "{name} {} {value}", op.symbol())
            }
            Expr::And(l, r) => write!(f, "({l}) and ({r})"),
            Expr::Or(l, r) => write!(f, "({l}) or ({r})"),
            Expr::Not(inner) => write!(f, "not ({inner})"),
            Expr::ByRes(inner) => write!(f, "byres ({inner})"),
            Expr::ByChain(inner) => write!(f, "bychain ({inner})"),
            Expr::ByMol(inner) => write!(f, "bymol ({inner})"),
            Expr::Within(r, inner) => write!(f, "within {r} of ({inner})"),
            Expr::Around(r, inner) => write!(f, "around {r} of ({inner})"),
            Expr::Expand(r, inner) => write!(f, "expand {r} of ({inner})"),
            Expr::Beyond(r, inner) => write!(f, "beyond {r} of ({inner})"),
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// Resolve a selection [`Expr`] to the matching atom indices (sorted ascending).
pub fn evaluate(structure: &Structure, expr: &Expr) -> Vec<usize> {
    let ctx = EvalCtx::new(structure);
    let mask = eval(expr, structure, &ctx);
    mask.iter()
        .enumerate()
        .filter_map(|(i, &b)| b.then_some(i))
        .collect()
}

/// Shared, lazily-built state for a single evaluation pass.
struct EvalCtx<'a> {
    structure: &'a Structure,
    /// `atom index -> connected-component id`, built once on first `bymol`.
    components: RefCell<Option<Vec<usize>>>,
}

impl<'a> EvalCtx<'a> {
    fn new(structure: &'a Structure) -> Self {
        EvalCtx {
            structure,
            components: RefCell::new(None),
        }
    }

    /// Connected components over inferred bonds (cached).
    fn components(&self) -> Vec<usize> {
        if self.components.borrow().is_none() {
            let n = self.structure.atoms.len();
            let comp = union_find_components(n, &self.structure.bonds());
            *self.components.borrow_mut() = Some(comp);
        }
        self.components.borrow().clone().unwrap()
    }
}

fn eval(expr: &Expr, structure: &Structure, ctx: &EvalCtx) -> Vec<bool> {
    let atoms = &structure.atoms;
    let mask_from = |pred: &dyn Fn(&Atom) -> bool| atoms.iter().map(pred).collect::<Vec<_>>();

    match expr {
        Expr::All => vec![true; atoms.len()],
        Expr::None => vec![false; atoms.len()],
        Expr::Protein => mask_from(&|a| !a.hetero),
        Expr::Nucleic => mask_from(&|a| !a.hetero && is_nucleic(&a.residue_name)),
        Expr::Hetero => mask_from(&|a| a.hetero),
        Expr::Ligand => mask_from(&|a| a.hetero && !is_water(&a.residue_name)),
        Expr::Water => mask_from(&|a| is_water(&a.residue_name)),
        Expr::Hydrogen => mask_from(&|a| a.element == Element::H),
        Expr::Backbone => mask_from(&|a| !a.hetero && is_backbone(&a.name)),
        Expr::Sidechain => {
            mask_from(&|a| !a.hetero && a.element != Element::H && !is_backbone(&a.name))
        }
        Expr::Chain(c) => mask_from(&|a| a.chain_id == *c),
        Expr::ResName(name) => mask_from(&|a| a.residue_name.eq_ignore_ascii_case(name)),
        Expr::Element(e) => mask_from(&|a| a.element == *e),
        Expr::ResId(lo, hi) => mask_from(&|a| a.residue_seq >= *lo && a.residue_seq <= *hi),
        Expr::Numeric { field, op, value } => mask_from(&|a| {
            let lhs = match field {
                NumField::BFactor => a.b_factor,
                NumField::Occupancy => a.occupancy,
            };
            op.apply(lhs, *value)
        }),
        Expr::And(l, r) => zip_mask(eval(l, structure, ctx), eval(r, structure, ctx), |a, b| {
            a && b
        }),
        Expr::Or(l, r) => zip_mask(eval(l, structure, ctx), eval(r, structure, ctx), |a, b| {
            a || b
        }),
        Expr::Not(inner) => eval(inner, structure, ctx).iter().map(|b| !b).collect(),
        Expr::ByRes(inner) => {
            let s = eval(inner, structure, ctx);
            let keys: HashSet<(String, i32, String)> =
                selected(&s).map(|i| residue_key(&atoms[i])).collect();
            mask_from(&|a| keys.contains(&residue_key(a)))
        }
        Expr::ByChain(inner) => {
            let s = eval(inner, structure, ctx);
            let chains: HashSet<String> = selected(&s).map(|i| atoms[i].chain_id.clone()).collect();
            mask_from(&|a| chains.contains(&a.chain_id))
        }
        Expr::ByMol(inner) => {
            let s = eval(inner, structure, ctx);
            let comp = ctx.components();
            let mols: HashSet<usize> = selected(&s).map(|i| comp[i]).collect();
            (0..atoms.len()).map(|i| mols.contains(&comp[i])).collect()
        }
        Expr::Within(r, inner) => {
            let s = eval(inner, structure, ctx);
            within_mask(structure, &s, *r)
        }
        Expr::Expand(r, inner) => {
            let s = eval(inner, structure, ctx);
            within_mask(structure, &s, *r)
        }
        Expr::Around(r, inner) => {
            let s = eval(inner, structure, ctx);
            let w = within_mask(structure, &s, *r);
            zip_mask(w, s, |inside, seed| inside && !seed)
        }
        Expr::Beyond(r, inner) => {
            let s = eval(inner, structure, ctx);
            within_mask(structure, &s, *r).iter().map(|b| !b).collect()
        }
    }
}

fn zip_mask(a: Vec<bool>, b: Vec<bool>, f: impl Fn(bool, bool) -> bool) -> Vec<bool> {
    a.into_iter().zip(b).map(|(x, y)| f(x, y)).collect()
}

fn selected(mask: &[bool]) -> impl Iterator<Item = usize> + '_ {
    mask.iter().enumerate().filter_map(|(i, &b)| b.then_some(i))
}

fn residue_key(a: &Atom) -> (String, i32, String) {
    (a.chain_id.clone(), a.residue_seq, a.residue_name.clone())
}

/// Atoms within `r` Å of any seeded atom (the seeds themselves included).
fn within_mask(structure: &Structure, seeds: &[bool], r: f64) -> Vec<bool> {
    let n = structure.atoms.len();
    let mut mask = vec![false; n];
    if r < 0.0 || !seeds.iter().any(|&b| b) {
        return mask;
    }
    let mut tree: KdTree<f64, 3> = KdTree::with_capacity(n);
    for (i, a) in structure.atoms.iter().enumerate() {
        tree.add(&[a.x, a.y, a.z], i as u64);
    }
    let r2 = r * r;
    for i in selected(seeds) {
        let a = &structure.atoms[i];
        for nn in tree.within_unsorted::<SquaredEuclidean>(&[a.x, a.y, a.z], r2) {
            mask[nn.item as usize] = true;
        }
    }
    mask
}

/// Union-find connected components over `bonds`; returns `atom -> root id`.
fn union_find_components(n: usize, bonds: &[(usize, usize)]) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..n).collect();
    for &(a, b) in bonds {
        let ra = find(&mut parent, a);
        let rb = find(&mut parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }
    (0..n).map(|i| find(&mut parent, i)).collect()
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]]; // path halving
        x = parent[x];
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::Atom;

    #[allow(clippy::too_many_arguments)]
    fn atom(
        i: usize,
        name: &str,
        elem: &str,
        resn: &str,
        resi: i32,
        chain: &str,
        het: bool,
        b: f64,
        q: f64,
    ) -> Atom {
        Atom {
            serial: i,
            name: name.into(),
            element: Element::from_symbol(elem),
            residue_name: resn.into(),
            residue_seq: resi,
            chain_id: chain.into(),
            hetero: het,
            b_factor: b,
            occupancy: q,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    /// Coordinate-aware atom builder for spatial / bymol tests.
    fn at(i: usize, elem: &str, resi: i32, x: f64, y: f64, z: f64) -> Atom {
        Atom {
            serial: i,
            name: elem.into(),
            element: Element::from_symbol(elem),
            residue_name: "LIG".into(),
            residue_seq: resi,
            chain_id: "A".into(),
            hetero: false,
            b_factor: 0.0,
            occupancy: 1.0,
            x,
            y,
            z,
        }
    }

    fn fixture() -> Structure {
        // b-factors increase by index (10..=60); occupancy is 1.0 except CB.
        Structure::new(vec![
            atom(0, "N", "N", "ALA", 1, "A", false, 10.0, 1.0),
            atom(1, "CA", "C", "ALA", 1, "A", false, 20.0, 1.0),
            atom(2, "C", "C", "ALA", 1, "A", false, 30.0, 1.0),
            atom(3, "CB", "C", "ALA", 1, "B", false, 40.0, 0.5),
            atom(4, "O", "O", "HOH", 101, "A", true, 50.0, 1.0),
            atom(5, "FE", "FE", "FE", 201, "A", true, 60.0, 1.0),
        ])
    }

    #[test]
    fn all_and_none() {
        let s = fixture();
        assert_eq!(evaluate(&s, &Expr::All), vec![0, 1, 2, 3, 4, 5]);
        assert!(evaluate(&s, &Expr::None).is_empty());
    }

    #[test]
    fn classification_macros() {
        let s = fixture();
        assert_eq!(evaluate(&s, &Expr::Protein), vec![0, 1, 2, 3]);
        assert_eq!(evaluate(&s, &Expr::Hetero), vec![4, 5]);
        assert_eq!(evaluate(&s, &Expr::Water), vec![4]);
        assert_eq!(evaluate(&s, &Expr::Ligand), vec![5]);
        assert_eq!(evaluate(&s, &Expr::Backbone), vec![0, 1, 2]);
        // sidechain: non-hetero, non-backbone, non-hydrogen -> just CB.
        assert_eq!(evaluate(&s, &Expr::Sidechain), vec![3]);
    }

    #[test]
    fn nucleic_by_residue_name() {
        // One DNA residue (DA), one protein residue (ALA), one water — only the
        // DA atoms are nucleic; protein() stays the broader non-hetero set.
        let s = Structure::new(vec![
            atom(0, "P", "P", "DA", 1, "A", false, 0.0, 1.0),
            atom(1, "CA", "C", "ALA", 2, "A", false, 0.0, 1.0),
            atom(2, "O", "O", "HOH", 101, "A", true, 0.0, 1.0),
        ]);
        assert_eq!(evaluate(&s, &Expr::Nucleic), vec![0]);
        assert_eq!(evaluate(&s, &Expr::Protein), vec![0, 1]);
    }

    #[test]
    fn predicates() {
        let s = fixture();
        assert_eq!(evaluate(&s, &Expr::chain("A")), vec![0, 1, 2, 4, 5]);
        assert_eq!(evaluate(&s, &Expr::element("C")), vec![1, 2, 3]);
        assert_eq!(evaluate(&s, &Expr::resn("HOH")), vec![4]);
        assert_eq!(evaluate(&s, &Expr::resi(1, 1)), vec![0, 1, 2, 3]);
        assert_eq!(evaluate(&s, &Expr::resi(100, 200)), vec![4]);
    }

    #[test]
    fn boolean_composition_evaluates() {
        let s = fixture();
        // chain A = {0,1,2,4,5}, water = {4}
        assert_eq!(evaluate(&s, &Expr::chain("A").and(Expr::Water)), vec![4]);
        // protein = {0,1,2,3}, water = {4}
        assert_eq!(
            evaluate(&s, &Expr::Protein.or(Expr::Water)),
            vec![0, 1, 2, 3, 4]
        );
        // no hydrogens in fixture -> not hydrogen = all
        assert_eq!(evaluate(&s, &Expr::Hydrogen.not()), vec![0, 1, 2, 3, 4, 5]);
        // nested
        assert_eq!(
            evaluate(&s, &Expr::chain("A").and(Expr::Protein).or(Expr::Ligand)),
            vec![0, 1, 2, 5]
        );
    }

    #[test]
    fn numeric_predicates() {
        let s = fixture();
        let b = |op, v| Expr::numeric(NumField::BFactor, op, v);
        let q = |op, v| Expr::numeric(NumField::Occupancy, op, v);
        assert_eq!(evaluate(&s, &b(CmpOp::Gt, 30.0)), vec![3, 4, 5]);
        assert_eq!(evaluate(&s, &b(CmpOp::Lt, 30.0)), vec![0, 1]);
        assert_eq!(evaluate(&s, &b(CmpOp::Ge, 30.0)), vec![2, 3, 4, 5]);
        assert_eq!(evaluate(&s, &q(CmpOp::Eq, 1.0)), vec![0, 1, 2, 4, 5]);
        assert_eq!(evaluate(&s, &q(CmpOp::Ne, 1.0)), vec![3]);
    }

    #[test]
    fn aggregation() {
        let s = fixture();
        // element N -> atom 0 (ALA 1 chain A); byres expands to that residue.
        assert_eq!(evaluate(&s, &Expr::element("N").byres()), vec![0, 1, 2]);
        // resi 201 -> FE in chain A; bychain expands to all chain A.
        assert_eq!(
            evaluate(&s, &Expr::resi(201, 201).bychain()),
            vec![0, 1, 2, 4, 5]
        );
    }

    #[test]
    fn spatial_operators() {
        // Three atoms in a line, separate residues.
        let s = Structure::new(vec![
            at(0, "C", 1, 0.0, 0.0, 0.0),
            at(1, "C", 2, 1.5, 0.0, 0.0),
            at(2, "C", 3, 3.0, 0.0, 0.0),
        ]);
        let r1 = || Expr::resi(1, 1);
        // within 2.0 of resi 1: atom0 (self) + atom1 (1.5) ; atom2 (3.0) excluded.
        assert_eq!(evaluate(&s, &r1().within(2.0)), vec![0, 1]);
        // around excludes the seed itself.
        assert_eq!(evaluate(&s, &r1().around(2.0)), vec![1]);
        // beyond is the complement of within.
        assert_eq!(evaluate(&s, &r1().beyond(2.0)), vec![2]);
        // expand behaves like within.
        assert_eq!(evaluate(&s, &r1().expand(2.0)), vec![0, 1]);
        // boundary: exactly the radius is included.
        assert_eq!(evaluate(&s, &r1().within(1.5)), vec![0, 1]);
        assert_eq!(evaluate(&s, &r1().within(1.4)), vec![0]);
    }

    #[test]
    fn bymol_uses_connected_components() {
        // atom0-atom1 bonded (1.5 Å); atom2 far away (isolated).
        let s = Structure::new(vec![
            at(0, "C", 1, 0.0, 0.0, 0.0),
            at(1, "C", 1, 1.5, 0.0, 0.0),
            at(2, "O", 2, 20.0, 0.0, 0.0),
        ]);
        // bymol of resi 1 (atoms 0,1) -> their whole bonded molecule = {0,1}.
        assert_eq!(evaluate(&s, &Expr::resi(1, 1).bymol()), vec![0, 1]);
        // bymol of the isolated atom -> just itself.
        assert_eq!(evaluate(&s, &Expr::resi(2, 2).bymol()), vec![2]);
    }

    #[test]
    fn deliverable_selection() {
        // chain A & around(ligand, 2) & ~water on a small arrangement.
        // atom0 ligand (hetero, chain A); atom1 protein near it; atom2 water near it.
        let mut a0 = at(0, "C", 1, 0.0, 0.0, 0.0);
        a0.hetero = true;
        a0.residue_name = "LIG".into();
        let mut a1 = at(1, "C", 2, 1.0, 0.0, 0.0);
        a1.hetero = false;
        let mut a2 = at(2, "O", 3, 1.2, 0.0, 0.0);
        a2.hetero = true;
        a2.residue_name = "HOH".into();
        let s = Structure::new(vec![a0, a1, a2]);
        // around 2 of ligand -> {1,2}; chain A -> all; not water -> drop atom2 => {1}
        let sel = Expr::chain("A")
            .and(Expr::Ligand.around(2.0))
            .and(Expr::Water.not());
        assert_eq!(evaluate(&s, &sel), vec![1]);
    }

    #[test]
    fn display_is_readable() {
        assert_eq!(Expr::Protein.to_string(), "protein");
        assert_eq!(Expr::chain("A").to_string(), "chain A");
        assert_eq!(Expr::resi(10, 30).to_string(), "resi 10-30");
        assert_eq!(Expr::resi(42, 42).to_string(), "resi 42");
        assert_eq!(
            Expr::numeric(NumField::BFactor, CmpOp::Gt, 30.0).to_string(),
            "b > 30"
        );
        assert_eq!(
            Expr::chain("A").and(Expr::Protein).to_string(),
            "(chain A) and (protein)"
        );
        assert_eq!(Expr::Hydrogen.not().to_string(), "not (hydrogen)");
        assert_eq!(Expr::Ligand.around(4.0).to_string(), "around 4 of (ligand)");
    }
}
