//! Error type for core helpers.

/// Errors raised by core helpers.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// A column name was not found in the dataset.
    #[error("column `{0}` not found")]
    UnknownColumn(String),
    /// Configuration values are inconsistent.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Result alias for core helpers.
pub type Result<T> = std::result::Result<T, CoreError>;
