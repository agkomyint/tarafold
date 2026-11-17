//! Measures how often GP recovers the non-basis forms for several budgets and seeds.
#![allow(clippy::unwrap_used)]

use std::time::Instant;

use tarafold_core::config::DiscoverConfig;
use tarafold_core::{Split, SplitStrategy, stats};
use tarafold_discover::{DiscoverInput, discover};
use tarafold_expr::parse;
use tarafold_synth::{find, generate};

fn main() {
    let budgets: Vec<usize> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let budgets = if budgets.is_empty() {
        vec![240_000]
    } else {
        budgets
    };
    for evals in budgets {
        for name in ["rational", "exp-log"] {
            let mut ok = 0;
            let t0 = Instant::now();
            let seeds = 20;
            for seed in 0..seeds {
                let g = generate(&find(name).unwrap(), 600, 0.0, 0, 11 + seed).unwrap();
                let split = Split::new(600, 0.6, 0.2, SplitStrategy::Random, 3).unwrap();
                let pick = |rows: &[u32], j: usize| stats::gather(&g.columns[j], rows);
                let tx: Vec<Vec<f64>> = (0..2).map(|j| pick(&split.train, j)).collect();
                let vx: Vec<Vec<f64>> = (0..2).map(|j| pick(&split.val, j)).collect();
                let sx: Vec<Vec<f64>> = (0..2).map(|j| pick(&split.test, j)).collect();
                let (ty, vy) = (pick(&split.train, 2), pick(&split.val, 2));
                let mut cfg = DiscoverConfig::default();
                cfg.gp.max_evals = evals;
                let out = discover(&DiscoverInput {
                    train_x: &tx,
                    train_y: &ty,
                    val_x: &vx,
                    val_y: &vy,
                    config: &cfg,
                    seed,
                })
                .unwrap();
                let sel = &out.front[out.selected].expr;
                let truth = parse(&g.truth.equation, &g.truth.features).unwrap();
                let refs: Vec<&[f64]> = sx.iter().map(Vec::as_slice).collect();
                let a = sel.eval_columns(&refs, sx[0].len()).unwrap();
                let b = truth.eval_columns(&refs, sx[0].len()).unwrap();
                let rel = a
                    .iter()
                    .zip(&b)
                    .map(|(x, y)| (x - y).abs() / y.abs().max(1e-9))
                    .fold(0.0, f64::max);
                ok += usize::from(rel < 1e-3);
            }
            println!(
                "evals={evals:>9} {name:9} recovered {ok}/{seeds}  avg {:.1}s",
                t0.elapsed().as_secs_f64() / seeds as f64
            );
        }
    }
}
