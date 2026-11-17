//! Structure-recovery rate across data seeds, to separate real regressions from seed noise.
//!
//! cargo run --release -p tarafold-bench --example seed_sweep -- <dataset> [rows] [noise] [first_seed] [n_seeds]
#![allow(clippy::unwrap_used)]

use tarafold_bench::{Case, run_case};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let name = a.first().cloned().unwrap_or_else(|| "exp-log".into());
    let rows: usize = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let noise: f64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.1);
    let first: u64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
    let n: u64 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(8);
    let ok = (first..first + n)
        .filter(|&seed| {
            run_case(&Case {
                dataset: name.clone(),
                rows,
                noise,
                distractors: 0,
                seed,
            })
            .structure
        })
        .count();
    println!(
        "{name} rows={rows} noise={noise} seeds {first}..{} structure {ok}/{n}",
        first + n - 1
    );
}
