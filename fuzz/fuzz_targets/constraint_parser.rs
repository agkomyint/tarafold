#![no_main]
//! The `--constrain` parser must reject or accept any text without panicking (R14.2).
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let names: Vec<String> = ["a", "b", "spend"].iter().map(|s| s.to_string()).collect();
    let _ = tarafold_optimize::spec::parse_constraint(s, &names);
});
