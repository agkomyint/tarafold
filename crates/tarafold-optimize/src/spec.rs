//! Constraint, goal, and input-kind specifications, parsed from user text by dedicated grammars
//! (never evaluated as code, R14.2).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tarafold_core::Rng;
use tarafold_expr::parse;

/// Comparison in a linear constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cmp {
    /// `≤`
    Le,
    /// `≥`
    Ge,
    /// `=`
    Eq,
}

/// `Σ coef_j · x_j  (≤ | ≥ | =)  rhs` over the full feature vector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Linear {
    /// Original text.
    pub text: String,
    /// `(feature index, coefficient)` pairs.
    pub coefs: Vec<(usize, f64)>,
    /// Comparison.
    pub cmp: Cmp,
    /// Right-hand side (constant terms of the left side already moved over).
    pub rhs: f64,
}

impl Linear {
    /// Left-hand value at `x`.
    pub fn lhs(&self, x: &[f64]) -> f64 {
        self.coefs.iter().map(|&(j, c)| c * x[j]).sum()
    }

    /// Violation at `x` (0 when satisfied), relative to the constraint's scale. Violations up
    /// to 1e-6 of the scale count as satisfied, so solutions on the boundary are kept.
    pub fn violation(&self, x: &[f64], scale: &[f64]) -> f64 {
        let s = self.rhs.abs()
            + self
                .coefs
                .iter()
                .map(|&(j, c)| (c * scale[j]).abs())
                .sum::<f64>();
        let s = s.max(1e-12);
        let d = self.lhs(x) - self.rhs;
        let v = match self.cmp {
            Cmp::Le => d.max(0.0) / s,
            Cmp::Ge => (-d).max(0.0) / s,
            Cmp::Eq => d.abs() / s,
        };
        if v <= 1e-6 { 0.0 } else { v }
    }
}

fn split_cmp(s: &str) -> Option<(&str, Cmp, &str)> {
    for (tok, cmp) in [
        ("<=", Cmp::Le),
        (">=", Cmp::Ge),
        ("==", Cmp::Eq),
        ("=", Cmp::Eq),
    ] {
        if let Some(i) = s.find(tok) {
            return Some((&s[..i], cmp, &s[i + tok.len()..]));
        }
    }
    None
}

/// Parse `linear_expr (<= | >= | =) number` over `names`. Rejects non-linear expressions by
/// checking that the left side equals its first-order expansion at random points.
pub fn parse_constraint(text: &str, names: &[String]) -> Result<Linear, String> {
    let (lhs, cmp, rhs) =
        split_cmp(text).ok_or_else(|| format!("constraint `{text}` needs <=, >= or ="))?;
    let rhs: f64 = rhs
        .trim()
        .parse()
        .ok()
        .filter(|v: &f64| v.is_finite())
        .ok_or_else(|| format!("right side of `{text}` must be a number"))?;
    let e = parse(lhs.trim(), names).map_err(|e| format!("in constraint `{text}`: {e}"))?;
    let p = names.len();
    let zero = vec![0.0; p];
    let c0 = e.eval_point(&zero);
    let grad = e.gradient(&zero, p);
    if !c0.is_finite() || grad.iter().any(|g| !g.is_finite()) {
        return Err(format!("constraint `{text}` is not linear"));
    }
    let mut r = Rng::derive(17, &[]);
    for _ in 0..5 {
        let x: Vec<f64> = (0..p).map(|_| r.range(-100.0, 100.0)).collect();
        let lin = c0 + grad.iter().zip(&x).map(|(g, v)| g * v).sum::<f64>();
        let v = e.eval_point(&x);
        if !v.is_finite() || (v - lin).abs() > 1e-9 * lin.abs().max(1.0) {
            return Err(format!(
                "constraint `{text}` is not linear; only sums of constant × column are allowed"
            ));
        }
    }
    let coefs: Vec<(usize, f64)> = grad
        .iter()
        .enumerate()
        .filter(|(_, g)| **g != 0.0)
        .map(|(j, g)| (j, *g))
        .collect();
    if coefs.is_empty() {
        return Err(format!("constraint `{text}` does not involve any column"));
    }
    Ok(Linear {
        text: text.trim().to_string(),
        coefs,
        cmp,
        rhs: rhs - c0,
    })
}

/// How a controllable may vary.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Any real value.
    Continuous,
    /// Whole numbers.
    Integer,
    /// Multiples of a step from the lower bound.
    Step(f64),
}

impl Kind {
    /// Grid spacing, if discrete.
    pub fn step(self) -> Option<f64> {
        match self {
            Self::Continuous => None,
            Self::Integer => Some(1.0),
            Self::Step(s) => Some(s),
        }
    }

    /// Round bounds inward to the grid. Integers use absolute integers; steps are counted from
    /// the rounded-up lower bound.
    pub fn round_bounds(self, [lo, hi]: [f64; 2]) -> Option<[f64; 2]> {
        match self {
            Self::Continuous => Some([lo, hi]),
            Self::Integer => {
                let (l, h) = (lo.ceil(), hi.floor());
                (l <= h).then_some([l, h])
            }
            Self::Step(s) => {
                let l = (lo / s).ceil() * s;
                let h = l + ((hi - l) / s).floor() * s;
                (l <= h + 1e-12).then_some([l, h.max(l)])
            }
        }
    }

    /// Snap a value to the grid within `[lo, hi]` (bounds already rounded).
    pub fn snap(self, v: f64, lo: f64, hi: f64) -> f64 {
        match self.step() {
            None => v.clamp(lo, hi),
            Some(s) => (lo + ((v - lo) / s).round() * s).clamp(lo, hi),
        }
    }
}

/// What a goal asks of a target.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum GoalKind {
    /// `col = v`
    Exact(f64),
    /// `col >= v`
    AtLeast(f64),
    /// `col <= v`
    AtMost(f64),
    /// `col = a..b`
    Range(f64, f64),
    /// `max:col`
    Maximize,
    /// `min:col`
    Minimize,
}

impl GoalKind {
    /// Is this an objective (max/min)?
    pub fn is_objective(self) -> bool {
        matches!(self, Self::Maximize | Self::Minimize)
    }
}

/// A parsed goal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GoalSpec {
    /// Original text.
    pub text: String,
    /// Target column.
    pub target: String,
    /// Kind.
    pub kind: GoalKind,
    /// Must hold (`--require`) or traded off (`--prefer`).
    pub hard: bool,
    /// Weight for soft goals and preferences.
    pub weight: f64,
}

fn num(s: &str, text: &str) -> Result<f64, String> {
    s.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("invalid number `{}` in goal `{text}`", s.trim()))
}

/// Parse `max:col[:w]`, `min:col[:w]`, `col>=v[:w]`, `col<=v[:w]`, `col=v[:w]`, `col=a..b[:w]`.
pub fn parse_goal(text: &str, hard: bool) -> Result<GoalSpec, String> {
    let t = text.trim();
    let parts: Vec<&str> = t.split(':').collect();
    let (body, weight) = match parts.as_slice() {
        ["max" | "min", col] => (format!("{}:{}", parts[0], col), 1.0),
        ["max" | "min", col, w] => (format!("{}:{}", parts[0], col), num(w, t)?),
        [b] => ((*b).to_string(), 1.0),
        [b, w] => ((*b).to_string(), num(w, t)?),
        _ => return Err(format!("cannot parse goal `{t}`")),
    };
    if weight.is_nan() || weight <= 0.0 {
        return Err(format!("goal weight must be positive in `{t}`"));
    }
    let (target, kind) = if let Some(col) = body.strip_prefix("max:") {
        (col.trim().to_string(), GoalKind::Maximize)
    } else if let Some(col) = body.strip_prefix("min:") {
        (col.trim().to_string(), GoalKind::Minimize)
    } else {
        let (col, cmp, v) =
            split_cmp(&body).ok_or_else(|| format!("goal `{t}` needs max:, min:, >=, <= or ="))?;
        let kind = match cmp {
            Cmp::Ge => GoalKind::AtLeast(num(v, t)?),
            Cmp::Le => GoalKind::AtMost(num(v, t)?),
            Cmp::Eq => match v.split_once("..") {
                Some((a, b)) => {
                    let (a, b) = (num(a, t)?, num(b, t)?);
                    if a > b {
                        return Err(format!("empty range in goal `{t}`"));
                    }
                    GoalKind::Range(a, b)
                }
                None => GoalKind::Exact(num(v, t)?),
            },
        };
        (col.trim().to_string(), kind)
    };
    if target.is_empty() {
        return Err(format!("goal `{t}` names no column"));
    }
    Ok(GoalSpec {
        text: t.to_string(),
        target,
        kind,
        hard,
        weight,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        ["spend_a", "spend_b", "price"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn linear_constraints() {
        let c = parse_constraint("spend_a + 2*spend_b - 100 <= 5000", &names()).unwrap();
        assert_eq!(c.coefs, vec![(0, 1.0), (1, 2.0)]);
        assert_eq!((c.cmp, c.rhs), (Cmp::Le, 5100.0));
        assert_eq!(c.violation(&[5100.0, 0.0, 0.0], &[1.0; 3]), 0.0);
        assert!(c.violation(&[6000.0, 0.0, 0.0], &[1.0; 3]) > 0.0);
        assert!(
            parse_constraint("spend_a * spend_b <= 5", &names())
                .unwrap_err()
                .contains("not linear")
        );
        assert!(parse_constraint("log(price) >= 1", &names()).is_err());
        assert!(parse_constraint("price > 3", &names()).is_err());
        assert!(parse_constraint("price <= abc", &names()).is_err());
        assert!(parse_constraint("unknown <= 3", &names()).is_err());
    }

    #[test]
    fn goals() {
        let g = parse_goal("max:yield:3", false).unwrap();
        assert_eq!(
            (g.target.as_str(), g.kind, g.weight),
            ("yield", GoalKind::Maximize, 3.0)
        );
        assert_eq!(
            parse_goal("defects<=0.01", true).unwrap().kind,
            GoalKind::AtMost(0.01)
        );
        assert_eq!(
            parse_goal("profit >= 2.5e5", true).unwrap().kind,
            GoalKind::AtLeast(250000.0)
        );
        assert_eq!(
            parse_goal("strength=40..50", true).unwrap().kind,
            GoalKind::Range(40.0, 50.0)
        );
        assert_eq!(
            parse_goal("revenue=1000000", true).unwrap().kind,
            GoalKind::Exact(1e6)
        );
        assert!(parse_goal("revenue", true).is_err());
        assert!(parse_goal("max:y:-1", false).is_err());
        assert!(parse_goal("y=5..1", true).is_err());
    }

    #[test]
    fn kinds() {
        assert_eq!(Kind::Integer.round_bounds([0.5, 10.2]), Some([1.0, 10.0]));
        assert_eq!(Kind::Step(0.5).round_bounds([0.2, 2.9]), Some([0.5, 2.5]));
        assert_eq!(Kind::Integer.snap(3.6, 1.0, 10.0), 4.0);
        assert_eq!(Kind::Step(0.5).snap(1.3, 0.5, 2.5), 1.5);
        assert_eq!(Kind::Integer.round_bounds([0.2, 0.8]), None);
    }
}
