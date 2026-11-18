//! Shared types, configuration, seeded randomness and numeric helpers used by every
//! Tarafold stage. This crate has no I/O.

pub mod config;
pub mod data;
pub mod error;
pub mod evidence;
pub mod linalg;
pub mod rng;
pub mod split;
pub mod stats;
pub mod stop;

pub use config::RunConfig;
pub use data::{Column, Dataset, ExcludedColumn, SourceInfo};
pub use error::{CoreError, Result};
pub use evidence::{DataSource, Evidence};
pub use rng::Rng;
pub use split::{Split, SplitStrategy};
