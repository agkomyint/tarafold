//! Ridge regression baseline with λ chosen on validation data.

use tarafold_core::{linalg, stats};

/// A fitted ridge model on standardized features.
#[derive(Debug, Clone)]
pub struct Ridge {
    mean: Vec<f64>,
    std: Vec<f64>,
    intercept: f64,
    beta: Vec<f64>,
    /// Chosen penalty.
    pub lambda: f64,
}

impl Ridge {
    /// Fit for each λ on a log grid and keep the best by validation MSE.
    pub fn fit(
        train_x: &[Vec<f64>],
        train_y: &[f64],
        val_x: &[Vec<f64>],
        val_y: &[f64],
    ) -> Option<Self> {
        let p = train_x.len();
        let n = train_y.len();
        if p == 0 || n < 2 {
            return None;
        }
        let mean: Vec<f64> = train_x.iter().map(|c| stats::mean(c)).collect();
        let std: Vec<f64> = train_x.iter().map(|c| stats::std(c).max(1e-12)).collect();
        let z: Vec<Vec<f64>> = (0..p)
            .map(|j| train_x[j].iter().map(|v| (v - mean[j]) / std[j]).collect())
            .collect();
        let ym = stats::mean(train_y);
        let mut g = vec![vec![0.0; p]; p];
        for i in 0..p {
            for j in i..p {
                let v: f64 = z[i].iter().zip(&z[j]).map(|(a, b)| a * b).sum();
                g[i][j] = v;
                g[j][i] = v;
            }
        }
        let b: Vec<f64> = z
            .iter()
            .map(|c| c.iter().zip(train_y).map(|(a, y)| a * (y - ym)).sum())
            .collect();
        let mut best: Option<(f64, Self)> = None;
        for k in 0..25 {
            let lambda = 10f64.powf(-4.0 + 7.0 * k as f64 / 24.0) * n as f64;
            let mut m = g.clone();
            for (i, row) in m.iter_mut().enumerate() {
                row[i] += lambda;
            }
            let Some(l) = linalg::cholesky(&m) else {
                continue;
            };
            let beta = linalg::cholesky_solve(&l, &b);
            let model = Self {
                mean: mean.clone(),
                std: std.clone(),
                intercept: ym,
                beta,
                lambda: lambda / n as f64,
            };
            let pred = model.predict(val_x);
            let mse = pred
                .iter()
                .zip(val_y)
                .map(|(p, y)| (p - y).powi(2))
                .sum::<f64>()
                / val_y.len().max(1) as f64;
            if mse.is_finite() && best.as_ref().is_none_or(|(b, _)| mse < *b) {
                best = Some((mse, model));
            }
        }
        best.map(|(_, m)| m)
    }

    /// Predict for column-major features.
    pub fn predict(&self, x: &[Vec<f64>]) -> Vec<f64> {
        let n = x.first().map_or(0, Vec::len);
        (0..n)
            .map(|i| {
                self.intercept
                    + (0..self.beta.len())
                        .map(|j| self.beta[j] * (x[j][i] - self.mean[j]) / self.std[j])
                        .sum::<f64>()
            })
            .collect()
    }
}
