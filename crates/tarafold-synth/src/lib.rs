//! Synthetic datasets generated from known equations (R11, design §6.1).
//!
//! Each registry entry defines input distributions, derived columns, the target, and the
//! true equation, so the benchmark can check whether discovery recovers it.

use serde::{Deserialize, Serialize};
use tarafold_core::{Rng, rng::stream, stats};
use tarafold_expr::{Expr, parse};
use tarafold_io::{Cell, write_csv};

/// How a column is produced.
#[derive(Debug, Clone)]
pub enum ColSpec {
    /// Uniform in `[lo, hi)`.
    Uniform(&'static str, f64, f64),
    /// Computed from earlier columns with an infix expression.
    Derived(&'static str, &'static str),
    /// Uniform over the grid `lo, lo + step, …, hi` (whole numbers when `step = 1`).
    Grid(&'static str, f64, f64, f64),
}

impl ColSpec {
    fn name(&self) -> &'static str {
        match self {
            Self::Uniform(n, ..) | Self::Derived(n, _) | Self::Grid(n, ..) => n,
        }
    }
}

/// One known-equation dataset.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Unique name used by `tarafold synth <name>`.
    pub name: &'static str,
    /// Benchmark tier (T1–T12).
    pub tier: &'static str,
    /// One-line description.
    pub description: &'static str,
    /// Columns in generation order.
    pub columns: Vec<ColSpec>,
    /// Target column (must be a `Derived` column).
    pub target: &'static str,
    /// Columns a user would plausibly control.
    pub controllables: Vec<&'static str>,
    /// A goal inside the achievable range.
    pub example_goal: f64,
    /// Heteroscedastic noise (T9): an infix expression of the inputs giving each row's relative
    /// noise scale. When set, a `weight` column `1 / scale²` (mean 1) is written before the target.
    pub noise_scale: Option<&'static str>,
}

/// Ground truth written next to the CSV.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Truth {
    /// Registry name.
    pub name: String,
    /// Tier.
    pub tier: String,
    /// Description.
    pub description: String,
    /// Target column.
    pub target: String,
    /// True equation for the target in terms of feature columns (infix).
    pub equation: String,
    /// All non-target columns in the CSV.
    pub features: Vec<String>,
    /// Columns the true equation uses.
    pub equation_variables: Vec<String>,
    /// Suggested controllables.
    pub controllables: Vec<String>,
    /// Suggested goal.
    pub example_goal: f64,
    /// Rows generated.
    pub rows: usize,
    /// Noise std as a fraction of the target's std.
    pub noise: f64,
    /// Number of pure-noise distractor columns.
    pub distractors: usize,
    /// Seed.
    pub seed: u64,
}

/// Generated dataset.
#[derive(Debug, Clone)]
pub struct Generated {
    /// Column names (features then target).
    pub headers: Vec<String>,
    /// Column values.
    pub columns: Vec<Vec<f64>>,
    /// Ground truth.
    pub truth: Truth,
}

impl Generated {
    /// The dataset as CSV text.
    pub fn to_csv(&self) -> Result<String, SynthError> {
        let n = self.columns.first().map_or(0, Vec::len);
        let rows: Vec<Vec<Cell>> = (0..n)
            .map(|i| self.columns.iter().map(|c| Cell::Num(c[i])).collect())
            .collect();
        write_csv(&self.headers, &rows).map_err(|e| SynthError::Write(e.to_string()))
    }
}

/// Generation errors.
#[derive(Debug, thiserror::Error)]
pub enum SynthError {
    /// Unknown registry name.
    #[error("unknown synthetic dataset `{0}`; run `tarafold synth --list`")]
    Unknown(String),
    /// Registry entry is malformed (a bug).
    #[error("registry entry `{name}` is invalid: {message}")]
    BadEntry {
        /// Entry name.
        name: String,
        /// Problem.
        message: String,
    },
    /// Invalid parameters.
    #[error("invalid parameters: {0}")]
    Params(String),
    /// CSV writing failed.
    #[error("could not write CSV: {0}")]
    Write(String),
}

use ColSpec::{Derived, Grid, Uniform};

/// All built-in entries.
pub fn registry() -> Vec<Entry> {
    vec![
        Entry {
            name: "linear",
            tier: "T1",
            description: "Linear: y = 3*x1 - 2*x2 + 5",
            columns: vec![
                Uniform("x1", 0.0, 10.0),
                Uniform("x2", 0.0, 10.0),
                Derived("y", "3*x1 - 2*x2 + 5"),
            ],
            target: "y",
            controllables: vec!["x1", "x2"],
            example_goal: 20.0,
            noise_scale: None,
        },
        Entry {
            name: "polynomial",
            tier: "T2",
            description: "Polynomial with interaction: y = x1^2 + 2*x1*x2 - x2",
            columns: vec![
                Uniform("x1", -3.0, 3.0),
                Uniform("x2", -3.0, 3.0),
                Derived("y", "x1^2 + 2*x1*x2 - x2"),
            ],
            target: "y",
            controllables: vec!["x1", "x2"],
            example_goal: 5.0,
            noise_scale: None,
        },
        Entry {
            name: "rational",
            tier: "T3",
            description: "Rational: y = x1 / (1 + x2)",
            columns: vec![
                Uniform("x1", 1.0, 10.0),
                Uniform("x2", 0.0, 5.0),
                Derived("y", "x1 / (1 + x2)"),
            ],
            target: "y",
            controllables: vec!["x1", "x2"],
            example_goal: 3.0,
            noise_scale: None,
        },
        Entry {
            name: "cobb-douglas",
            tier: "T4",
            description: "Power law (Cobb-Douglas): output = 2.5 * capital^0.7 * labor^0.3",
            columns: vec![
                Uniform("capital", 1.0, 100.0),
                Uniform("labor", 1.0, 100.0),
                Derived("output", "2.5 * capital^0.7 * labor^0.3"),
            ],
            target: "output",
            controllables: vec!["capital", "labor"],
            example_goal: 100.0,
            noise_scale: None,
        },
        Entry {
            name: "exp-log",
            tier: "T5",
            description: "Exponential plus log: y = 10*exp(-0.5*x1) + log(x2)",
            columns: vec![
                Uniform("x1", 0.0, 5.0),
                Uniform("x2", 1.0, 10.0),
                Derived("y", "10*exp(-0.5*x1) + log(x2)"),
            ],
            target: "y",
            controllables: vec!["x1", "x2"],
            example_goal: 5.0,
            noise_scale: None,
        },
        Entry {
            name: "interaction",
            tier: "T6",
            description: "Interaction: y = x1*x2 + x3 (use --distractors to add noise columns)",
            columns: vec![
                Uniform("x1", 0.0, 5.0),
                Uniform("x2", 0.0, 5.0),
                Uniform("x3", 0.0, 5.0),
                Derived("y", "x1*x2 + x3"),
            ],
            target: "y",
            controllables: vec!["x1", "x2", "x3"],
            example_goal: 10.0,
            noise_scale: None,
        },
        Entry {
            name: "kinetic-energy",
            tier: "T7",
            description: "Physics: energy = 0.5 * mass * velocity^2",
            columns: vec![
                Uniform("mass", 1.0, 10.0),
                Uniform("velocity", 0.5, 20.0),
                Derived("energy", "0.5 * mass * velocity^2"),
            ],
            target: "energy",
            controllables: vec!["mass", "velocity"],
            example_goal: 500.0,
            noise_scale: None,
        },
        Entry {
            name: "gravity",
            tier: "T7",
            description: "Physics: force = 6.674 * m1 * m2 / r^2",
            columns: vec![
                Uniform("m1", 1.0, 100.0),
                Uniform("m2", 1.0, 100.0),
                Uniform("r", 1.0, 10.0),
                Derived("force", "6.674 * m1 * m2 / r^2"),
            ],
            target: "force",
            controllables: vec!["m1", "r"],
            example_goal: 1000.0,
            noise_scale: None,
        },
        Entry {
            name: "business-revenue",
            tier: "T8",
            description: "Business funnel with intermediate columns: revenue = orders * price",
            columns: vec![
                Uniform("spend", 1_000.0, 50_000.0),
                Uniform("price", 10.0, 50.0),
                Derived("visitors", "120 * spend^0.6"),
                Derived("conversion", "0.08 * exp(-0.03 * price)"),
                Derived("orders", "visitors * conversion"),
                Derived("revenue", "orders * price"),
            ],
            target: "revenue",
            controllables: vec!["orders", "price"],
            example_goal: 50_000.0,
            noise_scale: None,
        },
        Entry {
            name: "business-direct",
            tier: "T8",
            description: "Business funnel from spend and price only: \
                          revenue = 9.6 * spend^0.6 * price * exp(-0.03 * price)",
            columns: vec![
                Uniform("spend", 1_000.0, 50_000.0),
                Uniform("price", 10.0, 50.0),
                Derived(
                    "revenue",
                    "120 * spend^0.6 * 0.08 * exp(-0.03 * price) * price",
                ),
            ],
            target: "revenue",
            controllables: vec!["spend", "price"],
            example_goal: 50_000.0,
            noise_scale: None,
        },
        Entry {
            name: "production",
            tier: "T10",
            description: "Discrete inputs: output = 40 * machines^0.8 * hours^0.5 (machines whole, hours in 0.5 steps)",
            columns: vec![
                Grid("machines", 1.0, 20.0, 1.0),
                Grid("hours", 1.0, 12.0, 0.5),
                Derived("output", "40 * machines^0.8 * hours^0.5"),
            ],
            target: "output",
            controllables: vec!["machines", "hours"],
            example_goal: 600.0,
            noise_scale: None,
        },
        Entry {
            name: "linear5",
            tier: "T1",
            description: "Linear in five inputs, one unused: y = 2*x1 - x2 + 0.5*x3 + 3*x4 + 1",
            columns: vec![
                Uniform("x1", 0.0, 10.0),
                Uniform("x2", 0.0, 10.0),
                Uniform("x3", 0.0, 10.0),
                Uniform("x4", 0.0, 10.0),
                Uniform("x5", 0.0, 10.0),
                Derived("y", "2*x1 - x2 + 0.5*x3 + 3*x4 + 1"),
            ],
            target: "y",
            controllables: vec!["x1", "x4"],
            example_goal: 40.0,
            noise_scale: None,
        },
        Entry {
            name: "cubic",
            tier: "T2",
            description: "Cubic: y = x^3 - x",
            columns: vec![Uniform("x", -2.0, 2.0), Derived("y", "x^3 - x")],
            target: "y",
            controllables: vec!["x"],
            example_goal: 3.0,
            noise_scale: None,
        },
        Entry {
            name: "quadratic3",
            tier: "T2",
            description: "Quadratic with a product: y = x1^2 + x2*x3",
            columns: vec![
                Uniform("x1", -3.0, 3.0),
                Uniform("x2", 0.0, 4.0),
                Uniform("x3", 0.0, 4.0),
                Derived("y", "x1^2 + x2*x3"),
            ],
            target: "y",
            controllables: vec!["x1", "x2"],
            example_goal: 8.0,
            noise_scale: None,
        },
        Entry {
            name: "rational2",
            tier: "T3",
            description: "Rational: y = (x1 + x2) / (x1 * x2)",
            columns: vec![
                Uniform("x1", 1.0, 5.0),
                Uniform("x2", 1.0, 5.0),
                Derived("y", "(x1 + x2) / (x1 * x2)"),
            ],
            target: "y",
            controllables: vec!["x1", "x2"],
            example_goal: 1.0,
            noise_scale: None,
        },
        Entry {
            name: "power-ratio",
            tier: "T4",
            description: "Power law ratio: y = x1^1.5 / x2",
            columns: vec![
                Uniform("x1", 1.0, 10.0),
                Uniform("x2", 1.0, 10.0),
                Derived("y", "x1^1.5 / x2"),
            ],
            target: "y",
            controllables: vec!["x1", "x2"],
            example_goal: 5.0,
            noise_scale: None,
        },
        Entry {
            name: "exp-growth",
            tier: "T5",
            description: "Exponential growth: y = x1 * exp(x2 / 10)",
            columns: vec![
                Uniform("x1", 1.0, 10.0),
                Uniform("x2", 0.0, 20.0),
                Derived("y", "x1 * exp(x2 / 10)"),
            ],
            target: "y",
            controllables: vec!["x1", "x2"],
            example_goal: 20.0,
            noise_scale: None,
        },
        Entry {
            name: "decay",
            tier: "T5",
            description: "Decay to a floor: y = 5 * exp(-0.2 * t) + 1",
            columns: vec![
                Uniform("t", 0.0, 20.0),
                Derived("y", "5 * exp(-0.2 * t) + 1"),
            ],
            target: "y",
            controllables: vec!["t"],
            example_goal: 2.0,
            noise_scale: None,
        },
        Entry {
            name: "saturating",
            tier: "T6",
            description: "Saturating share: y = x1 / (x1 + x2)",
            columns: vec![
                Uniform("x1", 0.5, 10.0),
                Uniform("x2", 0.5, 10.0),
                Derived("y", "x1 / (x1 + x2)"),
            ],
            target: "y",
            controllables: vec!["x1", "x2"],
            example_goal: 0.7,
            noise_scale: None,
        },
        Entry {
            name: "pendulum",
            tier: "T7",
            description: "Physics: period = 2*pi*sqrt(length / 9.81)",
            columns: vec![
                Uniform("length", 0.1, 5.0),
                Derived("period", "6.283185307179586 * sqrt(length / 9.81)"),
            ],
            target: "period",
            controllables: vec!["length"],
            example_goal: 3.0,
            noise_scale: None,
        },
        Entry {
            name: "ideal-gas",
            tier: "T7",
            description: "Physics: temperature = pressure * volume / (moles * 8.314)",
            columns: vec![
                Uniform("pressure", 50.0, 200.0),
                Uniform("volume", 1.0, 20.0),
                Uniform("moles", 0.5, 5.0),
                Derived("temperature", "pressure * volume / (moles * 8.314)"),
            ],
            target: "temperature",
            controllables: vec!["pressure", "volume"],
            example_goal: 300.0,
            noise_scale: None,
        },
        Entry {
            name: "coulomb",
            tier: "T7",
            description: "Physics: force = 8.988 * q1 * q2 / r^2",
            columns: vec![
                Uniform("q1", 0.5, 5.0),
                Uniform("q2", 0.5, 5.0),
                Uniform("r", 0.5, 3.0),
                Derived("force", "8.988 * q1 * q2 / r^2"),
            ],
            target: "force",
            controllables: vec!["q1", "r"],
            example_goal: 40.0,
            noise_scale: None,
        },
        Entry {
            name: "drag",
            tier: "T7",
            description: "Physics: drag = 0.6 * cd * area * speed^2",
            columns: vec![
                Uniform("cd", 0.2, 1.2),
                Uniform("area", 0.5, 3.0),
                Uniform("speed", 1.0, 30.0),
                Derived("drag", "0.6 * cd * area * speed^2"),
            ],
            target: "drag",
            controllables: vec!["speed"],
            example_goal: 300.0,
            noise_scale: None,
        },
        Entry {
            name: "weighted-linear",
            tier: "T9",
            description: "Heteroscedastic linear (noise grows with x1; weight = 1/sigma^2): y = 3*x1 - 2*x2 + 5",
            columns: vec![
                Uniform("x1", 0.0, 10.0),
                Uniform("x2", 0.0, 10.0),
                Derived("y", "3*x1 - 2*x2 + 5"),
            ],
            target: "y",
            controllables: vec!["x1", "x2"],
            example_goal: 20.0,
            noise_scale: Some("0.1 + x1^2"),
        },
        Entry {
            name: "weighted-power",
            tier: "T9",
            description: "Heteroscedastic power law (noise grows with capital; weight = 1/sigma^2): output = 2.5 * capital^0.7 * labor^0.3",
            columns: vec![
                Uniform("capital", 1.0, 100.0),
                Uniform("labor", 1.0, 100.0),
                Derived("output", "2.5 * capital^0.7 * labor^0.3"),
            ],
            target: "output",
            controllables: vec!["capital", "labor"],
            example_goal: 100.0,
            noise_scale: Some("capital"),
        },
    ]
}

/// Input kinds of the registry entry's grid columns: `(name, step)`.
pub fn grid_steps(entry: &Entry) -> Vec<(&'static str, f64)> {
    entry
        .columns
        .iter()
        .filter_map(|c| {
            if let Grid(n, _, _, s) = c {
                Some((*n, *s))
            } else {
                None
            }
        })
        .collect()
}

/// Input ranges `(name, lo, hi)` of the entry's uniform and grid columns.
pub fn input_ranges(entry: &Entry) -> Vec<(&'static str, f64, f64)> {
    entry
        .columns
        .iter()
        .filter_map(|c| match c {
            Uniform(n, lo, hi) | Grid(n, lo, hi, _) => Some((*n, *lo, *hi)),
            Derived(..) => None,
        })
        .collect()
}

/// Evaluate every column of an entry for one row of input values (no noise). Inputs not
/// given are an error. Used to "run an experiment" in closed-loop benchmarks.
pub fn evaluate_row(
    entry: &Entry,
    inputs: &[(String, f64)],
) -> Result<Vec<(String, f64)>, SynthError> {
    let bad = |message: String| SynthError::BadEntry {
        name: entry.name.into(),
        message,
    };
    let mut names: Vec<String> = Vec::new();
    let mut vals: Vec<f64> = Vec::new();
    for spec in &entry.columns {
        let v = match spec {
            Uniform(n, ..) | Grid(n, ..) => inputs
                .iter()
                .find(|(k, _)| k == n)
                .map(|(_, v)| *v)
                .ok_or_else(|| SynthError::Params(format!("missing input `{n}`")))?,
            Derived(_, src) => parse(src, &names)
                .map_err(|e| bad(e.to_string()))?
                .eval_point(&vals),
        };
        names.push(spec.name().to_string());
        vals.push(v);
    }
    Ok(names.into_iter().zip(vals).collect())
}
/// Look up an entry by name.
pub fn find(name: &str) -> Result<Entry, SynthError> {
    registry()
        .into_iter()
        .find(|e| e.name == name)
        .ok_or_else(|| SynthError::Unknown(name.into()))
}

/// Generate `rows` rows. `noise` is the std of Gaussian noise added to the target, as a
/// fraction of the noiseless target's std. Distractors are `z1..zk ~ U(0, 10)`.
pub fn generate(
    entry: &Entry,
    rows: usize,
    noise: f64,
    distractors: usize,
    seed: u64,
) -> Result<Generated, SynthError> {
    generate_widened(entry, rows, noise, distractors, seed, 0.0)
}

/// Like [`generate`], with every uniform input range widened by `widen × (hi − lo)` on each
/// side (lower bounds of non-negative ranges are clamped at 0). Used to test extrapolation.
pub fn generate_widened(
    entry: &Entry,
    rows: usize,
    noise: f64,
    distractors: usize,
    seed: u64,
    widen: f64,
) -> Result<Generated, SynthError> {
    if rows == 0 || rows > 10_000_000 {
        return Err(SynthError::Params(
            "rows must be between 1 and 10,000,000".into(),
        ));
    }
    if !(0.0..=10.0).contains(&noise) {
        return Err(SynthError::Params("noise must be between 0 and 10".into()));
    }
    if distractors > 100 {
        return Err(SynthError::Params("at most 100 distractors".into()));
    }
    let bad = |message: String| SynthError::BadEntry {
        name: entry.name.into(),
        message,
    };
    let mut rng = Rng::derive(seed, &[stream::SYNTH]);
    let mut names: Vec<String> = Vec::new();
    let mut cols: Vec<Vec<f64>> = Vec::new();
    let mut truth_eq: Option<(Expr, Vec<String>)> = None;

    for spec in &entry.columns {
        let values = match spec {
            Uniform(_, lo, hi) => {
                let span = hi - lo;
                let wlo = if *lo > 0.0 {
                    (lo - widen * span).max(lo / 2.0)
                } else if *lo == 0.0 {
                    0.0
                } else {
                    lo - widen * span
                };
                let whi = hi + widen * span;
                (0..rows).map(|_| rng.range(wlo, whi)).collect()
            }
            Grid(_, lo, hi, step) => {
                let k = ((hi - lo) / step).round() as usize;
                (0..rows)
                    .map(|_| lo + step * rng.below(k + 1) as f64)
                    .collect()
            }
            Derived(_, src) => {
                let e = parse(src, &names).map_err(|e| bad(e.to_string()))?;
                let refs: Vec<&[f64]> = cols.iter().map(Vec::as_slice).collect();
                let v = e
                    .eval_columns(&refs, rows)
                    .ok_or_else(|| bad("evaluation failed".into()))?;
                if v.iter().any(|x| !x.is_finite()) {
                    return Err(bad(format!("`{src}` produced non-finite values")));
                }
                if spec.name() == entry.target {
                    truth_eq = Some((e, names.clone()));
                }
                v
            }
        };
        names.push(spec.name().to_string());
        cols.push(values);
    }
    let (eq, eq_names) = truth_eq.ok_or_else(|| bad("target is not a derived column".into()))?;
    let t = names
        .iter()
        .position(|n| n == entry.target)
        .ok_or_else(|| bad("no target".into()))?;

    for k in 1..=distractors {
        names.push(format!("z{k}"));
        cols.push((0..rows).map(|_| rng.range(0.0, 10.0)).collect());
    }
    // Heteroscedastic noise: per-row scale with mean square 1, and the matching weight column.
    let scale: Option<Vec<f64>> = match entry.noise_scale {
        None => None,
        Some(src) => {
            let e = parse(src, &names).map_err(|e| bad(e.to_string()))?;
            let refs: Vec<&[f64]> = cols.iter().map(Vec::as_slice).collect();
            let s = e
                .eval_columns(&refs, rows)
                .ok_or_else(|| bad("noise scale evaluation failed".into()))?;
            if s.iter().any(|v| !v.is_finite() || *v <= 0.0) {
                return Err(bad("noise scale must be positive".into()));
            }
            let ms = (s.iter().map(|v| v * v).sum::<f64>() / rows as f64).sqrt();
            let s: Vec<f64> = s.iter().map(|v| v / ms).collect();
            let w: Vec<f64> = s.iter().map(|v| 1.0 / (v * v)).collect();
            let mw = w.iter().sum::<f64>() / rows as f64;
            names.push("weight".into());
            cols.push(w.iter().map(|v| v / mw).collect());
            Some(s)
        }
    };
    if noise > 0.0 {
        let sd = stats::std(&cols[t]) * noise;
        for (i, v) in cols[t].iter_mut().enumerate() {
            let k = scale.as_ref().map_or(1.0, |s| s[i]);
            *v += sd * k * rng.normal();
        }
    }
    // Put the target last.
    let target_col = cols.remove(t);
    let target_name = names.remove(t);
    cols.push(target_col);
    names.push(target_name);

    let equation_variables: Vec<String> = eq
        .variables()
        .iter()
        .map(|&i| eq_names[i as usize].clone())
        .collect();
    let truth = Truth {
        name: entry.name.into(),
        tier: entry.tier.into(),
        description: entry.description.into(),
        target: entry.target.into(),
        equation: eq.to_infix(&eq_names),
        features: names[..names.len() - 1].to_vec(),
        equation_variables,
        controllables: entry.controllables.iter().map(|s| s.to_string()).collect(),
        example_goal: entry.example_goal,
        rows,
        noise,
        distractors,
        seed,
    };
    Ok(Generated {
        headers: names,
        columns: cols,
        truth,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_generates() {
        for e in registry() {
            let g = generate(&e, 200, 0.0, 2, 1).unwrap();
            assert_eq!(g.headers.last().unwrap(), e.target);
            assert_eq!(g.columns.len(), g.headers.len());
            // The truth equation reproduces the noiseless target from the features.
            let feats = &g.truth.features;
            let eq = parse(&g.truth.equation, feats).unwrap();
            let refs: Vec<&[f64]> = g.columns[..feats.len()].iter().map(Vec::as_slice).collect();
            let pred = eq.eval_columns(&refs, 200).unwrap();
            let y = g.columns.last().unwrap();
            for (a, b) in pred.iter().zip(y) {
                assert!((a - b).abs() <= 1e-9 * b.abs().max(1.0), "{}", e.name);
            }
            for c in &e.controllables {
                assert!(feats.iter().any(|f| f == c), "{}: {c}", e.name);
            }
        }
    }

    #[test]
    fn reproducible_and_noisy() {
        let e = find("linear").unwrap();
        let a = generate(&e, 100, 0.1, 0, 5).unwrap();
        let b = generate(&e, 100, 0.1, 0, 5).unwrap();
        assert_eq!(a.columns, b.columns);
        let c = generate(&e, 100, 0.0, 0, 5).unwrap();
        assert_ne!(a.columns[2], c.columns[2]);
        assert!(a.to_csv().unwrap().starts_with("x1,x2,y\n"));
    }
}
