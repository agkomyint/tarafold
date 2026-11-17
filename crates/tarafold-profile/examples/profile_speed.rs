//! Times profiling of 1,000,000 rows × 50 columns (R13.2 target: < 30 s).
#![allow(clippy::unwrap_used)]

use std::time::Instant;

use tarafold_core::{Column, Dataset, Rng};

fn main() {
    let mut r = Rng::derive(1, &[]);
    let n = 1_000_000;
    let columns: Vec<Column> = (0..50)
        .map(|j| Column { name: format!("c{j}"), values: (0..n).map(|_| r.normal() * (j + 1) as f64).collect() })
        .collect();
    let data = Dataset { columns, weights: None, n_rows: n, source: Default::default() };
    let t = Instant::now();
    let p = tarafold_profile::profile(&data);
    println!("profiled {} columns × {n} rows in {:.2}s", p.len(), t.elapsed().as_secs_f64());
}
