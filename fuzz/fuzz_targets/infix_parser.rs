#![no_main]
//! The infix parser must not panic, and whatever it accepts must print and parse back to the
//! same expression.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let names: Vec<String> = ["x", "y", "unit price"].iter().map(|s| s.to_string()).collect();
    if let Ok(e) = tarafold_expr::parse(s, &names) {
        let printed = e.to_infix(&names);
        let back = tarafold_expr::parse(&printed, &names).expect("printed form must parse");
        assert_eq!(e, back, "{s} -> {printed}");
        let _ = e.eval_point(&[1.0, 2.0, 3.0]);
        let _ = e.simplify();
    }
});
