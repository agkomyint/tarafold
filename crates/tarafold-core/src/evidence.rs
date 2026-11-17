//! Evidence levels and data-source declarations (R9).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How strong a claim a result supports. There is deliberately no `Causal` level in v0.1.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    /// Variables move together.
    Association,
    /// A model predicts held-out data better than the mean.
    Predictive,
    /// An interpretable equation fits held-out data and passes reliability checks.
    FunctionalFit,
    /// As `FunctionalFit`, on user-declared designed data, inside the data domain.
    InterventionalFit,
}

/// How the data was produced, as declared by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    /// Values were observed, not set (default).
    #[default]
    Observational,
    /// Some inputs were set deliberately in experiments.
    Experimental,
    /// Rows come from a simulator whose inputs were set deliberately.
    Simulation,
}
