//! Stable-toolchain stand-in for the cargo-fuzz targets in `fuzz/` (run weekly on nightly):
//! seeded random inputs built from grammar tokens must never panic the CSV reader or the infix,
//! constraint, and goal parsers, and accepted expressions must round-trip.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use tarafold_core::Rng;

const TOKENS: &[&str] = &[
    "x",
    "y",
    "spend",
    "`unit price`",
    "1",
    "2.5",
    "-3",
    "1e308",
    "1e-320",
    "0",
    "+",
    "-",
    "*",
    "/",
    "^",
    "(",
    ")",
    "exp(",
    "log(",
    "sqrt(",
    "square(",
    "inv(",
    "abs(",
    "<=",
    ">=",
    "=",
    "..",
    "max:",
    "min:",
    ":",
    ",",
    " ",
    "\t",
    "nan",
    "inf",
    "é",
    "\"",
    ";",
    "\n",
    "%",
    "#",
];

fn random_text(rng: &mut Rng, max_tokens: usize) -> String {
    let n = rng.below(max_tokens + 1);
    (0..n).map(|_| TOKENS[rng.below(TOKENS.len())]).collect()
}

#[test]
fn infix_parser_never_panics_and_round_trips() {
    let names: Vec<String> = ["x", "y", "spend", "unit price"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut rng = Rng::derive(7, &[1]);
    let mut accepted = 0;
    for _ in 0..30_000 {
        let s = random_text(&mut rng, 12);
        if let Ok(e) = tarafold_expr::parse(&s, &names) {
            accepted += 1;
            let printed = e.to_infix(&names);
            let back = tarafold_expr::parse(&printed, &names).expect("printed form parses");
            assert_eq!(e, back, "{s:?} -> {printed:?}");
            let _ = e.eval_point(&[1.0, -2.0, 3.0, 0.0]);
            let _ = e.simplify();
            let _ = e.to_latex(&names, 4);
        }
    }
    assert!(accepted > 100, "{accepted}");
}

#[test]
fn constraint_and_goal_parsers_never_panic() {
    let names: Vec<String> = ["x", "y", "spend"].iter().map(|s| s.to_string()).collect();
    let mut rng = Rng::derive(7, &[2]);
    for _ in 0..30_000 {
        let s = random_text(&mut rng, 10);
        let _ = tarafold_optimize::spec::parse_constraint(&s, &names);
        let _ = tarafold_optimize::spec::parse_goal(&s, true);
        let _ = tarafold_optimize::spec::parse_goal(&s, false);
    }
}

#[test]
fn csv_reader_never_panics() {
    let mut rng = Rng::derive(7, &[3]);
    let cells = [
        "1", "2.5", "-3", "", "NA", "x", "1,5", "\"a,b\"", "1e400", "nan", "\"", "é",
    ];
    let seps = [",", ";", "\t", "\n", "\r\n", "\n\n"];
    let opts = tarafold_io::IngestOptions::default();
    for _ in 0..3_000 {
        let n = rng.below(400);
        let mut s = String::from("a,b,c\n");
        for _ in 0..n {
            s.push_str(cells[rng.below(cells.len())]);
            s.push_str(seps[rng.below(seps.len())]);
        }
        if let Ok(d) = tarafold_io::read_csv(s.as_bytes(), "fuzz", &opts) {
            assert!(d.columns.iter().all(|c| c.values.len() == d.n_rows));
        }
        // Raw bytes, including invalid UTF-8.
        let bytes: Vec<u8> = (0..rng.below(300)).map(|_| rng.below(256) as u8).collect();
        let _ = tarafold_io::read_csv(bytes.as_slice(), "fuzz", &opts);
    }
}
