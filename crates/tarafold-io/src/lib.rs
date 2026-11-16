//! CSV ingest (R3) and injection-safe CSV export (R14.3).
//!
//! Input is treated as untrusted: sizes are bounded before allocation, cells are only ever
//! parsed as numbers, and nothing in a cell is evaluated.

use std::io::Read;

use sha2::{Digest, Sha256};
use tarafold_core::{Column, Dataset, ExcludedColumn, SourceInfo};

/// Ingest failures. Each maps to CLI exit code 3 (invalid data) unless noted.
#[derive(Debug, thiserror::Error)]
pub enum IoError {
    /// Underlying read failure.
    #[error("could not read input: {0}")]
    Read(#[from] std::io::Error),
    /// A configured limit was exceeded.
    #[error("input exceeds the {what} limit of {limit}")]
    Limit {
        /// Which limit.
        what: &'static str,
        /// The limit value.
        limit: usize,
    },
    /// Malformed CSV.
    #[error("CSV error near line {line}: {message}")]
    Csv {
        /// 1-based line number (0 if unknown).
        line: u64,
        /// Description.
        message: String,
    },
    /// No header or no data rows.
    #[error("the CSV has no header row or no data rows")]
    Empty,
    /// A required column is missing.
    #[error("column `{0}` not found in the CSV header")]
    MissingColumn(String),
    /// A required column is not numeric or was excluded.
    #[error("column `{name}` cannot be used: {reason}")]
    UnusableColumn {
        /// Column name.
        name: String,
        /// Reason.
        reason: String,
    },
    /// Too few usable rows after dropping missing values (R3.7).
    #[error("only {0} usable rows remain after dropping rows with missing values; at least 30 are needed")]
    TooFewRows(usize),
    /// Invalid weight column (R3.10).
    #[error("invalid weights column `{name}`: {reason}")]
    InvalidWeights {
        /// Column name.
        name: String,
        /// Reason.
        reason: String,
    },
}

/// Options for [`read_csv`].
#[derive(Debug, Clone)]
pub struct IngestOptions {
    /// Field delimiter; auto-detected when `None`.
    pub delimiter: Option<u8>,
    /// Decimal separator (`.` or `,`); auto-detected when `None`.
    pub decimal: Option<char>,
    /// Maximum input size in bytes.
    pub max_bytes: usize,
    /// Maximum data rows.
    pub max_rows: usize,
    /// Maximum columns.
    pub max_cols: usize,
    /// Minimum usable rows.
    pub min_rows: usize,
    /// Columns that must be present and numeric (targets, controllables).
    pub required: Vec<String>,
    /// Optional row-weight column (R3.10).
    pub weights: Option<String>,
}

impl Default for IngestOptions {
    fn default() -> Self {
        Self {
            delimiter: None,
            decimal: None,
            max_bytes: 2 * 1024 * 1024 * 1024,
            max_rows: 10_000_000,
            max_cols: 500,
            min_rows: 30,
            required: Vec::new(),
            weights: None,
        }
    }
}

const MISSING_TOKENS: [&str; 10] = ["", "na", "n/a", "nan", "null", "none", "nil", "?", "-", "#n/a"];

fn is_missing_token(s: &str) -> bool {
    let t = s.trim();
    MISSING_TOKENS.iter().any(|m| t.eq_ignore_ascii_case(m))
}

fn parse_number(s: &str, decimal: char) -> Option<f64> {
    let t = s.trim();
    let v = if decimal == ',' { t.replace(',', ".").parse::<f64>() } else { t.parse::<f64>() };
    v.ok().filter(|v| v.is_finite())
}

/// Guess the delimiter from the first lines: the candidate with a consistent field count > 1,
/// preferring more fields. Quotes are ignored, which is fine for a guess.
fn sniff_delimiter(head: &str) -> u8 {
    let lines: Vec<&str> = head.lines().filter(|l| !l.trim().is_empty()).take(30).collect();
    let mut best = (b',', 0usize, false);
    for &d in &[b',', b';', b'\t', b'|'] {
        let counts: Vec<usize> = lines.iter().map(|l| l.matches(d as char).count() + 1).collect();
        let Some(&first) = counts.first() else { continue };
        let consistent = counts.iter().all(|&c| c == first);
        if first > 1 && (consistent && !best.2 || (consistent == best.2 && first > best.1)) {
            best = (d, first, consistent);
        }
    }
    best.0
}

/// Guess the decimal separator: `,` only when the delimiter is not `,` and comma decimals
/// appear in the data.
fn sniff_decimal(head: &str, delimiter: u8) -> char {
    if delimiter == b',' {
        return '.';
    }
    let mut comma = 0usize;
    let mut dot = 0usize;
    for line in head.lines().skip(1).take(50) {
        for f in line.split(delimiter as char) {
            let f = f.trim().trim_matches('"');
            let looks_num = |sep: char| {
                let mut parts = f.trim_start_matches('-').split(sep);
                matches!((parts.next(), parts.next(), parts.next()),
                    (Some(a), Some(b), None) if !a.is_empty() && !b.is_empty()
                        && a.chars().all(|c| c.is_ascii_digit())
                        && b.chars().all(|c| c.is_ascii_digit()))
            };
            if looks_num(',') {
                comma += 1;
            } else if looks_num('.') {
                dot += 1;
            }
        }
    }
    if comma > dot { ',' } else { '.' }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Read a CSV into a numeric [`Dataset`].
///
/// Columns are numeric when ≥ 95% of non-missing cells parse as finite numbers. Rows with a
/// missing value in any used column are dropped and counted. Constant, duplicate, mostly
/// missing, and non-numeric columns are excluded with a reason.
pub fn read_csv(mut input: impl Read, label: &str, opts: &IngestOptions) -> Result<Dataset, IoError> {
    let mut bytes = Vec::new();
    let limit = u64::try_from(opts.max_bytes).unwrap_or(u64::MAX).saturating_add(1);
    input.by_ref().take(limit).read_to_end(&mut bytes)?;
    if bytes.len() > opts.max_bytes {
        return Err(IoError::Limit { what: "file size (bytes)", limit: opts.max_bytes });
    }
    let sha256 = hex(&Sha256::digest(&bytes));
    let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes); // UTF-8 BOM
    let head = String::from_utf8_lossy(&body[..body.len().min(64 * 1024)]);
    let delimiter = opts.delimiter.unwrap_or_else(|| sniff_delimiter(&head));
    let decimal = opts.decimal.unwrap_or_else(|| sniff_decimal(&head, delimiter));

    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(true)
        .flexible(false)
        .from_reader(body);
    let csv_err = |e: csv::Error| IoError::Csv {
        line: e.position().map_or(0, csv::Position::line),
        message: e.to_string(),
    };
    let headers: Vec<String> =
        rdr.headers().map_err(csv_err)?.iter().map(|h| h.trim().to_string()).collect();
    if headers.is_empty() || headers.iter().all(String::is_empty) {
        return Err(IoError::Empty);
    }
    if headers.len() > opts.max_cols {
        return Err(IoError::Limit { what: "column count", limit: opts.max_cols });
    }
    let p = headers.len();
    let mut values: Vec<Vec<f64>> = vec![Vec::new(); p];
    let mut non_missing = vec![0usize; p];
    let mut parsed = vec![0usize; p];
    let mut n_rows = 0usize;
    for rec in rdr.records() {
        let rec = rec.map_err(csv_err)?;
        n_rows += 1;
        if n_rows > opts.max_rows {
            return Err(IoError::Limit { what: "row count", limit: opts.max_rows });
        }
        for (j, cell) in rec.iter().enumerate() {
            if is_missing_token(cell) {
                values[j].push(f64::NAN);
                continue;
            }
            non_missing[j] += 1;
            match parse_number(cell, decimal) {
                Some(v) => {
                    parsed[j] += 1;
                    values[j].push(v);
                }
                None => values[j].push(f64::NAN),
            }
        }
    }
    if n_rows == 0 {
        return Err(IoError::Empty);
    }

    // Decide which columns are usable.
    let mut excluded: Vec<ExcludedColumn> = Vec::new();
    let mut keep: Vec<usize> = Vec::new();
    for j in 0..p {
        let name = &headers[j];
        let reason = if name.is_empty() {
            Some("empty_header".to_string())
        } else if headers[..j].contains(name) {
            Some("duplicate_header".to_string())
        } else if non_missing[j] == 0 || (parsed[j] as f64) < 0.95 * non_missing[j] as f64 {
            Some("non_numeric".to_string())
        } else if parsed[j] * 2 < n_rows {
            Some("too_many_missing".to_string())
        } else {
            None
        };
        match reason {
            Some(r) => excluded.push(ExcludedColumn { name: name.clone(), reason: r }),
            None => keep.push(j),
        }
    }
    for req in opts.required.iter().chain(opts.weights.iter()) {
        if !headers.contains(req) {
            return Err(IoError::MissingColumn(req.clone()));
        }
        if let Some(e) = excluded.iter().find(|e| &e.name == req) {
            return Err(IoError::UnusableColumn { name: req.clone(), reason: e.reason.clone() });
        }
    }

    // Drop rows with missing values in kept columns.
    let complete: Vec<usize> =
        (0..n_rows).filter(|&i| keep.iter().all(|&j| values[j][i].is_finite())).collect();
    if complete.len() < opts.min_rows {
        return Err(IoError::TooFewRows(complete.len()));
    }
    let mut columns: Vec<Column> = Vec::new();
    for &j in &keep {
        let v: Vec<f64> = complete.iter().map(|&i| values[j][i]).collect();
        let name = headers[j].clone();
        let is_required = opts.required.contains(&name) || opts.weights.as_ref() == Some(&name);
        let first = v[0];
        if !is_required && v.iter().all(|&x| x == first) {
            excluded.push(ExcludedColumn { name, reason: "constant".into() });
            continue;
        }
        if let Some(dup) = columns.iter().find(|c| c.values == v) {
            if !is_required {
                excluded.push(ExcludedColumn { name, reason: format!("duplicate_of:{}", dup.name) });
                continue;
            }
        }
        columns.push(Column { name, values: v });
    }

    let weights = match &opts.weights {
        None => None,
        Some(wname) => {
            let pos = columns.iter().position(|c| &c.name == wname).ok_or_else(|| {
                IoError::InvalidWeights { name: wname.clone(), reason: "column not usable".into() }
            })?;
            let w = columns.remove(pos).values;
            if w.iter().any(|&x| x < 0.0) {
                return Err(IoError::InvalidWeights {
                    name: wname.clone(),
                    reason: "weights must be non-negative".into(),
                });
            }
            let mean = w.iter().sum::<f64>() / w.len() as f64;
            if mean <= 0.0 {
                return Err(IoError::InvalidWeights {
                    name: wname.clone(),
                    reason: "all weights are zero".into(),
                });
            }
            Some(w.iter().map(|x| x / mean).collect())
        }
    };

    Ok(Dataset {
        n_rows: complete.len(),
        columns,
        weights,
        source: SourceInfo {
            path: label.to_string(),
            sha256,
            delimiter: match delimiter {
                b'\t' => "\\t".into(),
                d => (d as char).to_string(),
            },
            decimal: decimal.to_string(),
            rows_read: n_rows,
            rows_dropped_missing: n_rows - complete.len(),
            excluded,
        },
    })
}

/// A cell for CSV export.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    /// Number, written in shortest round-trip form.
    Num(f64),
    /// Text, guarded against spreadsheet formula injection.
    Text(String),
    /// Empty cell.
    Empty,
}

/// Prefix text that a spreadsheet would treat as a formula (R14.3).
pub fn guard_text(s: &str) -> String {
    match s.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') => format!("'{s}"),
        _ => s.to_string(),
    }
}

/// Write rows as CSV (comma-delimited, RFC 4180 quoting).
pub fn write_csv(headers: &[String], rows: &[Vec<Cell>]) -> Result<String, IoError> {
    let mut w = csv::WriterBuilder::new().from_writer(Vec::new());
    let map_err = |e: csv::Error| IoError::Csv { line: 0, message: e.to_string() };
    let hdr: Vec<String> = headers.iter().map(|h| guard_text(h)).collect();
    w.write_record(&hdr).map_err(map_err)?;
    for row in rows {
        let rec: Vec<String> = row
            .iter()
            .map(|c| match c {
                Cell::Num(v) if v.is_finite() => format!("{v:?}"),
                Cell::Num(_) | Cell::Empty => String::new(),
                Cell::Text(t) => guard_text(t),
            })
            .collect();
        w.write_record(&rec).map_err(map_err)?;
    }
    let bytes = w.into_inner().map_err(|e| IoError::Csv { line: 0, message: e.to_string() })?;
    String::from_utf8(bytes).map_err(|e| IoError::Csv { line: 0, message: e.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: usize, f: impl Fn(usize) -> String) -> String {
        (0..n).map(f).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn comma_csv_with_mixed_columns() {
        let csv = format!(
            "x,y,region,const,dup,sparse\n{}",
            rows(40, |i| format!("{i},{},north,1,{i},{}", 2 * i, if i % 3 == 0 { "5" } else { "" }))
        );
        let d = read_csv(csv.as_bytes(), "t.csv", &IngestOptions::default()).unwrap();
        assert_eq!(d.names(), vec!["x", "y"]);
        let reasons: Vec<(&str, &str)> =
            d.source.excluded.iter().map(|e| (e.name.as_str(), e.reason.as_str())).collect();
        assert!(reasons.contains(&("region", "non_numeric")));
        assert!(reasons.contains(&("const", "constant")));
        assert!(reasons.contains(&("dup", "duplicate_of:x")));
        assert!(reasons.contains(&("sparse", "too_many_missing")));
        assert_eq!(d.n_rows, 40);
        assert_eq!(d.source.sha256.len(), 64);
    }

    #[test]
    fn semicolon_with_decimal_comma_and_missing() {
        let csv = format!("a;b\n{}\nNA;3,5", rows(35, |i| format!("{i},5;{}", i * 2)));
        let d = read_csv(csv.as_bytes(), "t.csv", &IngestOptions::default()).unwrap();
        assert_eq!(d.source.delimiter, ";");
        assert_eq!(d.source.decimal, ",");
        assert_eq!(d.column("a").unwrap().values[1], 1.5);
        assert_eq!(d.n_rows, 35);
        assert_eq!(d.source.rows_dropped_missing, 1);
    }

    #[test]
    fn too_few_rows_and_limits() {
        let csv = format!("a,b\n{}", rows(10, |i| format!("{i},{i}1")));
        assert!(matches!(
            read_csv(csv.as_bytes(), "t", &IngestOptions::default()),
            Err(IoError::TooFewRows(10))
        ));
        let opts = IngestOptions { max_rows: 5, ..Default::default() };
        assert!(matches!(read_csv(csv.as_bytes(), "t", &opts), Err(IoError::Limit { .. })));
        let opts = IngestOptions { max_bytes: 10, ..Default::default() };
        assert!(matches!(read_csv(csv.as_bytes(), "t", &opts), Err(IoError::Limit { .. })));
    }

    #[test]
    fn required_columns_and_weights() {
        let csv = format!("x,y,w,label\n{}", rows(40, |i| format!("{i},{},{},a", i * 3, i % 4)));
        let opts = IngestOptions { required: vec!["label".into()], ..Default::default() };
        assert!(matches!(
            read_csv(csv.as_bytes(), "t", &opts),
            Err(IoError::UnusableColumn { .. })
        ));
        let opts = IngestOptions { weights: Some("w".into()), ..Default::default() };
        let d = read_csv(csv.as_bytes(), "t", &opts).unwrap();
        assert_eq!(d.names(), vec!["x", "y"]);
        let w = d.weights.unwrap();
        assert!((w.iter().sum::<f64>() / w.len() as f64 - 1.0).abs() < 1e-12);
        let bad = format!("x,y,w\n{}", rows(40, |i| format!("{i},{i}2,-1")));
        let opts = IngestOptions { weights: Some("w".into()), ..Default::default() };
        assert!(matches!(read_csv(bad.as_bytes(), "t", &opts), Err(IoError::InvalidWeights { .. })));
    }

    #[test]
    fn export_guards_formulas() {
        let s = write_csv(
            &["equation".into(), "r2".into()],
            &[vec![Cell::Text("=HYPERLINK(\"x\")".into()), Cell::Num(0.5)], vec![
                Cell::Text("-2 * x".into()),
                Cell::Empty,
            ]],
        )
        .unwrap();
        assert!(s.contains("'=HYPERLINK"));
        assert!(s.contains("'-2 * x"));
        assert!(s.contains("0.5"));
    }
}
