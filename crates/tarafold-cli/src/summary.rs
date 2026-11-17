//! Human-readable summary printed after `tarafold run`.

use std::fmt::Write as _;
use std::path::Path;

use tarafold_report::Report;

fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "n/a".into();
    }
    let a = v.abs();
    if a >= 1e6 || (a < 1e-3 && a > 0.0) {
        format!("{v:.4e}")
    } else if a >= 100.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.4}")
    }
}

/// Render the summary.
pub fn render(r: &Report, out: &Path) -> String {
    let mut s = String::new();
    let d = &r.dataset;
    let excluded = &d.source.excluded;
    let _ = write!(
        s,
        "Dataset   {}  {} rows × {} numeric columns",
        d.source.path,
        d.n_rows,
        d.columns.len()
    );
    if !excluded.is_empty() {
        let names: Vec<String> = excluded
            .iter()
            .map(|e| format!("{} ({})", e.name, e.reason))
            .collect();
        let _ = write!(s, "  [excluded: {}]", names.join(", "));
    }
    s.push('\n');
    for t in &r.targets {
        let sel = &t.discover.front[t.discover.selected_id];
        let v = &t.validate;
        let _ = writeln!(s, "Target    {}", t.target);
        let _ = writeln!(s, "Selected  {} = {}", t.target, sel.infix);
        let _ = writeln!(
            s,
            "          R²(test) {:.6}  RMSE(test) {}  grade: {:?}  evidence: {:?}",
            sel.metrics.test.r2,
            fmt_num(sel.metrics.test.rmse),
            v.reliability.grade,
            v.evidence
        );
        if let Some(st) = &v.stability {
            let _ = writeln!(
                s,
                "          stability {}/{} resamples   90% interval ±{}   coverage(test) {:.2}",
                st.equivalent,
                st.resamples,
                fmt_num(v.interval_halfwidth_90),
                v.test_coverage_90
            );
        }
        let failed: Vec<&str> = v
            .reliability
            .checks
            .iter()
            .filter(|c| c.passed == Some(false))
            .map(|c| c.condition.as_str())
            .collect();
        if !failed.is_empty() {
            let _ = writeln!(s, "          not met: {}", failed.join("; "));
        }
        let _ = writeln!(
            s,
            "Front     {} candidates (simplest → most accurate):",
            t.discover.front.len()
        );
        for e in &t.discover.front {
            let mark = if e.id == t.discover.selected_id {
                "*"
            } else {
                " "
            };
            let mut eq = e.infix.clone();
            if eq.chars().count() > 70 {
                eq = eq.chars().take(67).collect::<String>() + "...";
            }
            let _ = writeln!(
                s,
                "  {mark}{:>2}  c={:<3} R²={:<9.6} {}",
                e.id, e.complexity, e.metrics.val.r2, eq
            );
        }
    }
    if let Some(o) = r.optimize.as_ref().filter(|o| !o.goal.is_finite()) {
        let _ = writeln!(
            s,
            "Goals     {}  ({})",
            o.goals
                .iter()
                .map(|g| {
                    let kind = if g.hard { "require" } else { "prefer" };
                    format!("{kind} {}", g.text)
                })
                .collect::<Vec<_>>()
                .join("; "),
            o.method
        );
        if o.candidates.is_empty() {
            let _ = writeln!(
                s,
                "  No candidate satisfies every hard goal and constraint."
            );
        }
        for c in &o.candidates {
            let inputs: Vec<String> = o
                .controllables
                .iter()
                .map(|n| format!("{n} {}", fmt_num(c.inputs[n])))
                .collect();
            let preds: Vec<String> = c
                .predictions
                .iter()
                .map(|(k, v)| format!("{k} {}", fmt_num(*v)))
                .collect();
            let p = if c.joint_probability.is_finite() {
                format!("  P(all hard goals) {:.2}", c.joint_probability)
            } else {
                String::new()
            };
            let marks = if c.marks.is_empty() {
                String::new()
            } else {
                format!("  [{}]", c.marks.join(", "))
            };
            let _ = writeln!(
                s,
                "  #{:<2} {}  →  {}{p}{marks}{}",
                c.id,
                inputs.join("  "),
                preds.join("  "),
                if c.in_domain {
                    ""
                } else {
                    "  outside data domain"
                }
            );
        }
        if let Some(k) = &o.conflict {
            let _ = writeln!(
                s,
                "  Conflict: no input meets this minimal set of goals: {} (other conflicts may also exist)",
                k.goals.join(", ")
            );
            for (g, v) in &k.best_achievable {
                let _ = writeln!(
                    s,
                    "    best achievable for `{g}` with the others held: {}",
                    fmt_num(*v)
                );
            }
        }
        let _ = writeln!(s, "Note      {}", o.disclaimer);
    }
    if let Some(o) = r.optimize.as_ref().filter(|o| o.goal.is_finite()) {
        let n_ok = o.candidates.iter().filter(|c| c.within_tolerance).count();
        let _ = writeln!(
            s,
            "Goal      {} = {}  →  {} candidate(s) within ±{}{}",
            o.target,
            fmt_num(o.goal),
            n_ok,
            fmt_num(o.tolerance),
            if o.reached {
                ""
            } else {
                "  (goal not reached)"
            }
        );
        for c in &o.candidates {
            let inputs: Vec<String> = o
                .controllables
                .iter()
                .map(|n| {
                    let pct = c
                        .change_pct
                        .get(n)
                        .copied()
                        .flatten()
                        .map_or(String::new(), |p| format!(" ({p:+.1}%)"));
                    format!("{n} {}{pct}", fmt_num(c.inputs[n]))
                })
                .collect();
            let _ = writeln!(
                s,
                "  #{}  {}   ŷ = {} [{}, {}]{}",
                c.id,
                inputs.join("  "),
                fmt_num(c.predicted),
                fmt_num(c.interval[0]),
                fmt_num(c.interval[1]),
                if c.in_domain {
                    ""
                } else {
                    "  outside data domain"
                }
            );
        }
        if !o.binding.is_empty() {
            let _ = writeln!(s, "  Binding: {}", o.binding.join(", "));
        }
        let _ = writeln!(s, "Note      {}", o.disclaimer);
    }
    for w in &r.warnings {
        let _ = writeln!(s, "Warning   {}", w.message);
    }
    let _ = writeln!(s, "Wrote     {}", out.join("report.json").display());
    s
}
