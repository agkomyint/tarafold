//! Basis library for sparse regression (design §5a).

use tarafold_core::stats;
use tarafold_expr::{BinOp, Expr, UnOp};

/// One candidate basis term with its training-column values.
#[derive(Debug, Clone)]
pub(crate) struct Term {
    pub expr: Expr,
    pub train: Vec<f64>,
    /// Feature indices; a single entry when the term is `log(x_j)`.
    pub log_of: Option<u16>,
}

fn all_finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// Is `x` bounded away from zero on every row (needed for `1/x` and ratios)?
fn away_from_zero(x: &[f64]) -> bool {
    let max = x.iter().map(|v| v.abs()).fold(0.0, f64::max);
    let min = x.iter().map(|v| v.abs()).fold(f64::INFINITY, f64::min);
    max > 0.0 && min > 1e-6 * max
}

/// Build the library from feature columns. `check` holds extra rows (validation) on which every
/// term must also be finite, so candidates never fail on validation data by construction.
pub(crate) fn library(
    train: &[Vec<f64>],
    check: &[Vec<f64>],
    y: &[f64],
    max_terms: usize,
) -> Vec<Term> {
    let p = train.len();
    let n = y.len();
    let mut out: Vec<(Term, bool)> = Vec::new(); // (term, always_keep)
    let full = |j: usize| -> Vec<f64> { train[j].iter().chain(&check[j]).copied().collect() };
    let mut push = |expr: Expr, keep: bool, log_of: Option<u16>| {
        let refs: Vec<&[f64]> = train.iter().map(Vec::as_slice).collect();
        let Some(v) = expr.eval_columns(&refs, n) else { return };
        let crefs: Vec<&[f64]> = check.iter().map(Vec::as_slice).collect();
        let m = check.first().map_or(0, Vec::len);
        let ok_check = m == 0 || expr.eval_columns(&crefs, m).is_some_and(|c| all_finite(&c));
        if all_finite(&v) && ok_check && stats::std(&v) > 0.0 {
            out.push((Term { expr, train: v, log_of }, keep));
        }
    };
    for j in 0..p {
        let j16 = j as u16;
        let x = full(j);
        let x_min = x.iter().copied().fold(f64::INFINITY, f64::min);
        push(Expr::var(j16), true, None);
        push(Expr::var(j16).unary(UnOp::Square), false, None);
        push(Expr::pow(Expr::var(j16), Expr::constant(3.0)), false, None);
        if away_from_zero(&x) {
            push(Expr::var(j16).unary(UnOp::Inv), false, None);
        }
        if x_min >= 0.0 {
            push(Expr::var(j16).unary(UnOp::Sqrt), false, None);
        }
        if x_min > 0.0 {
            push(Expr::var(j16).unary(UnOp::Log), false, Some(j16));
        }
    }
    for i in 0..p {
        for j in (i + 1)..p {
            push(Expr::mul(Expr::var(i as u16), Expr::var(j as u16)), false, None);
        }
    }
    // Degree-3 monomials: x_i^2 * x_j and x_i * x_j * x_k (e.g. m * v^2).
    for i in 0..p {
        for j in 0..p {
            if i != j {
                push(
                    Expr::mul(Expr::var(i as u16), Expr::var(j as u16).unary(UnOp::Square)),
                    false,
                    None,
                );
            }
        }
    }
    for i in 0..p {
        for j in (i + 1)..p {
            for k in (j + 1)..p {
                let e = Expr::mul(Expr::mul(Expr::var(i as u16), Expr::var(j as u16)), Expr::var(k as u16));
                push(e, false, None);
            }
        }
    }
    for i in 0..p {
        for j in 0..p {
            if i != j && away_from_zero(&full(j)) {
                push(Expr::binary(BinOp::Div, Expr::var(i as u16), Expr::var(j as u16)), false, None);
            }
        }
    }

    // Cap by |corr(term, y)|, always keeping the single-feature transforms (x, x², log x, …)
    // so exhaustive small-subset search can combine them.
    let mut scored: Vec<(f64, usize)> = out
        .iter()
        .enumerate()
        .map(|(i, (t, keep))| {
            let unary = t.expr.variables().len() == 1;
            (if *keep { 3.0 } else if unary { 2.0 } else { stats::pearson(&t.train, y).abs() }, i)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut chosen: Vec<usize> = scored.iter().take(max_terms).map(|&(_, i)| i).collect();
    chosen.sort_unstable();

    // Drop near-duplicates (|r| > 0.99999) so least squares stays well conditioned.
    let mut terms: Vec<Term> = Vec::new();
    for i in chosen {
        let t = &out[i].0;
        if terms.iter().all(|u| stats::pearson(&u.train, &t.train).abs() < 0.99999) {
            terms.push(t.clone());
        }
    }
    terms
}
