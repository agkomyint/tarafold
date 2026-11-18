//! Code generation: an expression as Python, JavaScript, SQL, or Excel source (R16).
//!
//! Output is fully parenthesized, so the target language's precedence rules (for example
//! Excel's `-2^2 = 4`) cannot change the meaning. Protected operations keep their semantics:
//! Python and JavaScript call small guard helpers that return NaN on domain errors, SQL wraps
//! them in `CASE` so the result is `NULL`, and Excel uses functions that return `#NUM!`.

use std::collections::BTreeSet;

use crate::{BinOp, Expr, Node, UnOp};

/// Target language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// Python 3 using only `math`.
    Python,
    /// JavaScript using only `Math`.
    Js,
    /// ANSI SQL expression (`EXP`, `LN`, `POWER`, `SQRT`, `TANH`).
    Sql,
    /// Excel formula over named cells.
    Excel,
}

/// Guard helpers that generated Python code calls. They match the engine's protected
/// semantics, including returning `inf` where Python would raise on overflow.
pub const PYTHON_HELPERS: &str = r#"import math

_EPS = 1e-12


def _div(a, b):
    return a / b if abs(b) >= _EPS else math.nan


def _inv(a):
    return 1.0 / a if abs(a) >= _EPS else math.nan


def _log(a):
    return math.log(a) if a > 0 else math.nan


def _sqrt(a):
    return math.sqrt(a) if a >= 0 else math.nan


def _exp(a):
    try:
        return math.exp(a)
    except OverflowError:
        return math.inf


def _pow(a, b):
    if a < 0 and not (math.isfinite(b) and b == math.floor(b)):
        return math.nan
    try:
        return math.pow(a, b)
    except OverflowError:
        odd = math.isfinite(b) and b == math.floor(b) and b % 2 == 1
        return -math.inf if (a < 0 and odd) else math.inf
    except ValueError:
        return math.inf  # 0 raised to a negative power


def _sin(a):
    return math.sin(a) if math.isfinite(a) else math.nan


def _cos(a):
    return math.cos(a) if math.isfinite(a) else math.nan
"#;

/// Guard helpers that generated JavaScript code calls.
pub const JS_HELPERS: &str = r#"const _EPS = 1e-12;
const _div = (a, b) => (Math.abs(b) >= _EPS ? a / b : NaN);
const _inv = (a) => (Math.abs(a) >= _EPS ? 1 / a : NaN);
const _log = (a) => (a > 0 ? Math.log(a) : NaN);
const _sqrt = (a) => (a >= 0 ? Math.sqrt(a) : NaN);
const _pow = (a, b) => {
  if (a === 1) return 1;
  if (a < 0 && !Number.isInteger(b)) return NaN;
  return Math.pow(a, b);
};
"#;

/// Reserved words per language that a sanitized identifier must not equal (case-sensitive for
/// Python and JavaScript, case-insensitive for SQL and Excel).
fn reserved(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Python => &[
            "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class",
            "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global",
            "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise",
            "return", "try", "while", "with", "yield", "math",
        ],
        Lang::Js => &[
            "arguments",
            "await",
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "debugger",
            "default",
            "delete",
            "do",
            "else",
            "enum",
            "eval",
            "export",
            "extends",
            "false",
            "finally",
            "for",
            "function",
            "if",
            "implements",
            "import",
            "in",
            "instanceof",
            "interface",
            "let",
            "new",
            "null",
            "package",
            "private",
            "protected",
            "public",
            "return",
            "static",
            "super",
            "switch",
            "this",
            "throw",
            "true",
            "try",
            "typeof",
            "undefined",
            "var",
            "void",
            "while",
            "with",
            "yield",
            "NaN",
            "Infinity",
            "Math",
        ],
        Lang::Sql => &[],
        Lang::Excel => &["TRUE", "FALSE"],
    }
}

/// True when an Excel name could be read as a cell reference (`A1`, `XFD10`, `R1C1`, `R`, `C`).
fn looks_like_excel_ref(s: &str) -> bool {
    let up = s.to_ascii_uppercase();
    if up == "R" || up == "C" {
        return true;
    }
    let letters = up.chars().take_while(char::is_ascii_alphabetic).count();
    let rest = &up[letters..];
    if (1..=3).contains(&letters) && !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    // R1C1 style.
    up.starts_with('R')
        && up[1..].chars().all(|c| c.is_ascii_digit() || c == 'C')
        && up.contains('C')
}

/// Sanitize column names into identifiers valid in `lang`, deterministically and without
/// collisions. SQL keeps the original names (they are emitted as quoted identifiers).
pub fn sanitize_identifiers(names: &[String], lang: Lang) -> Vec<String> {
    if lang == Lang::Sql {
        return names.to_vec();
    }
    let reserved = reserved(lang);
    let is_reserved = |s: &str| match lang {
        Lang::Excel => reserved.iter().any(|r| r.eq_ignore_ascii_case(s)),
        _ => reserved.contains(&s),
    };
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let mut s: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
            s.insert_str(0, "v_");
        }
        // Names starting with `_` are reserved for the guard helpers.
        if s.starts_with('_') {
            s.insert(0, 'v');
        }
        if is_reserved(&s) || (lang == Lang::Excel && looks_like_excel_ref(&s)) {
            s.insert_str(0, "v_");
        }
        let key = |s: &str| {
            if lang == Lang::Excel {
                s.to_ascii_uppercase()
            } else {
                s.to_string()
            }
        };
        let base = s.clone();
        let mut k = 2;
        while used.contains(&key(&s)) {
            s = format!("{base}_{k}");
            k += 1;
        }
        used.insert(key(&s));
        out.push(s);
    }
    out
}

/// Shortest round-trip literal, valid in Python, JavaScript, SQL, and Excel.
fn literal(c: f64, lang: Lang) -> String {
    let s = format!("{c:?}");
    let s = match lang {
        // Excel and SQL read `1e-7` but some engines need the `E` form; both accept upper case.
        Lang::Excel | Lang::Sql => s.to_ascii_uppercase(),
        _ => s,
    };
    if c.is_sign_negative() {
        format!("({s})")
    } else {
        s
    }
}

fn sql_ident(name: &str) -> String {
    format!(
        "CAST(\"{}\" AS DOUBLE PRECISION)",
        name.replace('"', "\"\"")
    )
}

impl Expr {
    /// The expression as source in `lang`. `names[i]` is the identifier for `Var(i)`; pass
    /// names from [`sanitize_identifiers`] (SQL quotes and casts the original names).
    ///
    /// Python and JavaScript output calls the guard helpers in [`PYTHON_HELPERS`] and
    /// [`JS_HELPERS`]. Excel output has no leading `=`.
    pub fn to_code(&self, names: &[String], lang: Lang) -> String {
        let mut stack: Vec<String> = Vec::new();
        for node in &self.nodes {
            let s = match *node {
                Node::Const(c) => literal(c, lang),
                Node::Var(i) => {
                    let n = names
                        .get(i as usize)
                        .cloned()
                        .unwrap_or_else(|| format!("x{i}"));
                    if lang == Lang::Sql { sql_ident(&n) } else { n }
                }
                Node::Unary(op) => {
                    let a = stack.pop().unwrap_or_default();
                    unary(op, &a, lang)
                }
                Node::Binary(op) => {
                    let b = stack.pop().unwrap_or_default();
                    let a = stack.pop().unwrap_or_default();
                    binary(op, &a, &b, lang)
                }
            };
            stack.push(s);
        }
        stack.pop().unwrap_or_default()
    }
}

fn unary(op: UnOp, a: &str, lang: Lang) -> String {
    match lang {
        Lang::Python => match op {
            UnOp::Neg => format!("(-{a})"),
            UnOp::Exp => format!("_exp({a})"),
            UnOp::Log => format!("_log({a})"),
            UnOp::Sqrt => format!("_sqrt({a})"),
            UnOp::Square => format!("({a} * {a})"),
            UnOp::Abs => format!("abs({a})"),
            UnOp::Sin => format!("_sin({a})"),
            UnOp::Cos => format!("_cos({a})"),
            UnOp::Tanh => format!("math.tanh({a})"),
            UnOp::Inv => format!("_inv({a})"),
        },
        Lang::Js => match op {
            UnOp::Neg => format!("(-{a})"),
            UnOp::Exp => format!("Math.exp({a})"),
            UnOp::Log => format!("_log({a})"),
            UnOp::Sqrt => format!("_sqrt({a})"),
            UnOp::Square => format!("({a} * {a})"),
            UnOp::Abs => format!("Math.abs({a})"),
            UnOp::Sin => format!("Math.sin({a})"),
            UnOp::Cos => format!("Math.cos({a})"),
            UnOp::Tanh => format!("Math.tanh({a})"),
            UnOp::Inv => format!("_inv({a})"),
        },
        Lang::Sql => match op {
            UnOp::Neg => format!("(-{a})"),
            UnOp::Exp => format!("EXP({a})"),
            UnOp::Log => format!("(CASE WHEN {a} > 0 THEN LN({a}) END)"),
            UnOp::Sqrt => format!("(CASE WHEN {a} >= 0 THEN SQRT({a}) END)"),
            UnOp::Square => format!("({a} * {a})"),
            UnOp::Abs => format!("ABS({a})"),
            UnOp::Sin => format!("SIN({a})"),
            UnOp::Cos => format!("COS({a})"),
            UnOp::Tanh => format!("TANH({a})"),
            UnOp::Inv => format!("(CASE WHEN ABS({a}) >= 1E-12 THEN 1.0 / {a} END)"),
        },
        Lang::Excel => match op {
            UnOp::Neg => format!("(-{a})"),
            UnOp::Exp => format!("EXP({a})"),
            UnOp::Log => format!("LN({a})"),
            UnOp::Sqrt => format!("SQRT({a})"),
            UnOp::Square => format!("({a}*{a})"),
            UnOp::Abs => format!("ABS({a})"),
            UnOp::Sin => format!("SIN({a})"),
            UnOp::Cos => format!("COS({a})"),
            UnOp::Tanh => format!("TANH({a})"),
            // SQRT(-1) yields #NUM!, the same error the other protected operations give.
            UnOp::Inv => format!("IF(ABS({a})>=1E-12,1/{a},SQRT(-1))"),
        },
    }
}

fn binary(op: BinOp, a: &str, b: &str, lang: Lang) -> String {
    match (op, lang) {
        (BinOp::Add, Lang::Excel) => format!("({a}+{b})"),
        (BinOp::Sub, Lang::Excel) => format!("({a}-{b})"),
        (BinOp::Mul, Lang::Excel) => format!("({a}*{b})"),
        (BinOp::Add, _) => format!("({a} + {b})"),
        (BinOp::Sub, _) => format!("({a} - {b})"),
        (BinOp::Mul, _) => format!("({a} * {b})"),
        (BinOp::Div, Lang::Python | Lang::Js) => format!("_div({a}, {b})"),
        (BinOp::Pow, Lang::Python | Lang::Js) => format!("_pow({a}, {b})"),
        (BinOp::Div, Lang::Sql) => format!("(CASE WHEN ABS({b}) >= 1E-12 THEN {a} / {b} END)"),
        (BinOp::Pow, Lang::Sql) => format!(
            "(CASE WHEN ({a} >= 0 OR {b} = FLOOR({b})) AND ({a} <> 0 OR {b} >= 0) \
             THEN POWER({a}, {b}) END)"
        ),
        (BinOp::Div, Lang::Excel) => format!("IF(ABS({b})>=1E-12,{a}/{b},SQRT(-1))"),
        (BinOp::Pow, Lang::Excel) => format!("POWER({a},{b})"),
    }
}
