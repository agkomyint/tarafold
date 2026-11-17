//! Histogram gradient-boosted regression trees (squared loss), in-house and deterministic.
//! Used as the "how much signal can any model find" reference and for importance and
//! interaction estimates. Not exposed as a discovery result.

use tarafold_core::stats;

const MAX_BINS: usize = 64;

#[derive(Debug, Clone)]
enum TreeNode {
    Split {
        feature: usize,
        threshold: f64,
        left: usize,
        right: usize,
    },
    Leaf(f64),
}

/// Boosted tree ensemble.
#[derive(Debug, Clone)]
pub struct Gbdt {
    base: f64,
    lr: f64,
    trees: Vec<Vec<TreeNode>>,
}

/// Settings.
#[derive(Debug, Clone, Copy)]
pub struct GbdtParams {
    /// Maximum trees.
    pub max_trees: usize,
    /// Tree depth.
    pub depth: usize,
    /// Shrinkage.
    pub learning_rate: f64,
    /// Minimum rows per leaf.
    pub min_leaf: usize,
    /// L2 penalty on leaf values.
    pub lambda: f64,
    /// Stop after this many trees without validation improvement.
    pub patience: usize,
}

impl Default for GbdtParams {
    fn default() -> Self {
        Self {
            max_trees: 400,
            depth: 5,
            learning_rate: 0.08,
            min_leaf: 8,
            lambda: 1.0,
            patience: 30,
        }
    }
}

/// Bin thresholds per feature: up to `MAX_BINS - 1` quantile cut points.
fn thresholds(col: &[f64]) -> Vec<f64> {
    let mut s = col.to_vec();
    s.sort_by(f64::total_cmp);
    s.dedup();
    if s.len() <= MAX_BINS {
        return s.windows(2).map(|w| 0.5 * (w[0] + w[1])).collect();
    }
    let mut t: Vec<f64> = (1..MAX_BINS)
        .map(|k| stats::quantile_sorted(&s, k as f64 / MAX_BINS as f64))
        .collect();
    t.dedup();
    t
}

impl Gbdt {
    /// Fit with early stopping on validation MSE.
    pub fn fit(
        train_x: &[Vec<f64>],
        train_y: &[f64],
        val_x: &[Vec<f64>],
        val_y: &[f64],
        params: GbdtParams,
    ) -> Self {
        let p = train_x.len();
        let n = train_y.len();
        let cuts: Vec<Vec<f64>> = train_x.iter().map(|c| thresholds(c)).collect();
        // Bin index per (feature, row): number of thresholds below the value.
        let bins: Vec<Vec<u8>> = (0..p)
            .map(|j| {
                train_x[j]
                    .iter()
                    .map(|&v| cuts[j].partition_point(|&t| t < v) as u8)
                    .collect()
            })
            .collect();
        let base = stats::mean(train_y);
        let mut model = Self {
            base,
            lr: params.learning_rate,
            trees: Vec::new(),
        };
        let mut pred = vec![base; n];
        let mut val_pred = vec![base; val_y.len()];
        let val_rows: Vec<Vec<f64>> = (0..val_y.len())
            .map(|i| (0..p).map(|j| val_x[j][i]).collect())
            .collect();
        let mse = |a: &[f64], b: &[f64]| {
            a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f64>() / b.len().max(1) as f64
        };
        let mut best = (mse(&val_pred, val_y), 0usize);
        for t in 0..params.max_trees {
            let resid: Vec<f64> = train_y.iter().zip(&pred).map(|(y, p)| y - p).collect();
            let tree = build_tree(&bins, &cuts, &resid, (0..n as u32).collect(), params);
            for (i, pr) in pred.iter_mut().enumerate() {
                *pr += params.learning_rate * eval_tree(&tree, |j| train_x[j][i]);
            }
            for (i, pr) in val_pred.iter_mut().enumerate() {
                *pr += params.learning_rate * eval_tree(&tree, |j| val_rows[i][j]);
            }
            model.trees.push(tree);
            let m = mse(&val_pred, val_y);
            if m < best.0 {
                best = (m, t + 1);
            } else if t + 1 - best.1 >= params.patience {
                break;
            }
        }
        model.trees.truncate(best.1);
        model
    }

    /// Number of trees kept.
    pub fn n_trees(&self) -> usize {
        self.trees.len()
    }

    /// Predict one row.
    pub fn predict_row(&self, row: &[f64]) -> f64 {
        self.base
            + self.lr
                * self
                    .trees
                    .iter()
                    .map(|t| eval_tree(t, |j| row[j]))
                    .sum::<f64>()
    }

    /// Predict column-major features.
    pub fn predict(&self, x: &[Vec<f64>]) -> Vec<f64> {
        let n = x.first().map_or(0, Vec::len);
        let mut row = vec![0.0; x.len()];
        (0..n)
            .map(|i| {
                for (j, c) in x.iter().enumerate() {
                    row[j] = c[i];
                }
                self.predict_row(&row)
            })
            .collect()
    }
}

fn eval_tree(tree: &[TreeNode], x: impl Fn(usize) -> f64) -> f64 {
    let mut k = 0;
    loop {
        match tree[k] {
            TreeNode::Leaf(v) => return v,
            TreeNode::Split {
                feature,
                threshold,
                left,
                right,
            } => {
                k = if x(feature) <= threshold { left } else { right };
            }
        }
    }
}

fn build_tree(
    bins: &[Vec<u8>],
    cuts: &[Vec<f64>],
    g: &[f64],
    rows: Vec<u32>,
    params: GbdtParams,
) -> Vec<TreeNode> {
    let mut nodes = Vec::new();
    grow(&mut nodes, bins, cuts, g, rows, 0, params);
    nodes
}

fn grow(
    nodes: &mut Vec<TreeNode>,
    bins: &[Vec<u8>],
    cuts: &[Vec<f64>],
    g: &[f64],
    rows: Vec<u32>,
    depth: usize,
    params: GbdtParams,
) -> usize {
    let id = nodes.len();
    let sum: f64 = rows.iter().map(|&i| g[i as usize]).sum();
    let cnt = rows.len() as f64;
    let leaf = sum / (cnt + params.lambda);
    nodes.push(TreeNode::Leaf(leaf));
    if depth >= params.depth || rows.len() < 2 * params.min_leaf {
        return id;
    }
    let parent_score = sum * sum / (cnt + params.lambda);
    let mut best: Option<(f64, usize, usize)> = None; // (gain, feature, bin)
    for (j, fb) in bins.iter().enumerate() {
        let nb = cuts[j].len() + 1;
        if nb < 2 {
            continue;
        }
        let mut hs = [0.0f64; MAX_BINS + 1];
        let mut hc = [0usize; MAX_BINS + 1];
        for &i in &rows {
            let b = fb[i as usize] as usize;
            hs[b] += g[i as usize];
            hc[b] += 1;
        }
        let (mut ls, mut lc) = (0.0, 0usize);
        for b in 0..nb - 1 {
            ls += hs[b];
            lc += hc[b];
            let rc = rows.len() - lc;
            if lc < params.min_leaf || rc < params.min_leaf {
                continue;
            }
            let rs = sum - ls;
            let gain = ls * ls / (lc as f64 + params.lambda)
                + rs * rs / (rc as f64 + params.lambda)
                - parent_score;
            if gain > 1e-12 && best.is_none_or(|(bg, _, _)| gain > bg) {
                best = Some((gain, j, b));
            }
        }
    }
    let Some((_, feature, b)) = best else {
        return id;
    };
    let threshold = cuts[feature][b];
    let (l, r): (Vec<u32>, Vec<u32>) = rows
        .iter()
        .partition(|&&i| (bins[feature][i as usize] as usize) <= b);
    let left = grow(nodes, bins, cuts, g, l, depth + 1, params);
    let right = grow(nodes, bins, cuts, g, r, depth + 1, params);
    nodes[id] = TreeNode::Split {
        feature,
        threshold,
        left,
        right,
    };
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use tarafold_core::Rng;

    #[test]
    fn fits_nonlinear_interaction() {
        let mut r = Rng::derive(1, &[]);
        let mk = |r: &mut Rng, n: usize| {
            let x: Vec<Vec<f64>> = (0..3)
                .map(|_| (0..n).map(|_| r.range(0.0, 5.0)).collect())
                .collect();
            let y: Vec<f64> = (0..n).map(|i| x[0][i] * x[1][i] + x[2][i]).collect();
            (x, y)
        };
        let (tx, ty) = mk(&mut r, 1500);
        let (vx, vy) = mk(&mut r, 500);
        let m = Gbdt::fit(&tx, &ty, &vx, &vy, GbdtParams::default());
        let p = m.predict(&vx);
        let r2 = 1.0
            - p.iter().zip(&vy).map(|(a, b)| (a - b).powi(2)).sum::<f64>()
                / (stats::variance(&vy) * vy.len() as f64);
        assert!(r2 > 0.95, "r2 {r2}");
    }
}
