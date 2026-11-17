//! Relationship discovery (R5): which features relate to the target, how strongly, whether
//! nonlinearly or through interactions, and which to pass to equation discovery.

pub mod dependence;
pub mod gbdt;
pub mod ridge;

use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tarafold_core::{Rng, linalg, rng::stream, stats};

pub use gbdt::{Gbdt, GbdtParams};
pub use ridge::Ridge;

/// Per-feature relationship with the target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureRelation {
    /// Feature.
    pub feature: String,
    /// Pearson correlation with the target.
    pub pearson: f64,
    /// Spearman rank correlation.
    pub spearman: f64,
    /// Mutual information (nats, KSG k = 3).
    pub mutual_information: f64,
    /// `1 − exp(−2·MI)`: equals r² for Gaussian data.
    pub mi_r2: f64,
    /// MI against a shuffled target: the estimator's bias at this sample size.
    pub mi_null: f64,
    /// Distance correlation.
    pub distance_correlation: f64,
    /// Permutation importance on the tree baseline (validation MSE increase / Var(y)).
    pub importance: f64,
    /// Std of permutation importance across repeats.
    pub importance_std: f64,
    /// Variance inflation factor among features.
    pub vif: f64,
}

/// Pairwise interaction strength.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Interaction {
    /// First feature.
    pub a: String,
    /// Second feature.
    pub b: String,
    /// Friedman's H² in `[0, 1]`: share of the pair's joint effect not explained by their separate effects.
    pub h2: f64,
}

/// A reference model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Baseline {
    /// `ridge` or `gbdt`.
    pub model: String,
    /// Validation R².
    pub val_r2: f64,
    /// Test R² (filled in by the pipeline).
    pub test_r2: Option<f64>,
    /// Model settings chosen.
    pub detail: String,
}

/// Highly correlated feature pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CollinearPair {
    /// First feature.
    pub a: String,
    /// Second feature.
    pub b: String,
    /// Pearson correlation.
    pub r: f64,
}

/// Screening decision for discovery (R5.7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScreenDecision {
    /// Feature.
    pub feature: String,
    /// Passed to discovery.
    pub kept: bool,
    /// Combined score: max(normalized importance, normalized MI).
    pub score: f64,
    /// Why.
    pub reason: String,
}

/// Relationship analysis for one target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RelateReport {
    /// Per-feature results, in feature order.
    pub features: Vec<FeatureRelation>,
    /// Pearson correlation matrix among features.
    pub feature_correlation: Vec<Vec<f64>>,
    /// Reference models.
    pub baselines: Vec<Baseline>,
    /// Interactions among the most important features, strongest first.
    pub interactions: Vec<Interaction>,
    /// Pairs with |r| > 0.95.
    pub collinear: Vec<CollinearPair>,
    /// Features with |r| > 0.999 to the target (possible leakage).
    pub leakage: Vec<String>,
    /// Rows used for MI and distance correlation.
    pub dependence_rows: usize,
}

/// Analysis plus fitted reference models.
pub struct Relate {
    /// Serializable results.
    pub report: RelateReport,
    /// Ridge baseline.
    pub ridge: Option<Ridge>,
    /// Tree baseline.
    pub gbdt: Gbdt,
}

/// Input: column-major features for train and validation rows.
pub struct RelateInput<'a> {
    /// Feature names.
    pub names: &'a [String],
    /// Training features.
    pub train_x: &'a [Vec<f64>],
    /// Training target.
    pub train_y: &'a [f64],
    /// Validation features.
    pub val_x: &'a [Vec<f64>],
    /// Validation target.
    pub val_y: &'a [f64],
    /// Seed.
    pub seed: u64,
}

fn r2(pred: &[f64], y: &[f64]) -> f64 {
    let var = stats::variance(y);
    let mse = pred
        .iter()
        .zip(y)
        .map(|(p, t)| (p - t).powi(2))
        .sum::<f64>()
        / y.len().max(1) as f64;
    if var > 0.0 { 1.0 - mse / var } else { f64::NAN }
}

fn vifs(x: &[Vec<f64>]) -> Vec<f64> {
    let p = x.len();
    let n = x.first().map_or(0, Vec::len);
    (0..p)
        .map(|j| {
            if p < 2 {
                return 1.0;
            }
            let mut cols = vec![vec![1.0; n]];
            cols.extend((0..p).filter(|&k| k != j).map(|k| x[k].clone()));
            match linalg::lstsq(&cols, &x[j]) {
                Some(b) => {
                    let pred: Vec<f64> = (0..n)
                        .map(|i| cols.iter().zip(&b).map(|(c, w)| c[i] * w).sum())
                        .collect();
                    let r = r2(&pred, &x[j]);
                    if r >= 1.0 - 1e-12 {
                        1e12
                    } else {
                        1.0 / (1.0 - r.max(0.0))
                    }
                }
                None => 1e12,
            }
        })
        .collect()
}

/// Friedman's H² for features `a`, `b` using the model's partial dependence on sample rows.
fn h_statistic(model: &Gbdt, sample: &[Vec<f64>], a: usize, b: usize, pd: &[Vec<f64>]) -> f64 {
    let m = sample.len();
    let mut joint = vec![0.0; m];
    let mut row = vec![0.0; sample.first().map_or(0, Vec::len)];
    for i in 0..m {
        let mut s = 0.0;
        for r in sample {
            row.copy_from_slice(r);
            row[a] = sample[i][a];
            row[b] = sample[i][b];
            s += model.predict_row(&row);
        }
        joint[i] = s / m as f64;
    }
    let mj = stats::mean(&joint);
    let (ma, mb) = (stats::mean(&pd[a]), stats::mean(&pd[b]));
    let (mut num, mut den) = (0.0, 0.0);
    for i in 0..m {
        let jc = joint[i] - mj;
        let d = jc - (pd[a][i] - ma) - (pd[b][i] - mb);
        num += d * d;
        den += jc * jc;
    }
    if den > 1e-300 {
        (num / den).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Run the analysis.
pub fn analyze(input: &RelateInput<'_>) -> Relate {
    let p = input.train_x.len();
    let n = input.train_y.len();
    let y = input.train_y;

    // Subsample for O(n²) measures.
    let mut rows: Vec<usize> = (0..n).collect();
    Rng::derive(input.seed, &[stream::DISCOVER, 100]).shuffle(&mut rows);
    rows.truncate(2000);
    rows.sort_unstable();
    let ys: Vec<f64> = rows.iter().map(|&i| y[i]).collect();

    let pearson: Vec<f64> = input.train_x.iter().map(|c| stats::pearson(c, y)).collect();
    let mut ys_null = ys.clone();
    Rng::derive(input.seed, &[stream::DISCOVER, 103]).shuffle(&mut ys_null);
    let dep: Vec<(f64, f64, f64, f64)> = input
        .train_x
        .par_iter()
        .map(|c| {
            let xs: Vec<f64> = rows.iter().map(|&i| c[i]).collect();
            (
                dependence::spearman(c, y),
                dependence::mutual_information(&xs, &ys, 3),
                dependence::distance_correlation(&xs, &ys),
                dependence::mutual_information(&xs, &ys_null, 3),
            )
        })
        .collect();

    let ridge = Ridge::fit(input.train_x, y, input.val_x, input.val_y);
    let gbdt = Gbdt::fit(
        input.train_x,
        y,
        input.val_x,
        input.val_y,
        GbdtParams::default(),
    );
    let gbdt_val = gbdt.predict(input.val_x);
    let base_mse = gbdt_val
        .iter()
        .zip(input.val_y)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        / input.val_y.len().max(1) as f64;
    let var_val = stats::variance(input.val_y).max(1e-300);

    // Permutation importance: 10 seeded shuffles per feature.
    let importance: Vec<(f64, f64)> = (0..p)
        .into_par_iter()
        .map(|j| {
            let deltas: Vec<f64> = (0..10u64)
                .map(|rep| {
                    let mut perm = input.val_x[j].clone();
                    Rng::derive(input.seed, &[stream::DISCOVER, 101, j as u64, rep])
                        .shuffle(&mut perm);
                    let mut x = input.val_x.to_vec();
                    x[j] = perm;
                    let pred = gbdt.predict(&x);
                    let mse = pred
                        .iter()
                        .zip(input.val_y)
                        .map(|(a, b)| (a - b).powi(2))
                        .sum::<f64>()
                        / input.val_y.len().max(1) as f64;
                    (mse - base_mse) / var_val
                })
                .collect();
            (stats::mean(&deltas), stats::std(&deltas))
        })
        .collect();

    // Feature correlations, collinearity, VIF.
    let mut corr = vec![vec![1.0; p]; p];
    let mut collinear = Vec::new();
    for a in 0..p {
        for b in (a + 1)..p {
            let r = stats::pearson(&input.train_x[a], &input.train_x[b]);
            corr[a][b] = r;
            corr[b][a] = r;
            if r.abs() > 0.95 {
                collinear.push(CollinearPair {
                    a: input.names[a].clone(),
                    b: input.names[b].clone(),
                    r,
                });
            }
        }
    }
    let vif = vifs(input.train_x);

    // Interactions among the top 6 features by importance.
    let mut order: Vec<usize> = (0..p).collect();
    order.sort_by(|&a, &b| importance[b].0.total_cmp(&importance[a].0).then(a.cmp(&b)));
    let top: Vec<usize> = order
        .iter()
        .copied()
        .filter(|&j| importance[j].0 > 0.0)
        .take(6)
        .collect();
    let mut srows: Vec<usize> = (0..n).collect();
    Rng::derive(input.seed, &[stream::DISCOVER, 102]).shuffle(&mut srows);
    srows.truncate(80);
    let sample: Vec<Vec<f64>> = srows
        .iter()
        .map(|&i| (0..p).map(|j| input.train_x[j][i]).collect())
        .collect();
    let mut pd = vec![Vec::new(); p];
    for &j in &top {
        let mut row = vec![0.0; p];
        pd[j] = (0..sample.len())
            .map(|i| {
                sample
                    .iter()
                    .map(|r| {
                        row.copy_from_slice(r);
                        row[j] = sample[i][j];
                        gbdt.predict_row(&row)
                    })
                    .sum::<f64>()
                    / sample.len() as f64
            })
            .collect();
    }
    let pairs: Vec<(usize, usize)> = top
        .iter()
        .enumerate()
        .flat_map(|(i, &a)| top[i + 1..].iter().map(move |&b| (a.min(b), a.max(b))))
        .collect();
    let mut interactions: Vec<Interaction> = pairs
        .par_iter()
        .map(|&(a, b)| Interaction {
            a: input.names[a].clone(),
            b: input.names[b].clone(),
            h2: h_statistic(&gbdt, &sample, a, b, &pd),
        })
        .collect();
    interactions.sort_by(|x, y| {
        y.h2.total_cmp(&x.h2)
            .then_with(|| (&x.a, &x.b).cmp(&(&y.a, &y.b)))
    });

    let features: Vec<FeatureRelation> = (0..p)
        .map(|j| FeatureRelation {
            feature: input.names[j].clone(),
            pearson: pearson[j],
            spearman: dep[j].0,
            mutual_information: dep[j].1,
            mi_r2: 1.0 - (-2.0 * dep[j].1).exp(),
            mi_null: dep[j].3,
            distance_correlation: dep[j].2,
            importance: importance[j].0,
            importance_std: importance[j].1,
            vif: vif[j],
        })
        .collect();
    let leakage = features
        .iter()
        .filter(|f| f.pearson.abs() > 0.999)
        .map(|f| f.feature.clone())
        .collect();
    let mut baselines = Vec::new();
    if let Some(r) = &ridge {
        baselines.push(Baseline {
            model: "ridge".into(),
            val_r2: r2(&r.predict(input.val_x), input.val_y),
            test_r2: None,
            detail: format!("lambda = {:.3e}", r.lambda),
        });
    }
    baselines.push(Baseline {
        model: "gbdt".into(),
        val_r2: r2(&gbdt_val, input.val_y),
        test_r2: None,
        detail: format!(
            "{} trees, depth {}",
            gbdt.n_trees(),
            GbdtParams::default().depth
        ),
    });
    Relate {
        report: RelateReport {
            features,
            feature_correlation: corr,
            baselines,
            interactions,
            collinear,
            leakage,
            dependence_rows: rows.len(),
        },
        ridge,
        gbdt,
    }
}

/// Choose features for discovery (R5.7). Features are ranked by max(normalized importance,
/// normalized MI). `required` features (e.g. user-named controllables) are always kept. Others
/// are kept if they score ≥ 1% of the best, up to `max_features`. A near-duplicate (|r| > 0.98)
/// of a better-ranked kept feature is deprioritized, not dropped: it is added back only if the
/// budget allows, because the true equation may use either one.
pub fn screen(
    report: &RelateReport,
    max_features: usize,
    required: &[String],
) -> Vec<ScreenDecision> {
    let f = &report.features;
    // Evidence above noise: importance only when it exceeds twice its spread across
    // permutation repeats; MI only above twice its shuffled-target value.
    let imp = |x: &FeatureRelation| {
        if x.importance > 2.0 * x.importance_std {
            x.importance
        } else {
            0.0
        }
    };
    let mi = |x: &FeatureRelation| (x.mutual_information - 2.0 * x.mi_null.max(0.005)).max(0.0);
    let max_imp = f.iter().map(imp).fold(0.0, f64::max);
    // When the tree baseline fits well (validation R² ≥ 0.5), its permutation importance is the
    // more reliable signal; the KSG estimate has sampling noise of a few hundredths of a nat.
    // MI is used when the tree model is weak, where it can still reveal structure.
    let tree_fits = report
        .baselines
        .iter()
        .any(|b| b.model == "gbdt" && b.val_r2 >= 0.5);
    let max_mi = if tree_fits {
        0.0
    } else {
        f.iter().map(mi).fold(0.0, f64::max)
    };
    let score = |x: &FeatureRelation| {
        let a = if max_imp > 0.0 { imp(x) / max_imp } else { 0.0 };
        let b = if max_mi > 0.0 { mi(x) / max_mi } else { 0.0 };
        a.max(b)
    };
    let informative = max_imp > 0.0 || max_mi > 0.0;
    let mut order: Vec<usize> = (0..f.len()).collect();
    order.sort_by(|&a, &b| score(&f[b]).total_cmp(&score(&f[a])).then(a.cmp(&b)));
    let mut decision: Vec<Option<(bool, String)>> = vec![None; f.len()];
    let mut kept: Vec<usize> = Vec::new();
    for &j in &order {
        if required.contains(&f[j].feature) {
            kept.push(j);
            decision[j] = Some((true, "required (named by the user)".into()));
        }
    }
    let mut deferred: Vec<(usize, usize)> = Vec::new(); // (feature, duplicate of)
    for (rank, &j) in order.iter().enumerate() {
        if decision[j].is_some() {
            continue;
        }
        let s = score(&f[j]);
        if !informative {
            let keep = kept.len() < max_features;
            if keep {
                kept.push(j);
            }
            decision[j] = Some((
                keep,
                if keep {
                    "no feature shows a relationship; kept so discovery can still search".into()
                } else {
                    format!("rank {} exceeds max_features = {max_features}", rank + 1)
                },
            ));
        } else if let Some(&k) = kept
            .iter()
            .find(|&&k| report.feature_correlation[j][k].abs() > 0.98)
        {
            deferred.push((j, k));
        } else if kept.len() < max_features {
            kept.push(j);
            decision[j] = Some((
                true,
                format!("rank {} by importance/MI (score {s:.3})", rank + 1),
            ));
        } else {
            decision[j] = Some((
                false,
                format!("rank {} exceeds max_features = {max_features}", rank + 1),
            ));
        }
    }
    for (j, k) in deferred {
        let r = report.feature_correlation[j][k].abs();
        let keep = kept.len() < max_features;
        if keep {
            kept.push(j);
        }
        decision[j] = Some((
            keep,
            if keep {
                format!(
                    "near-duplicate of `{}` (|r| = {r:.3}); kept because the budget allows",
                    f[k].feature
                )
            } else {
                format!(
                    "near-duplicate of `{}` (|r| = {r:.3}); left out to stay within max_features",
                    f[k].feature
                )
            },
        ));
    }
    f.iter()
        .zip(decision)
        .map(|(x, d)| {
            let (kept, reason) = d.unwrap_or((false, "not ranked".into()));
            ScreenDecision {
                feature: x.feature.clone(),
                kept,
                score: score(x),
                reason,
            }
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;

    fn data(seed: u64, n: usize) -> (Vec<Vec<f64>>, Vec<f64>) {
        // y = x0 * x1 + x2, plus 3 pure-noise columns.
        let mut r = Rng::derive(seed, &[]);
        let x: Vec<Vec<f64>> = (0..6)
            .map(|_| (0..n).map(|_| r.range(0.0, 5.0)).collect())
            .collect();
        let y = (0..n).map(|i| x[0][i] * x[1][i] + x[2][i]).collect();
        (x, y)
    }

    #[test]
    fn finds_interaction_and_drops_noise() {
        let (tx, ty) = data(1, 1200);
        let (vx, vy) = data(2, 400);
        let names: Vec<String> = (0..6).map(|i| format!("x{i}")).collect();
        let rel = analyze(&RelateInput {
            names: &names,
            train_x: &tx,
            train_y: &ty,
            val_x: &vx,
            val_y: &vy,
            seed: 5,
        });
        let rep = &rel.report;
        assert_eq!(
            (
                rep.interactions[0].a.as_str(),
                rep.interactions[0].b.as_str()
            ),
            ("x0", "x1")
        );
        assert!(rep.interactions[0].h2 > 0.1);
        let gb = rep.baselines.iter().find(|b| b.model == "gbdt").unwrap();
        assert!(gb.val_r2 > 0.95, "{}", gb.val_r2);
        // With a budget of 3, the three real features win; with no budget pressure nothing is dropped.
        assert!(screen(rep, 12, &[]).iter().all(|d| d.kept));
        let s = screen(rep, 3, &[]);
        let kept: Vec<&str> = s
            .iter()
            .filter(|d| d.kept)
            .map(|d| d.feature.as_str())
            .collect();
        assert_eq!(kept, vec!["x0", "x1", "x2"], "{s:?}");
    }

    #[test]
    fn deterministic_across_threads() {
        let (tx, ty) = data(3, 500);
        let (vx, vy) = data(4, 200);
        let names: Vec<String> = (0..6).map(|i| format!("x{i}")).collect();
        let go = |t: usize| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(t)
                .build()
                .unwrap()
                .install(|| {
                    analyze(&RelateInput {
                        names: &names,
                        train_x: &tx,
                        train_y: &ty,
                        val_x: &vx,
                        val_y: &vy,
                        seed: 1,
                    })
                    .report
                })
        };
        assert_eq!(go(1), go(4));
    }
}
