//! SQL:2003 window-function evaluation kernel.
//!
//! This module implements a self-contained window-evaluation engine over an
//! already-materialized set of rows. It is intentionally decoupled from the SQL
//! parser/planner: callers drive it directly by constructing a [`WindowFunction`]
//! and a [`WindowSpec`], then calling [`evaluate_window`] (closure-driven) or
//! [`evaluate_window_batch`] (over a [`RecordBatch`]).
//!
//! # Supported functions
//!
//! - `ROW_NUMBER()` — sequential 1-based ordinal within each partition.
//! - `RANK()` — competition ranking: equal order keys share a rank and the next
//!   distinct key skips ranks (`1, 1, 3, ...`).
//! - `DENSE_RANK()` — like `RANK()` but without gaps (`1, 1, 2, ...`).
//! - `LAG(expr[, offset[, default]])` — value of `expr` `offset` rows before the
//!   current row inside the sorted partition; out-of-range yields the supplied
//!   default or `NULL`.
//! - `LEAD(expr[, offset[, default]])` — symmetric to `LAG` but forward.
//! - `FIRST_VALUE(expr)` / `LAST_VALUE(expr)` — first / last value of `expr` in
//!   the sorted partition.
//! - `NTH_VALUE(expr, n)` — the `n`-th (1-based) value of `expr` in the sorted
//!   partition, or `NULL` when the partition has fewer than `n` rows.
//!
//! # Row model
//!
//! The crate stores data column-by-column ([`RecordBatch`] / [`ColumnData`]),
//! so there is no concrete "row" struct to borrow. The kernel therefore
//! addresses values by `(row_index, column_index)` pairs and pulls them through
//! a `value_at` closure. Column indices in [`WindowSpec`] and the `target_column`
//! arguments are positions into that columnar model, exactly as in
//! [`crate::executor::aggregate`] and [`crate::executor::sort`].
//!
//! # Value ordering
//!
//! Ordering follows the same rules as [`crate::executor::sort`]: real values
//! compare naturally, NULLs sort *after* non-NULLs in ascending order, and a
//! descending [`OrderKey`] simply reverses the per-key ordering. Numeric values
//! of differing widths are compared after promotion (matching the coercion used
//! by [`crate::executor::filter`]). The comparison never assumes the full set of
//! [`Value`] variants, so newer variants degrade gracefully to "equal".

use crate::error::{QueryError, Result};
use crate::executor::filter::Value;
use crate::executor::scan::{ColumnData, RecordBatch};
use std::cmp::Ordering;
use std::collections::HashMap;

/// A window function to evaluate over partitioned, ordered rows.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowFunction {
    /// `ROW_NUMBER()` — 1-based sequential ordinal within the partition.
    RowNumber,
    /// `RANK()` — competition ranking with gaps after ties.
    Rank,
    /// `DENSE_RANK()` — ranking without gaps after ties.
    DenseRank,
    /// `LAG(expr[, offset[, default]])`.
    Lag {
        /// Number of rows to look backward (defaults to 1 in the constructor).
        offset: usize,
        /// Value to use when the lookup falls before the partition start.
        default: Option<Value>,
    },
    /// `LEAD(expr[, offset[, default]])`.
    Lead {
        /// Number of rows to look forward (defaults to 1 in the constructor).
        offset: usize,
        /// Value to use when the lookup falls past the partition end.
        default: Option<Value>,
    },
    /// `FIRST_VALUE(expr)` — first value of `expr` in the sorted partition.
    FirstValue,
    /// `LAST_VALUE(expr)` — last value of `expr` in the sorted partition.
    LastValue,
    /// `NTH_VALUE(expr, n)` — the `n`-th (1-based) value in the sorted partition.
    NthValue {
        /// 1-based position to fetch; `0` is treated as out of range.
        n: usize,
    },
}

impl WindowFunction {
    /// `LAG(expr)` with the default offset of 1 and a `NULL` fallback.
    pub fn lag() -> Self {
        WindowFunction::Lag {
            offset: 1,
            default: None,
        }
    }

    /// `LAG(expr, offset)` with a `NULL` fallback.
    pub fn lag_offset(offset: usize) -> Self {
        WindowFunction::Lag {
            offset,
            default: None,
        }
    }

    /// `LAG(expr, offset, default)`.
    pub fn lag_offset_default(offset: usize, default: Value) -> Self {
        WindowFunction::Lag {
            offset,
            default: Some(default),
        }
    }

    /// `LEAD(expr)` with the default offset of 1 and a `NULL` fallback.
    pub fn lead() -> Self {
        WindowFunction::Lead {
            offset: 1,
            default: None,
        }
    }

    /// `LEAD(expr, offset)` with a `NULL` fallback.
    pub fn lead_offset(offset: usize) -> Self {
        WindowFunction::Lead {
            offset,
            default: None,
        }
    }

    /// `LEAD(expr, offset, default)`.
    pub fn lead_offset_default(offset: usize, default: Value) -> Self {
        WindowFunction::Lead {
            offset,
            default: Some(default),
        }
    }

    /// `NTH_VALUE(expr, n)`.
    pub fn nth_value(n: usize) -> Self {
        WindowFunction::NthValue { n }
    }

    /// Whether the function reads a target column (`LAG`/`LEAD`/`*_VALUE`).
    ///
    /// Ranking functions (`ROW_NUMBER`, `RANK`, `DENSE_RANK`) ignore the target
    /// column entirely.
    pub fn reads_target(&self) -> bool {
        matches!(
            self,
            WindowFunction::Lag { .. }
                | WindowFunction::Lead { .. }
                | WindowFunction::FirstValue
                | WindowFunction::LastValue
                | WindowFunction::NthValue { .. }
        )
    }
}

/// A single `ORDER BY` key inside an `OVER (... ORDER BY ...)` clause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderKey {
    /// Column index (position into the columnar row model).
    pub column: usize,
    /// `true` for `ASC`, `false` for `DESC`.
    pub ascending: bool,
}

impl OrderKey {
    /// Construct an ascending order key on `column`.
    pub fn asc(column: usize) -> Self {
        Self {
            column,
            ascending: true,
        }
    }

    /// Construct a descending order key on `column`.
    pub fn desc(column: usize) -> Self {
        Self {
            column,
            ascending: false,
        }
    }
}

/// The `OVER (PARTITION BY ... ORDER BY ...)` specification.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WindowSpec {
    /// Columns used to partition rows (empty means a single partition).
    pub partition_by: Vec<usize>,
    /// Ordering keys applied within each partition.
    pub order_by: Vec<OrderKey>,
}

impl WindowSpec {
    /// Build a spec from partition and ordering keys.
    pub fn new(partition_by: Vec<usize>, order_by: Vec<OrderKey>) -> Self {
        Self {
            partition_by,
            order_by,
        }
    }

    /// A spec with no partitioning and the given ordering keys.
    pub fn ordered(order_by: Vec<OrderKey>) -> Self {
        Self {
            partition_by: Vec::new(),
            order_by,
        }
    }
}

/// Compare two [`Value`]s using SQL ordering semantics.
///
/// This mirrors [`crate::executor::sort`]: non-NULL values compare naturally,
/// NULLs sort *after* non-NULLs (so ascending order places NULLs last), and
/// numeric values of differing widths are compared after promotion (matching the
/// coercion in [`crate::executor::filter`]). The match deliberately ends with a
/// catch-all arm so that [`Value`] variants this kernel does not model (for
/// example geometry values on the integration branch) never cause a panic.
fn compare_values(left: &Value, right: &Value) -> Ordering {
    // NULL handling first, identical to the sort executor: a present value sorts
    // before NULL, two NULLs are equal.
    match (left, right) {
        (Value::Null, Value::Null) => return Ordering::Equal,
        (Value::Null, _) => return Ordering::Greater,
        (_, Value::Null) => return Ordering::Less,
        _ => {}
    }

    match (left, right) {
        (Value::Boolean(a), Value::Boolean(b)) => a.cmp(b),
        (Value::String(a), Value::String(b)) => a.cmp(b),

        // Same-width integers.
        (Value::Int32(a), Value::Int32(b)) => a.cmp(b),
        (Value::Int64(a), Value::Int64(b)) => a.cmp(b),

        // Mixed-width integers: promote to i64.
        (Value::Int32(a), Value::Int64(b)) => (*a as i64).cmp(b),
        (Value::Int64(a), Value::Int32(b)) => a.cmp(&(*b as i64)),

        // Same-width floats.
        (Value::Float32(a), Value::Float32(b)) => float_cmp(*a as f64, *b as f64),
        (Value::Float64(a), Value::Float64(b)) => float_cmp(*a, *b),

        // Mixed-width floats: promote to f64.
        (Value::Float32(a), Value::Float64(b)) => float_cmp(*a as f64, *b),
        (Value::Float64(a), Value::Float32(b)) => float_cmp(*a, *b as f64),

        // Integer vs float: compare as f64 (matches filter coercion).
        (Value::Int32(a), Value::Float32(b)) => float_cmp(*a as f64, *b as f64),
        (Value::Int32(a), Value::Float64(b)) => float_cmp(*a as f64, *b),
        (Value::Int64(a), Value::Float32(b)) => float_cmp(*a as f64, *b as f64),
        (Value::Int64(a), Value::Float64(b)) => float_cmp(*a as f64, *b),
        (Value::Float32(a), Value::Int32(b)) => float_cmp(*a as f64, *b as f64),
        (Value::Float32(a), Value::Int64(b)) => float_cmp(*a as f64, *b as f64),
        (Value::Float64(a), Value::Int32(b)) => float_cmp(*a, *b as f64),
        (Value::Float64(a), Value::Int64(b)) => float_cmp(*a, *b as f64),

        // Unknown / incomparable combinations (including variants this kernel
        // does not model) are treated as equal so ordering stays total-ish and
        // never panics.
        _ => Ordering::Equal,
    }
}

/// Total order over `f64` for ordering purposes (NaN sorts last).
fn float_cmp(a: f64, b: f64) -> Ordering {
    match a.partial_cmp(&b) {
        Some(ordering) => ordering,
        None => {
            // Push NaN to the end deterministically.
            match (a.is_nan(), b.is_nan()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => Ordering::Equal,
            }
        }
    }
}

/// Whether two order-key tuples are equal for ranking purposes.
fn order_keys_equal(a: &[Value], b: &[Value]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| compare_values(x, y) == Ordering::Equal)
}

/// A hashable, order-preserving partition key.
///
/// Partition equality must group identical key tuples while preserving the
/// first-seen order of distinct keys. Rather than require [`Value`] to be
/// `Hash`/`Eq` (it is neither — it carries floats), we derive a stable string
/// fingerprint from each key value, in the same spirit as the join executor's
/// `to_hash_key`. The catch-all arm keeps unmodeled variants from breaking.
///
/// The `#[allow(unreachable_patterns)]` is deliberate: the integration branch
/// may carry additional [`Value`] variants (for example geometry values) that do
/// not exist at this base, so the trailing `other` arm is required for
/// forward-compatibility even though every current variant is matched explicitly.
#[allow(unreachable_patterns)]
fn partition_fingerprint(values: &[Value]) -> String {
    let mut key = String::new();
    for value in values {
        match value {
            Value::Null => key.push_str("N|"),
            Value::Boolean(b) => {
                key.push_str("b:");
                key.push_str(if *b { "1" } else { "0" });
                key.push('|');
            }
            Value::Int32(i) => {
                key.push_str("i:");
                key.push_str(&(*i as i64).to_string());
                key.push('|');
            }
            Value::Int64(i) => {
                key.push_str("i:");
                key.push_str(&i.to_string());
                key.push('|');
            }
            Value::Float32(fl) => {
                key.push_str("f:");
                key.push_str(&format!("{:?}", *fl as f64));
                key.push('|');
            }
            Value::Float64(fl) => {
                key.push_str("f:");
                key.push_str(&format!("{:?}", fl));
                key.push('|');
            }
            Value::String(s) => {
                key.push_str("s:");
                // Length-prefix to avoid delimiter collisions across values.
                key.push_str(&s.len().to_string());
                key.push(':');
                key.push_str(s);
                key.push('|');
            }
            // Unmodeled variants get a distinct, Debug-derived fingerprint so
            // distinct values stay distinct without an exhaustive match.
            other => {
                key.push_str("x:");
                key.push_str(&format!("{:?}", other));
                key.push('|');
            }
        }
    }
    key
}

/// Evaluate a window function over `num_rows` rows.
///
/// `value_at(row_index, column_index)` must return the [`Value`] stored at that
/// position; this is the closure-based analogue of indexing a concrete row. The
/// `target_column` is the column referenced by value-returning functions
/// (`LAG`, `LEAD`, `FIRST_VALUE`, `LAST_VALUE`, `NTH_VALUE`); it is ignored by
/// the ranking functions but still validated so callers fail fast on bad input.
///
/// Returns exactly one [`Value`] per input row, in **input order**.
pub fn evaluate_window(
    func: &WindowFunction,
    spec: &WindowSpec,
    num_rows: usize,
    target_column: usize,
    value_at: impl Fn(usize, usize) -> Value,
) -> Result<Vec<Value>> {
    if num_rows == 0 {
        return Ok(Vec::new());
    }

    // (1) Group rows into partitions, preserving the first-seen order of keys.
    let partitions = build_partitions(spec, num_rows, &value_at);

    // Output buffer, scattered back to input order at the end.
    let mut output = vec![Value::Null; num_rows];

    for partition in &partitions {
        // (2) Stably sort the partition's row indices by the ORDER BY keys.
        let sorted = sort_partition(spec, partition, &value_at);

        // (3) Assign per-function values along the sorted order.
        match func {
            WindowFunction::RowNumber => {
                assign_row_number(&sorted, &mut output);
            }
            WindowFunction::Rank => {
                assign_rank(spec, &sorted, &value_at, &mut output, true);
            }
            WindowFunction::DenseRank => {
                assign_rank(spec, &sorted, &value_at, &mut output, false);
            }
            WindowFunction::Lag { offset, default } => {
                assign_lag_lead(
                    &sorted,
                    target_column,
                    &value_at,
                    default,
                    *offset as isize,
                    true,
                    &mut output,
                );
            }
            WindowFunction::Lead { offset, default } => {
                assign_lag_lead(
                    &sorted,
                    target_column,
                    &value_at,
                    default,
                    *offset as isize,
                    false,
                    &mut output,
                );
            }
            WindowFunction::FirstValue => {
                assign_nth_in_partition(&sorted, target_column, &value_at, Some(0), &mut output);
            }
            WindowFunction::LastValue => {
                let last = sorted.len().checked_sub(1);
                assign_nth_in_partition(&sorted, target_column, &value_at, last, &mut output);
            }
            WindowFunction::NthValue { n } => {
                // 1-based; 0 (or out of range) yields NULL via checked_sub.
                let index = n.checked_sub(1);
                assign_nth_in_partition(&sorted, target_column, &value_at, index, &mut output);
            }
        }
    }

    Ok(output)
}

/// Evaluate a window function directly over a [`RecordBatch`].
///
/// This is a convenience wrapper around [`evaluate_window`] that extracts values
/// from columns exactly the way [`crate::executor::filter`] does. Column indices
/// in `spec` and `target_column` are positions into `batch.columns`.
pub fn evaluate_window_batch(
    func: &WindowFunction,
    spec: &WindowSpec,
    target_column: usize,
    batch: &RecordBatch,
) -> Result<Vec<Value>> {
    let max_column = spec
        .partition_by
        .iter()
        .chain(spec.order_by.iter().map(|key| &key.column))
        .copied()
        .max();

    if let Some(max_column) = max_column {
        if max_column >= batch.columns.len() {
            return Err(QueryError::execution(format!(
                "Window column index {} out of bounds (batch has {} columns)",
                max_column,
                batch.columns.len()
            )));
        }
    }

    if func.reads_target() && target_column >= batch.columns.len() {
        return Err(QueryError::execution(format!(
            "Window target column index {} out of bounds (batch has {} columns)",
            target_column,
            batch.columns.len()
        )));
    }

    evaluate_window(func, spec, batch.num_rows, target_column, |row, column| {
        column_value(&batch.columns[column], row)
    })
}

/// Extract a [`Value`] from a column at `row_idx`, mirroring the filter executor.
///
/// Binary columns have no scalar [`Value`] representation, so they surface as
/// `Value::Null` here (window ordering/partitioning over binary is not defined).
fn column_value(column: &ColumnData, row_idx: usize) -> Value {
    match column {
        ColumnData::Boolean(data) => data
            .get(row_idx)
            .and_then(|v| v.as_ref())
            .map(|&v| Value::Boolean(v))
            .unwrap_or(Value::Null),
        ColumnData::Int32(data) => data
            .get(row_idx)
            .and_then(|v| v.as_ref())
            .map(|&v| Value::Int32(v))
            .unwrap_or(Value::Null),
        ColumnData::Int64(data) => data
            .get(row_idx)
            .and_then(|v| v.as_ref())
            .map(|&v| Value::Int64(v))
            .unwrap_or(Value::Null),
        ColumnData::Float32(data) => data
            .get(row_idx)
            .and_then(|v| v.as_ref())
            .map(|&v| Value::Float32(v))
            .unwrap_or(Value::Null),
        ColumnData::Float64(data) => data
            .get(row_idx)
            .and_then(|v| v.as_ref())
            .map(|&v| Value::Float64(v))
            .unwrap_or(Value::Null),
        ColumnData::String(data) => data
            .get(row_idx)
            .and_then(|v| v.as_ref())
            .map(|v| Value::String(v.clone()))
            .unwrap_or(Value::Null),
        ColumnData::Binary(_) => Value::Null,
    }
}

/// Group rows into partitions, preserving the first-seen order of distinct keys.
fn build_partitions(
    spec: &WindowSpec,
    num_rows: usize,
    value_at: &impl Fn(usize, usize) -> Value,
) -> Vec<Vec<usize>> {
    if spec.partition_by.is_empty() {
        // A single partition containing every row in input order.
        return vec![(0..num_rows).collect()];
    }

    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();

    for row in 0..num_rows {
        let key_values: Vec<Value> = spec
            .partition_by
            .iter()
            .map(|&column| value_at(row, column))
            .collect();
        let fingerprint = partition_fingerprint(&key_values);

        match groups.get_mut(&fingerprint) {
            Some(bucket) => bucket.push(row),
            None => {
                order.push(fingerprint.clone());
                groups.insert(fingerprint, vec![row]);
            }
        }
    }

    order
        .into_iter()
        .filter_map(|fingerprint| groups.remove(&fingerprint))
        .collect()
}

/// Stably sort a partition's row indices according to the `ORDER BY` keys.
///
/// Stability matters: rows with equal keys (or no `ORDER BY` at all) keep their
/// relative input order, just as the sort executor relies on `sort_by`.
fn sort_partition(
    spec: &WindowSpec,
    partition: &[usize],
    value_at: &impl Fn(usize, usize) -> Value,
) -> Vec<usize> {
    let mut sorted = partition.to_vec();
    if spec.order_by.is_empty() {
        return sorted;
    }

    sorted.sort_by(|&a, &b| {
        for key in &spec.order_by {
            let left = value_at(a, key.column);
            let right = value_at(b, key.column);
            let mut ordering = compare_values(&left, &right);
            if !key.ascending {
                ordering = ordering.reverse();
            }
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        Ordering::Equal
    });

    sorted
}

/// Materialize the order-key tuple for `row`.
fn order_key_values(
    spec: &WindowSpec,
    row: usize,
    value_at: &impl Fn(usize, usize) -> Value,
) -> Vec<Value> {
    spec.order_by
        .iter()
        .map(|key| value_at(row, key.column))
        .collect()
}

/// `ROW_NUMBER()`: assign `1, 2, 3, ...` along the sorted order.
fn assign_row_number(sorted: &[usize], output: &mut [Value]) {
    for (position, &row) in sorted.iter().enumerate() {
        output[row] = Value::Int64((position + 1) as i64);
    }
}

/// `RANK()` / `DENSE_RANK()` over a sorted partition.
///
/// With `competition == true` this is `RANK()` (gaps after ties); with
/// `competition == false` it is `DENSE_RANK()` (no gaps). Ties are decided by
/// comparing the full `ORDER BY` key tuple. With no `ORDER BY`, every row is a
/// tie and shares rank `1`.
fn assign_rank(
    spec: &WindowSpec,
    sorted: &[usize],
    value_at: &impl Fn(usize, usize) -> Value,
    output: &mut [Value],
    competition: bool,
) {
    let mut current_rank: i64 = 0;
    let mut previous_key: Option<Vec<Value>> = None;

    for (position, &row) in sorted.iter().enumerate() {
        // 1-based ordinal of this row in the sorted partition; for RANK() this is
        // the rank assigned when a new (distinct) order-key group begins.
        let ordinal = (position + 1) as i64;
        let key = order_key_values(spec, row, value_at);

        let is_new_group = match &previous_key {
            None => true,
            Some(prev) => !order_keys_equal(prev, &key),
        };

        if is_new_group {
            current_rank = if competition {
                ordinal
            } else {
                current_rank + 1
            };
            previous_key = Some(key);
        }

        output[row] = Value::Int64(current_rank);
    }
}

/// `LAG` / `LEAD`: read `target_column` at a signed offset within the partition.
///
/// `backward == true` implements `LAG` (look at `position - offset`); otherwise
/// `LEAD` (look at `position + offset`). Out-of-range lookups yield the supplied
/// `default` if present, else `Value::Null`.
fn assign_lag_lead(
    sorted: &[usize],
    target_column: usize,
    value_at: &impl Fn(usize, usize) -> Value,
    default: &Option<Value>,
    offset: isize,
    backward: bool,
    output: &mut [Value],
) {
    let len = sorted.len() as isize;
    let signed_offset = if backward { -offset } else { offset };

    for (position, &row) in sorted.iter().enumerate() {
        let target_index = position as isize + signed_offset;
        let value = if target_index >= 0 && target_index < len {
            let source_row = sorted[target_index as usize];
            value_at(source_row, target_column)
        } else {
            default.clone().unwrap_or(Value::Null)
        };
        output[row] = value;
    }
}

/// `FIRST_VALUE` / `LAST_VALUE` / `NTH_VALUE`: broadcast one positional value to
/// every row of the partition.
///
/// `index` is the 0-based position within the sorted partition (already adjusted
/// from the 1-based SQL position by the caller). `None`, or an index past the
/// end, broadcasts `Value::Null`.
fn assign_nth_in_partition(
    sorted: &[usize],
    target_column: usize,
    value_at: &impl Fn(usize, usize) -> Value,
    index: Option<usize>,
    output: &mut [Value],
) {
    let picked = match index {
        Some(idx) => sorted
            .get(idx)
            .map(|&source_row| value_at(source_row, target_column))
            .unwrap_or(Value::Null),
        None => Value::Null,
    };

    for &row in sorted {
        output[row] = picked.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a closure over a small in-memory column-major table for unit tests.
    fn table(columns: Vec<Vec<Value>>) -> (usize, impl Fn(usize, usize) -> Value) {
        let num_rows = columns.first().map(|c| c.len()).unwrap_or(0);
        (num_rows, move |row: usize, col: usize| {
            columns[col][row].clone()
        })
    }

    #[test]
    fn unit_row_number_basic() -> Result<()> {
        // Single partition, ordered by column 0 ascending.
        let (rows, value_at) = table(vec![vec![
            Value::Int64(30),
            Value::Int64(10),
            Value::Int64(20),
        ]]);
        let spec = WindowSpec::ordered(vec![OrderKey::asc(0)]);
        let out = evaluate_window(&WindowFunction::RowNumber, &spec, rows, 0, value_at)?;
        // Sorted order is 10(row1),20(row2),30(row0) => ranks scattered to input.
        assert_eq!(out[1], Value::Int64(1));
        assert_eq!(out[2], Value::Int64(2));
        assert_eq!(out[0], Value::Int64(3));
        Ok(())
    }

    #[test]
    fn unit_compare_nulls_last() {
        assert_eq!(
            compare_values(&Value::Int64(1), &Value::Null),
            Ordering::Less
        );
        assert_eq!(
            compare_values(&Value::Null, &Value::Int64(1)),
            Ordering::Greater
        );
        assert_eq!(compare_values(&Value::Null, &Value::Null), Ordering::Equal);
    }

    #[test]
    fn unit_compare_mixed_numeric() {
        assert_eq!(
            compare_values(&Value::Int32(5), &Value::Float64(5.5)),
            Ordering::Less
        );
        assert_eq!(
            compare_values(&Value::Float64(2.0), &Value::Int64(2)),
            Ordering::Equal
        );
    }
}
