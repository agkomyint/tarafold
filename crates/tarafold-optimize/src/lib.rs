//! Target-seeking optimization (R8, design §5 Stage 7).
//!
//! Finds controllable inputs for which the fitted equation predicts a value close to the goal,
//! preferring small changes from the baseline and points inside the training data domain.
//! One controllable is solved exactly (all roots); several use differential evolution plus
//! multi-start Newton projection onto the goal surface, then diversity selection.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tarafold_core::{Rng, rng::stream};
use tarafold_expr::Expr;
use tarafold_validate::{Domain, DomainCheck};

/// The optimization problem over a full feature vector.
pub struct Problem<'a> {
    /// Selected equation over feature indices.
    pub expr: &'a Expr,
    /// Indices of controllable features.
    pub controllables: Vec<usize>,
    /// `[lo, hi]` per controllable.
    pub bounds: Vec<[f64; 2]>,
    /// Full baseline feature vector (non-controllables stay here).
    pub baseline: Vec<f64>,
    /// Per-feature scale (training std) for standardized distances.
    pub scale: Vec<f64>,
    /// Goal value.
    pub goal: f64,
    /// Target scale (training std of y).
    pub y_scale: f64,
    /// Absolute success tolerance.
    pub tolerance: f64,
    /// Applicability domain.
    pub domain: &'a Domain,
    /// Candidates to return.
    pub n_candidates: usize,
    /// Evaluation budget for multi-dimensional search.
    pub max_evals: usize,
    /// Prediction half-width for intervals.
    pub halfwidth: f64,
    /// Seed.
    pub seed: u64,
}

/// One proposed input combination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OptCandidate {
    /// Full feature vector.
    pub inputs: Vec<f64>,
    /// Predicted target value.
    pub predicted: f64,
    /// Prediction interval `[lo, hi]`.
    pub interval: [f64; 2],
    /// `predicted − goal`.
    pub error: f64,
    /// |error| ≤ tolerance.
    pub within_tolerance: bool,
    /// Domain check.
    pub domain: DomainCheck,
    /// Change from baseline per controllable.
    pub change: Vec<f64>,
    /// Percent change from baseline per controllable (`null` when baseline is 0).
    pub change_pct: Vec<Option<f64>>,
    /// ∂ŷ/∂x per controllable.
    pub sensitivity: Vec<f64>,
    /// Elasticity (∂ŷ/∂x · x/ŷ) per controllable.
    pub elasticity: Vec<f64>,
}

/// Optimization result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OptimizeOutput {
    /// Ranked, diverse candidates.
    pub candidates: Vec<OptCandidate>,
    /// At least one candidate is within tolerance.
    pub reached: bool,
    /// Closest predicted value found.
    pub closest_value: f64,
    /// Controllables at a bound at the closest point when the goal was not reached.
    pub binding: Vec<String>,
    /// Controllables the equation does not use (left at baseline).
    pub unused_controllables: Vec<usize>,
    /// Objective evaluations used.
    pub evaluations: usize,
}

struct Ctx<'a> {
    p: &'a Problem<'a>,
    /// Positions (in `p.controllables`) of controllables that the equation uses.
    active: Vec<usize>,
    evals: usize,
}

impl Ctx<'_> {
    fn full(&self, xc: &[f64]) -> Vec<f64> {
        let mut x = self.p.baseline.clone();
        for (k, &a) in self.active.iter().enumerate() {
            x[self.p.controllables[a]] = xc[k];
        }
        x
    }

    fn f(&mut self, xc: &[f64]) -> f64 {
        self.evals += 1;
        self.p.expr.eval_point(&self.full(xc))
    }

    fn bounds(&self, k: usize) -> [f64; 2] {
        self.p.bounds[self.active[k]]
    }

    fn scale(&self, k: usize) -> f64 {
        self.p.scale[self.p.controllables[self.active[k]]].max(1e-12)
    }

    /// Penalized objective J (design §5 Stage 7).
    fn objective(&mut self, xc: &[f64]) -> f64 {
        let y = self.f(xc);
        if !y.is_finite() {
            return f64::INFINITY;
        }
        let r = (y - self.p.goal) / self.p.y_scale.max(1e-300);
        let mut change = 0.0;
        for (k, &v) in xc.iter().enumerate() {
            let b = self.p.baseline[self.p.controllables[self.active[k]]];
            change += ((v - b) / self.scale(k)).powi(2);
        }
        let d = self.p.domain.distance(&self.full(xc));
        let out = (d - self.p.domain.threshold).max(0.0);
        r * r + 1e-3 * change + out * out
    }

    fn clamp(&self, xc: &mut [f64]) {
        for (k, v) in xc.iter_mut().enumerate() {
            let [lo, hi] = self.bounds(k);
            *v = v.clamp(lo, hi);
        }
    }

    /// Newton projection of `xc` onto `f = goal` in standardized coordinates, within bounds.
    fn project(&mut self, mut xc: Vec<f64>) -> Vec<f64> {
        let d = xc.len();
        for _ in 0..40 {
            let x = self.full(&xc);
            let r = self.p.expr.eval_point(&x) - self.p.goal;
            self.evals += 1;
            if !r.is_finite() || r.abs() <= 1e-12 * self.p.goal.abs().max(1.0) {
                break;
            }
            let g = self.p.expr.gradient(&x, x.len());
            // Gradient w.r.t. standardized coordinates z_k = x_k / s_k.
            let gz: Vec<f64> = (0..d)
                .map(|k| {
                    let [lo, hi] = self.bounds(k);
                    let gk = g[self.p.controllables[self.active[k]]] * self.scale(k);
                    // Freeze coordinates pinned at a bound in the direction of descent.
                    let step_dir = -r * gk;
                    let pinned = (xc[k] <= lo && step_dir < 0.0) || (xc[k] >= hi && step_dir > 0.0);
                    if pinned { 0.0 } else { gk }
                })
                .collect();
            let gg: f64 = gz.iter().map(|v| v * v).sum();
            if !gg.is_finite() || gg <= 0.0 {
                break;
            }
            let mut t = 1.0;
            let mut improved = false;
            for _ in 0..30 {
                let mut trial: Vec<f64> = (0..d)
                    .map(|k| xc[k] - t * r * gz[k] / gg * self.scale(k))
                    .collect();
                self.clamp(&mut trial);
                let rt = self.f(&trial) - self.p.goal;
                if rt.is_finite() && rt.abs() < r.abs() {
                    xc = trial;
                    improved = true;
                    break;
                }
                t *= 0.5;
            }
            if !improved {
                break;
            }
        }
        xc
    }
}

/// Exact search for one controllable: all roots of `f − goal` on a fine grid, refined by
/// bisection, plus the closest grid point when there is no root.
fn solve_1d(ctx: &mut Ctx<'_>) -> Vec<Vec<f64>> {
    let [lo, hi] = ctx.bounds(0);
    let m = 1000;
    let xs: Vec<f64> = (0..=m)
        .map(|i| lo + (hi - lo) * i as f64 / m as f64)
        .collect();
    let gs: Vec<f64> = xs.iter().map(|&x| ctx.f(&[x]) - ctx.p.goal).collect();
    let mut out = Vec::new();
    for i in 0..m {
        let (a, b, ga, gb) = (xs[i], xs[i + 1], gs[i], gs[i + 1]);
        if ga == 0.0 {
            out.push(vec![a]);
        } else if ga.is_finite() && gb.is_finite() && ga.signum() != gb.signum() {
            let (mut a, mut b, mut ga) = (a, b, ga);
            for _ in 0..100 {
                let mid = 0.5 * (a + b);
                let gm = ctx.f(&[mid]) - ctx.p.goal;
                if gm == 0.0 || (b - a) < 1e-15 * mid.abs().max(1.0) {
                    a = mid;
                    b = mid;
                    break;
                }
                if gm.signum() == ga.signum() {
                    a = mid;
                    ga = gm;
                } else {
                    b = mid;
                }
            }
            out.push(vec![0.5 * (a + b)]);
        }
    }
    if let Some(i) = (0..=m)
        .filter(|&i| gs[i].is_finite())
        .min_by(|&a, &b| gs[a].abs().total_cmp(&gs[b].abs()))
    {
        out.push(vec![xs[i]]);
    }
    out
}

/// Differential evolution (DE/rand/1/bin) on the penalized objective, then multi-start Newton
/// projection for diverse points on the goal surface.
fn solve_nd(ctx: &mut Ctx<'_>, rng: &mut Rng) -> Vec<Vec<f64>> {
    let d = ctx.active.len();
    let np = (15 * d).clamp(20, 150);
    let budget = ctx.p.max_evals.max(np * 10);
    let rand_point = |ctx: &Ctx<'_>, rng: &mut Rng| -> Vec<f64> {
        (0..d)
            .map(|k| {
                let [lo, hi] = ctx.bounds(k);
                rng.range(lo, hi)
            })
            .collect()
    };
    let mut pop: Vec<Vec<f64>> = (0..np).map(|_| rand_point(ctx, rng)).collect();
    let mut base: Vec<f64> = ctx
        .active
        .iter()
        .map(|&a| ctx.p.baseline[ctx.p.controllables[a]])
        .collect();
    ctx.clamp(&mut base);
    pop[0] = base.clone();
    let mut fit: Vec<f64> = pop.iter().map(|x| ctx.objective(x)).collect();
    let mut stall = 0;
    let mut best_prev = f64::INFINITY;
    while ctx.evals + np < budget / 2 && stall < 60 {
        for i in 0..np {
            let pick = |rng: &mut Rng, avoid: &[usize]| loop {
                let j = rng.below(np);
                if !avoid.contains(&j) {
                    break j;
                }
            };
            let a = pick(rng, &[i]);
            let b = pick(rng, &[i, a]);
            let c = pick(rng, &[i, a, b]);
            let jr = rng.below(d);
            let mut trial = pop[i].clone();
            for k in 0..d {
                if k == jr || rng.uniform() < 0.9 {
                    let [lo, hi] = ctx.bounds(k);
                    let mut v = pop[a][k] + 0.7 * (pop[b][k] - pop[c][k]);
                    if v < lo {
                        v = (2.0 * lo - v).min(hi);
                    } else if v > hi {
                        v = (2.0 * hi - v).max(lo);
                    }
                    trial[k] = v;
                }
            }
            let ft = ctx.objective(&trial);
            if ft <= fit[i] {
                pop[i] = trial;
                fit[i] = ft;
            }
        }
        let best = fit.iter().copied().fold(f64::INFINITY, f64::min);
        stall = if best < best_prev - 1e-12 * best_prev.abs().max(1e-300) {
            0
        } else {
            stall + 1
        };
        best_prev = best;
    }
    let mut order: Vec<usize> = (0..np).collect();
    order.sort_by(|&a, &b| fit[a].total_cmp(&fit[b]));
    let mut starts: Vec<Vec<f64>> = order.iter().take(5).map(|&i| pop[i].clone()).collect();
    starts.push(base);
    let n_random = (4 * ctx.p.n_candidates).max(10);
    for _ in 0..n_random {
        starts.push(rand_point(ctx, rng));
    }
    starts.into_iter().map(|s| ctx.project(s)).collect()
}

fn std_distance(ctx: &Ctx<'_>, a: &[f64], b: &[f64]) -> f64 {
    let d = a.len().max(1) as f64;
    ((0..a.len())
        .map(|k| ((a[k] - b[k]) / ctx.scale(k)).powi(2))
        .sum::<f64>()
        / d)
        .sqrt()
}

/// Solve the problem.
pub fn optimize(p: &Problem<'_>) -> OptimizeOutput {
    let used = p.expr.variables();
    let active: Vec<usize> = (0..p.controllables.len())
        .filter(|&k| u16::try_from(p.controllables[k]).is_ok_and(|v| used.contains(&v)))
        .collect();
    let unused: Vec<usize> = (0..p.controllables.len())
        .filter(|k| !active.contains(k))
        .map(|k| p.controllables[k])
        .collect();
    let mut ctx = Ctx {
        p,
        active,
        evals: 0,
    };
    let mut rng = Rng::derive(p.seed, &[stream::OPTIMIZE]);
    let raw = match ctx.active.len() {
        0 => vec![Vec::new()],
        1 => solve_1d(&mut ctx),
        _ => solve_nd(&mut ctx, &mut rng),
    };

    // Score every raw point.
    let mut scored: Vec<(Vec<f64>, OptCandidate)> = raw
        .into_iter()
        .filter_map(|xc| {
            let x = ctx.full(&xc);
            let y = p.expr.eval_point(&x);
            if !y.is_finite() {
                return None;
            }
            let err = y - p.goal;
            let grad = p.expr.gradient(&x, x.len());
            let cand = OptCandidate {
                change: p
                    .controllables
                    .iter()
                    .map(|&j| x[j] - p.baseline[j])
                    .collect(),
                change_pct: p
                    .controllables
                    .iter()
                    .map(|&j| {
                        (p.baseline[j] != 0.0)
                            .then(|| 100.0 * (x[j] - p.baseline[j]) / p.baseline[j].abs())
                    })
                    .collect(),
                sensitivity: p.controllables.iter().map(|&j| grad[j]).collect(),
                elasticity: p
                    .controllables
                    .iter()
                    .map(|&j| {
                        if y != 0.0 {
                            grad[j] * x[j] / y
                        } else {
                            f64::NAN
                        }
                    })
                    .collect(),
                domain: p.domain.check(&x),
                interval: [y - p.halfwidth, y + p.halfwidth],
                within_tolerance: err.abs() <= p.tolerance,
                error: err,
                predicted: y,
                inputs: x,
            };
            Some((xc, cand))
        })
        .collect();

    let change_size = |c: &OptCandidate| -> f64 {
        p.controllables
            .iter()
            .zip(&c.change)
            .map(|(&j, d)| (d / p.scale[j].max(1e-12)).powi(2))
            .sum::<f64>()
    };
    scored.sort_by(|(_, a), (_, b)| {
        b.within_tolerance
            .cmp(&a.within_tolerance)
            .then(b.domain.in_domain.cmp(&a.domain.in_domain))
            .then_with(|| {
                if a.within_tolerance && b.within_tolerance {
                    change_size(a).total_cmp(&change_size(b))
                } else {
                    a.error.abs().total_cmp(&b.error.abs())
                }
            })
    });

    // Diversity selection in standardized controllable space.
    let mut chosen: Vec<(Vec<f64>, OptCandidate)> = Vec::new();
    for (xc, c) in scored.iter() {
        if chosen.len() >= p.n_candidates {
            break;
        }
        if chosen
            .iter()
            .all(|(other, _)| xc.is_empty() || std_distance(&ctx, xc, other) > 0.25)
        {
            chosen.push((xc.clone(), c.clone()));
        }
    }
    let candidates: Vec<OptCandidate> = chosen.into_iter().map(|(_, c)| c).collect();
    let reached = candidates.iter().any(|c| c.within_tolerance);
    let closest = scored
        .iter()
        .min_by(|a, b| a.1.error.abs().total_cmp(&b.1.error.abs()))
        .map(|(_, c)| c.clone());
    let binding = match (&closest, reached) {
        (Some(c), false) => ctx
            .active
            .iter()
            .filter_map(|&a| {
                let j = p.controllables[a];
                let [lo, hi] = p.bounds[a];
                let eps = 1e-6 * (hi - lo).abs().max(1e-12);
                if (c.inputs[j] - lo).abs() <= eps {
                    Some(format!("{j}:lower"))
                } else if (c.inputs[j] - hi).abs() <= eps {
                    Some(format!("{j}:upper"))
                } else {
                    None
                }
            })
            .collect(),
        _ => Vec::new(),
    };
    OptimizeOutput {
        reached,
        closest_value: closest.map_or(f64::NAN, |c| c.predicted),
        binding,
        unused_controllables: unused,
        evaluations: ctx.evals,
        candidates,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tarafold_expr::parse;

    fn domain(names: &[String], ranges: &[[f64; 2]]) -> Domain {
        let mut r = Rng::derive(1, &[]);
        let x: Vec<Vec<f64>> = ranges
            .iter()
            .map(|[lo, hi]| (0..500).map(|_| r.range(*lo, *hi)).collect())
            .collect();
        Domain::fit(names, &x)
    }

    fn problem<'a>(expr: &'a Expr, dom: &'a Domain, ctrl: Vec<usize>, goal: f64) -> Problem<'a> {
        Problem {
            expr,
            bounds: ctrl.iter().map(|&j| [dom.min[j], dom.max[j]]).collect(),
            controllables: ctrl,
            baseline: dom.median.clone(),
            scale: dom.std.clone(),
            goal,
            y_scale: 1.0,
            tolerance: 0.01,
            domain: dom,
            n_candidates: 5,
            max_evals: 50_000,
            halfwidth: 0.0,
            seed: 3,
        }
    }

    #[test]
    fn one_dimensional_finds_all_roots() {
        let names: Vec<String> = vec!["x".into()];
        let e = parse("x^2", &names).unwrap();
        let dom = domain(&names, &[[-3.0, 3.0]]);
        let out = optimize(&problem(&e, &dom, vec![0], 4.0));
        assert!(out.reached);
        let roots: Vec<f64> = out
            .candidates
            .iter()
            .filter(|c| c.within_tolerance)
            .map(|c| c.inputs[0])
            .collect();
        assert!(
            roots.iter().any(|r| (r - 2.0).abs() < 1e-6)
                && roots.iter().any(|r| (r + 2.0).abs() < 1e-6)
        );
    }

    #[test]
    fn multi_dimensional_hits_goal_with_diverse_candidates() {
        let names: Vec<String> = vec!["orders".into(), "price".into()];
        let e = parse("orders * price", &names).unwrap();
        let dom = domain(&names, &[[100.0, 2000.0], [10.0, 50.0]]);
        let mut p = problem(&e, &dom, vec![0, 1], 30_000.0);
        p.y_scale = 15_000.0;
        p.tolerance = 0.01 * 100_000.0;
        let out = optimize(&p);
        assert!(out.reached);
        assert!(out.candidates.len() >= 3, "{}", out.candidates.len());
        for c in out.candidates.iter().filter(|c| c.within_tolerance) {
            assert!((c.inputs[0] * c.inputs[1] - 30_000.0).abs() <= p.tolerance);
            assert!(c.inputs[1] >= 10.0 - 1e-9 && c.inputs[1] <= 50.0 + 1e-9);
        }
    }

    #[test]
    fn unreachable_goal_reports_binding_bound() {
        let names: Vec<String> = vec!["x".into(), "y".into()];
        let e = parse("x + y", &names).unwrap();
        let dom = domain(&names, &[[0.0, 1.0], [0.0, 1.0]]);
        let out = optimize(&problem(&e, &dom, vec![0, 1], 10.0));
        assert!(!out.reached);
        // Bounds are the sampled min/max (≈ 0.001..0.999), so the best reachable value is ≈ 2.
        let max_reach = dom.max[0] + dom.max[1];
        assert!(
            (out.closest_value - max_reach).abs() < 1e-9,
            "{} vs {max_reach}",
            out.closest_value
        );
        assert_eq!(
            out.binding,
            vec!["0:upper".to_string(), "1:upper".to_string()]
        );
    }

    #[test]
    fn deterministic() {
        let names: Vec<String> = vec!["a".into(), "b".into()];
        let e = parse("a * b + a", &names).unwrap();
        let dom = domain(&names, &[[1.0, 5.0], [1.0, 5.0]]);
        let a = optimize(&problem(&e, &dom, vec![0, 1], 10.0));
        let b = optimize(&problem(&e, &dom, vec![0, 1], 10.0));
        assert_eq!(a, b);
    }
}
