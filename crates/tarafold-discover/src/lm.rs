//! Levenberg–Marquardt refit of an expression's constants in the original units of `y`
//! (design §5b "constant fitting"). Jacobians are exact (forward-mode over constants).

use tarafold_core::linalg;
use tarafold_expr::Expr;

fn sse(expr: &Expr, x: &[&[f64]], y: &[f64]) -> Option<(f64, Vec<f64>)> {
    let pred = expr.eval_columns(x, y.len())?;
    let r: Vec<f64> = pred.iter().zip(y).map(|(p, t)| p - t).collect();
    let s: f64 = r.iter().map(|v| v * v).sum();
    s.is_finite().then_some((s, r))
}

/// Refit constants to minimize squared error. Returns `None` when the start point is not
/// finite or no improvement was found.
pub(crate) fn refine(expr: &Expr, x: &[Vec<f64>], y: &[f64], max_iter: usize) -> Option<Expr> {
    let refs: Vec<&[f64]> = x.iter().map(Vec::as_slice).collect();
    let mut p = expr.constants();
    let k = p.len();
    if k == 0 || k > 40 {
        return None;
    }
    let (mut cur_sse, mut r) = sse(expr, &refs, y)?;
    let start = cur_sse;
    let mut cur = expr.clone();
    let mut lambda = 1e-3;
    for _ in 0..max_iter {
        // Jacobian columns J[:, i] = ∂pred/∂p_i, exact by forward-mode differentiation.
        let mut jac: Vec<Vec<f64>> = Vec::with_capacity(k);
        for i in 0..k {
            let (_, col) = cur.eval_columns_const_grad(&refs, y.len(), i)?;
            if col.iter().any(|v| !v.is_finite()) {
                return (cur_sse < start).then_some(cur);
            }
            jac.push(col);
        }
        let mut jtj = vec![vec![0.0; k]; k];
        let mut jtr = vec![0.0; k];
        for i in 0..k {
            for j in i..k {
                let v: f64 = jac[i].iter().zip(&jac[j]).map(|(a, b)| a * b).sum();
                jtj[i][j] = v;
                jtj[j][i] = v;
            }
            jtr[i] = -jac[i].iter().zip(&r).map(|(a, b)| a * b).sum::<f64>();
        }
        let mut accepted = false;
        while lambda < 1e12 {
            let mut m = jtj.clone();
            for i in 0..k {
                m[i][i] += lambda * jtj[i][i].max(1e-12);
            }
            if let Some(l) = linalg::cholesky(&m) {
                let delta = linalg::cholesky_solve(&l, &jtr);
                let trial_p: Vec<f64> = p.iter().zip(&delta).map(|(a, d)| a + d).collect();
                let trial = cur.with_constants(&trial_p);
                if let Some((s, rr)) = sse(&trial, &refs, y)
                    && s < cur_sse
                {
                    let rel = (cur_sse - s) / cur_sse.max(1e-300);
                    p = trial_p;
                    cur = trial;
                    cur_sse = s;
                    r = rr;
                    lambda = (lambda / 3.0).max(1e-12);
                    accepted = true;
                    if rel < 1e-12 {
                        return Some(cur);
                    }
                    break;
                }
            }
            lambda *= 4.0;
        }
        if !accepted {
            break;
        }
    }
    (cur_sse < start).then_some(cur)
}
