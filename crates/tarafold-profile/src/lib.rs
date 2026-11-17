//! Column profiling (R4): moments, quantiles, histograms, robust outlier counts, and flags.

use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tarafold_core::{Dataset, stats};

/// Selected quantiles.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Quantiles {
    /// 1st percentile.
    pub p01: f64,
    /// 5th percentile.
    pub p05: f64,
    /// 25th percentile.
    pub p25: f64,
    /// Median.
    pub p50: f64,
    /// 75th percentile.
    pub p75: f64,
    /// 95th percentile.
    pub p95: f64,
    /// 99th percentile.
    pub p99: f64,
}

/// Equal-width histogram between the 0.5% and 99.5% quantiles, with overflow counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Histogram {
    /// Bin edges (`counts.len() + 1` values).
    pub edges: Vec<f64>,
    /// Counts per bin.
    pub counts: Vec<u64>,
    /// Values below the first edge.
    pub below: u64,
    /// Values above the last edge.
    pub above: u64,
}

/// Profile of one numeric column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColumnProfile {
    /// Column name.
    pub name: String,
    /// Values.
    pub count: usize,
    /// Minimum.
    pub min: f64,
    /// Maximum.
    pub max: f64,
    /// Mean.
    pub mean: f64,
    /// Standard deviation (population).
    pub std: f64,
    /// Quantiles.
    pub quantiles: Quantiles,
    /// Skewness (population).
    pub skewness: f64,
    /// Distinct values.
    pub unique: usize,
    /// Values with robust z-score `0.6745·|x − median| / MAD > 5`.
    pub outliers: usize,
    /// Every value is > 0.
    pub all_positive: bool,
    /// Every value is within 1e-9 of an integer.
    pub integer_valued: bool,
    /// Every value is in `[0, 1]`.
    pub bounded_unit: bool,
    /// Histogram.
    pub histogram: Histogram,
}

const BINS: usize = 50;

/// Profile one column of finite values.
pub fn profile_column(name: &str, values: &[f64]) -> ColumnProfile {
    let n = values.len();
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let q = |p: f64| stats::quantile_sorted(&sorted, p);
    let mean = stats::mean(values);
    let (mut m2, mut m3) = (0.0, 0.0);
    for v in values {
        let d = v - mean;
        m2 += d * d;
        m3 += d * d * d;
    }
    let nf = n.max(1) as f64;
    m2 /= nf;
    m3 /= nf;
    let skewness = if m2 > 0.0 { m3 / m2.powf(1.5) } else { 0.0 };
    let unique = if sorted.is_empty() {
        0
    } else {
        1 + sorted.windows(2).filter(|w| w[0] != w[1]).count()
    };
    let median = q(0.5);
    let mut dev: Vec<f64> = values.iter().map(|v| (v - median).abs()).collect();
    dev.sort_by(f64::total_cmp);
    let mad = stats::quantile_sorted(&dev, 0.5);
    let outliers = if mad > 0.0 {
        values
            .iter()
            .filter(|v| 0.6745 * (*v - median).abs() / mad > 5.0)
            .count()
    } else {
        0
    };

    let (lo, hi) = (q(0.005), q(0.995));
    let histogram = if n == 0 {
        Histogram {
            edges: Vec::new(),
            counts: Vec::new(),
            below: 0,
            above: 0,
        }
    } else if hi > lo {
        let w = (hi - lo) / BINS as f64;
        let mut counts = vec![0u64; BINS];
        let (mut below, mut above) = (0, 0);
        for &v in values {
            if v < lo {
                below += 1;
            } else if v > hi {
                above += 1;
            } else {
                counts[(((v - lo) / w) as usize).min(BINS - 1)] += 1;
            }
        }
        Histogram {
            edges: (0..=BINS).map(|i| lo + w * i as f64).collect(),
            counts,
            below,
            above,
        }
    } else {
        // Nearly constant column: one bin around the value.
        let (b, a) = (
            values.iter().filter(|&&v| v < lo).count(),
            values.iter().filter(|&&v| v > hi).count(),
        );
        Histogram {
            edges: vec![lo, hi],
            counts: vec![(n - a - b) as u64],
            below: b as u64,
            above: a as u64,
        }
    };
    ColumnProfile {
        name: name.to_string(),
        count: n,
        min: sorted.first().copied().unwrap_or(f64::NAN),
        max: sorted.last().copied().unwrap_or(f64::NAN),
        mean,
        std: m2.sqrt(),
        quantiles: Quantiles {
            p01: q(0.01),
            p05: q(0.05),
            p25: q(0.25),
            p50: median,
            p75: q(0.75),
            p95: q(0.95),
            p99: q(0.99),
        },
        skewness,
        unique,
        outliers,
        all_positive: values.iter().all(|&v| v > 0.0),
        integer_valued: values.iter().all(|v| (v - v.round()).abs() <= 1e-9),
        bounded_unit: values.iter().all(|v| (0.0..=1.0).contains(v)),
        histogram,
    }
}

/// Profile every column (in parallel; order is preserved).
pub fn profile(data: &Dataset) -> Vec<ColumnProfile> {
    data.columns
        .par_iter()
        .map(|c| profile_column(&c.name, &c.values))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_profile() {
        let mut v: Vec<f64> = (1..=100).map(f64::from).collect();
        v.push(10_000.0);
        let p = profile_column("x", &v);
        assert_eq!(p.count, 101);
        assert_eq!(p.unique, 101);
        assert_eq!(p.outliers, 1);
        assert!(p.all_positive && p.integer_valued && !p.bounded_unit);
        assert!(p.skewness > 5.0);
        assert_eq!(p.histogram.counts.len(), 50);
        let total: u64 =
            p.histogram.counts.iter().sum::<u64>() + p.histogram.below + p.histogram.above;
        assert_eq!(total, 101);
        assert_eq!(p.quantiles.p50, 51.0);
    }

    #[test]
    fn unit_and_constant() {
        let p = profile_column("rate", &[0.1, 0.5, 0.9, 0.5]);
        assert!(p.bounded_unit && !p.integer_valued);
        let c = profile_column("c", &[3.0; 10]);
        assert_eq!(c.unique, 1);
        assert_eq!(c.histogram.counts, vec![10]);
        assert_eq!(c.outliers, 0);
    }

    #[test]
    fn large_column_is_fast() {
        let v: Vec<f64> = (0..1_000_000).map(|i| f64::from(i % 9973) * 0.37).collect();
        let t = std::time::Instant::now();
        let p = profile_column("big", &v);
        assert_eq!(p.count, 1_000_000);
        assert!(t.elapsed().as_secs_f64() < 5.0);
    }
}
