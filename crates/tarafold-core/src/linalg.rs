//! Small dense linear algebra: least squares and symmetric positive-definite solves.
//!
//! v0.1 problems are small (≤ a few hundred columns), so a careful Householder QR is enough
//! and keeps the binary free of BLAS/LAPACK. `faer` can replace this later behind the same API.

/// Solve `min ||A·x − b||₂` where `A` is given as columns (`cols[j][i]` = A[i, j]).
///
/// Columns are scaled to unit norm before factorization for conditioning. Returns `None`
/// when `A` is numerically rank deficient (relative pivot below `1e-10`) or has no rows.
pub fn lstsq(cols: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let p = cols.len();
    let n = b.len();
    if p == 0 || n < p || cols.iter().any(|c| c.len() != n) {
        return None;
    }
    let scale: Vec<f64> = cols
        .iter()
        .map(|c| {
            let s = c.iter().map(|v| v * v).sum::<f64>().sqrt();
            if s > 0.0 && s.is_finite() { s } else { 1.0 }
        })
        .collect();
    let mut a: Vec<Vec<f64>> = cols
        .iter()
        .zip(&scale)
        .map(|(c, s)| c.iter().map(|v| v / s).collect())
        .collect();
    let mut rhs = b.to_vec();
    let mut diag = vec![0.0; p];

    for k in 0..p {
        let norm = a[k][k..].iter().map(|v| v * v).sum::<f64>().sqrt();
        if !norm.is_finite() {
            return None;
        }
        let alpha = if a[k][k] > 0.0 { -norm } else { norm };
        // Householder vector v = x − alpha·e1, stored in a[k][k..].
        a[k][k] -= alpha;
        let vnorm2 = a[k][k..].iter().map(|v| v * v).sum::<f64>();
        diag[k] = alpha;
        if vnorm2 <= 0.0 {
            continue;
        }
        for j in (k + 1)..p {
            let dot: f64 = (k..n).map(|i| a[k][i] * a[j][i]).sum();
            let f = 2.0 * dot / vnorm2;
            for i in k..n {
                let vk = a[k][i];
                a[j][i] -= f * vk;
            }
        }
        let dot: f64 = (k..n).map(|i| a[k][i] * rhs[i]).sum();
        let f = 2.0 * dot / vnorm2;
        for (i, r) in rhs.iter_mut().enumerate().skip(k) {
            *r -= f * a[k][i];
        }
    }

    let max_diag = diag.iter().map(|d| d.abs()).fold(0.0, f64::max);
    if max_diag == 0.0 || diag.iter().any(|d| d.abs() < 1e-10 * max_diag) {
        return None;
    }
    // Back substitution with R: R[k,k] = diag[k], R[k,j] = a[j][k] for j > k.
    let mut x = vec![0.0; p];
    for k in (0..p).rev() {
        let mut s = rhs[k];
        for j in (k + 1)..p {
            s -= a[j][k] * x[j];
        }
        x[k] = s / diag[k];
    }
    let out: Vec<f64> = x.iter().zip(&scale).map(|(v, s)| v / s).collect();
    out.iter().all(|v| v.is_finite()).then_some(out)
}

/// Cholesky factor `L` (row-major, lower) of a symmetric positive-definite `p×p` matrix.
pub fn cholesky(m: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let p = m.len();
    let mut l = vec![vec![0.0; p]; p];
    for i in 0..p {
        for j in 0..=i {
            let mut s = m[i][j];
            for k in 0..j {
                s -= l[i][k] * l[j][k];
            }
            if i == j {
                if s <= 0.0 || !s.is_finite() {
                    return None;
                }
                l[i][i] = s.sqrt();
            } else {
                l[i][j] = s / l[j][j];
            }
        }
    }
    Some(l)
}

/// Solve `L·Lᵀ·x = b` given the Cholesky factor.
pub fn cholesky_solve(l: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let p = l.len();
    let mut y = vec![0.0; p];
    for i in 0..p {
        let mut s = b[i];
        for k in 0..i {
            s -= l[i][k] * y[k];
        }
        y[i] = s / l[i][i];
    }
    let mut x = vec![0.0; p];
    for i in (0..p).rev() {
        let mut s = y[i];
        for k in (i + 1)..p {
            s -= l[k][i] * x[k];
        }
        x[i] = s / l[i][i];
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_fit() {
        // y = 2 + 3x
        let x: Vec<f64> = (0..10).map(f64::from).collect();
        let y: Vec<f64> = x.iter().map(|v| 2.0 + 3.0 * v).collect();
        let coef = lstsq(&[vec![1.0; 10], x], &y).unwrap();
        assert!((coef[0] - 2.0).abs() < 1e-10 && (coef[1] - 3.0).abs() < 1e-10);
    }

    #[test]
    fn rank_deficient_is_none() {
        let x: Vec<f64> = (0..10).map(f64::from).collect();
        let x2: Vec<f64> = x.iter().map(|v| 2.0 * v).collect();
        assert!(lstsq(&[x.clone(), x2], &x).is_none());
    }

    #[test]
    fn cholesky_roundtrip() {
        let m = vec![vec![4.0, 2.0], vec![2.0, 3.0]];
        let l = cholesky(&m).unwrap();
        let x = cholesky_solve(&l, &[2.0, 1.0]);
        assert!((4.0 * x[0] + 2.0 * x[1] - 2.0).abs() < 1e-12);
        assert!((2.0 * x[0] + 3.0 * x[1] - 1.0).abs() < 1e-12);
    }
}
