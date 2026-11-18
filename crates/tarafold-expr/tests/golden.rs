//! Checks the engine evaluator against the shared golden vectors in `golden.json`.
//!
//! Each case also stores the JSON expression tree the browser evaluator reads. The tree must
//! equal the engine's `to_tree` output; run with `TARAFOLD_BLESS=1` to rewrite the trees after
//! adding a case.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde::{Deserialize, Serialize};
use tarafold_expr::{Tree, parse};

#[derive(Deserialize, Serialize)]
struct Golden {
    description: String,
    version: u32,
    tolerance: f64,
    cases: Vec<Case>,
}

#[derive(Deserialize, Serialize)]
struct Case {
    expr: String,
    variables: Vec<String>,
    points: Vec<Vec<f64>>,
    expected: Vec<Option<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tree: Option<Tree>,
}

#[test]
fn engine_matches_golden_vectors() {
    let text = include_str!("golden.json");
    let mut g: Golden = serde_json::from_str(text).unwrap();
    let bless = std::env::var_os("TARAFOLD_BLESS").is_some();
    let mut checked = 0;
    for c in &mut g.cases {
        let e = parse(&c.expr, &c.variables).unwrap_or_else(|err| panic!("{}: {err}", c.expr));
        let tree = e.to_tree(&c.variables);
        if bless {
            c.tree = Some(tree);
        } else {
            assert_eq!(
                c.tree.as_ref(),
                Some(&tree),
                "{}: stored tree differs; run with TARAFOLD_BLESS=1",
                c.expr
            );
        }
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
    if bless {
        let mut out = String::from("{\n");
        out += &format!(
            "  \"description\": {},\n  \"version\": {},\n  \"tolerance\": {},\n  \"cases\": [\n",
            serde_json::to_string(&g.description).unwrap(),
            g.version,
            serde_json::to_string(&g.tolerance).unwrap()
        );
        let lines: Vec<String> = g
            .cases
            .iter()
            .map(|c| format!("    {}", serde_json::to_string(c).unwrap()))
            .collect();
        out += &lines.join(",\n");
        out += "\n  ]\n}\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden.json");
        std::fs::write(path, out).unwrap();
    }
}
