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
