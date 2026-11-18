//! An interrupted run still produces a full report marked `interrupted` (task 9.1). This is
//! its own test binary because the stop flag is process-wide.
#![allow(clippy::unwrap_used)]

use tarafold_core::RunConfig;
use tarafold_report::RunStatus;

#[test]
fn interrupted_run_writes_a_report() {
    let e = tarafold_synth::find("cobb-douglas").unwrap();
    let g = tarafold_synth::generate(&e, 300, 0.0, 0, 1).unwrap();
    let data = tarafold_io::read_csv(
        g.to_csv().unwrap().as_bytes(),
        "cd.csv",
        &Default::default(),
    )
    .unwrap();
    let cfg = RunConfig {
        targets: vec!["output".into()],
        ..Default::default()
    };
    tarafold_core::stop::interrupt();
    let (r, _) = tarafold_engine::run(&data, &cfg).unwrap();
    assert_eq!(r.run.status, RunStatus::Interrupted);
    assert!(!r.targets[0].discover.front.is_empty());
    assert!(r.targets[0].validate.stability.is_none());
    assert!(r.notes.iter().any(|n| n.contains("interrupted")));
    let json = r.to_json().unwrap();
    assert!(json.contains("\"status\": \"interrupted\""));
    tarafold_core::stop::reset();
}
