//! Elastic-net regularization path by coordinate descent, used only to choose supports
//! (which terms are non-zero). Coefficients are refit afterwards by least squares.

use std::collections::BTreeSet;

use tarafold_core::stats;

fn soft(z: f64, g: f64) -> f64 {
    if z > g {
        z - g
    } else if z < -g {
        z + g
    } else {
        0.0
    }
}

/// Distinct supports along the path for each `alpha`, smallest first, each with at most
/// `max_support` terms.
pub(crate) fn supports(
    cols: &[&[f64]],
    y: &[f64],
    alphas: &[f64],
    n_lambda: usize,
    max_support: usize,
) -> Vec<Vec<usize>> {
    let p = cols.len();
    let n = y.len();
    if p == 0 || n < 3 {
        return Vec::new();
    }
    let nf = n as f64;
    // Standardize.
    let z: Vec<Vec<f64>> = cols
        .iter()
        .map(|c| {
            let m = stats::mean(c);
            let s = stats::std(c);
            c.iter().map(|v| (v - m) / s).collect()
        })
        .collect();
    let ym = stats::mean(y);
    let ys = stats::std(y);
    if !(ys > 0.0) {
        return Vec::new();
    }
    let yc: Vec<f64> = y.iter().map(|v| (v - ym) / ys).collect();
    let zy: Vec<f64> = z.iter().map(|c| c.iter().zip(&yc).map(|(a, b)| a * b).sum::<f64>() / nf).collect();

    let mut seen: BTreeSet<Vec<usize>> = BTreeSet::new();
    let mut out: Vec<Vec<usize>> = Vec::new();
    for &alpha in alphas {
        let lambda_max = zy.iter().map(|v| v.abs()).fold(0.0, f64::max) / alpha;
        if !(lambda_max > 0.0) {
            continue;
        }
        let mut beta = vec![0.0; p];
        let mut r = yc.clone();
        for k in 0..n_lambda {
            let frac = k as f64 / (n_lambda - 1).max(1) as f64;
            let lambda = lambda_max * (1e-4f64).powf(frac);
            let l1 = lambda * alpha;
            let l2 = 1.0 + lambda * (1.0 - alpha);
            // Full pass, then iterate on the active set until stable.
            for sweep in 0..200 {
                let active_only = sweep % 5 != 0;
                let mut max_delta: f64 = 0.0;
                for j in 0..p {
                    if active_only && beta[j] == 0.0 {
                        continue;
                    }
                    let rho = z[j].iter().zip(&r).map(|(a, b)| a * b).sum::<f64>() / nf + beta[j];
                    let new = soft(rho, l1) / l2;
                    let d = new - beta[j];
                    if d != 0.0 {
                        for (ri, zi) in r.iter_mut().zip(&z[j]) {
                            *ri -= zi * d;
                        }
                        beta[j] = new;
                        max_delta = max_delta.max(d.abs());
                    }
                }
                if max_delta < 1e-7 && !active_only {
                    break;
                }
            }
            let support: Vec<usize> = (0..p).filter(|&j| beta[j] != 0.0).collect();
            if support.len() > max_support {
                break;
            }
            if !support.is_empty() && seen.insert(support.clone()) {
                out.push(support);
            }
        }
    }
    out
}
