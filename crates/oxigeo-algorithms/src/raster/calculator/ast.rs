//! AST types for the raster expression language
//!
//! Defines tokens (used by the lexer), AST nodes (used by the parser),
//! and the operator enums shared between the parser and evaluator.

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

use std::hash::{Hash, Hasher};

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
///
/// `PartialEq`, `Eq`, and `Hash` are implemented manually so that `Number(f64)` variants
/// are compared and hashed via their bit-level representation (`f64::to_bits`), which
/// provides a total order and makes `Expr` suitable as a `HashMap` key required by the
/// common subexpression elimination pass.
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
    /// Cache slot reference — inserted by the CSE optimizer.
    ///
    /// During evaluation the slot index is used to look up (and lazily populate) a
    /// per-pixel memoisation table held in `Evaluator`, so each common sub-expression
    /// is computed at most once per pixel.
    CacheRef(u32),
}

impl PartialEq for Expr {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Expr::Number(a), Expr::Number(b)) => a.to_bits() == b.to_bits(),
            (Expr::Band(a), Expr::Band(b)) => a == b,
            (
                Expr::BinaryOp {
                    left: ll,
                    op: lo,
                    right: lr,
                },
                Expr::BinaryOp {
                    left: rl,
                    op: ro,
                    right: rr,
                },
            ) => lo == ro && ll == rl && lr == rr,
            (Expr::UnaryOp { op: lo, expr: le }, Expr::UnaryOp { op: ro, expr: re }) => {
                lo == ro && le == re
            }
            (Expr::Function { name: ln, args: la }, Expr::Function { name: rn, args: ra }) => {
                ln == rn && la == ra
            }
            (
                Expr::Conditional {
                    condition: lc,
                    then_expr: lt,
                    else_expr: le,
                },
                Expr::Conditional {
                    condition: rc,
                    then_expr: rt,
                    else_expr: re,
                },
            ) => lc == rc && lt == rt && le == re,
            (Expr::CacheRef(a), Expr::CacheRef(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for Expr {}

impl Hash for Expr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Discriminant tag — keep in sync with enum variant ordering.
        match self {
            Expr::Number(n) => {
                0u8.hash(state);
                n.to_bits().hash(state);
            }
            Expr::Band(b) => {
                1u8.hash(state);
                b.hash(state);
            }
            Expr::BinaryOp { left, op, right } => {
                2u8.hash(state);
                op.hash(state);
                left.hash(state);
                right.hash(state);
            }
            Expr::UnaryOp { op, expr } => {
                3u8.hash(state);
                op.hash(state);
                expr.hash(state);
            }
            Expr::Function { name, args } => {
                4u8.hash(state);
                name.hash(state);
                args.hash(state);
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                5u8.hash(state);
                condition.hash(state);
                then_expr.hash(state);
                else_expr.hash(state);
            }
            Expr::CacheRef(i) => {
                6u8.hash(state);
                i.hash(state);
            }
        }
    }
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum UnaryOp {
    Negate,
}
