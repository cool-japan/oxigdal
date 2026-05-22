//! Attribute predicate pushdown for GeoParquet readers.
//!
//! [`AttributeFilter`] represents a typed predicate on a single column.
//! It can be compiled into a [`parquet::arrow::arrow_reader::ArrowPredicate`]
//! for use with `RowFilter`, enabling Parquet-level late-materialisation
//! filtering without decoding all columns first.
//!
//! Supported filter shapes:
//! * `Eq { col, value }` — equality comparison.
//! * `Range { col, lo, hi }` — inclusive range `[lo, hi]`.
//! * `In { col, values }` — membership test (any-of).

use crate::error::{GeoParquetError, Result};
use arrow_array::{BooleanArray, RecordBatch};
use arrow_schema::{ArrowError, DataType, SchemaRef};
use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::ArrowPredicate;

// ── Scalar value ────────────────────────────────────────────────────────────────

/// A typed scalar value used by [`AttributeFilter`] predicates and by
/// [`crate::statistics::ColumnStatistics`].
///
/// Filter-side variants (`Int64`, `Float64`, `Utf8`) are the ones that
/// [`AttributeFilter::to_arrow_predicate`] knows how to evaluate against the
/// underlying Parquet column types.  The remaining variants are produced when
/// extracting Parquet column statistics from physical types that don't have a
/// natural filter representation (e.g. `Bool`, `Decimal`, `Binary`).
///
/// When a scalar with a stats-only variant is passed to a predicate compiler,
/// a [`GeoParquetError::TypeMismatch`] is returned — these variants exist
/// purely to surface the value to a user reading
/// [`crate::statistics::ColumnStatistics`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ScalarValue {
    /// 32-bit signed integer (stats-only).
    Int32(i32),
    /// 64-bit signed integer.
    Int64(i64),
    /// 32-bit float (stats-only).
    Float32(f32),
    /// 64-bit float.
    Float64(f64),
    /// UTF-8 string.
    Utf8(String),
    /// Boolean (stats-only).
    Bool(bool),
    /// Raw binary (stats-only).
    Binary(Vec<u8>),
    /// Unsupported / opaque value (stats-only); the string is the
    /// debug-format of the original Parquet value.
    Other(String),
}

// ── AttributeFilter ─────────────────────────────────────────────────────────────

/// A single-column predicate that can be pushed down into Parquet decoding.
///
/// Use [`to_arrow_predicate`] to compile this into a boxed [`ArrowPredicate`]
/// suitable for [`parquet::arrow::arrow_reader::RowFilter::new`].
///
/// [`to_arrow_predicate`]: AttributeFilter::to_arrow_predicate
#[derive(Debug, Clone)]
pub enum AttributeFilter {
    /// Equality: `col == value`.
    Eq {
        /// Column name.
        col: String,
        /// Value to compare against.
        value: ScalarValue,
    },
    /// Inclusive range: `lo <= col && col <= hi`.
    Range {
        /// Column name.
        col: String,
        /// Lower bound (inclusive).
        lo: ScalarValue,
        /// Upper bound (inclusive).
        hi: ScalarValue,
    },
    /// Membership: `col IN values`.
    In {
        /// Column name.
        col: String,
        /// Set of acceptable values.
        values: Vec<ScalarValue>,
    },
}

impl AttributeFilter {
    /// Compile this filter into a boxed [`ArrowPredicate`].
    ///
    /// The predicate will project only the referenced column, avoiding
    /// decoding of other columns during the predicate evaluation phase.
    ///
    /// # Errors
    ///
    /// Returns an error if the referenced column is not found in `schema`,
    /// or if the column type is not compatible with the filter's scalar type.
    pub fn to_arrow_predicate(
        self,
        schema: SchemaRef,
        parquet_schema: &parquet::schema::types::SchemaDescriptor,
    ) -> Result<Box<dyn ArrowPredicate>> {
        let col_name = self.col_name();
        let col_idx = schema
            .index_of(col_name)
            .map_err(|_| GeoParquetError::missing_field(col_name))?;
        let col_field = schema.field(col_idx);
        let data_type = col_field.data_type().clone();

        // Build ProjectionMask using the leaf index of this column.
        // For non-nested schemas the leaf index equals the root index.
        let leaf_indices = leaf_indices_for_root(parquet_schema, col_idx);
        let projection = ProjectionMask::leaves(parquet_schema, leaf_indices);

        let predicate: Box<dyn ArrowPredicate> = match self {
            AttributeFilter::Eq { col, value } => Box::new(EqPredicate {
                col,
                value,
                data_type,
                projection,
            }),
            AttributeFilter::Range { col, lo, hi } => Box::new(RangePredicate {
                col,
                lo,
                hi,
                data_type,
                projection,
            }),
            AttributeFilter::In { col, values } => Box::new(InPredicate {
                col,
                values,
                data_type,
                projection,
            }),
        };
        Ok(predicate)
    }

    fn col_name(&self) -> &str {
        match self {
            Self::Eq { col, .. } | Self::Range { col, .. } | Self::In { col, .. } => col,
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────────

/// Returns all leaf column indices that belong to the root column at `root_idx`.
fn leaf_indices_for_root(
    parquet_schema: &parquet::schema::types::SchemaDescriptor,
    root_idx: usize,
) -> Vec<usize> {
    (0..parquet_schema.num_columns())
        .filter(|&leaf| parquet_schema.get_column_root_idx(leaf) == root_idx)
        .collect()
}

/// Extract the first (and for simple schemas only) projected column from `batch`.
fn projected_column<'b>(
    batch: &'b RecordBatch,
    col_name: &str,
) -> Option<&'b dyn arrow_array::Array> {
    batch.column_by_name(col_name).map(|c| c.as_ref())
}

/// Evaluate a per-row equality over a column array returning a `BooleanArray`.
fn eval_eq_array(col: &dyn arrow_array::Array, value: &ScalarValue) -> Result<BooleanArray> {
    use arrow::compute::kernels::cmp::eq;
    use arrow_array::cast::AsArray;
    use arrow_array::{Float64Array, Int64Array, StringArray};

    match value {
        ScalarValue::Int64(v) => {
            let arr = col
                .as_primitive_opt::<arrow_array::types::Int64Type>()
                .ok_or_else(|| {
                    GeoParquetError::type_mismatch("Int64", format!("{:?}", col.data_type()))
                })?;
            let scalar_arr = Int64Array::new_scalar(*v);
            eq(arr, &scalar_arr).map_err(GeoParquetError::Arrow)
        }
        ScalarValue::Float64(v) => {
            let arr = col
                .as_primitive_opt::<arrow_array::types::Float64Type>()
                .ok_or_else(|| {
                    GeoParquetError::type_mismatch("Float64", format!("{:?}", col.data_type()))
                })?;
            let scalar_arr = Float64Array::new_scalar(*v);
            eq(arr, &scalar_arr).map_err(GeoParquetError::Arrow)
        }
        ScalarValue::Utf8(v) => {
            if let Some(arr) = col.as_any().downcast_ref::<StringArray>() {
                let scalar_arr = StringArray::new_scalar(v.as_str());
                eq(arr, &scalar_arr).map_err(GeoParquetError::Arrow)
            } else {
                Err(GeoParquetError::type_mismatch(
                    "Utf8",
                    format!("{:?}", col.data_type()),
                ))
            }
        }
        // Stats-only variants are not supported as filter scalars.
        other => Err(GeoParquetError::type_mismatch(
            "filter-eligible ScalarValue (Int64/Float64/Utf8)",
            format!("{other:?}"),
        )),
    }
}

/// Evaluate inclusive range `[lo, hi]` on an array.
fn eval_range_array(
    col: &dyn arrow_array::Array,
    lo: &ScalarValue,
    hi: &ScalarValue,
) -> Result<BooleanArray> {
    use arrow::compute::and;
    use arrow::compute::kernels::cmp::{gt_eq, lt_eq};
    use arrow_array::cast::AsArray;
    use arrow_array::{Float64Array, Int64Array, StringArray};

    match (lo, hi) {
        (ScalarValue::Int64(lo_v), ScalarValue::Int64(hi_v)) => {
            let arr = col
                .as_primitive_opt::<arrow_array::types::Int64Type>()
                .ok_or_else(|| {
                    GeoParquetError::type_mismatch("Int64", format!("{:?}", col.data_type()))
                })?;
            let lo_arr = Int64Array::new_scalar(*lo_v);
            let hi_arr = Int64Array::new_scalar(*hi_v);
            let ge = gt_eq(arr, &lo_arr).map_err(GeoParquetError::Arrow)?;
            let le = lt_eq(arr, &hi_arr).map_err(GeoParquetError::Arrow)?;
            and(&ge, &le).map_err(GeoParquetError::Arrow)
        }
        (ScalarValue::Float64(lo_v), ScalarValue::Float64(hi_v)) => {
            let arr = col
                .as_primitive_opt::<arrow_array::types::Float64Type>()
                .ok_or_else(|| {
                    GeoParquetError::type_mismatch("Float64", format!("{:?}", col.data_type()))
                })?;
            let lo_arr = Float64Array::new_scalar(*lo_v);
            let hi_arr = Float64Array::new_scalar(*hi_v);
            let ge = gt_eq(arr, &lo_arr).map_err(GeoParquetError::Arrow)?;
            let le = lt_eq(arr, &hi_arr).map_err(GeoParquetError::Arrow)?;
            and(&ge, &le).map_err(GeoParquetError::Arrow)
        }
        (ScalarValue::Utf8(lo_v), ScalarValue::Utf8(hi_v)) => {
            if let Some(arr) = col.as_any().downcast_ref::<StringArray>() {
                let lo_arr = StringArray::new_scalar(lo_v.as_str());
                let hi_arr = StringArray::new_scalar(hi_v.as_str());
                let ge = gt_eq(arr, &lo_arr).map_err(GeoParquetError::Arrow)?;
                let le = lt_eq(arr, &hi_arr).map_err(GeoParquetError::Arrow)?;
                and(&ge, &le).map_err(GeoParquetError::Arrow)
            } else {
                Err(GeoParquetError::type_mismatch(
                    "Utf8",
                    format!("{:?}", col.data_type()),
                ))
            }
        }
        _ => Err(GeoParquetError::type_mismatch(
            "matching ScalarValue types for lo/hi (one of Int64/Float64/Utf8)",
            "mismatched types",
        )),
    }
}

/// Evaluate `IN (values)` on an array — true if value matches any entry.
fn eval_in_array(col: &dyn arrow_array::Array, values: &[ScalarValue]) -> Result<BooleanArray> {
    use arrow::compute::or;

    if values.is_empty() {
        // Empty IN — no row matches.
        let falses = vec![false; col.len()];
        return Ok(BooleanArray::from(falses));
    }

    let mut combined: Option<BooleanArray> = None;
    for v in values {
        let mask = eval_eq_array(col, v)?;
        combined = Some(match combined {
            None => mask,
            Some(prev) => or(&prev, &mask).map_err(GeoParquetError::Arrow)?,
        });
    }
    // Safety: values is non-empty, so combined is Some.
    combined.ok_or_else(|| GeoParquetError::internal("IN predicate produced no mask"))
}

// ── Predicate implementations ───────────────────────────────────────────────────

struct EqPredicate {
    col: String,
    value: ScalarValue,
    data_type: DataType,
    projection: ProjectionMask,
}

impl ArrowPredicate for EqPredicate {
    fn projection(&self) -> &ProjectionMask {
        &self.projection
    }

    fn evaluate(&mut self, batch: RecordBatch) -> std::result::Result<BooleanArray, ArrowError> {
        let col = projected_column(&batch, &self.col).ok_or_else(|| {
            ArrowError::SchemaError(format!(
                "column '{}' not found in projected batch (type {:?})",
                self.col, self.data_type
            ))
        })?;
        eval_eq_array(col, &self.value).map_err(|e| ArrowError::ExternalError(Box::new(e)))
    }
}

struct RangePredicate {
    col: String,
    lo: ScalarValue,
    hi: ScalarValue,
    data_type: DataType,
    projection: ProjectionMask,
}

impl ArrowPredicate for RangePredicate {
    fn projection(&self) -> &ProjectionMask {
        &self.projection
    }

    fn evaluate(&mut self, batch: RecordBatch) -> std::result::Result<BooleanArray, ArrowError> {
        let col = projected_column(&batch, &self.col).ok_or_else(|| {
            ArrowError::SchemaError(format!(
                "column '{}' not found in projected batch (type {:?})",
                self.col, self.data_type
            ))
        })?;
        eval_range_array(col, &self.lo, &self.hi)
            .map_err(|e| ArrowError::ExternalError(Box::new(e)))
    }
}

struct InPredicate {
    col: String,
    values: Vec<ScalarValue>,
    data_type: DataType,
    projection: ProjectionMask,
}

impl ArrowPredicate for InPredicate {
    fn projection(&self) -> &ProjectionMask {
        &self.projection
    }

    fn evaluate(&mut self, batch: RecordBatch) -> std::result::Result<BooleanArray, ArrowError> {
        let col = projected_column(&batch, &self.col).ok_or_else(|| {
            ArrowError::SchemaError(format!(
                "column '{}' not found in projected batch (type {:?})",
                self.col, self.data_type
            ))
        })?;
        eval_in_array(col, &self.values).map_err(|e| ArrowError::ExternalError(Box::new(e)))
    }
}

// ── Covering bbox ArrowPredicate ─────────────────────────────────────────────────

/// An [`ArrowPredicate`] that checks four covering.bbox columns for intersection
/// with a query bounding box `(qxmin, qymin, qxmax, qymax)`.
///
/// Intersection condition (AABB-vs-AABB, inclusive):
/// ```text
/// row_xmax >= qxmin && row_xmin <= qxmax && row_ymax >= qymin && row_ymin <= qymax
/// ```
pub struct CoveringBboxPredicate {
    xmin_col: String,
    ymin_col: String,
    xmax_col: String,
    ymax_col: String,
    qxmin: f64,
    qymin: f64,
    qxmax: f64,
    qymax: f64,
    projection: ProjectionMask,
}

impl CoveringBboxPredicate {
    /// Create a new predicate for the given covering bbox column names and query bbox.
    pub fn new(
        xmin_col: impl Into<String>,
        ymin_col: impl Into<String>,
        xmax_col: impl Into<String>,
        ymax_col: impl Into<String>,
        qxmin: f64,
        qymin: f64,
        qxmax: f64,
        qymax: f64,
        projection: ProjectionMask,
    ) -> Self {
        Self {
            xmin_col: xmin_col.into(),
            ymin_col: ymin_col.into(),
            xmax_col: xmax_col.into(),
            ymax_col: ymax_col.into(),
            qxmin,
            qymin,
            qxmax,
            qymax,
            projection,
        }
    }

    fn eval_inner(&self, batch: &RecordBatch) -> Result<BooleanArray> {
        use arrow::compute::and;
        use arrow::compute::kernels::cmp::{gt_eq, lt_eq};
        use arrow_array::cast::AsArray;

        let xmin = get_f64_col(batch, &self.xmin_col)?;
        let ymin = get_f64_col(batch, &self.ymin_col)?;
        let xmax = get_f64_col(batch, &self.xmax_col)?;
        let ymax = get_f64_col(batch, &self.ymax_col)?;

        // row_xmax >= qxmin
        let q_xmin = arrow_array::Float64Array::new_scalar(self.qxmin);
        let c1 = gt_eq(
            xmax.as_primitive::<arrow_array::types::Float64Type>(),
            &q_xmin,
        )
        .map_err(GeoParquetError::Arrow)?;

        // row_xmin <= qxmax
        let q_xmax = arrow_array::Float64Array::new_scalar(self.qxmax);
        let c2 = lt_eq(
            xmin.as_primitive::<arrow_array::types::Float64Type>(),
            &q_xmax,
        )
        .map_err(GeoParquetError::Arrow)?;

        // row_ymax >= qymin
        let q_ymin = arrow_array::Float64Array::new_scalar(self.qymin);
        let c3 = gt_eq(
            ymax.as_primitive::<arrow_array::types::Float64Type>(),
            &q_ymin,
        )
        .map_err(GeoParquetError::Arrow)?;

        // row_ymin <= qymax
        let q_ymax = arrow_array::Float64Array::new_scalar(self.qymax);
        let c4 = lt_eq(
            ymin.as_primitive::<arrow_array::types::Float64Type>(),
            &q_ymax,
        )
        .map_err(GeoParquetError::Arrow)?;

        let tmp = and(&c1, &c2).map_err(GeoParquetError::Arrow)?;
        let tmp = and(&tmp, &c3).map_err(GeoParquetError::Arrow)?;
        and(&tmp, &c4).map_err(GeoParquetError::Arrow)
    }
}

impl ArrowPredicate for CoveringBboxPredicate {
    fn projection(&self) -> &ProjectionMask {
        &self.projection
    }

    fn evaluate(&mut self, batch: RecordBatch) -> std::result::Result<BooleanArray, ArrowError> {
        self.eval_inner(&batch)
            .map_err(|e| ArrowError::ExternalError(Box::new(e)))
    }
}

fn get_f64_col<'a>(batch: &'a RecordBatch, col_name: &str) -> Result<&'a dyn arrow_array::Array> {
    batch
        .column_by_name(col_name)
        .map(|c| c.as_ref())
        .ok_or_else(|| GeoParquetError::missing_field(col_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::{Float64Array, Int64Array, RecordBatch, StringArray};
    use arrow_schema::{DataType, Field, Schema};
    use std::sync::Arc;

    fn string_schema() -> SchemaRef {
        Arc::new(Schema::new(vec![Field::new("name", DataType::Utf8, true)]))
    }

    fn int_schema() -> SchemaRef {
        Arc::new(Schema::new(vec![Field::new(
            "population",
            DataType::Int64,
            true,
        )]))
    }

    fn string_batch(values: &[&str]) -> RecordBatch {
        let schema = string_schema();
        RecordBatch::try_new(schema, vec![Arc::new(StringArray::from(values.to_vec()))])
            .expect("batch")
    }

    fn int_batch(values: &[i64]) -> RecordBatch {
        let schema = int_schema();
        RecordBatch::try_new(schema, vec![Arc::new(Int64Array::from(values.to_vec()))])
            .expect("batch")
    }

    #[test]
    fn test_eval_eq_utf8() {
        let batch = string_batch(&["alpha", "beta", "alpha"]);
        let col = batch.column(0).as_ref();
        let result = eval_eq_array(col, &ScalarValue::Utf8("alpha".into())).expect("eval");
        assert!(result.value(0));
        assert!(!result.value(1));
        assert!(result.value(2));
    }

    #[test]
    fn test_eval_range_int() {
        let batch = int_batch(&[100, 500_000, 1_000_000, 250_000]);
        let col = batch.column(0).as_ref();
        let result = eval_range_array(col, &ScalarValue::Int64(0), &ScalarValue::Int64(500_000))
            .expect("eval");
        assert!(result.value(0));
        assert!(result.value(1));
        assert!(!result.value(2));
        assert!(result.value(3));
    }

    #[test]
    fn test_eval_in_utf8() {
        let batch = string_batch(&["alpha", "beta", "gamma"]);
        let col = batch.column(0).as_ref();
        let values = vec![
            ScalarValue::Utf8("alpha".into()),
            ScalarValue::Utf8("gamma".into()),
        ];
        let result = eval_in_array(col, &values).expect("eval");
        assert!(result.value(0));
        assert!(!result.value(1));
        assert!(result.value(2));
    }

    #[test]
    fn test_eval_in_empty_values() {
        let batch = string_batch(&["alpha", "beta"]);
        let col = batch.column(0).as_ref();
        let result = eval_in_array(col, &[]).expect("eval");
        assert!(!result.value(0));
        assert!(!result.value(1));
    }

    #[test]
    fn test_covering_bbox_predicate_intersection() {
        // Setup: 3 rows with bbox columns
        // Row 0: bbox (0,0,5,5)  — overlaps query (3,3,10,10) ✓
        // Row 1: bbox (20,20,30,30) — disjoint from query ✗
        // Row 2: bbox (8,8,12,12) — overlaps query ✓
        let schema = Arc::new(Schema::new(vec![
            Field::new("xmin", DataType::Float64, false),
            Field::new("ymin", DataType::Float64, false),
            Field::new("xmax", DataType::Float64, false),
            Field::new("ymax", DataType::Float64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Float64Array::from(vec![0.0, 20.0, 8.0])),
                Arc::new(Float64Array::from(vec![0.0, 20.0, 8.0])),
                Arc::new(Float64Array::from(vec![5.0, 30.0, 12.0])),
                Arc::new(Float64Array::from(vec![5.0, 30.0, 12.0])),
            ],
        )
        .expect("batch");

        let predicate = CoveringBboxPredicate {
            xmin_col: "xmin".into(),
            ymin_col: "ymin".into(),
            xmax_col: "xmax".into(),
            ymax_col: "ymax".into(),
            qxmin: 3.0,
            qymin: 3.0,
            qxmax: 10.0,
            qymax: 10.0,
            projection: ProjectionMask::all(),
        };
        let result = predicate.eval_inner(&batch).expect("eval");
        assert!(result.value(0));
        assert!(!result.value(1));
        assert!(result.value(2));
    }
}
