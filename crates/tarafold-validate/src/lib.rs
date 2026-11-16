//! Validation (R7): accuracy metrics, applicability domain, reliability grade, evidence level.
//!
//! Phase 1 computes metrics on all splits, the applicability domain, residual-quantile
//! prediction intervals, and a provisional grade. Bootstrap intervals and structural
//! stability arrive in Phase 5; until then the grade is capped at `medium` and says why.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tarafold_core::{DataSource, Evidence, linalg, stats};
use tarafold_expr::Expr;

/// Accuracy metrics on one split.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Metrics {
    /// Coefficient of determination.
    pub r2: f64,
    /// Root mean squared error.
    pub rmse: f64,
    /// Mean absolute error.
    pub mae: f64,
    /// RMSE / std(y).
    pub nrmse: f64,
    /// Rows.
    pub n: usize,
}

/// Metrics for predictions `pred` against truth `y`. Non-finite predictions give `NaN` metrics.
pub fn metrics(pred: &[f64], y: &[f64]) -> Metrics {
    let n = y.len().min(pred.len());
    if n == 0 || pred[..n].iter().any(|p| !p.is_finite()) {
        return Metrics { r2: f64::NAN, rmse: f64::NAN, mae: f64::NAN, nrmse: f64::NAN, n };
    }
    let sse: f64 = (0..n).map(|i| (pred[i] - y[i]).powi(2)).sum();
    let sae: f64 = (0..n).map(|i| (pred[i] - y[i]).abs()).sum();
    let var = stats::variance(&y[..n]);
    let mse = sse / n as f64;
    let rmse = mse.sqrt();
    let sd = var.sqrt();
    Metrics {
        r2: if var > 0.0 { 1.0 - mse / var } else { f64::NAN },
        rmse,
        mae: sae / n as f64,
        nrmse: if sd > 0.0 { rmse / sd } else { f64::NAN },
        n,
    }
}

/// Evaluate an expression on column-major features and score it.
pub fn score(expr: &Expr, x: &[Vec<f64>], y: &[f64]) -> (Metrics, Vec<f64>) {
    let refs: Vec<&[f64]> = x.iter().map(Vec::as_slice).collect();
    let pred = expr.eval_columns(&refs, y.len()).unwrap_or_else(|| vec![f64::NAN; y.len()]);
    (metrics(&pred, y), pred)
}

/// Region of input space supported by training data (R7.8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Domain {
    /// Feature names.
    pub names: Vec<String>,
    /// Per-feature training minimum.
    pub min: Vec<f64>,
    /// Per-feature training maximum.
    pub max: Vec<f64>,
    /// Per-feature training mean.
    pub mean: Vec<f64>,
    /// Per-feature training std.
    pub std: Vec<f64>,
    /// Per-feature training median.
    pub median: Vec<f64>,
    /// 99th percentile of training Mahalanobis distances.
    pub threshold: f64,
    #[serde(skip)]
    #[schemars(skip)]
    chol: Vec<Vec<f64>>,
}

/// Where a point sits relative to the domain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DomainCheck {
    /// Every feature within its training range.
    pub in_ranges: bool,
    /// Mahalanobis distance to the training data.
    pub distance: f64,
    /// `in_ranges` and distance ≤ threshold.
    pub in_domain: bool,
}

impl Domain {
    /// Fit on training feature columns.
    pub fn fit(names: &[String], x: &[Vec<f64>]) -> Self {
        let p = x.len();
        let n = x.first().map_or(0, Vec::len);
        let mean: Vec<f64> = x.iter().map(|c| stats::mean(c)).collect();
        let std: Vec<f64> = x.iter().map(|c| stats::std(c)).collect();
        // Covariance of standardized features, ridge-regularized.
        let z: Vec<Vec<f64>> = (0..p)
            .map(|j| x[j].iter().map(|v| (v - mean[j]) / std[j].max(1e-300)).collect())
            .collect();
        let mut cov = vec![vec![0.0; p]; p];
        for i in 0..p {
            for j in i..p {
                let v = (0..n).map(|k| z[i][k] * z[j][k]).sum::<f64>() / n.max(1) as f64;
                cov[i][j] = v;
                cov[j][i] = v;
            }
            cov[i][i] += 1e-6;
        }
        let chol = linalg::cholesky(&cov).unwrap_or_else(|| {
            (0..p).map(|i| (0..p).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect()
        });
        let mut d = Self {
            names: names.to_vec(),
            min: x.iter().map(|c| c.iter().copied().fold(f64::INFINITY, f64::min)).collect(),
            max: x.iter().map(|c| c.iter().copied().fold(f64::NEG_INFINITY, f64::max)).collect(),
            median: x.iter().map(|c| stats::median(c)).collect(),
            mean,
            std,
            threshold: f64::INFINITY,
            chol,
        };
        let dists: Vec<f64> =
            (0..n).map(|k| d.distance(&(0..p).map(|j| x[j][k]).collect::<Vec<_>>())).collect();
        d.threshold = stats::quantile(&dists, 0.99);
        d
    }

    /// Mahalanobis distance of a point.
    pub fn distance(&self, point: &[f64]) -> f64 {
        let zs: Vec<f64> = point
            .iter()
            .enumerate()
            .map(|(j, v)| (v - self.mean[j]) / self.std[j].max(1e-300))
            .collect();
        let w = linalg::cholesky_solve(&self.chol, &zs);
        zs.iter().zip(&w).map(|(a, b)| a * b).sum::<f64>().max(0.0).sqrt()
    }

    /// Check a point.
    pub fn check(&self, point: &[f64]) -> DomainCheck {
        let tol = |j: usize| 1e-9 * (self.max[j] - self.min[j]).abs().max(1.0);
        let in_ranges = point
            .iter()
            .enumerate()
            .all(|(j, v)| *v >= self.min[j] - tol(j) && *v <= self.max[j] + tol(j));
        let distance = self.distance(point);
        DomainCheck { in_ranges, distance, in_domain: in_ranges && distance <= self.threshold }
    }
}

/// Reliability grade (R7.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Grade {
    /// Do not rely on this equation.
    Low,
    /// Useful with care.
    Medium,
    /// Passed every check.
    High,
}

/// One grading condition and whether it passed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Check {
    /// Machine-readable id.
    pub id: String,
    /// Human-readable condition.
    pub condition: String,
    /// `true` passed, `false` failed, `null` not computed yet.
    pub passed: Option<bool>,
}

/// Grade with reasons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Reliability {
    /// Grade.
    pub grade: Grade,
    /// Conditions considered.
    pub checks: Vec<Check>,
}

/// Inputs to grading that may not be available yet.
#[derive(Debug, Clone, Copy, Default)]
pub struct GradeInputs {
    /// Test R² of the selected equation.
    pub test_r2: f64,
    /// Best baseline test R² (Phase 4), if computed.
    pub best_baseline_r2: Option<f64>,
    /// Structural stability (Phase 5), if computed.
    pub stability: Option<f64>,
    /// Training rows.
    pub n_train: usize,
    /// Fitted constants in the equation.
    pub n_constants: usize,
    /// A leakage suspect is among the used variables.
    pub leakage: bool,
}

/// Apply the grading rule in design §5 (Stage 6). Unknown inputs count as not passed for
/// `high`, so the grade is capped at `medium` until they are computed.
pub fn grade(g: &GradeInputs) -> Reliability {
    let c = |id: &str, condition: String, passed: Option<bool>| Check { id: id.into(), condition, passed };
    let checks = vec![
        c("test_r2_high", "test R² ≥ 0.95".into(), Some(g.test_r2 >= 0.95)),
        c("test_r2_medium", "test R² ≥ 0.80".into(), Some(g.test_r2 >= 0.80)),
        c("stability_high", "structural stability ≥ 0.7".into(), g.stability.map(|s| s >= 0.7)),
        c("stability_medium", "structural stability ≥ 0.4".into(), g.stability.map(|s| s >= 0.4)),
        c(
            "baseline",
            "test R² within 0.02 of the best baseline model".into(),
            g.best_baseline_r2.map(|b| g.test_r2 >= b - 0.02),
        ),
        c(
            "sample_size",
            format!("training rows ≥ 20 × constants ({} ≥ {})", g.n_train, 20 * g.n_constants),
            Some(g.n_train >= 20 * g.n_constants),
        ),
        c("no_leakage", "no target-leakage suspect among used variables".into(), Some(!g.leakage)),
    ];
    let ok = |id: &str| checks.iter().any(|k| k.id == id && k.passed == Some(true));
    let not_failed = |id: &str| checks.iter().any(|k| k.id == id && k.passed != Some(false));
    let grade = if ["test_r2_high", "stability_high", "baseline", "sample_size", "no_leakage"]
        .iter()
        .all(|id| ok(id))
    {
        Grade::High
    } else if ok("test_r2_medium") && not_failed("stability_medium") {
        Grade::Medium
    } else {
        Grade::Low
    };
    Reliability { grade, checks }
}

/// Evidence level (R9.1, R9.4). `designed_controls` is true when every controllable is a
/// designed column of experimental or simulation data.
pub fn evidence(test_r2: f64, grade: Grade, source: DataSource, designed_controls: bool) -> Evidence {
    if grade >= Grade::Medium {
        if source != DataSource::Observational && designed_controls {
            Evidence::InterventionalFit
        } else {
            Evidence::FunctionalFit
        }
    } else if test_r2 >= 0.5 {
        Evidence::Predictive
    } else {
        Evidence::Association
    }
}

/// Symmetric prediction half-width from validation residuals: the `level` quantile of
/// |residual|. Used until bootstrap ensembles exist (Phase 5).
pub fn residual_halfwidth(val_pred: &[f64], val_y: &[f64], level: f64) -> f64 {
    let abs: Vec<f64> = val_pred.iter().zip(val_y).map(|(p, y)| (p - y).abs()).collect();
    stats::quantile(&abs, level)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_basic() {
        let m = metrics(&[1.0, 2.0, 3.0], &[1.0, 2.0, 4.0]);
        assert!((m.rmse - (1.0f64 / 3.0).sqrt()).abs() < 1e-12);
        assert!((m.mae - 1.0 / 3.0).abs() < 1e-12);
        assert!((m.r2 - (1.0 - (1.0 / 3.0) / stats::variance(&[1.0, 2.0, 4.0]))).abs() < 1e-12);
        assert!(metrics(&[f64::NAN], &[1.0]).r2.is_nan());
    }

    #[test]
    fn domain_flags_far_points() {
        let x = vec![(0..200).map(f64::from).collect::<Vec<_>>(), (0..200).map(|i| f64::from(i) * 2.0 + f64::from(i % 7)).collect()];
        let d = Domain::fit(&["a".into(), "b".into()], &x);
        assert!(d.check(&[100.0, 203.0]).in_domain);
        assert!(!d.check(&[500.0, 10.0]).in_ranges);
        // Inside both ranges but against the correlation structure.
        let c = d.check(&[190.0, 5.0]);
        assert!(c.in_ranges && !c.in_domain);
    }

    #[test]
    fn grade_capped_without_stability() {
        let g = grade(&GradeInputs { test_r2: 0.999, n_train: 600, n_constants: 2, ..Default::default() });
        assert_eq!(g.grade, Grade::Medium);
        let g = grade(&GradeInputs {
            test_r2: 0.999,
            stability: Some(0.9),
            best_baseline_r2: Some(0.99),
            n_train: 600,
            n_constants: 2,
            leakage: false,
        });
        assert_eq!(g.grade, Grade::High);
        assert_eq!(grade(&GradeInputs { test_r2: 0.3, ..Default::default() }).grade, Grade::Low);
    }

    #[test]
    fn evidence_levels() {
        assert_eq!(evidence(0.99, Grade::Medium, DataSource::Observational, true), Evidence::FunctionalFit);
        assert_eq!(evidence(0.99, Grade::Medium, DataSource::Simulation, true), Evidence::InterventionalFit);
        assert_eq!(evidence(0.99, Grade::Medium, DataSource::Experimental, false), Evidence::FunctionalFit);
        assert_eq!(evidence(0.6, Grade::Low, DataSource::Simulation, true), Evidence::Predictive);
    }
}
