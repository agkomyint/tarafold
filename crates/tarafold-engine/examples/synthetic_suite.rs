//! Runs every synthetic dataset through the full pipeline and scores the results against the
//! hidden true equation. Preview of `tarafold bench` (Phase 2).
//!
//! ```sh
//! cargo run --release -p tarafold-engine --example synthetic_suite
//! ```
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Instant;

use tarafold_core::RunConfig;
use tarafold_expr::parse;
use tarafold_io::{IngestOptions, read_csv};
use tarafold_synth::{generate, registry};

fn main() {
    let rows = 2000;
    println!(
        "| dataset | noise | exact | structure | R² fresh data | inverse success | time | selected equation |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    let (mut n_rec, mut n_struct, mut n_acc, mut n_inv_ok, mut n_inv, mut n_runs) =
        (0, 0, 0, 0, 0, 0);
    for entry in registry() {
        let distractors = if entry.name == "interaction" { 5 } else { 0 };
        for noise in [0.0, 0.01, 0.1] {
            let g = generate(&entry, rows, noise, distractors, 1).unwrap();
            let data = read_csv(
                g.to_csv().unwrap().as_bytes(),
                entry.name,
                &IngestOptions::default(),
            )
            .unwrap();
            let mut cfg = RunConfig {
                targets: vec![entry.target.into()],
                ..Default::default()
            };
            cfg.optimize.goal = Some(entry.example_goal);
            cfg.optimize.controllables =
                entry.controllables.iter().map(|s| s.to_string()).collect();
            let t0 = Instant::now();
            let (report, _) = tarafold_engine::run(&data, &cfg).unwrap();
            let secs = t0.elapsed().as_secs_f64();
            let t = &report.targets[0];
            let sel = &t.discover.front[t.discover.selected_id];
            let found = parse(&sel.infix_exact, &t.features).unwrap();
            let truth = parse(&g.truth.equation, &g.truth.features).unwrap();

            // Fresh noiseless data from a different seed, same distractor layout.
            let fresh = generate(&entry, 1000, 0.0, distractors, 99).unwrap();
            let col = |name: &str| -> &[f64] {
                let i = fresh.headers.iter().position(|h| h == name).unwrap();
                &fresh.columns[i]
            };
            let f_refs: Vec<&[f64]> = t.features.iter().map(|n| col(n)).collect();
            let t_refs: Vec<&[f64]> = g.truth.features.iter().map(|n| col(n)).collect();
            let a = found.eval_columns(&f_refs, 1000).unwrap();
            let b = truth.eval_columns(&t_refs, 1000).unwrap();
            let max_rel = a
                .iter()
                .zip(&b)
                .map(|(x, y)| (x - y).abs() / y.abs().max(1e-9))
                .fold(0.0, f64::max);
            let recovered = max_rel < 1e-3;
            let r2 = tarafold_validate::metrics(&a, &b).r2;
            // Structure check: refit the found equation's constants on fresh noiseless data; the
            // structure is right if it then matches the truth.
            let fx: Vec<Vec<f64>> = f_refs.iter().map(|c| c.to_vec()).collect();
            let refit = tarafold_discover::refit_constants(&found, &fx, &b);
            let ra = refit.eval_columns(&f_refs, 1000).unwrap();
            let structure = recovered
                || ra
                    .iter()
                    .zip(&b)
                    .map(|(x, y)| (x - y).abs() / y.abs().max(1e-9))
                    .fold(0.0, f64::max)
                    < 1e-3;

            // Inverse success: evaluate the TRUE equation at every within-tolerance candidate.
            let o = report.optimize.as_ref().unwrap();
            let mut ok = 0;
            let mut total = 0;
            for c in o.candidates.iter().filter(|c| c.within_tolerance) {
                let x: Vec<f64> = g
                    .truth
                    .features
                    .iter()
                    .map(|n| c.inputs.get(n).or(o.baseline.get(n)).copied().unwrap())
                    .collect();
                total += 1;
                if (truth.eval_point(&x) - o.goal).abs() <= o.tolerance {
                    ok += 1;
                }
            }
            n_runs += 1;
            n_rec += usize::from(recovered);
            n_struct += usize::from(structure);
            n_acc += usize::from(r2 > 0.999);
            n_inv += total;
            n_inv_ok += ok;
            let mut eq = sel.infix.clone();
            if eq.chars().count() > 60 {
                eq = eq.chars().take(57).collect::<String>() + "...";
            }
            println!(
                "| {} | {noise} | {} | {} | {r2:.6} | {ok}/{total} | {secs:.2}s | `{eq}` |",
                entry.name,
                if recovered { "yes" } else { "no" },
                if structure { "yes" } else { "no" },
            );
        }
    }
    println!(
        "\nExact recovery {n_rec}/{n_runs}. Correct structure {n_struct}/{n_runs}. R² > 0.999 on fresh data {n_acc}/{n_runs}.\nInverse success {n_inv_ok}/{n_inv} candidates \
         (true equation within tolerance of the goal)."
    );
}
