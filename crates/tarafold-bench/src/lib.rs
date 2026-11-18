//! Benchmark runner (R11, design §6.2): runs the synthetic registry through the full pipeline
//! and scores each result against the hidden true equation.

pub mod active;
pub mod multi;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tarafold_core::{RunConfig, Split, SplitStrategy, stats};
use tarafold_expr::{Expr, parse};
use tarafold_io::{IngestOptions, read_csv};
use tarafold_synth::{Entry, Generated, generate, generate_widened, registry};
use tarafold_validate::metrics;

/// Which cases to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Suite {
    /// Fast CI subset.
    Smoke,
    /// Every registry entry, noise level, and size.
    Full,
}

impl Suite {
    /// Key used in baseline files.
    pub fn key(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Full => "full",
        }
    }
}

/// One benchmark case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Case {
    /// Registry name.
    pub dataset: String,
    /// Rows generated.
    pub rows: usize,
    /// Noise level.
    pub noise: f64,
    /// Distractor columns.
    pub distractors: usize,
    /// Seed.
    pub seed: u64,
}

/// Entries in the smoke suite (one or two per tier, fast).
pub const SMOKE: [&str; 9] = [
    "linear",
    "polynomial",
    "cobb-douglas",
    "interaction",
    "kinetic-energy",
    "gravity",
    "business-revenue",
    "business-direct",
    "production",
];

/// The cases in a suite.
pub fn cases(suite: Suite) -> Vec<Case> {
    let mut out = Vec::new();
    let (rows, noises): (&[usize], &[f64]) = match suite {
        Suite::Smoke => (&[500], &[0.0, 0.1]),
        Suite::Full => (&[200, 2000], &[0.0, 0.01, 0.1]),
    };
    for e in registry() {
        if suite == Suite::Smoke && !SMOKE.contains(&e.name) {
            continue;
        }
        let distractor_options: &[usize] = if e.name == "interaction" {
            if suite == Suite::Smoke { &[5] } else { &[0, 5] }
        } else {
            &[0]
        };
        for &r in rows {
            for &n in noises {
                for &d in distractor_options {
                    // Two data seeds per case in the full suite: the search is stochastic, and
                    // one seed per case made a single flipped case look like a regression.
                    let seeds: &[u64] = if suite == Suite::Full { &[1, 2] } else { &[1] };
                    for &seed in seeds {
                        out.push(Case {
                            dataset: e.name.into(),
                            rows: r,
                            noise: n,
                            distractors: d,
                            seed,
                        });
                    }
                }
            }
        }
    }
    out
}

/// Scores for one case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseResult {
    /// The case.
    pub case: Case,
    /// Tier.
    pub tier: String,
    /// True equation.
    pub truth: String,
    /// Selected equation.
    pub equation: String,
    /// Selected equation equals the truth (max rel. error < 1e-3 on fresh in-domain points).
    pub exact: bool,
    /// Same structure: after refitting constants on noiseless data it equals the truth.
    pub structure: bool,
    /// R² on fresh noiseless in-domain data > 0.999.
    pub accuracy_solution: bool,
    /// R² on fresh noiseless in-domain data.
    pub r2_fresh: f64,
    /// R² on fresh noiseless data from ranges widened by 50%.
    pub r2_extrapolation: f64,
    /// Truth R² on the noisy test split minus the selected equation's.
    pub oracle_gap: f64,
    /// Precision of used variables vs. truth variables.
    pub var_precision: f64,
    /// Recall of used variables vs. truth variables.
    pub var_recall: f64,
    /// complexity(selected) / complexity(truth).
    pub complexity_ratio: f64,
    /// Within-tolerance candidates that also hit the goal under the true equation.
    pub inverse_ok: usize,
    /// Within-tolerance candidates.
    pub inverse_total: usize,
    /// Share of test rows inside the 90% interval.
    pub coverage_90: f64,
    /// Reliability grade of the selected equation.
    pub grade: String,
    /// Wall time in seconds.
    pub seconds: f64,
    /// T9 with noise: unweighted ÷ weighted RMSE against the truth (`null` otherwise).
    #[serde(default)]
    pub weighting_gain: Option<f64>,
    /// Failure, if the pipeline errored.
    pub error: Option<String>,
}

/// Aggregate metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Aggregate {
    /// Cases.
    pub cases: usize,
    /// Cases that errored.
    pub errors: usize,
    /// Exact recovery rate.
    pub exact_rate: f64,
    /// Structure recovery rate.
    pub structure_rate: f64,
    /// Accuracy-solution rate.
    pub accuracy_rate: f64,
    /// Median oracle gap.
    pub median_oracle_gap: f64,
    /// Median extrapolation R².
    pub median_extrapolation_r2: f64,
    /// Mean variable-selection precision.
    pub var_precision: f64,
    /// Mean variable-selection recall.
    pub var_recall: f64,
    /// Inverse success over all within-tolerance candidates.
    pub inverse_success: f64,
    /// Mean 90% interval coverage on noisy cases.
    pub coverage_90_noisy: f64,
    /// Median complexity ratio.
    pub median_complexity_ratio: f64,
    /// Share graded `high` among cases with the correct structure.
    #[serde(default)]
    pub high_given_structure: f64,
    /// Share graded `high` among cases with the wrong structure (should be low).
    #[serde(default)]
    pub high_given_wrong: f64,
    /// Median weighting gain on noisy T9 cases (`null` when there are none).
    #[serde(default)]
    pub median_weighting_gain: Option<f64>,
    /// Total seconds.
    pub seconds: f64,
}

/// Full benchmark output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchReport {
    /// Suite.
    pub suite: Suite,
    /// Tool version.
    pub tarafold_version: String,
    /// Per-case results.
    pub cases: Vec<CaseResult>,
    /// Overall aggregate.
    pub aggregate: Aggregate,
    /// Aggregate per tier.
    pub by_tier: BTreeMap<String, Aggregate>,
}

/// (found predictions, true values, feature columns) on fresh data.
type Fresh = (Vec<f64>, Vec<f64>, Vec<Vec<f64>>);

fn max_rel(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs() / y.abs().max(1e-9))
        .fold(0.0, f64::max)
}

fn columns<'a>(g: &'a Generated, names: &[String]) -> Option<Vec<&'a [f64]>> {
    names
        .iter()
        .map(|n| {
            g.headers
                .iter()
                .position(|h| h == n)
                .map(|i| g.columns[i].as_slice())
        })
        .collect()
}

fn failed(case: &Case, entry: &Entry, msg: String, seconds: f64) -> CaseResult {
    CaseResult {
        case: case.clone(),
        tier: entry.tier.into(),
        truth: String::new(),
        equation: String::new(),
        exact: false,
        structure: false,
        accuracy_solution: false,
        r2_fresh: f64::NAN,
        r2_extrapolation: f64::NAN,
        oracle_gap: f64::NAN,
        var_precision: 0.0,
        var_recall: 0.0,
        complexity_ratio: f64::NAN,
        inverse_ok: 0,
        inverse_total: 0,
        coverage_90: f64::NAN,
        grade: String::new(),
        seconds,
        weighting_gain: None,
        error: Some(msg),
    }
}

/// T9 entries are run with their `weight` column as row weights.
fn ingest_options(entry: &Entry) -> IngestOptions {
    IngestOptions {
        weights: entry.noise_scale.map(|_| "weight".to_string()),
        ..Default::default()
    }
}

/// Weighting gain on a T9 case: RMSE against the truth on fresh noiseless points of an
/// unweighted run (weight column removed) divided by that of the weighted run.
fn weighting_gain(
    entry: &Entry,
    g: &Generated,
    cfg: &RunConfig,
    weighted: &Expr,
    weighted_features: &[String],
    case: &Case,
) -> Option<f64> {
    let wi = g.headers.iter().position(|h| h == "weight")?;
    let mut unw = g.clone();
    unw.headers.remove(wi);
    unw.columns.remove(wi);
    let data = read_csv(
        unw.to_csv().ok()?.as_bytes(),
        entry.name,
        &IngestOptions::default(),
    )
    .ok()?;
    let mut cfg = cfg.clone();
    cfg.optimize = Default::default();
    cfg.validate.stability = 0;
    let (report, _) = tarafold_engine::run(&data, &cfg).ok()?;
    let t = &report.targets[0];
    let sel = &t.discover.front[t.discover.selected_id];
    let plain = parse(&sel.infix_exact, &t.features).ok()?;
    let truth = parse(&g.truth.equation, &g.truth.features).ok()?;
    let fresh = generate(entry, 1000, 0.0, case.distractors, case.seed + 2000).ok()?;
    let rmse = |e: &Expr, names: &[String]| -> Option<f64> {
        let a = e.eval_columns(&columns(&fresh, names)?, 1000)?;
        let b = truth.eval_columns(&columns(&fresh, &g.truth.features)?, 1000)?;
        Some((a.iter().zip(&b).map(|(x, y)| (x - y).powi(2)).sum::<f64>() / 1000.0).sqrt())
    };
    let r_w = rmse(weighted, weighted_features)?;
    let r_u = rmse(&plain, &t.features)?;
    Some(r_u / r_w.max(1e-12))
}

/// Run one case.
pub fn run_case(case: &Case) -> CaseResult {
    let t0 = Instant::now();
    let Some(entry) = registry().into_iter().find(|e| e.name == case.dataset) else {
        let e = Entry {
            name: "unknown",
            tier: "?",
            description: "",
            columns: vec![],
            target: "",
            controllables: vec![],
            example_goal: 0.0,
            noise_scale: None,
        };
        return failed(case, &e, format!("unknown dataset {}", case.dataset), 0.0);
    };
    let fail = |m: String| failed(case, &entry, m, t0.elapsed().as_secs_f64());
    let g = match generate(&entry, case.rows, case.noise, case.distractors, case.seed) {
        Ok(g) => g,
        Err(e) => return fail(e.to_string()),
    };
    let csv = match g.to_csv() {
        Ok(c) => c,
        Err(e) => return fail(e.to_string()),
    };
    let data = match read_csv(csv.as_bytes(), entry.name, &ingest_options(&entry)) {
        Ok(d) => d,
        Err(e) => return fail(e.to_string()),
    };
    let mut cfg = RunConfig {
        targets: vec![entry.target.into()],
        ..Default::default()
    };
    cfg.optimize.goal = Some(entry.example_goal);
    cfg.optimize.controllables = entry.controllables.iter().map(|s| s.to_string()).collect();
    for (n, step) in tarafold_synth::grid_steps(&entry) {
        if step == 1.0 {
            cfg.optimize.integer.push(n.to_string());
        } else {
            cfg.optimize.steps.insert(n.to_string(), step);
        }
    }
    // Fewer stability rediscoveries than the CLI default keep the 132-case suite practical;
    // selection and structure metrics do not depend on them.
    cfg.validate.stability = 3;
    let (report, _) = match tarafold_engine::run(&data, &cfg) {
        Ok(r) => r,
        Err(e) => return fail(e.to_string()),
    };
    let seconds = t0.elapsed().as_secs_f64();
    let t = &report.targets[0];
    let sel = &t.discover.front[t.discover.selected_id];
    let (Ok(found), Ok(truth)) = (
        parse(&sel.infix_exact, &t.features),
        parse(&g.truth.equation, &g.truth.features),
    ) else {
        return fail("could not parse equations".into());
    };

    // Fresh noiseless data (different seed), in-domain and widened.
    let score_fresh = |widen: f64| -> Option<Fresh> {
        let fresh =
            generate_widened(&entry, 1000, 0.0, case.distractors, case.seed + 1000, widen).ok()?;
        let fr = columns(&fresh, &t.features)?;
        let tr = columns(&fresh, &g.truth.features)?;
        let a = found.eval_columns(&fr, 1000)?;
        let b = truth.eval_columns(&tr, 1000)?;
        Some((a, b, fr.iter().map(|c| c.to_vec()).collect()))
    };
    let Some((a, b, fx)) = score_fresh(0.0) else {
        return fail("fresh-data evaluation failed".into());
    };
    let exact = max_rel(&a, &b) < 1e-3;
    let r2_fresh = metrics(&a, &b).r2;
    let refit = tarafold_discover::refit_constants(&found, &fx, &b);
    let structure = exact
        || refit
            .eval_columns(&fx.iter().map(Vec::as_slice).collect::<Vec<_>>(), b.len())
            .is_some_and(|ra| max_rel(&ra, &b) < 1e-3);
    let r2_extrapolation = score_fresh(0.5).map_or(f64::NAN, |(a, b, _)| metrics(&a, &b).r2);

    // Oracle gap on the noisy test split (same split the engine used).
    let oracle_gap = Split::new(
        data.n_rows,
        cfg.split.train,
        cfg.split.val,
        SplitStrategy::Random,
        cfg.seed,
    )
    .ok()
    .and_then(|s| {
        let cols: Vec<Vec<f64>> = g
            .truth
            .features
            .iter()
            .map(|n| {
                data.column(n)
                    .map(|c| stats::gather(&c.values, &s.test))
                    .unwrap_or_default()
            })
            .collect();
        let y = stats::gather(&data.column(entry.target).ok()?.values, &s.test);
        let refs: Vec<&[f64]> = cols.iter().map(Vec::as_slice).collect();
        let pred = truth.eval_columns(&refs, y.len())?;
        Some(metrics(&pred, &y).r2 - sel.metrics.test.r2)
    })
    .unwrap_or(f64::NAN);

    let used: BTreeSet<&str> = sel.variables.iter().map(String::as_str).collect();
    let want: BTreeSet<&str> = g
        .truth
        .equation_variables
        .iter()
        .map(String::as_str)
        .collect();
    let hit = used.intersection(&want).count() as f64;
    let var_precision = if used.is_empty() {
        0.0
    } else {
        hit / used.len() as f64
    };
    let var_recall = if want.is_empty() {
        1.0
    } else {
        hit / want.len() as f64
    };

    let (mut inverse_ok, mut inverse_total) = (0, 0);
    if let Some(o) = &report.optimize {
        for c in o.candidates.iter().filter(|c| c.within_tolerance) {
            let x: Option<Vec<f64>> = g
                .truth
                .features
                .iter()
                .map(|n| c.inputs.get(n).or(o.baseline.get(n)).copied())
                .collect();
            let Some(x) = x else { continue };
            inverse_total += 1;
            // Discrete inputs must be on their grid.
            let on_grid = tarafold_synth::grid_steps(&entry).iter().all(|(n, s)| {
                c.inputs
                    .get(*n)
                    .is_none_or(|v| ((v - 1.0) / s - ((v - 1.0) / s).round()).abs() < 1e-9)
            });
            if !on_grid {
                continue;
            }
            if (truth.eval_point(&x) - o.goal).abs() <= o.tolerance {
                inverse_ok += 1;
            }
        }
    }
    CaseResult {
        case: case.clone(),
        tier: entry.tier.into(),
        truth: g.truth.equation.clone(),
        equation: sel.infix.clone(),
        exact,
        structure,
        accuracy_solution: r2_fresh > 0.999,
        r2_fresh,
        r2_extrapolation,
        oracle_gap,
        var_precision,
        var_recall,
        complexity_ratio: found.complexity() as f64 / Expr::complexity(&truth).max(1) as f64,
        inverse_ok,
        inverse_total,
        coverage_90: t.validate.test_coverage_90,
        grade: format!("{:?}", t.validate.reliability.grade).to_lowercase(),
        seconds,
        weighting_gain: (entry.noise_scale.is_some() && case.noise > 0.0)
            .then(|| weighting_gain(&entry, &g, &cfg, &found, &t.features, case))
            .flatten(),
        error: None,
    }
}

fn median(v: &[f64]) -> f64 {
    let f: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if f.is_empty() {
        f64::NAN
    } else {
        stats::median(&f)
    }
}

fn share_high(results: &[&CaseResult], structure: bool) -> f64 {
    let set: Vec<&&CaseResult> = results
        .iter()
        .filter(|r| r.error.is_none() && r.structure == structure)
        .collect();
    if set.is_empty() {
        return f64::NAN;
    }
    set.iter().filter(|r| r.grade == "high").count() as f64 / set.len() as f64
}

/// Aggregate a set of results.
pub fn aggregate(results: &[&CaseResult]) -> Aggregate {
    let n = results.len().max(1) as f64;
    let rate = |f: &dyn Fn(&CaseResult) -> bool| results.iter().filter(|r| f(r)).count() as f64 / n;
    let ok: usize = results.iter().map(|r| r.inverse_ok).sum();
    let total: usize = results.iter().map(|r| r.inverse_total).sum();
    let noisy: Vec<f64> = results
        .iter()
        .filter(|r| r.case.noise > 0.0)
        .map(|r| r.coverage_90)
        .filter(|v| v.is_finite())
        .collect();
    Aggregate {
        cases: results.len(),
        errors: results.iter().filter(|r| r.error.is_some()).count(),
        exact_rate: rate(&|r| r.exact),
        structure_rate: rate(&|r| r.structure),
        accuracy_rate: rate(&|r| r.accuracy_solution),
        median_oracle_gap: median(&results.iter().map(|r| r.oracle_gap).collect::<Vec<_>>()),
        median_extrapolation_r2: median(
            &results
                .iter()
                .map(|r| r.r2_extrapolation)
                .collect::<Vec<_>>(),
        ),
        var_precision: results.iter().map(|r| r.var_precision).sum::<f64>() / n,
        var_recall: results.iter().map(|r| r.var_recall).sum::<f64>() / n,
        inverse_success: if total == 0 {
            f64::NAN
        } else {
            ok as f64 / total as f64
        },
        coverage_90_noisy: if noisy.is_empty() {
            f64::NAN
        } else {
            stats::mean(&noisy)
        },
        median_complexity_ratio: median(
            &results
                .iter()
                .map(|r| r.complexity_ratio)
                .collect::<Vec<_>>(),
        ),
        high_given_structure: share_high(results, true),
        high_given_wrong: share_high(results, false),
        median_weighting_gain: {
            let g: Vec<f64> = results.iter().filter_map(|r| r.weighting_gain).collect();
            (!g.is_empty()).then(|| median(&g))
        },
        seconds: results.iter().map(|r| r.seconds).sum(),
    }
}

/// Run a suite, calling `progress` after each case.
pub fn run(suite: Suite, mut progress: impl FnMut(usize, usize, &CaseResult)) -> BenchReport {
    let all = cases(suite);
    let mut results = Vec::with_capacity(all.len());
    for (i, c) in all.iter().enumerate() {
        let r = run_case(c);
        progress(i + 1, all.len(), &r);
        results.push(r);
    }
    let refs: Vec<&CaseResult> = results.iter().collect();
    let overall = aggregate(&refs);
    let mut tiers: BTreeMap<String, Vec<&CaseResult>> = BTreeMap::new();
    for r in &results {
        tiers.entry(r.tier.clone()).or_default().push(r);
    }
    let by_tier = tiers.into_iter().map(|(k, v)| (k, aggregate(&v))).collect();
    BenchReport {
        suite,
        tarafold_version: tarafold_engine::VERSION.into(),
        cases: results,
        aggregate: overall,
        by_tier,
    }
}

/// A metric that got worse than the baseline allows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Regression {
    /// Metric name.
    pub metric: String,
    /// Baseline value.
    pub baseline: f64,
    /// Current value.
    pub current: f64,
    /// Allowed change.
    pub allowed: f64,
}

/// Compare against a baseline: rates may drop by at most 0.03, the median oracle gap may rise by
/// at most 0.01, and no new errors are allowed (R11.4).
pub fn compare(current: &Aggregate, baseline: &Aggregate) -> Vec<Regression> {
    let mut out = Vec::new();
    let mut higher_is_better = |name: &str, c: f64, b: f64, allowed: f64| {
        if b.is_finite() && (!c.is_finite() || c < b - allowed) {
            out.push(Regression {
                metric: name.into(),
                baseline: b,
                current: c,
                allowed,
            });
        }
    };
    higher_is_better("exact_rate", current.exact_rate, baseline.exact_rate, 0.03);
    higher_is_better(
        "structure_rate",
        current.structure_rate,
        baseline.structure_rate,
        0.03,
    );
    higher_is_better(
        "accuracy_rate",
        current.accuracy_rate,
        baseline.accuracy_rate,
        0.03,
    );
    higher_is_better(
        "var_precision",
        current.var_precision,
        baseline.var_precision,
        0.03,
    );
    higher_is_better("var_recall", current.var_recall, baseline.var_recall, 0.03);
    higher_is_better(
        "inverse_success",
        current.inverse_success,
        baseline.inverse_success,
        0.03,
    );
    higher_is_better(
        "median_oracle_gap (negated)",
        -current.median_oracle_gap,
        -baseline.median_oracle_gap,
        0.01,
    );
    higher_is_better(
        "errors (negated)",
        -(current.errors as f64),
        -(baseline.errors as f64),
        0.0,
    );
    out
}

/// Markdown summary.
pub fn markdown(r: &BenchReport) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "## Tarafold bench: {} suite ({} cases)\n",
        r.suite.key(),
        r.cases.len()
    );
    let _ = writeln!(
        s,
        "| dataset | rows | noise | distr. | exact | structure | R² fresh | R² extrap. | inverse | equation |"
    );
    let _ = writeln!(s, "|---|---|---|---|---|---|---|---|---|---|");
    let yn = |b: bool| if b { "yes" } else { "no" };
    for c in &r.cases {
        let mut eq = c
            .error
            .clone()
            .map_or_else(|| c.equation.clone(), |e| format!("ERROR: {e}"));
        if eq.chars().count() > 55 {
            eq = eq.chars().take(52).collect::<String>() + "...";
        }
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} | {} | {:.6} | {:.4} | {}/{} | `{}` |",
            c.case.dataset,
            c.case.rows,
            c.case.noise,
            c.case.distractors,
            yn(c.exact),
            yn(c.structure),
            c.r2_fresh,
            c.r2_extrapolation,
            c.inverse_ok,
            c.inverse_total,
            eq
        );
    }
    let _ = writeln!(
        s,
        "\n| tier | cases | exact | structure | R²>0.999 | var. precision | inverse | median oracle gap |"
    );
    let _ = writeln!(s, "|---|---|---|---|---|---|---|---|");
    let row = |s: &mut String, name: &str, a: &Aggregate| {
        let _ = writeln!(
            s,
            "| {name} | {} | {:.0}% | {:.0}% | {:.0}% | {:.2} | {:.1}% | {:.4} |",
            a.cases,
            100.0 * a.exact_rate,
            100.0 * a.structure_rate,
            100.0 * a.accuracy_rate,
            a.var_precision,
            100.0 * a.inverse_success,
            a.median_oracle_gap
        );
    };
    for (k, a) in &r.by_tier {
        row(&mut s, k, a);
    }
    row(&mut s, "**all**", &r.aggregate);
    let _ = writeln!(
        s,
        "\nMedian extrapolation R² {:.4}; mean 90% coverage on noisy cases {:.3}; median complexity ratio {:.2}; graded high: {:.0}% of correct structures, {:.0}% of wrong ones; {:.1}s total.",
        r.aggregate.median_extrapolation_r2,
        r.aggregate.coverage_90_noisy,
        r.aggregate.median_complexity_ratio,
        100.0 * r.aggregate.high_given_structure,
        100.0 * r.aggregate.high_given_wrong,
        r.aggregate.seconds
    );
    if let Some(g) = r.aggregate.median_weighting_gain {
        let _ = writeln!(
            s,
            "Median weighting gain on noisy T9 cases (unweighted ÷ weighted RMSE against the truth): {g:.2}."
        );
    }
    s
}
