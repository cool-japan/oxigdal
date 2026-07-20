//! Abstract Syntax Tree for Raster Algebra DSL
//!
//! This module defines the AST nodes for the raster algebra DSL, providing
//! a type-safe representation of parsed expressions.

#![allow(missing_docs)]

use serde::{Deserialize, Serialize};

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, string::String, vec::Vec};

/// Type of a value in the DSL
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Type {
    /// Floating point number
    Number,
    /// Boolean value
    Bool,
    /// Raster band
    Raster,
    /// Unknown type (to be inferred)
    Unknown,
}

impl Type {
    /// Checks if two types are compatible
    pub fn is_compatible(&self, other: &Type) -> bool {
        matches!(
            (self, other),
            (Type::Number, Type::Number)
                | (Type::Bool, Type::Bool)
                | (Type::Raster, Type::Raster)
                | (Type::Unknown, _)
                | (_, Type::Unknown)
        )
    }

    /// Gets the common type for binary operations
    pub fn common_type(&self, other: &Type) -> Option<Type> {
        match (self, other) {
            (Type::Number, Type::Number) => Some(Type::Number),
            (Type::Bool, Type::Bool) => Some(Type::Bool),
            (Type::Raster, Type::Raster) => Some(Type::Raster),
            (Type::Raster, Type::Number) | (Type::Number, Type::Raster) => Some(Type::Raster),
            (Type::Unknown, t) | (t, Type::Unknown) => Some(*t),
            _ => None,
        }
    }
}

/// Program AST - top level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    pub statements: Vec<Statement>,
}

/// Statement in the DSL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Statement {
    /// Variable declaration: let x = expr;
    VariableDecl { name: String, value: Box<Expr> },
    /// Function declaration: fn name(params) = expr;
    FunctionDecl {
        name: String,
        params: Vec<String>,
        body: Box<Expr>,
    },
    /// Return statement: return expr;
    Return(Box<Expr>),
    /// Expression statement: expr;
    Expr(Box<Expr>),
}

/// Expression node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expr {
    /// Number literal
    Number(f64),

    /// Band reference (e.g., B1, B2)
    Band(usize),

    /// Variable reference
    Variable(String),

    /// Binary operation
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
        ty: Type,
    },

    /// Unary operation
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        ty: Type,
    },

    /// Function call
    Call {
        name: String,
        args: Vec<Expr>,
        ty: Type,
    },

    /// Conditional expression: if cond then expr1 else expr2
    Conditional {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
        ty: Type,
    },

    /// Block expression: { stmts; expr }
    Block {
        statements: Vec<Statement>,
        result: Option<Box<Expr>>,
        ty: Type,
    },

    /// For loop (for optimization/unrolling)
    ForLoop {
        var: String,
        start: Box<Expr>,
        end: Box<Expr>,
        body: Box<Expr>,
        ty: Type,
    },
}

impl Expr {
    /// Gets the type of this expression
    pub fn get_type(&self) -> Type {
        match self {
            Expr::Number(_) => Type::Number,
            Expr::Band(_) => Type::Raster,
            Expr::Variable(_) => Type::Unknown,
            Expr::Binary { ty, .. }
            | Expr::Unary { ty, .. }
            | Expr::Call { ty, .. }
            | Expr::Conditional { ty, .. }
            | Expr::Block { ty, .. }
            | Expr::ForLoop { ty, .. } => *ty,
        }
    }

    /// Sets the type of this expression
    pub fn set_type(&mut self, new_type: Type) {
        match self {
            Expr::Binary { ty, .. }
            | Expr::Unary { ty, .. }
            | Expr::Call { ty, .. }
            | Expr::Conditional { ty, .. }
            | Expr::Block { ty, .. }
            | Expr::ForLoop { ty, .. } => *ty = new_type,
            _ => {}
        }
    }

    /// Checks if this expression is constant
    pub fn is_constant(&self) -> bool {
        matches!(self, Expr::Number(_))
    }

    /// Checks if this expression is pure (has no side effects)
    pub fn is_pure(&self) -> bool {
        match self {
            Expr::Number(_) | Expr::Band(_) | Expr::Variable(_) => true,
            Expr::Binary { left, right, .. } => left.is_pure() && right.is_pure(),
            Expr::Unary { expr, .. } => expr.is_pure(),
            Expr::Call { args, .. } => args.iter().all(|a| a.is_pure()),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => condition.is_pure() && then_expr.is_pure() && else_expr.is_pure(),
            Expr::Block { .. } | Expr::ForLoop { .. } => false,
        }
    }
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    // Arithmetic
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,

    // Comparison
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,

    // Logical
    And,
    Or,
}

impl BinaryOp {
    /// Gets the precedence of this operator (higher = tighter binding)
    pub fn precedence(&self) -> u8 {
        match self {
            BinaryOp::Or => 1,
            BinaryOp::And => 2,
            BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::Less
            | BinaryOp::LessEqual
            | BinaryOp::Greater
            | BinaryOp::GreaterEqual => 3,
            BinaryOp::Add | BinaryOp::Subtract => 4,
            BinaryOp::Multiply | BinaryOp::Divide | BinaryOp::Modulo => 5,
            BinaryOp::Power => 6,
        }
    }

    /// Checks if this operator is associative
    pub fn is_associative(&self) -> bool {
        matches!(
            self,
            BinaryOp::Add | BinaryOp::Multiply | BinaryOp::And | BinaryOp::Or
        )
    }

    /// Checks if this operator is commutative
    pub fn is_commutative(&self) -> bool {
        matches!(
            self,
            BinaryOp::Add
                | BinaryOp::Multiply
                | BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::And
                | BinaryOp::Or
        )
    }

    /// Gets the result type for this operation
    pub fn result_type(&self, left: Type, right: Type) -> Option<Type> {
        match self {
            BinaryOp::Add
            | BinaryOp::Subtract
            | BinaryOp::Multiply
            | BinaryOp::Divide
            | BinaryOp::Modulo
            | BinaryOp::Power => left.common_type(&right),
            BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::Less
            | BinaryOp::LessEqual
            | BinaryOp::Greater
            | BinaryOp::GreaterEqual => {
                if left.is_compatible(&right) {
                    Some(Type::Bool)
                } else {
                    None
                }
            }
            BinaryOp::And | BinaryOp::Or => {
                if left == Type::Bool && right == Type::Bool {
                    Some(Type::Bool)
                } else {
                    None
                }
            }
        }
    }
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    /// Negation (-)
    Negate,
    /// Logical not (!)
    Not,
    /// Unary plus (+)
    Plus,
}

impl UnaryOp {
    /// Gets the result type for this operation
    pub fn result_type(&self, operand: Type) -> Option<Type> {
        match self {
            UnaryOp::Negate | UnaryOp::Plus => {
                if matches!(operand, Type::Number | Type::Raster) {
                    Some(operand)
                } else {
                    None
                }
            }
            UnaryOp::Not => {
                if operand == Type::Bool {
                    Some(Type::Bool)
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_compatibility() {
        assert!(Type::Number.is_compatible(&Type::Number));
        assert!(Type::Unknown.is_compatible(&Type::Number));
        assert!(!Type::Number.is_compatible(&Type::Bool));
    }

    #[test]
    fn test_common_type() {
        assert_eq!(Type::Number.common_type(&Type::Number), Some(Type::Number));
        assert_eq!(Type::Raster.common_type(&Type::Number), Some(Type::Raster));
        assert_eq!(Type::Number.common_type(&Type::Bool), None);
    }

    #[test]
    fn test_expr_constant() {
        let expr = Expr::Number(42.0);
        assert!(expr.is_constant());

        let expr = Expr::Band(1);
        assert!(!expr.is_constant());
    }

    #[test]
    fn test_binary_op_precedence() {
        assert!(BinaryOp::Multiply.precedence() > BinaryOp::Add.precedence());
        assert!(BinaryOp::Power.precedence() > BinaryOp::Multiply.precedence());
    }

    #[test]
    fn test_binary_op_properties() {
        assert!(BinaryOp::Add.is_commutative());
        assert!(BinaryOp::Add.is_associative());
        assert!(!BinaryOp::Subtract.is_commutative());
        assert!(!BinaryOp::Divide.is_associative());
    }
}
