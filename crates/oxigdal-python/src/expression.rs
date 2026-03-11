//! Raster Band Math Expression Parser and Evaluator
//!
//! This module provides a full expression parser for raster band calculations,
//! supporting arithmetic, trigonometric, comparison, and conditional operations.
//!
//! # Supported Operators
//! - Arithmetic: +, -, *, /, %, ^
//! - Comparison: <, >, <=, >=, ==, !=
//! - Logical: and, or, not (or &&, ||, !)
//!
//! # Supported Functions
//! - Math: sin, cos, tan, sqrt, abs, log, log10, exp, floor, ceil, round
//! - Min/Max: min, max
//! - Clamp: clamp(value, min, max)
//!
//! # Variables
//! - Single letter: A, B, C, ... Z
//! - Indexed: band[0], band[1], ...
//!
//! # Conditionals
//! - Ternary: condition ? then : else
//! - If-else: if condition then value1 else value2

use std::collections::HashMap;
use std::fmt;

/// Error type for expression parsing and evaluation
#[derive(Debug, Clone)]
pub struct ExprError {
    pub message: String,
    pub position: Option<usize>,
}

impl ExprError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            position: None,
        }
    }

    pub fn with_position(mut self, pos: usize) -> Self {
        self.position = Some(pos);
        self
    }
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(pos) = self.position {
            write!(f, "{} at position {}", self.message, pos)
        } else {
            write!(f, "{}", self.message)
        }
    }
}

impl std::error::Error for ExprError {}

pub type ExprResult<T> = Result<T, ExprError>;

// ============================================================================
// Token Types
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Number(f64),
    Variable(String),
    BandIndex(usize),

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,

    // Comparison
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,

    // Logical
    And,
    Or,
    Not,

    // Punctuation
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
    Question,
    Colon,

    // Keywords
    If,
    Then,
    Else,

    // End of input
    Eof,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Number(n) => write!(f, "{}", n),
            Token::Variable(s) => write!(f, "{}", s),
            Token::BandIndex(i) => write!(f, "band[{}]", i),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::Percent => write!(f, "%"),
            Token::Caret => write!(f, "^"),
            Token::Less => write!(f, "<"),
            Token::LessEqual => write!(f, "<="),
            Token::Greater => write!(f, ">"),
            Token::GreaterEqual => write!(f, ">="),
            Token::Equal => write!(f, "=="),
            Token::NotEqual => write!(f, "!="),
            Token::And => write!(f, "and"),
            Token::Or => write!(f, "or"),
            Token::Not => write!(f, "not"),
            Token::LeftParen => write!(f, "("),
            Token::RightParen => write!(f, ")"),
            Token::LeftBracket => write!(f, "["),
            Token::RightBracket => write!(f, "]"),
            Token::Comma => write!(f, ","),
            Token::Question => write!(f, "?"),
            Token::Colon => write!(f, ":"),
            Token::If => write!(f, "if"),
            Token::Then => write!(f, "then"),
            Token::Else => write!(f, "else"),
            Token::Eof => write!(f, "EOF"),
        }
    }
}

// ============================================================================
// Tokenizer
// ============================================================================

pub struct Tokenizer {
    input: Vec<char>,
    pos: usize,
}

impl Tokenizer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
        }
    }

    fn current(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.current();
        self.pos += 1;
        c
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

    fn read_number(&mut self) -> ExprResult<f64> {
        let start = self.pos;
        let mut has_dot = false;
        let mut has_e = false;

        while let Some(c) = self.current() {
            if c.is_ascii_digit() {
                self.advance();
            } else if c == '.' && !has_dot && !has_e {
                has_dot = true;
                self.advance();
            } else if (c == 'e' || c == 'E') && !has_e {
                has_e = true;
                self.advance();
                // Handle optional sign after 'e'
                if let Some(sign) = self.current() {
                    if sign == '+' || sign == '-' {
                        self.advance();
                    }
                }
            } else {
                break;
            }
        }

        let num_str: String = self.input[start..self.pos].iter().collect();
        num_str.parse().map_err(|_| {
            ExprError::new(format!("Invalid number: {}", num_str)).with_position(start)
        })
    }

    fn read_identifier(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.current() {
            if c.is_alphanumeric() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }
        self.input[start..self.pos].iter().collect()
    }

    pub fn tokenize(&mut self) -> ExprResult<Vec<Token>> {
        let mut tokens = Vec::new();

        loop {
            self.skip_whitespace();

            let start_pos = self.pos;
            let c = match self.current() {
                Some(c) => c,
                None => {
                    tokens.push(Token::Eof);
                    break;
                }
            };

            let token = match c {
                // Numbers
                '0'..='9' => Token::Number(self.read_number()?),

                // Identifiers and keywords
                'a'..='z' | 'A'..='Z' | '_' => {
                    let ident = self.read_identifier();

                    // Check for band[index] syntax
                    if ident.to_lowercase() == "band" {
                        self.skip_whitespace();
                        if self.current() == Some('[') {
                            self.advance(); // consume '['
                            self.skip_whitespace();
                            let idx = self.read_number()? as usize;
                            self.skip_whitespace();
                            if self.current() != Some(']') {
                                return Err(ExprError::new("Expected ']' after band index")
                                    .with_position(self.pos));
                            }
                            self.advance(); // consume ']'
                            Token::BandIndex(idx)
                        } else {
                            Token::Variable(ident)
                        }
                    } else {
                        // Keywords
                        match ident.to_lowercase().as_str() {
                            "if" => Token::If,
                            "then" => Token::Then,
                            "else" => Token::Else,
                            "and" => Token::And,
                            "or" => Token::Or,
                            "not" => Token::Not,
                            _ => Token::Variable(ident),
                        }
                    }
                }

                // Operators
                '+' => {
                    self.advance();
                    Token::Plus
                }
                '-' => {
                    self.advance();
                    Token::Minus
                }
                '*' => {
                    self.advance();
                    Token::Star
                }
                '/' => {
                    self.advance();
                    Token::Slash
                }
                '%' => {
                    self.advance();
                    Token::Percent
                }
                '^' => {
                    self.advance();
                    Token::Caret
                }

                // Comparison operators
                '<' => {
                    self.advance();
                    if self.current() == Some('=') {
                        self.advance();
                        Token::LessEqual
                    } else {
                        Token::Less
                    }
                }
                '>' => {
                    self.advance();
                    if self.current() == Some('=') {
                        self.advance();
                        Token::GreaterEqual
                    } else {
                        Token::Greater
                    }
                }
                '=' => {
                    self.advance();
                    if self.current() == Some('=') {
                        self.advance();
                        Token::Equal
                    } else {
                        return Err(ExprError::new("Expected '==' for equality comparison")
                            .with_position(start_pos));
                    }
                }
                '!' => {
                    self.advance();
                    if self.current() == Some('=') {
                        self.advance();
                        Token::NotEqual
                    } else {
                        Token::Not
                    }
                }

                // Logical operators (alternative syntax)
                '&' => {
                    self.advance();
                    if self.current() == Some('&') {
                        self.advance();
                        Token::And
                    } else {
                        return Err(ExprError::new("Expected '&&' for logical and")
                            .with_position(start_pos));
                    }
                }
                '|' => {
                    self.advance();
                    if self.current() == Some('|') {
                        self.advance();
                        Token::Or
                    } else {
                        return Err(
                            ExprError::new("Expected '||' for logical or").with_position(start_pos)
                        );
                    }
                }

                // Punctuation
                '(' => {
                    self.advance();
                    Token::LeftParen
                }
                ')' => {
                    self.advance();
                    Token::RightParen
                }
                '[' => {
                    self.advance();
                    Token::LeftBracket
                }
                ']' => {
                    self.advance();
                    Token::RightBracket
                }
                ',' => {
                    self.advance();
                    Token::Comma
                }
                '?' => {
                    self.advance();
                    Token::Question
                }
                ':' => {
                    self.advance();
                    Token::Colon
                }

                _ => {
                    return Err(ExprError::new(format!("Unexpected character: '{}'", c))
                        .with_position(start_pos));
                }
            };

            tokens.push(token);
        }

        Ok(tokens)
    }
}

// ============================================================================
// AST (Abstract Syntax Tree)
// ============================================================================

#[derive(Debug, Clone)]
pub enum Expr {
    /// Numeric literal
    Number(f64),

    /// Variable reference (A, B, C, etc.)
    Variable(String),

    /// Band index reference (band[0], band[1], etc.)
    BandIndex(usize),

    /// Binary operation
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },

    /// Unary operation
    Unary { op: UnaryOp, expr: Box<Expr> },

    /// Function call
    Call { name: String, args: Vec<Expr> },

    /// Ternary conditional: condition ? then : else
    Ternary {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },

    /// If-then-else: if condition then expr1 else expr2
    IfElse {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,

    // Comparison
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,

    // Logical
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    Pos,
}

// ============================================================================
// Parser
// ============================================================================

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn current(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let token = self.current().clone();
        self.pos += 1;
        token
    }

    fn expect(&mut self, expected: &Token) -> ExprResult<Token> {
        let current = self.current().clone();
        if std::mem::discriminant(&current) == std::mem::discriminant(expected) {
            Ok(self.advance())
        } else {
            Err(ExprError::new(format!(
                "Expected '{}', found '{}'",
                expected, current
            )))
        }
    }

    pub fn parse(&mut self) -> ExprResult<Expr> {
        let expr = self.parse_expression()?;
        if !matches!(self.current(), Token::Eof) {
            return Err(ExprError::new(format!(
                "Unexpected token after expression: {}",
                self.current()
            )));
        }
        Ok(expr)
    }

    fn parse_expression(&mut self) -> ExprResult<Expr> {
        self.parse_ternary()
    }

    fn parse_ternary(&mut self) -> ExprResult<Expr> {
        // Check for if-then-else
        if matches!(self.current(), Token::If) {
            return self.parse_if_else();
        }

        let expr = self.parse_or()?;

        // Check for ternary operator: condition ? then : else
        if matches!(self.current(), Token::Question) {
            self.advance(); // consume '?'
            let then_expr = self.parse_expression()?;
            self.expect(&Token::Colon)?;
            let else_expr = self.parse_expression()?;
            return Ok(Expr::Ternary {
                condition: Box::new(expr),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            });
        }

        Ok(expr)
    }

    fn parse_if_else(&mut self) -> ExprResult<Expr> {
        self.expect(&Token::If)?;
        let condition = self.parse_or()?;
        self.expect(&Token::Then)?;
        let then_expr = self.parse_expression()?;
        self.expect(&Token::Else)?;
        let else_expr = self.parse_expression()?;

        Ok(Expr::IfElse {
            condition: Box::new(condition),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
        })
    }

    fn parse_or(&mut self) -> ExprResult<Expr> {
        let mut left = self.parse_and()?;

        while matches!(self.current(), Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Binary {
                left: Box::new(left),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_and(&mut self) -> ExprResult<Expr> {
        let mut left = self.parse_equality()?;

        while matches!(self.current(), Token::And) {
            self.advance();
            let right = self.parse_equality()?;
            left = Expr::Binary {
                left: Box::new(left),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_equality(&mut self) -> ExprResult<Expr> {
        let mut left = self.parse_comparison()?;

        loop {
            let op = match self.current() {
                Token::Equal => BinaryOp::Eq,
                Token::NotEqual => BinaryOp::Ne,
                _ => break,
            };
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_comparison(&mut self) -> ExprResult<Expr> {
        let mut left = self.parse_additive()?;

        loop {
            let op = match self.current() {
                Token::Less => BinaryOp::Lt,
                Token::LessEqual => BinaryOp::Le,
                Token::Greater => BinaryOp::Gt,
                Token::GreaterEqual => BinaryOp::Ge,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_additive(&mut self) -> ExprResult<Expr> {
        let mut left = self.parse_multiplicative()?;

        loop {
            let op = match self.current() {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> ExprResult<Expr> {
        let mut left = self.parse_power()?;

        loop {
            let op = match self.current() {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_power()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_power(&mut self) -> ExprResult<Expr> {
        let left = self.parse_unary()?;

        if matches!(self.current(), Token::Caret) {
            self.advance();
            // Power is right-associative
            let right = self.parse_power()?;
            return Ok(Expr::Binary {
                left: Box::new(left),
                op: BinaryOp::Pow,
                right: Box::new(right),
            });
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> ExprResult<Expr> {
        match self.current() {
            Token::Minus => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                })
            }
            Token::Plus => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnaryOp::Pos,
                    expr: Box::new(expr),
                })
            }
            Token::Not => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                })
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> ExprResult<Expr> {
        match self.current().clone() {
            Token::Number(n) => {
                self.advance();
                Ok(Expr::Number(n))
            }
            Token::Variable(name) => {
                self.advance();
                // Check if this is a function call
                if matches!(self.current(), Token::LeftParen) {
                    self.advance(); // consume '('
                    let args = self.parse_arguments()?;
                    self.expect(&Token::RightParen)?;
                    Ok(Expr::Call { name, args })
                } else {
                    Ok(Expr::Variable(name))
                }
            }
            Token::BandIndex(idx) => {
                self.advance();
                Ok(Expr::BandIndex(idx))
            }
            Token::LeftParen => {
                self.advance();
                let expr = self.parse_expression()?;
                self.expect(&Token::RightParen)?;
                Ok(expr)
            }
            token => Err(ExprError::new(format!("Unexpected token: {}", token))),
        }
    }

    fn parse_arguments(&mut self) -> ExprResult<Vec<Expr>> {
        let mut args = Vec::new();

        if matches!(self.current(), Token::RightParen) {
            return Ok(args);
        }

        args.push(self.parse_expression()?);

        while matches!(self.current(), Token::Comma) {
            self.advance();
            args.push(self.parse_expression()?);
        }

        Ok(args)
    }
}

// ============================================================================
// Expression Parser (convenience function)
// ============================================================================

/// Parses an expression string into an AST
pub fn parse_expression(input: &str) -> ExprResult<Expr> {
    let mut tokenizer = Tokenizer::new(input);
    let tokens = tokenizer.tokenize()?;
    let mut parser = Parser::new(tokens);
    parser.parse()
}

// ============================================================================
// Evaluator
// ============================================================================

/// Evaluator for raster band expressions
pub struct Evaluator {
    /// Map of variable names to their indices in the input arrays
    variable_map: HashMap<String, usize>,
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl Evaluator {
    pub fn new() -> Self {
        Self {
            variable_map: HashMap::new(),
        }
    }

    /// Sets up the variable map from the provided variable names
    pub fn with_variables(names: &[String]) -> Self {
        let mut variable_map = HashMap::new();
        for (idx, name) in names.iter().enumerate() {
            variable_map.insert(name.clone(), idx);
        }
        Self { variable_map }
    }

    /// Evaluates an expression on array data
    ///
    /// # Arguments
    /// * `expr` - The parsed expression AST
    /// * `arrays` - Slice of input arrays (each array is a flattened 2D array)
    /// * `width` - Width of the arrays
    /// * `height` - Height of the arrays
    ///
    /// # Returns
    /// Result containing the output array
    pub fn evaluate(
        &self,
        expr: &Expr,
        arrays: &[&[f64]],
        width: usize,
        height: usize,
    ) -> ExprResult<Vec<f64>> {
        let size = width * height;

        // Validate array sizes
        for (i, arr) in arrays.iter().enumerate() {
            if arr.len() != size {
                return Err(ExprError::new(format!(
                    "Array {} has size {}, expected {}",
                    i,
                    arr.len(),
                    size
                )));
            }
        }

        (0..size)
            .map(|i| self.evaluate_at(expr, arrays, i))
            .collect()
    }

    /// Evaluates the expression at a single pixel position
    fn evaluate_at(&self, expr: &Expr, arrays: &[&[f64]], idx: usize) -> ExprResult<f64> {
        match expr {
            Expr::Number(n) => Ok(*n),

            Expr::Variable(name) => {
                // Try to find the variable in the map
                if let Some(&arr_idx) = self.variable_map.get(name) {
                    if arr_idx < arrays.len() {
                        return Ok(arrays[arr_idx][idx]);
                    }
                }

                // Try single letter variable (A=0, B=1, etc.)
                if name.len() == 1 {
                    let c = name.chars().next().unwrap_or(' ');
                    let arr_idx = if c.is_ascii_uppercase() {
                        (c as usize) - ('A' as usize)
                    } else if c.is_ascii_lowercase() {
                        (c as usize) - ('a' as usize)
                    } else {
                        return Err(ExprError::new(format!("Unknown variable: {}", name)));
                    };

                    if arr_idx < arrays.len() {
                        return Ok(arrays[arr_idx][idx]);
                    }
                }

                Err(ExprError::new(format!("Unknown variable: {}", name)))
            }

            Expr::BandIndex(band_idx) => {
                if *band_idx < arrays.len() {
                    Ok(arrays[*band_idx][idx])
                } else {
                    Err(ExprError::new(format!(
                        "Band index {} out of range (0..{})",
                        band_idx,
                        arrays.len()
                    )))
                }
            }

            Expr::Binary { left, op, right } => {
                let left_val = self.evaluate_at(left, arrays, idx)?;
                let right_val = self.evaluate_at(right, arrays, idx)?;
                self.apply_binary_op(*op, left_val, right_val)
            }

            Expr::Unary { op, expr } => {
                let val = self.evaluate_at(expr, arrays, idx)?;
                self.apply_unary_op(*op, val)
            }

            Expr::Call { name, args } => {
                let arg_vals: ExprResult<Vec<f64>> = args
                    .iter()
                    .map(|arg| self.evaluate_at(arg, arrays, idx))
                    .collect();
                self.apply_function(name, &arg_vals?)
            }

            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
            }
            | Expr::IfElse {
                condition,
                then_expr,
                else_expr,
            } => {
                let cond_val = self.evaluate_at(condition, arrays, idx)?;
                if cond_val.abs() > f64::EPSILON {
                    self.evaluate_at(then_expr, arrays, idx)
                } else {
                    self.evaluate_at(else_expr, arrays, idx)
                }
            }
        }
    }

    fn apply_binary_op(&self, op: BinaryOp, left: f64, right: f64) -> ExprResult<f64> {
        let result = match op {
            BinaryOp::Add => left + right,
            BinaryOp::Sub => left - right,
            BinaryOp::Mul => left * right,
            BinaryOp::Div => {
                if right.abs() < f64::EPSILON {
                    f64::NAN
                } else {
                    left / right
                }
            }
            BinaryOp::Mod => left % right,
            BinaryOp::Pow => left.powf(right),
            BinaryOp::Lt => {
                if left < right {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::Le => {
                if left <= right {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::Gt => {
                if left > right {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::Ge => {
                if left >= right {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::Eq => {
                if (left - right).abs() < f64::EPSILON {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::Ne => {
                if (left - right).abs() >= f64::EPSILON {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::And => {
                if left.abs() > f64::EPSILON && right.abs() > f64::EPSILON {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::Or => {
                if left.abs() > f64::EPSILON || right.abs() > f64::EPSILON {
                    1.0
                } else {
                    0.0
                }
            }
        };
        Ok(result)
    }

    fn apply_unary_op(&self, op: UnaryOp, val: f64) -> ExprResult<f64> {
        let result = match op {
            UnaryOp::Neg => -val,
            UnaryOp::Pos => val,
            UnaryOp::Not => {
                if val.abs() < f64::EPSILON {
                    1.0
                } else {
                    0.0
                }
            }
        };
        Ok(result)
    }

    fn apply_function(&self, name: &str, args: &[f64]) -> ExprResult<f64> {
        let name_lower = name.to_lowercase();
        match name_lower.as_str() {
            // Single-argument math functions
            "sin" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].sin())
            }
            "cos" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].cos())
            }
            "tan" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].tan())
            }
            "asin" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].asin())
            }
            "acos" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].acos())
            }
            "atan" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].atan())
            }
            "sinh" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].sinh())
            }
            "cosh" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].cosh())
            }
            "tanh" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].tanh())
            }
            "sqrt" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].sqrt())
            }
            "abs" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].abs())
            }
            "log" | "ln" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].ln())
            }
            "log10" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].log10())
            }
            "log2" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].log2())
            }
            "exp" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].exp())
            }
            "floor" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].floor())
            }
            "ceil" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].ceil())
            }
            "round" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].round())
            }
            "trunc" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].trunc())
            }
            "sign" | "signum" => {
                self.check_arity(name, args, 1)?;
                Ok(args[0].signum())
            }

            // Two-argument functions
            "atan2" => {
                self.check_arity(name, args, 2)?;
                Ok(args[0].atan2(args[1]))
            }
            "pow" => {
                self.check_arity(name, args, 2)?;
                Ok(args[0].powf(args[1]))
            }
            "hypot" => {
                self.check_arity(name, args, 2)?;
                Ok(args[0].hypot(args[1]))
            }
            "min" => {
                if args.is_empty() {
                    return Err(ExprError::new("min() requires at least 1 argument"));
                }
                Ok(args.iter().cloned().fold(f64::INFINITY, f64::min))
            }
            "max" => {
                if args.is_empty() {
                    return Err(ExprError::new("max() requires at least 1 argument"));
                }
                Ok(args.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
            }

            // Three-argument functions
            "clamp" => {
                self.check_arity(name, args, 3)?;
                Ok(args[0].clamp(args[1], args[2]))
            }
            "select" => {
                self.check_arity(name, args, 3)?;
                Ok(if args[0].abs() > f64::EPSILON {
                    args[1]
                } else {
                    args[2]
                })
            }
            "lerp" | "mix" => {
                self.check_arity(name, args, 3)?;
                // lerp(a, b, t) = a + (b - a) * t
                Ok(args[0] + (args[1] - args[0]) * args[2])
            }

            // Constants
            "pi" => {
                self.check_arity(name, args, 0)?;
                Ok(std::f64::consts::PI)
            }
            "e" => {
                self.check_arity(name, args, 0)?;
                Ok(std::f64::consts::E)
            }

            // Special functions
            "nan" => {
                self.check_arity(name, args, 0)?;
                Ok(f64::NAN)
            }
            "inf" => {
                self.check_arity(name, args, 0)?;
                Ok(f64::INFINITY)
            }
            "isnan" => {
                self.check_arity(name, args, 1)?;
                Ok(if args[0].is_nan() { 1.0 } else { 0.0 })
            }
            "isinf" => {
                self.check_arity(name, args, 1)?;
                Ok(if args[0].is_infinite() { 1.0 } else { 0.0 })
            }
            "isfinite" => {
                self.check_arity(name, args, 1)?;
                Ok(if args[0].is_finite() { 1.0 } else { 0.0 })
            }

            _ => Err(ExprError::new(format!("Unknown function: {}", name))),
        }
    }

    fn check_arity(&self, name: &str, args: &[f64], expected: usize) -> ExprResult<()> {
        if args.len() != expected {
            Err(ExprError::new(format!(
                "Function '{}' expects {} argument(s), got {}",
                name,
                expected,
                args.len()
            )))
        } else {
            Ok(())
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_simple() {
        let mut tokenizer = Tokenizer::new("A + B * 2");
        let tokens = tokenizer.tokenize().expect("Should tokenize");

        assert_eq!(tokens.len(), 6); // A, +, B, *, 2, EOF
        assert!(matches!(tokens[0], Token::Variable(_)));
        assert!(matches!(tokens[1], Token::Plus));
        assert!(matches!(tokens[2], Token::Variable(_)));
        assert!(matches!(tokens[3], Token::Star));
        assert!(matches!(tokens[4], Token::Number(_)));
        assert!(matches!(tokens[5], Token::Eof));
    }

    #[test]
    fn test_tokenize_comparison() {
        let mut tokenizer = Tokenizer::new("A > 0 && B <= 10");
        let tokens = tokenizer.tokenize().expect("Should tokenize");
        // Tokens (whitespace skipped): [A, >, 0, &&, B, <=, 10, EOF]
        assert!(matches!(tokens[1], Token::Greater));
        assert!(matches!(tokens[3], Token::And));
        assert!(matches!(tokens[5], Token::LessEqual));
    }

    #[test]
    fn test_tokenize_band_index() {
        let mut tokenizer = Tokenizer::new("band[0] + band[1]");
        let tokens = tokenizer.tokenize().expect("Should tokenize");

        assert!(matches!(tokens[0], Token::BandIndex(0)));
        assert!(matches!(tokens[2], Token::BandIndex(1)));
    }

    #[test]
    fn test_parse_simple_add() {
        let expr = parse_expression("A + B").expect("Should parse");
        assert!(matches!(
            expr,
            Expr::Binary {
                op: BinaryOp::Add,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_precedence() {
        // A + B * C should parse as A + (B * C)
        let expr = parse_expression("A + B * C").expect("Should parse");

        if let Expr::Binary { op, right, .. } = expr {
            assert_eq!(op, BinaryOp::Add);
            assert!(matches!(
                *right,
                Expr::Binary {
                    op: BinaryOp::Mul,
                    ..
                }
            ));
        } else {
            panic!("Expected binary expression");
        }
    }

    #[test]
    fn test_parse_parentheses() {
        // (A + B) * C should parse as (A + B) * C
        let expr = parse_expression("(A + B) * C").expect("Should parse");

        if let Expr::Binary { op, left, .. } = expr {
            assert_eq!(op, BinaryOp::Mul);
            assert!(matches!(
                *left,
                Expr::Binary {
                    op: BinaryOp::Add,
                    ..
                }
            ));
        } else {
            panic!("Expected binary expression");
        }
    }

    #[test]
    fn test_parse_function_call() {
        let expr = parse_expression("sqrt(A)").expect("Should parse");

        if let Expr::Call { name, args } = expr {
            assert_eq!(name, "sqrt");
            assert_eq!(args.len(), 1);
        } else {
            panic!("Expected function call");
        }
    }

    #[test]
    fn test_parse_ternary() {
        let expr = parse_expression("A > 0 ? A : 0").expect("Should parse");
        assert!(matches!(expr, Expr::Ternary { .. }));
    }

    #[test]
    fn test_parse_if_else() {
        let expr = parse_expression("if A > 0 then A else 0").expect("Should parse");
        assert!(matches!(expr, Expr::IfElse { .. }));
    }

    #[test]
    fn test_parse_ndvi() {
        let expr = parse_expression("(A - B) / (A + B)").expect("Should parse");
        assert!(matches!(
            expr,
            Expr::Binary {
                op: BinaryOp::Div,
                ..
            }
        ));
    }

    #[test]
    fn test_evaluate_simple_add() {
        let expr = parse_expression("A + B").expect("Should parse");
        let evaluator = Evaluator::new();

        let a = [1.0, 2.0, 3.0, 4.0];
        let b = [5.0, 6.0, 7.0, 8.0];
        let arrays: Vec<&[f64]> = vec![&a, &b];

        let result = evaluator
            .evaluate(&expr, &arrays, 2, 2)
            .expect("Should evaluate");

        assert_eq!(result, vec![6.0, 8.0, 10.0, 12.0]);
    }

    #[test]
    fn test_evaluate_ndvi() {
        let expr = parse_expression("(A - B) / (A + B)").expect("Should parse");
        let evaluator = Evaluator::new();

        let nir = [0.8, 0.6, 0.4, 0.2];
        let red = [0.2, 0.2, 0.2, 0.2];
        let arrays: Vec<&[f64]> = vec![&nir, &red];

        let result = evaluator
            .evaluate(&expr, &arrays, 2, 2)
            .expect("Should evaluate");

        // NDVI = (NIR - RED) / (NIR + RED)
        // [0] = (0.8 - 0.2) / (0.8 + 0.2) = 0.6 / 1.0 = 0.6
        // [1] = (0.6 - 0.2) / (0.6 + 0.2) = 0.4 / 0.8 = 0.5
        // [2] = (0.4 - 0.2) / (0.4 + 0.2) = 0.2 / 0.6 = 0.333...
        // [3] = (0.2 - 0.2) / (0.2 + 0.2) = 0.0 / 0.4 = 0.0
        assert!((result[0] - 0.6).abs() < 1e-10);
        assert!((result[1] - 0.5).abs() < 1e-10);
        assert!((result[2] - (1.0 / 3.0)).abs() < 1e-10);
        assert!((result[3] - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_evaluate_conditional() {
        let expr = parse_expression("if A > 0.5 then 1 else 0").expect("Should parse");
        let evaluator = Evaluator::new();

        let a = [0.8, 0.3, 0.6, 0.4];
        let arrays: Vec<&[f64]> = vec![&a];

        let result = evaluator
            .evaluate(&expr, &arrays, 2, 2)
            .expect("Should evaluate");

        assert_eq!(result, vec![1.0, 0.0, 1.0, 0.0]);
    }

    #[test]
    fn test_evaluate_function_sqrt() {
        let expr = parse_expression("sqrt(A)").expect("Should parse");
        let evaluator = Evaluator::new();

        let a = [4.0, 9.0, 16.0, 25.0];
        let arrays: Vec<&[f64]> = vec![&a];

        let result = evaluator
            .evaluate(&expr, &arrays, 2, 2)
            .expect("Should evaluate");

        assert_eq!(result, vec![2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn test_evaluate_clamp() {
        let expr = parse_expression("clamp(A, 0, 1)").expect("Should parse");
        let evaluator = Evaluator::new();

        let a = [-0.5, 0.5, 1.5, 0.8];
        let arrays: Vec<&[f64]> = vec![&a];

        let result = evaluator
            .evaluate(&expr, &arrays, 2, 2)
            .expect("Should evaluate");

        assert_eq!(result, vec![0.0, 0.5, 1.0, 0.8]);
    }

    #[test]
    fn test_evaluate_band_index() {
        let expr = parse_expression("band[0] + band[1]").expect("Should parse");
        let evaluator = Evaluator::new();

        let a = [1.0, 2.0];
        let b = [3.0, 4.0];
        let arrays: Vec<&[f64]> = vec![&a, &b];

        let result = evaluator
            .evaluate(&expr, &arrays, 1, 2)
            .expect("Should evaluate");

        assert_eq!(result, vec![4.0, 6.0]);
    }

    #[test]
    fn test_evaluate_power() {
        let expr = parse_expression("A ^ 2").expect("Should parse");
        let evaluator = Evaluator::new();

        let a = [2.0, 3.0, 4.0, 5.0];
        let arrays: Vec<&[f64]> = vec![&a];

        let result = evaluator
            .evaluate(&expr, &arrays, 2, 2)
            .expect("Should evaluate");

        assert_eq!(result, vec![4.0, 9.0, 16.0, 25.0]);
    }

    #[test]
    fn test_evaluate_complex_expression() {
        // Calculate EVI-like index: 2.5 * (NIR - RED) / (NIR + 6*RED - 7.5*BLUE + 1)
        let expr = parse_expression("2.5 * (A - B) / (A + 6*B - 7.5*C + 1)").expect("Should parse");
        let evaluator = Evaluator::new();

        let nir = [0.5];
        let red = [0.1];
        let blue = [0.05];
        let arrays: Vec<&[f64]> = vec![&nir, &red, &blue];

        let result = evaluator
            .evaluate(&expr, &arrays, 1, 1)
            .expect("Should evaluate");

        // EVI = 2.5 * (0.5 - 0.1) / (0.5 + 6*0.1 - 7.5*0.05 + 1)
        //     = 2.5 * 0.4 / (0.5 + 0.6 - 0.375 + 1)
        //     = 1.0 / 1.725 = 0.5797...
        assert!((result[0] - 0.5797101449275362).abs() < 1e-10);
    }

    #[test]
    fn test_evaluate_min_max() {
        let expr = parse_expression("max(A, B, C)").expect("Should parse");
        let evaluator = Evaluator::new();

        let a = [1.0, 5.0];
        let b = [3.0, 2.0];
        let c = [2.0, 4.0];
        let arrays: Vec<&[f64]> = vec![&a, &b, &c];

        let result = evaluator
            .evaluate(&expr, &arrays, 1, 2)
            .expect("Should evaluate");

        assert_eq!(result, vec![3.0, 5.0]);
    }

    #[test]
    fn test_named_variables() {
        let expr = parse_expression("NIR - RED").expect("Should parse");
        let evaluator = Evaluator::with_variables(&["NIR".to_string(), "RED".to_string()]);

        let nir = [0.8, 0.6];
        let red = [0.2, 0.3];
        let arrays: Vec<&[f64]> = vec![&nir, &red];

        let result = evaluator
            .evaluate(&expr, &arrays, 1, 2)
            .expect("Should evaluate");

        // Use approximate comparison to handle floating-point rounding
        // (e.g. 0.8 - 0.2 = 0.6000000000000001 in IEEE 754)
        assert_eq!(result.len(), 2);
        assert!((result[0] - 0.6).abs() < 1e-10, "result[0] = {}", result[0]);
        assert!((result[1] - 0.3).abs() < 1e-10, "result[1] = {}", result[1]);
    }

    #[test]
    fn test_logical_operations() {
        let expr = parse_expression("A > 0.5 and B > 0.5").expect("Should parse");
        let evaluator = Evaluator::new();

        let a = [0.8, 0.3, 0.6, 0.4];
        let b = [0.6, 0.8, 0.3, 0.7];
        let arrays: Vec<&[f64]> = vec![&a, &b];

        let result = evaluator
            .evaluate(&expr, &arrays, 2, 2)
            .expect("Should evaluate");

        // A > 0.5 and B > 0.5
        // [0]: 0.8 > 0.5 (T) and 0.6 > 0.5 (T) = 1.0
        // [1]: 0.3 > 0.5 (F) and 0.8 > 0.5 (T) = 0.0
        // [2]: 0.6 > 0.5 (T) and 0.3 > 0.5 (F) = 0.0
        // [3]: 0.4 > 0.5 (F) and 0.7 > 0.5 (T) = 0.0
        assert_eq!(result, vec![1.0, 0.0, 0.0, 0.0]);
    }
}
