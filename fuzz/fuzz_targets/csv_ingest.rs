#![no_main]
//! CSV ingest must never panic or allocate without bound on arbitrary bytes (R14.1).
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let opts = tarafold_io::IngestOptions::default();
    if let Ok(d) = tarafold_io::read_csv(data, "fuzz", &opts) {
        assert!(d.columns.iter().all(|c| c.values.len() == d.n_rows));
    }
});
