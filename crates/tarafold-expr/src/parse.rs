//! Pratt parser for infix expressions. Input is data, never evaluated as code.

use crate::{BinOp, Expr, Node, UnOp};

/// Parse failure with byte position.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{message} at position {pos}")]
pub struct ParseError {
    /// What went wrong.
    pub message: String,
    /// Byte offset in the input.
    pub pos: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
    LParen,
    RParen,
}

const MAX_INPUT: usize = 64 * 1024;
const MAX_DEPTH: usize = 256;

fn err<T>(message: impl Into<String>, pos: usize) -> Result<T, ParseError> {
    Err(ParseError {
        message: message.into(),
        pos,
    })
}

fn tokenize(s: &str) -> Result<Vec<(Tok, usize)>, ParseError> {
    if s.len() > MAX_INPUT {
        return err("expression too long", MAX_INPUT);
    }
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let start = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                let mut j = i + 1;
                if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                    j += 1;
                }
                if j < b.len() && b[j].is_ascii_digit() {
                    i = j;
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text = &s[start..i];
            match text.parse::<f64>() {
                Ok(v) if v.is_finite() => out.push((Tok::Num(v), start)),
                _ => return err(format!("invalid number `{text}`"), start),
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push((Tok::Ident(s[start..i].to_string()), start));
        } else if c == '`' {
            let start = i;
            let Some(end) = s[i + 1..].find('`') else {
                return err("unterminated `name`", start);
            };
            out.push((Tok::Ident(s[i + 1..i + 1 + end].to_string()), start));
            i += end + 2;
        } else if "+-*/^".contains(c) {
            out.push((Tok::Op(c), i));
            i += 1;
        } else if c == '(' {
            out.push((Tok::LParen, i));
            i += 1;
        } else if c == ')' {
            out.push((Tok::RParen, i));
            i += 1;
        } else {
            return err(format!("unexpected character `{c}`"), i);
        }
    }
    Ok(out)
}

struct Parser<'a> {
    toks: Vec<(Tok, usize)>,
    pos: usize,
    names: &'a [String],
    end: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|(t, _)| t)
    }

    fn here(&self) -> usize {
        self.toks.get(self.pos).map_or(self.end, |(_, p)| *p)
    }

    fn infix_bp(op: char) -> Option<(u8, u8, BinOp)> {
        // (left binding power, right binding power). `^` is right-associative.
        Some(match op {
            '+' => (10, 11, BinOp::Add),
            '-' => (10, 11, BinOp::Sub),
            '*' => (20, 21, BinOp::Mul),
            '/' => (20, 21, BinOp::Div),
            '^' => (41, 40, BinOp::Pow),
            _ => return None,
        })
    }

    fn expr(&mut self, min_bp: u8, depth: usize) -> Result<Expr, ParseError> {
        if depth > MAX_DEPTH {
            return err("expression nested too deeply", self.here());
        }
        let start = self.here();
        let tok = self.toks.get(self.pos).cloned();
        self.pos += 1;
        let mut lhs = match tok {
            Some((Tok::Num(v), _)) => Expr::constant(v),
            Some((Tok::Op('-'), _)) => {
                // Prefix minus binds tighter than * but looser than ^ (so -x^2 = -(x^2)).
                let rhs = self.expr(30, depth + 1)?;
                if let [Node::Const(c)] = rhs.nodes() {
                    Expr::constant(-c)
                } else {
                    rhs.unary(UnOp::Neg)
                }
            }
            Some((Tok::LParen, _)) => {
                let inner = self.expr(0, depth + 1)?;
                if self.peek() != Some(&Tok::RParen) {
                    return err("expected `)`", self.here());
                }
                self.pos += 1;
                inner
            }
            Some((Tok::Ident(name), p)) => {
                if self.peek() == Some(&Tok::LParen) {
                    let Some(op) = UnOp::from_name(&name) else {
                        return err(format!("unknown function `{name}`"), p);
                    };
                    self.pos += 1;
                    let arg = self.expr(0, depth + 1)?;
                    if self.peek() != Some(&Tok::RParen) {
                        return err("expected `)`", self.here());
                    }
                    self.pos += 1;
                    arg.unary(op)
                } else {
                    let Some(i) = self.names.iter().position(|n| *n == name) else {
                        return err(format!("unknown variable `{name}`"), p);
                    };
                    let i = u16::try_from(i).map_err(|_| ParseError {
                        message: "too many variables".into(),
                        pos: p,
                    })?;
                    Expr::var(i)
                }
            }
            _ => return err("expected a number, variable, function, or `(`", start),
        };
        while let Some(Tok::Op(c)) = self.peek().cloned() {
            let Some((lbp, rbp, op)) = Self::infix_bp(c) else {
                break;
            };
            if lbp < min_bp {
                break;
            }
            self.pos += 1;
            let rhs = self.expr(rbp, depth + 1)?;
            lhs = Expr::binary(op, lhs, rhs);
        }
        Ok(lhs)
    }
}

/// Parse an infix expression. Variables resolve to their index in `names`.
///
/// Grammar: numbers, names (bare or in backticks), `+ - * / ^`, unary minus, parentheses,
/// and the functions `exp log ln sqrt square abs sin cos tanh inv`.
pub fn parse(input: &str, names: &[String]) -> Result<Expr, ParseError> {
    let toks = tokenize(input)?;
    let mut p = Parser {
        toks,
        pos: 0,
        names,
        end: input.len(),
    };
    let e = p.expr(0, 0)?;
    if p.pos != p.toks.len() {
        return err("unexpected input", p.here());
    }
    Ok(e)
}
