//! Equation export as standalone code (R16): Python, JavaScript, SQL, Excel, LaTeX, JSON.

use std::fmt::Write as _;

use tarafold_core::{DataSource, Evidence};
use tarafold_expr::codegen::{JS_HELPERS, Lang, PYTHON_HELPERS, sanitize_identifiers};
use tarafold_expr::{Expr, parse};

use crate::{EquationReport, Report, ReportError, TargetReport};

/// Export format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// Python function using only `math`.
    Python,
    /// JavaScript function using only `Math`.
    Js,
    /// ANSI SQL `SELECT` expression.
    Sql,
    /// Excel formula over named cells.
    Excel,
    /// LaTeX display equation.
    Latex,
    /// JSON tree with metadata.
    Json,
}

impl ExportFormat {
    /// Parse a `--to` value.
    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "python" | "py" => Self::Python,
            "js" | "javascript" => Self::Js,
            "sql" => Self::Sql,
            "excel" | "xlsx" => Self::Excel,
            "latex" | "tex" => Self::Latex,
            "json" => Self::Json,
            _ => return None,
        })
    }

    /// Usual file extension.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Python => "py",
            Self::Js => "js",
            Self::Sql => "sql",
            Self::Excel => "txt",
            Self::Latex => "tex",
            Self::Json => "json",
        }
    }
}

/// One line of header text, with line breaks removed so names cannot end a comment.
fn one_line(s: &str) -> String {
    s.replace(['\r', '\n'], " ")
}

fn evidence_name(e: Evidence) -> &'static str {
    match e {
        Evidence::Association => "association",
        Evidence::Predictive => "predictive",
        Evidence::FunctionalFit => "functional_fit",
        Evidence::InterventionalFit => "interventional_fit",
    }
}

/// Facts the header states, shared by every format.
struct Header {
    lines: Vec<String>,
}

fn header(
    report: &Report,
    t: &TargetReport,
    eq: &EquationReport,
    mapping: &[(String, String)],
) -> Header {
    let selected = eq.id == t.discover.selected_id;
    let mut lines = vec![
        format!("Exported by Tarafold {}", report.tarafold_version),
        format!("Run: {}", report.run.id),
        format!(
            "Target: {}   equation id {}{}",
            one_line(&t.target),
            eq.id,
            if selected { " (selected)" } else { "" }
        ),
        format!(
            "Equation: {} = {}",
            one_line(&t.target),
            one_line(&eq.infix)
        ),
    ];
    if selected {
        let grade = serde_json::to_value(t.validate.reliability.grade)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        lines.push(format!(
            "Reliability grade: {grade}   Evidence: {}",
            evidence_name(t.validate.evidence)
        ));
    } else {
        lines.push(
            "Reliability grade: not graded (only the selected equation is graded)".to_string(),
        );
    }
    lines.push(format!(
        "Test R2: {:.6}   test RMSE: {:.4e}",
        eq.metrics.test.r2, eq.metrics.test.rmse
    ));
    lines.push("Training range (outside it the equation is an extrapolation):".to_string());
    let d = &t.validate.domain;
    for v in &eq.variables {
        if let Some(j) = d.names.iter().position(|n| n == v) {
            lines.push(format!("  {}: {} .. {}", one_line(v), d.min[j], d.max[j]));
        }
    }
    let renamed: Vec<&(String, String)> = mapping.iter().filter(|(a, b)| a != b).collect();
    if !renamed.is_empty() {
        lines.push("Identifier mapping (column -> name in this code):".to_string());
        for (a, b) in renamed {
            lines.push(format!("  {} -> {b}", one_line(a)));
        }
    }
    lines.push(match report.dataset.data_source {
        DataSource::Observational => {
            "Fitted on observational data: it describes association, not the effect of changing an input."
                .to_string()
        }
        _ => "Fitted on data the user declared as designed; it holds inside the data domain only."
            .to_string(),
    });
    lines.push(
        "Protected operations (division by ~0, log/sqrt of invalid values) return NaN/NULL/#NUM!."
            .to_string(),
    );
    Header { lines }
}

impl Header {
    fn commented(&self, prefix: &str) -> String {
        let mut s = String::new();
        for l in &self.lines {
            let _ = writeln!(s, "{prefix} {l}");
        }
        s
    }
}

/// Select a target (default: the first) and equation (default: the selected one).
fn pick<'a>(
    report: &'a Report,
    target: Option<&str>,
    id: Option<usize>,
) -> Result<(&'a TargetReport, &'a EquationReport), ReportError> {
    let t = match target {
        Some(name) => report
            .targets
            .iter()
            .find(|t| t.target == name)
            .ok_or_else(|| {
                ReportError(format!(
                    "target `{name}` is not in the report (targets: {})",
                    report
                        .targets
                        .iter()
                        .map(|t| t.target.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?,
        None => report
            .targets
            .first()
            .ok_or_else(|| ReportError("the report has no targets".into()))?,
    };
    let want = id.unwrap_or(t.discover.selected_id);
    let eq = t
        .discover
        .front
        .iter()
        .find(|e| e.id == want)
        .ok_or_else(|| {
            ReportError(format!(
                "equation id {want} is not on the front of `{}` (ids 0..{})",
                t.target,
                t.discover.front.len()
            ))
        })?;
    Ok((t, eq))
}

/// Rebuild the engine expression for an equation.
pub fn equation_expr(eq: &EquationReport) -> Result<Expr, ReportError> {
    parse(&eq.infix_exact, &eq.variables)
        .map_err(|e| ReportError(format!("cannot read equation `{}`: {e}", eq.infix_exact)))
}

/// Export an equation as standalone code.
pub fn export(
    report: &Report,
    target: Option<&str>,
    id: Option<usize>,
    format: ExportFormat,
) -> Result<String, ReportError> {
    let (t, eq) = pick(report, target, id)?;
    let expr = equation_expr(eq)?;
    let lang = match format {
        ExportFormat::Python => Some(Lang::Python),
        ExportFormat::Js => Some(Lang::Js),
        ExportFormat::Sql => Some(Lang::Sql),
        ExportFormat::Excel => Some(Lang::Excel),
        ExportFormat::Latex | ExportFormat::Json => None,
    };
    let idents = lang.map_or_else(
        || eq.variables.clone(),
        |l| sanitize_identifiers(&eq.variables, l),
    );
    let mapping: Vec<(String, String)> = eq
        .variables
        .iter()
        .cloned()
        .zip(idents.iter().cloned())
        .collect();
    let h = header(report, t, eq, &mapping);
    let fn_name = {
        let base = sanitize_identifiers(std::slice::from_ref(&t.target), Lang::Python)
            .pop()
            .unwrap_or_else(|| "target".into());
        format!("predict_{}", base.trim_start_matches("v_"))
    };
    let out = match format {
        ExportFormat::Python => {
            let body = expr.to_code(&idents, Lang::Python);
            format!(
                "{}\n{PYTHON_HELPERS}\n\ndef {fn_name}({}):\n    \"\"\"Predicted {}.\"\"\"\n    return {body}\n",
                h.commented("#"),
                idents.join(", "),
                one_line(&t.target).replace('"', "'"),
            )
        }
        ExportFormat::Js => {
            let body = expr.to_code(&idents, Lang::Js);
            format!(
                "{}\n{JS_HELPERS}\n/** Predicted {}. */\nexport function {fn_name}({}) {{\n  return {body};\n}}\n",
                h.commented("//"),
                one_line(&t.target).replace("*/", "* /"),
                idents.join(", "),
            )
        }
        ExportFormat::Sql => {
            let body = expr.to_code(&idents, Lang::Sql);
            let alias = format!("predicted_{}", t.target).replace('"', "\"\"");
            format!(
                "{}-- Replace \"data\" with your table. NULL marks an undefined value.\nSELECT {body} AS \"{alias}\"\nFROM \"data\";\n",
                h.commented("--"),
            )
        }
        ExportFormat::Excel => {
            let body = expr.to_code(&idents, Lang::Excel);
            let mut s = h.commented("'");
            s.push_str("' Define a name for each input cell (Formulas > Define Name) and enter:\n");
            for (col, name) in &mapping {
                let _ = writeln!(s, "'   {name}  (column {})", one_line(col));
            }
            let _ = writeln!(s, "={body}");
            s
        }
        ExportFormat::Latex => {
            let target_tex = format!(
                "\\mathrm{{{}}}",
                t.target
                    .replace('\\', "")
                    .replace('_', "\\_")
                    .replace(' ', "\\ ")
            );
            format!(
                "{}\\[\n  {target_tex} = {}\n\\]\n",
                h.commented("%"),
                expr.to_latex(&eq.variables, 6)
            )
        }
        ExportFormat::Json => {
            let d = &t.validate.domain;
            let ranges: serde_json::Map<String, serde_json::Value> = eq
                .variables
                .iter()
                .filter_map(|v| {
                    let j = d.names.iter().position(|n| n == v)?;
                    Some((v.clone(), serde_json::json!([d.min[j], d.max[j]])))
                })
                .collect();
            let selected = eq.id == t.discover.selected_id;
            let v = serde_json::json!({
                "tarafold_version": report.tarafold_version,
                "run_id": report.run.id,
                "target": t.target,
                "id": eq.id,
                "selected": selected,
                "grade": if selected { serde_json::to_value(t.validate.reliability.grade).ok() } else { None },
                "evidence": if selected { Some(evidence_name(t.validate.evidence)) } else { None },
                "infix": eq.infix_exact,
                "variables": eq.variables,
                "training_range": ranges,
                "tree": eq.tree,
                "notes": h.lines,
            });
            let mut s = serde_json::to_string_pretty(&v).map_err(|e| ReportError(e.to_string()))?;
            s.push('\n');
            s
        }
    };
    Ok(out)
}
