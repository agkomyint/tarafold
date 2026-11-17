//! Run configuration. Every tunable lives here with its default, so `run.toml` fully
//! describes a run.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::evidence::DataSource;
use crate::split::SplitStrategy;

/// Full configuration for one pipeline run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RunConfig {
    /// Target column(s).
    pub targets: Vec<String>,
    /// Do not use any other target as a feature. By default a target may use targets listed
    /// before it in `targets` (so chained equations follow the given order and cannot cycle).
    pub exclude_other_targets: bool,
    /// Run seed. All randomness derives from it.
    pub seed: u64,
    /// Declared data source.
    pub data_source: DataSource,
    /// Columns set deliberately by the experimenter or simulator (R9.3). Empty means the
    /// default: controllables for experimental data, all features for simulation data.
    pub designed: Vec<String>,
    /// Split settings.
    pub split: SplitConfig,
    /// Discovery settings.
    pub discover: DiscoverConfig,
    /// Reliability settings.
    pub validate: ValidateConfig,
    /// Optimization settings; inactive when `goal` is unset.
    pub optimize: OptimizeConfig,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            exclude_other_targets: false,
            seed: 42,
            data_source: DataSource::Observational,
            designed: Vec::new(),
            split: SplitConfig::default(),
            discover: DiscoverConfig::default(),
            validate: ValidateConfig::default(),
            optimize: OptimizeConfig::default(),
        }
    }
}

/// Train/validation/test settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SplitConfig {
    /// Fraction of rows for training.
    pub train: f64,
    /// Fraction of rows for validation.
    pub val: f64,
    /// Random or ordered.
    pub strategy: SplitStrategy,
}

impl Default for SplitConfig {
    fn default() -> Self {
        Self {
            train: 0.6,
            val: 0.2,
            strategy: SplitStrategy::Random,
        }
    }
}

/// Equation discovery settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DiscoverConfig {
    /// Maximum features passed to discovery.
    pub max_features: usize,
    /// Maximum training rows used by discovery (subsampled beyond this).
    pub max_rows: usize,
    /// Maximum basis terms in sparse regression.
    pub max_basis: usize,
    /// Maximum non-zero terms per sparse-regression candidate.
    pub max_terms: usize,
    /// Maximum Pareto-front size.
    pub max_front: usize,
    /// Override the selected equation by front id.
    pub select: Option<usize>,
    /// Genetic-programming search settings.
    pub gp: GpConfig,
}

impl Default for DiscoverConfig {
    fn default() -> Self {
        Self {
            max_features: 12,
            max_rows: 5_000,
            max_basis: 400,
            max_terms: 12,
            max_front: 20,
            select: None,
            gp: GpConfig::default(),
        }
    }
}

/// Reliability settings (R7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ValidateConfig {
    /// Bootstrap resamples for constant and prediction intervals (0 disables).
    pub bootstrap: usize,
    /// Reduced-budget rediscoveries for structural stability (0 disables; caps the grade).
    pub stability: usize,
    /// Share of the GP budget each stability rediscovery uses.
    pub stability_budget: f64,
}

impl Default for ValidateConfig {
    fn default() -> Self {
        Self {
            bootstrap: 200,
            stability: 10,
            stability_budget: 0.1,
        }
    }
}

/// Genetic-programming symbolic regression settings (design §5b).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct GpConfig {
    /// Run the GP stage.
    pub enabled: bool,
    /// Independent populations, evolved in parallel and joined by migration.
    pub islands: usize,
    /// Expressions per island.
    pub population: usize,
    /// Total expression evaluations across islands. Bounds the search deterministically.
    pub max_evals: usize,
    /// Training rows used for fitness (subsampled beyond this).
    pub rows: usize,
    /// Largest complexity allowed.
    pub max_complexity: usize,
    /// Evolution steps per island between migrations.
    pub epoch_steps: usize,
    /// Unary operators the search may use (`+ - * /` are always available). Supported:
    /// `exp log sqrt square abs sin cos tanh inv`.
    pub operators: Vec<String>,
}

impl Default for GpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            islands: 8,
            population: 64,
            max_evals: 240_000,
            rows: 800,
            max_complexity: 30,
            epoch_steps: 250,
            operators: ["exp", "log", "sqrt", "square", "inv"]
                .map(String::from)
                .to_vec(),
        }
    }
}

/// Target-seeking settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct OptimizeConfig {
    /// Desired target value. Optimization runs only when set.
    pub goal: Option<f64>,
    /// Columns the user can change. Empty means every feature used by the equation.
    pub controllables: Vec<String>,
    /// Per-column bounds `[lo, hi]`; default is the observed training range.
    pub bounds: BTreeMap<String, [f64; 2]>,
    /// Per-column fixed values for non-controllables; default is the training median.
    pub fixed: BTreeMap<String, f64>,
    /// Number of diverse candidates to return.
    pub n_candidates: usize,
    /// Success tolerance as a fraction of the target's observed range.
    pub tolerance: f64,
    /// Maximum objective evaluations for multi-dimensional search.
    pub max_evals: usize,
    /// Linear constraints, e.g. `a + b <= 5000`.
    pub constraints: Vec<String>,
    /// Controllables restricted to whole numbers.
    pub integer: Vec<String>,
    /// Controllables restricted to multiples of a step.
    pub steps: BTreeMap<String, f64>,
    /// Largest allowed change from baseline, as a fraction (0.2 = ±20%).
    pub max_change: BTreeMap<String, f64>,
    /// Hard goals (`--require`).
    pub require: Vec<String>,
    /// Soft goals and objectives (`--prefer`).
    pub prefer: Vec<String>,
    /// Minimum joint probability of meeting all hard goals.
    pub robust: Option<f64>,
    /// Pick the preferred trade-off lexicographically instead of by weights.
    pub lexicographic: bool,
}

impl Default for OptimizeConfig {
    fn default() -> Self {
        Self {
            goal: None,
            controllables: Vec::new(),
            bounds: BTreeMap::new(),
            fixed: BTreeMap::new(),
            n_candidates: 5,
            tolerance: 0.01,
            max_evals: 200_000,
            constraints: Vec::new(),
            integer: Vec::new(),
            steps: BTreeMap::new(),
            max_change: BTreeMap::new(),
            require: Vec::new(),
            prefer: Vec::new(),
            robust: None,
            lexicographic: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_roundtrip_and_partial() {
        let mut c = RunConfig {
            targets: vec!["y".into()],
            ..Default::default()
        };
        c.optimize.goal = Some(10.0);
        c.optimize.bounds.insert("x".into(), [0.0, 5.0]);
        let s = toml::to_string_pretty(&c).unwrap();
        let back: RunConfig = toml::from_str(&s).unwrap();
        assert_eq!(c, back);
        let partial: RunConfig = toml::from_str("seed = 7\n[discover]\nmax_terms = 4\n").unwrap();
        assert_eq!(partial.seed, 7);
        assert_eq!(partial.discover.max_terms, 4);
        assert_eq!(partial.discover.max_rows, 5_000);
        assert!(toml::from_str::<RunConfig>("sed = 1").is_err());
    }
}
