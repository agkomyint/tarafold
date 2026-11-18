//! Multi-goal benchmark (T11, design §6.2): problems with known Pareto fronts. Each run goes
//! through the whole pipeline (discover every objective, then optimize all of them jointly);
//! the returned candidates are scored with the true functions.

use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_2;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use tarafold_core::{Rng, RunConfig, rng::stream};
use tarafold_io::{Cell, IngestOptions, read_csv, write_csv};

/// A known-front problem.
struct Problem {
    name: &'static str,
    objectives: usize,
    inputs: usize,
    /// True objective values at a point in `[0, 1]^inputs` (all minimized).
    f: fn(&[f64]) -> Vec<f64>,
    /// A point on the true front for a parameter `u` in `[0, 1]^(objectives - 1)`.
    front: fn(&[f64]) -> Vec<f64>,
    /// Hard goals (`require`) and how to check them on true values.
    require: &'static [(&'static str, usize, f64)],
    /// Extra GP operators the true equations need.
    operators: &'static [&'static str],
}

fn dtlz2(x: &[f64], m: usize) -> Vec<f64> {
    // Angles from the first m-1 inputs, distance from the last.
    let g = x[m - 1] * x[m - 1];
    let r = 1.0 + g;
    (0..m)
        .map(|i| {
            let mut v = r;
            for a in &x[..m - 1 - i] {
                v *= (a * FRAC_PI_2).cos();
            }
            if i > 0 {
                v *= (x[m - 1 - i] * FRAC_PI_2).sin();
            }
            v
        })
        .collect()
}

fn dtlz2_front(u: &[f64]) -> Vec<f64> {
    let mut x = u.to_vec();
    x.push(0.0);
    dtlz2(&x, u.len() + 1)
}

const PROBLEMS: [Problem; 5] = [
    Problem {
        name: "convex-2",
        objectives: 2,
        inputs: 2,
        f: |x| {
            vec![
                x[0] * x[0] + x[1] * x[1],
                (x[0] - 1.0).powi(2) + x[1] * x[1],
            ]
        },
        front: |u| vec![u[0] * u[0], (u[0] - 1.0).powi(2)],
        require: &[],
        operators: &[],
    },
    Problem {
        name: "concave-2",
        objectives: 2,
        inputs: 2,
        f: |x| vec![2.0 * x[0], 1.0 - x[0] * x[0] + x[1]],
        front: |u| vec![2.0 * u[0], 1.0 - u[0] * u[0]],
        require: &[],
        operators: &[],
    },
    Problem {
        name: "convex-2-goal",
        objectives: 2,
        inputs: 2,
        f: |x| {
            vec![
                x[0] * x[0] + x[1] * x[1],
                (x[0] - 1.0).powi(2) + x[1] * x[1],
            ]
        },
        // With f1 <= 0.25 the front is the part with u <= 0.5.
        front: |u| {
            let v = 0.5 * u[0];
            vec![v * v, (v - 1.0).powi(2)]
        },
        require: &[("f1<=0.25", 0, 0.25)],
        operators: &[],
    },
    Problem {
        name: "dtlz2-3",
        objectives: 3,
        inputs: 3,
        f: |x| dtlz2(x, 3),
        front: dtlz2_front,
        require: &[],
        operators: &["sin", "cos", "square"],
    },
    Problem {
        name: "dtlz2-5",
        objectives: 5,
        inputs: 5,
        f: |x| dtlz2(x, 5),
        front: dtlz2_front,
        require: &[],
        operators: &["sin", "cos", "square"],
    },
];

/// Result of one problem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiResult {
    /// Problem name.
    pub problem: String,
    /// Objectives.
    pub objectives: usize,
    /// Candidates returned.
    pub candidates: usize,
    /// Hypervolume of the returned candidates (true values) ÷ that of the same number of
    /// well-spread points on the true front.
    pub hypervolume_ratio: f64,
    /// Hypervolume of the returned candidates ÷ that of the densely sampled true front (a finite
    /// set cannot reach 1 here; shown for reference).
    pub hypervolume_ratio_dense: f64,
    /// Share of candidates whose hard goals hold under the true functions (`null` without goals).
    pub goal_truth_rate: Option<f64>,
    /// Reported joint probability and whether the goals held under truth, per candidate.
    pub calibration: Vec<(f64, bool)>,
    /// Selected equation and test R² per objective.
    pub equations: Vec<(String, f64)>,
    /// Failure, if any.
    pub error: Option<String>,
    /// Seconds.
    pub seconds: f64,
}

/// Suite output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiReport {
    /// Per problem.
    pub results: Vec<MultiResult>,
    /// Mean hypervolume ratio over 2- and 3-objective problems.
    pub hv_ratio_2_3: f64,
    /// Hypervolume ratio on 5 objectives.
    pub hv_ratio_5: f64,
    /// Pooled goal truth rate.
    pub goal_truth_rate: f64,
    /// Largest |reported − observed| over joint-probability bins with ≥ 5 candidates.
    pub calibration_error: f64,
    /// T11 thresholds: HV ≥ 0.9 (2–3 objectives), ≥ 0.75 (5), goal truth ≥ 0.9, calibration ≤ 0.1.
    pub t11_met: bool,
}

fn dominates_point(p: &[f64], q: &[f64]) -> bool {
    p.iter().zip(q).all(|(a, b)| a <= b)
}

/// Hypervolume of `pts` (minimization) against `reference`: exact in 2-D, Monte Carlo with
/// deterministic samples otherwise.
fn hypervolume(pts: &[Vec<f64>], reference: &[f64], samples: usize, seed: u64) -> f64 {
    let pts: Vec<&Vec<f64>> = pts
        .iter()
        .filter(|p| p.iter().zip(reference).all(|(a, r)| a < r && a.is_finite()))
        .collect();
    if pts.is_empty() {
        return 0.0;
    }
    let m = reference.len();
    if m == 2 {
        let mut s: Vec<&Vec<f64>> = pts.clone();
        s.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let mut hv = 0.0;
        let mut best_y = reference[1];
        for p in s {
            if p[1] < best_y {
                hv += (reference[0] - p[0]) * (best_y - p[1]);
                best_y = p[1];
            }
        }
        return hv;
    }
    let lo: Vec<f64> = (0..m)
        .map(|j| {
            pts.iter()
                .map(|p| p[j])
                .fold(f64::INFINITY, f64::min)
                .min(0.0)
        })
        .collect();
    let vol: f64 = (0..m).map(|j| reference[j] - lo[j]).product();
    let mut rng = Rng::derive(seed, &[stream::SYNTH, 1100]);
    let mut hit = 0usize;
    let mut q = vec![0.0; m];
    for _ in 0..samples {
        for j in 0..m {
            q[j] = rng.range(lo[j], reference[j]);
        }
        if pts.iter().any(|p| dominates_point(p, &q)) {
            hit += 1;
        }
    }
    vol * hit as f64 / samples as f64
}

/// `k` well-spread points of `front`: the best point per objective, then repeatedly the point
/// farthest from those chosen (the same rule the optimizer uses to trim its trade-off set).
fn spread_subset(front: &[Vec<f64>], k: usize) -> Vec<Vec<f64>> {
    let m = front.first().map_or(0, Vec::len);
    let mut keep: Vec<usize> = Vec::new();
    for j in 0..m {
        if let Some(b) = (0..front.len()).min_by(|&a, &b| front[a][j].total_cmp(&front[b][j]))
            && !keep.contains(&b)
        {
            keep.push(b);
        }
    }
    let d2 = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f64>();
    let mut near: Vec<f64> = front
        .iter()
        .map(|p| {
            keep.iter()
                .map(|&q| d2(p, &front[q]))
                .fold(f64::INFINITY, f64::min)
        })
        .collect();
    while keep.len() < k.min(front.len()) {
        let Some(next) = (0..front.len())
            .filter(|i| !keep.contains(i))
            .max_by(|&a, &b| near[a].total_cmp(&near[b]))
        else {
            break;
        };
        keep.push(next);
        for (i, p) in front.iter().enumerate() {
            near[i] = near[i].min(d2(p, &front[next]));
        }
    }
    keep.into_iter().map(|i| front[i].clone()).collect()
}

fn true_front(p: &Problem, k: usize) -> Vec<Vec<f64>> {
    let d = p.objectives - 1;
    let mut rng = Rng::derive(7, &[stream::SYNTH, 1200]);
    (0..k)
        .map(|i| {
            let u: Vec<f64> = if d == 1 {
                vec![i as f64 / (k - 1).max(1) as f64]
            } else {
                (0..d).map(|_| rng.uniform()).collect()
            };
            (p.front)(&u)
        })
        .collect()
}

fn run_problem(p: &Problem, seed: u64) -> MultiResult {
    let t0 = std::time::Instant::now();
    let fail = |e: String| MultiResult {
        problem: p.name.into(),
        objectives: p.objectives,
        candidates: 0,
        hypervolume_ratio: 0.0,
        hypervolume_ratio_dense: 0.0,
        goal_truth_rate: None,
        calibration: Vec::new(),
        equations: Vec::new(),
        error: Some(e),
        seconds: t0.elapsed().as_secs_f64(),
    };
    let rows = 1000;
    let mut rng = Rng::derive(seed, &[stream::SYNTH, 1300]);
    let xs: Vec<Vec<f64>> = (0..rows)
        .map(|_| (0..p.inputs).map(|_| rng.uniform()).collect())
        .collect();
    let mut headers: Vec<String> = (1..=p.inputs).map(|i| format!("x{i}")).collect();
    headers.extend((1..=p.objectives).map(|i| format!("f{i}")));
    let cells: Vec<Vec<Cell>> = xs
        .iter()
        .map(|x| x.iter().copied().chain((p.f)(x)).map(Cell::Num).collect())
        .collect();
    let Ok(csv) = write_csv(&headers, &cells) else {
        return fail("csv".into());
    };
    let data = match read_csv(csv.as_bytes(), p.name, &IngestOptions::default()) {
        Ok(d) => d,
        Err(e) => return fail(e.to_string()),
    };
    let xnames: Vec<String> = (1..=p.inputs).map(|i| format!("x{i}")).collect();
    let mut cfg = RunConfig {
        targets: (1..=p.objectives).map(|i| format!("f{i}")).collect(),
        exclude_other_targets: true,
        seed,
        ..Default::default()
    };
    cfg.optimize.prefer = (1..=p.objectives).map(|i| format!("min:f{i}")).collect();
    cfg.optimize.require = p.require.iter().map(|r| r.0.to_string()).collect();
    cfg.optimize.controllables = xnames.clone();
    for n in &xnames {
        cfg.optimize.bounds.insert(n.clone(), [0.0, 1.0]);
    }
    cfg.optimize.n_candidates = 20;
    cfg.validate.stability = 0;
    cfg.validate.bootstrap = 50;
    for op in p.operators {
        if !cfg.discover.gp.operators.iter().any(|o| o == op) {
            cfg.discover.gp.operators.push((*op).to_string());
        }
    }
    let (report, _) = match tarafold_engine::run(&data, &cfg) {
        Ok(r) => r,
        Err(e) => return fail(e.to_string()),
    };
    let Some(o) = &report.optimize else {
        return fail("no optimization result".into());
    };
    let mut values = Vec::new();
    let mut calibration = Vec::new();
    let mut met = 0usize;
    for c in &o.candidates {
        let x: Option<Vec<f64>> = xnames.iter().map(|n| c.inputs.get(n).copied()).collect();
        let Some(x) = x else { continue };
        let f = (p.f)(&x);
        let ok = p.require.iter().all(|&(_, j, v)| f[j] <= v + 1e-9);
        if ok {
            met += 1;
            values.push(f);
        }
        if !p.require.is_empty() && c.joint_probability.is_finite() {
            calibration.push((c.joint_probability, ok));
        }
    }
    let samples = 60_000;
    let front = true_front(p, if p.objectives == 2 { 2000 } else { 3000 });
    // Reference point: 10% beyond the true front's worst value per objective.
    let reference: Vec<f64> = (0..p.objectives)
        .map(|j| 1.1 * front.iter().map(|f| f[j]).fold(f64::NEG_INFINITY, f64::max))
        .collect();
    let hv_dense = hypervolume(&front, &reference, samples, 1);
    // The same number of well-spread points on the true front: what a perfect run could return.
    let ideal = spread_subset(&front, o.candidates.len().max(1));
    let hv_true = hypervolume(&ideal, &reference, samples, 1);
    let hv = hypervolume(&values, &reference, samples, 1);
    MultiResult {
        problem: p.name.into(),
        objectives: p.objectives,
        candidates: o.candidates.len(),
        hypervolume_ratio: if hv_true > 0.0 { hv / hv_true } else { 0.0 },
        hypervolume_ratio_dense: if hv_dense > 0.0 { hv / hv_dense } else { 0.0 },
        goal_truth_rate: (!p.require.is_empty())
            .then(|| met as f64 / o.candidates.len().max(1) as f64),
        calibration,
        equations: report
            .targets
            .iter()
            .map(|t| {
                let e = &t.discover.front[t.discover.selected_id];
                (format!("{} = {}", t.target, e.infix), e.metrics.test.r2)
            })
            .collect(),
        error: None,
        seconds: t0.elapsed().as_secs_f64(),
    }
}

/// Run every problem.
pub fn run_multi(seed: u64, mut progress: impl FnMut(&MultiResult)) -> MultiReport {
    let results: Vec<MultiResult> = PROBLEMS
        .iter()
        .map(|p| {
            let r = run_problem(p, seed);
            progress(&r);
            r
        })
        .collect();
    let mean = |v: &[f64]| {
        if v.is_empty() {
            f64::NAN
        } else {
            v.iter().sum::<f64>() / v.len() as f64
        }
    };
    let hv23: Vec<f64> = results
        .iter()
        .filter(|r| r.objectives <= 3)
        .map(|r| r.hypervolume_ratio)
        .collect();
    let hv5: Vec<f64> = results
        .iter()
        .filter(|r| r.objectives == 5)
        .map(|r| r.hypervolume_ratio)
        .collect();
    let truth: Vec<f64> = results.iter().filter_map(|r| r.goal_truth_rate).collect();
    let mut bins: BTreeMap<usize, (f64, f64, usize)> = BTreeMap::new();
    for (p, ok) in results.iter().flat_map(|r| r.calibration.iter()) {
        let b = ((p * 10.0).floor() as usize).min(9);
        let e = bins.entry(b).or_insert((0.0, 0.0, 0));
        e.0 += p;
        e.1 += f64::from(u8::from(*ok));
        e.2 += 1;
    }
    let calibration_error = bins
        .values()
        .filter(|(_, _, n)| *n >= 5)
        .map(|(sp, so, n)| ((sp - so) / *n as f64).abs())
        .fold(0.0, f64::max);
    let hv_ratio_2_3 = mean(&hv23);
    let hv_ratio_5 = mean(&hv5);
    let goal_truth_rate = mean(&truth);
    MultiReport {
        t11_met: hv_ratio_2_3 >= 0.9
            && hv_ratio_5 >= 0.75
            && goal_truth_rate >= 0.9
            && calibration_error <= 0.1,
        results,
        hv_ratio_2_3,
        hv_ratio_5,
        goal_truth_rate,
        calibration_error,
    }
}

/// Markdown summary.
pub fn markdown(r: &MultiReport) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "## Tarafold bench: multi-goal suite (T11)\n");
    let _ = writeln!(
        s,
        "Each problem: 1,000 noiseless rows, every objective discovered from data, then all minimized jointly; candidates scored with the true functions. HV ratio: hypervolume of the candidates ÷ that of the same number of well-spread points on the true front. Hypervolume against a reference point 10% beyond the true front's worst value per objective (exact for 2 objectives, Monte Carlo otherwise).\n"
    );
    let _ = writeln!(
        s,
        "| problem | objectives | candidates | HV ratio | HV vs dense front | goal truth rate | seconds | error |"
    );
    let _ = writeln!(s, "|---|---|---|---|---|---|---|---|");
    for x in &r.results {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {:.3} | {:.3} | {} | {:.1} | {} |",
            x.problem,
            x.objectives,
            x.candidates,
            x.hypervolume_ratio,
            x.hypervolume_ratio_dense,
            x.goal_truth_rate
                .map_or("—".to_string(), |v| format!("{v:.2}")),
            x.seconds,
            x.error.clone().unwrap_or_default()
        );
    }
    let _ = writeln!(s, "\nSelected equations (test R²):\n");
    for x in &r.results {
        for (e, r2) in &x.equations {
            let _ = writeln!(s, "- {}: `{e}` ({r2:.4})", x.problem);
        }
    }
    let _ = writeln!(
        s,
        "\nHV ratio (2–3 objectives) {:.3}; HV ratio (5 objectives) {:.3}; goal truth rate {:.2}; calibration error {:.2}. T11 thresholds: {}.",
        r.hv_ratio_2_3,
        r.hv_ratio_5,
        r.goal_truth_rate,
        r.calibration_error,
        if r.t11_met { "met" } else { "NOT met" }
    );
    s
}
