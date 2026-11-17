//! Bootstrap uncertainty, structural equivalence, and residual diagnostics (R7.3–R7.5).

use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tarafold_core::{Rng, rng::stream, stats};
use tarafold_expr::Expr;

/// Least-squares refit of an expression's constants on (x, y) (e.g. Levenberg–Marquardt).
pub type Refit = dyn Fn(&Expr, &[Vec<f64>], &[f64]) -> Expr + Sync;

/// Confidence interval for one fitted constant.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConstantInterval {
    /// Point estimate.
    pub value: f64,
    /// 2.5th percentile over bootstrap refits.
    pub lo: f64,
    /// 97.5th percentile over bootstrap refits.
    pub hi: f64,
}

/// Bootstrap of the selected equation's constants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Bootstrap {
    /// Resamples that produced a finite refit.
    pub resamples: usize,
    /// Per-constant intervals, in postfix order.
    pub constants: Vec<ConstantInterval>,
    /// Refitted constants per resample (rows × constants), reused for prediction intervals
    /// and optimization uncertainty.
    pub ensemble: Vec<Vec<f64>>,
}

/// Bootstrap resample indices shared by every target in a run: resample `b` uses the same rows
/// for all targets, so joint uncertainty across targets stays consistent (design §5).
pub fn resample_indices(n: usize, b: usize, seed: u64) -> Vec<usize> {
    resample_m(n, n, b, seed)
}

/// `m` row indices drawn with replacement from `0..n` (an m-out-of-n bootstrap).
pub fn resample_m(n: usize, m: usize, b: usize, seed: u64) -> Vec<usize> {
    let mut rng = Rng::derive(seed, &[stream::BOOTSTRAP, b as u64]);
    (0..m).map(|_| rng.below(n)).collect()
}

/// Refit the constants of `expr` on `resamples` bootstrap resamples of `(x, y)`.
/// `refit(expr, x, y)` performs the least-squares refit (Levenberg–Marquardt).
pub fn bootstrap(
    expr: &Expr,
    x: &[Vec<f64>],
    y: &[f64],
    resamples: usize,
    seed: u64,
    refit: &Refit,
) -> Bootstrap {
    let point = expr.constants();
    let n = y.len();
    if point.is_empty() || n < 5 || resamples == 0 {
        return Bootstrap {
            resamples: 0,
            constants: point
                .iter()
                .map(|&v| ConstantInterval {
                    value: v,
                    lo: v,
                    hi: v,
                })
                .collect(),
            ensemble: Vec::new(),
        };
    }
    let ensemble: Vec<Vec<f64>> = (0..resamples)
        .into_par_iter()
        .filter_map(|b| {
            let idx = resample_indices(n, b, seed);
            let bx: Vec<Vec<f64>> = x
                .iter()
                .map(|c| idx.iter().map(|&i| c[i]).collect())
                .collect();
            let by: Vec<f64> = idx.iter().map(|&i| y[i]).collect();
            let c = refit(expr, &bx, &by).constants();
            (c.len() == point.len() && c.iter().all(|v| v.is_finite())).then_some(c)
        })
        .collect();
    let constants = (0..point.len())
        .map(|k| {
            let vals: Vec<f64> = ensemble.iter().map(|c| c[k]).collect();
            if vals.is_empty() {
                ConstantInterval {
                    value: point[k],
                    lo: point[k],
                    hi: point[k],
                }
            } else {
                ConstantInterval {
                    value: point[k],
                    lo: stats::quantile(&vals, 0.025),
                    hi: stats::quantile(&vals, 0.975),
                }
            }
        })
        .collect();
    Bootstrap {
        resamples: ensemble.len(),
        constants,
        ensemble,
    }
}

/// 90% prediction interval at one point: `ŷ ± 1.645·sqrt(Var_ensemble(ŷ) + σ²)`, where σ is the
/// residual standard deviation on validation data.
pub fn prediction_interval(
    expr: &Expr,
    ensemble: &[Vec<f64>],
    point: &[f64],
    sigma: f64,
) -> [f64; 2] {
    let y = expr.eval_point(point);
    let preds: Vec<f64> = ensemble
        .iter()
        .map(|c| expr.with_constants(c).eval_point(point))
        .filter(|v| v.is_finite())
        .collect();
    let var = if preds.len() >= 2 {
        stats::variance(&preds)
    } else {
        0.0
    };
    let h = 1.645 * (var + sigma * sigma).sqrt();
    [y - h, y + h]
}

/// Column-wise version of [`prediction_interval`]: half-widths for every row.
pub fn interval_halfwidths(
    expr: &Expr,
    ensemble: &[Vec<f64>],
    x: &[Vec<f64>],
    sigma: f64,
) -> Vec<f64> {
    let n = x.first().map_or(0, Vec::len);
    let refs: Vec<&[f64]> = x.iter().map(Vec::as_slice).collect();
    let members: Vec<Vec<f64>> = ensemble
        .iter()
        .filter_map(|c| expr.with_constants(c).eval_columns(&refs, n))
        .collect();
    (0..n)
        .map(|i| {
            let p: Vec<f64> = members
                .iter()
                .map(|m| m[i])
                .filter(|v| v.is_finite())
                .collect();
            let var = if p.len() >= 2 {
                stats::variance(&p)
            } else {
                0.0
            };
            1.645 * (var + sigma * sigma).sqrt()
        })
        .collect()
}

fn nrmse(a: &[f64], b: &[f64]) -> f64 {
    let sd = stats::std(b);
    let mse = a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f64>() / b.len().max(1) as f64;
    if sd > 0.0 {
        mse.sqrt() / sd
    } else if mse == 0.0 {
        0.0
    } else {
        f64::INFINITY
    }
}

/// Two equations are structurally equivalent on `x` when each one's form, with constants refit,
/// reproduces the other's predictions within 1% NRMSE.
pub fn equivalent(a: &Expr, b: &Expr, x: &[Vec<f64>], refit: &Refit) -> bool {
    let n = x.first().map_or(0, Vec::len);
    let refs: Vec<&[f64]> = x.iter().map(Vec::as_slice).collect();
    let (Some(pa), Some(pb)) = (a.eval_columns(&refs, n), b.eval_columns(&refs, n)) else {
        return false;
    };
    if pa.iter().chain(&pb).any(|v| !v.is_finite()) {
        return false;
    }
    let fits = |form: &Expr, target: &[f64]| {
        let r = refit(form, x, target);
        r.eval_columns(&refs, n)
            .is_some_and(|p| nrmse(&p, target) < 0.01)
    };
    (nrmse(&pa, &pb) < 0.01) || (fits(a, &pb) && fits(b, &pa))
}

/// Residual diagnostics on held-out rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Residuals {
    /// Spearman correlation of |residual| with the prediction (heteroscedasticity indicator).
    pub abs_residual_vs_prediction: f64,
    /// Pearson correlation of residual with the prediction (systematic misfit indicator).
    pub residual_vs_prediction: f64,
    /// |abs_residual_vs_prediction| > 0.3.
    pub heteroscedastic: bool,
    /// Largest residuals: original row number, actual, predicted.
    pub largest: Vec<ResidualRow>,
}

/// One residual.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResidualRow {
    /// Row number in the dataset (0-based, after dropping rows with missing values).
    pub row: usize,
    /// Observed value.
    pub actual: f64,
    /// Predicted value.
    pub predicted: f64,
}

fn ranks(x: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..x.len()).collect();
    idx.sort_by(|&a, &b| x[a].total_cmp(&x[b]));
    let mut r = vec![0.0; x.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && x[idx[j + 1]] == x[idx[i]] {
            j += 1;
        }
        for &k in &idx[i..=j] {
            r[k] = (i + j) as f64 / 2.0 + 1.0;
        }
        i = j + 1;
    }
    r
}

/// Diagnose residuals; `rows[i]` is the dataset row of prediction `i`.
pub fn residuals(pred: &[f64], y: &[f64], rows: &[u32]) -> Residuals {
    let res: Vec<f64> = pred.iter().zip(y).map(|(p, t)| t - p).collect();
    let abs: Vec<f64> = res.iter().map(|v| v.abs()).collect();
    let s = stats::pearson(&ranks(&abs), &ranks(pred));
    let mut order: Vec<usize> = (0..res.len()).collect();
    order.sort_by(|&a, &b| abs[b].total_cmp(&abs[a]).then(a.cmp(&b)));
    Residuals {
        abs_residual_vs_prediction: s,
        residual_vs_prediction: stats::pearson(&res, pred),
        heteroscedastic: s.abs() > 0.3,
        largest: order
            .iter()
            .take(20)
            .map(|&i| ResidualRow {
                row: rows.get(i).map_or(i, |&r| r as usize),
                actual: y[i],
                predicted: pred[i],
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tarafold_core::linalg;
    use tarafold_expr::parse;

    /// Linear least squares refit for `c0 + c1 * x` style expressions in tests.
    fn linear_refit(e: &Expr, x: &[Vec<f64>], y: &[f64]) -> Expr {
        // Gauss–Newton with exact gradients; fine for the small test expressions.
        let refs: Vec<&[f64]> = x.iter().map(Vec::as_slice).collect();
        let mut cur = e.clone();
        for _ in 0..30 {
            let p = cur.constants();
            let Some(pred) = cur.eval_columns(&refs, y.len()) else {
                return cur;
            };
            let r: Vec<f64> = y.iter().zip(&pred).map(|(a, b)| a - b).collect();
            let jac: Vec<Vec<f64>> = (0..p.len())
                .filter_map(|k| cur.eval_columns_const_grad(&refs, y.len(), k).map(|v| v.1))
                .collect();
            let Some(step) = linalg::lstsq(&jac, &r) else {
                return cur;
            };
            let next: Vec<f64> = p.iter().zip(&step).map(|(a, d)| a + d).collect();
            cur = cur.with_constants(&next);
            if step.iter().all(|d| d.abs() < 1e-12) {
                break;
            }
        }
        cur
    }

    fn noisy_line(n: usize, seed: u64) -> (Vec<Vec<f64>>, Vec<f64>) {
        let mut r = Rng::derive(seed, &[]);
        let x: Vec<f64> = (0..n).map(|_| r.range(0.0, 10.0)).collect();
        let y = x.iter().map(|v| 2.0 + 3.0 * v + r.normal()).collect();
        (vec![x], y)
    }

    #[test]
    fn bootstrap_intervals_cover_truth() {
        let names = vec!["x".to_string()];
        let (x, y) = noisy_line(400, 1);
        let e = linear_refit(&parse("1 + 1 * x", &names).unwrap(), &x, &y);
        let b = bootstrap(&e, &x, &y, 200, 7, &linear_refit);
        assert_eq!(b.resamples, 200);
        let [c0, c1] = [b.constants[0], b.constants[1]];
        assert!(c0.lo < 2.0 && 2.0 < c0.hi, "{c0:?}");
        assert!(c1.lo < 3.0 && 3.0 < c1.hi, "{c1:?}");
        assert!(c1.hi - c1.lo < 0.2);
        // Prediction interval includes noise, so it is wider than the ensemble spread alone.
        let [lo, hi] = prediction_interval(&e, &b.ensemble, &[5.0], 1.0);
        assert!(hi - lo > 3.0 && hi - lo < 4.0, "{lo} {hi}");
    }

    #[test]
    fn equivalence_is_structural() {
        let names = vec!["x".to_string()];
        let x = vec![
            (0..200)
                .map(|i| f64::from(i) / 20.0 + 0.1)
                .collect::<Vec<f64>>(),
        ];
        let a = parse("2 + 3 * x", &names).unwrap();
        let b = parse("2.1 + 2.95 * x", &names).unwrap();
        let c = parse("2 + 3 * square(x)", &names).unwrap();
        assert!(equivalent(&a, &b, &x, &linear_refit));
        assert!(!equivalent(&a, &c, &x, &linear_refit));
    }

    #[test]
    fn residual_diagnostics_flag_heteroscedasticity() {
        let pred: Vec<f64> = (1..=300).map(f64::from).collect();
        let y: Vec<f64> = pred
            .iter()
            .enumerate()
            .map(|(i, p)| p + if i % 2 == 0 { 0.05 } else { -0.05 } * p)
            .collect();
        let rows: Vec<u32> = (0..300).collect();
        let r = residuals(&pred, &y, &rows);
        assert!(r.heteroscedastic, "{}", r.abs_residual_vs_prediction);
        assert_eq!(r.largest.len(), 20);
        assert_eq!(r.largest[0].row, 299);
    }
}
