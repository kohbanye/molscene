//! Selection evaluation.
//!
//! A selection is a human-readable string (so a `Scene` stays AI-generatable and
//! hand-editable). This module compiles it to an expression tree ([`Expr`]) and
//! evaluates that tree against a [`Structure`] into atom indices.
//!
//! The language has boolean composition (`and` / `or` / `not`, grouped with
//! parens), spatial operators (`around` / `within` / `expand` / `beyond … of`,
//! backed by a k-d tree), aggregation (`byres` / `bychain` / `bymol`), numeric
//! predicates (`b` / `q` comparisons), plus the classification macros and
//! single-clause predicates (`chain`, `resn`, `element`, `resi`, …).

use std::cell::RefCell;
use std::collections::HashSet;

use kiddo::{KdTree, SquaredEuclidean};

use crate::structure::{Atom, Structure};

const WATER_RESNAMES: [&str; 6] = ["HOH", "WAT", "H2O", "TIP3", "TIP", "SOL"];
const BACKBONE_NAMES: [&str; 4] = ["N", "CA", "C", "O"];

fn is_water(residue_name: &str) -> bool {
    let r = residue_name.trim().to_ascii_uppercase();
    WATER_RESNAMES.contains(&r.as_str())
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
}

/// A parsed selection expression tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    All,
    None,
    // Classification macros.
    Protein, // protein | polymer | nucleic  (non-hetero)
    Hetero,  // hetero | hetatm
    Ligand,
    Water, // water | solvent
    Hydrogen,
    Backbone,
    Sidechain,
    // Single-clause predicates.
    Chain(String),
    ResName(String),
    Element(String),
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

/// An error produced while parsing a selection string.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    /// Reached the end of input while more was expected.
    UnexpectedEnd { expected: &'static str },
    /// Found a token where a different one was expected.
    Expected {
        expected: &'static str,
        found: String,
    },
    /// A keyword that isn't part of the language.
    UnknownKeyword(String),
    /// A number that didn't parse.
    BadNumber(String),
    /// A spatial radius that was negative.
    NegativeRadius(f64),
    /// A `resi` range that didn't parse.
    BadRange(String),
    /// Leftover tokens after a complete expression.
    TrailingInput(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::UnexpectedEnd { expected } => {
                write!(f, "unexpected end of selection; expected {expected}")
            }
            ParseError::Expected { expected, found } => {
                write!(f, "expected {expected}, found {found:?}")
            }
            ParseError::UnknownKeyword(k) => write!(f, "unknown selection keyword {k:?}"),
            ParseError::BadNumber(s) => write!(f, "invalid number {s:?}"),
            ParseError::NegativeRadius(r) => write!(f, "spatial radius must be >= 0, got {r}"),
            ParseError::BadRange(s) => write!(f, "invalid resi range {s:?}"),
            ParseError::TrailingInput(s) => {
                write!(f, "unexpected trailing input starting at {s:?}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// A bareword: keywords and predicate arguments (`chain`, `A`, `10-30`, `4.0`).
    Ident(String),
    LParen,
    RParen,
    Cmp(CmpOp),
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            c if c.is_whitespace() => i += 1,
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '&' => {
                tokens.push(Token::Ident("and".into()));
                i += 1;
            }
            '|' => {
                tokens.push(Token::Ident("or".into()));
                i += 1;
            }
            '<' | '>' | '=' | '!' => {
                let next = bytes.get(i + 1).map(|b| *b as char);
                let (op, len) = match (c, next) {
                    ('<', Some('=')) => (CmpOp::Le, 2),
                    ('<', _) => (CmpOp::Lt, 1),
                    ('>', Some('=')) => (CmpOp::Ge, 2),
                    ('>', _) => (CmpOp::Gt, 1),
                    ('=', Some('=')) => (CmpOp::Eq, 2),
                    ('=', _) => (CmpOp::Eq, 1),
                    ('!', Some('=')) => (CmpOp::Ne, 2),
                    // bare `!` is a `not` alias.
                    ('!', _) => {
                        tokens.push(Token::Ident("not".into()));
                        i += 1;
                        continue;
                    }
                    _ => unreachable!(),
                };
                tokens.push(Token::Cmp(op));
                i += len;
            }
            _ => {
                let start = i;
                while i < bytes.len() {
                    let ch = bytes[i] as char;
                    if ch.is_whitespace()
                        || matches!(ch, '(' | ')' | '&' | '|' | '<' | '>' | '=' | '!')
                    {
                        break;
                    }
                    i += 1;
                }
                tokens.push(Token::Ident(input[start..i].to_string()));
            }
        }
    }
    tokens
}

// ---------------------------------------------------------------------------
// Parser (recursive descent; precedence: or < and < prefix unary)
// ---------------------------------------------------------------------------

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

/// Parse a selection string into an expression tree.
pub fn parse(selection: &str) -> Result<Expr, ParseError> {
    let mut parser = Parser {
        tokens: tokenize(selection),
        pos: 0,
    };
    // An empty selection means "everything".
    if parser.tokens.is_empty() {
        return Ok(Expr::All);
    }
    let expr = parser.parse_or()?;
    if let Some(tok) = parser.peek() {
        return Err(ParseError::TrailingInput(describe(tok)));
    }
    Ok(expr)
}

fn describe(tok: &Token) -> String {
    match tok {
        Token::Ident(s) => s.clone(),
        Token::LParen => "(".into(),
        Token::RParen => ")".into(),
        Token::Cmp(_) => "comparison".into(),
    }
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let tok = self.tokens.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    /// If the next token is the given keyword, consume it and return true.
    fn eat_keyword(&mut self, kw: &str) -> bool {
        if let Some(Token::Ident(s)) = self.peek() {
            if s.eq_ignore_ascii_case(kw) {
                self.pos += 1;
                return true;
            }
        }
        false
    }

    fn expect_keyword(&mut self, kw: &'static str) -> Result<(), ParseError> {
        match self.next() {
            Some(Token::Ident(ref s)) if s.eq_ignore_ascii_case(kw) => Ok(()),
            Some(tok) => Err(ParseError::Expected {
                expected: kw,
                found: describe(&tok),
            }),
            None => Err(ParseError::UnexpectedEnd { expected: kw }),
        }
    }

    fn next_ident(&mut self, expected: &'static str) -> Result<String, ParseError> {
        match self.next() {
            Some(Token::Ident(s)) => Ok(s),
            Some(tok) => Err(ParseError::Expected {
                expected,
                found: describe(&tok),
            }),
            None => Err(ParseError::UnexpectedEnd { expected }),
        }
    }

    fn next_number(&mut self) -> Result<f64, ParseError> {
        let s = self.next_ident("a number")?;
        s.parse::<f64>().map_err(|_| ParseError::BadNumber(s))
    }

    fn next_cmp(&mut self) -> Result<CmpOp, ParseError> {
        match self.next() {
            Some(Token::Cmp(op)) => Ok(op),
            Some(tok) => Err(ParseError::Expected {
                expected: "a comparison operator",
                found: describe(&tok),
            }),
            None => Err(ParseError::UnexpectedEnd {
                expected: "a comparison operator",
            }),
        }
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_and()?;
        while self.eat_keyword("or") {
            let rhs = self.parse_and()?;
            lhs = Expr::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_unary()?;
        while self.eat_keyword("and") {
            let rhs = self.parse_unary()?;
            lhs = Expr::And(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.eat_keyword("not") {
            return Ok(Expr::Not(Box::new(self.parse_unary()?)));
        }
        if self.eat_keyword("byres") {
            return Ok(Expr::ByRes(Box::new(self.parse_unary()?)));
        }
        if self.eat_keyword("bychain") {
            return Ok(Expr::ByChain(Box::new(self.parse_unary()?)));
        }
        if self.eat_keyword("bymol") {
            return Ok(Expr::ByMol(Box::new(self.parse_unary()?)));
        }
        for kw in ["around", "within", "expand", "beyond"] {
            if self.eat_keyword(kw) {
                let radius = self.next_number()?;
                if radius < 0.0 {
                    return Err(ParseError::NegativeRadius(radius));
                }
                self.expect_keyword("of")?;
                let operand = Box::new(self.parse_unary()?);
                return Ok(match kw {
                    "around" => Expr::Around(radius, operand),
                    "within" => Expr::Within(radius, operand),
                    "expand" => Expr::Expand(radius, operand),
                    _ => Expr::Beyond(radius, operand),
                });
            }
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.next() {
            Some(Token::LParen) => {
                let inner = self.parse_or()?;
                match self.next() {
                    Some(Token::RParen) => Ok(inner),
                    Some(tok) => Err(ParseError::Expected {
                        expected: ")",
                        found: describe(&tok),
                    }),
                    None => Err(ParseError::UnexpectedEnd { expected: ")" }),
                }
            }
            Some(Token::Ident(kw)) => self.parse_keyword(&kw),
            Some(tok) => Err(ParseError::Expected {
                expected: "a selection term",
                found: describe(&tok),
            }),
            None => Err(ParseError::UnexpectedEnd {
                expected: "a selection term",
            }),
        }
    }

    fn parse_keyword(&mut self, kw: &str) -> Result<Expr, ParseError> {
        let lower = kw.to_ascii_lowercase();
        Ok(match lower.as_str() {
            "all" => Expr::All,
            "none" => Expr::None,
            "protein" | "polymer" | "nucleic" => Expr::Protein,
            "hetero" | "hetatm" => Expr::Hetero,
            "ligand" => Expr::Ligand,
            "water" | "solvent" => Expr::Water,
            "hydrogen" | "hydrogens" => Expr::Hydrogen,
            "backbone" => Expr::Backbone,
            "sidechain" => Expr::Sidechain,
            "chain" => Expr::Chain(self.next_ident("a chain id")?),
            "resn" | "resname" => Expr::ResName(self.next_ident("a residue name")?),
            "element" | "elem" => Expr::Element(self.next_ident("an element symbol")?),
            "resi" | "resid" => {
                let spec = self.next_ident("a residue number or range")?;
                let (lo, hi) = parse_range(&spec).ok_or(ParseError::BadRange(spec))?;
                Expr::ResId(lo, hi)
            }
            "b" | "q" => {
                let field = if lower == "b" {
                    NumField::BFactor
                } else {
                    NumField::Occupancy
                };
                let op = self.next_cmp()?;
                let value = self.next_number()?;
                Expr::Numeric { field, op, value }
            }
            _ => return Err(ParseError::UnknownKeyword(kw.to_string())),
        })
    }
}

/// Parse `"N"` or `"N-M"` into an inclusive `(lo, hi)` range (allowing a leading
/// negative low bound).
fn parse_range(spec: &str) -> Option<(i32, i32)> {
    if let Some(rel) = spec.get(1..).and_then(|s| s.find('-')) {
        let idx = rel + 1;
        let lo = spec[..idx].trim().parse().ok()?;
        let hi = spec[idx + 1..].trim().parse().ok()?;
        Some((lo, hi))
    } else {
        let n: i32 = spec.trim().parse().ok()?;
        Some((n, n))
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// Resolve a selection string to the matching atom indices (sorted ascending).
///
/// On a parse error this warns and selects nothing; the Python bindings validate
/// selections up front (raising `ValueError`) so this path is defensive.
pub fn evaluate(structure: &Structure, selection: &str) -> Vec<usize> {
    match parse(selection) {
        Ok(expr) => {
            let ctx = EvalCtx::new(structure);
            let mask = eval(&expr, structure, &ctx);
            mask.iter()
                .enumerate()
                .filter_map(|(i, &b)| b.then_some(i))
                .collect()
        }
        Err(e) => {
            eprintln!("molscene: invalid selection {selection:?}: {e}; selecting nothing.");
            Vec::new()
        }
    }
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
        Expr::Hetero => mask_from(&|a| a.hetero),
        Expr::Ligand => mask_from(&|a| a.hetero && !is_water(&a.residue_name)),
        Expr::Water => mask_from(&|a| is_water(&a.residue_name)),
        Expr::Hydrogen => mask_from(&|a| a.element.eq_ignore_ascii_case("H")),
        Expr::Backbone => mask_from(&|a| !a.hetero && is_backbone(&a.name)),
        Expr::Sidechain => mask_from(&|a| {
            !a.hetero && !a.element.eq_ignore_ascii_case("H") && !is_backbone(&a.name)
        }),
        Expr::Chain(c) => mask_from(&|a| a.chain_id == *c),
        Expr::ResName(name) => mask_from(&|a| a.residue_name.eq_ignore_ascii_case(name)),
        Expr::Element(e) => mask_from(&|a| a.element.eq_ignore_ascii_case(e)),
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
            element: elem.into(),
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
            element: elem.into(),
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
        // sidechain: non-hetero, non-backbone, non-hydrogen -> just CB.
        assert_eq!(evaluate(&s, "sidechain"), vec![3]);
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
    fn boolean_composition_evaluates() {
        let s = fixture();
        // chain A = {0,1,2,4,5}, water = {4}
        assert_eq!(evaluate(&s, "(chain A) and (water)"), vec![4]);
        // protein = {0,1,2,3}, water = {4}
        assert_eq!(evaluate(&s, "(protein) or (water)"), vec![0, 1, 2, 3, 4]);
        // no hydrogens in fixture -> not hydrogen = all
        assert_eq!(evaluate(&s, "not (hydrogen)"), vec![0, 1, 2, 3, 4, 5]);
        // nested
        assert_eq!(
            evaluate(&s, "((chain A) and (protein)) or (ligand)"),
            vec![0, 1, 2, 5]
        );
    }

    #[test]
    fn parses_operator_emitted_strings() {
        // Exactly what the Python `& | ~` operators produce.
        let s = fixture();
        assert_eq!(evaluate(&s, "(chain A) and (ligand)"), vec![5]);
        assert_eq!(evaluate(&s, "not (water)"), vec![0, 1, 2, 3, 5]);
    }

    #[test]
    fn numeric_predicates() {
        let s = fixture();
        assert_eq!(evaluate(&s, "b > 30"), vec![3, 4, 5]);
        assert_eq!(evaluate(&s, "b < 30"), vec![0, 1]);
        assert_eq!(evaluate(&s, "b >= 30"), vec![2, 3, 4, 5]);
        assert_eq!(evaluate(&s, "q = 1"), vec![0, 1, 2, 4, 5]);
        assert_eq!(evaluate(&s, "q != 1"), vec![3]);
    }

    #[test]
    fn aggregation() {
        let s = fixture();
        // element N -> atom 0 (ALA 1 chain A); byres expands to that residue.
        assert_eq!(evaluate(&s, "byres (element N)"), vec![0, 1, 2]);
        // resi 201 -> FE in chain A; bychain expands to all chain A.
        assert_eq!(evaluate(&s, "bychain (resi 201)"), vec![0, 1, 2, 4, 5]);
    }

    #[test]
    fn spatial_operators() {
        // Three atoms in a line, separate residues.
        let s = Structure::new(vec![
            at(0, "C", 1, 0.0, 0.0, 0.0),
            at(1, "C", 2, 1.5, 0.0, 0.0),
            at(2, "C", 3, 3.0, 0.0, 0.0),
        ]);
        // within 2.0 of resi 1: atom0 (self) + atom1 (1.5) ; atom2 (3.0) excluded.
        assert_eq!(evaluate(&s, "within 2.0 of (resi 1)"), vec![0, 1]);
        // around excludes the seed itself.
        assert_eq!(evaluate(&s, "around 2.0 of (resi 1)"), vec![1]);
        // beyond is the complement of within.
        assert_eq!(evaluate(&s, "beyond 2.0 of (resi 1)"), vec![2]);
        // expand behaves like within.
        assert_eq!(evaluate(&s, "expand 2.0 of (resi 1)"), vec![0, 1]);
        // boundary: exactly the radius is included.
        assert_eq!(evaluate(&s, "within 1.5 of (resi 1)"), vec![0, 1]);
        assert_eq!(evaluate(&s, "within 1.4 of (resi 1)"), vec![0]);
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
        assert_eq!(evaluate(&s, "bymol (resi 1)"), vec![0, 1]);
        // bymol of the isolated atom -> just itself.
        assert_eq!(evaluate(&s, "bymol (resi 2)"), vec![2]);
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
        let got = evaluate(
            &s,
            "((chain A) and (around 2.0 of (ligand))) and (not (water))",
        );
        assert_eq!(got, vec![1]);
    }

    #[test]
    fn parse_errors() {
        assert!(matches!(
            parse("chain"),
            Err(ParseError::UnexpectedEnd { .. })
        ));
        assert!(matches!(
            parse("around of (ligand)"),
            Err(ParseError::BadNumber(_))
        ));
        assert!(matches!(
            parse("around 4 (ligand)"),
            Err(ParseError::Expected { expected: "of", .. })
        ));
        assert!(matches!(
            parse("(chain A"),
            Err(ParseError::UnexpectedEnd { expected: ")" })
        ));
        assert!(matches!(
            parse("b >"),
            Err(ParseError::UnexpectedEnd { .. })
        ));
        assert!(matches!(
            parse("frobnicate"),
            Err(ParseError::UnknownKeyword(_))
        ));
        assert!(matches!(
            parse("around -1 of (ligand)"),
            Err(ParseError::NegativeRadius(_))
        ));
        // a parse error selects nothing (and warns).
        assert!(evaluate(&fixture(), "frobnicate").is_empty());
    }
}
