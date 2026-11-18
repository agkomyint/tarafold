//! Pipeline orchestration (design §5): split → screen → discover → validate → optimize →
//! report. Library code only: no printing, no environment access, no sockets.

mod optimize_stage;

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rayon::prelude::*;
use tarafold_core::{DataSource, Dataset, Evidence, Rng, RunConfig, Split, rng::stream, stats};
use tarafold_discover::{DiscoverInput, discover};
use tarafold_expr::Expr;
use tarafold_report::{
    ColumnSummary, DatasetInfo, DiscoverReport, EquationReport, OptimizeReport, Report, RunInfo,
    RunStatus, SCHEMA_VERSION, SplitMetrics, Stability, StageTiming, SuggestReport, TargetReport,
    ValidateReport, Warning, text,
};
use tarafold_validate::reliability;
use tarafold_validate::{Domain, Grade, GradeInputs, evidence, grade, score};

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

/// Engines available in this build (R20).
pub const ENGINES: [&str; 1] = ["classical"];

/// Reject engines this build does not have.
pub fn check_engine(name: &str) -> Result<(), EngineError> {
    if ENGINES.contains(&name) {
        return Ok(());
    }
    Err(EngineError::Config(match name {
        "quantum" | "hybrid" => format!(
            "engine `{name}` is not available in this build: only `classical` is implemented; quantum and hybrid engines are planned (requirement R20)"
        ),
        _ => format!("unknown engine `{name}`; available: classical"),
    }))
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

pub(crate) struct Prepared {
    pub(crate) names: Vec<String>,
    tx: Vec<Vec<f64>>,
    vx: Vec<Vec<f64>>,
    sx: Vec<Vec<f64>>,
    pub(crate) ty: Vec<f64>,
    vy: Vec<f64>,
    sy: Vec<f64>,
}

/// Fitted state for one target, kept for optimization.
pub(crate) struct Fitted {
    pub(crate) report: TargetReport,
    pub(crate) expr: tarafold_expr::Expr,
    pub(crate) domain: Domain,
    pub(crate) prepared: Prepared,
    pub(crate) halfwidth: f64,
    pub(crate) sigma: f64,
    pub(crate) ensemble: Vec<Vec<f64>>,
    pub(crate) evidence: Evidence,
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
    // Other targets are features only if listed earlier (chains follow the given order).
    let pos = cfg.targets.iter().position(|t| t == target).unwrap_or(0);
    let blocked: Vec<&String> = cfg
        .targets
        .iter()
        .enumerate()
        .filter(|(k, _)| cfg.exclude_other_targets || *k >= pos)
        .map(|(_, t)| t)
        .collect();
    let all_features: Vec<usize> = (0..data.columns.len())
        .filter(|&j| j != ti && !blocked.contains(&&data.columns[j].name))
        .collect();
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
    let disc_tw: Option<Vec<f64>> = data.weights.as_ref().map(|w| stats::gather(w, &train_rows));
    let val_w: Option<Vec<f64>> = data.weights.as_ref().map(|w| stats::gather(w, &split.val));

    let out = timed(stages, &format!("discover:{target}"), || {
        discover(&DiscoverInput {
            train_x: &disc_tx,
            train_y: &disc_ty,
            val_x: &prepared.vx,
            val_y: &prepared.vy,
            train_w: disc_tw.as_deref(),
            val_w: val_w.as_deref(),
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

    // Validation of the selected equation (R7).
    let all_names: Vec<String> = all_features
        .iter()
        .map(|&j| data.columns[j].name.clone())
        .collect();
    let rel_corr = rel.report.feature_correlation.clone();
    // With row weights, the weights travel as one extra column after the features, so bootstrap
    // resampling keeps each row's weight; the refit splits it off again.
    let n_feat = prepared.tx.len();
    let refit = move |e: &Expr, x: &[Vec<f64>], y: &[f64]| {
        if x.len() == n_feat + 1 {
            let (fx, w) = x.split_at(n_feat);
            tarafold_discover::refit_constants_w(e, fx, y, w.first().map(Vec::as_slice))
        } else {
            tarafold_discover::refit_constants(e, x, y)
        }
    };
    let (val_pred, test_pred) = (
        score(&sel.expr, &prepared.vx, &prepared.vy).1,
        score(&sel.expr, &prepared.sx, &prepared.sy).1,
    );
    let sigma = {
        let r: Vec<f64> = val_pred
            .iter()
            .zip(&prepared.vy)
            .map(|(p, y)| y - p)
            .collect();
        stats::std(&r)
    };
    // Bootstrap on train ∪ validation rows (capped at 2,000 rows), shared indices across targets.
    let mut fit_x: Vec<Vec<f64>> = prepared
        .tx
        .iter()
        .zip(&prepared.vx)
        .map(|(a, b)| a.iter().chain(b).copied().take(2000).collect())
        .collect();
    if let Some(w) = &data.weights {
        let rows: Vec<u32> = split.train.iter().chain(&split.val).copied().collect();
        fit_x.push(stats::gather(w, &rows).into_iter().take(2000).collect());
    }
    let fit_y: Vec<f64> = prepared
        .ty
        .iter()
        .chain(&prepared.vy)
        .copied()
        .take(2000)
        .collect();
    let boot = timed(stages, &format!("bootstrap:{target}"), || {
        reliability::bootstrap(
            &sel.expr,
            &fit_x,
            &fit_y,
            cfg.validate.bootstrap,
            cfg.seed,
            &refit,
        )
    });
    let half = reliability::interval_halfwidths(&sel.expr, &boot.ensemble, &prepared.sx, sigma);
    let covered = test_pred
        .iter()
        .zip(&prepared.sy)
        .zip(&half)
        .filter(|((p, y), h)| (*p - *y).abs() <= **h)
        .count();
    let coverage = covered as f64 / prepared.sy.len().max(1) as f64;
    let halfwidth = if half.is_empty() {
        1.645 * sigma
    } else {
        stats::median(&half)
    };
    let residuals = reliability::residuals(&test_pred, &prepared.sy, &split.test);

    // Structural stability: rediscover on bootstrap resamples of the training rows with a
    // reduced budget and count how often an equivalent equation comes back.
    // Skipped when the search was stopped early: rediscoveries would stop at once too.
    let stability = (cfg.validate.stability > 0 && !tarafold_core::stop::requested()).then(|| {
        timed(stages, &format!("stability:{target}"), || {
            let mut dcfg = cfg.discover.clone();
            dcfg.gp.max_evals =
                ((dcfg.gp.max_evals as f64) * cfg.validate.stability_budget).max(1000.0) as usize;
            let n = disc_ty.len();
            let check_x: Vec<Vec<f64>> = prepared.vx.clone();
            let results: Vec<Option<String>> = (0..cfg.validate.stability)
                .into_par_iter()
                .map(|r| {
                    let idx = reliability::resample_m(n, n.min(1000), 10_000 + r, cfg.seed);
                    let bx: Vec<Vec<f64>> = disc_tx
                        .iter()
                        .map(|c| idx.iter().map(|&i| c[i]).collect())
                        .collect();
                    let by: Vec<f64> = idx.iter().map(|&i| disc_ty[i]).collect();
                    let bw: Option<Vec<f64>> = disc_tw
                        .as_ref()
                        .map(|w| idx.iter().map(|&i| w[i]).collect());
                    let out = discover(&DiscoverInput {
                        train_x: &bx,
                        train_y: &by,
                        val_x: &prepared.vx,
                        val_y: &prepared.vy,
                        train_w: bw.as_deref(),
                        val_w: val_w.as_deref(),
                        config: &dcfg,
                        seed: cfg.seed.wrapping_add(1 + r as u64),
                    })
                    .ok()?;
                    let e = &out.front[out.selected].expr;
                    reliability::equivalent(&sel.expr, e, &check_x, &refit)
                        .then(|| e.to_infix_pretty(names, 4))
                })
                .collect();
            let same = results.iter().filter(|r| r.is_some()).count();
            Stability {
                resamples: results.len(),
                equivalent: same,
                fraction: same as f64 / results.len().max(1) as f64,
                gp_budget: dcfg.gp.max_evals,
            }
        })
    });

    let test_r2 = sel_report.metrics.test.r2;
    let leak_used = sel_report.variables.iter().any(|v| leakage.contains(v));
    let reliability = grade(&GradeInputs {
        test_r2: if test_r2.is_finite() {
            test_r2
        } else {
            f64::NEG_INFINITY
        },
        best_baseline_r2: best_baseline.is_finite().then_some(best_baseline),
        stability: stability.as_ref().map(|s| s.fraction),
        n_train: prepared.ty.len(),
        n_constants: sel.expr.constants().len(),
        leakage: leak_used,
    });

    // Evidence level (R9.3, R9.4): interventional only for declared designed data where every
    // input being interpreted (controllables, else the equation's variables) is designed.
    let inputs: Vec<String> = if cfg.optimize.controllables.is_empty() {
        sel_report.variables.clone()
    } else {
        cfg.optimize.controllables.clone()
    };
    let designed_set: Vec<String> = match cfg.data_source {
        DataSource::Observational => Vec::new(),
        _ if !cfg.designed.is_empty() => cfg.designed.clone(),
        DataSource::Experimental => cfg.optimize.controllables.clone(),
        DataSource::Simulation => all_names.clone(),
    };
    let designed = !designed_set.is_empty() && inputs.iter().all(|v| designed_set.contains(v));
    let ev = evidence(test_r2, reliability.grade, cfg.data_source, designed);
    if cfg.data_source != DataSource::Observational {
        if !designed {
            warnings.push(Warning {
                code: "inputs_not_designed".into(),
                message: format!(
                    "data source is {:?} but not every interpreted input is a designed column ({}); evidence stays at functional_fit or below. Use --designed.",
                    cfg.data_source,
                    inputs.join(", ")
                ),
                target: Some(target.into()),
            });
        }
        for (i, a) in designed_set.iter().enumerate() {
            for b in &designed_set[i + 1..] {
                let (Some(ia), Some(ib)) = (
                    all_names.iter().position(|n| n == a),
                    all_names.iter().position(|n| n == b),
                ) else {
                    continue;
                };
                let r = rel_corr[ia][ib];
                if r.abs() > 0.5 {
                    warnings.push(Warning {
                        code: "correlated_design".into(),
                        message: format!("designed columns `{a}` and `{b}` are correlated (r = {r:.2}); the design may not vary them independently"),
                        target: Some(target.into()),
                    });
                }
            }
        }
    }
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
    if residuals.heteroscedastic {
        warnings.push(Warning {
            code: "heteroscedastic".into(),
            message: format!(
                "residual size changes with the prediction for `{target}`; intervals may be too narrow in parts of the range ({})",
                if data.weights.is_some() {
                    "the fit uses the row weights, but intervals assume one residual size"
                } else {
                    "consider --weights"
                }
            ),
            target: Some(target.into()),
        });
    }
    if let Some(s) = &stability
        && s.fraction < 0.4
    {
        warnings.push(Warning {
            code: "unstable_structure".into(),
            message: format!(
                "only {}/{} resampled rediscoveries found an equivalent equation for `{target}`; treat its form as tentative",
                s.equivalent, s.resamples
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
            evidence_basis: if ev == Evidence::InterventionalFit {
                "user_declared_design".into()
            } else {
                "observational_fit".into()
            },
            interval_halfwidth_90: halfwidth,
            interval_method: "bootstrap_ensemble_plus_residual_sd".into(),
            test_coverage_90: coverage,
            residual_sd: sigma,
            bootstrap: boot.clone(),
            stability,
            residuals,
            domain: domain.clone(),
        },
    };
    Ok(Fitted {
        report,
        expr: sel.expr.clone(),
        domain,
        prepared,
        halfwidth,
        sigma,
        ensemble: boot.ensemble,
        evidence: ev,
    })
}
/// Optimization and suggestion stages shared by `run` and saved-report reruns.
fn optimize_and_suggest(
    data: &Dataset,
    split: &Split,
    fitted: &[Fitted],
    cfg: &RunConfig,
    stages: &mut Vec<StageTiming>,
    warnings: &mut Vec<Warning>,
    skip_optimize: bool,
) -> Result<(Option<OptimizeReport>, Option<SuggestReport>), EngineError> {
    let multi = !cfg.optimize.require.is_empty() || !cfg.optimize.prefer.is_empty();
    let shared_names: Vec<String> = data
        .columns
        .iter()
        .map(|c| c.name.clone())
        .filter(|n| !cfg.targets.contains(n))
        .collect();
    let shared_cols: Vec<Vec<f64>> = data
        .columns
        .iter()
        .filter(|c| !cfg.targets.contains(&c.name))
        .map(|c| stats::gather(&c.values, &split.train))
        .collect();
    let shared = optimize_stage::Shared {
        domain: Domain::fit(&shared_names, &shared_cols),
        names: shared_names,
        fitted,
    };
    let optimize = if skip_optimize {
        None
    } else if multi {
        if cfg.optimize.goal.is_some() {
            return Err(EngineError::Config(
                "use either --goal (one exact goal) or --require/--prefer (multi-goal), not both"
                    .into(),
            ));
        }
        Some(timed(stages, "optimize", || {
            optimize_stage::multi_goal(&shared, cfg, cfg.seed)
        })?)
    } else {
        match cfg.optimize.goal {
            None => None,
            Some(goal) => {
                if cfg.targets.len() > 1 {
                    warnings.push(Warning {
                        code: "goal_first_target".into(),
                        message: format!(
                            "--goal applies to the first target `{}`; use --require/--prefer for goals on several targets",
                            cfg.targets[0]
                        ),
                        target: None,
                    });
                }
                Some(timed(stages, "optimize", || {
                    optimize_stage::single_goal(&fitted[0], cfg, goal, cfg.seed)
                })?)
            }
        }
    };
    if let Some(o) = &optimize {
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
            let message = match &o.conflict {
                Some(k) => format!(
                    "the goals cannot all be met; conflicting: {}",
                    k.goals.join(", ")
                ),
                None if o.goal.is_finite() => format!(
                    "no candidate reaches {} within tolerance; closest predicted value is {:.6}",
                    o.goal, o.closest_value
                ),
                None => "no candidate satisfies every hard goal and constraint".into(),
            };
            warnings.push(Warning {
                code: "goal_not_reached".into(),
                message,
                target: Some(o.target.clone()),
            });
        }
        for r in &o.redundant_objectives {
            warnings.push(Warning {
                code: "redundant_objectives".into(),
                message: format!(
                    "objectives `{}` and `{}` move together (rank correlation {:.2}); consider turning one into a threshold",
                    r.a, r.b, r.rho
                ),
                target: None,
            });
        }
    }
    let suggest = if cfg.suggest.n > 0 {
        Some(timed(stages, "suggest", || {
            optimize_stage::suggest_stage(&shared, cfg, data, split, cfg.seed)
        })?)
    } else {
        None
    };

    Ok((optimize, suggest))
}

/// Rebuild the fitted state of every target from a saved report and the same data, then run
/// optimization and/or suggestions with new settings, without re-running discovery
/// (`tarafold optimize <report-dir>`, `tarafold suggest <report-dir>`).
pub fn rerun_from_report(
    saved: &Report,
    data: &Dataset,
    optimize: tarafold_core::config::OptimizeConfig,
    suggest: tarafold_core::config::SuggestConfig,
    only_suggest: bool,
) -> Result<(Report, Outcome), EngineError> {
    if saved.dataset.source.sha256 != data.source.sha256 {
        return Err(EngineError::Config(format!(
            "the data does not match the report (SHA-256 {} vs {}); pass the CSV the report was built from",
            data.source.sha256.get(..12).unwrap_or(""),
            saved.dataset.source.sha256.get(..12).unwrap_or("")
        )));
    }
    let mut cfg = saved.run.config.clone();
    cfg.optimize = optimize;
    cfg.suggest = suggest;
    let split = Split::new(
        data.n_rows,
        cfg.split.train,
        cfg.split.val,
        cfg.split.strategy,
        cfg.seed,
    )
    .map_err(|e| EngineError::Config(e.to_string()))?;
    let ti = |name: &str| {
        data.index_of(name)
            .map_err(|_| EngineError::Data(format!("column `{name}` is missing")))
    };
    let mut fitted = Vec::new();
    for t in &saved.targets {
        let names = t.features.clone();
        let cols = |rows: &[u32]| -> Result<Vec<Vec<f64>>, EngineError> {
            names
                .iter()
                .map(|n| Ok(stats::gather(&data.columns[ti(n)?].values, rows)))
                .collect()
        };
        let y = &data.columns[ti(&t.target)?].values;
        let sel = t
            .discover
            .front
            .get(t.discover.selected_id)
            .ok_or_else(|| EngineError::Data("saved report has no selected equation".into()))?;
        let expr = tarafold_expr::parse(&sel.infix_exact, &names).map_err(|e| {
            EngineError::Data(format!(
                "cannot read the saved equation for `{}`: {e}",
                t.target
            ))
        })?;
        let v = &t.validate;
        fitted.push(Fitted {
            report: t.clone(),
            expr,
            domain: v.domain.clone(),
            prepared: Prepared {
                tx: cols(&split.train)?,
                vx: cols(&split.val)?,
                sx: cols(&split.test)?,
                ty: stats::gather(y, &split.train),
                vy: stats::gather(y, &split.val),
                sy: stats::gather(y, &split.test),
                names,
            },
            halfwidth: v.interval_halfwidth_90,
            sigma: v.residual_sd,
            ensemble: v.bootstrap.ensemble.clone(),
            evidence: v.evidence,
        });
    }
    let mut stages = Vec::new();
    let mut warnings = Vec::new();
    let (opt, sug) = optimize_and_suggest(
        data,
        &split,
        &fitted,
        &cfg,
        &mut stages,
        &mut warnings,
        only_suggest,
    )?;
    let mut report = saved.clone();
    report.run.config = cfg;
    report.run.stages.extend(stages);
    report.warnings.retain(|w| {
        !matches!(
            w.code.as_str(),
            "goal_not_reached"
                | "unused_controllables"
                | "redundant_objectives"
                | "goal_first_target"
        )
    });
    report.warnings.extend(warnings);
    if !only_suggest {
        report.optimize = opt;
    }
    report.suggest = sug;
    let outcome = Outcome {
        low_grade: fitted
            .iter()
            .any(|f| f.report.validate.reliability.grade == Grade::Low),
        goal_missed: report.optimize.as_ref().is_some_and(|o| !o.reached),
    };
    Ok((report, outcome))
}

/// Run the pipeline on a loaded dataset.
pub fn run(data: &Dataset, cfg: &RunConfig) -> Result<(Report, Outcome), EngineError> {
    check_engine(&cfg.engine)?;
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

    let (optimize, suggest) = optimize_and_suggest(
        data,
        &split,
        &fitted,
        cfg,
        &mut stages,
        &mut warnings,
        false,
    )?;

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
    if cfg.validate.stability == 0 {
        notes.push(text::STABILITY_PENDING.into());
    }
    if tarafold_core::stop::interrupted() {
        notes.push(text::INTERRUPTED_NOTE.into());
    } else if tarafold_core::stop::deadline_hit() {
        notes.push(text::TIME_BUDGET_NOTE.into());
    }
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
            status: if tarafold_core::stop::interrupted() {
                RunStatus::Interrupted
            } else {
                RunStatus::Completed
            },
            engine: cfg.engine.clone(),
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
        suggest,
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
    fn evidence_depends_on_declared_design() {
        let data = load("business-revenue", 600, 0.0);
        let base = || {
            let mut c = RunConfig {
                targets: vec!["revenue".into()],
                ..Default::default()
            };
            c.optimize.controllables = vec!["orders".into(), "price".into()];
            c.validate.stability = 3;
            c
        };
        let ev = |cfg: &RunConfig| run(&data, cfg).unwrap().0.targets[0].validate.evidence;
        assert_eq!(ev(&base()), Evidence::FunctionalFit);
        let mut sim = base();
        sim.data_source = DataSource::Simulation;
        assert_eq!(ev(&sim), Evidence::InterventionalFit);
        let mut exp = base();
        exp.data_source = DataSource::Experimental;
        exp.designed = vec!["price".into()]; // `orders` was observed, not set
        let (report, _) = run(&data, &exp).unwrap();
        assert_eq!(report.targets[0].validate.evidence, Evidence::FunctionalFit);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.code == "inputs_not_designed")
        );
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
