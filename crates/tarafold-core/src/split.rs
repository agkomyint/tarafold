//! Train / validation / test splits (R7.1).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::rng::{Rng, stream};

/// How rows are assigned to splits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SplitStrategy {
    /// Shuffle rows with the run seed (default).
    #[default]
    Random,
    /// Keep row order: first rows train, last rows test. Use for time-ordered data.
    Ordered,
}

/// Row indices for each split.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Split {
    /// Training rows.
    pub train: Vec<u32>,
    /// Validation rows (model selection).
    pub val: Vec<u32>,
    /// Test rows (final reporting only).
    pub test: Vec<u32>,
    /// Strategy used.
    pub strategy: SplitStrategy,
}

impl Split {
    /// Split `n` rows with fractions `train` and `val`; the rest is test.
    pub fn new(n: usize, train: f64, val: f64, strategy: SplitStrategy, seed: u64) -> Result<Self> {
        if !(0.0..1.0).contains(&train) || !(0.0..1.0).contains(&val) || train + val >= 1.0 {
            return Err(CoreError::InvalidConfig(format!(
                "split fractions train={train}, val={val} must be in (0,1) and sum to < 1"
            )));
        }
        let n32 = u32::try_from(n)
            .map_err(|_| CoreError::InvalidConfig("too many rows for u32 indices".into()))?;
        let mut idx: Vec<u32> = (0..n32).collect();
        if strategy == SplitStrategy::Random {
            Rng::derive(seed, &[stream::SPLIT]).shuffle(&mut idx);
        }
        let n_train = ((n as f64) * train).round() as usize;
        let n_val = ((n as f64) * val).round() as usize;
        let n_val = n_val.min(n.saturating_sub(n_train));
        let test = idx.split_off(n_train + n_val);
        let val_rows = idx.split_off(n_train);
        let mut s = Self {
            train: idx,
            val: val_rows,
            test,
            strategy,
        };
        if strategy == SplitStrategy::Random {
            s.train.sort_unstable();
            s.val.sort_unstable();
            s.test.sort_unstable();
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reproducible_and_disjoint() {
        let a = Split::new(1000, 0.6, 0.2, SplitStrategy::Random, 9).unwrap();
        let b = Split::new(1000, 0.6, 0.2, SplitStrategy::Random, 9).unwrap();
        assert_eq!(a, b);
        assert_eq!((a.train.len(), a.val.len(), a.test.len()), (600, 200, 200));
        let mut all: Vec<u32> = a
            .train
            .iter()
            .chain(&a.val)
            .chain(&a.test)
            .copied()
            .collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), 1000);
        let c = Split::new(1000, 0.6, 0.2, SplitStrategy::Random, 10).unwrap();
        assert_ne!(a.train, c.train);
    }

    #[test]
    fn ordered_keeps_order() {
        let s = Split::new(10, 0.6, 0.2, SplitStrategy::Ordered, 1).unwrap();
        assert_eq!(s.train, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(s.test, vec![8, 9]);
    }
}
