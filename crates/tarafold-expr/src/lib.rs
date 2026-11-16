//! Expression trees for Tarafold.
//!
//! An [`Expr`] is stored in postfix order, which is compact, cheap to evaluate column-wise,
//! and easy to splice. Operations use *protected* semantics: domain errors produce `NaN`
//! instead of silently changing the meaning of the equation (design §4).

mod eval;
mod parse;
mod print;

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use parse::{ParseError, parse};
pub use print::Tree;

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UnOp {
    /// `-a`
    Neg,
    /// `exp(a)`
    Exp,
    /// `log(a)`, natural log, `a > 0`
    Log,
    /// `sqrt(a)`, `a ≥ 0`
    Sqrt,
    /// `square(a)` = a²
    Square,
    /// `abs(a)`
    Abs,
    /// `sin(a)`
    Sin,
    /// `cos(a)`
    Cos,
    /// `tanh(a)`
    Tanh,
    /// `inv(a)` = 1/a
    Inv,
}

impl UnOp {
    /// Function name used in infix form (`Neg` prints as a prefix minus).
    pub fn name(self) -> &'static str {
        match self {
            Self::Neg => "neg",
            Self::Exp => "exp",
            Self::Log => "log",
            Self::Sqrt => "sqrt",
            Self::Square => "square",
            Self::Abs => "abs",
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tanh => "tanh",
            Self::Inv => "inv",
        }
    }

    /// Look up a function by name.
    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "exp" => Self::Exp,
            "log" | "ln" => Self::Log,
            "sqrt" => Self::Sqrt,
            "square" => Self::Square,
            "abs" => Self::Abs,
            "sin" => Self::Sin,
            "cos" => Self::Cos,
            "tanh" => Self::Tanh,
            "inv" => Self::Inv,
            _ => return None,
        })
    }
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BinOp {
    /// `a + b`
    Add,
    /// `a - b`
    Sub,
    /// `a * b`
    Mul,
    /// `a / b`, `|b| ≥ 1e-12`
    Div,
    /// `a ^ b`
    Pow,
}

/// One postfix node.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Node {
    /// Literal constant.
    Const(f64),
    /// Variable by index into the variable list.
    Var(u16),
    /// Unary operator applied to the top of the stack.
    Unary(UnOp),
    /// Binary operator applied to the two top stack entries.
    Binary(BinOp),
}

/// An expression in postfix order. Always well-formed when built through the public API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expr {
    nodes: Vec<Node>,
}

impl Expr {
    /// Constant expression.
    pub fn constant(c: f64) -> Self {
        Self { nodes: vec![Node::Const(c)] }
    }

    /// Variable expression.
    pub fn var(i: u16) -> Self {
        Self { nodes: vec![Node::Var(i)] }
    }

    /// Apply a unary operator.
    pub fn unary(mut self, op: UnOp) -> Self {
        self.nodes.push(Node::Unary(op));
        self
    }

    /// Combine two expressions with a binary operator.
    pub fn binary(op: BinOp, mut a: Self, b: Self) -> Self {
        a.nodes.extend(b.nodes);
        a.nodes.push(Node::Binary(op));
        a
    }

    /// `a + b`
    pub fn add(a: Self, b: Self) -> Self {
        Self::binary(BinOp::Add, a, b)
    }

    /// `a * b`
    pub fn mul(a: Self, b: Self) -> Self {
        Self::binary(BinOp::Mul, a, b)
    }

    /// `a ^ b`
    pub fn pow(a: Self, b: Self) -> Self {
        Self::binary(BinOp::Pow, a, b)
    }

    /// Postfix nodes.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Always false for a built expression; present for API completeness.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Weighted node count: 2 for `exp`, `log`, `pow`; 1 otherwise (design §4).
    pub fn complexity(&self) -> usize {
        self.nodes
            .iter()
            .map(|n| match n {
                Node::Unary(UnOp::Exp | UnOp::Log) | Node::Binary(BinOp::Pow) => 2,
                _ => 1,
            })
            .sum()
    }

    /// Variable indices used, sorted.
    pub fn variables(&self) -> BTreeSet<u16> {
        self.nodes
            .iter()
            .filter_map(|n| if let Node::Var(i) = n { Some(*i) } else { None })
            .collect()
    }

    /// Constants in postfix order.
    pub fn constants(&self) -> Vec<f64> {
        self.nodes
            .iter()
            .filter_map(|n| if let Node::Const(c) = n { Some(*c) } else { None })
            .collect()
    }

    /// Rename variables: `Var(i)` becomes `Var(map[i])`.
    pub fn remap_vars(&self, map: &[u16]) -> Self {
        Self {
            nodes: self
                .nodes
                .iter()
                .map(|n| match n {
                    Node::Var(i) => Node::Var(map.get(*i as usize).copied().unwrap_or(*i)),
                    other => *other,
                })
                .collect(),
        }
    }

    /// Fold constant sub-expressions and drop algebraic identities that are always safe:
    /// `x*1`, `1*x`, `x+0`, `0+x`, `x-0`, `x/1`, `x^1`. Values are unchanged.
    pub fn simplify(&self) -> Self {
        let mut stack: Vec<Expr> = Vec::new();
        for &n in &self.nodes {
            match n {
                Node::Const(_) | Node::Var(_) => stack.push(Expr { nodes: vec![n] }),
                Node::Unary(op) => {
                    let Some(a) = stack.pop() else { return self.clone() };
                    if let [Node::Const(c)] = a.nodes[..] {
                        let v = eval::apply_unary(op, c);
                        if v.is_finite() {
                            stack.push(Expr::constant(v));
                            continue;
                        }
                    }
                    stack.push(a.unary(op));
                }
                Node::Binary(op) => {
                    let (Some(b), Some(a)) = (stack.pop(), stack.pop()) else {
                        return self.clone();
                    };
                    let ca = if let [Node::Const(c)] = a.nodes[..] { Some(c) } else { None };
                    let cb = if let [Node::Const(c)] = b.nodes[..] { Some(c) } else { None };
                    if let (Some(x), Some(y)) = (ca, cb) {
                        let v = eval::apply_binary(op, x, y);
                        if v.is_finite() {
                            stack.push(Expr::constant(v));
                            continue;
                        }
                    }
                    let out = match (op, ca, cb) {
                        (BinOp::Mul, Some(c), _) if c == 1.0 => b,
                        (BinOp::Mul | BinOp::Div | BinOp::Pow, _, Some(c)) if c == 1.0 => a,
                        (BinOp::Add, Some(c), _) if c == 0.0 => b,
                        (BinOp::Add | BinOp::Sub, _, Some(c)) if c == 0.0 => a,
                        _ => Expr::binary(op, a, b),
                    };
                    stack.push(out);
                }
            }
        }
        match (stack.pop(), stack.is_empty()) {
            (Some(e), true) => e,
            _ => self.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
