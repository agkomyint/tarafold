//! Multi-goal optimization (R18, design §5 Stage 7 "multi-goal").
//!
//! Hard goals and linear constraints must hold; soft goals add penalties; maximize/minimize
//! goals are objectives. With zero or one objective the problem is solved as a single
//! penalized objective (differential evolution + pattern search). With 2–3 objectives NSGA-II
//! returns a trade-off set; with 4–10, NSGA-III-style reference-direction niching keeps the set
//! spread out. Constraints use Deb's feasibility rules (feasible beats infeasible; among
//! infeasible, smaller total violation wins). All randomness is seeded.

use tarafold_core::{Rng, rng::stream, stats};
use tarafold_expr::Expr;
use tarafold_validate::Domain;

use crate::spec::{GoalKind, GoalSpec, Kind, Linear};

/// A fitted target equation over the shared feature vector (already composed with any
/// equations it depends on).
pub struct Model {
    /// Target name.
    pub name: String,
    /// Point equation.
    pub expr: Expr,
    /// Bootstrap ensemble members (same structure, refit constants), for joint probabilities.
    pub members: Vec<Expr>,
    /// Training standard deviation of the target (scales violations and objectives).
    pub scale: f64,
    /// Absolute tolerance for exact goals.
    pub tolerance: f64,
}

/// A goal bound to a model.
pub struct Goal {
    /// Parsed goal.
    pub spec: GoalSpec,
    /// Index into the model list.
    pub model: usize,
}

/// How to pick the single preferred candidate among trade-offs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// Weighted Tchebycheff on normalized objectives (weights from `--prefer max:x:w`).
    Weighted,
    /// Objectives in the order given, each within 1% of its best before the next is used.
    Lexicographic,
}

/// The multi-goal problem.
pub struct MultiProblem<'a> {
    /// Target models.
    pub models: &'a [Model],
    /// Goals.
    pub goals: &'a [Goal],
    /// Indices of controllable features.
    pub controllables: Vec<usize>,
    /// `[lo, hi]` per controllable.
    pub bounds: Vec<[f64; 2]>,
    /// Kind per controllable.
    pub kinds: Vec<Kind>,
    /// Linear constraints over the full feature vector.
    pub constraints: &'a [Linear],
    /// Baseline full feature vector.
    pub baseline: Vec<f64>,
    /// Per-feature scale.
    pub scale: Vec<f64>,
    /// Data domain over the full feature vector.
    pub domain: &'a Domain,
    /// Candidates to return when there are 0–1 objectives (trade-off sets return up to 20).
    pub n_candidates: usize,
    /// Evaluation budget.
    pub max_evals: usize,
    /// Minimum joint probability of meeting all hard goals.
    pub robust: Option<f64>,
    /// Preferred-candidate rule.
    pub priority: Priority,
    /// Seed.
    pub seed: u64,
}

/// One candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiCandidate {
    /// Full feature vector.
    pub inputs: Vec<f64>,
    /// Prediction per model.
    pub predictions: Vec<f64>,
    /// Whether each goal is met (objectives count as met).
    pub goals_met: Vec<bool>,
    /// Share of ensemble members for which every hard goal holds (`NaN` without members).
    pub joint_probability: f64,
    /// Inside the data domain.
    pub in_domain: bool,
    /// Mahalanobis distance.
    pub domain_distance: f64,
    /// Labels such as `knee`, `best:max:yield`, `most_likely`, `preferred`.
    pub marks: Vec<String>,
}

/// Goals that cannot be met together.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    /// Goal indices in a minimal conflicting subset.
    pub goals: Vec<usize>,
    /// Best achievable predicted value of each conflicting goal's target with the others held.
    pub best_achievable: Vec<(usize, f64)>,
}

/// Result.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiOutput {
    /// Candidates (feasible only).
    pub candidates: Vec<MultiCandidate>,
    /// Method used.
    pub method: String,
    /// A feasible candidate exists.
    pub feasible: bool,
    /// Conflict analysis when infeasible.
    pub conflict: Option<Conflict>,
    /// Objective pairs whose values are rank-correlated above 0.9: `(goal a, goal b, rho)`.
    pub redundant: Vec<(usize, usize, f64)>,
    /// Evaluations used.
    pub evaluations: usize,
}

#[derive(Clone)]
struct Ev {
    z: Vec<f64>,
    x: Vec<f64>,
    pred: Vec<f64>,
    cv: f64,
    obj: Vec<f64>,
}

struct Solver<'a> {
    p: &'a MultiProblem<'a>,
    bounds: Vec<[f64; 2]>,
    hard: Vec<bool>,
    /// Goal indices treated as objectives in this solve.
    objectives: Vec<usize>,
    /// Weight of the small-change preference (0 while exploring from random starts).
    change_w: f64,
    evals: usize,
}

fn violation(kind: GoalKind, y: f64, scale: f64, tol: f64) -> f64 {
    let s = scale.max(1e-300);
    match kind {
        GoalKind::Exact(v) => ((y - v).abs() - tol).max(0.0) / s,
        GoalKind::AtLeast(v) => (v - y).max(0.0) / s,
        GoalKind::AtMost(v) => (y - v).max(0.0) / s,
        GoalKind::Range(a, b) => ((a - y).max(0.0) + (y - b).max(0.0)) / s,
        GoalKind::Maximize | GoalKind::Minimize => 0.0,
    }
}

impl Solver<'_> {
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

    fn eval(&mut self, z: &[f64]) -> Ev {
        self.evals += 1;
        let x = self.full(z);
        let pred: Vec<f64> = self
            .p
            .models
            .iter()
            .map(|m| m.expr.eval_point(&x))
            .collect();
        if pred.iter().any(|v| !v.is_finite()) {
            let n = self.objectives.len().max(1);
            return Ev {
                z: z.to_vec(),
                x,
                pred,
                cv: f64::INFINITY,
                obj: vec![f64::INFINITY; n],
            };
        }
        let mut cv: f64 = self
            .p
            .constraints
            .iter()
            .map(|c| c.violation(&x, &self.p.scale))
            .sum();
        let mut soft = 0.0;
        for (gi, g) in self.p.goals.iter().enumerate() {
            let m = &self.p.models[g.model];
            let v = violation(g.spec.kind, pred[g.model], m.scale, m.tolerance);
            if self.hard[gi] {
                cv += v;
            } else if !self.objectives.contains(&gi) {
                soft += g.spec.weight * v * v;
            }
        }
        let change: f64 = self
            .p
            .controllables
            .iter()
            .map(|&j| ((x[j] - self.p.baseline[j]) / self.p.scale[j].max(1e-12)).powi(2))
            .sum();
        let out = (self.p.domain.distance(&x) - self.p.domain.threshold).max(0.0);
        let pen = soft + self.change_w * change + out * out;
        let obj = if self.objectives.is_empty() {
            vec![pen]
        } else {
            self.objectives
                .iter()
                .map(|&gi| {
                    let g = &self.p.goals[gi];
                    let m = &self.p.models[g.model];
                    let y = pred[g.model] / m.scale.max(1e-300);
                    match g.spec.kind {
                        GoalKind::Maximize => -y + pen,
                        GoalKind::Minimize => y + pen,
                        // A non-objective goal used as an objective (conflict analysis): minimize its violation.
                        k => violation(k, pred[g.model], m.scale, m.tolerance) + pen,
                    }
                })
                .collect()
        };
        Ev {
            z: z.to_vec(),
            x,
            pred,
            cv: if cv <= 1e-9 { 0.0 } else { cv },
            obj,
        }
    }

    fn random_z(&self, rng: &mut Rng) -> Vec<f64> {
        self.bounds
            .iter()
            .map(|[lo, hi]| rng.range(*lo, *hi))
            .collect()
    }

    fn baseline_z(&self) -> Vec<f64> {
        self.p
            .controllables
            .iter()
            .zip(&self.bounds)
            .map(|(&j, [lo, hi])| self.p.baseline[j].clamp(*lo, *hi))
            .collect()
    }

    /// Deb-rule scalar for single-objective search.
    fn scalar(e: &Ev) -> f64 {
        if e.cv > 0.0 { 1e6 + e.cv } else { e.obj[0] }
    }

    fn pattern(&mut self, mut e: Ev) -> Ev {
        let d = self.bounds.len();
        let mut steps: Vec<f64> = (0..d)
            .map(|k| {
                let [lo, hi] = self.bounds[k];
                match self.p.kinds.get(k).and_then(|k| k.step()) {
                    Some(s) => s * ((hi - lo) / s / 8.0).round().max(1.0),
                    None => 0.1 * (hi - lo),
                }
            })
            .collect();
        for _ in 0..300 {
            let mut improved = false;
            for k in 0..d {
                for dir in [1.0, -1.0] {
                    let [lo, hi] = self.bounds[k];
                    let mut z = e.z.clone();
                    z[k] = (z[k] + dir * steps[k]).clamp(lo, hi);
                    let t = self.eval(&z);
                    if Self::scalar(&t) < Self::scalar(&e) {
                        e = t;
                        improved = true;
                    }
                }
            }
            if !improved {
                let mut any = false;
                for k in 0..d {
                    let [lo, hi] = self.bounds[k];
                    let step = self.p.kinds.get(k).and_then(|k| k.step());
                    let min = step.unwrap_or(1e-7 * (hi - lo).max(1e-300));
                    if steps[k] > min {
                        steps[k] = (steps[k] / 2.0).max(min);
                        if let Some(s) = step {
                            steps[k] = (steps[k] / s).round().max(1.0) * s;
                        }
                        any = true;
                    }
                }
                if !any {
                    break;
                }
            }
        }
        e
    }

    /// Single penalized objective: differential evolution, then pattern search from the best
    /// points and random starts.
    fn single(&mut self, rng: &mut Rng, budget: usize, n_out: usize) -> Vec<Ev> {
        let d = self.bounds.len();
        let np = (15 * d).clamp(20, 150);
        let mut pop: Vec<Ev> = (0..np)
            .map(|i| {
                let z = if i == 0 {
                    self.baseline_z()
                } else {
                    self.random_z(rng)
                };
                self.eval(&z)
            })
            .collect();
        let start = self.evals;
        let mut stall = 0;
        let mut best_prev = f64::INFINITY;
        while self.evals - start + np < budget / 2 && stall < 60 {
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
                let mut z = pop[i].z.clone();
                for k in 0..d {
                    if k == jr || rng.uniform() < 0.9 {
                        let [lo, hi] = self.bounds[k];
                        let mut v = pop[a].z[k] + 0.7 * (pop[b].z[k] - pop[c].z[k]);
                        if v < lo {
                            v = (2.0 * lo - v).min(hi);
                        } else if v > hi {
                            v = (2.0 * hi - v).max(lo);
                        }
                        z[k] = v;
                    }
                }
                let t = self.eval(&z);
                if Self::scalar(&t) <= Self::scalar(&pop[i]) {
                    pop[i] = t;
                }
            }
            let best = pop.iter().map(Self::scalar).fold(f64::INFINITY, f64::min);
            stall = if best < best_prev - 1e-12 * best_prev.abs().max(1e-300) {
                0
            } else {
                stall + 1
            };
            best_prev = best;
        }
        pop.sort_by(|a, b| Self::scalar(a).total_cmp(&Self::scalar(b)));
        let mut out: Vec<Ev> = pop
            .iter()
            .take(10)
            .cloned()
            .map(|e| self.pattern(e))
            .collect();
        // Random starts explore without the small-change preference, so solutions spread over
        // the goal surface instead of all collapsing to the point nearest the baseline.
        let w = self.change_w;
        self.change_w = 0.0;
        for _ in 0..(3 * n_out).max(10) {
            let z = self.random_z(rng);
            let e = self.eval(&z);
            out.push(self.pattern(e));
        }
        self.change_w = w;
        let rescored: Vec<Ev> = out.iter().map(|e| self.eval(&e.z)).collect();
        let mut out = rescored;
        out.extend(pop);
        out.sort_by(|a, b| Self::scalar(a).total_cmp(&Self::scalar(b)));
        out
    }
}

/// Constrained dominance (Deb).
fn dominates(a: &Ev, b: &Ev) -> bool {
    if a.cv > 0.0 || b.cv > 0.0 {
        return a.cv < b.cv;
    }
    a.obj.iter().zip(&b.obj).all(|(x, y)| x <= y) && a.obj.iter().zip(&b.obj).any(|(x, y)| x < y)
}

/// Fast non-dominated sort: fronts of indices.
fn fronts(pop: &[Ev]) -> Vec<Vec<usize>> {
    let n = pop.len();
    let mut dominated_by = vec![0usize; n];
    let mut dominates_list: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in (i + 1)..n {
            if dominates(&pop[i], &pop[j]) {
                dominates_list[i].push(j);
                dominated_by[j] += 1;
            } else if dominates(&pop[j], &pop[i]) {
                dominates_list[j].push(i);
                dominated_by[i] += 1;
            }
        }
    }
    let mut out = Vec::new();
    let mut cur: Vec<usize> = (0..n).filter(|&i| dominated_by[i] == 0).collect();
    while !cur.is_empty() {
        let mut next = Vec::new();
        for &i in &cur {
            for &j in &dominates_list[i] {
                dominated_by[j] -= 1;
                if dominated_by[j] == 0 {
                    next.push(j);
                }
            }
        }
        next.sort_unstable();
        out.push(cur);
        cur = next;
    }
    out
}

fn crowding(pop: &[Ev], idx: &[usize]) -> Vec<f64> {
    let m = pop.first().map_or(0, |e| e.obj.len());
    let mut d = vec![0.0; idx.len()];
    for o in 0..m {
        let mut order: Vec<usize> = (0..idx.len()).collect();
        order.sort_by(|&a, &b| pop[idx[a]].obj[o].total_cmp(&pop[idx[b]].obj[o]));
        let (lo, hi) = (
            pop[idx[order[0]]].obj[o],
            pop[idx[*order.last().unwrap_or(&0)]].obj[o],
        );
        d[order[0]] = f64::INFINITY;
        if let Some(&l) = order.last() {
            d[l] = f64::INFINITY;
        }
        let range = (hi - lo).max(1e-300);
        for w in 1..order.len().saturating_sub(1) {
            d[order[w]] += (pop[idx[order[w + 1]]].obj[o] - pop[idx[order[w - 1]]].obj[o]) / range;
        }
    }
    d
}

/// Das–Dennis reference directions with `h` divisions in `m` objectives.
fn reference_directions(m: usize, h: usize) -> Vec<Vec<f64>> {
    fn rec(m: usize, left: usize, cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if cur.len() == m - 1 {
            cur.push(left);
            out.push(cur.clone());
            cur.pop();
            return;
        }
        for k in 0..=left {
            cur.push(k);
            rec(m, left - k, cur, out);
            cur.pop();
        }
    }
    let mut out = Vec::new();
    rec(m, h, &mut Vec::new(), &mut out);
    out.into_iter()
        .map(|v| v.into_iter().map(|k| k as f64 / h as f64).collect())
        .collect()
}

fn binom(n: usize, k: usize) -> usize {
    (0..k).fold(1usize, |acc, i| acc.saturating_mul(n - i) / (i + 1))
}

/// NSGA-III niching: choose `need` members of `last` given already-selected `chosen`.
fn niche_select(
    pop: &[Ev],
    chosen: &[usize],
    last: &[usize],
    need: usize,
    refs: &[Vec<f64>],
    rng: &mut Rng,
) -> Vec<usize> {
    let m = refs.first().map_or(0, Vec::len);
    let all: Vec<usize> = chosen.iter().chain(last).copied().collect();
    let ideal: Vec<f64> = (0..m)
        .map(|o| {
            all.iter()
                .map(|&i| pop[i].obj[o])
                .fold(f64::INFINITY, f64::min)
        })
        .collect();
    let span: Vec<f64> = (0..m)
        .map(|o| {
            (all.iter()
                .map(|&i| pop[i].obj[o])
                .fold(f64::NEG_INFINITY, f64::max)
                - ideal[o])
                .max(1e-12)
        })
        .collect();
    let assoc = |i: usize| -> (usize, f64) {
        let f: Vec<f64> = (0..m)
            .map(|o| (pop[i].obj[o] - ideal[o]) / span[o])
            .collect();
        let mut best = (0, f64::INFINITY);
        for (r, w) in refs.iter().enumerate() {
            let ww: f64 = w.iter().map(|v| v * v).sum::<f64>().max(1e-300);
            let t = f.iter().zip(w).map(|(a, b)| a * b).sum::<f64>() / ww;
            let d2: f64 = f.iter().zip(w).map(|(a, b)| (a - t * b).powi(2)).sum();
            if d2 < best.1 {
                best = (r, d2);
            }
        }
        best
    };
    let mut count = vec![0usize; refs.len()];
    for &i in chosen {
        count[assoc(i).0] += 1;
    }
    let mut pool: Vec<(usize, usize, f64)> = last
        .iter()
        .map(|&i| {
            let (r, d) = assoc(i);
            (i, r, d)
        })
        .collect();
    let mut out = Vec::new();
    let mut excluded = vec![false; refs.len()];
    while out.len() < need && !pool.is_empty() {
        let Some(minc) = (0..refs.len())
            .filter(|&r| !excluded[r])
            .map(|r| count[r])
            .min()
        else {
            break;
        };
        let cands: Vec<usize> = (0..refs.len())
            .filter(|&r| !excluded[r] && count[r] == minc)
            .collect();
        let r = cands[rng.below(cands.len())];
        let members: Vec<usize> = (0..pool.len()).filter(|&k| pool[k].1 == r).collect();
        if members.is_empty() {
            excluded[r] = true;
            continue;
        }
        let k = if count[r] == 0 {
            *members
                .iter()
                .min_by(|&&a, &&b| pool[a].2.total_cmp(&pool[b].2))
                .unwrap_or(&members[0])
        } else {
            members[rng.below(members.len())]
        };
        out.push(pool[k].0);
        pool.remove(k);
        count[r] += 1;
    }
    out
}

fn sbx(a: f64, b: f64, rng: &mut Rng) -> (f64, f64) {
    let eta = 15.0;
    let u = rng.uniform();
    let beta = if u <= 0.5 {
        (2.0 * u).powf(1.0 / (eta + 1.0))
    } else {
        (1.0 / (2.0 * (1.0 - u))).powf(1.0 / (eta + 1.0))
    };
    (
        0.5 * ((1.0 + beta) * a + (1.0 - beta) * b),
        0.5 * ((1.0 - beta) * a + (1.0 + beta) * b),
    )
}

fn poly_mut(v: f64, rng: &mut Rng) -> f64 {
    let eta = 20.0;
    let u = rng.uniform();
    let d = if u < 0.5 {
        (2.0 * u).powf(1.0 / (eta + 1.0)) - 1.0
    } else {
        1.0 - (2.0 * (1.0 - u)).powf(1.0 / (eta + 1.0))
    };
    (v + d).clamp(0.0, 1.0)
}

impl Solver<'_> {
    fn nsga(&mut self, rng: &mut Rng, budget: usize) -> Vec<Ev> {
        let m = self.objectives.len();
        let d = self.bounds.len();
        let n = ((100 * m).clamp(100, 400) / 2) * 2;
        let refs = if m >= 4 {
            let mut h = 1;
            while binom(h + 1 + m - 1, m - 1) <= n {
                h += 1;
            }
            reference_directions(m, h)
        } else {
            Vec::new()
        };
        let to_z = |u: &[f64], b: &[[f64; 2]]| -> Vec<f64> {
            u.iter()
                .zip(b)
                .map(|(v, [lo, hi])| lo + v * (hi - lo))
                .collect()
        };
        let to_u = |z: &[f64], b: &[[f64; 2]]| -> Vec<f64> {
            z.iter()
                .zip(b)
                .map(|(v, [lo, hi])| if hi > lo { (v - lo) / (hi - lo) } else { 0.0 })
                .collect()
        };
        let mut pop: Vec<Ev> = (0..n)
            .map(|i| {
                let z = if i == 0 {
                    self.baseline_z()
                } else {
                    self.random_z(rng)
                };
                self.eval(&z)
            })
            .collect();
        let start = self.evals;
        while self.evals - start + n <= budget {
            let fr = fronts(&pop);
            let mut rank = vec![0usize; pop.len()];
            let mut crowd = vec![0.0; pop.len()];
            for (r, f) in fr.iter().enumerate() {
                let c = crowding(&pop, f);
                for (k, &i) in f.iter().enumerate() {
                    rank[i] = r;
                    crowd[i] = c[k];
                }
            }
            let tour = |rng: &mut Rng| {
                let (a, b) = (rng.below(pop.len()), rng.below(pop.len()));
                if rank[a] < rank[b] || (rank[a] == rank[b] && crowd[a] >= crowd[b]) {
                    a
                } else {
                    b
                }
            };
            let mut kids: Vec<Ev> = Vec::with_capacity(n);
            while kids.len() < n {
                let (pa, pb) = (tour(rng), tour(rng));
                let (mut ua, mut ub) = (
                    to_u(&pop[pa].z, &self.bounds),
                    to_u(&pop[pb].z, &self.bounds),
                );
                if rng.uniform() < 0.9 {
                    for k in 0..d {
                        if rng.uniform() < 0.5 {
                            let (x, y) = sbx(ua[k], ub[k], rng);
                            ua[k] = x.clamp(0.0, 1.0);
                            ub[k] = y.clamp(0.0, 1.0);
                        }
                    }
                }
                for u in [&mut ua, &mut ub] {
                    for v in u.iter_mut() {
                        if rng.uniform() < 1.0 / d as f64 {
                            *v = poly_mut(*v, rng);
                        }
                    }
                }
                let za = to_z(&ua, &self.bounds);
                let zb = to_z(&ub, &self.bounds);
                kids.push(self.eval(&za));
                kids.push(self.eval(&zb));
            }
            let mut all = pop;
            all.extend(kids);
            let fr = fronts(&all);
            let mut chosen: Vec<usize> = Vec::with_capacity(n);
            for f in &fr {
                if chosen.len() + f.len() <= n {
                    chosen.extend(f);
                    continue;
                }
                let need = n - chosen.len();
                if m >= 4 && f.iter().all(|&i| all[i].cv == 0.0) {
                    let pick = niche_select(&all, &chosen, f, need, &refs, rng);
                    chosen.extend(pick);
                } else {
                    let c = crowding(&all, f);
                    let mut order: Vec<usize> = (0..f.len()).collect();
                    order.sort_by(|&a, &b| c[b].total_cmp(&c[a]).then(a.cmp(&b)));
                    chosen.extend(order.into_iter().take(need).map(|k| f[k]));
                }
                break;
            }
            pop = chosen.into_iter().map(|i| all[i].clone()).collect();
        }
        pop
    }
}

fn spearman(a: &[f64], b: &[f64]) -> f64 {
    let rank = |v: &[f64]| -> Vec<f64> {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        idx.sort_by(|&x, &y| v[x].total_cmp(&v[y]));
        let mut r = vec![0.0; v.len()];
        for (k, &i) in idx.iter().enumerate() {
            r[i] = k as f64;
        }
        r
    };
    stats::pearson(&rank(a), &rank(b))
}

fn std_dist(a: &[f64], b: &[f64], bounds: &[[f64; 2]]) -> f64 {
    let d = a.len().max(1) as f64;
    (a.iter()
        .zip(b)
        .zip(bounds)
        .map(|((x, y), [lo, hi])| ((x - y) / (hi - lo).max(1e-12)).powi(2))
        .sum::<f64>()
        / d)
        .sqrt()
}

/// Solve a multi-goal problem.
pub fn optimize_goals(p: &MultiProblem<'_>) -> MultiOutput {
    let mut bounds = Vec::with_capacity(p.bounds.len());
    for (k, b) in p.bounds.iter().enumerate() {
        let kind = p.kinds.get(k).copied().unwrap_or(Kind::Continuous);
        bounds.push(kind.round_bounds(*b).unwrap_or(*b));
    }
    let objectives: Vec<usize> = (0..p.goals.len())
        .filter(|&g| p.goals[g].spec.kind.is_objective())
        .collect();
    let hard: Vec<bool> = p
        .goals
        .iter()
        .map(|g| g.spec.hard && !g.spec.kind.is_objective())
        .collect();
    let mut rng = Rng::derive(p.seed, &[stream::OPTIMIZE, 50]);
    let mut s = Solver {
        p,
        bounds: bounds.clone(),
        hard: hard.clone(),
        objectives: objectives.clone(),
        change_w: 1e-3,
        evals: 0,
    };
    let (method, pool) = if objectives.len() <= 1 {
        (
            "penalized differential evolution + pattern search".to_string(),
            s.single(&mut rng, p.max_evals, p.n_candidates),
        )
    } else {
        let name = if objectives.len() <= 3 {
            "NSGA-II"
        } else {
            "NSGA-III (reference directions)"
        };
        (name.to_string(), s.nsga(&mut rng, p.max_evals))
    };
    let evaluations_main = s.evals;

    // Feasible candidates.
    let mut feas: Vec<Ev> = pool
        .into_iter()
        .filter(|e| e.cv == 0.0 && e.obj.iter().all(|v| v.is_finite()))
        .collect();
    if objectives.len() >= 2 {
        let fr = fronts(&feas);
        let first: Vec<usize> = fr.first().cloned().unwrap_or_default();
        feas = first.into_iter().map(|i| feas[i].clone()).collect();
    }
    // Deduplicate near-identical inputs.
    let mut uniq: Vec<Ev> = Vec::new();
    for e in feas {
        let min_d = if objectives.len() >= 2 { 1e-6 } else { 0.25 };
        if uniq.iter().all(|u| std_dist(&u.z, &e.z, &bounds) > min_d) {
            uniq.push(e);
        }
    }

    let joint = |e: &Ev| -> f64 {
        let k = p.models.iter().map(|m| m.members.len()).min().unwrap_or(0);
        if k == 0 {
            return f64::NAN;
        }
        let ok = (0..k)
            .filter(|&b| {
                p.goals.iter().zip(&hard).all(|(g, &h)| {
                    if !h {
                        return true;
                    }
                    let m = &p.models[g.model];
                    let y = m.members[b].eval_point(&e.x);
                    y.is_finite() && violation(g.spec.kind, y, m.scale, m.tolerance) <= 1e-9
                })
            })
            .count();
        ok as f64 / k as f64
    };
    let mut cands: Vec<(Ev, f64)> = uniq
        .into_iter()
        .map(|e| {
            let j = joint(&e);
            (e, j)
        })
        .collect();
    if let Some(r) = p.robust {
        cands.retain(|(_, j)| j.is_nan() || *j >= r);
    }
    // Trim trade-off sets to 20, well spread: keep the best point per objective, then add the
    // point farthest (normalized objective space) from those kept, repeatedly. Crowding distance
    // spreads poorly beyond two objectives. Single-objective runs keep the n best.
    let full_front: Vec<Vec<f64>> = cands.iter().map(|(e, _)| e.pred.clone()).collect();
    if objectives.len() >= 2 && cands.len() > 20 {
        let m = cands[0].0.obj.len();
        let lo: Vec<f64> = (0..m)
            .map(|j| {
                cands
                    .iter()
                    .map(|(e, _)| e.obj[j])
                    .fold(f64::INFINITY, f64::min)
            })
            .collect();
        let hi: Vec<f64> = (0..m)
            .map(|j| {
                cands
                    .iter()
                    .map(|(e, _)| e.obj[j])
                    .fold(f64::NEG_INFINITY, f64::max)
            })
            .collect();
        let norm = |e: &Ev| -> Vec<f64> {
            (0..m)
                .map(|j| (e.obj[j] - lo[j]) / (hi[j] - lo[j]).max(1e-300))
                .collect()
        };
        let pts: Vec<Vec<f64>> = cands.iter().map(|(e, _)| norm(e)).collect();
        let mut keep: Vec<usize> = Vec::new();
        for j in 0..m {
            let best = (0..pts.len())
                .min_by(|&a, &b| pts[a][j].total_cmp(&pts[b][j]).then(a.cmp(&b)))
                .unwrap_or(0);
            if !keep.contains(&best) {
                keep.push(best);
            }
        }
        let d2 = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f64>();
        let mut near: Vec<f64> = (0..pts.len())
            .map(|i| {
                keep.iter()
                    .map(|&k| d2(&pts[i], &pts[k]))
                    .fold(f64::INFINITY, f64::min)
            })
            .collect();
        while keep.len() < 20 {
            let next = (0..pts.len())
                .filter(|i| !keep.contains(i))
                .max_by(|&a, &b| near[a].total_cmp(&near[b]).then(b.cmp(&a)));
            let Some(next) = next else { break };
            keep.push(next);
            for i in 0..pts.len() {
                near[i] = near[i].min(d2(&pts[i], &pts[next]));
            }
        }
        keep.truncate(20);
        keep.sort_unstable();
        cands = keep.into_iter().map(|i| cands[i].clone()).collect();
    } else if objectives.len() < 2 {
        cands.truncate(p.n_candidates.max(1));
    }

    let mut out: Vec<MultiCandidate> = cands
        .iter()
        .map(|(e, j)| {
            let dc = p.domain.check(&e.x);
            MultiCandidate {
                inputs: e.x.clone(),
                predictions: e.pred.clone(),
                goals_met: p
                    .goals
                    .iter()
                    .map(|g| {
                        let m = &p.models[g.model];
                        violation(g.spec.kind, e.pred[g.model], m.scale, m.tolerance) <= 1e-9
                    })
                    .collect(),
                joint_probability: *j,
                in_domain: dc.in_domain,
                domain_distance: dc.distance,
                marks: Vec::new(),
            }
        })
        .collect();

    // Marks.
    let raw = |c: &MultiCandidate, gi: usize| -> f64 {
        let g = &p.goals[gi];
        let y = c.predictions[g.model];
        if g.spec.kind == GoalKind::Maximize {
            -y
        } else {
            y
        }
    };
    if !out.is_empty() {
        if objectives.len() >= 2 {
            for &gi in &objectives {
                if let Some(b) =
                    (0..out.len()).min_by(|&a, &b| raw(&out[a], gi).total_cmp(&raw(&out[b], gi)))
                {
                    out[b].marks.push(format!("best:{}", p.goals[gi].spec.text));
                }
            }
            let lo: Vec<f64> = objectives
                .iter()
                .map(|&g| out.iter().map(|c| raw(c, g)).fold(f64::INFINITY, f64::min))
                .collect();
            let hi: Vec<f64> = objectives
                .iter()
                .map(|&g| {
                    out.iter()
                        .map(|c| raw(c, g))
                        .fold(f64::NEG_INFINITY, f64::max)
                })
                .collect();
            let norm = |c: &MultiCandidate, k: usize| {
                (raw(c, objectives[k]) - lo[k]) / (hi[k] - lo[k]).max(1e-300)
            };
            if let Some(k) = (0..out.len()).min_by(|&a, &b| {
                let sa: f64 = (0..objectives.len()).map(|k| norm(&out[a], k)).sum();
                let sb: f64 = (0..objectives.len()).map(|k| norm(&out[b], k)).sum();
                sa.total_cmp(&sb)
            }) {
                out[k].marks.push("knee".into());
            }
            let pref = match p.priority {
                Priority::Weighted => (0..out.len()).min_by(|&a, &b| {
                    let t = |c: &MultiCandidate| {
                        (0..objectives.len())
                            .map(|k| p.goals[objectives[k]].spec.weight * norm(c, k))
                            .fold(0.0, f64::max)
                    };
                    t(&out[a]).total_cmp(&t(&out[b]))
                }),
                Priority::Lexicographic => {
                    let mut keep: Vec<usize> = (0..out.len()).collect();
                    for k in 0..objectives.len() {
                        let best = keep
                            .iter()
                            .map(|&i| norm(&out[i], k))
                            .fold(f64::INFINITY, f64::min);
                        keep.retain(|&i| norm(&out[i], k) <= best + 0.01);
                    }
                    keep.first().copied()
                }
            };
            if let Some(k) = pref {
                out[k].marks.push("preferred".into());
            }
        } else {
            out[0].marks.push("best".into());
        }
        if let Some(k) = (0..out.len())
            .filter(|&i| out[i].joint_probability.is_finite())
            .max_by(|&a, &b| {
                out[a]
                    .joint_probability
                    .total_cmp(&out[b].joint_probability)
                    .then(b.cmp(&a))
            })
        {
            out[k].marks.push("most_likely".into());
        }
    }

    // Redundant objectives, judged on the whole first front (before trimming to 20).
    let mut redundant = Vec::new();
    if full_front.len() >= 4 {
        for (i, &a) in objectives.iter().enumerate() {
            for &b in &objectives[i + 1..] {
                let sign = |g: usize| {
                    if p.goals[g].spec.kind == GoalKind::Maximize {
                        -1.0
                    } else {
                        1.0
                    }
                };
                let va: Vec<f64> = full_front
                    .iter()
                    .map(|y| sign(a) * y[p.goals[a].model])
                    .collect();
                let vb: Vec<f64> = full_front
                    .iter()
                    .map(|y| sign(b) * y[p.goals[b].model])
                    .collect();
                let rho = spearman(&va, &vb);
                if rho > 0.9 {
                    redundant.push((a, b, rho));
                }
            }
        }
    }

    // Conflict analysis when nothing is feasible.
    let feasible = !out.is_empty();
    let mut evaluations = evaluations_main;
    let conflict = if feasible {
        None
    } else {
        let sub_budget = (p.max_evals / 10).max(2000);
        let feasible_with = |hset: &[usize], seed_k: u64, evals: &mut usize| -> bool {
            let hm: Vec<bool> = (0..p.goals.len()).map(|g| hset.contains(&g)).collect();
            let mut sv = Solver {
                p,
                bounds: bounds.clone(),
                hard: hm,
                objectives: Vec::new(),
                change_w: 1e-3,
                evals: 0,
            };
            let mut r = Rng::derive(p.seed, &[stream::OPTIMIZE, 51, seed_k]);
            let res = sv.single(&mut r, sub_budget, 1);
            *evals += sv.evals;
            res.first().is_some_and(|e| e.cv == 0.0)
        };
        let mut set: Vec<usize> = (0..p.goals.len()).filter(|&g| hard[g]).collect();
        let all_hard = set.clone();
        for (k, &h) in all_hard.iter().enumerate() {
            let without: Vec<usize> = set.iter().copied().filter(|&g| g != h).collect();
            if !feasible_with(&without, k as u64, &mut evaluations) {
                set = without;
            }
        }
        let mut best = Vec::new();
        for (k, &h) in set.iter().enumerate() {
            let others: Vec<usize> = set.iter().copied().filter(|&g| g != h).collect();
            let hm: Vec<bool> = (0..p.goals.len()).map(|g| others.contains(&g)).collect();
            let mut sv = Solver {
                p,
                bounds: bounds.clone(),
                hard: hm,
                objectives: vec![h],
                change_w: 1e-3,
                evals: 0,
            };
            let mut r = Rng::derive(p.seed, &[stream::OPTIMIZE, 52, k as u64]);
            let res = sv.single(&mut r, sub_budget, 1);
            evaluations += sv.evals;
            if let Some(e) = res.iter().find(|e| e.cv == 0.0).or(res.first()) {
                best.push((h, e.pred[p.goals[h].model]));
            }
        }
        Some(Conflict {
            goals: set,
            best_achievable: best,
        })
    };
    MultiOutput {
        candidates: out,
        method,
        feasible,
        conflict,
        redundant,
        evaluations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::parse_goal;
    use tarafold_expr::parse;

    fn domain(names: &[String], ranges: &[[f64; 2]]) -> Domain {
        let mut r = Rng::derive(1, &[]);
        let x: Vec<Vec<f64>> = ranges
            .iter()
            .map(|[lo, hi]| (0..500).map(|_| r.range(*lo, *hi)).collect())
            .collect();
        Domain::fit(names, &x)
    }

    fn model(name: &str, src: &str, names: &[String], scale: f64) -> Model {
        let e = parse(src, names).unwrap();
        Model {
            name: name.into(),
            members: vec![e.clone(); 5],
            expr: e,
            scale,
            tolerance: 0.01 * scale,
        }
    }

    fn problem<'a>(
        models: &'a [Model],
        goals: &'a [Goal],
        dom: &'a Domain,
        names: &[String],
    ) -> MultiProblem<'a> {
        let _ = names;
        MultiProblem {
            models,
            goals,
            controllables: vec![0, 1],
            bounds: vec![[dom.min[0], dom.max[0]], [dom.min[1], dom.max[1]]],
            kinds: vec![Kind::Continuous; 2],
            constraints: &[],
            baseline: dom.median.clone(),
            scale: dom.std.clone(),
            domain: dom,
            n_candidates: 5,
            max_evals: 40_000,
            robust: None,
            priority: Priority::Weighted,
            seed: 1,
        }
    }

    fn goal(text: &str, hard: bool, models: &[Model]) -> Goal {
        let spec = parse_goal(text, hard).unwrap();
        let model = models.iter().position(|m| m.name == spec.target).unwrap();
        Goal { spec, model }
    }

    #[test]
    fn threshold_plus_objective() {
        // Minimize cost = a + b subject to output = a * b >= 20.
        let names: Vec<String> = vec!["a".into(), "b".into()];
        let dom = domain(&names, &[[1.0, 10.0], [1.0, 10.0]]);
        let models = vec![
            model("output", "a * b", &names, 20.0),
            model("cost", "a + b", &names, 3.0),
        ];
        let goals = vec![
            goal("output>=20", true, &models),
            goal("min:cost", false, &models),
        ];
        let out = optimize_goals(&problem(&models, &goals, &dom, &names));
        assert!(out.feasible);
        let best = &out.candidates[0];
        assert!(best.predictions[0] >= 20.0 - 1e-6);
        // Optimum a = b = sqrt(20) ≈ 4.472, cost ≈ 8.944 (small penalties shift it slightly).
        assert!(
            (best.predictions[1] - 8.944).abs() < 0.1,
            "{:?}",
            best.predictions
        );
        assert_eq!(best.joint_probability, 1.0);
    }

    #[test]
    fn two_objective_front_and_marks() {
        // Maximize f1 = a, maximize f2 = 10 - a - b... trade-off: f1 = a, f2 = b, with a + b <= 10.
        let names: Vec<String> = vec!["a".into(), "b".into()];
        let dom = domain(&names, &[[0.0, 10.0], [0.0, 10.0]]);
        let models = vec![model("f1", "a", &names, 3.0), model("f2", "b", &names, 3.0)];
        let goals = vec![
            goal("max:f1", false, &models),
            goal("max:f2:2", false, &models),
        ];
        let cons = vec![crate::spec::parse_constraint("a + b <= 10", &names).unwrap()];
        let mut p = problem(&models, &goals, &dom, &names);
        p.constraints = &cons;
        let out = optimize_goals(&p);
        assert!(out.method.starts_with("NSGA-II"));
        assert!(out.candidates.len() >= 8, "{}", out.candidates.len());
        for c in &out.candidates {
            // Pareto-optimal points sit on a + b = 10 (up to small penalties).
            assert!(
                c.predictions[0] + c.predictions[1] > 9.0,
                "{:?}",
                c.predictions
            );
        }
        let marks: Vec<&str> = out
            .candidates
            .iter()
            .flat_map(|c| c.marks.iter().map(String::as_str))
            .collect();
        for m in [
            "best:max:f1",
            "best:max:f2:2",
            "knee",
            "preferred",
            "most_likely",
        ] {
            assert!(marks.contains(&m), "{m} missing from {marks:?}");
        }
    }

    #[test]
    fn five_objectives_use_reference_directions() {
        let names: Vec<String> = vec!["a".into(), "b".into()];
        let dom = domain(&names, &[[0.0, 1.0], [0.0, 1.0]]);
        let models: Vec<Model> = (0..5)
            .map(|k| {
                model(
                    &format!("f{k}"),
                    &format!(
                        "square(a - {}) + square(b - {})",
                        k as f64 / 4.0,
                        1.0 - k as f64 / 4.0
                    ),
                    &names,
                    0.3,
                )
            })
            .collect();
        let goals: Vec<Goal> = (0..5)
            .map(|k| goal(&format!("min:f{k}"), false, &models))
            .collect();
        let mut p = problem(&models, &goals, &dom, &names);
        p.max_evals = 30_000;
        let out = optimize_goals(&p);
        assert!(out.method.starts_with("NSGA-III"));
        assert!(!out.candidates.is_empty() && out.candidates.len() <= 20);
    }

    #[test]
    fn conflicting_goals_are_reported() {
        let names: Vec<String> = vec!["a".into(), "b".into()];
        let dom = domain(&names, &[[0.0, 10.0], [0.0, 10.0]]);
        let models = vec![
            model("s", "a + b", &names, 3.0),
            model("d", "a - b", &names, 3.0),
            model("q", "a", &names, 3.0),
        ];
        // s >= 15 and d >= 8 cannot both hold with a, b in [0, 10]... a + b >= 15 needs b >= 5,
        // a - b >= 8 needs b <= 2. q <= 9 is satisfiable alone.
        let goals = vec![
            goal("s>=15", true, &models),
            goal("d>=8", true, &models),
            goal("q<=9.9", true, &models),
        ];
        let out = optimize_goals(&problem(&models, &goals, &dom, &names));
        assert!(!out.feasible);
        let c = out.conflict.unwrap();
        assert!(
            c.goals.contains(&0) && c.goals.contains(&1),
            "{:?}",
            c.goals
        );
        assert!(!c.goals.contains(&2), "{:?}", c.goals);
        assert_eq!(c.best_achievable.len(), c.goals.len());
    }

    #[test]
    fn redundant_objectives_detected() {
        let names: Vec<String> = vec!["a".into(), "b".into()];
        let dom = domain(&names, &[[0.0, 10.0], [0.0, 10.0]]);
        let models = vec![
            model("energy", "a", &names, 3.0),
            model("co2", "2 * a + 1", &names, 6.0),
            model("yield", "b - 0.1 * a", &names, 3.0),
        ];
        let goals = vec![
            goal("min:energy", false, &models),
            goal("min:co2", false, &models),
            goal("max:yield", false, &models),
        ];
        let out = optimize_goals(&problem(&models, &goals, &dom, &names));
        assert!(
            out.redundant.iter().any(|&(a, b, _)| (a, b) == (0, 1)),
            "{:?}",
            out.redundant
        );
    }

    #[test]
    fn deterministic() {
        let names: Vec<String> = vec!["a".into(), "b".into()];
        let dom = domain(&names, &[[0.0, 10.0], [0.0, 10.0]]);
        let models = vec![
            model("f1", "a", &names, 3.0),
            model("f2", "b * b - a", &names, 3.0),
        ];
        let goals = vec![
            goal("max:f1", false, &models),
            goal("min:f2", false, &models),
        ];
        let a = optimize_goals(&problem(&models, &goals, &dom, &names));
        let b = optimize_goals(&problem(&models, &goals, &dom, &names));
        assert_eq!(a, b);
    }
}
