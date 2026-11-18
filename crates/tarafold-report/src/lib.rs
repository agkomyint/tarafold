//! Report schema (R10.1), JSON and CSV writers, and user-facing text.
//!
//! The JSON Schema printed by `tarafold schema` is generated from these types, so the
//! schema and the writer cannot drift.

pub mod export;
pub mod html;
mod read;
pub mod text;

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tarafold_core::{DataSource, Evidence, RunConfig, SourceInfo};
use tarafold_discover::{Strategy, StrategyRun};
use tarafold_expr::Tree;
use tarafold_io::{Cell, write_csv};
use tarafold_optimize::spec::GoalSpec;
use tarafold_profile::ColumnProfile;
pub use tarafold_relate::{RelateReport, ScreenDecision};
use tarafold_validate::reliability::{Bootstrap, Residuals};
use tarafold_validate::{Domain, Metrics, Reliability};

/// Report schema version (SemVer, independent of the tool version).
pub const SCHEMA_VERSION: &str = "0.1.0";

/// Top-level report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Report {
    /// Schema version.
    pub schema_version: String,
    /// Tarafold version that wrote the report.
    pub tarafold_version: String,
    /// Run metadata.
    pub run: RunInfo,
    /// Dataset summary.
    pub dataset: DatasetInfo,
    /// One entry per target.
    pub targets: Vec<TargetReport>,
    /// Optimization results, when a goal was given.
    pub optimize: Option<OptimizeReport>,
    /// Next-experiment suggestions, when requested.
    #[serde(default)]
    pub suggest: Option<SuggestReport>,
    /// Warnings and threats to interpretation.
    pub warnings: Vec<Warning>,
    /// Interpretation notes.
    pub notes: Vec<String>,
}

/// Run status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// All requested stages finished.
    Completed,
    /// Stopped early by the user.
    Interrupted,
}

/// Run metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunInfo {
    /// Run id (derived from input hash and seed).
    pub id: String,
    /// Start time, seconds since the Unix epoch.
    pub started_unix: u64,
    /// Status.
    pub status: RunStatus,
    /// Computation engine.
    pub engine: String,
    /// Fully resolved configuration.
    pub config: RunConfig,
    /// Stage timings.
    pub stages: Vec<StageTiming>,
}

/// Wall time of one stage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StageTiming {
    /// Stage name.
    pub name: String,
    /// Milliseconds.
    pub millis: u64,
}

/// Dataset summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DatasetInfo {
    /// Provenance, exclusions, missing-value counts.
    pub source: SourceInfo,
    /// Usable rows.
    pub n_rows: usize,
    /// Per-column summary.
    pub columns: Vec<ColumnSummary>,
    /// Per-column profile (R4).
    pub profile: Vec<ColumnProfile>,
    /// Declared data source.
    pub data_source: DataSource,
    /// Whether row weights were supplied.
    pub weighted: bool,
}

/// Column summary statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColumnSummary {
    /// Name.
    pub name: String,
    /// Minimum.
    pub min: f64,
    /// Maximum.
    pub max: f64,
    /// Mean.
    pub mean: f64,
    /// Standard deviation.
    pub std: f64,
    /// Median.
    pub median: f64,
}

/// Results for one target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TargetReport {
    /// Target column.
    pub target: String,
    /// Features passed to discovery.
    pub features: Vec<String>,
    /// Relationship analysis over all candidate features.
    pub relate: RelateReport,
    /// Screening decisions.
    pub screening: Vec<ScreenDecision>,
    /// Discovery.
    pub discover: DiscoverReport,
    /// Validation of the selected equation.
    pub validate: ValidateReport,
}

/// Discovery results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DiscoverReport {
    /// Pareto front, simplest first.
    pub front: Vec<EquationReport>,
    /// Id of the selected equation.
    pub selected_id: usize,
    /// Strategy log.
    pub strategies: Vec<StrategyRun>,
    /// Candidates evaluated.
    pub evaluated: usize,
    /// Training rows used by discovery (after subsampling).
    pub rows_used: usize,
}

/// One equation on the front.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EquationReport {
    /// Position on the front.
    pub id: usize,
    /// Readable infix (4 significant digits).
    pub infix: String,
    /// Exact infix that parses back to the same expression.
    pub infix_exact: String,
    /// LaTeX.
    pub latex: String,
    /// JSON expression tree.
    pub tree: Tree,
    /// Complexity.
    pub complexity: usize,
    /// Fitted constants in postfix order.
    pub constants: Vec<f64>,
    /// Variables used.
    pub variables: Vec<String>,
    /// Strategy that produced it.
    pub strategy: Strategy,
    /// Metrics on each split.
    pub metrics: SplitMetrics,
}

/// Metrics on train, validation, and test.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SplitMetrics {
    /// Training split.
    pub train: Metrics,
    /// Validation split.
    pub val: Metrics,
    /// Test split.
    pub test: Metrics,
}

/// Validation of the selected equation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ValidateReport {
    /// Reliability grade with checks.
    pub reliability: Reliability,
    /// Evidence level.
    pub evidence: Evidence,
    /// What the evidence level rests on.
    pub evidence_basis: String,
    /// Half-width of the 90% prediction interval.
    pub interval_halfwidth_90: f64,
    /// How the interval was computed.
    pub interval_method: String,
    /// Share of test rows inside the 90% interval.
    pub test_coverage_90: f64,
    /// Standard deviation of validation residuals.
    pub residual_sd: f64,
    /// Bootstrap intervals for the constants.
    pub bootstrap: Bootstrap,
    /// Structural stability, when computed.
    pub stability: Option<Stability>,
    /// Residual diagnostics on test rows.
    pub residuals: Residuals,
    /// Training-data domain.
    pub domain: Domain,
}

/// Structural stability of the selected equation (R7.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Stability {
    /// Resampled rediscoveries run.
    pub resamples: usize,
    /// Rediscoveries whose selected equation was equivalent.
    pub equivalent: usize,
    /// `equivalent / resamples`.
    pub fraction: f64,
    /// GP evaluation budget per rediscovery.
    pub gp_budget: usize,
}

/// Next-experiment suggestions (R19).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SuggestReport {
    /// Mode requested.
    pub mode: String,
    /// Picks chosen to exploit (reach goals / improve objectives).
    pub exploit: usize,
    /// Picks chosen to explore (reduce uncertainty).
    pub explore: usize,
    /// Controllables varied.
    pub controllables: Vec<String>,
    /// Search range per controllable (observed range widened by `expand`).
    pub bounds: BTreeMap<String, [f64; 2]>,
    /// Competing equations used per target, with weights relative to the selected one.
    pub structures: BTreeMap<String, Vec<(String, f64)>>,
    /// Suggestions in order of choice.
    pub suggestions: Vec<SuggestionReport>,
    /// What suggestions are and are not.
    pub note: String,
}

/// One suggested experiment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SuggestionReport {
    /// Order of choice, starting at 1.
    pub id: usize,
    /// Settings for the controllables.
    pub inputs: BTreeMap<String, f64>,
    /// Every non-target column for this experiment (non-controllables at their fixed values).
    pub row: BTreeMap<String, f64>,
    /// `exploit` or `explore`.
    pub mode: String,
    /// `uncertainty`, `equation_discrimination`, `goal_likelihood`, or `improvement`.
    pub reason: String,
    /// Acquisition score at the pick.
    pub acquisition: f64,
    /// Ensemble mean per target.
    pub predictions: BTreeMap<String, f64>,
    /// 90% ensemble range per target.
    pub intervals: BTreeMap<String, [f64; 2]>,
    /// Probability all hard goals hold (`null` without hard goals).
    pub joint_probability: f64,
    /// Inside the observed ranges and data domain.
    pub in_domain: bool,
}

/// Optimization results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OptimizeReport {
    /// Target.
    pub target: String,
    /// Goal value for single-goal runs (`null` for multi-goal runs).
    pub goal: f64,
    /// Absolute tolerance for single-goal runs.
    pub tolerance: f64,
    /// Goals (one exact goal for `--goal`; any number for `--require`/`--prefer`).
    #[serde(default)]
    pub goals: Vec<GoalSpec>,
    /// Search method used.
    #[serde(default)]
    pub method: String,
    /// Controllables.
    pub controllables: Vec<String>,
    /// Bounds per controllable.
    pub bounds: BTreeMap<String, [f64; 2]>,
    /// Baseline values of all features.
    pub baseline: BTreeMap<String, f64>,
    /// Ranked candidates.
    pub candidates: Vec<CandidateReport>,
    /// A candidate is within tolerance.
    pub reached: bool,
    /// Closest predicted value found.
    pub closest_value: f64,
    /// Bounds active at the closest point when the goal was not reached (`name:lower|upper`).
    pub binding: Vec<String>,
    /// Controllables that the selected equation does not use.
    pub unused_controllables: Vec<String>,
    /// Objective evaluations.
    pub evaluations: usize,
    /// Evidence level the candidates rest on.
    pub evidence: Evidence,
    /// Goals that cannot all hold, when no feasible candidate exists.
    #[serde(default)]
    pub conflict: Option<ConflictReport>,
    /// Objective pairs that move together across the trade-off set (rank correlation > 0.9).
    #[serde(default)]
    pub redundant_objectives: Vec<RedundantPair>,
    /// Disclaimer.
    pub disclaimer: String,
}

/// Minimal set of goals that conflict, with the best value each can reach while the others hold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConflictReport {
    /// Goal texts in the conflicting subset.
    pub goals: Vec<String>,
    /// Best achievable predicted value per conflicting goal: `(goal, value)`.
    pub best_achievable: Vec<(String, f64)>,
}

/// Two objectives that are redundant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RedundantPair {
    /// First goal.
    pub a: String,
    /// Second goal.
    pub b: String,
    /// Spearman rank correlation across the trade-off set.
    pub rho: f64,
}

/// One optimization candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CandidateReport {
    /// Rank, starting at 1.
    pub id: usize,
    /// Controllable values.
    pub inputs: BTreeMap<String, f64>,
    /// Predicted target.
    pub predicted: f64,
    /// 90% prediction interval.
    pub interval: [f64; 2],
    /// Predicted minus goal.
    pub error: f64,
    /// Within tolerance.
    pub within_tolerance: bool,
    /// Inside the training data domain.
    pub in_domain: bool,
    /// Mahalanobis distance to training data.
    pub domain_distance: f64,
    /// Absolute change from baseline.
    pub change: BTreeMap<String, f64>,
    /// Percent change from baseline.
    pub change_pct: BTreeMap<String, Option<f64>>,
    /// ∂ŷ/∂x.
    pub sensitivity: BTreeMap<String, f64>,
    /// Elasticity.
    pub elasticity: BTreeMap<String, f64>,
    /// Predicted value of every goal target.
    #[serde(default)]
    pub predictions: BTreeMap<String, f64>,
    /// Whether each goal is met, in goal order.
    #[serde(default)]
    pub goals_met: Vec<bool>,
    /// Share of bootstrap ensemble members for which every hard goal holds.
    #[serde(default)]
    pub joint_probability: f64,
    /// Labels: `best`, `knee`, `best:<goal>`, `preferred`, `most_likely`.
    #[serde(default)]
    pub marks: Vec<String>,
}

/// A warning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Warning {
    /// Machine-readable code.
    pub code: String,
    /// Message.
    pub message: String,
    /// Target it applies to.
    pub target: Option<String>,
}

/// Errors writing outputs.
#[derive(Debug)]
pub struct ReportError(pub String);

impl std::fmt::Display for ReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ReportError {}

impl Report {
    /// Read a report written by [`Report::to_json`]. `null` numbers read back as `NaN`.
    pub fn from_json(text: &str) -> Result<Self, ReportError> {
        use serde::Deserialize;
        let v: serde_json::Value =
            serde_json::from_str(text).map_err(|e| ReportError(e.to_string()))?;
        Report::deserialize(read::Lenient(v)).map_err(|e| ReportError(e.to_string()))
    }

    /// Pretty JSON.
    pub fn to_json(&self) -> Result<String, ReportError> {
        serde_json::to_string_pretty(self).map_err(|e| ReportError(e.to_string()))
    }

    /// `equations.csv`: every front equation of every target.
    pub fn equations_csv(&self) -> Result<String, ReportError> {
        let headers: Vec<String> = [
            "target",
            "id",
            "selected",
            "equation",
            "complexity",
            "strategy",
            "r2_train",
            "r2_val",
            "r2_test",
            "rmse_test",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mut rows = Vec::new();
        for t in &self.targets {
            for e in &t.discover.front {
                rows.push(vec![
                    Cell::Text(t.target.clone()),
                    Cell::Num(e.id as f64),
                    Cell::Text((e.id == t.discover.selected_id).to_string()),
                    Cell::Text(e.infix.clone()),
                    Cell::Num(e.complexity as f64),
                    Cell::Text(format!("{:?}", e.strategy)),
                    Cell::Num(e.metrics.train.r2),
                    Cell::Num(e.metrics.val.r2),
                    Cell::Num(e.metrics.test.r2),
                    Cell::Num(e.metrics.test.rmse),
                ]);
            }
        }
        write_csv(&headers, &rows).map_err(|e| ReportError(e.to_string()))
    }

    /// `suggestions.csv`: one row per suggestion with the dataset's columns; controllables and
    /// fixed inputs filled, target columns empty so results can be filled in and appended.
    pub fn suggestions_csv(
        &self,
        columns: &[String],
        values: &[BTreeMap<String, f64>],
    ) -> Result<String, ReportError> {
        let rows: Vec<Vec<Cell>> = values
            .iter()
            .map(|v| {
                columns
                    .iter()
                    .map(|c| v.get(c).map_or(Cell::Empty, |x| Cell::Num(*x)))
                    .collect()
            })
            .collect();
        write_csv(columns, &rows).map_err(|e| ReportError(e.to_string()))
    }

    /// `candidates.csv`: optimization candidates (header only when there are none).
    pub fn candidates_csv(&self) -> Result<String, ReportError> {
        let Some(o) = &self.optimize else {
            return write_csv(&["id".into()], &[]).map_err(|e| ReportError(e.to_string()));
        };
        let mut headers = vec!["id".to_string()];
        headers.extend(o.controllables.iter().cloned());
        headers.extend(
            [
                "predicted",
                "interval_lo",
                "interval_hi",
                "error",
                "within_tolerance",
                "in_domain",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
        let rows: Vec<Vec<Cell>> = o
            .candidates
            .iter()
            .map(|c| {
                let mut r = vec![Cell::Num(c.id as f64)];
                r.extend(
                    o.controllables
                        .iter()
                        .map(|n| Cell::Num(c.inputs.get(n).copied().unwrap_or(f64::NAN))),
                );
                r.extend([
                    Cell::Num(c.predicted),
                    Cell::Num(c.interval[0]),
                    Cell::Num(c.interval[1]),
                    Cell::Num(c.error),
                    Cell::Text(c.within_tolerance.to_string()),
                    Cell::Text(c.in_domain.to_string()),
                ]);
                r
            })
            .collect();
        write_csv(&headers, &rows).map_err(|e| ReportError(e.to_string()))
    }
}

/// JSON Schema of [`Report`].
///
/// Non-finite numbers (`NaN`, `±∞`, e.g. an undefined R²) are written as `null`, so every
/// `number` in the schema also allows `null`.
pub fn schema_json() -> Result<String, ReportError> {
    let schema = schemars::schema_for!(Report);
    let mut v = serde_json::to_value(&schema).map_err(|e| ReportError(e.to_string()))?;
    allow_null_numbers(&mut v);
    serde_json::to_string_pretty(&v).map_err(|e| ReportError(e.to_string()))
}

fn allow_null_numbers(v: &mut serde_json::Value) {
    use serde_json::Value;
    match v {
        Value::Object(map) => {
            match map.get("type").cloned() {
                Some(Value::String(t)) if t == "number" => {
                    map.insert("type".into(), serde_json::json!(["number", "null"]));
                }
                Some(Value::Array(ts))
                    if ts.iter().any(|t| t == "number") && !ts.iter().any(|t| t == "null") =>
                {
                    let mut ts = ts;
                    ts.push(Value::String("null".into()));
                    map.insert("type".into(), Value::Array(ts));
                }
                _ => {}
            }
            for (_, child) in map.iter_mut() {
                allow_null_numbers(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(allow_null_numbers),
        _ => {}
    }
}
