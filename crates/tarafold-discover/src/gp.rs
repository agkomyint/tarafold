//! Genetic-programming symbolic regression (design §5b).
//!
//! Islands of expressions evolve by regularized evolution (tournament selection, replace the
//! oldest). Constants are tuned by Levenberg–Marquardt. A hall of fame keeps the best
//! expression per complexity. Search runs in epochs: within an epoch every island evolves
//! independently with its own RNG stream `(seed, island, epoch)`; migration happens between
//! epochs in island order. Results are therefore identical for any thread count.

use rayon::prelude::*;
use tarafold_core::config::GpConfig;
use tarafold_core::{Rng, rng::stream, stats};
use tarafold_expr::{BinOp, Expr, Node, UnOp};

use crate::lm;

const BINARY: [BinOp; 4] = [BinOp::Add, BinOp::Sub, BinOp::Mul, BinOp::Div];
/// Log-loss penalty per unit of complexity, one per island (cycled). Diverse pressure keeps some
/// islands exploring small expressions while others refine accurate large ones; the Pareto
/// front and validation decide among them later.
const PARSIMONY: [f64; 4] = [0.01, 0.05, 0.2, 0.5];
const LOSS_FLOOR: f64 = 1e-14;

#[derive(Clone)]
struct Member {
    expr: Expr,
    loss: f64,
    complexity: usize,
    birth: u64,
}

impl Member {
    fn score(&self, parsimony: f64) -> f64 {
        self.loss.max(LOSS_FLOOR).ln() + parsimony * self.complexity as f64
    }
}

/// Fitness data shared by all islands.
struct Data {
    x: Vec<Vec<f64>>,
    y: Vec<f64>,
    var_y: f64,
    n_vars: u16,
    unary: Vec<UnOp>,
}

/// Top-level additive terms of `e` with constant factors stripped: `3*f(x) - g(x) + 2` gives
/// `[f(x), g(x)]`. Their coefficients and the intercept are then solved by least squares.
fn additive_terms(e: &Expr, out: &mut Vec<Expr>) {
    let n = e.len();
    match e.nodes()[n - 1] {
        Node::Binary(BinOp::Add | BinOp::Sub) => {
            let right_end = n - 2;
            let left_end = e.subtree_start(right_end) - 1;
            additive_terms(&e.subtree(left_end), out);
            additive_terms(&e.subtree(right_end), out);
        }
        Node::Unary(UnOp::Neg) => additive_terms(&e.subtree(n - 2), out),
        Node::Binary(BinOp::Mul) => {
            let right_end = n - 2;
            let left_end = e.subtree_start(right_end) - 1;
            let (l, r) = (e.subtree(left_end), e.subtree(right_end));
            match (l.nodes(), r.nodes()) {
                ([Node::Const(_)], _) => additive_terms(&r, out),
                (_, [Node::Const(_)]) => additive_terms(&l, out),
                _ => out.push(e.clone()),
            }
        }
        Node::Const(_) => {}
        _ => out.push(e.clone()),
    }
}

/// Fitness view: an expression's additive terms with least-squares coefficients
/// (multi-gene linear scaling).
struct Scaled {
    nmse: f64,
    terms: Vec<Expr>,
    coefs: Vec<f64>,
}

impl Data {
    fn scaled(&self, e: &Expr) -> Scaled {
        let bad = Scaled {
            nmse: f64::INFINITY,
            terms: Vec::new(),
            coefs: Vec::new(),
        };
        let mut terms = Vec::new();
        additive_terms(e, &mut terms);
        if terms.len() > 8 {
            return bad;
        }
        let n = self.y.len();
        let refs: Vec<&[f64]> = self.x.iter().map(Vec::as_slice).collect();
        let mut cols = vec![vec![1.0; n]];
        for t in &terms {
            let Some(v) = t.eval_columns(&refs, n) else {
                return bad;
            };
            if v.iter().any(|x| !x.is_finite()) {
                return bad;
            }
            cols.push(v);
        }
        let Some(coefs) = tarafold_core::linalg::lstsq(&cols, &self.y) else {
            return bad;
        };
        let mse = (0..n)
            .map(|i| {
                let p: f64 = cols.iter().zip(&coefs).map(|(c, b)| c[i] * b).sum();
                (p - self.y[i]).powi(2)
            })
            .sum::<f64>()
            / n as f64;
        if !mse.is_finite() {
            return bad;
        }
        Scaled {
            nmse: mse / self.var_y,
            terms,
            coefs,
        }
    }

    fn nmse(&self, e: &Expr) -> f64 {
        self.scaled(e).nmse
    }
}

/// `c0 + Σ c_i · t_i`, with coefficients within 1e-9 (relative to the largest) of an integer
/// snapped to it, so `2.5e-17 + 1.0000000001 * f` prints as `f`.
fn apply_scaling(s: &Scaled) -> Expr {
    let scale = s.coefs.iter().map(|c| c.abs()).fold(1.0, f64::max);
    let snap = |c: f64| {
        if (c - c.round()).abs() < 1e-9 * scale {
            c.round()
        } else {
            c
        }
    };
    let mut acc = Expr::constant(snap(s.coefs[0]));
    for (t, &b) in s.terms.iter().zip(&s.coefs[1..]) {
        let b = snap(b);
        let term = if b == 1.0 {
            t.clone()
        } else {
            Expr::constant(b) * t.clone()
        };
        acc = acc + term;
    }
    acc.simplify()
}
fn random_const(rng: &mut Rng) -> f64 {
    match rng.below(4) {
        0 => 1.0,
        1 => 2.0,
        2 => 0.5,
        _ => (rng.normal() * 3.0 * 100.0).round() / 100.0,
    }
}

fn random_tree(rng: &mut Rng, depth: usize, n_vars: u16, unary: &[UnOp]) -> Expr {
    if depth == 0 || rng.uniform() < 0.3 {
        return if rng.uniform() < 0.65 {
            Expr::var(rng.below(n_vars as usize) as u16)
        } else {
            Expr::constant(random_const(rng))
        };
    }
    if !unary.is_empty() && rng.uniform() < 0.25 {
        random_tree(rng, depth - 1, n_vars, unary).unary(unary[rng.below(unary.len())])
    } else {
        let a = random_tree(rng, depth - 1, n_vars, unary);
        let b = random_tree(rng, depth - 1, n_vars, unary);
        Expr::binary(BINARY[rng.below(BINARY.len())], a, b)
    }
}

fn mutate(rng: &mut Rng, e: &Expr, other: &Expr, n_vars: u16, unary: &[UnOp]) -> Expr {
    let n = e.len();
    let i = rng.below(n);
    let node = e.nodes()[i];
    match rng.below(8) {
        // Perturb a constant.
        0 => {
            let consts: Vec<usize> = (0..n)
                .filter(|&k| matches!(e.nodes()[k], Node::Const(_)))
                .collect();
            if let Some(&k) = consts.get(rng.below(consts.len().max(1)))
                && let Node::Const(c) = e.nodes()[k]
            {
                let v = if rng.uniform() < 0.5 {
                    c * (rng.normal() * 0.5).exp()
                } else {
                    c + rng.normal()
                };
                if let Some(m) = e.with_node(k, Node::Const(v)) {
                    return m;
                }
            }
            e.clone()
        }
        // Swap an operator or terminal for another of the same arity.
        1 => {
            let repl = match node {
                Node::Binary(_) => Node::Binary(BINARY[rng.below(BINARY.len())]),
                Node::Unary(op) if unary.is_empty() => Node::Unary(op),
                Node::Unary(_) => Node::Unary(unary[rng.below(unary.len())]),
                Node::Var(_) => Node::Var(rng.below(n_vars as usize) as u16),
                Node::Const(_) => Node::Const(random_const(rng)),
            };
            e.with_node(i, repl).unwrap_or_else(|| e.clone())
        }
        // Replace a subtree with a random one.
        2 => e.replace_subtree(i, &random_tree(rng, 2, n_vars, unary)),
        // Wrap a subtree in a unary op.
        3 if !unary.is_empty() => {
            e.replace_subtree(i, &e.subtree(i).unary(unary[rng.below(unary.len())]))
        }
        // Combine a subtree with a new terminal via a binary op.
        4 => {
            let t = random_tree(rng, 0, n_vars, unary);
            let s = e.subtree(i);
            let op = BINARY[rng.below(BINARY.len())];
            let new = if rng.uniform() < 0.5 {
                Expr::binary(op, s, t)
            } else {
                Expr::binary(op, t, s)
            };
            e.replace_subtree(i, &new)
        }
        // Delete an operator: keep one of its children.
        5 => match node {
            Node::Unary(_) if i > 0 => e.replace_subtree(i, &e.subtree(i - 1)),
            Node::Binary(_) if i > 0 => {
                let right_end = i - 1;
                let left_end = e.subtree_start(right_end).saturating_sub(1);
                let keep = if rng.uniform() < 0.5 {
                    right_end
                } else {
                    left_end
                };
                e.replace_subtree(i, &e.subtree(keep))
            }
            _ => e.clone(),
        },
        // Crossover: insert a subtree of another parent.
        6 => {
            let j = rng.below(other.len());
            e.replace_subtree(i, &other.subtree(j))
        }
        // Simplify.
        _ => e.simplify(),
    }
}

struct Island {
    pop: Vec<Member>,
    parsimony: f64,
    next_birth: u64,
    evals: usize,
}

impl Island {
    fn tournament(&self, rng: &mut Rng) -> usize {
        let k = 8.min(self.pop.len());
        let mut picks: Vec<usize> = (0..k).map(|_| rng.below(self.pop.len())).collect();
        picks.sort_by(|&a, &b| {
            self.pop[a]
                .score(self.parsimony)
                .total_cmp(&self.pop[b].score(self.parsimony))
                .then(a.cmp(&b))
        });
        // Best with probability 0.9, otherwise the next one.
        if rng.uniform() < 0.9 || picks.len() < 2 {
            picks[0]
        } else {
            picks[1]
        }
    }

    fn evolve(&mut self, data: &Data, cfg: &GpConfig, rng: &mut Rng) {
        for _ in 0..cfg.epoch_steps {
            let a = self.tournament(rng);
            let b = self.tournament(rng);
            let mut child = mutate(
                rng,
                &self.pop[a].expr,
                &self.pop[b].expr,
                data.n_vars,
                &data.unary,
            );
            if child.complexity() > cfg.max_complexity || child.len() > 4 * cfg.max_complexity {
                continue;
            }
            let n_const = child.constants().len();
            if n_const > 0
                && n_const <= 6
                && rng.uniform() < 0.25
                && let Some(refit) = lm::refine(&child, &data.x, &data.y, 8)
            {
                child = refit;
                self.evals += 8;
            }
            let loss = data.nmse(&child);
            self.evals += 1;
            if !loss.is_finite() {
                continue;
            }
            let oldest = (0..self.pop.len())
                .min_by_key(|&k| self.pop[k].birth)
                .unwrap_or(0);
            self.pop[oldest] = Member {
                complexity: child.complexity(),
                expr: child,
                loss,
                birth: self.next_birth,
            };
            self.next_birth += 1;
        }
    }
}

/// Run GP and return the hall of fame (best expression per complexity), each refit by LM.
pub(crate) fn search(
    train_x: &[Vec<f64>],
    train_y: &[f64],
    seeds: &[Expr],
    cfg: &GpConfig,
    unary: Vec<UnOp>,
    seed: u64,
) -> (Vec<Expr>, usize) {
    let n = train_y.len();
    let p = train_x.len();
    if p == 0 || n < 10 || cfg.islands == 0 || cfg.population < 4 {
        return (Vec::new(), 0);
    }
    // Fixed fitness subsample.
    let mut rows: Vec<usize> = (0..n).collect();
    if n > cfg.rows {
        Rng::derive(seed, &[stream::DISCOVER, 0]).shuffle(&mut rows);
        rows.truncate(cfg.rows);
        rows.sort_unstable();
    }
    let y: Vec<f64> = rows.iter().map(|&i| train_y[i]).collect();
    let var_y = stats::variance(&y);
    if var_y.is_nan() || var_y <= 0.0 {
        return (Vec::new(), 0);
    }
    let data = Data {
        x: train_x
            .iter()
            .map(|c| rows.iter().map(|&i| c[i]).collect())
            .collect(),
        y,
        var_y,
        n_vars: p.min(u16::MAX as usize) as u16,
        unary,
    };

    // Initial populations: seeds spread across islands, the rest random.
    let mut islands: Vec<Island> = (0..cfg.islands)
        .map(|isl| {
            let mut rng = Rng::derive(seed, &[stream::DISCOVER, 1, isl as u64]);
            let mut pop = Vec::with_capacity(cfg.population);
            // Seeds go to even islands only, so odd islands search from scratch.
            let seeded: Vec<usize> = (0..cfg.islands).filter(|i| i % 2 == 0).collect();
            for (k, s) in seeds.iter().enumerate() {
                if isl % 2 == 0 && seeded[k % seeded.len()] == isl && pop.len() < cfg.population / 2
                {
                    pop.push(s.clone());
                }
            }
            while pop.len() < cfg.population {
                let depth = 1 + rng.below(4);
                pop.push(random_tree(&mut rng, depth, data.n_vars, &data.unary));
            }
            let pop = pop
                .into_iter()
                .enumerate()
                .map(|(b, expr)| {
                    let loss = data.nmse(&expr);
                    Member {
                        complexity: expr.complexity(),
                        expr,
                        loss: if loss.is_finite() {
                            loss
                        } else {
                            f64::INFINITY
                        },
                        birth: b as u64,
                    }
                })
                .collect();
            let depth_pop = pop;
            Island {
                pop: depth_pop,
                parsimony: PARSIMONY[isl % PARSIMONY.len()],
                next_birth: cfg.population as u64,
                evals: cfg.population,
            }
        })
        .collect();

    let mut hof: Vec<Option<Member>> = vec![None; cfg.max_complexity + 1];
    let mut epoch: u64 = 0;
    let total = |isl: &[Island]| isl.iter().map(|i| i.evals).sum::<usize>();
    while total(&islands) < cfg.max_evals {
        islands.par_iter_mut().enumerate().for_each(|(k, isl)| {
            let mut rng = Rng::derive(seed, &[stream::DISCOVER, 2, k as u64, epoch]);
            isl.evolve(&data, cfg, &mut rng);
        });
        // Hall of fame, in island order.
        for isl in &islands {
            for m in &isl.pop {
                if m.complexity <= cfg.max_complexity && m.loss.is_finite() {
                    let slot = &mut hof[m.complexity];
                    if slot.as_ref().is_none_or(|h| m.loss < h.loss) {
                        *slot = Some(m.clone());
                    }
                }
            }
        }
        // Ring migration: each island's best 3 replace the next island's oldest 3.
        let k = islands.len();
        if k > 1 {
            let bests: Vec<Vec<Member>> = islands
                .iter()
                .map(|isl| {
                    let mut idx: Vec<usize> = (0..isl.pop.len()).collect();
                    idx.sort_by(|&a, &b| {
                        isl.pop[a]
                            .score(isl.parsimony)
                            .total_cmp(&isl.pop[b].score(isl.parsimony))
                            .then(a.cmp(&b))
                    });
                    idx.iter().take(3).map(|&i| isl.pop[i].clone()).collect()
                })
                .collect();
            for (i, isl) in islands.iter_mut().enumerate() {
                for m in &bests[(i + k - 1) % k] {
                    let oldest = (0..isl.pop.len())
                        .min_by_key(|&j| isl.pop[j].birth)
                        .unwrap_or(0);
                    let mut m = m.clone();
                    m.birth = isl.next_birth;
                    isl.next_birth += 1;
                    isl.pop[oldest] = m;
                }
            }
        }
        epoch += 1;
    }

    let evals = total(&islands);
    let out: Vec<Expr> = hof
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let s = data.scaled(&m.expr);
            s.nmse.is_finite().then(|| apply_scaling(&s))
        })
        .flat_map(|e| {
            let refit = lm::refine(&e, train_x, train_y, 50);
            // Keep both the scaled form and its full refit; the Pareto front decides.
            std::iter::once(e).chain(refit)
        })
        .collect();
    (out, evals)
}
