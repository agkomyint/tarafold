//! Support search beyond the elastic-net path: greedy forward selection and exhaustive search
//! over small supports, both scored exactly from a centered Gram matrix (intercept model).
//!
//! The lasso path alone misses exact supports when basis terms are correlated (e.g. `price`,
//! `log price`, `sqrt price` over a narrow range). Exhaustive search of 1–3 terms recovers
//! the small true supports that most real laws have.

use std::collections::BTreeSet;

use tarafold_core::{linalg, stats};

struct Gram {
    g: Vec<Vec<f64>>,
    b: Vec<f64>,
    yy: f64,
}

fn gram(cols: &[&[f64]], y: &[f64]) -> Option<Gram> {
    let n = y.len();
    let ym = stats::mean(y);
    let yc: Vec<f64> = y.iter().map(|v| v - ym).collect();
    let z: Vec<Vec<f64>> = cols
        .iter()
        .map(|c| {
            let m = stats::mean(c);
            let centered: Vec<f64> = c.iter().map(|v| v - m).collect();
            let s = centered.iter().map(|v| v * v).sum::<f64>().sqrt();
            centered.iter().map(|v| v / s).collect()
        })
        .collect();
    let p = z.len();
    let mut g = vec![vec![0.0; p]; p];
    for i in 0..p {
        for j in i..p {
            let v: f64 = (0..n).map(|k| z[i][k] * z[j][k]).sum();
            g[i][j] = v;
            g[j][i] = v;
        }
    }
    let b: Vec<f64> = z
        .iter()
        .map(|c| c.iter().zip(&yc).map(|(a, b)| a * b).sum())
        .collect();
    let yy: f64 = yc.iter().map(|v| v * v).sum();
    (yy > 0.0 && g.iter().flatten().all(|v| v.is_finite())).then_some(Gram { g, b, yy })
}

impl Gram {
    /// Residual sum of squares for the least-squares fit on support `s` (plus intercept).
    fn rss(&self, s: &[usize]) -> f64 {
        let m: Vec<Vec<f64>> = s
            .iter()
            .map(|&i| s.iter().map(|&j| self.g[i][j]).collect())
            .collect();
        let bs: Vec<f64> = s.iter().map(|&i| self.b[i]).collect();
        let Some(l) = linalg::cholesky(&m) else {
            return f64::INFINITY;
        };
        // Reject numerically singular supports (pivot ratio below 1e-6 → condition > 1e12).
        let d: Vec<f64> = (0..s.len()).map(|i| l[i][i]).collect();
        let dmax = d.iter().copied().fold(0.0, f64::max);
        if d.iter().any(|&v| v < 1e-6 * dmax) {
            return f64::INFINITY;
        }
        let x = linalg::cholesky_solve(&l, &bs);
        let explained: f64 = x.iter().zip(&bs).map(|(a, b)| a * b).sum();
        (self.yy - explained).max(0.0)
    }
}

/// Candidate supports: forward selection up to `max_k`, plus the best few supports of each
/// size from exhaustive search over pairs (≤ 400 terms) and triples (≤ 80 terms).
pub(crate) fn supports(cols: &[&[f64]], y: &[f64], max_k: usize) -> Vec<Vec<usize>> {
    let p = cols.len();
    let Some(gr) = gram(cols, y) else {
        return Vec::new();
    };
    let mut out: BTreeSet<Vec<usize>> = BTreeSet::new();

    // Forward selection.
    let mut cur: Vec<usize> = Vec::new();
    for _ in 0..max_k.min(p) {
        let mut best: Option<(f64, usize)> = None;
        for j in 0..p {
            if cur.contains(&j) {
                continue;
            }
            let mut s = cur.clone();
            s.push(j);
            s.sort_unstable();
            let r = gr.rss(&s);
            if best.is_none_or(|(br, _)| r < br) {
                best = Some((r, j));
            }
        }
        let Some((r, j)) = best else { break };
        if !r.is_finite() {
            break;
        }
        cur.push(j);
        cur.sort_unstable();
        out.insert(cur.clone());
        if r <= 1e-24 * gr.yy {
            break;
        }
    }

    // Exhaustive small supports; keep the 3 best per size.
    let keep_best = |scored: &mut Vec<(f64, Vec<usize>)>, out: &mut BTreeSet<Vec<usize>>| {
        scored.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        for (r, s) in scored.iter().take(3) {
            if r.is_finite() {
                out.insert(s.clone());
            }
        }
    };
    let mut singles: Vec<(f64, Vec<usize>)> = (0..p).map(|i| (gr.rss(&[i]), vec![i])).collect();
    keep_best(&mut singles, &mut out);
    if p <= 400 && max_k >= 2 {
        let mut pairs = Vec::new();
        for i in 0..p {
            for j in (i + 1)..p {
                pairs.push((gr.rss(&[i, j]), vec![i, j]));
            }
        }
        keep_best(&mut pairs, &mut out);
    }
    if p <= 80 && max_k >= 3 {
        let mut triples = Vec::new();
        for i in 0..p {
            for j in (i + 1)..p {
                for k in (j + 1)..p {
                    triples.push((gr.rss(&[i, j, k]), vec![i, j, k]));
                }
            }
        }
        keep_best(&mut triples, &mut out);
    }
    out.into_iter().collect()
}
