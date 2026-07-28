//! Typed column builders for join output.
//!
//! Join results historically collapsed every column to
//! [`ColumnData::String`], discarding the native `Int64`/`Float64`/`Boolean`/…
//! types while the output schema still claimed the original types. Any
//! `WHERE`/aggregate/sort applied to a numeric or boolean column produced by a
//! join then failed with a spurious type mismatch. [`ColumnBuilder`] fixes this
//! by accumulating each output column in a builder whose variant matches the
//! source column, so the finished [`ColumnData`] preserves the declared type.

use crate::error::{QueryError, Result};
use crate::executor::scan::ColumnData;
use bytes::Bytes;

/// Accumulates values for one output column, preserving the native
/// [`ColumnData`] variant of the source column it was created from.
pub(super) enum ColumnBuilder {
    /// Boolean output column.
    Boolean(Vec<Option<bool>>),
    /// 32-bit integer output column.
    Int32(Vec<Option<i32>>),
    /// 64-bit integer output column.
    Int64(Vec<Option<i64>>),
    /// 32-bit float output column.
    Float32(Vec<Option<f32>>),
    /// 64-bit float output column.
    Float64(Vec<Option<f64>>),
    /// String output column.
    String(Vec<Option<String>>),
    /// Binary output column.
    Binary(Vec<Option<Bytes>>),
}

impl ColumnBuilder {
    /// Create an empty builder whose variant matches `source`.
    pub(super) fn for_column(source: &ColumnData) -> Self {
        match source {
            ColumnData::Boolean(_) => ColumnBuilder::Boolean(Vec::new()),
            ColumnData::Int32(_) => ColumnBuilder::Int32(Vec::new()),
            ColumnData::Int64(_) => ColumnBuilder::Int64(Vec::new()),
            ColumnData::Float32(_) => ColumnBuilder::Float32(Vec::new()),
            ColumnData::Float64(_) => ColumnBuilder::Float64(Vec::new()),
            ColumnData::String(_) => ColumnBuilder::String(Vec::new()),
            ColumnData::Binary(_) => ColumnBuilder::Binary(Vec::new()),
        }
    }

    /// Append the value at `row` of `source` into this builder, preserving type.
    ///
    /// `source` must be the same [`ColumnData`] variant this builder was created
    /// from (guaranteed by construction in the join executors); a mismatch is
    /// reported as an internal error rather than silently coerced.
    pub(super) fn push_from(&mut self, source: &ColumnData, row: usize) -> Result<()> {
        match (self, source) {
            (ColumnBuilder::Boolean(dst), ColumnData::Boolean(src)) => {
                dst.push(src.get(row).copied().flatten());
            }
            (ColumnBuilder::Int32(dst), ColumnData::Int32(src)) => {
                dst.push(src.get(row).copied().flatten());
            }
            (ColumnBuilder::Int64(dst), ColumnData::Int64(src)) => {
                dst.push(src.get(row).copied().flatten());
            }
            (ColumnBuilder::Float32(dst), ColumnData::Float32(src)) => {
                dst.push(src.get(row).copied().flatten());
            }
            (ColumnBuilder::Float64(dst), ColumnData::Float64(src)) => {
                dst.push(src.get(row).copied().flatten());
            }
            (ColumnBuilder::String(dst), ColumnData::String(src)) => {
                dst.push(src.get(row).cloned().flatten());
            }
            (ColumnBuilder::Binary(dst), ColumnData::Binary(src)) => {
                dst.push(src.get(row).cloned().flatten());
            }
            _ => {
                return Err(QueryError::internal(
                    "join column builder variant does not match source column",
                ));
            }
        }
        Ok(())
    }

    /// Append a NULL, used for the null-filled side of an outer join.
    pub(super) fn push_null(&mut self) {
        match self {
            ColumnBuilder::Boolean(dst) => dst.push(None),
            ColumnBuilder::Int32(dst) => dst.push(None),
            ColumnBuilder::Int64(dst) => dst.push(None),
            ColumnBuilder::Float32(dst) => dst.push(None),
            ColumnBuilder::Float64(dst) => dst.push(None),
            ColumnBuilder::String(dst) => dst.push(None),
            ColumnBuilder::Binary(dst) => dst.push(None),
        }
    }

    /// Consume the builder, producing the finished typed column.
    pub(super) fn finish(self) -> ColumnData {
        match self {
            ColumnBuilder::Boolean(v) => ColumnData::Boolean(v),
            ColumnBuilder::Int32(v) => ColumnData::Int32(v),
            ColumnBuilder::Int64(v) => ColumnData::Int64(v),
            ColumnBuilder::Float32(v) => ColumnData::Float32(v),
            ColumnBuilder::Float64(v) => ColumnData::Float64(v),
            ColumnBuilder::String(v) => ColumnData::String(v),
            ColumnBuilder::Binary(v) => ColumnData::Binary(v),
        }
    }
}
