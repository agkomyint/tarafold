//! End-to-end CLI tests: synth → run, outputs, JSON mode, and exit codes (task 1.9).
#![allow(clippy::unwrap_used)]

use std::fs;

use assert_cmd::Command;

fn tarafold() -> Command {
    Command::cargo_bin("tarafold").unwrap()
}

#[test]
fn synth_then_run_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let synth = dir.path().join("synth");
    tarafold()
        .args(["synth", "business-revenue", "--rows", "800", "--out"])
        .arg(&synth)
        .assert()
        .success();
    let truth: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(synth.join("truth.json")).unwrap()).unwrap();
    assert_eq!(truth["equation"], "orders * price");

    let out = dir.path().join("out");
    let assert = tarafold()
        .arg("run")
        .arg(synth.join("data.csv"))
        .args([
            "--target",
            "revenue",
            "--goal",
            "50000",
            "--control",
            "orders,price",
            "--out",
        ])
        .arg(&out)
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("Selected  revenue = price * orders"),
        "{stdout}"
    );
    for f in ["report.json", "equations.csv", "candidates.csv", "run.toml"] {
        assert!(out.join(f).exists(), "missing {f}");
    }
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["optimize"]["reached"], true);
    // Candidates checked against the TRUE equation.
    for c in report["optimize"]["candidates"].as_array().unwrap() {
        if c["within_tolerance"] == true {
            let v =
                c["inputs"]["orders"].as_f64().unwrap() * c["inputs"]["price"].as_f64().unwrap();
            assert!((v - 50_000.0).abs() <= report["optimize"]["tolerance"].as_f64().unwrap());
        }
    }
    // run.toml reproduces the configuration.
    assert!(
        fs::read_to_string(out.join("run.toml"))
            .unwrap()
            .contains("goal = 50000.0")
    );

    // report.json validates against the schema printed by `tarafold schema` (R10.1).
    let schema_out = tarafold().arg("schema").assert().success();
    let schema: serde_json::Value =
        serde_json::from_slice(&schema_out.get_output().stdout).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<String> = validator
        .iter_errors(&report)
        .map(|e| format!("{e} at {}", e.instance_path()))
        .take(10)
        .collect();
    assert!(
        errors.is_empty(),
        "report.json does not match its schema: {errors:#?}"
    );

    // Export the selected equation in every format (R16).
    for (to, needle) in [
        ("python", "def predict_revenue("),
        ("js", "export function predict_revenue("),
        ("sql", "AS \"predicted_revenue\""),
        ("excel", "\n=("),
        ("latex", "\\mathrm{revenue} ="),
        ("json", "\"selected\": true"),
    ] {
        let a = tarafold()
            .arg("export")
            .arg(&out)
            .args(["--to", to])
            .assert()
            .success();
        let text = String::from_utf8(a.get_output().stdout.clone()).unwrap();
        assert!(text.contains(needle), "{to}: {text}");
        assert!(text.contains("Reliability grade: high"), "{to}: {text}");
    }
    tarafold()
        .arg("export")
        .arg(&out)
        .args(["--to", "python", "--id", "999"])
        .assert()
        .code(2);
}

#[test]
fn json_mode_writes_only_json_to_stdout() {
    let dir = tempfile::tempdir().unwrap();
    tarafold()
        .args(["synth", "linear", "--rows", "300", "--out"])
        .arg(dir.path())
        .assert()
        .success();
    let assert = tarafold()
        .arg("run")
        .arg(dir.path().join("data.csv"))
        .args(["--target", "y", "--format", "json", "--out"])
        .arg(dir.path().join("o"))
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(v["schema_version"], "0.1.0");
}

#[test]
fn stdin_input() {
    let dir = tempfile::tempdir().unwrap();
    tarafold()
        .args(["synth", "linear", "--rows", "300", "--out"])
        .arg(dir.path())
        .assert()
        .success();
    let csv = fs::read(dir.path().join("data.csv")).unwrap();
    tarafold()
        .args(["run", "-", "--target", "y", "--out"])
        .arg(dir.path().join("o"))
        .write_stdin(csv)
        .assert()
        .success();
}

#[test]
fn exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    tarafold()
        .args(["synth", "linear", "--rows", "300", "--out"])
        .arg(dir.path())
        .assert()
        .success();
    let data = dir.path().join("data.csv");
    let o = dir.path().join("o");
    // 2: invalid usage (unknown target, unavailable engine, bad bounds).
    tarafold()
        .arg("run")
        .arg(&data)
        .args(["--target", "nope", "--out"])
        .arg(&o)
        .assert()
        .code(2);
    tarafold()
        .arg("run")
        .arg(&data)
        .args(["--target", "y", "--engine", "quantum", "--out"])
        .arg(&o)
        .assert()
        .code(2);
    tarafold()
        .arg("run")
        .arg(&data)
        .args([
            "--target", "y", "--goal", "1", "--bounds", "x1=5..1", "--out",
        ])
        .arg(&o)
        .assert()
        .code(2);
    // 3: invalid data (too few rows).
    let small = dir.path().join("small.csv");
    fs::write(&small, "a,b\n1,2\n2,4\n3,6\n").unwrap();
    tarafold()
        .arg("run")
        .arg(&small)
        .args(["--target", "b", "--out"])
        .arg(&o)
        .assert()
        .code(3);
    // 5: goal outside what the bounds allow (y = 3*x1 - 2*x2 + 5 ≤ 35 on [0,10]²).
    tarafold()
        .arg("run")
        .arg(&data)
        .args(["--target", "y", "--goal", "1000", "--out"])
        .arg(&o)
        .assert()
        .code(5);
    // 4: no usable signal (pure-noise target graded low).
    let noise = dir.path().join("noise.csv");
    let mut s = String::from("a,b,y\n");
    let mut state: u64 = 7;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    for _ in 0..300 {
        s.push_str(&format!("{},{},{}\n", next(), next(), next()));
    }
    fs::write(&noise, s).unwrap();
    tarafold()
        .arg("run")
        .arg(&noise)
        .args(["--target", "y", "--out"])
        .arg(&o)
        .assert()
        .code(4);
}

#[test]
fn version_and_schema() {
    let v = tarafold().arg("--version").assert().success();
    assert!(String::from_utf8_lossy(&v.get_output().stdout).starts_with("tarafold "));
    let s = tarafold().arg("schema").assert().success();
    let schema: serde_json::Value = serde_json::from_slice(&s.get_output().stdout).unwrap();
    assert_eq!(schema["title"], "Report");
}

#[test]
fn multi_goal_constraints_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    tarafold()
        .args(["synth", "business-revenue", "--rows", "800", "--out"])
        .arg(dir.path())
        .assert()
        .success();
    let data = dir.path().join("data.csv");
    let out = dir.path().join("o");
    let chain = [
        "--target",
        "visitors,conversion,orders,revenue",
        "--control",
        "spend,price",
    ];
    // Threshold + two objectives through the equation chain, with a constraint and an integer.
    tarafold()
        .arg("run")
        .arg(&data)
        .args(chain)
        .args([
            "--require",
            "revenue>=30000",
            "--prefer",
            "max:orders",
            "--prefer",
            "min:visitors",
            "--constrain",
            "spend <= 40000",
            "--integer",
            "spend",
            "--stability",
            "2",
            "--bootstrap",
            "30",
            "--out",
        ])
        .arg(&out)
        .assert()
        .success();
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("report.json")).unwrap()).unwrap();
    let o = &report["optimize"];
    assert_eq!(o["method"], "NSGA-II");
    let cands = o["candidates"].as_array().unwrap();
    assert!(cands.len() >= 3, "{}", cands.len());
    for c in cands {
        let spend = c["inputs"]["spend"].as_f64().unwrap();
        assert!(spend <= 40000.0 + 1e-6 && spend.fract() == 0.0, "{spend}");
        assert!(c["predictions"]["revenue"].as_f64().unwrap() >= 30000.0 - 1e-3);
    }
    let marks: Vec<String> = cands
        .iter()
        .flat_map(|c| {
            c["marks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m.as_str().unwrap().to_string())
        })
        .collect();
    assert!(marks.iter().any(|m| m == "knee"), "{marks:?}");

    // Impossible goals: exit 5 with a conflict report.
    tarafold()
        .arg("run")
        .arg(&data)
        .args(chain)
        .args([
            "--require",
            "revenue>=1e9",
            "--stability",
            "0",
            "--bootstrap",
            "10",
            "--out",
        ])
        .arg(&out)
        .assert()
        .code(5);
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["optimize"]["conflict"]["goals"][0], "revenue>=1e9");

    // Usage errors: non-linear constraint, goal on a non-target, too many objectives, a target as control.
    let bad = |extra: &[&str]| {
        tarafold()
            .arg("run")
            .arg(&data)
            .args(chain)
            .args(extra)
            .args(["--stability", "0", "--out"])
            .arg(&out)
            .assert()
            .code(2);
    };
    bad(&["--goal", "50000", "--constrain", "spend * price <= 5"]);
    bad(&["--require", "profit>=5"]);
    bad(&["--goal", "50000", "--require", "revenue>=5"]);
    let many: Vec<String> = (0..11)
        .flat_map(|_| ["--prefer".to_string(), "max:revenue".to_string()])
        .collect();
    let many_ref: Vec<&str> = many.iter().map(String::as_str).collect();
    bad(&many_ref);
    tarafold()
        .arg("run")
        .arg(&data)
        .args([
            "--target",
            "orders,revenue",
            "--control",
            "orders,price",
            "--require",
            "revenue>=5",
            "--stability",
            "0",
            "--out",
        ])
        .arg(&out)
        .assert()
        .code(2);
}

#[test]
fn suggest_and_saved_report_commands() {
    let dir = tempfile::tempdir().unwrap();
    tarafold()
        .args([
            "synth",
            "cobb-douglas",
            "--rows",
            "300",
            "--noise",
            "0.05",
            "--out",
        ])
        .arg(dir.path())
        .assert()
        .success();
    let data = dir.path().join("data.csv");
    let out = dir.path().join("o");
    tarafold()
        .arg("run")
        .arg(&data)
        .args([
            "--target",
            "output",
            "--control",
            "capital,labor",
            "--suggest",
            "4",
            "--stability",
            "2",
            "--out",
        ])
        .arg(&out)
        .assert()
        .success();
    // suggestions.csv has the input's columns, controllables filled, target empty.
    let sug = fs::read_to_string(out.join("suggestions.csv")).unwrap();
    let mut lines = sug.lines();
    assert_eq!(lines.next(), Some("capital,labor,output"));
    let rows: Vec<&str> = lines.collect();
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|r| r.ends_with(',')), "{rows:?}");

    // Fill in results with the true equation and append: the next run must work unchanged.
    let mut csv = fs::read_to_string(&data).unwrap();
    for r in &rows {
        let v: Vec<f64> = r
            .trim_end_matches(',')
            .split(',')
            .map(|x| x.parse().unwrap())
            .collect();
        let y = 2.5 * v[0].powf(0.7) * v[1].powf(0.3);
        csv.push_str(&format!("{},{},{}\n", v[0], v[1], y));
    }
    let data2 = dir.path().join("data2.csv");
    fs::write(&data2, csv).unwrap();
    tarafold()
        .arg("run")
        .arg(&data2)
        .args(["--target", "output", "--stability", "0", "--out"])
        .arg(dir.path().join("o2"))
        .assert()
        .success();

    // Saved-report optimization: no rediscovery, same equation.
    let a = tarafold()
        .arg("optimize")
        .arg(&out)
        .args(["--goal", "80", "--control", "capital,labor", "--out"])
        .arg(dir.path().join("o3"))
        .assert()
        .success();
    assert!(String::from_utf8_lossy(&a.get_output().stdout).contains("Goal      output = 80"));
    let report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(dir.path().join("o3").join("report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report["optimize"]["reached"], true);
    // Saved-report suggestions (explore) and a data mismatch error.
    tarafold()
        .arg("suggest")
        .arg(&out)
        .args(["-n", "2", "--mode", "explore", "--out"])
        .arg(dir.path().join("o4"))
        .assert()
        .success();
    assert!(dir.path().join("o4").join("suggestions.csv").exists());
    tarafold()
        .arg("optimize")
        .arg(&out)
        .args(["--goal", "80", "--data"])
        .arg(&data2)
        .assert()
        .code(2);
}

#[test]
fn time_budget_stops_search_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let synth = dir.path().join("s");
    tarafold()
        .args(["synth", "exp-log", "--rows", "300", "--out"])
        .arg(&synth)
        .assert()
        .success();
    let out = dir.path().join("o");
    let a = tarafold()
        .arg("run")
        .arg(synth.join("data.csv"))
        .args([
            "--target",
            "y",
            "--time-budget",
            "0.001s",
            "-v",
            "--threads",
            "2",
            "--out",
        ])
        .arg(&out)
        .assert();
    let code = a.get_output().status.code().unwrap();
    assert!(code == 0 || code == 4, "exit {code}");
    let stderr = String::from_utf8(a.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("stage "), "{stderr}");
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["run"]["status"], "completed");
    let notes = report["notes"].to_string();
    assert!(notes.contains("time budget ran out"), "{notes}");
    assert!(out.join("report.html").exists());
}
