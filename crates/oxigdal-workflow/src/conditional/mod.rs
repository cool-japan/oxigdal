//! Conditional execution system for workflows.
//!
//! This module provides conditional branching, switch-case logic, and loop support
//! for dynamic workflow execution.

pub mod branching;
pub mod expressions;

pub use branching::{
    Case, ConditionalBranch, ConditionalEvaluator, ExecutionDecision, LoopCondition, SwitchCase,
};
pub use expressions::{
    BinaryOperator, Expression, ExpressionContext, UnaryOperator, parse_simple_expression,
};
