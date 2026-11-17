//! Closed-loop benchmark (T12, R11.3): start with a few rows, add a batch of experiments per
//! round chosen by `--suggest`, by uniform random sampling, or by Latin-hypercube sampling,
//! "run" them by evaluating the true equation plus noise, and count how many experiments each
//! method needs before the pipeline's optimizer proposes inputs that hit the goal under the
//! true equation.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use tarafold_core::{Rng, RunConfig, rng::stream, stats};
use tarafold_expr::parse;
use tarafold_io::{Cell, IngestOptions, read_csv, write_csv};
use tarafold_synth::{Entry, evaluate_row, find, generate, input_ranges};

/// Entries used: every feature is a set input (no intermediate observed columns).
pub const ENTRIES: [&str; 5] = [
    "cobb-douglas",
    "exp-log",
    "kinetic-energy",
    "business-direct",
    "production",
];
/// Ingest needs at least 30 usable rows.
const START_ROWS: usize = 30;
const BATCH: usize = 5;
const ROUNDS: usize = 8;
const NOISE: f64 = 0.05;

/// Experiment-selection method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// `--suggest` (balanced).
    Suggest,
    /// Uniform random inputs.
    Random,
    /// Latin-hypercube batch per round.
    Lhs,
}

/// One closed loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoopResult {
    /// Dataset.
    pub dataset: String,
    /// Method.
    pub method: Method,
    /// Seed.
    pub seed: u64,
    /// Experiments added when the goal was first reached under the true equation (`None` if
    /// not reached within the budget).
    pub experiments_to_goal: Option<usize>,
    /// Experiments added when the selected equation first matched the truth.
    pub experiments_to_recovery: Option<usize>,
}

/// Active-suite output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveReport {
    /// Every loop.
    pub loops: Vec<LoopResult>,
    /// Median experiments to goal per method (unreached counts as budget + batch).
    pub median_to_goal: BTreeMap<String, f64>,
    /// Median experiments to recovery per method.
    pub median_to_recovery: BTreeMap<String, f64>,
    /// Budget of added experiments per loop.
    pub budget: usize,
}

fn csv_of(headers: &[String], cols: &[Vec<f64>]) -> String {
    let n = cols.first().map_or(0, Vec::len);
    let rows: Vec<Vec<Cell>> = (0..n)
        .map(|i| cols.iter().map(|c| Cell::Num(c[i])).collect())
        .collect();
    write_csv(headers, &rows).unwrap_or_default()
}

fn snap(entry: &Entry, name: &str, v: f64) -> f64 {
    match tarafold_synth::grid_steps(entry)
        .iter()
        .find(|(n, _)| *n == name)
    {
        Some((_, s)) => {
            let lo = input_ranges(entry)
                .iter()
                .find(|(n, ..)| *n == name)
                .map_or(0.0, |r| r.1);
            lo + ((v - lo) / s).round() * s
        }
        None => v,
    }
}

/// Run one loop.
pub fn run_loop(dataset: &str, method: Method, seed: u64) -> Option<LoopResult> {
    let entry = find(dataset).ok()?;
    let g = generate(&entry, START_ROWS, NOISE, 0, seed).ok()?;
    let mut headers = g.headers.clone();
    let mut cols = g.columns.clone();
    let target = entry.target;
    let ti = headers.iter().position(|h| h == target)?;
    let sd = stats::std(&cols[ti]) * NOISE;
    let truth = parse(&g.truth.equation, &g.truth.features).ok()?;
    let ranges = input_ranges(&entry);
    let mut rng = Rng::derive(seed, &[stream::SYNTH, 900, method as u64]);
    let mut to_goal = None;
    let mut to_rec = None;
    let fresh = generate(&entry, 500, 0.0, 0, seed + 77).ok()?;
    for round in 0..=ROUNDS {
        let added = round * BATCH;
        let data = read_csv(
            csv_of(&headers, &cols).as_bytes(),
            dataset,
            &IngestOptions::default(),
        )
        .ok()?;
        let mut cfg = RunConfig {
            targets: vec![target.into()],
            seed,
            ..Default::default()
        };
        cfg.optimize.goal = Some(entry.example_goal);
        cfg.optimize.controllables = entry.controllables.iter().map(|s| s.to_string()).collect();
        for (n, step) in tarafold_synth::grid_steps(&entry) {
            if step == 1.0 {
                cfg.optimize.integer.push(n.into());
            } else {
                cfg.optimize.steps.insert(n.into(), step);
            }
        }
        cfg.validate.stability = 0;
        cfg.validate.bootstrap = 50;
        if method == Method::Suggest && round < ROUNDS {
            cfg.suggest.n = BATCH;
        }
        let Ok((report, _)) = tarafold_engine::run(&data, &cfg) else {
            break;
        };
        // Goal reached under the truth?
        if to_goal.is_none()
            && let Some(o) = &report.optimize
        {
            let hit = o.candidates.iter().filter(|c| c.within_tolerance).any(|c| {
                let x: Option<Vec<f64>> = g
                    .truth
                    .features
                    .iter()
                    .map(|n| c.inputs.get(n).or(o.baseline.get(n)).copied())
                    .collect();
                x.is_some_and(|x| (truth.eval_point(&x) - entry.example_goal).abs() <= o.tolerance)
            });
            if hit {
                to_goal = Some(added);
            }
        }
        // Equation recovered?
        if to_rec.is_none() {
            let t = &report.targets[0];
            let sel = &t.discover.front[t.discover.selected_id];
            if let Ok(found) = parse(&sel.infix_exact, &t.features) {
                let refs: Option<Vec<&[f64]>> = t
                    .features
                    .iter()
                    .map(|n| {
                        fresh
                            .headers
                            .iter()
                            .position(|h| h == n)
                            .map(|i| fresh.columns[i].as_slice())
                    })
                    .collect();
                let trefs: Option<Vec<&[f64]>> = g
                    .truth
                    .features
                    .iter()
                    .map(|n| {
                        fresh
                            .headers
                            .iter()
                            .position(|h| h == n)
                            .map(|i| fresh.columns[i].as_slice())
                    })
                    .collect();
                if let (Some(r), Some(tr)) = (refs, trefs)
                    && let (Some(a), Some(b)) =
                        (found.eval_columns(&r, 500), truth.eval_columns(&tr, 500))
                {
                    let rel = a
                        .iter()
                        .zip(&b)
                        .map(|(x, y)| (x - y).abs() / y.abs().max(1e-9))
                        .fold(0.0, f64::max);
                    if rel < 2e-2 {
                        to_rec = Some(added);
                    }
                }
            }
        }
        if (to_goal.is_some() && to_rec.is_some()) || round == ROUNDS {
            break;
        }
        // Next batch of inputs.
        let batch: Vec<Vec<(String, f64)>> = match method {
            Method::Suggest => report
                .suggest
                .as_ref()
                .map(|s| {
                    s.suggestions
                        .iter()
                        .map(|x| {
                            ranges
                                .iter()
                                .map(|(n, ..)| {
                                    (n.to_string(), x.row.get(*n).copied().unwrap_or(f64::NAN))
                                })
                                .collect()
                        })
                        .collect()
                })
                .unwrap_or_default(),
            Method::Random => (0..BATCH)
                .map(|_| {
                    ranges
                        .iter()
                        .map(|(n, lo, hi)| (n.to_string(), snap(&entry, n, rng.range(*lo, *hi))))
                        .collect()
                })
                .collect(),
            Method::Lhs => {
                let mut perms: Vec<Vec<usize>> = ranges
                    .iter()
                    .map(|_| {
                        let mut p: Vec<usize> = (0..BATCH).collect();
                        rng.shuffle(&mut p);
                        p
                    })
                    .collect();
                (0..BATCH)
                    .map(|i| {
                        ranges
                            .iter()
                            .zip(perms.iter_mut())
                            .map(|((n, lo, hi), p)| {
                                let u = (p[i] as f64 + rng.uniform()) / BATCH as f64;
                                (n.to_string(), snap(&entry, n, lo + u * (hi - lo)))
                            })
                            .collect()
                    })
                    .collect()
            }
        };
        if batch.is_empty() {
            break;
        }
        for inputs in batch {
            let Ok(row) = evaluate_row(&entry, &inputs) else {
                continue;
            };
            for (j, h) in headers.iter().enumerate() {
                let mut v = row
                    .iter()
                    .find(|(n, _)| n == h)
                    .map_or(f64::NAN, |(_, v)| *v);
                if j == ti {
                    v += sd * rng.normal();
                }
                cols[j].push(v);
            }
        }
        headers = headers.clone();
    }
    Some(LoopResult {
        dataset: dataset.into(),
        method,
        seed,
        experiments_to_goal: to_goal,
        experiments_to_recovery: to_rec,
    })
}

/// Run the active suite over `ENTRIES` × `seeds` × methods.
pub fn run_active(seeds: &[u64], mut progress: impl FnMut(&LoopResult)) -> ActiveReport {
    let mut loops = Vec::new();
    for d in ENTRIES {
        for &s in seeds {
            for m in [Method::Suggest, Method::Random, Method::Lhs] {
                if let Some(r) = run_loop(d, m, s) {
                    progress(&r);
                    loops.push(r);
                }
            }
        }
    }
    let budget = ROUNDS * BATCH;
    let med = |m: Method, f: &dyn Fn(&LoopResult) -> Option<usize>| -> f64 {
        let v: Vec<f64> = loops
            .iter()
            .filter(|l| l.method == m)
            .map(|l| f(l).map_or((budget + BATCH) as f64, |x| x as f64))
            .collect();
        if v.is_empty() {
            f64::NAN
        } else {
            stats::median(&v)
        }
    };
    let mut median_to_goal = BTreeMap::new();
    let mut median_to_recovery = BTreeMap::new();
    for (k, m) in [
        ("suggest", Method::Suggest),
        ("random", Method::Random),
        ("lhs", Method::Lhs),
    ] {
        median_to_goal.insert(k.to_string(), med(m, &|l| l.experiments_to_goal));
        median_to_recovery.insert(k.to_string(), med(m, &|l| l.experiments_to_recovery));
    }
    ActiveReport {
        loops,
        median_to_goal,
        median_to_recovery,
        budget,
    }
}

/// Markdown summary.
pub fn markdown(r: &ActiveReport) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "## Tarafold bench: active (closed-loop) suite\n");
    let _ = writeln!(
        s,
        "Start with {START_ROWS} rows; add {BATCH} experiments per round for up to {ROUNDS} rounds ({} max); noise {NOISE}.\n",
        r.budget
    );
    let _ = writeln!(
        s,
        "| dataset | seed | method | experiments to goal | experiments to recovery |"
    );
    let _ = writeln!(s, "|---|---|---|---|---|");
    let f = |v: Option<usize>| v.map_or("not reached".to_string(), |x| x.to_string());
    for l in &r.loops {
        let _ = writeln!(
            s,
            "| {} | {} | {:?} | {} | {} |",
            l.dataset,
            l.seed,
            l.method,
            f(l.experiments_to_goal),
            f(l.experiments_to_recovery)
        );
    }
    let _ = writeln!(
        s,
        "\n| method | median experiments to goal | median experiments to recovery |"
    );
    let _ = writeln!(s, "|---|---|---|");
    for k in ["suggest", "random", "lhs"] {
        let _ = writeln!(
            s,
            "| {k} | {:.1} | {:.1} |",
            r.median_to_goal[k], r.median_to_recovery[k]
        );
    }
    let _ = writeln!(
        s,
        "\nUnreached loops count as {} experiments in the medians.",
        r.budget + BATCH
    );
    s
}
