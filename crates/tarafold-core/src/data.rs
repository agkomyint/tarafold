//! In-memory numeric dataset.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// One numeric column. Missing values are stored as `NaN` until rows are dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    /// Column name from the CSV header.
    pub name: String,
    /// Values, one per row.
    pub values: Vec<f64>,
}

/// A column that was not used, with the reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExcludedColumn {
    /// Column name.
    pub name: String,
    /// Machine-readable reason code, e.g. `non_numeric`, `constant`, `duplicate_of:x`.
    pub reason: String,
}

/// Where the data came from.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct SourceInfo {
    /// File path, or `stdin`.
    pub path: String,
    /// SHA-256 of the raw input bytes, lowercase hex.
    pub sha256: String,
    /// Detected field delimiter.
    pub delimiter: String,
    /// Detected decimal separator.
    pub decimal: String,
    /// Rows read before dropping missing values.
    pub rows_read: usize,
    /// Rows dropped because of missing values in used columns.
    pub rows_dropped_missing: usize,
    /// Columns that were not used.
    pub excluded: Vec<ExcludedColumn>,
}

/// A numeric dataset: equal-length columns plus provenance.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dataset {
    /// Numeric columns.
    pub columns: Vec<Column>,
    /// Optional per-row weights (already validated and normalized to mean 1).
    pub weights: Option<Vec<f64>>,
    /// Number of rows.
    pub n_rows: usize,
    /// Provenance.
    pub source: SourceInfo,
}

impl Dataset {
    /// Index of the column with this name.
    pub fn index_of(&self, name: &str) -> Result<usize> {
        self.columns
            .iter()
            .position(|c| c.name == name)
            .ok_or_else(|| CoreError::UnknownColumn(name.to_string()))
    }

    /// Column by name.
    pub fn column(&self, name: &str) -> Result<&Column> {
        Ok(&self.columns[self.index_of(name)?])
    }

    /// Column names in order.
    pub fn names(&self) -> Vec<String> {
        self.columns.iter().map(|c| c.name.clone()).collect()
    }
}
