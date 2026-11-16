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
    let out = discover(&DiscoverInput { train_x: &tx, train_y: &ty, val_x: &vx, val_y: &vy, config: &cfg })
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
    assert!(o.equivalent, "interaction with distractors: found {}", o.equation);
}

#[test]
fn noisy_data_stays_accurate() {
    for name in ["linear", "cobb-douglas", "business-revenue", "kinetic-energy"] {
        let o = run(name, 600, 0.1, 0);
        // Noise std 0.1 × std(y) means the best possible NMSE is about 0.01.
        assert!(o.test_nmse < 0.02, "{name}: test NMSE {}", o.test_nmse);
    }
}

#[test]
fn hard_forms_get_reasonable_approximations() {
    // Not exactly expressible by sparse regression; GP (Phase 3) is expected to recover them.
    for name in ["rational", "exp-log"] {
        let o = run(name, 600, 0.0, 0);
        assert!(o.test_nmse < 0.05, "{name}: test NMSE {}", o.test_nmse);
    }
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
