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
}

impl ColSpec {
    fn name(&self) -> &'static str {
        match self {
            Self::Uniform(n, ..) | Self::Derived(n, _) => n,
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

use ColSpec::{Derived, Uniform};

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
        },
    ]
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
            Uniform(_, lo, hi) => (0..rows).map(|_| rng.range(*lo, *hi)).collect(),
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
    if noise > 0.0 {
        let sd = stats::std(&cols[t]) * noise;
        for v in cols[t].iter_mut() {
            *v += sd * rng.normal();
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
