//! Basic statistics. All functions ignore nothing: callers pass finite data.

/// Arithmetic mean; `NaN` for empty input.
pub fn mean(x: &[f64]) -> f64 {
    if x.is_empty() {
        return f64::NAN;
    }
    // Pairwise-style accuracy is not needed at v0.1 sizes; Kahan keeps it stable.
    let mut sum = 0.0;
    let mut c = 0.0;
    for &v in x {
        let y = v - c;
        let t = sum + y;
        c = (t - sum) - y;
        sum = t;
    }
    sum / x.len() as f64
}

/// Population variance (divides by n).
pub fn variance(x: &[f64]) -> f64 {
    let m = mean(x);
    if x.is_empty() {
        return f64::NAN;
    }
    x.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / x.len() as f64
}

/// Population standard deviation.
pub fn std(x: &[f64]) -> f64 {
    variance(x).sqrt()
}

/// Quantile `q` in `[0, 1]` with linear interpolation (type 7). `NaN` for empty input.
pub fn quantile(x: &[f64], q: f64) -> f64 {
    let mut v: Vec<f64> = x.to_vec();
    v.sort_by(f64::total_cmp);
    quantile_sorted(&v, q)
}

/// Quantile of already sorted data.
pub fn quantile_sorted(v: &[f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let pos = q.clamp(0.0, 1.0) * (v.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    v[lo] + (v[hi] - v[lo]) * (pos - lo as f64)
}

/// Median.
pub fn median(x: &[f64]) -> f64 {
    quantile(x, 0.5)
}

/// Pearson correlation; 0 when either side is constant.
pub fn pearson(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len().min(y.len());
    if n < 2 {
        return 0.0;
    }
    let mx = mean(&x[..n]);
    let my = mean(&y[..n]);
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let dx = x[i] - mx;
        let dy = y[i] - my;
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    if sxx <= 0.0 || syy <= 0.0 {
        return 0.0;
    }
    (sxy / (sxx.sqrt() * syy.sqrt())).clamp(-1.0, 1.0)
}

/// Gather `values[idx]` for a list of row indices.
pub fn gather(values: &[f64], idx: &[u32]) -> Vec<f64> {
    idx.iter().map(|&i| values[i as usize]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        let x = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(mean(&x), 2.5);
        assert!((variance(&x) - 1.25).abs() < 1e-12);
        assert_eq!(median(&x), 2.5);
        assert_eq!(quantile(&x, 0.0), 1.0);
        assert_eq!(quantile(&x, 1.0), 4.0);
        assert!((pearson(&x, &[2.0, 4.0, 6.0, 8.0]) - 1.0).abs() < 1e-12);
        assert_eq!(pearson(&x, &[1.0, 1.0, 1.0, 1.0]), 0.0);
    }
}
