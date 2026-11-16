//! Report schema (R10.1), JSON and CSV writers, and user-facing text.
//!
//! The JSON Schema printed by `tarafold schema` is generated from these types, so the
//! schema and the writer cannot drift.

pub mod text;

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tarafold_core::{DataSource, Evidence, RunConfig, SourceInfo};
use tarafold_discover::{Strategy, StrategyRun};
use tarafold_expr::Tree;
use tarafold_io::{Cell, write_csv};
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

/// Feature screening decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScreenDecision {
    /// Feature.
    pub feature: String,
    /// Kept for discovery.
    pub kept: bool,
    /// Reason.
    pub reason: String,
}

/// Results for one target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TargetReport {
    /// Target column.
    pub target: String,
    /// Features passed to discovery.
    pub features: Vec<String>,
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
    /// Training-data domain.
    pub domain: Domain,
}

/// Optimization results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OptimizeReport {
    /// Target.
    pub target: String,
    /// Goal value.
    pub goal: f64,
    /// Absolute tolerance.
    pub tolerance: f64,
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
    /// Disclaimer.
    pub disclaimer: String,
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
    /// Pretty JSON.
    pub fn to_json(&self) -> Result<String, ReportError> {
        serde_json::to_string_pretty(self).map_err(|e| ReportError(e.to_string()))
    }

    /// `equations.csv`: every front equation of every target.
    pub fn equations_csv(&self) -> Result<String, ReportError> {
        let headers: Vec<String> = [
            "target", "id", "selected", "equation", "complexity", "strategy", "r2_train", "r2_val",
            "r2_test", "rmse_test",
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

    /// `candidates.csv`: optimization candidates (header only when there are none).
    pub fn candidates_csv(&self) -> Result<String, ReportError> {
        let Some(o) = &self.optimize else {
            return write_csv(&["id".into()], &[]).map_err(|e| ReportError(e.to_string()));
        };
        let mut headers = vec!["id".to_string()];
        headers.extend(o.controllables.iter().cloned());
        headers.extend(
            ["predicted", "interval_lo", "interval_hi", "error", "within_tolerance", "in_domain"]
                .iter()
                .map(|s| s.to_string()),
        );
        let rows: Vec<Vec<Cell>> = o
            .candidates
            .iter()
            .map(|c| {
                let mut r = vec![Cell::Num(c.id as f64)];
                r.extend(o.controllables.iter().map(|n| Cell::Num(c.inputs.get(n).copied().unwrap_or(f64::NAN))));
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
pub fn schema_json() -> Result<String, ReportError> {
    let schema = schemars::schema_for!(Report);
    serde_json::to_string_pretty(&schema).map_err(|e| ReportError(e.to_string()))
}
