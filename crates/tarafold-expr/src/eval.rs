//! Evaluation with protected semantics, column-wise and at single points, plus
//! forward-mode derivatives.

use crate::{BinOp, Expr, Node, UnOp};

const DIV_EPS: f64 = 1e-12;

pub(crate) fn apply_unary(op: UnOp, a: f64) -> f64 {
    match op {
        UnOp::Neg => -a,
        UnOp::Exp => a.exp(),
        UnOp::Log => {
            if a > 0.0 {
                a.ln()
            } else {
                f64::NAN
            }
        }
        UnOp::Sqrt => {
            if a >= 0.0 {
                a.sqrt()
            } else {
                f64::NAN
            }
        }
        UnOp::Square => a * a,
        UnOp::Abs => a.abs(),
        UnOp::Sin => a.sin(),
        UnOp::Cos => a.cos(),
        UnOp::Tanh => a.tanh(),
        UnOp::Inv => {
            if a.abs() >= DIV_EPS {
                1.0 / a
            } else {
                f64::NAN
            }
        }
    }
}

pub(crate) fn apply_binary(op: BinOp, a: f64, b: f64) -> f64 {
    match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => {
            if b.abs() >= DIV_EPS {
                a / b
            } else {
                f64::NAN
            }
        }
        BinOp::Pow => {
            if a < 0.0 && b.fract() != 0.0 {
                f64::NAN
            } else {
                a.powf(b)
            }
        }
    }
}

/// Value and derivative of a unary op given `(a, da)`.
fn dual_unary(op: UnOp, a: f64, da: f64) -> (f64, f64) {
    let v = apply_unary(op, a);
    let d = match op {
        UnOp::Neg => -da,
        UnOp::Exp => v * da,
        UnOp::Log => da / a,
        UnOp::Sqrt => da / (2.0 * v),
        UnOp::Square => 2.0 * a * da,
        UnOp::Abs => a.signum() * da,
        UnOp::Sin => a.cos() * da,
        UnOp::Cos => -a.sin() * da,
        UnOp::Tanh => (1.0 - v * v) * da,
        UnOp::Inv => -da / (a * a),
    };
    (v, if v.is_nan() { f64::NAN } else { d })
}

/// Value and derivative of a binary op given `(a, da)`, `(b, db)`.
fn dual_binary(op: BinOp, a: f64, da: f64, b: f64, db: f64) -> (f64, f64) {
    let v = apply_binary(op, a, b);
    let d = match op {
        BinOp::Add => da + db,
        BinOp::Sub => da - db,
        BinOp::Mul => da * b + a * db,
        BinOp::Div => (da * b - a * db) / (b * b),
        BinOp::Pow => {
            let base_part = if da == 0.0 { 0.0 } else { b * apply_binary(BinOp::Pow, a, b - 1.0) * da };
            let exp_part = if db == 0.0 { 0.0 } else { v * a.ln() * db };
            base_part + exp_part
        }
    };
    (v, if v.is_nan() { f64::NAN } else { d })
}

impl Expr {
    /// Evaluate on whole columns. `vars[i]` is the column for `Var(i)`; all columns must
    /// have length `n`. Returns `None` if the expression references a missing variable.
    pub fn eval_columns(&self, vars: &[&[f64]], n: usize) -> Option<Vec<f64>> {
        let mut stack: Vec<Vec<f64>> = Vec::with_capacity(8);
        for node in &self.nodes {
            match *node {
                Node::Const(c) => stack.push(vec![c; n]),
                Node::Var(i) => {
                    let col = vars.get(i as usize)?;
                    if col.len() != n {
                        return None;
                    }
                    stack.push(col.to_vec());
                }
                Node::Unary(op) => {
                    let a = stack.last_mut()?;
                    for v in a.iter_mut() {
                        *v = apply_unary(op, *v);
                    }
                }
                Node::Binary(op) => {
                    let b = stack.pop()?;
                    let a = stack.last_mut()?;
                    for (x, y) in a.iter_mut().zip(&b) {
                        *x = apply_binary(op, *x, *y);
                    }
                }
            }
        }
        if stack.len() == 1 { stack.pop() } else { None }
    }

    /// Evaluate at one point. Missing variables evaluate to `NaN`.
    pub fn eval_point(&self, x: &[f64]) -> f64 {
        let mut stack: Vec<f64> = Vec::with_capacity(16);
        for node in &self.nodes {
            match *node {
                Node::Const(c) => stack.push(c),
                Node::Var(i) => stack.push(x.get(i as usize).copied().unwrap_or(f64::NAN)),
                Node::Unary(op) => {
                    let Some(a) = stack.pop() else { return f64::NAN };
                    stack.push(apply_unary(op, a));
                }
                Node::Binary(op) => {
                    let (Some(b), Some(a)) = (stack.pop(), stack.pop()) else {
                        return f64::NAN;
                    };
                    stack.push(apply_binary(op, a, b));
                }
            }
        }
        if stack.len() == 1 { stack[0] } else { f64::NAN }
    }

    /// Value and partial derivative with respect to `x[var]` at one point (forward mode).
    pub fn eval_partial(&self, x: &[f64], var: u16) -> (f64, f64) {
        let mut stack: Vec<(f64, f64)> = Vec::with_capacity(16);
        for node in &self.nodes {
            match *node {
                Node::Const(c) => stack.push((c, 0.0)),
                Node::Var(i) => {
                    let v = x.get(i as usize).copied().unwrap_or(f64::NAN);
                    stack.push((v, if i == var { 1.0 } else { 0.0 }));
                }
                Node::Unary(op) => {
                    let Some((a, da)) = stack.pop() else { return (f64::NAN, f64::NAN) };
                    stack.push(dual_unary(op, a, da));
                }
                Node::Binary(op) => {
                    let (Some((b, db)), Some((a, da))) = (stack.pop(), stack.pop()) else {
                        return (f64::NAN, f64::NAN);
                    };
                    stack.push(dual_binary(op, a, da, b, db));
                }
            }
        }
        if stack.len() == 1 { stack[0] } else { (f64::NAN, f64::NAN) }
    }

    /// Gradient with respect to every variable `0..n_vars` at one point.
    pub fn gradient(&self, x: &[f64], n_vars: usize) -> Vec<f64> {
        let used = self.variables();
        (0..n_vars)
            .map(|i| {
                let i16 = u16::try_from(i).unwrap_or(u16::MAX);
                if used.contains(&i16) { self.eval_partial(x, i16).1 } else { 0.0 }
            })
            .collect()
    }
}
