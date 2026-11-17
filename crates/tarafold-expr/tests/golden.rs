//! Checks the engine evaluator against the shared golden vectors in `golden.json`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde::Deserialize;
use tarafold_expr::parse;

#[derive(Deserialize)]
struct Golden {
    tolerance: f64,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    expr: String,
    variables: Vec<String>,
    points: Vec<Vec<f64>>,
    expected: Vec<Option<f64>>,
}

#[test]
fn engine_matches_golden_vectors() {
    let text = include_str!("golden.json");
    let g: Golden = serde_json::from_str(text).unwrap();
    let mut checked = 0;
    for c in &g.cases {
        let e = parse(&c.expr, &c.variables).unwrap_or_else(|err| panic!("{}: {err}", c.expr));
        assert_eq!(c.points.len(), c.expected.len(), "{}", c.expr);
        for (p, want) in c.points.iter().zip(&c.expected) {
            let got = e.eval_point(p);
            match want {
                None => assert!(
                    !got.is_finite(),
                    "{} at {p:?}: expected undefined, got {got}",
                    c.expr
                ),
                Some(w) => assert!(
                    (got - w).abs() <= g.tolerance * w.abs().max(1.0),
                    "{} at {p:?}: expected {w}, got {got}",
                    c.expr
                ),
            }
            checked += 1;
        }
    }
    assert!(checked >= 30, "{checked}");
}
