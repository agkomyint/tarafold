//! Equivalence of two expressions (task 2.2): equal after simplification, or numerically equal
//! (max relative error < 1e-3) at deterministic sample points, with constants rounded to
//! 3 significant figures first. The benchmark scores recovery with the same rule on in-domain
//! data; this version needs no data and is used in tests and tools.

use crate::Expr;

fn round_sig(c: f64, sig: i32) -> f64 {
    if c == 0.0 || !c.is_finite() {
        return c;
    }
    let mag = c.abs().log10().floor() as i32;
    let f = 10f64.powi(sig - 1 - mag);
    (c * f).round() / f
}

/// Same structure with every constant rounded to `sig` significant figures.
pub fn round_constants(e: &Expr, sig: i32) -> Expr {
    let c: Vec<f64> = e.constants().iter().map(|&v| round_sig(v, sig)).collect();
    e.with_constants(&c)
}

/// Deterministic quasi-random points in `[lo, hi]^n` (Weyl sequence).
fn points(n_vars: usize, k: usize, lo: f64, hi: f64) -> Vec<Vec<f64>> {
    // Fractional parts of multiples of square roots of primes.
    const ALPHA: [f64; 8] = [
        0.414_213_562_373_095,
        0.732_050_807_568_877,
        0.236_067_977_499_790,
        0.645_751_311_064_591,
        0.316_624_790_355_400,
        0.605_551_275_463_989,
        0.123_105_625_617_661,
        0.358_898_943_540_674,
    ];
    (1..=k)
        .map(|i| {
            (0..n_vars)
                .map(|j| {
                    let a = ALPHA[j % ALPHA.len()] + 0.1 * (j / ALPHA.len()) as f64;
                    let u = (i as f64 * a).fract();
                    lo + u * (hi - lo)
                })
                .collect()
        })
        .collect()
}

/// Whether `a` and `b` (over the same variable list of length `n_vars`) are equivalent: equal
/// nodes after simplification, or equal values within 1e-3 relative error at 200 points in
/// `[0.5, 3]^n` once constants are rounded to 3 significant figures. Points where both are
/// undefined are skipped; a point where only one is undefined means not equivalent.
pub fn equivalent(a: &Expr, b: &Expr, n_vars: usize) -> bool {
    let (ra, rb) = (round_constants(a, 3), round_constants(b, 3));
    if ra.simplify().nodes() == rb.simplify().nodes() {
        return true;
    }
    let mut compared = 0;
    for p in points(n_vars.max(1), 200, 0.5, 3.0) {
        let (x, y) = (ra.eval_point(&p), rb.eval_point(&p));
        match (x.is_finite(), y.is_finite()) {
            (false, false) => continue,
            (true, true) => {
                if (x - y).abs() > 1e-3 * y.abs().max(1e-9) {
                    return false;
                }
                compared += 1;
            }
            _ => return false,
        }
    }
    compared > 0
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::parse;

    fn e(s: &str) -> Expr {
        let names: Vec<String> = ["x", "y"].iter().map(|s| s.to_string()).collect();
        parse(s, &names).unwrap()
    }

    #[test]
    fn algebraically_equal_forms_are_equivalent() {
        assert!(equivalent(&e("x * (y + 1)"), &e("x * y + x"), 2));
        assert!(equivalent(&e("(x + y)^2"), &e("x^2 + 2*x*y + y^2"), 2));
        assert!(equivalent(&e("log(x * y)"), &e("log(x) + log(y)"), 2));
        // Constants equal to 3 significant figures.
        assert!(equivalent(&e("2.50001 * x^0.7"), &e("2.5 * x^0.7"), 2));
    }

    #[test]
    fn different_constants_or_structure_are_not() {
        assert!(!equivalent(&e("2.5 * x^0.7"), &e("2.5 * x^0.8"), 2));
        assert!(!equivalent(&e("x * y"), &e("x + y"), 2));
        assert!(!equivalent(&e("x / (1 + y)"), &e("x / y"), 2));
    }
}
