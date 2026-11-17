//! Optimization stage: single-goal (`--goal`) and multi-goal (`--require` / `--prefer`).

use std::collections::BTreeMap;

use tarafold_core::{Evidence, RunConfig, stats};
use tarafold_expr::Expr;
use tarafold_optimize::multi::{Goal, Model, MultiProblem, Priority, optimize_goals};
use tarafold_optimize::spec::{GoalSpec, Kind, Linear, parse_constraint, parse_goal};
use tarafold_optimize::{Problem, optimize};
use tarafold_report::{CandidateReport, ConflictReport, OptimizeReport, RedundantPair, text};
use tarafold_validate::{Domain, reliability};

use crate::{EngineError, Fitted};

/// Bounds, kinds, constraints and baseline for a set of controllables over `names`.
pub(crate) struct InputSpace {
    pub bounds: Vec<[f64; 2]>,
    pub kinds: Vec<Kind>,
    pub constraints: Vec<Linear>,
    pub baseline: Vec<f64>,
}

pub(crate) fn input_space(
    names: &[String],
    controllables: &[usize],
    cfg: &RunConfig,
    domain: &Domain,
) -> Result<InputSpace, EngineError> {
    let oc = &cfg.optimize;
    let idx = |n: &str| -> Result<usize, EngineError> {
        names
            .iter()
            .position(|m| m == n)
            .ok_or_else(|| EngineError::Config(format!("`{n}` is not an available feature")))
    };
    let is_ctrl = |n: &str| controllables.iter().any(|&j| names[j] == n);
    let mut baseline = domain.median.clone();
    for (n, v) in &oc.fixed {
        baseline[idx(n)?] = *v;
    }
    let mut bounds = Vec::new();
    for &j in controllables {
        let mut b = match oc.bounds.get(&names[j]) {
            Some(&[lo, hi]) if lo <= hi && lo.is_finite() && hi.is_finite() => [lo, hi],
            Some(b) => {
                return Err(EngineError::Config(format!(
                    "invalid bounds for `{}`: {b:?}",
                    names[j]
                )));
            }
            None => [domain.min[j], domain.max[j]],
        };
        if let Some(&frac) = oc.max_change.get(&names[j]) {
            let base = baseline[j];
            let d = frac.abs() * base.abs();
            b = [b[0].max(base - d), b[1].min(base + d)];
            if b[0] > b[1] {
                return Err(EngineError::Config(format!(
                    "--max-change for `{}` leaves no values inside its bounds",
                    names[j]
                )));
            }
        }
        bounds.push(b);
    }
    for n in oc
        .bounds
        .keys()
        .chain(oc.max_change.keys())
        .chain(oc.integer.iter())
        .chain(oc.steps.keys())
    {
        if !is_ctrl(n) {
            return Err(EngineError::Config(format!(
                "`{n}` has bounds or an input kind but is not a controllable"
            )));
        }
    }
    let mut kinds = Vec::new();
    for &j in controllables {
        let n = &names[j];
        let k = match (oc.integer.contains(n), oc.steps.get(n)) {
            (true, Some(_)) => {
                return Err(EngineError::Config(format!(
                    "`{n}` cannot be both --integer and --step"
                )));
            }
            (true, None) => Kind::Integer,
            (false, Some(&s)) if s > 0.0 && s.is_finite() => Kind::Step(s),
            (false, Some(s)) => {
                return Err(EngineError::Config(format!("invalid step {s} for `{n}`")));
            }
            (false, None) => Kind::Continuous,
        };
        kinds.push(k);
    }
    let constraints = oc
        .constraints
        .iter()
        .map(|t| parse_constraint(t, names).map_err(EngineError::Config))
        .collect::<Result<Vec<_>, _>>()?;
    for c in &constraints {
        if let Some(&(j, _)) = c.coefs.iter().find(|(j, _)| !controllables.contains(j)) {
            return Err(EngineError::Config(format!(
                "constraint `{}` uses `{}`, which is not a controllable",
                c.text, names[j]
            )));
        }
    }
    Ok(InputSpace {
        bounds,
        kinds,
        constraints,
        baseline,
    })
}

fn disclaimer(ev: Evidence) -> String {
    if ev == Evidence::InterventionalFit {
        text::INTERVENTIONAL_OPTIMIZE_DISCLAIMER.into()
    } else {
        text::OBSERVATIONAL_OPTIMIZE_DISCLAIMER.into()
    }
}

/// Single exact goal on one target (`--goal`).
pub(crate) fn single_goal(
    f: &Fitted,
    cfg: &RunConfig,
    goal: f64,
    seed: u64,
) -> Result<OptimizeReport, EngineError> {
    let names = &f.prepared.names;
    let oc = &cfg.optimize;
    let controllables: Vec<usize> = if oc.controllables.is_empty() {
        f.expr.variables().iter().map(|&v| v as usize).collect()
    } else {
        oc.controllables
            .iter()
            .map(|n| {
                names.iter().position(|m| m == n).ok_or_else(|| {
                    EngineError::Config(format!(
                        "`{n}` is not a feature used for `{}` (features: {})",
                        f.report.target,
                        names.join(", ")
                    ))
                })
            })
            .collect::<Result<_, _>>()?
    };
    if controllables.is_empty() {
        return Err(EngineError::Config(
            "no controllable features; pass --control".into(),
        ));
    }
    let space = input_space(names, &controllables, cfg, &f.domain)?;
    let y = &f.prepared.ty;
    let y_range = y.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - y.iter().copied().fold(f64::INFINITY, f64::min);
    let tolerance = oc.tolerance * y_range.abs().max(1e-12);
    let out = optimize(&Problem {
        expr: &f.expr,
        controllables: controllables.clone(),
        bounds: space.bounds.clone(),
        baseline: space.baseline.clone(),
        scale: f.domain.std.clone(),
        goal,
        y_scale: stats::std(y),
        tolerance,
        domain: &f.domain,
        n_candidates: oc.n_candidates.max(1),
        max_evals: oc.max_evals,
        halfwidth: f.halfwidth,
        constraints: space.constraints.clone(),
        kinds: space.kinds.clone(),
        seed,
    });
    let cnames: Vec<String> = controllables.iter().map(|&j| names[j].clone()).collect();
    let by_ctrl = |vals: &[f64]| -> BTreeMap<String, f64> {
        cnames.iter().cloned().zip(vals.iter().copied()).collect()
    };
    let members: Vec<Expr> = f
        .ensemble
        .iter()
        .map(|c| f.expr.with_constants(c))
        .collect();
    let candidates = out
        .candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let hits = members
                .iter()
                .filter(|m| (m.eval_point(&c.inputs) - goal).abs() <= tolerance)
                .count();
            CandidateReport {
                id: i + 1,
                inputs: controllables
                    .iter()
                    .map(|&j| (names[j].clone(), c.inputs[j]))
                    .collect(),
                predicted: c.predicted,
                interval: reliability::prediction_interval(
                    &f.expr,
                    &f.ensemble,
                    &c.inputs,
                    f.sigma,
                ),
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
                predictions: [(f.report.target.clone(), c.predicted)]
                    .into_iter()
                    .collect(),
                goals_met: vec![c.within_tolerance],
                joint_probability: if members.is_empty() {
                    f64::NAN
                } else {
                    hits as f64 / members.len() as f64
                },
                marks: if i == 0 {
                    vec!["best".into()]
                } else {
                    Vec::new()
                },
            }
        })
        .collect();
    let binding = out
        .binding
        .iter()
        .filter_map(|b| {
            let (j, side) = b.split_once(':')?;
            if j == "constraint" {
                let k: usize = side.parse().ok()?;
                return Some(format!("constraint: {}", space.constraints.get(k)?.text));
            }
            Some(format!("{}:{side}", names.get(j.parse::<usize>().ok()?)?))
        })
        .collect();
    let gspec = GoalSpec {
        text: format!("{}={goal}", f.report.target),
        target: f.report.target.clone(),
        kind: tarafold_optimize::spec::GoalKind::Exact(goal),
        hard: true,
        weight: 1.0,
    };
    Ok(OptimizeReport {
        target: f.report.target.clone(),
        goal,
        tolerance,
        goals: vec![gspec],
        method: if space.constraints.is_empty() && space.kinds.iter().all(|k| k.step().is_none()) {
            "root finding / differential evolution + Newton projection".into()
        } else {
            "differential evolution + pattern search (constraints or discrete inputs)".into()
        },
        controllables: cnames.clone(),
        bounds: cnames.iter().cloned().zip(space.bounds).collect(),
        baseline: names.iter().cloned().zip(space.baseline).collect(),
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
        conflict: None,
        redundant_objectives: Vec::new(),
        disclaimer: disclaimer(f.evidence),
    })
}

/// Everything the multi-goal stage needs about the shared feature space.
pub(crate) struct Shared<'a> {
    /// Non-target column names (the shared feature space).
    pub names: Vec<String>,
    /// Domain over those columns (training rows).
    pub domain: Domain,
    /// Fitted targets in `cfg.targets` order.
    pub fitted: &'a [Fitted],
}

/// Compose target `t` (and recursively the targets it uses) into an expression over the shared
/// space. `member` selects bootstrap member `b` instead of the point equation.
fn compose(
    t: usize,
    shared: &Shared<'_>,
    member: Option<usize>,
    stack: &mut Vec<usize>,
) -> Result<Expr, EngineError> {
    let f = &shared.fitted[t];
    if stack.contains(&t) {
        let mut cycle: Vec<String> = stack
            .iter()
            .map(|&k| shared.fitted[k].report.target.clone())
            .collect();
        cycle.push(f.report.target.clone());
        return Err(EngineError::Config(format!(
            "targets depend on each other in a cycle: {}",
            cycle.join(" → ")
        )));
    }
    stack.push(t);
    let g = shared.names.len();
    let tnames: Vec<&String> = shared.fitted.iter().map(|x| &x.report.target).collect();
    let map: Vec<u16> = f
        .prepared
        .names
        .iter()
        .map(|n| {
            shared
                .names
                .iter()
                .position(|m| m == n)
                .or_else(|| tnames.iter().position(|m| *m == n).map(|k| g + k))
                .unwrap_or(usize::from(u16::MAX)) as u16
        })
        .collect();
    let base = match member {
        Some(b) if b < f.ensemble.len() => f.expr.with_constants(&f.ensemble[b]),
        _ => f.expr.clone(),
    };
    let mut e = base.remap_vars(&map);
    for v in e.variables() {
        let v = usize::from(v);
        if v >= g && v - g < shared.fitted.len() {
            let sub = compose(v - g, shared, member, stack)?;
            e = e.substitute(v as u16, &sub);
        }
    }
    stack.pop();
    Ok(e)
}

/// Multi-goal optimization over the targets named in `--require` / `--prefer` (R18).
pub(crate) fn multi_goal(
    shared: &Shared<'_>,
    cfg: &RunConfig,
    seed: u64,
) -> Result<OptimizeReport, EngineError> {
    let oc = &cfg.optimize;
    let mut specs: Vec<GoalSpec> = Vec::new();
    for t in &oc.require {
        specs.push(parse_goal(t, true).map_err(EngineError::Config)?);
    }
    for t in &oc.prefer {
        specs.push(parse_goal(t, false).map_err(EngineError::Config)?);
    }
    let n_obj = specs.iter().filter(|s| s.kind.is_objective()).count();
    if n_obj > 10 {
        return Err(EngineError::Config(format!(
            "{n_obj} maximize/minimize goals given; at most 10 are supported. Turn some into thresholds (e.g. `cost<=5000`)."
        )));
    }
    // Models for every goal target.
    let mut model_targets: Vec<usize> = Vec::new();
    for s in &specs {
        let t = shared
            .fitted
            .iter()
            .position(|f| f.report.target == s.target)
            .ok_or_else(|| {
                EngineError::Config(format!(
                    "goal `{}` is on `{}`, which is not a --target",
                    s.text, s.target
                ))
            })?;
        if !model_targets.contains(&t) {
            model_targets.push(t);
        }
    }
    // Equations without fitted constants have no bootstrap members (they are exact under the
    // ensemble), so the member count comes from the targets that do have members.
    let n_members = shared
        .fitted
        .iter()
        .map(|f| f.ensemble.len())
        .filter(|&n| n > 0)
        .min()
        .unwrap_or(0);
    let mut models = Vec::new();
    for &t in &model_targets {
        let f = &shared.fitted[t];
        let expr = compose(t, shared, None, &mut Vec::new())?;
        let members = (0..n_members)
            .map(|b| compose(t, shared, Some(b), &mut Vec::new()))
            .collect::<Result<Vec<_>, _>>()?;
        let y = &f.prepared.ty;
        let range = y.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - y.iter().copied().fold(f64::INFINITY, f64::min);
        models.push(Model {
            name: f.report.target.clone(),
            expr,
            members,
            scale: stats::std(y).max(1e-12),
            tolerance: oc.tolerance * range.abs().max(1e-12),
        });
    }
    let goals: Vec<Goal> = specs
        .iter()
        .map(|s| Goal {
            model: models.iter().position(|m| m.name == s.target).unwrap_or(0),
            spec: s.clone(),
        })
        .collect();
    let names = &shared.names;
    let used: std::collections::BTreeSet<u16> =
        models.iter().flat_map(|m| m.expr.variables()).collect();
    let controllables: Vec<usize> = if oc.controllables.is_empty() {
        used.iter()
            .map(|&v| usize::from(v))
            .filter(|&v| v < names.len())
            .collect()
    } else {
        oc.controllables
            .iter()
            .map(|n| {
                names.iter().position(|m| m == n).ok_or_else(|| {
                    EngineError::Config(format!(
                        "`{n}` cannot be a controllable: it is a target or not a numeric column"
                    ))
                })
            })
            .collect::<Result<_, _>>()?
    };
    if controllables.is_empty() {
        return Err(EngineError::Config(
            "no controllable features; pass --control".into(),
        ));
    }
    let space = input_space(names, &controllables, cfg, &shared.domain)?;
    let out = optimize_goals(&MultiProblem {
        models: &models,
        goals: &goals,
        controllables: controllables.clone(),
        bounds: space.bounds.clone(),
        kinds: space.kinds.clone(),
        constraints: &space.constraints,
        baseline: space.baseline.clone(),
        scale: shared.domain.std.clone(),
        domain: &shared.domain,
        n_candidates: oc.n_candidates.max(1),
        max_evals: oc.max_evals,
        robust: oc.robust,
        priority: if oc.lexicographic {
            Priority::Lexicographic
        } else {
            Priority::Weighted
        },
        seed,
    });
    let evidence = model_targets
        .iter()
        .map(|&t| shared.fitted[t].evidence)
        .min()
        .unwrap_or(Evidence::Association);
    let cnames: Vec<String> = controllables.iter().map(|&j| names[j].clone()).collect();
    let first = &models[goals[0].model];
    let first_sigma = shared.fitted[model_targets[goals[0].model]].sigma;
    let candidates = out
        .candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let x = &c.inputs;
            let y0 = c.predictions[goals[0].model];
            let mp: Vec<f64> = first
                .members
                .iter()
                .map(|m| m.eval_point(x))
                .filter(|v| v.is_finite())
                .collect();
            let var = if mp.len() >= 2 {
                stats::variance(&mp)
            } else {
                0.0
            };
            let h = 1.645 * (var + first_sigma * first_sigma).sqrt();
            let grad = first.expr.gradient(x, x.len());
            CandidateReport {
                id: i + 1,
                inputs: controllables
                    .iter()
                    .map(|&j| (names[j].clone(), x[j]))
                    .collect(),
                predicted: y0,
                interval: [y0 - h, y0 + h],
                error: f64::NAN,
                within_tolerance: c.goals_met.iter().all(|&m| m),
                in_domain: c.in_domain,
                domain_distance: c.domain_distance,
                change: controllables
                    .iter()
                    .map(|&j| (names[j].clone(), x[j] - space.baseline[j]))
                    .collect(),
                change_pct: controllables
                    .iter()
                    .map(|&j| {
                        let b = space.baseline[j];
                        (
                            names[j].clone(),
                            (b != 0.0).then(|| 100.0 * (x[j] - b) / b.abs()),
                        )
                    })
                    .collect(),
                sensitivity: controllables
                    .iter()
                    .map(|&j| (names[j].clone(), grad[j]))
                    .collect(),
                elasticity: controllables
                    .iter()
                    .map(|&j| {
                        (
                            names[j].clone(),
                            if y0 != 0.0 {
                                grad[j] * x[j] / y0
                            } else {
                                f64::NAN
                            },
                        )
                    })
                    .collect(),
                predictions: models
                    .iter()
                    .zip(&c.predictions)
                    .map(|(m, &v)| (m.name.clone(), v))
                    .collect(),
                goals_met: c.goals_met.clone(),
                joint_probability: c.joint_probability,
                marks: c.marks.clone(),
            }
        })
        .collect();
    let conflict = out.conflict.as_ref().map(|k| ConflictReport {
        goals: k.goals.iter().map(|&g| specs[g].text.clone()).collect(),
        best_achievable: k
            .best_achievable
            .iter()
            .map(|&(g, v)| (specs[g].text.clone(), v))
            .collect(),
    });
    let unused: Vec<String> = controllables
        .iter()
        .filter(|&&j| !used.contains(&(j as u16)))
        .map(|&j| names[j].clone())
        .collect();
    Ok(OptimizeReport {
        target: specs[0].target.clone(),
        goal: f64::NAN,
        tolerance: f64::NAN,
        goals: specs.clone(),
        method: out.method.clone(),
        controllables: cnames.clone(),
        bounds: cnames.iter().cloned().zip(space.bounds).collect(),
        baseline: names.iter().cloned().zip(space.baseline).collect(),
        candidates,
        reached: out.feasible,
        closest_value: f64::NAN,
        binding: Vec::new(),
        unused_controllables: unused,
        evaluations: out.evaluations,
        evidence,
        conflict,
        redundant_objectives: out
            .redundant
            .iter()
            .map(|&(a, b, rho)| RedundantPair {
                a: specs[a].text.clone(),
                b: specs[b].text.clone(),
                rho,
            })
            .collect(),
        disclaimer: disclaimer(evidence),
    })
}
