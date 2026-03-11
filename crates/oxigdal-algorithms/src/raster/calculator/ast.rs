//! AST types for the raster expression language
//!
//! Defines tokens (used by the lexer), AST nodes (used by the parser),
//! and the operator enums shared between the parser and evaluator.

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

/// Token types for expression lexer
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Token {
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
pub(super) enum Expr {
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
pub(super) enum BinaryOp {
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
pub(super) enum UnaryOp {
    Negate,
}
