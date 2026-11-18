use tarafold_core::config::DiscoverConfig;
use tarafold_core::{Split, SplitStrategy};
use tarafold_expr::parse;
use tarafold_synth::{find, generate};

use super::*;

struct Outcome {
    equation: String,
    test_nmse: f64,
    equivalent: bool,
}

/// Generate, split, discover, and compare the selected equation with the truth.
fn run(name: &str, rows: usize, noise: f64, distractors: usize) -> Outcome {
    run_seeded(name, rows, noise, distractors, 1)
}

fn run_seeded(name: &str, rows: usize, noise: f64, distractors: usize, seed: u64) -> Outcome {
    let g = generate(&find(name).unwrap(), rows, noise, distractors, 11).unwrap();
    let p = g.truth.features.len();
    let split = Split::new(rows, 0.6, 0.2, SplitStrategy::Random, 3).unwrap();
    let pick = |rows: &[u32], j: usize| -> Vec<f64> { stats::gather(&g.columns[j], rows) };
    let tx: Vec<Vec<f64>> = (0..p).map(|j| pick(&split.train, j)).collect();
    let vx: Vec<Vec<f64>> = (0..p).map(|j| pick(&split.val, j)).collect();
    let sx: Vec<Vec<f64>> = (0..p).map(|j| pick(&split.test, j)).collect();
    let ty = pick(&split.train, p);
    let vy = pick(&split.val, p);
    let sy = pick(&split.test, p);
    let cfg = DiscoverConfig::default();
    let out = discover(&DiscoverInput {
        train_x: &tx,
        train_y: &ty,
        val_x: &vx,
        val_y: &vy,
        train_w: None,
        val_w: None,
        config: &cfg,
        seed,
    })
    .unwrap();
    let sel = &out.front[out.selected];
    let truth = parse(&g.truth.equation, &g.truth.features).unwrap();
    // Numeric equivalence against the noiseless truth on test rows.
    let refs: Vec<&[f64]> = sx.iter().map(Vec::as_slice).collect();
    let a = sel.expr.eval_columns(&refs, sy.len()).unwrap();
    let b = truth.eval_columns(&refs, sy.len()).unwrap();
    let max_rel = a
        .iter()
        .zip(&b)
        .map(|(x, y)| (x - y).abs() / y.abs().max(1e-9))
        .fold(0.0, f64::max);
    let names = &g.truth.features;
    let o = Outcome {
        equation: sel.expr.to_infix_pretty(names, 4),
        test_nmse: nmse(&sel.expr, &sx, &sy),
        equivalent: max_rel < 1e-3,
    };
    println!(
        "{name:18} noise={noise:<5} truth: {:40} found: {:50} test NMSE={:.2e} equiv={}",
        g.truth.equation, o.equation, o.test_nmse, o.equivalent
    );
    o
}

#[test]
fn recovers_noiseless_phase1_targets() {
    // Phase 1 "done when": T1, T4, and T8 revenue recovered symbolically.
    for name in ["linear", "cobb-douglas", "business-revenue"] {
        let o = run(name, 600, 0.0, 0);
        assert!(o.equivalent, "{name}: found {}", o.equation);
    }
}

#[test]
fn recovers_other_basis_expressible_forms() {
    for name in ["polynomial", "kinetic-energy", "gravity", "business-direct"] {
        let o = run(name, 600, 0.0, 0);
        assert!(o.equivalent, "{name}: found {}", o.equation);
    }
    let o = run("interaction", 600, 0.0, 5);
    assert!(
        o.equivalent,
        "interaction with distractors: found {}",
        o.equation
    );
}

#[test]
fn noisy_data_stays_accurate() {
    for name in [
        "linear",
        "cobb-douglas",
        "business-revenue",
        "kinetic-energy",
    ] {
        let o = run(name, 600, 0.1, 0);
        // Noise std 0.1 × std(y) means the best possible NMSE is about 0.01.
        assert!(o.test_nmse < 0.02, "{name}: test NMSE {}", o.test_nmse);
    }
}

#[test]
fn genetic_search_recovers_non_basis_forms() {
    // Not expressible by sparse regression; found by the GP stage. GP is stochastic, so this
    // checks the recovery rate over several search seeds rather than one lucky seed.
    for name in ["rational", "exp-log"] {
        let hits = (0..5)
            .filter(|&s| run_seeded(name, 600, 0.0, 0, s).equivalent)
            .count();
        assert!(hits >= 4, "{name}: recovered {hits}/5");
    }
}

#[test]
fn genetic_search_is_thread_count_independent() {
    let g = generate(&find("rational").unwrap(), 300, 0.05, 0, 4).unwrap();
    let split = Split::new(300, 0.6, 0.2, SplitStrategy::Random, 3).unwrap();
    let pick = |rows: &[u32], j: usize| stats::gather(&g.columns[j], rows);
    let tx: Vec<Vec<f64>> = (0..2).map(|j| pick(&split.train, j)).collect();
    let vx: Vec<Vec<f64>> = (0..2).map(|j| pick(&split.val, j)).collect();
    let (ty, vy) = (pick(&split.train, 2), pick(&split.val, 2));
    let mut cfg = DiscoverConfig::default();
    cfg.gp.max_evals = 30_000;
    let run_with = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            let out = discover(&DiscoverInput {
                train_x: &tx,
                train_y: &ty,
                val_x: &vx,
                val_y: &vy,
                train_w: None,
                val_w: None,
                config: &cfg,
                seed: 9,
            })
            .unwrap();
            out.front
                .iter()
                .map(|c| format!("{:?}", c.expr.nodes()))
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(run_with(1), run_with(4));
}

#[test]
fn selection_prefers_simplest_exact() {
    let mk = |complexity, val| Candidate {
        expr: Expr::constant(0.0),
        complexity,
        train_nmse: val,
        val_nmse: val,
        strategy: Strategy::SparseLinear,
    };
    let front = vec![mk(1, 1.0), mk(5, 0.3), mk(9, 1e-25), mk(15, 1e-29)];
    assert_eq!(select(&front), 2);
}

/// Deterministic noisy data for y = x^2 on [0, 4].
fn square_data(n: usize) -> (Vec<Vec<f64>>, Vec<f64>) {
    let mut state = 12345_u64;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 11) as f64) / ((1_u64 << 53) as f64)
    };
    let x: Vec<f64> = (0..n).map(|_| 4.0 * next()).collect();
    let y: Vec<f64> = x.iter().map(|v| v * v + 0.02 * (next() - 0.5)).collect();
    (vec![x], y)
}

fn cand(expr: &str, complexity: usize, val: f64) -> Candidate {
    Candidate {
        expr: parse(expr, &["x".to_string()]).unwrap(),
        complexity,
        train_nmse: val,
        val_nmse: val,
        strategy: Strategy::Genetic,
    }
}

#[test]
fn extrapolation_error_separates_right_and_wrong_structure() {
    let (x, y) = square_data(1_000);
    let right = parse("0.9 * square(x)", &["x".to_string()]).unwrap();
    let wrong = parse("0.5 * exp(0.9 * x)", &["x".to_string()]).unwrap();
    let r = extrapolation_nmse(&right, &x, &y);
    let w = extrapolation_nmse(&wrong, &x, &y);
    assert!(r < 1e-3, "right structure tail error {r}");
    assert!(w > 10.0 * r.max(1e-6), "wrong {w} vs right {r}");
}

#[test]
fn guard_overrides_a_simpler_structure_that_fails_outside_the_range() {
    let (x, y) = square_data(1_000);
    // The exponential is simpler and looks equally good in range; only the square survives
    // holding out the ends of the range.
    let front = vec![
        cand("0.5 * exp(0.9 * x)", 5, 1.0e-3),
        cand("0.9 * square(x)", 6, 1.0e-3),
    ];
    assert_eq!(select(&front), 0, "unguarded rule picks the simpler one");
    assert_eq!(select_guarded(&front, &x, &y), 1);
}

#[test]
fn guard_keeps_the_simple_exact_choice() {
    let (x, y) = square_data(1_000);
    let front = vec![
        cand("0.9 * square(x)", 6, 1.0e-3),
        cand("0.9 * square(x) + 0.001 * x", 10, 1.0e-3),
    ];
    assert_eq!(select_guarded(&front, &x, &y), select(&front));
}

#[test]
fn guard_ignores_tiny_datasets() {
    let front = vec![cand("x", 2, 0.5), cand("square(x)", 3, 0.5)];
    let (x, y) = (vec![vec![1.0, 2.0, 3.0]], vec![1.0, 4.0, 9.0]);
    assert_eq!(select_guarded(&front, &x, &y), select(&front));
}
