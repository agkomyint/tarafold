//! Dependence measures: Spearman correlation, KSG mutual information, distance correlation.

use tarafold_core::stats;

/// Ranks with ties averaged (1-based).
pub fn ranks(x: &[f64]) -> Vec<f64> {
    let n = x.len();
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| x[a].total_cmp(&x[b]));
    let mut r = vec![0.0; n];
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j + 1 < n && x[idx[j + 1]] == x[idx[i]] {
            j += 1;
        }
        let avg = (i + j) as f64 / 2.0 + 1.0;
        for &k in &idx[i..=j] {
            r[k] = avg;
        }
        i = j + 1;
    }
    r
}

/// Spearman rank correlation.
pub fn spearman(x: &[f64], y: &[f64]) -> f64 {
    stats::pearson(&ranks(x), &ranks(y))
}

/// Digamma function ψ(x) for x > 0 (recurrence plus asymptotic series).
pub fn digamma(mut x: f64) -> f64 {
    let mut r = 0.0;
    while x < 6.0 {
        r -= 1.0 / x;
        x += 1.0;
    }
    let f = 1.0 / (x * x);
    r + x.ln()
        - 0.5 / x
        - f * (1.0 / 12.0 - f * (1.0 / 120.0 - f * (1.0 / 252.0 - f * (1.0 / 240.0 - f / 132.0))))
}

fn standardize(x: &[f64]) -> Vec<f64> {
    let m = stats::mean(x);
    let s = stats::std(x);
    if s > 0.0 {
        x.iter().map(|v| (v - m) / s).collect()
    } else {
        vec![0.0; x.len()]
    }
}

/// Count of sorted values strictly inside `(c − r, c + r)`, excluding the point itself.
fn count_within(sorted: &[f64], c: f64, r: f64) -> usize {
    let lo = sorted.partition_point(|&v| v <= c - r);
    let hi = sorted.partition_point(|&v| v < c + r);
    (hi - lo).saturating_sub(1)
}

/// Mutual information in nats by the Kraskov–Stögbauer–Grassberger estimator (algorithm 1),
/// with `k` neighbours in the max-norm. Inputs are standardized; ties are broken by a tiny
/// deterministic jitter. Negative estimates are clamped to 0.
pub fn mutual_information(x: &[f64], y: &[f64], k: usize) -> f64 {
    let n = x.len().min(y.len());
    if n <= k + 1 {
        return 0.0;
    }
    let jitter = |v: &[f64]| -> Vec<f64> {
        standardize(v)
            .iter()
            .enumerate()
            .map(|(i, a)| a + 1e-10 * (((i * 2_654_435_761) % 1000) as f64 / 1000.0 - 0.5))
            .collect()
    };
    let (xs, ys) = (jitter(&x[..n]), jitter(&y[..n]));
    let mut sx = xs.clone();
    sx.sort_by(f64::total_cmp);
    let mut sy = ys.clone();
    sy.sort_by(f64::total_cmp);
    let mut acc = 0.0;
    let mut dists = vec![0.0; n];
    for i in 0..n {
        for j in 0..n {
            dists[j] = if i == j {
                f64::INFINITY
            } else {
                (xs[i] - xs[j]).abs().max((ys[i] - ys[j]).abs())
            };
        }
        let (_, eps, _) = dists.select_nth_unstable_by(k - 1, f64::total_cmp);
        let eps = *eps;
        let nx = count_within(&sx, xs[i], eps);
        let ny = count_within(&sy, ys[i], eps);
        acc += digamma(nx as f64 + 1.0) + digamma(ny as f64 + 1.0);
    }
    (digamma(k as f64) + digamma(n as f64) - acc / n as f64).max(0.0)
}

/// Distance correlation (Székely), O(n²) time and O(n) memory.
pub fn distance_correlation(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len().min(y.len());
    if n < 2 {
        return 0.0;
    }
    let row_means = |v: &[f64]| -> Vec<f64> {
        (0..n)
            .map(|i| (0..n).map(|j| (v[i] - v[j]).abs()).sum::<f64>() / n as f64)
            .collect()
    };
    let (ax, ay) = (row_means(x), row_means(y));
    let (gx, gy) = (stats::mean(&ax), stats::mean(&ay));
    let (mut cxy, mut cxx, mut cyy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        for j in 0..n {
            let a = (x[i] - x[j]).abs() - ax[i] - ax[j] + gx;
            let b = (y[i] - y[j]).abs() - ay[i] - ay[j] + gy;
            cxy += a * b;
            cxx += a * a;
            cyy += b * b;
        }
    }
    if cxx <= 0.0 || cyy <= 0.0 {
        return 0.0;
    }
    (cxy.max(0.0) / (cxx.sqrt() * cyy.sqrt())).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tarafold_core::Rng;

    #[test]
    fn digamma_values() {
        assert!((digamma(1.0) + std::f64::consts::EULER_GAMMA).abs() < 1e-10);
        assert!((digamma(10.0) - 2.251_752_589_066_721).abs() < 1e-10);
    }

    #[test]
    fn spearman_monotone() {
        let x: Vec<f64> = (0..50).map(f64::from).collect();
        let y: Vec<f64> = x.iter().map(|v| v.powi(3)).collect();
        assert!((spearman(&x, &y) - 1.0).abs() < 1e-12);
        assert_eq!(ranks(&[2.0, 1.0, 2.0]), vec![2.5, 1.0, 2.5]);
    }

    #[test]
    fn ksg_matches_gaussian_mi() {
        // For bivariate Gaussians, MI = -0.5 ln(1 - rho²).
        let mut r = Rng::derive(3, &[]);
        let rho: f64 = 0.8;
        let n = 2000;
        let x: Vec<f64> = (0..n).map(|_| r.normal()).collect();
        let y: Vec<f64> = x
            .iter()
            .map(|a| rho * a + (1.0 - rho * rho).sqrt() * r.normal())
            .collect();
        let truth = -0.5 * (1.0 - rho * rho).ln();
        let est = mutual_information(&x, &y, 3);
        assert!(
            (est - truth).abs() / truth < 0.05,
            "est {est} truth {truth}"
        );
        let z: Vec<f64> = (0..n).map(|_| r.normal()).collect();
        assert!(mutual_information(&x, &z, 3) < 0.02);
    }

    #[test]
    fn dcor_detects_nonlinear() {
        let x: Vec<f64> = (0..300).map(|i| f64::from(i) / 30.0 - 5.0).collect();
        let y: Vec<f64> = x.iter().map(|v| v * v).collect();
        assert!(stats::pearson(&x, &y).abs() < 0.05);
        assert!(distance_correlation(&x, &y) > 0.4);
        assert!((distance_correlation(&x, &x) - 1.0).abs() < 1e-9);
    }
}
