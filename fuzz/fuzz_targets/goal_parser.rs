#![no_main]
//! The `--require` / `--prefer` goal parser must not panic on any text.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let _ = tarafold_optimize::spec::parse_goal(s, true);
    let _ = tarafold_optimize::spec::parse_goal(s, false);
});
