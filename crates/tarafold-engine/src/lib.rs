//! Pipeline orchestration (design §5): split → screen → discover → validate → optimize →
//! report. Library code only: no printing, no environment access, no sockets.

use std::collections::BTreeMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tarafold_core::{DataSource, Dataset, Evidence, Rng, RunConfig, Split, rng::stream, stats};
use tarafold_discover::{DiscoverInput, discover};
use tarafold_optimize::{Problem, optimize};
use tarafold_report::{
    CandidateReport, ColumnSummary, DatasetInfo, DiscoverReport, EquationReport, OptimizeReport,
    Report, RunInfo, RunStatus, SCHEMA_VERSION, SplitMetrics, StageTiming, TargetReport,
    ValidateReport, Warning, text,
};
use tarafold_validate::{Domain, Grade, GradeInputs, evidence, grade, residual_halfwidth, score};

/// Pipeline failures. The CLI maps these to exit codes.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// Invalid usage or configuration (exit code 2).
    #[error("{0}")]
    Config(String),
    /// The data cannot support the requested analysis (exit code 3).
    #[error("{0}")]
    Data(String),
}

/// Outcome flags used for exit codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// Some target's selected equation was graded `low`.
    pub low_grade: bool,
    /// Optimization ran and did not reach the goal.
    pub goal_missed: bool,
}

/// Tool version recorded in reports.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn summarize(d: &Dataset) -> Vec<ColumnSummary> {
    d.columns
        .iter()
        .map(|c| ColumnSummary {
            name: c.name.clone(),
            min: c.values.iter().copied().fold(f64::INFINITY, f64::min),
            max: c.values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            mean: stats::mean(&c.values),
            std: stats::std(&c.values),
            median: stats::median(&c.values),
        })
        .collect()
}

struct Prepared {
    names: Vec<String>,
    tx: Vec<Vec<f64>>,
    vx: Vec<Vec<f64>>,
    sx: Vec<Vec<f64>>,
    ty: Vec<f64>,
    vy: Vec<f64>,
    sy: Vec<f64>,
}

/// Fitted state for one target, kept for optimization.
struct Fitted {
    report: TargetReport,
    expr: tarafold_expr::Expr,
    domain: Domain,
    prepared: Prepared,
    halfwidth: f64,
    evidence: Evidence,
}

fn timed<T>(stages: &mut Vec<StageTiming>, name: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    stages.push(StageTiming {
        name: name.into(),
        millis: t.elapsed().as_millis() as u64,
    });
    out
}

/// Relationship analysis of `target` against `features` with baseline test R² filled in.
/// Returns the analysis and the best baseline test R² (−∞ when none is finite).
fn relate_features(
    data: &Dataset,
    split: &Split,
    features: &[usize],
    y: &[f64],
    cfg: &RunConfig,
    target: &str,
    stages: &mut Vec<StageTiming>,
) -> (tarafold_relate::Relate, f64) {
    let names: Vec<String> = features
        .iter()
        .map(|&j| data.columns[j].name.clone())
        .collect();
    let cols = |rows: &[u32]| -> Vec<Vec<f64>> {
        features
            .iter()
            .map(|&j| stats::gather(&data.columns[j].values, rows))
            .collect()
    };
    let (tx, vx, sx) = (cols(&split.train), cols(&split.val), cols(&split.test));
    let (ty, vy, sy) = (
        stats::gather(y, &split.train),
        stats::gather(y, &split.val),
        stats::gather(y, &split.test),
    );
    let mut rel = timed(stages, &format!("relate:{target}"), || {
        tarafold_relate::analyze(&tarafold_relate::RelateInput {
            names: &names,
            train_x: &tx,
            train_y: &ty,
            val_x: &vx,
            val_y: &vy,
            seed: cfg.seed,
        })
    });
    let ridge_test = rel
        .ridge
        .as_ref()
        .map(|r| tarafold_validate::metrics(&r.predict(&sx), &sy).r2);
    let gbdt_test = tarafold_validate::metrics(&rel.gbdt.predict(&sx), &sy).r2;
    for b in &mut rel.report.baselines {
        b.test_r2 = if b.model == "ridge" {
            ridge_test
        } else {
            Some(gbdt_test)
        };
    }
    let best = rel
        .report
        .baselines
        .iter()
        .filter_map(|b| b.test_r2)
        .filter(|v| v.is_finite())
        .fold(f64::NEG_INFINITY, f64::max);
    (rel, best)
}

/// Relationship analysis and screening for one target without equation discovery
/// (`tarafold relate`).
pub fn relate(
    data: &Dataset,
    cfg: &RunConfig,
    target: &str,
) -> Result<
    (
        tarafold_relate::RelateReport,
        Vec<tarafold_relate::ScreenDecision>,
    ),
    EngineError,
> {
    let ti = data.index_of(target).map_err(|_| {
        EngineError::Config(format!("target `{target}` is not a usable numeric column"))
    })?;
    let features: Vec<usize> = (0..data.columns.len()).filter(|&j| j != ti).collect();
    if features.is_empty() {
        return Err(EngineError::Data(
            "no feature columns besides the target".into(),
        ));
    }
    let split = Split::new(
        data.n_rows,
        cfg.split.train,
        cfg.split.val,
        cfg.split.strategy,
        cfg.seed,
    )
    .map_err(|e| EngineError::Config(e.to_string()))?;
    let (rel, _) = relate_features(
        data,
        &split,
        &features,
        &data.columns[ti].values,
        cfg,
        target,
        &mut Vec::new(),
    );
    let screening = tarafold_relate::screen(
        &rel.report,
        cfg.discover.max_features.max(1),
        &cfg.optimize.controllables,
    );
    Ok((rel.report, screening))
}

fn fit_target(
    data: &Dataset,
    split: &Split,
    target: &str,
    cfg: &RunConfig,
    warnings: &mut Vec<Warning>,
    stages: &mut Vec<StageTiming>,
) -> Result<Fitted, EngineError> {
    let ti = data.index_of(target).map_err(|_| {
        EngineError::Config(format!("target `{target}` is not a usable numeric column"))
    })?;
    let y = &data.columns[ti].values;
    let ty_full = stats::gather(y, &split.train);
    let all_features: Vec<usize> = (0..data.columns.len()).filter(|&j| j != ti).collect();
    if all_features.is_empty() {
        return Err(EngineError::Data(
            "no feature columns besides the target".into(),
        ));
    }

    let max_f = cfg.discover.max_features.max(1);
    let (rel, best_baseline) = relate_features(data, split, &all_features, y, cfg, target, stages);
    let screening = tarafold_relate::screen(&rel.report, max_f, &cfg.optimize.controllables);
    let mut kept: Vec<usize> = screening
        .iter()
        .zip(&all_features)
        .filter(|(d, _)| d.kept)
        .map(|(_, &j)| j)
        .collect();
    if kept.is_empty() {
        kept = all_features.iter().copied().take(max_f).collect();
    }
    let dropped = all_features.len() - kept.len();
    if dropped > 0 {
        warnings.push(Warning {
            code: "features_screened".into(),
            message: format!(
                "{dropped} of {} features were not passed to equation discovery (see screening)",
                all_features.len()
            ),
            target: Some(target.into()),
        });
    }
    let leakage: Vec<String> = rel.report.leakage.clone();
    // Subsample training rows for discovery.
    let mut train_rows = split.train.clone();
    if train_rows.len() > cfg.discover.max_rows {
        Rng::derive(cfg.seed, &[stream::SUBSAMPLE]).shuffle(&mut train_rows);
        train_rows.truncate(cfg.discover.max_rows);
        train_rows.sort_unstable();
        warnings.push(Warning {
            code: "subsampled".into(),
            message: format!(
                "discovery used {} of {} training rows",
                train_rows.len(),
                split.train.len()
            ),
            target: Some(target.into()),
        });
    }
    let cols = |rows: &[u32]| -> Vec<Vec<f64>> {
        kept.iter()
            .map(|&j| stats::gather(&data.columns[j].values, rows))
            .collect()
    };
    let prepared = Prepared {
        names: kept.iter().map(|&j| data.columns[j].name.clone()).collect(),
        tx: cols(&split.train),
        vx: cols(&split.val),
        sx: cols(&split.test),
        ty: ty_full.clone(),
        vy: stats::gather(y, &split.val),
        sy: stats::gather(y, &split.test),
    };
    let disc_tx = cols(&train_rows);
    let disc_ty = stats::gather(y, &train_rows);

    let out = timed(stages, &format!("discover:{target}"), || {
        discover(&DiscoverInput {
            train_x: &disc_tx,
            train_y: &disc_ty,
            val_x: &prepared.vx,
            val_y: &prepared.vy,
            config: &cfg.discover,
            seed: cfg.seed,
        })
    })
    .map_err(|e| EngineError::Data(e.to_string()))?;

    let names = &prepared.names;
    let front: Vec<EquationReport> = out
        .front
        .iter()
        .enumerate()
        .map(|(id, c)| EquationReport {
            id,
            infix: c.expr.to_infix_pretty(names, 4),
            infix_exact: c.expr.to_infix(names),
            latex: c.expr.to_latex(names, 4),
            tree: c.expr.to_tree(names),
            complexity: c.complexity,
            constants: c.expr.constants(),
            variables: c
                .expr
                .variables()
                .iter()
                .map(|&i| names[i as usize].clone())
                .collect(),
            strategy: c.strategy,
            metrics: SplitMetrics {
                train: score(&c.expr, &prepared.tx, &prepared.ty).0,
                val: score(&c.expr, &prepared.vx, &prepared.vy).0,
                test: score(&c.expr, &prepared.sx, &prepared.sy).0,
            },
        })
        .collect();
    let sel = &out.front[out.selected];
    let sel_report = &front[out.selected];

    // Validation of the selected equation.
    let (val_pred, test_pred) = timed(stages, &format!("validate:{target}"), || {
        (
            score(&sel.expr, &prepared.vx, &prepared.vy).1,
            score(&sel.expr, &prepared.sx, &prepared.sy).1,
        )
    });
    let halfwidth = residual_halfwidth(&val_pred, &prepared.vy, 0.9);
    let covered = test_pred
        .iter()
        .zip(&prepared.sy)
        .filter(|(p, y)| (*p - *y).abs() <= halfwidth)
        .count();
    let coverage = covered as f64 / prepared.sy.len().max(1) as f64;
    let test_r2 = sel_report.metrics.test.r2;
    let leak_used = sel_report.variables.iter().any(|v| leakage.contains(v));
    let reliability = grade(&GradeInputs {
        test_r2: if test_r2.is_finite() {
            test_r2
        } else {
            f64::NEG_INFINITY
        },
        best_baseline_r2: best_baseline.is_finite().then_some(best_baseline),
        stability: None,
        n_train: prepared.ty.len(),
        n_constants: sel.expr.constants().len(),
        leakage: leak_used,
    });
    let designed = cfg.data_source != DataSource::Observational;
    let ev = evidence(test_r2, reliability.grade, cfg.data_source, designed);
    if leak_used {
        warnings.push(Warning {
            code: "leakage_suspect".into(),
            message: format!("the selected equation uses a variable with |r| > 0.999 to `{target}`; check for target leakage"),
            target: Some(target.into()),
        });
    }
    if reliability.grade == Grade::Low {
        warnings.push(Warning {
            code: "low_reliability".into(),
            message: format!(
                "the selected equation for `{target}` is graded low (test R² = {test_r2:.3})"
            ),
            target: Some(target.into()),
        });
    }
    let domain = Domain::fit(names, &prepared.tx);
    let report = TargetReport {
        target: target.into(),
        features: names.clone(),
        relate: rel.report,
        screening,
        discover: DiscoverReport {
            front,
            selected_id: out.selected,
            strategies: out.strategies.clone(),
            evaluated: out.evaluated,
            rows_used: disc_ty.len(),
        },
        validate: ValidateReport {
            reliability,
            evidence: ev,
            evidence_basis: if designed && ev == Evidence::InterventionalFit {
                "user_declared_design".into()
            } else {
                "observational_fit".into()
            },
            interval_halfwidth_90: halfwidth,
            interval_method: "validation_residual_quantile".into(),
            test_coverage_90: coverage,
            domain: domain.clone(),
        },
    };
    Ok(Fitted {
        report,
        expr: sel.expr.clone(),
        domain,
        prepared,
        halfwidth,
        evidence: ev,
    })
}

fn run_optimize(
    f: &Fitted,
    cfg: &RunConfig,
    goal: f64,
    seed: u64,
) -> Result<OptimizeReport, EngineError> {
    let names = &f.prepared.names;
    let oc = &cfg.optimize;
    let idx = |n: &str| -> Result<usize, EngineError> {
        names.iter().position(|m| m == n).ok_or_else(|| {
            EngineError::Config(format!(
                "`{n}` is not a feature used for `{}` (features: {})",
                f.report.target,
                names.join(", ")
            ))
        })
    };
    let controllables: Vec<usize> = if oc.controllables.is_empty() {
        f.expr.variables().iter().map(|&v| v as usize).collect()
    } else {
        oc.controllables
            .iter()
            .map(|n| idx(n))
            .collect::<Result<_, _>>()?
    };
    if controllables.is_empty() {
        return Err(EngineError::Config(
            "no controllable features; pass --control".into(),
        ));
    }
    let mut baseline = f.domain.median.clone();
    for (n, v) in &oc.fixed {
        baseline[idx(n)?] = *v;
    }
    let mut bounds = Vec::new();
    for &j in &controllables {
        let b = match oc.bounds.get(&names[j]) {
            Some(&[lo, hi]) if lo <= hi && lo.is_finite() && hi.is_finite() => [lo, hi],
            Some(b) => {
                return Err(EngineError::Config(format!(
                    "invalid bounds for `{}`: {b:?}",
                    names[j]
                )));
            }
            None => [f.domain.min[j], f.domain.max[j]],
        };
        bounds.push(b);
    }
    for n in oc.bounds.keys() {
        if !controllables.iter().any(|&j| &names[j] == n) {
            return Err(EngineError::Config(format!(
                "bounds given for `{n}`, which is not a controllable"
            )));
        }
    }
    let y = &f.prepared.ty;
    let y_range = y.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - y.iter().copied().fold(f64::INFINITY, f64::min);
    let tolerance = oc.tolerance * y_range.abs().max(1e-12);
    let out = optimize(&Problem {
        expr: &f.expr,
        controllables: controllables.clone(),
        bounds: bounds.clone(),
        baseline: baseline.clone(),
        scale: f.domain.std.clone(),
        goal,
        y_scale: stats::std(y),
        tolerance,
        domain: &f.domain,
        n_candidates: oc.n_candidates.max(1),
        max_evals: oc.max_evals,
        halfwidth: f.halfwidth,
        seed,
    });
    let cnames: Vec<String> = controllables.iter().map(|&j| names[j].clone()).collect();
    let by_ctrl = |vals: &[f64]| -> BTreeMap<String, f64> {
        cnames.iter().cloned().zip(vals.iter().copied()).collect()
    };
    let candidates = out
        .candidates
        .iter()
        .enumerate()
        .map(|(i, c)| CandidateReport {
            id: i + 1,
            inputs: controllables
                .iter()
                .map(|&j| (names[j].clone(), c.inputs[j]))
                .collect(),
            predicted: c.predicted,
            interval: c.interval,
            error: c.error,
            within_tolerance: c.within_tolerance,
            in_domain: c.domain.in_domain,
            domain_distance: c.domain.distance,
            change: by_ctrl(&c.change),
            change_pct: cnames
                .iter()
                .cloned()
                .zip(c.change_pct.iter().copied())
                .collect(),
            sensitivity: by_ctrl(&c.sensitivity),
            elasticity: by_ctrl(&c.elasticity),
        })
        .collect();
    let binding = out
        .binding
        .iter()
        .filter_map(|b| {
            let (j, side) = b.split_once(':')?;
            Some(format!("{}:{side}", names.get(j.parse::<usize>().ok()?)?))
        })
        .collect();
    let interventional = f.evidence == Evidence::InterventionalFit;
    Ok(OptimizeReport {
        target: f.report.target.clone(),
        goal,
        tolerance,
        controllables: cnames.clone(),
        bounds: cnames.iter().cloned().zip(bounds).collect(),
        baseline: names.iter().cloned().zip(baseline).collect(),
        candidates,
        reached: out.reached,
        closest_value: out.closest_value,
        binding,
        unused_controllables: out
            .unused_controllables
            .iter()
            .map(|&j| names[j].clone())
            .collect(),
        evaluations: out.evaluations,
        evidence: f.evidence,
        disclaimer: if interventional {
            text::INTERVENTIONAL_OPTIMIZE_DISCLAIMER.into()
        } else {
            text::OBSERVATIONAL_OPTIMIZE_DISCLAIMER.into()
        },
    })
}

/// Run the pipeline on a loaded dataset.
pub fn run(data: &Dataset, cfg: &RunConfig) -> Result<(Report, Outcome), EngineError> {
    if cfg.targets.is_empty() {
        return Err(EngineError::Config("no target given; pass --target".into()));
    }
    if let Some(g) = cfg.optimize.goal
        && !g.is_finite()
    {
        return Err(EngineError::Config("goal must be a finite number".into()));
    }
    tarafold_discover::parse_operators(&cfg.discover.gp.operators).map_err(EngineError::Config)?;
    let started_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut stages = Vec::new();
    let mut warnings = Vec::new();
    for e in &data.source.excluded {
        warnings.push(Warning {
            code: "column_excluded".into(),
            message: format!("column `{}` was not used ({})", e.name, e.reason),
            target: None,
        });
    }
    let split = timed(&mut stages, "split", || {
        Split::new(
            data.n_rows,
            cfg.split.train,
            cfg.split.val,
            cfg.split.strategy,
            cfg.seed,
        )
    })
    .map_err(|e| EngineError::Config(e.to_string()))?;
    if split.val.len() < 5 || split.test.len() < 5 {
        return Err(EngineError::Data(
            "too few rows for validation and test splits".into(),
        ));
    }

    let profile = timed(&mut stages, "profile", || tarafold_profile::profile(data));
    let mut fitted = Vec::new();
    for t in &cfg.targets {
        fitted.push(fit_target(
            data,
            &split,
            t,
            cfg,
            &mut warnings,
            &mut stages,
        )?);
    }

    let optimize = match cfg.optimize.goal {
        None => None,
        Some(goal) => {
            if cfg.targets.len() > 1 {
                warnings.push(Warning {
                    code: "goal_first_target".into(),
                    message: format!("the goal applies to the first target `{}`; joint multi-goal optimization arrives in a later version", cfg.targets[0]),
                    target: None,
                });
            }
            let o = timed(&mut stages, "optimize", || {
                run_optimize(&fitted[0], cfg, goal, cfg.seed)
            })?;
            if !o.unused_controllables.is_empty() {
                warnings.push(Warning {
                    code: "unused_controllables".into(),
                    message: format!(
                        "the selected equation does not use {}; they were left at baseline",
                        o.unused_controllables.join(", ")
                    ),
                    target: Some(o.target.clone()),
                });
            }
            if !o.reached {
                warnings.push(Warning {
                    code: "goal_not_reached".into(),
                    message: format!("no candidate reaches {} within tolerance; closest predicted value is {:.6}", o.goal, o.closest_value),
                    target: Some(o.target.clone()),
                });
            }
            Some(o)
        }
    };

    let outcome = Outcome {
        low_grade: fitted
            .iter()
            .any(|f| f.report.validate.reliability.grade == Grade::Low),
        goal_missed: optimize.as_ref().is_some_and(|o| !o.reached),
    };
    let interventional = fitted
        .iter()
        .any(|f| f.evidence == Evidence::InterventionalFit);
    let mut notes = vec![
        if interventional {
            text::INTERVENTIONAL_FIT_NOTE
        } else {
            text::OBSERVATIONAL_FIT_NOTE
        }
        .to_string(),
    ];
    notes.push(text::STABILITY_PENDING.into());
    let report = Report {
        schema_version: SCHEMA_VERSION.into(),
        tarafold_version: VERSION.into(),
        run: RunInfo {
            id: format!(
                "{}-{}",
                data.source.sha256.get(..12).unwrap_or("nohash"),
                cfg.seed
            ),
            started_unix,
            status: RunStatus::Completed,
            engine: "classical".into(),
            config: cfg.clone(),
            stages,
        },
        dataset: DatasetInfo {
            source: data.source.clone(),
            n_rows: data.n_rows,
            columns: summarize(data),
            profile,
            data_source: cfg.data_source,
            weighted: data.weights.is_some(),
        },
        targets: fitted.into_iter().map(|f| f.report).collect(),
        optimize,
        warnings,
        notes,
    };
    Ok((report, outcome))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tarafold_io::{IngestOptions, read_csv};
    use tarafold_synth::{find, generate};

    fn load(name: &str, rows: usize, noise: f64) -> Dataset {
        let g = generate(&find(name).unwrap(), rows, noise, 0, 1).unwrap();
        read_csv(
            g.to_csv().unwrap().as_bytes(),
            name,
            &IngestOptions::default(),
        )
        .unwrap()
    }

    #[test]
    fn end_to_end_business_revenue() {
        let data = load("business-revenue", 800, 0.0);
        let mut cfg = RunConfig {
            targets: vec!["revenue".into()],
            ..Default::default()
        };
        cfg.optimize.goal = Some(50_000.0);
        cfg.optimize.controllables = vec!["orders".into(), "price".into()];
        let (report, outcome) = run(&data, &cfg).unwrap();
        let t = &report.targets[0];
        let sel = &t.discover.front[t.discover.selected_id];
        assert_eq!(sel.infix, "price * orders");
        assert!(sel.metrics.test.r2 > 0.999_999);
        let o = report.optimize.as_ref().unwrap();
        assert!(o.reached && !outcome.goal_missed);
        // Check candidates against the TRUE function revenue = orders * price.
        for c in o.candidates.iter().filter(|c| c.within_tolerance) {
            let truth = c.inputs["orders"] * c.inputs["price"];
            assert!((truth - 50_000.0).abs() <= o.tolerance, "{truth}");
        }
        assert!(
            report
                .to_json()
                .unwrap()
                .contains("\"schema_version\": \"0.1.0\"")
        );
    }

    #[test]
    fn deterministic_reports() {
        let data = load("cobb-douglas", 500, 0.05);
        let mut cfg = RunConfig {
            targets: vec!["output".into()],
            ..Default::default()
        };
        cfg.optimize.goal = Some(100.0);
        let strip = |mut r: Report| {
            r.run.started_unix = 0;
            r.run.stages.clear();
            r
        };
        let a = strip(run(&data, &cfg).unwrap().0);
        let b = strip(run(&data, &cfg).unwrap().0);
        assert_eq!(a.to_json().unwrap(), b.to_json().unwrap());
    }

    #[test]
    fn config_errors() {
        let data = load("linear", 200, 0.0);
        let cfg = RunConfig {
            targets: vec!["nope".into()],
            ..Default::default()
        };
        assert!(matches!(run(&data, &cfg), Err(EngineError::Config(_))));
        let mut cfg = RunConfig {
            targets: vec!["y".into()],
            ..Default::default()
        };
        cfg.optimize.goal = Some(1.0);
        cfg.optimize.controllables = vec!["zzz".into()];
        assert!(matches!(run(&data, &cfg), Err(EngineError::Config(_))));
    }
}
