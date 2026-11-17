//! Rule-based algebraic simplification, applied bottom-up until nothing changes.
//!
//! Every rule preserves values (up to floating-point rounding), including protected
//! semantics: a rule is only used when both sides give `NaN` on the same inputs. This replaces
//! the e-graph approach in design §4 for v0.1: the rule set is small, deterministic, and adds
//! no dependency; equivalence checks elsewhere use numeric comparison.

use crate::{BinOp, Expr, Node, UnOp, eval};

fn konst(e: &Expr) -> Option<f64> {
    if let [Node::Const(c)] = e.nodes[..] {
        Some(c)
    } else {
        None
    }
}

/// The operand of a top-level unary op.
fn un(e: &Expr, op: UnOp) -> Option<Expr> {
    match e.nodes.last() {
        Some(Node::Unary(o)) if *o == op && e.len() >= 2 => Some(e.subtree(e.len() - 2)),
        _ => None,
    }
}

/// The operands of a top-level binary op.
fn bin(e: &Expr, op: BinOp) -> Option<(Expr, Expr)> {
    match e.nodes.last() {
        Some(Node::Binary(o)) if *o == op && e.len() >= 3 => {
            let right_end = e.len() - 2;
            let left_end = e.subtree_start(right_end).checked_sub(1)?;
            Some((e.subtree(left_end), e.subtree(right_end)))
        }
        _ => None,
    }
}

fn unary(op: UnOp, a: Expr) -> Expr {
    if let Some(c) = konst(&a) {
        let v = eval::apply_unary(op, c);
        if v.is_finite() {
            return Expr::constant(v);
        }
    }
    match op {
        UnOp::Neg => {
            if let Some(x) = un(&a, UnOp::Neg) {
                return x; // -(-x) = x
            }
        }
        UnOp::Sqrt => {
            if let Some(x) = un(&a, UnOp::Square) {
                return x.unary(UnOp::Abs); // sqrt(x²) = |x|
            }
        }
        _ => {}
    }
    a.unary(op)
}

fn binary(op: BinOp, a: Expr, b: Expr) -> Expr {
    let (ca, cb) = (konst(&a), konst(&b));
    if let (Some(x), Some(y)) = (ca, cb) {
        let v = eval::apply_binary(op, x, y);
        if v.is_finite() {
            return Expr::constant(v);
        }
    }
    match op {
        BinOp::Add => {
            if ca == Some(0.0) {
                return b;
            }
            if cb == Some(0.0) {
                return a;
            }
            if let Some(c) = cb.filter(|c| *c < 0.0) {
                return Expr::binary(BinOp::Sub, a, Expr::constant(-c)); // a + (-c) = a - c
            }
            if let Some(x) = un(&b, UnOp::Neg) {
                return Expr::binary(BinOp::Sub, a, x); // a + (-x) = a - x
            }
            if let Some(x) = un(&a, UnOp::Neg) {
                return Expr::binary(BinOp::Sub, b, x); // (-x) + b = b - x
            }
            // c1 + (c2 + x) and (x + c2) + c1 → (c1 + c2) + x
            if let Some(c1) = ca
                && let Some((l, r)) = bin(&b, BinOp::Add)
            {
                if let Some(c2) = konst(&l) {
                    return binary(BinOp::Add, Expr::constant(c1 + c2), r);
                }
                if let Some(c2) = konst(&r) {
                    return binary(BinOp::Add, Expr::constant(c1 + c2), l);
                }
            }
            if let Some(c1) = cb
                && let Some((l, r)) = bin(&a, BinOp::Add)
                && let Some(c2) = konst(&r)
            {
                return binary(BinOp::Add, l, Expr::constant(c1 + c2));
            }
            // a + (-c) * x = a - c * x
            if let Some((l, r)) = bin(&b, BinOp::Mul)
                && let Some(c) = konst(&l).filter(|c| *c < 0.0)
            {
                return Expr::binary(BinOp::Sub, a, binary(BinOp::Mul, Expr::constant(-c), r));
            }
        }
        BinOp::Sub => {
            if cb == Some(0.0) {
                return a;
            }
            if let Some(c) = cb.filter(|c| *c < 0.0) {
                return Expr::binary(BinOp::Add, a, Expr::constant(-c)); // a - (-c) = a + c
            }
            if let Some(x) = un(&b, UnOp::Neg) {
                return Expr::binary(BinOp::Add, a, x); // a - (-x) = a + x
            }
        }
        BinOp::Mul => {
            if ca == Some(1.0) {
                return b;
            }
            if cb == Some(1.0) {
                return a;
            }
            // Constants first: x * c → c * x.
            if cb.is_some() && ca.is_none() {
                return binary(BinOp::Mul, b, a);
            }
            if let Some(c) = ca {
                if let Some(x) = un(&b, UnOp::Neg) {
                    return binary(BinOp::Mul, Expr::constant(-c), x); // c * (-x) = (-c) * x
                }
                if let Some((l, r)) = bin(&b, BinOp::Mul)
                    && let Some(c2) = konst(&l)
                {
                    return binary(BinOp::Mul, Expr::constant(c * c2), r); // c1 * (c2 * x)
                }
            }
            if let Some(y) = un(&b, UnOp::Inv) {
                return binary(BinOp::Div, a, y); // a * inv(y) = a / y
            }
            if let Some(y) = un(&a, UnOp::Inv) {
                return binary(BinOp::Div, b, y); // inv(y) * b = b / y
            }
            if a == b {
                return a.unary(UnOp::Square); // x * x = square(x)
            }
        }
        BinOp::Div => {
            if cb == Some(1.0) {
                return a;
            }
            // (c * x) / d = (c / d) * x
            if let Some(d) = cb
                && let Some((l, r)) = bin(&a, BinOp::Mul)
                && let Some(c) = konst(&l)
            {
                let q = c / d;
                if q.is_finite() && d.abs() >= 1e-12 {
                    return binary(BinOp::Mul, Expr::constant(q), r);
                }
            }
        }
        BinOp::Pow => match cb {
            Some(1.0) => return a,
            Some(2.0) => return a.unary(UnOp::Square),
            Some(0.5) => return a.unary(UnOp::Sqrt),
            _ => {}
        },
    }
    Expr::binary(op, a, b)
}

fn pass(e: &Expr) -> Expr {
    let mut stack: Vec<Expr> = Vec::new();
    for &n in &e.nodes {
        match n {
            Node::Const(_) | Node::Var(_) => stack.push(Expr { nodes: vec![n] }),
            Node::Unary(op) => {
                let Some(a) = stack.pop() else {
                    return e.clone();
                };
                stack.push(unary(op, a));
            }
            Node::Binary(op) => {
                let (Some(b), Some(a)) = (stack.pop(), stack.pop()) else {
                    return e.clone();
                };
                stack.push(binary(op, a, b));
            }
        }
    }
    match (stack.pop(), stack.is_empty()) {
        (Some(out), true) => out,
        _ => e.clone(),
    }
}

/// Simplify to a fixed point (at most 8 passes).
pub(crate) fn simplify(e: &Expr) -> Expr {
    let mut cur = e.clone();
    for _ in 0..8 {
        let next = pass(&cur);
        if next == cur {
            break;
        }
        cur = next;
    }
    cur
}
