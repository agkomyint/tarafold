//! Equation discovery (R6, design §5).
//!
//! v0.1 Phase 1 implements the sparse basis regression stage (5a) with linear, log-log and
//! log-target variants, the Pareto front, and the selection rule. Genetic-programming search
//! (5b) plugs into the same candidate pool in Phase 3.

mod basis;
mod enet;
mod lm;
mod subset;

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tarafold_core::config::DiscoverConfig;
use tarafold_core::{linalg, stats};
use tarafold_expr::{Expr, UnOp};

use basis::Term;

/// Which strategy produced a candidate.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    /// Mean of the target (reference point).
    Constant,
    /// Sparse regression of `y` on the basis library.
    SparseLinear,
    /// Sparse regression of `log y` on `log x` (power laws and products).
    LogLog,
    /// Sparse regression of `log y` on the basis library (exponentials).
    LogTarget,
}

/// One candidate equation over the feature index space.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// The expression; variables index the input feature list.
    pub expr: Expr,
    /// Weighted node count.
    pub complexity: usize,
    /// Training normalized MSE (MSE / Var(y)).
    pub train_nmse: f64,
    /// Validation normalized MSE.
    pub val_nmse: f64,
    /// Strategy.
    pub strategy: Strategy,
}

/// What each strategy did, for the report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StrategyRun {
    /// Strategy.
    pub strategy: Strategy,
    /// Whether it ran.
    pub ran: bool,
    /// Why it was skipped, if it was.
    pub skipped_reason: Option<String>,
    /// Candidates produced.
    pub candidates: usize,
}

/// Input to discovery: column-major features for train and validation rows.
pub struct DiscoverInput<'a> {
    /// Training feature columns.
    pub train_x: &'a [Vec<f64>],
    /// Training target.
    pub train_y: &'a [f64],
    /// Validation feature columns.
    pub val_x: &'a [Vec<f64>],
    /// Validation target.
    pub val_y: &'a [f64],
    /// Settings.
    pub config: &'a DiscoverConfig,
}

/// Discovery output.
#[derive(Debug, Clone)]
pub struct DiscoverOutput {
    /// Pareto front, sorted by complexity.
    pub front: Vec<Candidate>,
    /// Index into `front` of the selected equation.
    pub selected: usize,
    /// Strategy log.
    pub strategies: Vec<StrategyRun>,
    /// Total candidates evaluated.
    pub evaluated: usize,
}

/// Discovery failures.
#[derive(Debug, Clone, PartialEq)]
pub enum DiscoverError {
    /// Inputs are inconsistent or empty.
    BadInput(String),
}

impl std::fmt::Display for DiscoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadInput(m) => write!(f, "invalid discovery input: {m}"),
        }
    }
}

impl std::error::Error for DiscoverError {}

/// Normalized MSE of `expr` on columns, `inf` when evaluation fails or is non-finite.
pub fn nmse(expr: &Expr, x: &[Vec<f64>], y: &[f64]) -> f64 {
    let refs: Vec<&[f64]> = x.iter().map(Vec::as_slice).collect();
    let Some(pred) = expr.eval_columns(&refs, y.len()) else {
        return f64::INFINITY;
    };
    let var = stats::variance(y);
    let mse = pred
        .iter()
        .zip(y)
        .map(|(p, t)| (p - t) * (p - t))
        .sum::<f64>()
        / y.len() as f64;
    if !mse.is_finite() {
        return f64::INFINITY;
    }
    if var > 0.0 { mse / var } else { mse }
}

/// `c0 + Σ c_i * t_i`, omitting the intercept when `c0` is `None`. Negative coefficients
/// print as subtraction and unit coefficients are dropped.
fn linear_expr(c0: Option<f64>, coefs: &[f64], terms: &[&Expr]) -> Expr {
    let scaled = |c: f64, t: &Expr| -> Expr {
        if c == 1.0 {
            t.clone()
        } else {
            Expr::constant(c) * t.clone()
        }
    };
    let mut acc: Option<Expr> = c0.filter(|c| *c != 0.0).map(Expr::constant);
    for (&c, t) in coefs.iter().zip(terms) {
        if c == 0.0 {
            continue;
        }
        acc = Some(match acc {
            None if c == -1.0 => (*t).clone().unary(UnOp::Neg),
            None => scaled(c, t),
            Some(a) if c < 0.0 => Expr::binary(tarafold_expr::BinOp::Sub, a, scaled(-c, t)),
            Some(a) => a + scaled(c, t),
        });
    }
    acc.unwrap_or_else(|| Expr::constant(c0.unwrap_or(0.0)))
        .simplify()
}

/// Round coefficients that are within `1e-6` (relative) of an integer.
fn snap_integers(c: &[f64]) -> Option<Vec<f64>> {
    let snapped: Vec<f64> = c
        .iter()
        .map(|&v| {
            if (v - v.round()).abs() < 1e-6 * v.abs().max(1.0) {
                v.round()
            } else {
                v
            }
        })
        .collect();
    (snapped != c).then_some(snapped)
}

/// `x^a` written in its simplest form.
fn power(var: u16, a: f64) -> Option<Expr> {
    let x = Expr::var(var);
    Some(match a {
        0.0 => return None,
        1.0 => x,
        2.0 => x.unary(UnOp::Square),
        0.5 => x.unary(UnOp::Sqrt),
        -1.0 => x.unary(UnOp::Inv),
        a => Expr::pow(x, Expr::constant(a)),
    })
}

/// Round exponents within 0.02 of a multiple of 0.5 (noise-robust power-law snapping).
fn snap_halves(c: &[f64]) -> Option<Vec<f64>> {
    let snapped: Vec<f64> = c
        .iter()
        .map(|&v| {
            let h = (v * 2.0).round() / 2.0;
            if (v - h).abs() < 0.02 { h } else { v }
        })
        .collect();
    (snapped != c).then_some(snapped)
}

/// Least-squares scale `k` for `y ≈ k·g`.
fn fit_scale(g: &[f64], y: &[f64]) -> Option<f64> {
    let gg: f64 = g.iter().map(|v| v * v).sum();
    let gy: f64 = g.iter().zip(y).map(|(a, b)| a * b).sum();
    let k = gy / gg;
    (gg > 0.0 && k.is_finite()).then_some(k)
}

/// Training rows with `y > 0`, used by the log-space strategies. Noise can push values of a
/// positive quantity below zero; those rows are left out of the log fit only. The log fit only
/// seeds the structure: constants are then refit by Levenberg–Marquardt on all rows in the
/// original units, which removes the bias of fitting in log space.
fn positive_rows(y: &[f64]) -> Result<Vec<usize>, String> {
    let rows: Vec<usize> = (0..y.len()).filter(|&i| y[i] > 0.0).collect();
    if rows.len() < 20 || rows.len() * 2 < y.len() {
        return Err(format!(
            "target is positive on only {} of {} training rows (needs ≥ 50%)",
            rows.len(),
            y.len()
        ));
    }
    Ok(rows)
}

/// Union of elastic-net path supports and subset-search supports, in a stable order.
fn all_supports(cols: &[&[f64]], y: &[f64], max_terms: usize) -> Vec<Vec<usize>> {
    let mut set: BTreeSet<Vec<usize>> = enet::supports(cols, y, &[0.95, 0.5], 100, max_terms)
        .into_iter()
        .collect();
    set.extend(subset::supports(cols, y, max_terms));
    set.into_iter().collect()
}

struct Pool<'a> {
    input: &'a DiscoverInput<'a>,
    cands: Vec<Candidate>,
    seen: BTreeSet<String>,
    evaluated: usize,
}

impl Pool<'_> {
    fn add(&mut self, expr: Expr, strategy: Strategy) -> bool {
        self.evaluated += 1;
        let key = format!("{:?}", expr.nodes());
        if !self.seen.insert(key) {
            return false;
        }
        let train_nmse = nmse(&expr, self.input.train_x, self.input.train_y);
        let val_nmse = nmse(&expr, self.input.val_x, self.input.val_y);
        if !train_nmse.is_finite() || !val_nmse.is_finite() {
            return false;
        }
        let complexity = expr.complexity();
        if matches!(strategy, Strategy::LogLog | Strategy::LogTarget)
            && let Some(refit) = lm::refine(&expr, self.input.train_x, self.input.train_y, 30)
        {
            let key = format!("{:?}", refit.nodes());
            let tn = nmse(&refit, self.input.train_x, self.input.train_y);
            let vn = nmse(&refit, self.input.val_x, self.input.val_y);
            if tn.is_finite() && vn.is_finite() && self.seen.insert(key) {
                self.cands.push(Candidate {
                    complexity: refit.complexity(),
                    expr: refit,
                    train_nmse: tn,
                    val_nmse: vn,
                    strategy,
                });
            }
        }
        self.cands.push(Candidate {
            expr,
            complexity,
            train_nmse,
            val_nmse,
            strategy,
        });
        true
    }
}

fn sparse_linear(
    pool: &mut Pool<'_>,
    terms: &[Term],
    y: &[f64],
    enet_rows: usize,
    max_terms: usize,
) -> usize {
    let n = y.len();
    let m = enet_rows.min(n);
    let cols: Vec<&[f64]> = terms.iter().map(|t| &t.train[..m]).collect();
    let mut added = 0;
    for support in all_supports(&cols, &y[..m], max_terms) {
        let texprs: Vec<&Expr> = support.iter().map(|&j| &terms[j].expr).collect();
        let raw: Vec<Vec<f64>> = support.iter().map(|&j| terms[j].train.clone()).collect();
        // With intercept.
        let mut with_c = vec![vec![1.0; n]];
        with_c.extend(raw.iter().cloned());
        if let Some(c) = linalg::lstsq(&with_c, y) {
            for coefs in std::iter::once(c.clone()).chain(snap_integers(&c)) {
                if pool.add(
                    linear_expr(Some(coefs[0]), &coefs[1..], &texprs),
                    Strategy::SparseLinear,
                ) {
                    added += 1;
                }
            }
        }
        // Through the origin.
        if let Some(c) = linalg::lstsq(&raw, y) {
            for coefs in std::iter::once(c.clone()).chain(snap_integers(&c)) {
                if pool.add(linear_expr(None, &coefs, &texprs), Strategy::SparseLinear) {
                    added += 1;
                }
            }
        }
    }
    added
}

fn log_log(
    pool: &mut Pool<'_>,
    y: &[f64],
    enet_rows: usize,
    max_terms: usize,
) -> Result<usize, String> {
    let input = pool.input;
    let positive = |c: &[f64]| c.iter().all(|&v| v > 0.0);
    let rows = positive_rows(y)?;
    let feats: Vec<u16> = (0..input.train_x.len())
        .filter(|&j| positive(&input.train_x[j]) && positive(&input.val_x[j]))
        .map(|j| j as u16)
        .collect();
    if feats.is_empty() {
        return Err("no strictly positive features".into());
    }
    let logx: Vec<Vec<f64>> = feats
        .iter()
        .map(|&j| {
            rows.iter()
                .map(|&i| input.train_x[j as usize][i].ln())
                .collect()
        })
        .collect();
    let logy: Vec<f64> = rows.iter().map(|&i| y[i].ln()).collect();
    let n = y.len();
    let nl = logy.len();
    let m = enet_rows.min(nl);
    let cols: Vec<&[f64]> = logx.iter().map(|c| &c[..m]).collect();
    let mut added = 0;
    for support in all_supports(&cols, &logy[..m], max_terms) {
        let mut a = vec![vec![1.0; nl]];
        a.extend(support.iter().map(|&j| logx[j].clone()));
        let Some(c) = linalg::lstsq(&a, &logy) else {
            continue;
        };
        let exps = c[1..].to_vec();
        for e in std::iter::once(exps.clone()).chain(snap_halves(&exps)) {
            let factors: Vec<Expr> = support
                .iter()
                .zip(&e)
                .filter_map(|(&j, &a)| power(feats[j], a))
                .collect();
            let Some(g_expr) = factors.into_iter().reduce(|a, b| a * b) else {
                continue;
            };
            let refs: Vec<&[f64]> = input.train_x.iter().map(Vec::as_slice).collect();
            let Some(g) = g_expr.eval_columns(&refs, n) else {
                continue;
            };
            let Some(k) = fit_scale(&g, y) else { continue };
            let expr = (Expr::constant(k) * g_expr.clone()).simplify();
            if pool.add(expr, Strategy::LogLog) {
                added += 1;
            }
            if let Some(ks) = snap_integers(&[k])
                && pool.add(
                    (Expr::constant(ks[0]) * g_expr).simplify(),
                    Strategy::LogLog,
                )
            {
                added += 1;
            }
        }
    }
    Ok(added)
}

/// `y = exp(c0 + Σ c_i t_i)`, rewritten as `K · Π x_j^{c_j} · exp(rest)` for log terms.
fn log_target(
    pool: &mut Pool<'_>,
    terms: &[Term],
    y: &[f64],
    enet_rows: usize,
    max_terms: usize,
) -> Result<usize, String> {
    let input = pool.input;
    let rows = positive_rows(y)?;
    let logy: Vec<f64> = rows.iter().map(|&i| y[i].ln()).collect();
    let sub: Vec<Vec<f64>> = terms
        .iter()
        .map(|t| rows.iter().map(|&i| t.train[i]).collect())
        .collect();
    let n = y.len();
    let nl = logy.len();
    let m = enet_rows.min(nl);
    let cols: Vec<&[f64]> = sub.iter().map(|c| &c[..m]).collect();
    let mut added = 0;
    for support in all_supports(&cols, &logy[..m], max_terms) {
        let mut a = vec![vec![1.0; nl]];
        a.extend(support.iter().map(|&j| sub[j].clone()));
        let Some(c) = linalg::lstsq(&a, &logy) else {
            continue;
        };
        for coefs in std::iter::once(c[1..].to_vec()).chain(snap_halves(&c[1..])) {
            let mut factors: Vec<Expr> = Vec::new();
            let mut rest_terms: Vec<&Expr> = Vec::new();
            let mut rest_coefs: Vec<f64> = Vec::new();
            for (&j, &cj) in support.iter().zip(&coefs) {
                match terms[j].log_of {
                    Some(v) => factors.extend(power(v, cj)),
                    None => {
                        rest_terms.push(&terms[j].expr);
                        rest_coefs.push(cj);
                    }
                }
            }
            if !rest_terms.is_empty() {
                factors.push(linear_expr(None, &rest_coefs, &rest_terms).unary(UnOp::Exp));
            }
            let Some(g_expr) = factors.into_iter().reduce(|a, b| a * b) else {
                continue;
            };
            let refs: Vec<&[f64]> = input.train_x.iter().map(Vec::as_slice).collect();
            let Some(g) = g_expr.eval_columns(&refs, n) else {
                continue;
            };
            if g.iter().any(|v| !v.is_finite()) {
                continue;
            }
            let Some(k) = fit_scale(&g, y) else { continue };
            if pool.add((Expr::constant(k) * g_expr).simplify(), Strategy::LogTarget) {
                added += 1;
            }
        }
    }
    Ok(added)
}

/// Refit the constants of a fixed structure by Levenberg–Marquardt (least squares on `y`).
/// Returns the input unchanged when no improvement is found.
pub fn refit_constants(expr: &Expr, x: &[Vec<f64>], y: &[f64]) -> Expr {
    lm::refine(expr, x, y, 100).unwrap_or_else(|| expr.clone())
}

/// Pareto front over (complexity, validation NMSE), sorted by complexity.
pub fn pareto_front(cands: &[Candidate], max: usize) -> Vec<Candidate> {
    let mut sorted: Vec<&Candidate> = cands.iter().collect();
    sorted.sort_by(|a, b| {
        a.complexity
            .cmp(&b.complexity)
            .then(a.val_nmse.total_cmp(&b.val_nmse))
            .then(a.strategy.cmp(&b.strategy))
    });
    let mut front: Vec<Candidate> = Vec::new();
    let mut best = f64::INFINITY;
    for c in sorted {
        if c.val_nmse < best {
            best = c.val_nmse;
            front.push(c.clone());
        }
    }
    if front.len() > max {
        let mut idx: Vec<usize> = (0..front.len()).collect();
        idx.sort_by(|&a, &b| front[a].val_nmse.total_cmp(&front[b].val_nmse));
        let mut keep: Vec<usize> = idx.into_iter().take(max).collect();
        keep.sort_unstable();
        front = keep.into_iter().map(|i| front[i].clone()).collect();
    }
    front
}

/// Selection rule (design §5c): among front members within 10% of the best validation loss,
/// pick the largest drop in log-loss per unit of complexity versus the previous front member.
pub fn select(front: &[Candidate]) -> usize {
    const FLOOR: f64 = 1e-14;
    let best = front
        .iter()
        .map(|c| c.val_nmse)
        .fold(f64::INFINITY, f64::min);
    let cutoff = best + (0.1 * best).max(1e-6);
    let mut pick = 0;
    let mut pick_score = f64::NEG_INFINITY;
    for i in 0..front.len() {
        if front[i].val_nmse > cutoff {
            continue;
        }
        let score = if i == 0 {
            f64::INFINITY
        } else {
            let prev = front[i - 1].val_nmse.max(FLOOR).ln();
            let cur = front[i].val_nmse.max(FLOOR).ln();
            let dc = front[i]
                .complexity
                .saturating_sub(front[i - 1].complexity)
                .max(1) as f64;
            (prev - cur) / dc
        };
        if score > pick_score + 1e-12 {
            pick = i;
            pick_score = score;
        }
    }
    pick
}

/// Run discovery.
pub fn discover(input: &DiscoverInput<'_>) -> Result<DiscoverOutput, DiscoverError> {
    let p = input.train_x.len();
    let n = input.train_y.len();
    if p == 0 {
        return Err(DiscoverError::BadInput("no features".into()));
    }
    if n < 10 || input.val_y.is_empty() {
        return Err(DiscoverError::BadInput(
            "too few training or validation rows".into(),
        ));
    }
    if input.train_x.iter().any(|c| c.len() != n) || input.val_x.len() != p {
        return Err(DiscoverError::BadInput(
            "feature columns have inconsistent lengths".into(),
        ));
    }
    let cfg = input.config;
    let y = input.train_y;
    let enet_rows = 2_000;
    let mut pool = Pool {
        input,
        cands: Vec::new(),
        seen: BTreeSet::new(),
        evaluated: 0,
    };
    let mut strategies = Vec::new();

    pool.add(Expr::constant(stats::mean(y)), Strategy::Constant);
    strategies.push(StrategyRun {
        strategy: Strategy::Constant,
        ran: true,
        skipped_reason: None,
        candidates: 1,
    });

    let terms = basis::library(input.train_x, input.val_x, y, cfg.max_basis);
    let k = sparse_linear(&mut pool, &terms, y, enet_rows, cfg.max_terms);
    strategies.push(StrategyRun {
        strategy: Strategy::SparseLinear,
        ran: true,
        skipped_reason: None,
        candidates: k,
    });

    for (strategy, result) in [
        (
            Strategy::LogLog,
            log_log(&mut pool, y, enet_rows, cfg.max_terms),
        ),
        (
            Strategy::LogTarget,
            log_target(&mut pool, &terms, y, enet_rows, cfg.max_terms),
        ),
    ] {
        strategies.push(match result {
            Ok(k) => StrategyRun {
                strategy,
                ran: true,
                skipped_reason: None,
                candidates: k,
            },
            Err(why) => StrategyRun {
                strategy,
                ran: false,
                skipped_reason: Some(why),
                candidates: 0,
            },
        });
    }

    let front = pareto_front(&pool.cands, cfg.max_front);
    let selected = match cfg.select {
        Some(i) if i < front.len() => i,
        _ => select(&front),
    };
    Ok(DiscoverOutput {
        front,
        selected,
        strategies,
        evaluated: pool.evaluated,
    })
}

#[cfg(test)]
mod tests;
