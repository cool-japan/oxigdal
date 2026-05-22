//! Lexer + Pratt-style precedence-climbing parser for band-math strings.
//!
//! Output is a [`BandExpression`] AST that can be evaluated directly via
//! [`BandExpression::evaluate`](crate::algebra::BandExpression::evaluate) or
//! lowered to WGSL via [`crate::band_math::band_expression_to_wgsl`].

use crate::algebra::BandExpression;
use std::fmt;

/// Errors produced while lexing/parsing a band-math expression.
#[derive(Debug, Clone, PartialEq)]
pub enum BandMathError {
    /// Encountered a character or token that wasn't expected at this position.
    UnexpectedToken {
        /// Byte offset into the original source string.
        pos: usize,
        /// Display text of the offending fragment.
        found: String,
    },
    /// Identifier used like a function (`name(...)`) is not a known builtin.
    UnknownFunction(String),
    /// `Bn` band reference is outside the allowed inclusive range `1..=999`.
    ///
    /// The wrapped value is the raw integer that was parsed, or `0` for `B0`.
    InvalidBandIndex(u32),
    /// Function call had the wrong number of arguments.
    WrongArgCount {
        /// Function name as it appeared in the source.
        function: String,
        /// Number of arguments the function expects.
        expected: usize,
        /// Number of arguments actually supplied.
        actual: usize,
    },
    /// Detected literal `/ 0` or `/ 0.0` at parse time.
    DivByConstantZero,
    /// Parser stopped before consuming the entire token stream.
    TrailingTokens(usize),
    /// Input string was empty or contained only whitespace.
    EmptyExpression,
    /// `(` without a matching `)` (or vice-versa).
    UnmatchedParen,
    /// Number literal failed to parse as `f32`.
    InvalidNumber {
        /// Byte offset into the original source string.
        pos: usize,
        /// Raw text that failed to parse.
        text: String,
    },
}

impl fmt::Display for BandMathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BandMathError::UnexpectedToken { pos, found } => {
                write!(f, "unexpected token `{}` at position {}", found, pos)
            }
            BandMathError::UnknownFunction(name) => write!(f, "unknown function `{}`", name),
            BandMathError::InvalidBandIndex(n) => {
                write!(f, "invalid band index B{} (allowed range: 1..=999)", n)
            }
            BandMathError::WrongArgCount {
                function,
                expected,
                actual,
            } => write!(
                f,
                "function `{}` expects {} argument(s), got {}",
                function, expected, actual
            ),
            BandMathError::DivByConstantZero => write!(f, "division by literal zero"),
            BandMathError::TrailingTokens(pos) => {
                write!(f, "trailing tokens after expression (position {})", pos)
            }
            BandMathError::EmptyExpression => write!(f, "empty expression"),
            BandMathError::UnmatchedParen => write!(f, "unmatched parenthesis"),
            BandMathError::InvalidNumber { pos, text } => {
                write!(f, "invalid number literal `{}` at position {}", text, pos)
            }
        }
    }
}

impl std::error::Error for BandMathError {}

/// Token kinds produced by [`Lexer`].
#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// A floating-point literal (already validated to parse as `f64`).
    Number(f64),
    /// `Bn` — a 1-based band reference; stored as the 0-based index for the AST.
    Band(usize),
    /// One of `+ - * / ^`.
    Op(char),
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `,`
    Comma,
    /// Function-name identifier immediately followed by `(`.
    Func(String),
}

/// Hand-rolled lexer that walks the source bytes left-to-right.
struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src: src.as_bytes(),
            pos: 0,
        }
    }

    fn tokenize(mut self) -> Result<Vec<(Token, usize)>, BandMathError> {
        let mut out: Vec<(Token, usize)> = Vec::new();
        while self.pos < self.src.len() {
            let b = self.src[self.pos];
            if b.is_ascii_whitespace() {
                self.pos += 1;
                continue;
            }
            let start = self.pos;
            if b.is_ascii_digit() || b == b'.' {
                let (tok, new_pos) = self.lex_number(start)?;
                out.push((tok, start));
                self.pos = new_pos;
            } else if b == b'B' && self.peek_is_digit(self.pos + 1) {
                let (tok, new_pos) = self.lex_band(start)?;
                out.push((tok, start));
                self.pos = new_pos;
            } else if b.is_ascii_alphabetic() || b == b'_' {
                let (tok, new_pos) = self.lex_identifier(start)?;
                out.push((tok, start));
                self.pos = new_pos;
            } else {
                match b {
                    b'+' | b'-' | b'*' | b'/' | b'^' => {
                        out.push((Token::Op(b as char), start));
                        self.pos += 1;
                    }
                    b'(' => {
                        out.push((Token::LParen, start));
                        self.pos += 1;
                    }
                    b')' => {
                        out.push((Token::RParen, start));
                        self.pos += 1;
                    }
                    b',' => {
                        out.push((Token::Comma, start));
                        self.pos += 1;
                    }
                    _ => {
                        return Err(BandMathError::UnexpectedToken {
                            pos: start,
                            found: (b as char).to_string(),
                        });
                    }
                }
            }
        }
        Ok(out)
    }

    fn peek_is_digit(&self, idx: usize) -> bool {
        idx < self.src.len() && self.src[idx].is_ascii_digit()
    }

    fn lex_number(&self, start: usize) -> Result<(Token, usize), BandMathError> {
        let mut i = start;
        let mut saw_dot = false;
        let mut saw_digit = false;
        while i < self.src.len() {
            let c = self.src[i];
            if c.is_ascii_digit() {
                saw_digit = true;
                i += 1;
            } else if c == b'.' && !saw_dot {
                saw_dot = true;
                i += 1;
            } else {
                break;
            }
        }
        // Optional exponent: e/E [+-]? digits+
        if i < self.src.len() && (self.src[i] == b'e' || self.src[i] == b'E') {
            i += 1;
            if i < self.src.len() && (self.src[i] == b'+' || self.src[i] == b'-') {
                i += 1;
            }
            let exp_start = i;
            while i < self.src.len() && self.src[i].is_ascii_digit() {
                i += 1;
            }
            if i == exp_start {
                let text = std::str::from_utf8(&self.src[start..i])
                    .unwrap_or("")
                    .to_string();
                return Err(BandMathError::InvalidNumber { pos: start, text });
            }
        }
        if !saw_digit {
            // Just a `.` with no digits — invalid.
            let text = std::str::from_utf8(&self.src[start..i])
                .unwrap_or("")
                .to_string();
            return Err(BandMathError::InvalidNumber { pos: start, text });
        }
        let text = std::str::from_utf8(&self.src[start..i]).unwrap_or("");
        let n: f64 = text.parse().map_err(|_| BandMathError::InvalidNumber {
            pos: start,
            text: text.to_string(),
        })?;
        Ok((Token::Number(n), i))
    }

    fn lex_band(&self, start: usize) -> Result<(Token, usize), BandMathError> {
        // start points at 'B'; subsequent chars are digits.
        let mut i = start + 1;
        while i < self.src.len() && self.src[i].is_ascii_digit() {
            i += 1;
        }
        let digits = std::str::from_utf8(&self.src[start + 1..i]).unwrap_or("");
        let n: u32 = digits
            .parse()
            .map_err(|_| BandMathError::InvalidBandIndex(0))?;
        if n == 0 || n > 999 {
            return Err(BandMathError::InvalidBandIndex(n));
        }
        // Internal AST index is 0-based.
        Ok((Token::Band((n - 1) as usize), i))
    }

    fn lex_identifier(&self, start: usize) -> Result<(Token, usize), BandMathError> {
        let mut i = start;
        while i < self.src.len() {
            let c = self.src[i];
            if c.is_ascii_alphanumeric() || c == b'_' {
                i += 1;
            } else {
                break;
            }
        }
        let name = std::str::from_utf8(&self.src[start..i]).unwrap_or("");
        // Skip whitespace to see if `(` follows.
        let mut j = i;
        while j < self.src.len() && self.src[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < self.src.len() && self.src[j] == b'(' {
            Ok((Token::Func(name.to_string()), i))
        } else {
            Err(BandMathError::UnexpectedToken {
                pos: start,
                found: name.to_string(),
            })
        }
    }
}

/// Pratt-style precedence-climbing parser.
struct Parser {
    tokens: Vec<(Token, usize)>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&(Token, usize)> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<(Token, usize)> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn current_pos(&self) -> usize {
        self.tokens
            .get(self.pos)
            .map(|(_, p)| *p)
            .unwrap_or_else(|| self.tokens.last().map(|(_, p)| *p).unwrap_or(0))
    }

    fn parse_expr(&mut self) -> Result<BandExpression, BandMathError> {
        let mut lhs = self.parse_term()?;
        while let Some((Token::Op(op), _)) = self.peek().cloned() {
            if op != '+' && op != '-' {
                break;
            }
            self.advance();
            let rhs = self.parse_term()?;
            lhs = match op {
                '+' => BandExpression::Add(Box::new(lhs), Box::new(rhs)),
                '-' => BandExpression::Sub(Box::new(lhs), Box::new(rhs)),
                _ => unreachable!("operator dispatch guarded above"),
            };
        }
        Ok(lhs)
    }

    fn parse_term(&mut self) -> Result<BandExpression, BandMathError> {
        let mut lhs = self.parse_factor()?;
        while let Some((Token::Op(op), _)) = self.peek().cloned() {
            if op != '*' && op != '/' {
                break;
            }
            self.advance();
            let rhs = self.parse_factor()?;
            lhs = match op {
                '*' => BandExpression::Mul(Box::new(lhs), Box::new(rhs)),
                '/' => {
                    // Reject literal divide-by-zero at parse time when the
                    // right-hand side is a single `Constant(0.0)`.
                    if let BandExpression::Constant(c) = &rhs {
                        if c.abs() < 1e-20 {
                            return Err(BandMathError::DivByConstantZero);
                        }
                    }
                    BandExpression::Div(Box::new(lhs), Box::new(rhs))
                }
                _ => unreachable!("operator dispatch guarded above"),
            };
        }
        Ok(lhs)
    }

    fn parse_factor(&mut self) -> Result<BandExpression, BandMathError> {
        self.parse_power()
    }

    fn parse_power(&mut self) -> Result<BandExpression, BandMathError> {
        let lhs = self.parse_unary()?;
        if let Some((Token::Op('^'), _)) = self.peek().cloned() {
            self.advance();
            // Right-associative: recurse into parse_power again so that
            // `a ^ b ^ c` parses as `a ^ (b ^ c)`.
            let rhs = self.parse_power()?;
            Ok(BandExpression::Pow(Box::new(lhs), Box::new(rhs)))
        } else {
            Ok(lhs)
        }
    }

    fn parse_unary(&mut self) -> Result<BandExpression, BandMathError> {
        if let Some((Token::Op('-'), _)) = self.peek().cloned() {
            self.advance();
            let inner = self.parse_unary()?;
            return Ok(BandExpression::Neg(Box::new(inner)));
        }
        if let Some((Token::Op('+'), _)) = self.peek().cloned() {
            // Unary plus is a no-op.
            self.advance();
            return self.parse_unary();
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<BandExpression, BandMathError> {
        let (tok, pos) = self.advance().ok_or(BandMathError::EmptyExpression)?;
        match tok {
            Token::Number(n) => Ok(BandExpression::Constant(n as f32)),
            Token::Band(idx) => Ok(BandExpression::Band(idx)),
            Token::Func(name) => {
                // Expect `(`.
                match self.advance() {
                    Some((Token::LParen, _)) => {}
                    Some((other, p)) => {
                        return Err(BandMathError::UnexpectedToken {
                            pos: p,
                            found: format!("{:?}", other),
                        });
                    }
                    None => return Err(BandMathError::UnmatchedParen),
                }
                let args = self.parse_args()?;
                // Expect `)`.
                match self.advance() {
                    Some((Token::RParen, _)) => {}
                    Some((_, p)) => {
                        return Err(BandMathError::UnexpectedToken {
                            pos: p,
                            found: ")".to_string(),
                        });
                    }
                    None => return Err(BandMathError::UnmatchedParen),
                }
                self.dispatch_function(&name, args)
            }
            Token::LParen => {
                let inner = self.parse_expr()?;
                match self.advance() {
                    Some((Token::RParen, _)) => Ok(inner),
                    _ => Err(BandMathError::UnmatchedParen),
                }
            }
            other => Err(BandMathError::UnexpectedToken {
                pos,
                found: format!("{:?}", other),
            }),
        }
    }

    fn parse_args(&mut self) -> Result<Vec<BandExpression>, BandMathError> {
        let mut args: Vec<BandExpression> = Vec::new();
        // Empty argument list `f()`.
        if let Some((Token::RParen, _)) = self.peek() {
            return Ok(args);
        }
        args.push(self.parse_expr()?);
        while let Some((Token::Comma, _)) = self.peek() {
            self.advance();
            args.push(self.parse_expr()?);
        }
        Ok(args)
    }

    fn dispatch_function(
        &self,
        name: &str,
        mut args: Vec<BandExpression>,
    ) -> Result<BandExpression, BandMathError> {
        let lower = name.to_ascii_lowercase();
        match lower.as_str() {
            "log" | "ln" => {
                self.check_arity(name, 1, &args)?;
                Ok(BandExpression::Log(Box::new(args.remove(0))))
            }
            "exp" => {
                self.check_arity(name, 1, &args)?;
                Ok(BandExpression::Exp(Box::new(args.remove(0))))
            }
            "sqrt" => {
                self.check_arity(name, 1, &args)?;
                Ok(BandExpression::Sqrt(Box::new(args.remove(0))))
            }
            "abs" => {
                self.check_arity(name, 1, &args)?;
                Ok(BandExpression::Abs(Box::new(args.remove(0))))
            }
            "min" => {
                self.check_arity(name, 2, &args)?;
                let b = args.remove(1);
                let a = args.remove(0);
                Ok(BandExpression::Min(Box::new(a), Box::new(b)))
            }
            "max" => {
                self.check_arity(name, 2, &args)?;
                let b = args.remove(1);
                let a = args.remove(0);
                Ok(BandExpression::Max(Box::new(a), Box::new(b)))
            }
            "pow" => {
                self.check_arity(name, 2, &args)?;
                let b = args.remove(1);
                let a = args.remove(0);
                Ok(BandExpression::Pow(Box::new(a), Box::new(b)))
            }
            "clamp" => {
                self.check_arity(name, 3, &args)?;
                let hi = args.remove(2);
                let lo = args.remove(1);
                let value = args.remove(0);
                Ok(BandExpression::Clamp {
                    value: Box::new(value),
                    lo: Box::new(lo),
                    hi: Box::new(hi),
                })
            }
            _ => Err(BandMathError::UnknownFunction(name.to_string())),
        }
    }

    fn check_arity(
        &self,
        name: &str,
        expected: usize,
        args: &[BandExpression],
    ) -> Result<(), BandMathError> {
        if args.len() == expected {
            Ok(())
        } else {
            Err(BandMathError::WrongArgCount {
                function: name.to_string(),
                expected,
                actual: args.len(),
            })
        }
    }
}

/// Parse `s` into a [`BandExpression`].
///
/// # Errors
///
/// Returns [`BandMathError`] on any lex/parse failure.
pub fn parse_band_expression(s: &str) -> Result<BandExpression, BandMathError> {
    let tokens = Lexer::new(s).tokenize()?;
    if tokens.is_empty() {
        return Err(BandMathError::EmptyExpression);
    }
    let mut p = Parser { tokens, pos: 0 };
    let expr = p.parse_expr()?;
    if p.pos < p.tokens.len() {
        return Err(BandMathError::TrailingTokens(p.current_pos()));
    }
    Ok(expr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lex_number_simple() {
        let toks = Lexer::new("1.5 + 2").tokenize().expect("tokenize");
        assert_eq!(toks.len(), 3);
        assert_eq!(toks[0].0, Token::Number(1.5));
        assert_eq!(toks[1].0, Token::Op('+'));
        assert_eq!(toks[2].0, Token::Number(2.0));
    }

    #[test]
    fn test_lex_band_reference() {
        let toks = Lexer::new("B1 - B42").tokenize().expect("tokenize");
        assert_eq!(toks[0].0, Token::Band(0));
        assert_eq!(toks[2].0, Token::Band(41));
    }

    #[test]
    fn test_lex_unknown_char_errors() {
        let err = Lexer::new("B1 @ B2").tokenize().expect_err("must fail");
        assert!(
            matches!(err, BandMathError::UnexpectedToken { .. }),
            "wrong variant: {:?}",
            err
        );
    }

    #[test]
    fn test_parse_addition() {
        let e = parse_band_expression("B1 + B2").expect("parse");
        assert!(
            matches!(e, BandExpression::Add(_, _)),
            "expected Add, got {:?}",
            e
        );
    }
}
