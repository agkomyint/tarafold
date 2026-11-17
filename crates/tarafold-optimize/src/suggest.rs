//! Next-experiment suggestions (R19, design §5 Stage 7b).
//!
//! An ensemble per target combines bootstrap refits of the selected equation (constant
//! uncertainty) with competing equations from the Pareto front (structure uncertainty). Three
//! scores use it:
//! - explore: ensemble variance, i.e. where the model is least certain (its structure part is
//!   reported as "equation discrimination": where competing equations disagree);
//! - exploit: probability that all hard goals hold, times the probability of improving the
//!   best observed objective value when there are objectives;
//! - balanced: half the batch exploit, half explore.
//!
//! A batch is built greedily: each pick maximizes the score times a local penalty
//! `1 − exp(−d²/2r²)` (r = 0.15 in standardized units) around earlier picks. Existing rows
//! are excluded only within 0.02 (near-duplicates).

use tarafold_core::{Rng, rng::stream, stats};
use tarafold_expr::Expr;
use tarafold_validate::Domain;

use crate::multi::{Goal, Model};
use crate::spec::{GoalKind, Kind, Linear};

/// Suggestion mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Reduce uncertainty.
    Explore,
    /// Reach goals or improve objectives.
    Exploit,
    /// Half and half.
    Balanced,
}

/// A competing equation for one target, with its weight relative to the selected one.
#[derive(Debug, Clone)]
pub struct Alternative {
    /// Equation over the shared feature vector.
    pub expr: Expr,
    /// Weight relative to the selected equation (selected = 1).
    pub weight: f64,
}

/// The suggestion problem.
pub struct SuggestProblem<'a> {
    /// Target models (point equation + bootstrap members).
    pub models: &'a [Model],
    /// Competing equations per model.
    pub alternatives: &'a [Vec<Alternative>],
    /// Goals (may be empty: then explore only).
    pub goals: &'a [Goal],
    /// Controllable feature indices.
    pub controllables: Vec<usize>,
    /// Search bounds per controllable (already widened by `--suggest-expand`).
    pub bounds: Vec<[f64; 2]>,
    /// Observed range per controllable, for the in-domain label.
    pub observed: Vec<[f64; 2]>,
    /// Kind per controllable.
    pub kinds: Vec<Kind>,
    /// Linear constraints.
    pub constraints: &'a [Linear],
    /// Baseline full feature vector (non-controllables stay here).
    pub baseline: Vec<f64>,
    /// Per-feature scale.
    pub scale: Vec<f64>,
    /// Data domain.
    pub domain: &'a Domain,
    /// Existing rows as full feature vectors (no suggestion may duplicate one).
    pub existing: &'a [Vec<f64>],
    /// Observed target values per model for the existing rows (for improvement probability).
    pub observed_targets: &'a [Vec<f64>],
    /// Batch size.
    pub n: usize,
    /// Mode.
    pub mode: Mode,
    /// Evaluation budget per pick.
    pub evals_per_pick: usize,
    /// Seed.
    pub seed: u64,
}

/// Why a suggestion was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// High overall uncertainty.
    Uncertainty,
    /// Competing equations disagree most here.
    EquationDiscrimination,
    /// Likely to meet the goals.
    GoalLikelihood,
    /// Likely to improve the best observed objective.
    Improvement,
}

impl Reason {
    /// Machine-readable code.
    pub fn code(self) -> &'static str {
        match self {
            Self::Uncertainty => "uncertainty",
            Self::EquationDiscrimination => "equation_discrimination",
            Self::GoalLikelihood => "goal_likelihood",
            Self::Improvement => "improvement",
        }
    }
}

/// One suggested experiment.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    /// Full feature vector.
    pub inputs: Vec<f64>,
    /// Exploit or explore pick.
    pub exploit: bool,
    /// Reason.
    pub reason: Reason,
    /// Acquisition value at the pick.
    pub acquisition: f64,
    /// Ensemble mean per model.
    pub predictions: Vec<f64>,
    /// 90% ensemble interval per model.
    pub intervals: Vec<[f64; 2]>,
    /// Probability that all hard goals hold (`NaN` without hard goals).
    pub joint_probability: f64,
    /// Inside the observed ranges and data domain.
    pub in_domain: bool,
}

struct Sample {
    /// Predictions per model.
    pred: Vec<f64>,
    weight: f64,
    /// True for samples from alternative structures.
    alt: bool,
}

fn violation(kind: GoalKind, y: f64, tol: f64) -> f64 {
    match kind {
        GoalKind::Exact(v) => ((y - v).abs() - tol).max(0.0),
        GoalKind::AtLeast(v) => (v - y).max(0.0),
        GoalKind::AtMost(v) => (y - v).max(0.0),
        GoalKind::Range(a, b) => (a - y).max(0.0) + (y - b).max(0.0),
        _ => 0.0,
    }
}

struct Ctx<'a> {
    p: &'a SuggestProblem<'a>,
    bounds: Vec<[f64; 2]>,
    best_observed: f64,
    has_hard: bool,
    has_obj: bool,
}

impl Ctx<'_> {
    fn full(&self, z: &[f64]) -> Vec<f64> {
        let mut x = self.p.baseline.clone();
        for (k, &j) in self.p.controllables.iter().enumerate() {
            let [lo, hi] = self.bounds[k];
            x[j] = self
                .p
                .kinds
                .get(k)
                .copied()
                .unwrap_or(Kind::Continuous)
                .snap(z[k], lo, hi);
        }
        x
    }

    /// Ensemble samples at x: paired bootstrap members of the selected structures, then one
    /// sample per alternative rank (models without that many alternatives use their point).
    fn samples(&self, x: &[f64]) -> Vec<Sample> {
        let m = self.p.models;
        let nb = m.iter().map(|md| md.members.len()).min().unwrap_or(0);
        let alt_w: Vec<Vec<f64>> = self
            .p
            .alternatives
            .iter()
            .map(|a| a.iter().map(|v| v.weight).collect())
            .collect();
        let max_alt = self.p.alternatives.iter().map(Vec::len).max().unwrap_or(0);
        let total_alt: f64 = (0..max_alt)
            .map(|k| {
                alt_w
                    .iter()
                    .filter_map(|w| w.get(k))
                    .copied()
                    .fold(0.0, f64::max)
            })
            .sum();
        let w_sel = 1.0 / (1.0 + total_alt);
        let mut out = Vec::new();
        if nb == 0 {
            out.push(Sample {
                pred: m.iter().map(|md| md.expr.eval_point(x)).collect(),
                weight: w_sel,
                alt: false,
            });
        } else {
            for b in 0..nb {
                out.push(Sample {
                    pred: m.iter().map(|md| md.members[b].eval_point(x)).collect(),
                    weight: w_sel / nb as f64,
                    alt: false,
                });
            }
        }
        for k in 0..max_alt {
            let w = alt_w
                .iter()
                .filter_map(|w| w.get(k))
                .copied()
                .fold(0.0, f64::max)
                * w_sel;
            if w <= 0.0 {
                continue;
            }
            let pred = m
                .iter()
                .enumerate()
                .map(|(i, md)| {
                    self.p.alternatives[i]
                        .get(k)
                        .map_or_else(|| md.expr.eval_point(x), |a| a.expr.eval_point(x))
                })
                .collect();
            out.push(Sample {
                pred,
                weight: w,
                alt: true,
            });
        }
        out
    }

    fn scalar_objective(&self, pred: &[f64]) -> f64 {
        self.p
            .goals
            .iter()
            .filter(|g| g.spec.kind.is_objective())
            .map(|g| {
                let s = self.p.models[g.model].scale.max(1e-300);
                let v = pred[g.model] / s * g.spec.weight;
                if g.spec.kind == GoalKind::Maximize {
                    v
                } else {
                    -v
                }
            })
            .sum()
    }

    fn hard_ok(&self, pred: &[f64]) -> bool {
        self.p.goals.iter().all(|g| {
            !(g.spec.hard || !g.spec.kind.is_objective())
                || g.spec.kind.is_objective()
                || violation(g.spec.kind, pred[g.model], self.p.models[g.model].tolerance) <= 1e-9
        })
    }

    /// (explore, discrimination, exploit, joint probability) at x.
    fn scores(&self, x: &[f64]) -> (f64, f64, f64, f64) {
        let s = self.samples(x);
        if s.iter().any(|v| v.pred.iter().any(|p| !p.is_finite())) {
            return (f64::NEG_INFINITY, 0.0, f64::NEG_INFINITY, f64::NAN);
        }
        let wsum: f64 = s.iter().map(|v| v.weight).sum::<f64>().max(1e-300);
        let mut explore = 0.0;
        let mut disc = 0.0;
        for (i, md) in self.p.models.iter().enumerate() {
            let mean = s.iter().map(|v| v.weight * v.pred[i]).sum::<f64>() / wsum;
            let var = s
                .iter()
                .map(|v| v.weight * (v.pred[i] - mean).powi(2))
                .sum::<f64>()
                / wsum;
            explore += var / md.scale.powi(2).max(1e-300);
            // Structure part: spread of the alternative structures around the selected mean.
            let sel_mean = {
                let sw: f64 = s
                    .iter()
                    .filter(|v| !v.alt)
                    .map(|v| v.weight)
                    .sum::<f64>()
                    .max(1e-300);
                s.iter()
                    .filter(|v| !v.alt)
                    .map(|v| v.weight * v.pred[i])
                    .sum::<f64>()
                    / sw
            };
            let alt: f64 = s
                .iter()
                .filter(|v| v.alt)
                .map(|v| v.weight * (v.pred[i] - sel_mean).powi(2))
                .sum::<f64>()
                / wsum;
            disc += alt / md.scale.powi(2).max(1e-300);
        }
        let joint = if self.has_hard {
            s.iter()
                .filter(|v| self.hard_ok(&v.pred))
                .map(|v| v.weight)
                .sum::<f64>()
                / wsum
        } else {
            f64::NAN
        };
        let improve = if self.has_obj {
            s.iter()
                .filter(|v| self.scalar_objective(&v.pred) > self.best_observed)
                .map(|v| v.weight)
                .sum::<f64>()
                / wsum
        } else {
            1.0
        };
        // Probabilities are flat (zero) far from the goal; a small mean-violation term and a
        // small objective term give the search a direction there without changing the ranking
        // where goals are likely met.
        let mean_viol: f64 = s
            .iter()
            .map(|v| {
                v.weight
                    * self
                        .p
                        .goals
                        .iter()
                        .filter(|g| !g.spec.kind.is_objective())
                        .map(|g| {
                            let md = &self.p.models[g.model];
                            violation(g.spec.kind, v.pred[g.model], md.tolerance)
                                / md.scale.max(1e-300)
                        })
                        .sum::<f64>()
            })
            .sum::<f64>()
            / wsum;
        let mean_obj = if self.has_obj {
            s.iter()
                .map(|v| v.weight * self.scalar_objective(&v.pred))
                .sum::<f64>()
                / wsum
        } else {
            0.0
        };
        let exploit = joint.max(0.0).max(if self.has_hard { 0.0 } else { 1.0 }) * improve
            - 0.1 * mean_viol
            + 0.01 * mean_obj;
        (explore, disc, exploit, joint)
    }

    fn std_dist(&self, a: &[f64], b: &[f64]) -> f64 {
        let d = self.p.controllables.len().max(1) as f64;
        (self
            .p
            .controllables
            .iter()
            .map(|&j| ((a[j] - b[j]) / self.p.scale[j].max(1e-12)).powi(2))
            .sum::<f64>()
            / d)
            .sqrt()
    }
}

/// Build a batch of suggestions.
pub fn suggest(p: &SuggestProblem<'_>) -> Vec<Suggestion> {
    let bounds: Vec<[f64; 2]> = p
        .bounds
        .iter()
        .enumerate()
        .map(|(k, b)| {
            p.kinds
                .get(k)
                .copied()
                .unwrap_or(Kind::Continuous)
                .round_bounds(*b)
                .unwrap_or(*b)
        })
        .collect();
    let has_hard = p.goals.iter().any(|g| !g.spec.kind.is_objective());
    let has_obj = p.goals.iter().any(|g| g.spec.kind.is_objective());
    let mut ctx = Ctx {
        p,
        bounds,
        best_observed: f64::NEG_INFINITY,
        has_hard,
        has_obj,
    };
    if has_obj {
        let rows = p.observed_targets.first().map_or(0, Vec::len);
        // Best observed objective among rows that already meet the hard goals.
        ctx.best_observed = (0..rows)
            .filter_map(|r| {
                let pred: Vec<f64> = p.observed_targets.iter().map(|v| v[r]).collect();
                ctx.hard_ok(&pred).then(|| ctx.scalar_objective(&pred))
            })
            .fold(f64::NEG_INFINITY, f64::max);
    }
    let n_exploit = match (p.mode, has_hard || has_obj) {
        (_, false) | (Mode::Explore, _) => 0,
        (Mode::Exploit, true) => p.n,
        (Mode::Balanced, true) => p.n.div_ceil(2),
    };
    let r = 0.15;
    let mut picks: Vec<Suggestion> = Vec::new();
    let d = ctx.bounds.len();
    for slot in 0..p.n {
        let exploit = slot < n_exploit;
        let mut rng = Rng::derive(p.seed, &[stream::OPTIMIZE, 70, slot as u64]);
        let picked: Vec<Vec<f64>> = picks.iter().map(|s| s.inputs.clone()).collect();
        let objective = |ctx: &Ctx<'_>, z: &[f64]| -> f64 {
            let x = ctx.full(z);
            if p.constraints
                .iter()
                .any(|c| c.violation(&x, &p.scale) > 0.0)
            {
                return f64::NEG_INFINITY;
            }
            let (explore, _disc, exploit_s, _) = ctx.scores(&x);
            // Picks stay at least r apart (R19.4); existing rows are only excluded within a
            // small radius (a near-duplicate experiment), so dense data does not block the interior.
            let r_existing = 0.02;
            let mut pen = 1.0;
            let mut min_d = f64::INFINITY;
            for q in &picked {
                let dd = ctx.std_dist(&x, q);
                if dd < r {
                    return f64::NEG_INFINITY;
                }
                min_d = min_d.min(dd);
                pen *= 1.0 - (-dd * dd / (2.0 * r * r)).exp();
            }
            for q in p.existing {
                let dd = ctx.std_dist(&x, q);
                if dd < r_existing {
                    return f64::NEG_INFINITY;
                }
                min_d = min_d.min(dd);
            }
            // A small space-filling term breaks ties where the ensemble agrees everywhere.
            let fill = if min_d.is_finite() {
                1e-4 * min_d * min_d
            } else {
                0.0
            };
            let base = if exploit {
                exploit_s + 1e-3 * explore.max(0.0).sqrt() + fill
            } else {
                explore + fill
            };
            if !base.is_finite() {
                return f64::NEG_INFINITY;
            }
            base * pen
        };
        // Differential evolution maximizing the penalized acquisition.
        let np = (10 * d).clamp(16, 60);
        let mut pop: Vec<Vec<f64>> = (0..np)
            .map(|_| {
                ctx.bounds
                    .iter()
                    .map(|[lo, hi]| rng.range(*lo, *hi))
                    .collect()
            })
            .collect();
        let mut fit: Vec<f64> = pop.iter().map(|z| objective(&ctx, z)).collect();
        let mut evals = np;
        while evals + np <= p.evals_per_pick.max(2 * np) {
            for i in 0..np {
                let (a, b, c) = (rng.below(np), rng.below(np), rng.below(np));
                let jr = rng.below(d.max(1));
                let mut z = pop[i].clone();
                for k in 0..d {
                    if k == jr || rng.uniform() < 0.9 {
                        let [lo, hi] = ctx.bounds[k];
                        z[k] = (pop[a][k] + 0.7 * (pop[b][k] - pop[c][k])).clamp(lo, hi);
                    }
                }
                let f = objective(&ctx, &z);
                evals += 1;
                if f >= fit[i] {
                    pop[i] = z;
                    fit[i] = f;
                }
            }
        }
        let Some(best) = (0..np)
            .filter(|&i| fit[i].is_finite())
            .max_by(|&a, &b| fit[a].total_cmp(&fit[b]).then(b.cmp(&a)))
        else {
            continue;
        };
        let x = ctx.full(&pop[best]);
        let (explore, disc, _, joint) = ctx.scores(&x);
        let samples = ctx.samples(&x);
        let wsum: f64 = samples.iter().map(|v| v.weight).sum::<f64>().max(1e-300);
        let predictions: Vec<f64> = (0..p.models.len())
            .map(|i| samples.iter().map(|v| v.weight * v.pred[i]).sum::<f64>() / wsum)
            .collect();
        let intervals: Vec<[f64; 2]> = (0..p.models.len())
            .map(|i| {
                let vals: Vec<f64> = samples.iter().map(|v| v.pred[i]).collect();
                [stats::quantile(&vals, 0.05), stats::quantile(&vals, 0.95)]
            })
            .collect();
        let in_ranges = p
            .controllables
            .iter()
            .zip(&p.observed)
            .all(|(&j, [lo, hi])| x[j] >= *lo - 1e-9 && x[j] <= *hi + 1e-9);
        let reason = if exploit {
            if has_obj {
                Reason::Improvement
            } else {
                Reason::GoalLikelihood
            }
        } else if disc > 0.5 * explore && disc > 0.0 {
            Reason::EquationDiscrimination
        } else {
            Reason::Uncertainty
        };
        picks.push(Suggestion {
            in_domain: in_ranges && p.domain.check(&x).in_domain,
            inputs: x,
            exploit,
            reason,
            acquisition: fit[best],
            predictions,
            intervals,
            joint_probability: joint,
        });
    }
    picks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::parse_goal;
    use tarafold_expr::parse;

    fn setup(names: &[String]) -> (Domain, Vec<Vec<f64>>) {
        let mut r = Rng::derive(2, &[]);
        let rows: Vec<Vec<f64>> = (0..60)
            .map(|_| vec![r.range(0.0, 1.0), r.range(0.0, 1.0)])
            .collect();
        let cols: Vec<Vec<f64>> = (0..2)
            .map(|j| rows.iter().map(|row| row[j]).collect())
            .collect();
        (Domain::fit(names, &cols), rows)
    }

    fn base_problem<'a>(
        models: &'a [Model],
        alts: &'a [Vec<Alternative>],
        goals: &'a [Goal],
        dom: &'a Domain,
        existing: &'a [Vec<f64>],
        obs: &'a [Vec<f64>],
        mode: Mode,
    ) -> SuggestProblem<'a> {
        SuggestProblem {
            models,
            alternatives: alts,
            goals,
            controllables: vec![0, 1],
            bounds: vec![[0.0, 1.0], [0.0, 1.0]],
            observed: vec![[0.0, 1.0], [0.0, 1.0]],
            kinds: vec![Kind::Continuous; 2],
            constraints: &[],
            baseline: vec![0.5, 0.5],
            scale: dom.std.clone(),
            domain: dom,
            existing,
            observed_targets: obs,
            n: 4,
            mode,
            evals_per_pick: 2000,
            seed: 1,
        }
    }

    #[test]
    fn explore_targets_equation_disagreement() {
        // Two candidate equations agree for x < 0.5 and disagree strongly near x = 1.
        let names: Vec<String> = vec!["x".into(), "y".into()];
        let (dom, rows) = setup(&names);
        let e = parse("x", &names).unwrap();
        let alt = parse("x + 4 * square(x - 0.5) * (x > 0.5)", &names)
            .unwrap_or_else(|_| parse("x + 4 * square(abs(x - 0.5) + x - 0.5)", &names).unwrap());
        let models = vec![Model {
            name: "t".into(),
            expr: e.clone(),
            members: vec![e.clone(); 3],
            scale: 0.3,
            tolerance: 0.01,
        }];
        let alts = vec![vec![Alternative {
            expr: alt,
            weight: 1.0,
        }]];
        let obs = vec![rows.iter().map(|r| r[0]).collect::<Vec<f64>>()];
        let p = base_problem(&models, &alts, &[], &dom, &rows, &obs, Mode::Explore);
        let s = suggest(&p);
        assert_eq!(s.len(), 4);
        assert!(s[0].inputs[0] > 0.8, "{:?}", s[0].inputs);
        assert_eq!(s[0].reason, Reason::EquationDiscrimination);
        // Diverse and never on top of existing rows.
        for (i, a) in s.iter().enumerate() {
            for b in &s[i + 1..] {
                assert!(
                    (a.inputs[0] - b.inputs[0]).abs() + (a.inputs[1] - b.inputs[1]).abs() > 1e-3
                );
            }
        }
    }

    #[test]
    fn exploit_seeks_goal_and_balanced_splits() {
        let names: Vec<String> = vec!["x".into(), "y".into()];
        let (dom, rows) = setup(&names);
        let e = parse("x + y", &names).unwrap();
        let members: Vec<Expr> = [0.9, 1.0, 1.1]
            .iter()
            .map(|k| parse(&format!("{k} * (x + y)"), &names).unwrap())
            .collect();
        let models = vec![Model {
            name: "t".into(),
            expr: e,
            members,
            scale: 0.4,
            tolerance: 0.02,
        }];
        let alts = vec![Vec::new()];
        let goals = vec![Goal {
            spec: parse_goal("t>=1.7", true).unwrap(),
            model: 0,
        }];
        let obs = vec![rows.iter().map(|r| r[0] + r[1]).collect::<Vec<f64>>()];
        let p = base_problem(&models, &alts, &goals, &dom, &rows, &obs, Mode::Balanced);
        let s = suggest(&p);
        assert_eq!(s.iter().filter(|x| x.exploit).count(), 2);
        let first = &s[0];
        assert_eq!(first.reason, Reason::GoalLikelihood);
        assert!(
            first.inputs[0] + first.inputs[1] > 1.7,
            "{:?}",
            first.inputs
        );
        assert!(first.joint_probability > 0.6);
    }

    #[test]
    fn deterministic() {
        let names: Vec<String> = vec!["x".into(), "y".into()];
        let (dom, rows) = setup(&names);
        let e = parse("x * y", &names).unwrap();
        let models = vec![Model {
            name: "t".into(),
            expr: e.clone(),
            members: vec![e; 2],
            scale: 0.2,
            tolerance: 0.01,
        }];
        let alts = vec![Vec::new()];
        let obs = vec![rows.iter().map(|r| r[0] * r[1]).collect::<Vec<f64>>()];
        let a = suggest(&base_problem(
            &models,
            &alts,
            &[],
            &dom,
            &rows,
            &obs,
            Mode::Explore,
        ));
        let b = suggest(&base_problem(
            &models,
            &alts,
            &[],
            &dom,
            &rows,
            &obs,
            Mode::Explore,
        ));
        // NaN joint probabilities make derived PartialEq fail, so compare debug output.
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
        // With no model uncertainty, picks still spread out (space filling).
        for (i, s) in a.iter().enumerate() {
            for t in &a[i + 1..] {
                let d = ((s.inputs[0] - t.inputs[0]).powi(2) + (s.inputs[1] - t.inputs[1]).powi(2))
                    .sqrt();
                assert!(d > 0.1, "{:?} {:?}", s.inputs, t.inputs);
            }
        }
    }
}
