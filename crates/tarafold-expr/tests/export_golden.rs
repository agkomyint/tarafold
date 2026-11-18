//! Exported Python and JavaScript reproduce the golden vectors (R16.3). The interpreters are
//! optional locally; CI sets `TARAFOLD_REQUIRE_EXPORT_RUNTIMES=1` so a missing one fails.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;
use std::process::Command;

use serde::Deserialize;
use tarafold_expr::codegen::{JS_HELPERS, Lang, PYTHON_HELPERS, sanitize_identifiers};
use tarafold_expr::parse;

#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    expr: String,
    variables: Vec<String>,
    points: Vec<Vec<f64>>,
    expected: Vec<Option<f64>>,
}

fn golden() -> Golden {
    serde_json::from_str(include_str!("golden.json")).unwrap()
}

fn lit(v: f64, lang: Lang) -> String {
    if v.is_finite() {
        format!("({v:?})")
    } else {
        match lang {
            Lang::Js => "NaN".into(),
            _ => "math.nan".into(),
        }
    }
}

/// Program that prints one value per golden point, plus the expected values in order.
fn program(lang: Lang) -> (String, Vec<Option<f64>>) {
    let g = golden();
    let mut src = String::from(match lang {
        Lang::Python => PYTHON_HELPERS,
        _ => JS_HELPERS,
    });
    src.push('\n');
    let mut expected = Vec::new();
    for (ci, c) in g.cases.iter().enumerate() {
        let e = parse(&c.expr, &c.variables).unwrap();
        let ids = sanitize_identifiers(&c.variables, lang);
        let body = e.to_code(&ids, lang);
        match lang {
            Lang::Python => {
                let _ = writeln!(src, "\ndef f{ci}({}):\n    return {body}", ids.join(", "));
            }
            _ => {
                let _ = writeln!(
                    src,
                    "function f{ci}({}) {{ return {body}; }}",
                    ids.join(", ")
                );
            }
        }
        for (p, want) in c.points.iter().zip(&c.expected) {
            let args: Vec<String> = p.iter().map(|v| lit(*v, lang)).collect();
            match lang {
                Lang::Python => {
                    let _ = writeln!(src, "print(repr(f{ci}({})))", args.join(", "));
                }
                _ => {
                    let _ = writeln!(src, "console.log(String(f{ci}({})));", args.join(", "));
                }
            }
            expected.push(*want);
        }
    }
    (src, expected)
}

fn run(lang: Lang) {
    let (exe, ext): (&[&str], &str) = match lang {
        Lang::Python => (&["python3", "python"], "py"),
        _ => (&["node"], "js"),
    };
    let (src, expected) = program(lang);
    let path = std::env::temp_dir().join(format!(
        "tarafold-export-golden-{}.{ext}",
        std::process::id()
    ));
    std::fs::write(&path, &src).unwrap();
    let output = exe
        .iter()
        .find(|e| {
            Command::new(e)
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success())
        })
        .map(|e| Command::new(e).arg(&path).output().unwrap());
    let _ = std::fs::remove_file(&path);
    let Some(out) = output else {
        assert!(
            std::env::var_os("TARAFOLD_REQUIRE_EXPORT_RUNTIMES").is_none(),
            "{exe:?} not found"
        );
        eprintln!("skipping: {exe:?} not found");
        return;
    };
    assert!(
        out.status.success(),
        "{}\n{src}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let got: Vec<f64> = text
        .lines()
        .map(|l| l.trim().parse::<f64>().unwrap())
        .collect();
    assert_eq!(got.len(), expected.len());
    for (i, (g, w)) in got.iter().zip(&expected).enumerate() {
        match w {
            None => assert!(
                !g.is_finite(),
                "{lang:?} value {i}: expected undefined, got {g}"
            ),
            Some(w) => assert!(
                (g - w).abs() <= 1e-9 * w.abs().max(1.0),
                "{lang:?} value {i}: expected {w}, got {g}"
            ),
        }
    }
}

#[test]
fn python_export_matches_golden_vectors() {
    run(Lang::Python);
}

#[test]
fn js_export_matches_golden_vectors() {
    run(Lang::Js);
}

#[test]
fn sql_and_excel_keep_protected_semantics() {
    let names: Vec<String> = ["x", "y"].iter().map(|s| s.to_string()).collect();
    let e = parse("log(x) / y + sqrt(y) - x ^ 0.5", &names).unwrap();
    let sql = e.to_code(&sanitize_identifiers(&names, Lang::Sql), Lang::Sql);
    assert!(sql.contains("CASE WHEN CAST(\"x\" AS DOUBLE PRECISION) > 0 THEN LN("));
    assert!(sql.contains("ABS(CAST(\"y\" AS DOUBLE PRECISION)) >= 1E-12"));
    assert!(sql.contains("POWER("));
    assert!(
        !sql.contains("ELSE"),
        "undefined values must be NULL: {sql}"
    );
    let xl = e.to_code(&sanitize_identifiers(&names, Lang::Excel), Lang::Excel);
    assert_eq!(
        xl,
        "((IF(ABS(y)>=1E-12,LN(x)/y,SQRT(-1))+SQRT(y))-POWER(x,0.5))"
    );
    // Excel's unary minus binds tighter than ^, so negation must be parenthesized.
    let n = parse("-x ^ 2", &names).unwrap();
    assert_eq!(
        n.to_code(&sanitize_identifiers(&names, Lang::Excel), Lang::Excel),
        "(-POWER(x,2.0))"
    );
}

#[test]
fn identifiers_are_sanitized_deterministically() {
    let names: Vec<String> = [
        "unit price",
        "class",
        "2x",
        "A1",
        "a-b",
        "a_b",
        "_x",
        "TRUE",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        sanitize_identifiers(&names, Lang::Python),
        [
            "unit_price",
            "v_class",
            "v_2x",
            "A1",
            "a_b",
            "a_b_2",
            "v_x",
            "TRUE"
        ]
        .map(String::from)
    );
    assert_eq!(
        sanitize_identifiers(&names, Lang::Excel),
        [
            "unit_price",
            "class",
            "v_2x",
            "v_A1",
            "a_b",
            "a_b_2",
            "v_x",
            "v_TRUE"
        ]
        .map(String::from)
    );
    assert_eq!(sanitize_identifiers(&names, Lang::Sql), names);
}
