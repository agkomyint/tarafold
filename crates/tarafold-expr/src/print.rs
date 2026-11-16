//! Printing: infix (exact and pretty), LaTeX, and a JSON tree for the UI evaluator.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{BinOp, Expr, Node, UnOp};

/// Expression tree for JSON output (`report.json` and the browser evaluator).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Tree {
    /// `{"const": 2.5}`
    Const {
        /// Value.
        #[serde(rename = "const")]
        value: f64,
    },
    /// `{"var": "price"}`
    Var {
        /// Variable name.
        var: String,
    },
    /// `{"op": "mul", "args": [...]}`
    Op {
        /// Operator name: `add sub mul div pow neg exp log sqrt square abs sin cos tanh inv`.
        op: String,
        /// Operands.
        args: Vec<Tree>,
    },
}

const PREC_ADD: u8 = 1;
const PREC_MUL: u8 = 2;
const PREC_NEG: u8 = 3;
const PREC_POW: u8 = 4;
const PREC_ATOM: u8 = 5;

fn bin_prec(op: BinOp) -> u8 {
    match op {
        BinOp::Add | BinOp::Sub => PREC_ADD,
        BinOp::Mul | BinOp::Div => PREC_MUL,
        BinOp::Pow => PREC_POW,
    }
}

fn bin_symbol(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Pow => "^",
    }
}

fn bin_name(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "add",
        BinOp::Sub => "sub",
        BinOp::Mul => "mul",
        BinOp::Div => "div",
        BinOp::Pow => "pow",
    }
}

/// True when `name` can be printed bare and parsed back as one identifier.
pub(crate) fn is_plain_ident(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && UnOp::from_name(name).is_none()
}

fn fmt_ident(name: &str) -> String {
    if is_plain_ident(name) { name.to_string() } else { format!("`{}`", name.replace('`', "")) }
}

/// Shortest round-trip representation (`{:?}` gives e.g. `2.5`, `3.0`, `1e-7`).
fn fmt_exact(c: f64) -> String {
    format!("{c:?}")
}

/// Human-friendly constant with `sig` significant digits and no trailing zeros.
pub(crate) fn fmt_sig(c: f64, sig: usize) -> String {
    if c == 0.0 {
        return "0".into();
    }
    let mag = c.abs().log10().floor() as i32;
    if !(-4..=9).contains(&mag) {
        return format!("{:.*e}", sig.saturating_sub(1), c);
    }
    let decimals = (sig as i32 - 1 - mag).max(0) as usize;
    let s = format!("{c:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

impl Expr {
    fn infix_with(&self, names: &[String], fmt_c: &dyn Fn(f64) -> String) -> String {
        // Stack of (text, precedence).
        let mut stack: Vec<(String, u8)> = Vec::new();
        for node in &self.nodes {
            match *node {
                Node::Const(c) => {
                    let p = if c.is_sign_negative() { PREC_NEG } else { PREC_ATOM };
                    stack.push((fmt_c(c), p));
                }
                Node::Var(i) => {
                    let name = names.get(i as usize).cloned().unwrap_or_else(|| format!("x{i}"));
                    stack.push((fmt_ident(&name), PREC_ATOM));
                }
                Node::Unary(op) => {
                    let (a, pa) = stack.pop().unwrap_or_default();
                    if op == UnOp::Neg {
                        let a = if pa <= PREC_NEG { format!("({a})") } else { a };
                        stack.push((format!("-{a}"), PREC_NEG));
                    } else {
                        stack.push((format!("{}({a})", op.name()), PREC_ATOM));
                    }
                }
                Node::Binary(op) => {
                    let (b, pb) = stack.pop().unwrap_or_default();
                    let (a, pa) = stack.pop().unwrap_or_default();
                    let p = bin_prec(op);
                    let (left_paren, right_paren) =
                        if op == BinOp::Pow { (pa <= p, pb < p) } else { (pa < p, pb <= p) };
                    let a = if left_paren { format!("({a})") } else { a };
                    let b = if right_paren { format!("({b})") } else { b };
                    let text = if op == BinOp::Pow {
                        format!("{a}^{b}")
                    } else {
                        format!("{a} {} {b}", bin_symbol(op))
                    };
                    stack.push((text, p));
                }
            }
        }
        stack.pop().map(|(s, _)| s).unwrap_or_default()
    }

    /// Exact infix form that [`crate::parse`] reads back to the identical expression.
    pub fn to_infix(&self, names: &[String]) -> String {
        self.infix_with(names, &fmt_exact)
    }

    /// Readable infix form with constants rounded to `sig` significant digits.
    pub fn to_infix_pretty(&self, names: &[String], sig: usize) -> String {
        self.infix_with(names, &|c| fmt_sig(c, sig))
    }

    /// LaTeX form with constants rounded to `sig` significant digits.
    pub fn to_latex(&self, names: &[String], sig: usize) -> String {
        let mut stack: Vec<(String, u8)> = Vec::new();
        let paren = |s: String| format!("\\left({s}\\right)");
        for node in &self.nodes {
            match *node {
                Node::Const(c) => {
                    let s = fmt_sig(c, sig);
                    let s = if let Some((m, e)) = s.split_once('e') {
                        format!("{m} \\times 10^{{{}}}", e.trim_start_matches('+'))
                    } else {
                        s
                    };
                    stack.push((s, if c.is_sign_negative() { PREC_NEG } else { PREC_ATOM }));
                }
                Node::Var(i) => {
                    let name = names.get(i as usize).cloned().unwrap_or_else(|| format!("x{i}"));
                    let esc = name.replace('\\', "").replace('_', "\\_").replace(' ', "\\ ");
                    stack.push((format!("\\mathrm{{{esc}}}"), PREC_ATOM));
                }
                Node::Unary(op) => {
                    let (a, pa) = stack.pop().unwrap_or_default();
                    let out = match op {
                        UnOp::Neg => {
                            (format!("-{}", if pa <= PREC_NEG { paren(a) } else { a }), PREC_NEG)
                        }
                        UnOp::Sqrt => (format!("\\sqrt{{{a}}}"), PREC_ATOM),
                        UnOp::Square => {
                            (format!("{}^{{2}}", if pa < PREC_ATOM { paren(a) } else { a }), PREC_POW)
                        }
                        UnOp::Abs => (format!("\\left|{a}\\right|"), PREC_ATOM),
                        UnOp::Inv => (format!("\\frac{{1}}{{{a}}}"), PREC_ATOM),
                        UnOp::Exp => (format!("e^{{{a}}}"), PREC_POW),
                        UnOp::Log => (format!("\\ln{}", paren(a)), PREC_ATOM),
                        UnOp::Sin | UnOp::Cos | UnOp::Tanh => {
                            (format!("\\{}{}", op.name(), paren(a)), PREC_ATOM)
                        }
                    };
                    stack.push(out);
                }
                Node::Binary(op) => {
                    let (b, pb) = stack.pop().unwrap_or_default();
                    let (a, pa) = stack.pop().unwrap_or_default();
                    let out = match op {
                        BinOp::Div => (format!("\\frac{{{a}}}{{{b}}}"), PREC_ATOM),
                        BinOp::Pow => (
                            format!("{}^{{{b}}}", if pa <= PREC_POW { paren(a) } else { a }),
                            PREC_POW,
                        ),
                        _ => {
                            let p = bin_prec(op);
                            let a = if pa < p { paren(a) } else { a };
                            let b = if pb <= p { paren(b) } else { b };
                            let sym = match op {
                                BinOp::Add => "+",
                                BinOp::Sub => "-",
                                _ => "\\cdot",
                            };
                            (format!("{a} {sym} {b}"), p)
                        }
                    };
                    stack.push(out);
                }
            }
        }
        stack.pop().map(|(s, _)| s).unwrap_or_default()
    }

    /// JSON tree with variable names resolved.
    pub fn to_tree(&self, names: &[String]) -> Tree {
        let mut stack: Vec<Tree> = Vec::new();
        for node in &self.nodes {
            match *node {
                Node::Const(c) => stack.push(Tree::Const { value: c }),
                Node::Var(i) => stack.push(Tree::Var {
                    var: names.get(i as usize).cloned().unwrap_or_else(|| format!("x{i}")),
                }),
                Node::Unary(op) => {
                    let a = stack.pop().unwrap_or(Tree::Const { value: f64::NAN });
                    stack.push(Tree::Op { op: op.name().into(), args: vec![a] });
                }
                Node::Binary(op) => {
                    let b = stack.pop().unwrap_or(Tree::Const { value: f64::NAN });
                    let a = stack.pop().unwrap_or(Tree::Const { value: f64::NAN });
                    stack.push(Tree::Op { op: bin_name(op).into(), args: vec![a, b] });
                }
            }
        }
        stack.pop().unwrap_or(Tree::Const { value: f64::NAN })
    }
}
