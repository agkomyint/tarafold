//! `report.html`: the single-file web UI with the report and a data sample injected (R10.2).

use serde::{Deserialize, Serialize};
use tarafold_core::Dataset;

use crate::{Report, ReportError};

/// The embedded UI page (or a placeholder when the binary was built without `web/`).
pub const UI_HTML: &str = include_str!(concat!(env!("OUT_DIR"), "/ui.html"));

/// Whether the real web UI is embedded.
pub const UI_BUILT: bool = matches!(env!("TARAFOLD_UI_BUILT").as_bytes(), b"1");

const MARKER: &str = "<!--TARAFOLD_DATA-->";

/// Rows of the input data shown in charts, column-major. Non-finite values are `null`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataSample {
    /// Column names.
    pub columns: Vec<String>,
    /// `values[j][i]`: column `j`, sampled row `i`.
    pub values: Vec<Vec<f64>>,
    /// Rows in the full dataset.
    pub rows_total: usize,
    /// Whether rows were left out.
    pub sampled: bool,
}

impl DataSample {
    /// Up to `max_rows` evenly spaced rows (deterministic) of every numeric column.
    pub fn from_dataset(data: &Dataset, max_rows: usize) -> Self {
        let n = data.n_rows;
        let k = n.min(max_rows.max(1));
        let idx: Vec<usize> = if k >= n {
            (0..n).collect()
        } else {
            (0..k).map(|i| i * n / k).collect()
        };
        Self {
            columns: data.names(),
            values: data
                .columns
                .iter()
                .map(|c| idx.iter().map(|&i| c.values[i]).collect())
                .collect(),
            rows_total: n,
            sampled: k < n,
        }
    }
}

/// JSON that is safe inside `<script type="application/json">`: `<` is written as `\u003c`, so
/// the text cannot close the script element or open a comment.
fn script_json<T: Serialize>(v: &T) -> Result<String, ReportError> {
    let s = serde_json::to_string(v).map_err(|e| ReportError(e.to_string()))?;
    Ok(s.replace('<', "\\u003c"))
}

/// Inject the report (and optional data sample) into a UI page.
pub fn inject<R: Serialize>(
    page: &str,
    report: &R,
    data: Option<&DataSample>,
) -> Result<String, ReportError> {
    let mut tags = format!(
        "<script type=\"application/json\" id=\"tarafold-report\">{}</script>",
        script_json(report)?
    );
    if let Some(d) = data {
        tags.push_str(&format!(
            "\n<script type=\"application/json\" id=\"tarafold-data\">{}</script>",
            script_json(d)?
        ));
    }
    Ok(match page.rfind(MARKER) {
        Some(i) => format!("{}{tags}{}", &page[..i], &page[i + MARKER.len()..]),
        None => match page.rfind("</body>") {
            Some(i) => format!("{}{tags}\n{}", &page[..i], &page[i..]),
            None => format!("{page}{tags}"),
        },
    })
}

/// `report.html` from the embedded UI.
pub fn report_html(report: &Report, data: Option<&DataSample>) -> Result<String, ReportError> {
    inject(UI_HTML, report, data)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn script_json_cannot_close_the_script() {
        let s = script_json(&serde_json::json!({"a": "</script><!-- x"})).unwrap();
        assert!(!s.contains("</"));
        assert!(!s.contains("<!--"));
        let back: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(back["a"], "</script><!-- x");
    }

    #[test]
    fn inject_replaces_the_marker_once() {
        let page = "<html><body><div></div><!--TARAFOLD_DATA--><script></script></body></html>";
        let d = DataSample {
            columns: vec!["x".into()],
            values: vec![vec![1.0, f64::NAN]],
            rows_total: 2,
            sampled: false,
        };
        let v: serde_json::Value = serde_json::to_value(&d).unwrap();
        assert!(v["values"][0][1].is_null());
        let html = inject(page, &serde_json::json!({"k": 1}), Some(&d)).unwrap();
        assert!(!html.contains(MARKER));
        assert_eq!(html.matches("id=\"tarafold-report\"").count(), 1);
        assert!(html.find("tarafold-data").unwrap() < html.find("<script></script>").unwrap());
        assert!(
            UI_HTML.contains(MARKER),
            "embedded page must carry the marker"
        );
    }
}
