//! Row-level spatial and attribute filtering for GeoParquet.
//!
//! This module provides:
//! - [`AttributePredicates`] — a set of column-level conditions combined with
//!   an AND or OR logic operator.
//! - [`ColumnCondition`] — a single column comparison (col op value).
//! - [`CompareOp`] — the comparison operator (Eq, Ne, Gt, Lt, Gte, Lte).
//! - [`LogicOp`] — whether multiple conditions are AND-ed or OR-ed.
//!
//! These are consumed by [`crate::reader::GeoParquetReader::read_filtered_exact`]
//! and [`crate::reader::GeoParquetReader::read_with_filter`].

use crate::error::{GeoParquetError, Result};
use arrow_array::{
    Array, BooleanArray, Float32Array, Float64Array, Int8Array, Int16Array, Int32Array, Int64Array,
    RecordBatch, StringArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array,
};

// ── Logic operator ─────────────────────────────────────────────────────────────

/// Whether multiple [`ColumnCondition`]s are AND-ed or OR-ed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicOp {
    /// All conditions must be satisfied (logical AND).
    And,
    /// At least one condition must be satisfied (logical OR).
    Or,
}

// ── Compare operator ───────────────────────────────────────────────────────────

/// Comparison operator for a [`ColumnCondition`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    /// Equal to.
    Eq,
    /// Not equal to.
    Ne,
    /// Greater than.
    Gt,
    /// Less than.
    Lt,
    /// Greater than or equal to.
    Gte,
    /// Less than or equal to.
    Lte,
}

// ── Column condition ───────────────────────────────────────────────────────────

/// A single comparison against a column value.
///
/// The `value` is a [`serde_json::Value`] so it can hold strings, numbers and
/// booleans without requiring callers to import Arrow types.
#[derive(Debug, Clone)]
pub struct ColumnCondition {
    /// Arrow column name to test.
    pub column: String,
    /// Comparison operator.
    pub op: CompareOp,
    /// Right-hand side value.
    pub value: serde_json::Value,
}

impl ColumnCondition {
    /// Creates a new column condition.
    pub fn new(column: impl Into<String>, op: CompareOp, value: serde_json::Value) -> Self {
        Self {
            column: column.into(),
            op,
            value,
        }
    }
}

// ── Attribute predicates ───────────────────────────────────────────────────────

/// A set of column conditions combined with a [`LogicOp`].
#[derive(Debug, Clone)]
pub struct AttributePredicates {
    /// The list of conditions.
    pub conditions: Vec<ColumnCondition>,
    /// Whether conditions are AND-ed or OR-ed.
    pub logic: LogicOp,
}

impl AttributePredicates {
    /// Creates a new set of predicates with AND logic.
    pub fn all_of(conditions: Vec<ColumnCondition>) -> Self {
        Self {
            conditions,
            logic: LogicOp::And,
        }
    }

    /// Creates a new set of predicates with OR logic.
    pub fn any_of(conditions: Vec<ColumnCondition>) -> Self {
        Self {
            conditions,
            logic: LogicOp::Or,
        }
    }

    /// Builds a boolean mask (one entry per row) for the given `RecordBatch`.
    ///
    /// Returns a `Vec<bool>` with the same length as `batch.num_rows()`.
    /// For an empty conditions list, every row matches.
    pub fn row_mask(&self, batch: &RecordBatch) -> Result<Vec<bool>> {
        let num_rows = batch.num_rows();
        if self.conditions.is_empty() {
            return Ok(vec![true; num_rows]);
        }

        // Compute per-condition masks and fold with the logic operator.
        let mut combined: Option<Vec<bool>> = None;

        for cond in &self.conditions {
            let mask = condition_mask(batch, cond)?;

            combined = Some(match combined {
                None => mask,
                Some(prev) => match self.logic {
                    LogicOp::And => prev
                        .iter()
                        .zip(mask.iter())
                        .map(|(a, b)| *a && *b)
                        .collect(),
                    LogicOp::Or => prev
                        .iter()
                        .zip(mask.iter())
                        .map(|(a, b)| *a || *b)
                        .collect(),
                },
            });
        }

        Ok(combined.unwrap_or_else(|| vec![true; num_rows]))
    }
}

// ── Core mask computation ─────────────────────────────────────────────────────

/// Computes a boolean mask for a single [`ColumnCondition`] applied to `batch`.
pub fn condition_mask(batch: &RecordBatch, cond: &ColumnCondition) -> Result<Vec<bool>> {
    let col = batch
        .column_by_name(&cond.column)
        .ok_or_else(|| GeoParquetError::missing_field(&cond.column))?;

    let num_rows = col.len();
    let mut mask = vec![false; num_rows];

    // Downcast to the concrete Arrow array type and apply the comparison.
    // We support: Utf8, Int8/16/32/64, UInt8/16/32/64, Float32/64, Boolean.

    if let Some(arr) = col.as_any().downcast_ref::<StringArray>() {
        let FilterTarget::Str(rhs) = extract_target(&cond.value, col.data_type())? else {
            return Ok(mask);
        };
        for (i, m) in mask.iter_mut().enumerate() {
            if arr.is_null(i) {
                continue;
            }
            *m = cmp_str(cond.op, arr.value(i), &rhs);
        }
    } else if let Some(arr) = col.as_any().downcast_ref::<Int64Array>() {
        let FilterTarget::I64(rhs) = extract_target(&cond.value, col.data_type())? else {
            return Ok(mask);
        };
        for (i, m) in mask.iter_mut().enumerate() {
            if arr.is_null(i) {
                continue;
            }
            *m = cmp_ord(cond.op, arr.value(i), rhs);
        }
    } else if let Some(arr) = col.as_any().downcast_ref::<Int32Array>() {
        let FilterTarget::I64(rhs) = extract_target(&cond.value, col.data_type())? else {
            return Ok(mask);
        };
        for (i, m) in mask.iter_mut().enumerate() {
            if arr.is_null(i) {
                continue;
            }
            *m = cmp_ord(cond.op, arr.value(i) as i64, rhs);
        }
    } else if let Some(arr) = col.as_any().downcast_ref::<Int16Array>() {
        let FilterTarget::I64(rhs) = extract_target(&cond.value, col.data_type())? else {
            return Ok(mask);
        };
        for (i, m) in mask.iter_mut().enumerate() {
            if arr.is_null(i) {
                continue;
            }
            *m = cmp_ord(cond.op, arr.value(i) as i64, rhs);
        }
    } else if let Some(arr) = col.as_any().downcast_ref::<Int8Array>() {
        let FilterTarget::I64(rhs) = extract_target(&cond.value, col.data_type())? else {
            return Ok(mask);
        };
        for (i, m) in mask.iter_mut().enumerate() {
            if arr.is_null(i) {
                continue;
            }
            *m = cmp_ord(cond.op, arr.value(i) as i64, rhs);
        }
    } else if let Some(arr) = col.as_any().downcast_ref::<UInt64Array>() {
        let FilterTarget::I64(rhs) = extract_target(&cond.value, col.data_type())? else {
            return Ok(mask);
        };
        for (i, m) in mask.iter_mut().enumerate() {
            if arr.is_null(i) {
                continue;
            }
            *m = cmp_ord(cond.op, arr.value(i) as i64, rhs);
        }
    } else if let Some(arr) = col.as_any().downcast_ref::<UInt32Array>() {
        let FilterTarget::I64(rhs) = extract_target(&cond.value, col.data_type())? else {
            return Ok(mask);
        };
        for (i, m) in mask.iter_mut().enumerate() {
            if arr.is_null(i) {
                continue;
            }
            *m = cmp_ord(cond.op, arr.value(i) as i64, rhs);
        }
    } else if let Some(arr) = col.as_any().downcast_ref::<UInt16Array>() {
        let FilterTarget::I64(rhs) = extract_target(&cond.value, col.data_type())? else {
            return Ok(mask);
        };
        for (i, m) in mask.iter_mut().enumerate() {
            if arr.is_null(i) {
                continue;
            }
            *m = cmp_ord(cond.op, arr.value(i) as i64, rhs);
        }
    } else if let Some(arr) = col.as_any().downcast_ref::<UInt8Array>() {
        let FilterTarget::I64(rhs) = extract_target(&cond.value, col.data_type())? else {
            return Ok(mask);
        };
        for (i, m) in mask.iter_mut().enumerate() {
            if arr.is_null(i) {
                continue;
            }
            *m = cmp_ord(cond.op, arr.value(i) as i64, rhs);
        }
    } else if let Some(arr) = col.as_any().downcast_ref::<Float64Array>() {
        let FilterTarget::F64(rhs) = extract_target(&cond.value, col.data_type())? else {
            return Ok(mask);
        };
        for (i, m) in mask.iter_mut().enumerate() {
            if arr.is_null(i) {
                continue;
            }
            *m = cmp_f64(cond.op, arr.value(i), rhs);
        }
    } else if let Some(arr) = col.as_any().downcast_ref::<Float32Array>() {
        let FilterTarget::F64(rhs) = extract_target(&cond.value, col.data_type())? else {
            return Ok(mask);
        };
        for (i, m) in mask.iter_mut().enumerate() {
            if arr.is_null(i) {
                continue;
            }
            *m = cmp_f64(cond.op, arr.value(i) as f64, rhs);
        }
    } else if let Some(arr) = col.as_any().downcast_ref::<BooleanArray>() {
        let FilterTarget::Bool(rhs) = extract_target(&cond.value, col.data_type())? else {
            return Ok(mask);
        };
        for (i, m) in mask.iter_mut().enumerate() {
            if arr.is_null(i) {
                continue;
            }
            let lhs = arr.value(i);
            *m = match cond.op {
                CompareOp::Eq => lhs == rhs,
                CompareOp::Ne => lhs != rhs,
                // Ordering on booleans: true > false
                CompareOp::Gt => lhs & !rhs,
                CompareOp::Lt => !lhs & rhs,
                CompareOp::Gte => lhs == rhs || (lhs & !rhs),
                CompareOp::Lte => lhs == rhs || (!lhs & rhs),
            };
        }
    } else {
        // Unsupported column type — no rows match.
    }

    Ok(mask)
}

// ── Filter target (typed RHS) ─────────────────────────────────────────────────

/// Typed right-hand side extracted from a [`serde_json::Value`].
enum FilterTarget {
    Str(String),
    I64(i64),
    F64(f64),
    Bool(bool),
}

/// Extracts a [`FilterTarget`] from a JSON value, coercing to the column's
/// data type where possible.
fn extract_target(
    json: &serde_json::Value,
    col_type: &arrow_schema::DataType,
) -> Result<FilterTarget> {
    use arrow_schema::DataType;

    match col_type {
        DataType::Utf8 | DataType::LargeUtf8 => match json {
            serde_json::Value::String(s) => Ok(FilterTarget::Str(s.clone())),
            serde_json::Value::Number(n) => Ok(FilterTarget::Str(n.to_string())),
            serde_json::Value::Bool(b) => Ok(FilterTarget::Str(b.to_string())),
            other => Err(GeoParquetError::type_mismatch("string", other.to_string())),
        },
        DataType::Float32 | DataType::Float64 => match json {
            serde_json::Value::Number(n) => {
                let f = n
                    .as_f64()
                    .ok_or_else(|| GeoParquetError::type_mismatch("f64", json.to_string()))?;
                Ok(FilterTarget::F64(f))
            }
            other => Err(GeoParquetError::type_mismatch("float", other.to_string())),
        },
        DataType::Boolean => match json {
            serde_json::Value::Bool(b) => Ok(FilterTarget::Bool(*b)),
            other => Err(GeoParquetError::type_mismatch("bool", other.to_string())),
        },
        // All integer-like types
        _ => match json {
            serde_json::Value::Number(n) => {
                // Prefer integer, fall back to float-as-integer.
                let i = n
                    .as_i64()
                    .or_else(|| n.as_f64().map(|f| f as i64))
                    .ok_or_else(|| GeoParquetError::type_mismatch("integer", json.to_string()))?;
                Ok(FilterTarget::I64(i))
            }
            serde_json::Value::String(s) => {
                let i = s
                    .parse::<i64>()
                    .map_err(|_| GeoParquetError::type_mismatch("integer", s.clone()))?;
                Ok(FilterTarget::I64(i))
            }
            other => Err(GeoParquetError::type_mismatch("integer", other.to_string())),
        },
    }
}

// ── Comparison helpers ────────────────────────────────────────────────────────

fn cmp_str(op: CompareOp, lhs: &str, rhs: &str) -> bool {
    match op {
        CompareOp::Eq => lhs == rhs,
        CompareOp::Ne => lhs != rhs,
        CompareOp::Gt => lhs > rhs,
        CompareOp::Lt => lhs < rhs,
        CompareOp::Gte => lhs >= rhs,
        CompareOp::Lte => lhs <= rhs,
    }
}

fn cmp_ord(op: CompareOp, lhs: i64, rhs: i64) -> bool {
    match op {
        CompareOp::Eq => lhs == rhs,
        CompareOp::Ne => lhs != rhs,
        CompareOp::Gt => lhs > rhs,
        CompareOp::Lt => lhs < rhs,
        CompareOp::Gte => lhs >= rhs,
        CompareOp::Lte => lhs <= rhs,
    }
}

fn cmp_f64(op: CompareOp, lhs: f64, rhs: f64) -> bool {
    match op {
        CompareOp::Eq => lhs == rhs,
        CompareOp::Ne => lhs != rhs,
        CompareOp::Gt => lhs > rhs,
        CompareOp::Lt => lhs < rhs,
        CompareOp::Gte => lhs >= rhs,
        CompareOp::Lte => lhs <= rhs,
    }
}

// ── Utility: slice a RecordBatch by a boolean mask ─────────────────────────────

/// Returns a new [`RecordBatch`] containing only the rows where `mask[i]` is
/// `true`.
///
/// Uses [`RecordBatch::slice`] for contiguous runs when possible; otherwise
/// falls back to building boolean arrays and using arrow's `filter` kernel.
pub fn filter_batch_by_mask(batch: &RecordBatch, mask: &[bool]) -> Result<RecordBatch> {
    assert_eq!(
        batch.num_rows(),
        mask.len(),
        "mask length must equal batch row count"
    );

    // Build a BooleanArray from the mask and use the Arrow `filter` kernel.
    let bool_array = BooleanArray::from(mask.to_vec());
    let filtered_cols: std::result::Result<Vec<_>, _> = batch
        .columns()
        .iter()
        .map(|col| arrow::compute::filter(col.as_ref(), &bool_array))
        .collect();

    let filtered_cols = filtered_cols.map_err(|e| {
        GeoParquetError::Arrow(arrow::error::ArrowError::ExternalError(Box::new(e)))
    })?;

    let filtered_cols: Vec<std::sync::Arc<dyn Array>> = filtered_cols.into_iter().collect();

    RecordBatch::try_new(batch.schema(), filtered_cols).map_err(GeoParquetError::Arrow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::{Int64Array, StringArray};
    use arrow_schema::{DataType, Field, Schema};
    use std::sync::Arc;

    fn make_batch_with_string_and_int() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("name", DataType::Utf8, true),
            Field::new("score", DataType::Int64, true),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec!["alice", "bob", "carol"])),
                Arc::new(Int64Array::from(vec![10i64, 20, 30])),
            ],
        )
        .expect("test batch")
    }

    #[test]
    fn test_string_eq_mask() {
        let batch = make_batch_with_string_and_int();
        let cond = ColumnCondition::new(
            "name",
            CompareOp::Eq,
            serde_json::Value::String("bob".into()),
        );
        let mask = condition_mask(&batch, &cond).expect("mask");
        assert_eq!(mask, vec![false, true, false]);
    }

    #[test]
    fn test_int_gt_mask() {
        let batch = make_batch_with_string_and_int();
        let cond = ColumnCondition::new("score", CompareOp::Gt, serde_json::json!(15));
        let mask = condition_mask(&batch, &cond).expect("mask");
        assert_eq!(mask, vec![false, true, true]);
    }

    #[test]
    fn test_and_predicates() {
        let batch = make_batch_with_string_and_int();
        let preds = AttributePredicates::all_of(vec![
            ColumnCondition::new("score", CompareOp::Gte, serde_json::json!(20)),
            ColumnCondition::new(
                "name",
                CompareOp::Ne,
                serde_json::Value::String("carol".into()),
            ),
        ]);
        let mask = preds.row_mask(&batch).expect("mask");
        // score>=20: bob(20), carol(30) → [F,T,T]
        // name!="carol": alice, bob → [T,T,F]
        // AND → [F,T,F]
        assert_eq!(mask, vec![false, true, false]);
    }

    #[test]
    fn test_filter_batch() {
        let batch = make_batch_with_string_and_int();
        let mask = vec![true, false, true];
        let filtered = filter_batch_by_mask(&batch, &mask).expect("filter");
        assert_eq!(filtered.num_rows(), 2);

        let name_col = filtered
            .column_by_name("name")
            .expect("name col")
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("string");
        assert_eq!(name_col.value(0), "alice");
        assert_eq!(name_col.value(1), "carol");
    }
}
