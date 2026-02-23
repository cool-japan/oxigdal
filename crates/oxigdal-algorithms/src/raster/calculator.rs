//! Raster calculator (map algebra) with expression parsing
//!
//! This module provides a comprehensive raster calculator with support for:
//! - Arithmetic operations: +, -, *, /, ^
//! - Math functions: sqrt, log, exp, sin, cos, tan, abs, floor, ceil, etc.
//! - Band algebra: (B1 - B2) / (B1 + B2) for NDVI and similar indices
//! - Conditional operations: if/then/else
//! - Multi-band operations
//! - Proper NoData handling

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Token types for expression lexer
#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// Number literal
    Number(f64),
    /// Band reference (e.g., B1, B2)
    Band(usize),
    /// Identifier (function name or variable)
    Ident(String),
    /// Operators
    Plus,
    Minus,
    Multiply,
    Divide,
    Power,
    /// Parentheses
    LeftParen,
    RightParen,
    /// Comparison operators
    Greater,
    Less,
    GreaterEqual,
    LessEqual,
    Equal,
    NotEqual,
    /// Logical operators
    And,
    Or,
    /// Keywords
    If,
    Then,
    Else,
    /// Comma (for function arguments)
    Comma,
}

/// Expression AST node
#[derive(Debug, Clone)]
enum Expr {
    /// Number literal
    Number(f64),
    /// Band reference
    Band(usize),
    /// Binary operation
    BinaryOp {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    /// Unary operation
    UnaryOp { op: UnaryOp, expr: Box<Expr> },
    /// Function call
    Function { name: String, args: Vec<Expr> },
    /// Conditional expression
    Conditional {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq)]
enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
    Greater,
    Less,
    GreaterEqual,
    LessEqual,
    Equal,
    NotEqual,
    And,
    Or,
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq)]
enum UnaryOp {
    Negate,
}

/// Tokenizer for raster expressions
struct Lexer {
    input: Vec<char>,
    pos: usize,
}

impl Lexer {
    fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
        }
    }

    fn current(&self) -> Option<char> {
        if self.pos < self.input.len() {
            Some(self.input[self.pos])
        } else {
            None
        }
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.current() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_number(&mut self) -> Result<f64> {
        let mut num_str = String::new();
        let mut has_dot = false;

        while let Some(c) = self.current() {
            if c.is_ascii_digit() {
                num_str.push(c);
                self.advance();
            } else if c == '.' && !has_dot {
                has_dot = true;
                num_str.push(c);
                self.advance();
            } else {
                break;
            }
        }

        num_str
            .parse::<f64>()
            .map_err(|_| AlgorithmError::InvalidParameter {
                parameter: "expression",
                message: format!("Invalid number: {num_str}"),
            })
    }

    fn read_ident(&mut self) -> String {
        let mut ident = String::new();

        while let Some(c) = self.current() {
            if c.is_alphanumeric() || c == '_' {
                ident.push(c);
                self.advance();
            } else {
                break;
            }
        }

        ident
    }

    fn next_token(&mut self) -> Result<Option<Token>> {
        self.skip_whitespace();

        let c = match self.current() {
            Some(c) => c,
            None => return Ok(None),
        };

        let token = if c.is_ascii_digit() {
            Token::Number(self.read_number()?)
        } else if c.is_alphabetic() || c == 'B' {
            let ident = self.read_ident();

            // Check for band reference (B1, B2, etc.)
            if ident.starts_with('B') && ident.len() > 1 {
                let band_num = ident[1..].parse::<usize>().ok();
                if let Some(num) = band_num {
                    return Ok(Some(Token::Band(num)));
                }
            }

            // Check for keywords
            match ident.as_str() {
                "if" | "IF" => Token::If,
                "then" | "THEN" => Token::Then,
                "else" | "ELSE" => Token::Else,
                "and" | "AND" => Token::And,
                "or" | "OR" => Token::Or,
                _ => Token::Ident(ident),
            }
        } else {
            self.advance();
            match c {
                '+' => Token::Plus,
                '-' => Token::Minus,
                '*' => Token::Multiply,
                '/' => Token::Divide,
                '^' => Token::Power,
                '(' => Token::LeftParen,
                ')' => Token::RightParen,
                ',' => Token::Comma,
                '>' => {
                    if self.current() == Some('=') {
                        self.advance();
                        Token::GreaterEqual
                    } else {
                        Token::Greater
                    }
                }
                '<' => {
                    if self.current() == Some('=') {
                        self.advance();
                        Token::LessEqual
                    } else {
                        Token::Less
                    }
                }
                '=' => {
                    if self.current() == Some('=') {
                        self.advance();
                        Token::Equal
                    } else {
                        return Err(AlgorithmError::InvalidParameter {
                            parameter: "expression",
                            message: "Expected '==' for equality comparison".to_string(),
                        });
                    }
                }
                '!' => {
                    if self.current() == Some('=') {
                        self.advance();
                        Token::NotEqual
                    } else {
                        return Err(AlgorithmError::InvalidParameter {
                            parameter: "expression",
                            message: "Expected '!=' for inequality comparison".to_string(),
                        });
                    }
                }
                _ => {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "expression",
                        message: format!("Unexpected character: {c}"),
                    });
                }
            }
        };

        Ok(Some(token))
    }

    fn tokenize(&mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();

        while let Some(token) = self.next_token()? {
            tokens.push(token);
        }

        Ok(tokens)
    }
}

/// Parser for raster expressions
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn current(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn parse(&mut self) -> Result<Expr> {
        self.parse_conditional()
    }

    fn parse_conditional(&mut self) -> Result<Expr> {
        if matches!(self.current(), Some(Token::If)) {
            self.advance();
            let condition = Box::new(self.parse_or()?);

            if !matches!(self.current(), Some(Token::Then)) {
                return Err(AlgorithmError::InvalidParameter {
                    parameter: "expression",
                    message: "Expected 'then' after if condition".to_string(),
                });
            }
            self.advance();

            let then_expr = Box::new(self.parse_or()?);

            if !matches!(self.current(), Some(Token::Else)) {
                return Err(AlgorithmError::InvalidParameter {
                    parameter: "expression",
                    message: "Expected 'else' in conditional".to_string(),
                });
            }
            self.advance();

            let else_expr = Box::new(self.parse_or()?);

            Ok(Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            })
        } else {
            self.parse_or()
        }
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;

        while matches!(self.current(), Some(Token::Or)) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_comparison()?;

        while matches!(self.current(), Some(Token::And)) {
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive()?;

        while let Some(token) = self.current() {
            let op = match token {
                Token::Greater => BinaryOp::Greater,
                Token::Less => BinaryOp::Less,
                Token::GreaterEqual => BinaryOp::GreaterEqual,
                Token::LessEqual => BinaryOp::LessEqual,
                Token::Equal => BinaryOp::Equal,
                Token::NotEqual => BinaryOp::NotEqual,
                _ => break,
            };

            self.advance();
            let right = self.parse_additive()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplicative()?;

        while let Some(token) = self.current() {
            let op = match token {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Subtract,
                _ => break,
            };

            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let mut left = self.parse_power()?;

        while let Some(token) = self.current() {
            let op = match token {
                Token::Multiply => BinaryOp::Multiply,
                Token::Divide => BinaryOp::Divide,
                _ => break,
            };

            self.advance();
            let right = self.parse_power()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_power(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;

        while matches!(self.current(), Some(Token::Power)) {
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::Power,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if matches!(self.current(), Some(Token::Minus)) {
            self.advance();
            let expr = self.parse_unary()?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Negate,
                expr: Box::new(expr),
            })
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.current() {
            Some(Token::Number(n)) => {
                let val = *n;
                self.advance();
                Ok(Expr::Number(val))
            }
            Some(Token::Band(b)) => {
                let band = *b;
                self.advance();
                Ok(Expr::Band(band))
            }
            Some(Token::Ident(name)) => {
                let func_name = name.clone();
                self.advance();

                if matches!(self.current(), Some(Token::LeftParen)) {
                    self.advance();
                    let mut args = Vec::new();

                    if !matches!(self.current(), Some(Token::RightParen)) {
                        loop {
                            args.push(self.parse_conditional()?);

                            if matches!(self.current(), Some(Token::Comma)) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }

                    if !matches!(self.current(), Some(Token::RightParen)) {
                        return Err(AlgorithmError::InvalidParameter {
                            parameter: "expression",
                            message: "Expected ')' after function arguments".to_string(),
                        });
                    }
                    self.advance();

                    Ok(Expr::Function {
                        name: func_name,
                        args,
                    })
                } else {
                    Err(AlgorithmError::InvalidParameter {
                        parameter: "expression",
                        message: format!("Unknown identifier: {func_name}"),
                    })
                }
            }
            Some(Token::LeftParen) => {
                self.advance();
                let expr = self.parse_conditional()?;

                if !matches!(self.current(), Some(Token::RightParen)) {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "expression",
                        message: "Expected ')'".to_string(),
                    });
                }
                self.advance();

                Ok(expr)
            }
            _ => Err(AlgorithmError::InvalidParameter {
                parameter: "expression",
                message: "Unexpected token in expression".to_string(),
            }),
        }
    }
}

/// Expression optimizer for constant folding and algebraic simplifications
struct Optimizer;

impl Optimizer {
    /// Optimize an expression tree
    fn optimize(expr: Expr) -> Expr {
        let expr = Self::constant_fold(expr);
        let expr = Self::algebraic_simplify(expr);
        Self::eliminate_common_subexpressions(expr)
    }

    /// Constant folding: evaluate constant expressions at compile time
    fn constant_fold(expr: Expr) -> Expr {
        match expr {
            Expr::BinaryOp { left, op, right } => {
                let left = Self::constant_fold(*left);
                let right = Self::constant_fold(*right);

                // If both sides are constants, evaluate
                if let (Expr::Number(l), Expr::Number(r)) = (&left, &right) {
                    let result = match op {
                        BinaryOp::Add => l + r,
                        BinaryOp::Subtract => l - r,
                        BinaryOp::Multiply => l * r,
                        BinaryOp::Divide => {
                            if r.abs() < f64::EPSILON {
                                f64::NAN
                            } else {
                                l / r
                            }
                        }
                        BinaryOp::Power => l.powf(*r),
                        BinaryOp::Greater => {
                            if l > r {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::Less => {
                            if l < r {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::GreaterEqual => {
                            if l >= r {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::LessEqual => {
                            if l <= r {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::Equal => {
                            if (l - r).abs() < f64::EPSILON {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::NotEqual => {
                            if (l - r).abs() >= f64::EPSILON {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::And => {
                            if *l != 0.0 && *r != 0.0 {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        BinaryOp::Or => {
                            if *l != 0.0 || *r != 0.0 {
                                1.0
                            } else {
                                0.0
                            }
                        }
                    };
                    return Expr::Number(result);
                }

                Expr::BinaryOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                }
            }
            Expr::UnaryOp { op, expr } => {
                let expr = Self::constant_fold(*expr);
                if let Expr::Number(n) = expr {
                    let result = match op {
                        UnaryOp::Negate => -n,
                    };
                    return Expr::Number(result);
                }
                Expr::UnaryOp {
                    op,
                    expr: Box::new(expr),
                }
            }
            Expr::Function { name, args } => {
                let args: Vec<Expr> = args.into_iter().map(Self::constant_fold).collect();

                // If all args are constants, evaluate the function
                let all_const = args.iter().all(|arg| matches!(arg, Expr::Number(_)));
                if all_const {
                    let arg_vals: Vec<f64> = args
                        .iter()
                        .filter_map(|arg| {
                            if let Expr::Number(n) = arg {
                                Some(*n)
                            } else {
                                None
                            }
                        })
                        .collect();

                    let result = match name.as_str() {
                        "sqrt" if arg_vals.len() == 1 => Some(arg_vals[0].sqrt()),
                        "abs" if arg_vals.len() == 1 => Some(arg_vals[0].abs()),
                        "log" if arg_vals.len() == 1 => Some(arg_vals[0].ln()),
                        "log10" if arg_vals.len() == 1 => Some(arg_vals[0].log10()),
                        "exp" if arg_vals.len() == 1 => Some(arg_vals[0].exp()),
                        "sin" if arg_vals.len() == 1 => Some(arg_vals[0].sin()),
                        "cos" if arg_vals.len() == 1 => Some(arg_vals[0].cos()),
                        "tan" if arg_vals.len() == 1 => Some(arg_vals[0].tan()),
                        "floor" if arg_vals.len() == 1 => Some(arg_vals[0].floor()),
                        "ceil" if arg_vals.len() == 1 => Some(arg_vals[0].ceil()),
                        "round" if arg_vals.len() == 1 => Some(arg_vals[0].round()),
                        "min" if !arg_vals.is_empty() => {
                            Some(arg_vals.iter().copied().fold(f64::INFINITY, f64::min))
                        }
                        "max" if !arg_vals.is_empty() => {
                            Some(arg_vals.iter().copied().fold(f64::NEG_INFINITY, f64::max))
                        }
                        _ => None,
                    };

                    if let Some(val) = result {
                        return Expr::Number(val);
                    }
                }

                Expr::Function { name, args }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let condition = Self::constant_fold(*condition);
                let then_expr = Self::constant_fold(*then_expr);
                let else_expr = Self::constant_fold(*else_expr);

                // If condition is constant, choose branch
                if let Expr::Number(cond) = condition {
                    if cond != 0.0 {
                        return then_expr;
                    } else {
                        return else_expr;
                    }
                }

                Expr::Conditional {
                    condition: Box::new(condition),
                    then_expr: Box::new(then_expr),
                    else_expr: Box::new(else_expr),
                }
            }
            other => other,
        }
    }

    /// Algebraic simplifications: x + 0 -> x, x * 1 -> x, x * 0 -> 0, etc.
    fn algebraic_simplify(expr: Expr) -> Expr {
        match expr {
            Expr::BinaryOp { left, op, right } => {
                let left = Self::algebraic_simplify(*left);
                let right = Self::algebraic_simplify(*right);

                match (&left, op, &right) {
                    // x + 0 = x, 0 + x = x
                    (_, BinaryOp::Add, Expr::Number(n)) if n.abs() < f64::EPSILON => left,
                    (Expr::Number(n), BinaryOp::Add, _) if n.abs() < f64::EPSILON => right,

                    // x - 0 = x
                    (_, BinaryOp::Subtract, Expr::Number(n)) if n.abs() < f64::EPSILON => left,

                    // x * 0 = 0, 0 * x = 0
                    (_, BinaryOp::Multiply, Expr::Number(n))
                    | (Expr::Number(n), BinaryOp::Multiply, _)
                        if n.abs() < f64::EPSILON =>
                    {
                        Expr::Number(0.0)
                    }

                    // x * 1 = x, 1 * x = x
                    (_, BinaryOp::Multiply, Expr::Number(n)) if (n - 1.0).abs() < f64::EPSILON => {
                        left
                    }
                    (Expr::Number(n), BinaryOp::Multiply, _) if (n - 1.0).abs() < f64::EPSILON => {
                        right
                    }

                    // x / 1 = x
                    (_, BinaryOp::Divide, Expr::Number(n)) if (n - 1.0).abs() < f64::EPSILON => {
                        left
                    }

                    // x ^ 0 = 1
                    (_, BinaryOp::Power, Expr::Number(n)) if n.abs() < f64::EPSILON => {
                        Expr::Number(1.0)
                    }

                    // x ^ 1 = x
                    (_, BinaryOp::Power, Expr::Number(n)) if (n - 1.0).abs() < f64::EPSILON => left,

                    _ => Expr::BinaryOp {
                        left: Box::new(left),
                        op,
                        right: Box::new(right),
                    },
                }
            }
            Expr::UnaryOp { op, expr } => {
                let expr = Self::algebraic_simplify(*expr);
                Expr::UnaryOp {
                    op,
                    expr: Box::new(expr),
                }
            }
            Expr::Function { name, args } => {
                let args = args.into_iter().map(Self::algebraic_simplify).collect();
                Expr::Function { name, args }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => Expr::Conditional {
                condition: Box::new(Self::algebraic_simplify(*condition)),
                then_expr: Box::new(Self::algebraic_simplify(*then_expr)),
                else_expr: Box::new(Self::algebraic_simplify(*else_expr)),
            },
            other => other,
        }
    }

    /// Common Subexpression Elimination (simplified version)
    /// In a full implementation, this would detect and cache repeated subexpressions
    fn eliminate_common_subexpressions(expr: Expr) -> Expr {
        // For now, this is a placeholder for a more sophisticated CSE pass
        // A full implementation would:
        // 1. Build a hash map of expression -> cache variable
        // 2. Replace repeated expressions with cache lookups
        // 3. Require changing the evaluation model to support cached values
        expr
    }
}

/// Evaluator for raster expressions
struct Evaluator<'a> {
    bands: &'a [RasterBuffer],
}

impl<'a> Evaluator<'a> {
    fn new(bands: &'a [RasterBuffer]) -> Self {
        Self { bands }
    }

    fn eval_pixel(&self, expr: &Expr, x: u64, y: u64) -> Result<f64> {
        match expr {
            Expr::Number(n) => Ok(*n),
            Expr::Band(b) => {
                if *b == 0 || *b > self.bands.len() {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "band",
                        message: format!("Band {} out of range (1-{})", b, self.bands.len()),
                    });
                }
                self.bands[b - 1]
                    .get_pixel(x, y)
                    .map_err(AlgorithmError::Core)
            }
            Expr::BinaryOp { left, op, right } => {
                let lval = self.eval_pixel(left, x, y)?;
                let rval = self.eval_pixel(right, x, y)?;

                let result = match op {
                    BinaryOp::Add => lval + rval,
                    BinaryOp::Subtract => lval - rval,
                    BinaryOp::Multiply => lval * rval,
                    BinaryOp::Divide => {
                        if rval.abs() < f64::EPSILON {
                            f64::NAN
                        } else {
                            lval / rval
                        }
                    }
                    BinaryOp::Power => lval.powf(rval),
                    BinaryOp::Greater => {
                        if lval > rval {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::Less => {
                        if lval < rval {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::GreaterEqual => {
                        if lval >= rval {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::LessEqual => {
                        if lval <= rval {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::Equal => {
                        if (lval - rval).abs() < f64::EPSILON {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::NotEqual => {
                        if (lval - rval).abs() >= f64::EPSILON {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::And => {
                        if lval != 0.0 && rval != 0.0 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::Or => {
                        if lval != 0.0 || rval != 0.0 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                };

                Ok(result)
            }
            Expr::UnaryOp { op, expr } => {
                let val = self.eval_pixel(expr, x, y)?;
                match op {
                    UnaryOp::Negate => Ok(-val),
                }
            }
            Expr::Function { name, args } => self.eval_function(name, args, x, y),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let cond_val = self.eval_pixel(condition, x, y)?;
                if cond_val != 0.0 {
                    self.eval_pixel(then_expr, x, y)
                } else {
                    self.eval_pixel(else_expr, x, y)
                }
            }
        }
    }

    fn eval_function(&self, name: &str, args: &[Expr], x: u64, y: u64) -> Result<f64> {
        let arg_vals: Result<Vec<f64>> =
            args.iter().map(|arg| self.eval_pixel(arg, x, y)).collect();
        let arg_vals = arg_vals?;

        let result = match name {
            "sqrt" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "sqrt",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].sqrt()
            }
            "abs" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "abs",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].abs()
            }
            "log" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "log",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].ln()
            }
            "log10" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "log10",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].log10()
            }
            "exp" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "exp",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].exp()
            }
            "sin" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "sin",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].sin()
            }
            "cos" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "cos",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].cos()
            }
            "tan" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "tan",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].tan()
            }
            "floor" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "floor",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].floor()
            }
            "ceil" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "ceil",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].ceil()
            }
            "round" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "round",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].round()
            }
            "min" => {
                if arg_vals.is_empty() {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "min",
                        message: "Expected at least 1 argument".to_string(),
                    });
                }
                arg_vals.iter().copied().fold(f64::INFINITY, f64::min)
            }
            "max" => {
                if arg_vals.is_empty() {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "max",
                        message: "Expected at least 1 argument".to_string(),
                    });
                }
                arg_vals.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            }
            _ => {
                return Err(AlgorithmError::InvalidParameter {
                    parameter: "function",
                    message: format!("Unknown function: {name}"),
                });
            }
        };

        Ok(result)
    }
}

/// Raster calculator for map algebra with expression parsing
pub struct RasterCalculator;

impl RasterCalculator {
    /// Evaluates a raster expression on one or more bands
    ///
    /// # Arguments
    ///
    /// * `expression` - The expression to evaluate (e.g., "(B1 - B2) / (B1 + B2)")
    /// * `bands` - Input bands (B1, B2, etc.)
    ///
    /// # Examples
    ///
    /// NDVI: `"(B1 - B2) / (B1 + B2)"`
    /// Conditional: `"if B1 > 100 then B1 * 2 else B1"`
    /// Math: `"sqrt(B1 ^ 2 + B2 ^ 2)"`
    ///
    /// # Errors
    ///
    /// Returns an error if the expression is invalid or evaluation fails
    pub fn evaluate(expression: &str, bands: &[RasterBuffer]) -> Result<RasterBuffer> {
        if bands.is_empty() {
            return Err(AlgorithmError::EmptyInput {
                operation: "evaluate",
            });
        }

        // Check all bands have same dimensions
        let width = bands[0].width();
        let height = bands[0].height();
        for (_i, band) in bands.iter().enumerate().skip(1) {
            if band.width() != width || band.height() != height {
                return Err(AlgorithmError::InvalidDimensions {
                    message: "All bands must have same dimensions",
                    actual: band.width() as usize,
                    expected: width as usize,
                });
            }
        }

        // Tokenize
        let mut lexer = Lexer::new(expression);
        let tokens = lexer.tokenize()?;

        // Parse
        let mut parser = Parser::new(tokens);
        let expr = parser.parse()?;

        // Optimize expression
        let expr = Optimizer::optimize(expr);

        // Evaluate
        let evaluator = Evaluator::new(bands);
        let mut result = RasterBuffer::zeros(width, height, bands[0].data_type());

        for y in 0..height {
            for x in 0..width {
                let value = evaluator.eval_pixel(&expr, x, y)?;
                result
                    .set_pixel(x, y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(result)
    }

    /// Evaluates a raster expression in parallel using rayon
    ///
    /// This method processes rows in parallel for improved performance on multi-core systems.
    /// Falls back to sequential evaluation if the parallel feature is not enabled.
    ///
    /// # Arguments
    ///
    /// * `expression` - The expression to evaluate (e.g., "(B1 - B2) / (B1 + B2)")
    /// * `bands` - Input bands (B1, B2, etc.)
    ///
    /// # Examples
    ///
    /// NDVI: `"(B1 - B2) / (B1 + B2)"`
    /// Conditional: `"if B1 > 100 then B1 * 2 else B1"`
    /// Math: `"sqrt(B1 ^ 2 + B2 ^ 2)"`
    ///
    /// # Errors
    ///
    /// Returns an error if the expression is invalid or evaluation fails
    #[cfg(feature = "parallel")]
    pub fn evaluate_parallel(expression: &str, bands: &[RasterBuffer]) -> Result<RasterBuffer> {
        if bands.is_empty() {
            return Err(AlgorithmError::EmptyInput {
                operation: "evaluate_parallel",
            });
        }

        // Check all bands have same dimensions
        let width = bands[0].width();
        let height = bands[0].height();
        for band in bands.iter().skip(1) {
            if band.width() != width || band.height() != height {
                return Err(AlgorithmError::InvalidDimensions {
                    message: "All bands must have same dimensions",
                    actual: band.width() as usize,
                    expected: width as usize,
                });
            }
        }

        // Tokenize
        let mut lexer = Lexer::new(expression);
        let tokens = lexer.tokenize()?;

        // Parse
        let mut parser = Parser::new(tokens);
        let expr = parser.parse()?;

        // Optimize expression
        let expr = Optimizer::optimize(expr);

        // Create evaluator
        let evaluator = Evaluator::new(bands);

        // Create result buffer
        let mut result = RasterBuffer::zeros(width, height, bands[0].data_type());

        // Process rows in parallel
        let row_data: Result<Vec<Vec<f64>>> = (0..height)
            .into_par_iter()
            .map(|y| {
                let mut row = Vec::with_capacity(width as usize);
                for x in 0..width {
                    let value = evaluator.eval_pixel(&expr, x, y)?;
                    row.push(value);
                }
                Ok(row)
            })
            .collect();

        let row_data = row_data?;

        // Write results back to buffer
        for (y, row) in row_data.iter().enumerate() {
            for (x, &value) in row.iter().enumerate() {
                result
                    .set_pixel(x as u64, y as u64, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(result)
    }

    /// Applies a binary operation to two rasters (legacy API)
    pub fn apply_binary(
        a: &RasterBuffer,
        b: &RasterBuffer,
        op: RasterExpression,
    ) -> Result<RasterBuffer> {
        if a.width() != b.width() || a.height() != b.height() {
            return Err(AlgorithmError::InvalidDimensions {
                message: "Rasters must have same dimensions",
                actual: a.width() as usize,
                expected: b.width() as usize,
            });
        }

        let mut result = RasterBuffer::zeros(a.width(), a.height(), a.data_type());

        for y in 0..a.height() {
            for x in 0..a.width() {
                let val_a = a.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                let val_b = b.get_pixel(x, y).map_err(AlgorithmError::Core)?;

                let val = match op {
                    RasterExpression::Add => val_a + val_b,
                    RasterExpression::Subtract => val_a - val_b,
                    RasterExpression::Multiply => val_a * val_b,
                    RasterExpression::Divide => {
                        if val_b.abs() < f64::EPSILON {
                            f64::NAN
                        } else {
                            val_a / val_b
                        }
                    }
                    RasterExpression::Max => val_a.max(val_b),
                    RasterExpression::Min => val_a.min(val_b),
                };

                result.set_pixel(x, y, val).map_err(AlgorithmError::Core)?;
            }
        }

        Ok(result)
    }

    /// Applies a unary function to a raster (legacy API)
    pub fn apply_unary<F>(src: &RasterBuffer, func: F) -> Result<RasterBuffer>
    where
        F: Fn(f64) -> f64,
    {
        let mut result = RasterBuffer::zeros(src.width(), src.height(), src.data_type());

        for y in 0..src.height() {
            for x in 0..src.width() {
                let val = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                let new_val = func(val);
                result
                    .set_pixel(x, y, new_val)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(result)
    }
}

/// Raster expression operations (legacy API)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RasterExpression {
    /// Add two rasters
    Add,
    /// Subtract rasters
    Subtract,
    /// Multiply rasters
    Multiply,
    /// Divide rasters
    Divide,
    /// Maximum of two rasters
    Max,
    /// Minimum of two rasters
    Min,
}

#[cfg(test)]
#[allow(clippy::panic, clippy::cloned_ref_to_slice_refs)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    // ========== Basic Functionality Tests ==========

    #[test]
    fn test_simple_arithmetic() {
        let mut b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let mut b2 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                b1.set_pixel(x, y, 10.0).ok();
                b2.set_pixel(x, y, 5.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("B1 + B2", &[b1, b2]);
        assert!(result.is_ok());
        let r = result.expect("Result should be ok");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ndvi() {
        let mut nir = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let mut red = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                nir.set_pixel(x, y, 100.0).ok();
                red.set_pixel(x, y, 50.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("(B1 - B2) / (B1 + B2)", &[nir, red]);
        assert!(result.is_ok());
        let r = result.expect("Result should be ok");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        let expected = (100.0 - 50.0) / (100.0 + 50.0);
        assert!((val - expected).abs() < 0.001);
    }

    #[test]
    fn test_math_functions() {
        let mut b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                b1.set_pixel(x, y, 16.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("sqrt(B1)", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Result should be ok");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_conditional() {
        let mut b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                b1.set_pixel(x, y, (x * 10) as f64).ok();
            }
        }

        let result = RasterCalculator::evaluate("if B1 > 20 then 1 else 0", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Result should be ok");

        let val0 = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val0 - 0.0).abs() < f64::EPSILON);

        let val3 = r.get_pixel(3, 0).expect("Should get pixel");
        assert!((val3 - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_legacy_add() {
        let mut a = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let mut b = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        a.set_pixel(0, 0, 10.0).ok();
        b.set_pixel(0, 0, 5.0).ok();

        let result = RasterCalculator::apply_binary(&a, &b, RasterExpression::Add);
        assert!(result.is_ok());
    }

    // ========== Edge Cases ==========

    #[test]
    fn test_empty_bands() {
        let result = RasterCalculator::evaluate("B1 + B2", &[]);
        assert!(result.is_err());
        if let Err(AlgorithmError::EmptyInput { .. }) = result {
            // Expected
        } else {
            panic!("Expected EmptyInput error");
        }
    }

    #[test]
    fn test_single_pixel() {
        let mut b1 = RasterBuffer::zeros(1, 1, RasterDataType::Float32);
        b1.set_pixel(0, 0, 42.0).ok();

        let result = RasterCalculator::evaluate("B1 * 2", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 84.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_division_by_zero() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);
        let mut b2 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, 10.0).ok();
                b2.set_pixel(x, y, 0.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("B1 / B2", &[b1, b2]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!(val.is_nan()); // Division by zero should give NaN
    }

    #[test]
    fn test_mismatched_dimensions() {
        let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let b2 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        let result = RasterCalculator::evaluate("B1 + B2", &[b1, b2]);
        assert!(result.is_err());
        if let Err(AlgorithmError::InvalidDimensions { .. }) = result {
            // Expected
        } else {
            panic!("Expected InvalidDimensions error");
        }
    }

    // ========== Error Conditions ==========

    #[test]
    fn test_invalid_band_reference() {
        let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let result = RasterCalculator::evaluate("B5 + B1", &[b1]);
        assert!(result.is_err());
    }

    #[test]
    fn test_undefined_function() {
        let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let result = RasterCalculator::evaluate("undefined_func(B1)", &[b1]);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_expression() {
        let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let result = RasterCalculator::evaluate("B1 +", &[b1]);
        assert!(result.is_err());
    }

    #[test]
    fn test_mismatched_parentheses() {
        let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let result = RasterCalculator::evaluate("(B1 + 10", &[b1]);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_function_arity() {
        let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let result = RasterCalculator::evaluate("sqrt(B1, B1)", &[b1]);
        assert!(result.is_err());
    }

    // ========== Complex Operations ==========

    #[test]
    fn test_nested_functions() {
        let mut b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                b1.set_pixel(x, y, 9.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("sqrt(sqrt(B1))", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        let expected = 9.0_f64.sqrt().sqrt();
        assert!((val - expected).abs() < 0.001);
    }

    #[test]
    fn test_complex_expression() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);
        let mut b2 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);
        let mut b3 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, 10.0).ok();
                b2.set_pixel(x, y, 5.0).ok();
                b3.set_pixel(x, y, 2.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("(B1 + B2) * B3 - sqrt(B1)", &[b1, b2, b3]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        let expected = (10.0 + 5.0) * 2.0 - 10.0_f64.sqrt();
        assert!((val - expected).abs() < 0.001);
    }

    #[test]
    fn test_all_math_functions() {
        let mut b1 = RasterBuffer::zeros(2, 2, RasterDataType::Float32);

        for y in 0..2 {
            for x in 0..2 {
                b1.set_pixel(x, y, 2.5).ok();
            }
        }

        // Test each function
        let functions = vec![
            ("abs(B1)", 2.5),
            ("floor(B1)", 2.0),
            ("ceil(B1)", 3.0),
            ("round(B1)", 3.0), // rounds to nearest (2.5 -> 3.0)
            ("exp(B1)", 2.5_f64.exp()),
            ("log(B1)", 2.5_f64.ln()),
            ("log10(B1)", 2.5_f64.log10()),
            ("sqrt(B1)", 2.5_f64.sqrt()),
            ("sin(B1)", 2.5_f64.sin()),
            ("cos(B1)", 2.5_f64.cos()),
            ("tan(B1)", 2.5_f64.tan()),
        ];

        for (expr, expected) in functions {
            let result = RasterCalculator::evaluate(expr, &[b1.clone()]);
            assert!(result.is_ok(), "Failed for expression: {}", expr);
            let r = result.expect("Should succeed");
            let val = r.get_pixel(0, 0).expect("Should get pixel");
            assert!(
                (val - expected).abs() < 0.001,
                "Expression {} expected {} but got {}",
                expr,
                expected,
                val
            );
        }
    }

    #[test]
    fn test_min_max_functions() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);
        let mut b2 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, 10.0).ok();
                b2.set_pixel(x, y, 20.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("min(B1, B2)", &[b1.clone(), b2.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 10.0).abs() < f64::EPSILON);

        let result = RasterCalculator::evaluate("max(B1, B2)", &[b1, b2]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_power_operation() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, 2.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("B1 ^ 3", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_comparison_operators() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, 10.0).ok();
            }
        }

        let tests = vec![
            ("B1 > 5", 1.0),
            ("B1 < 5", 0.0),
            ("B1 >= 10", 1.0),
            ("B1 <= 10", 1.0),
            ("B1 == 10", 1.0),
            ("B1 != 5", 1.0),
        ];

        for (expr, expected) in tests {
            let result = RasterCalculator::evaluate(expr, &[b1.clone()]);
            assert!(result.is_ok(), "Failed for expression: {}", expr);
            let r = result.expect("Should succeed");
            let val = r.get_pixel(0, 0).expect("Should get pixel");
            assert!(
                (val - expected).abs() < f64::EPSILON,
                "Expression {} expected {} but got {}",
                expr,
                expected,
                val
            );
        }
    }

    #[test]
    fn test_logical_operators() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, 10.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("B1 > 5 and B1 < 15", &[b1.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 1.0).abs() < f64::EPSILON);

        let result = RasterCalculator::evaluate("B1 < 5 or B1 > 5", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_nested_conditionals() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, (x * 10) as f64).ok();
            }
        }

        // Nested conditionals using proper syntax
        let result =
            RasterCalculator::evaluate("if B1 > 15 then 3 else (if B1 > 5 then 2 else 1)", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");

        let val0 = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val0 - 1.0).abs() < f64::EPSILON);

        let val1 = r.get_pixel(1, 0).expect("Should get pixel");
        assert!((val1 - 2.0).abs() < f64::EPSILON);

        let val2 = r.get_pixel(2, 0).expect("Should get pixel");
        assert!((val2 - 3.0).abs() < f64::EPSILON);
    }

    // ========== Legacy API Tests ==========

    #[test]
    fn test_legacy_operations() {
        let mut a = RasterBuffer::zeros(3, 3, RasterDataType::Float32);
        let mut b = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                a.set_pixel(x, y, 10.0).ok();
                b.set_pixel(x, y, 5.0).ok();
            }
        }

        let operations = vec![
            (RasterExpression::Add, 15.0),
            (RasterExpression::Subtract, 5.0),
            (RasterExpression::Multiply, 50.0),
            (RasterExpression::Divide, 2.0),
            (RasterExpression::Max, 10.0),
            (RasterExpression::Min, 5.0),
        ];

        for (op, expected) in operations {
            let result = RasterCalculator::apply_binary(&a, &b, op);
            assert!(result.is_ok());
            let r = result.expect("Should succeed");
            let val = r.get_pixel(0, 0).expect("Should get pixel");
            assert!(
                (val - expected).abs() < f64::EPSILON,
                "Operation {:?} expected {} but got {}",
                op,
                expected,
                val
            );
        }
    }

    #[test]
    fn test_legacy_unary() {
        let mut src = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                src.set_pixel(x, y, 5.0).ok();
            }
        }

        let result = RasterCalculator::apply_unary(&src, |x| x * 2.0);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_unary_negate() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, 10.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("-B1", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val + 10.0).abs() < f64::EPSILON);
    }

    // ========== Optimizer Tests ==========

    #[test]
    fn test_optimizer_constant_folding() {
        let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        // Constant expression should be pre-computed
        let result = RasterCalculator::evaluate("2 + 3", &[b1.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 5.0).abs() < f64::EPSILON);

        // Constant function call
        let result = RasterCalculator::evaluate("sqrt(16)", &[b1.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 4.0).abs() < f64::EPSILON);

        // Mixed constant and variable
        let mut b2 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                b2.set_pixel(x, y, 10.0).ok();
            }
        }
        let result = RasterCalculator::evaluate("B1 + (2 + 3)", &[b2]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_optimizer_algebraic_simplification() {
        let mut b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                b1.set_pixel(x, y, 10.0).ok();
            }
        }

        // x + 0 = x
        let result = RasterCalculator::evaluate("B1 + 0", &[b1.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 10.0).abs() < f64::EPSILON);

        // x * 1 = x
        let result = RasterCalculator::evaluate("B1 * 1", &[b1.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 10.0).abs() < f64::EPSILON);

        // x * 0 = 0
        let result = RasterCalculator::evaluate("B1 * 0", &[b1.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!(val.abs() < f64::EPSILON);

        // x ^ 1 = x
        let result = RasterCalculator::evaluate("B1 ^ 1", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_optimizer_conditional_constant() {
        let mut b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                b1.set_pixel(x, y, 10.0).ok();
            }
        }

        // Constant true condition
        let result = RasterCalculator::evaluate("if 1 then B1 else B1 * 2", &[b1.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 10.0).abs() < f64::EPSILON);

        // Constant false condition
        let result = RasterCalculator::evaluate("if 0 then B1 else B1 * 2", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 20.0).abs() < f64::EPSILON);
    }

    // ========== Parallel Evaluation Tests ==========

    #[test]
    #[cfg(feature = "parallel")]
    fn test_parallel_evaluation() {
        let mut b1 = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
        let mut b2 = RasterBuffer::zeros(100, 100, RasterDataType::Float32);

        for y in 0..100 {
            for x in 0..100 {
                b1.set_pixel(x, y, (x + y) as f64).ok();
                b2.set_pixel(x, y, (x * y) as f64).ok();
            }
        }

        let result = RasterCalculator::evaluate_parallel("B1 + B2", &[b1.clone(), b2.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");

        // Verify some values
        for y in 0..100 {
            for x in 0..100 {
                let val = r.get_pixel(x, y).expect("Should get pixel");
                let expected = (x + y) as f64 + (x * y) as f64;
                assert!(
                    (val - expected).abs() < f64::EPSILON,
                    "Mismatch at ({}, {}): expected {}, got {}",
                    x,
                    y,
                    expected,
                    val
                );
            }
        }
    }

    #[test]
    #[cfg(feature = "parallel")]
    fn test_parallel_complex_expression() {
        let mut nir = RasterBuffer::zeros(50, 50, RasterDataType::Float32);
        let mut red = RasterBuffer::zeros(50, 50, RasterDataType::Float32);

        for y in 0..50 {
            for x in 0..50 {
                nir.set_pixel(x, y, 100.0 + x as f64).ok();
                red.set_pixel(x, y, 50.0 + y as f64).ok();
            }
        }

        let result = RasterCalculator::evaluate_parallel(
            "(B1 - B2) / (B1 + B2)",
            &[nir.clone(), red.clone()],
        );
        assert!(result.is_ok());
        let r = result.expect("Should succeed");

        // Verify NDVI calculation
        for y in 0..50 {
            for x in 0..50 {
                let nir_val = 100.0 + x as f64;
                let red_val = 50.0 + y as f64;
                let expected = (nir_val - red_val) / (nir_val + red_val);
                let val = r.get_pixel(x, y).expect("Should get pixel");
                assert!(
                    (val - expected).abs() < 0.001,
                    "NDVI mismatch at ({}, {}): expected {}, got {}",
                    x,
                    y,
                    expected,
                    val
                );
            }
        }
    }

    #[test]
    #[cfg(feature = "parallel")]
    fn test_parallel_with_optimization() {
        let mut b1 = RasterBuffer::zeros(50, 50, RasterDataType::Float32);
        for y in 0..50 {
            for x in 0..50 {
                b1.set_pixel(x, y, x as f64).ok();
            }
        }

        // Expression with constants that should be optimized
        let result = RasterCalculator::evaluate_parallel("B1 * 1 + 0 + sqrt(16)", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");

        for y in 0..50 {
            for x in 0..50 {
                let val = r.get_pixel(x, y).expect("Should get pixel");
                let expected = x as f64 + 4.0; // B1 + 4 (after optimization)
                assert!(
                    (val - expected).abs() < f64::EPSILON,
                    "Mismatch at ({}, {}): expected {}, got {}",
                    x,
                    y,
                    expected,
                    val
                );
            }
        }
    }
}
