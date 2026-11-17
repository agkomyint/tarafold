//! Expression trees for Tarafold.
//!
//! An [`Expr`] is stored in postfix order, which is compact, cheap to evaluate column-wise,
//! and easy to splice. Operations use *protected* semantics: domain errors produce `NaN`
//! instead of silently changing the meaning of the equation (design §4).

mod eval;
mod parse;
mod print;
mod simplify;

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
        Self {
            nodes: vec![Node::Const(c)],
        }
    }

    /// Variable expression.
    pub fn var(i: u16) -> Self {
        Self {
            nodes: vec![Node::Var(i)],
        }
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

    /// `a ^ b`
    pub fn pow(a: Self, b: Self) -> Self {
        Self::binary(BinOp::Pow, a, b)
    }

    /// Postfix nodes.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Build from postfix nodes; `None` unless the nodes form exactly one well-formed expression.
    pub fn from_nodes(nodes: Vec<Node>) -> Option<Self> {
        let mut depth: usize = 0;
        for n in &nodes {
            match n {
                Node::Const(c) if !c.is_finite() => return None,
                Node::Const(_) | Node::Var(_) => depth += 1,
                Node::Unary(_) => {
                    if depth == 0 {
                        return None;
                    }
                }
                Node::Binary(_) => {
                    if depth < 2 {
                        return None;
                    }
                    depth -= 1;
                }
            }
        }
        (depth == 1).then_some(Self { nodes })
    }

    /// Index of the first node of the subtree that ends at node `end` (postfix order).
    pub fn subtree_start(&self, end: usize) -> usize {
        let mut need: i64 = 1;
        let mut i = end.min(self.nodes.len().saturating_sub(1));
        loop {
            need -= 1;
            need += match self.nodes[i] {
                Node::Const(_) | Node::Var(_) => 0,
                Node::Unary(_) => 1,
                Node::Binary(_) => 2,
            };
            if need == 0 || i == 0 {
                return i;
            }
            i -= 1;
        }
    }

    /// The subtree ending at node `end`.
    pub fn subtree(&self, end: usize) -> Self {
        let s = self.subtree_start(end);
        Self {
            nodes: self.nodes[s..=end].to_vec(),
        }
    }

    /// Replace the subtree ending at node `end` with `with`.
    pub fn replace_subtree(&self, end: usize, with: &Expr) -> Self {
        let s = self.subtree_start(end);
        let mut nodes = Vec::with_capacity(self.nodes.len() + with.nodes.len());
        nodes.extend_from_slice(&self.nodes[..s]);
        nodes.extend_from_slice(&with.nodes);
        nodes.extend_from_slice(&self.nodes[end + 1..]);
        Self { nodes }
    }

    /// Replace the node at `i` with another node of the same arity. Returns `None` on arity
    /// mismatch.
    pub fn with_node(&self, i: usize, node: Node) -> Option<Self> {
        let arity = |n: &Node| match n {
            Node::Const(_) | Node::Var(_) => 0,
            Node::Unary(_) => 1,
            Node::Binary(_) => 2,
        };
        let old = self.nodes.get(i)?;
        if arity(old) != arity(&node) {
            return None;
        }
        let mut nodes = self.nodes.clone();
        nodes[i] = node;
        Some(Self { nodes })
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
            .filter_map(|n| {
                if let Node::Const(c) = n {
                    Some(*c)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Same structure with constants replaced, in postfix order. Extra values are ignored and
    /// missing ones keep their current value.
    pub fn with_constants(&self, values: &[f64]) -> Self {
        let mut it = values.iter();
        Self {
            nodes: self
                .nodes
                .iter()
                .map(|n| match n {
                    Node::Const(c) => Node::Const(it.next().copied().unwrap_or(*c)),
                    other => *other,
                })
                .collect(),
        }
    }

    /// Replace every `Var(var)` with the expression `with` (equation chaining).
    pub fn substitute(&self, var: u16, with: &Expr) -> Self {
        let mut nodes = Vec::with_capacity(self.nodes.len() + with.nodes.len());
        for n in &self.nodes {
            match n {
                Node::Var(i) if *i == var => nodes.extend_from_slice(&with.nodes),
                other => nodes.push(*other),
            }
        }
        Self { nodes }
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

    /// Algebraic simplification that preserves values: constant folding, identities, sign
    /// and constant normalization, `a * inv(b) → a / b`, `x * x → square(x)`, and more
    /// (see the `simplify` module). Repeated until nothing changes.
    pub fn simplify(&self) -> Self {
        simplify::simplify(self)
    }
}
impl std::ops::Add for Expr {
    type Output = Expr;

    /// `a + b`.
    fn add(self, rhs: Expr) -> Expr {
        Expr::binary(BinOp::Add, self, rhs)
    }
}

impl std::ops::Mul for Expr {
    type Output = Expr;

    /// `a * b`.
    fn mul(self, rhs: Expr) -> Expr {
        Expr::binary(BinOp::Mul, self, rhs)
    }
}

#[cfg(test)]
mod tests;
