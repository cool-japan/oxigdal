//! Join executor with proper condition evaluation.

use crate::error::Result;
use crate::executor::scan::RecordBatch;
use crate::parser::ast::{Expr, JoinType};

// Submodules
mod expression;
mod hash_join;
mod nested_loop;
mod value;

// Re-exports
pub use value::JoinValue;

/// Join execution context containing the left and right inputs and join condition.
///
/// This context is used by join executors to perform various join algorithms
/// (nested loop, hash join, sort-merge join) on the input data streams.
pub struct JoinContext<'a> {
    /// Left record batch.
    pub left: &'a RecordBatch,
    /// Right record batch.
    pub right: &'a RecordBatch,
    /// Current left row index.
    pub left_row: usize,
    /// Current right row index.
    pub right_row: usize,
    /// Left table alias (if any).
    pub left_alias: Option<&'a str>,
    /// Right table alias (if any).
    pub right_alias: Option<&'a str>,
}

/// Join operator with proper condition evaluation.
pub struct Join {
    /// Join type.
    pub join_type: JoinType,
    /// Join condition.
    pub on_condition: Option<Expr>,
    /// Left table alias.
    pub left_alias: Option<String>,
    /// Right table alias.
    pub right_alias: Option<String>,
}

impl Join {
    /// Create a new join.
    pub fn new(join_type: JoinType, on_condition: Option<Expr>) -> Self {
        Self {
            join_type,
            on_condition,
            left_alias: None,
            right_alias: None,
        }
    }

    /// Set left table alias.
    pub fn with_left_alias(mut self, alias: impl Into<String>) -> Self {
        self.left_alias = Some(alias.into());
        self
    }

    /// Set right table alias.
    pub fn with_right_alias(mut self, alias: impl Into<String>) -> Self {
        self.right_alias = Some(alias.into());
        self
    }

    /// Execute the join.
    pub fn execute(&self, left: &RecordBatch, right: &RecordBatch) -> Result<RecordBatch> {
        match self.join_type {
            JoinType::Inner => self.inner_join(left, right),
            JoinType::Left => self.left_join(left, right),
            JoinType::Right => self.right_join(left, right),
            JoinType::Full => self.full_join(left, right),
            JoinType::Cross => self.cross_join(left, right),
        }
    }
}

#[cfg(test)]
mod tests;
