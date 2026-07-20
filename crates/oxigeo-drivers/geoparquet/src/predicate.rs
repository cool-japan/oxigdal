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
//! * `Cmp { col, op, value }` — scalar comparison (`>`, `>=`, `<`, `<=`, `<>`).

use crate::error::{GeoParquetError, Result};
use arrow_array::{BooleanArray, RecordBatch};
use arrow_schema::{ArrowError, DataType, SchemaRef};
use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::ArrowPredicate;

// ── Scalar value ────────────────────────────────────────────────────────────────

/// A typed scalar value used by [`AttributeFilter`] predicates and by
/// [`crate::statistics::ColumnStatistics`].
///
/// Filter-side variants (`Int64`, `Float64`, `Utf8`, `Bool`) are the ones that
/// [`AttributeFilter::to_arrow_predicate`] knows how to evaluate against the
/// underlying Parquet column types.  The remaining variants are produced when
/// extracting Parquet column statistics from physical types that don't have a
/// natural filter representation (e.g. `Decimal`, `Binary`).
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
    /// Boolean (usable both as column statistics and as a filter literal
    /// against a `Boolean` column).
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
    /// Scalar comparison: `col <op> value` for a non-equality operator.
    ///
    /// Covers `>` ([`CmpOp::Gt`]), `>=` ([`CmpOp::Ge`]), `<` ([`CmpOp::Lt`]),
    /// `<=` ([`CmpOp::Le`]) and `<>` ([`CmpOp::NotEq`]).  Plain equality is
    /// expressed with [`AttributeFilter::Eq`].
    Cmp {
        /// Column name.
        col: String,
        /// Comparison operator.
        op: CmpOp,
        /// Value to compare each row against (right-hand side).
        value: ScalarValue,
    },
}

/// Comparison operator for [`AttributeFilter::Cmp`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    /// Greater than (`>`).
    Gt,
    /// Greater than or equal (`>=`).
    Ge,
    /// Less than (`<`).
    Lt,
    /// Less than or equal (`<=`).
    Le,
    /// Not equal (`<>` / `!=`).
    NotEq,
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
            AttributeFilter::Cmp { col, op, value } => Box::new(CmpPredicate {
                col,
                op,
                value,
                data_type,
                projection,
            }),
        };
        Ok(predicate)
    }

    /// Returns the name of the column this filter references.
    pub(crate) fn col_name(&self) -> &str {
        match self {
            Self::Eq { col, .. }
            | Self::Range { col, .. }
            | Self::In { col, .. }
            | Self::Cmp { col, .. } => col,
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────────

/// Returns all leaf column indices that belong to the root column at `root_idx`.
pub(crate) fn leaf_indices_for_root(
    parquet_schema: &parquet::schema::types::SchemaDescriptor,
    root_idx: usize,
) -> Vec<usize> {
    (0..parquet_schema.num_columns())
        .filter(|&leaf| parquet_schema.get_column_root_idx(leaf) == root_idx)
        .collect()
}

/// Returns the leaf name (last path component) for the leaf column at `leaf_idx`.
///
/// For a flat column `geometry_bbox_xmin` the path has one part
/// (`"geometry_bbox_xmin"`).  For a struct column `bbox.xmin` the path has two
/// parts; the last (`"xmin"`) is returned.
pub(crate) fn col_name_from_leaf(
    schema: &parquet::schema::types::SchemaDescriptor,
    leaf_idx: usize,
) -> String {
    let col = schema.column(leaf_idx);
    col.path()
        .parts()
        .last()
        .cloned()
        .unwrap_or_else(|| col.name().to_owned())
}

/// Extract the first (and for simple schemas only) projected column from `batch`.
fn projected_column<'b>(
    batch: &'b RecordBatch,
    col_name: &str,
) -> Option<&'b dyn arrow_array::Array> {
    batch.column_by_name(col_name).map(|c| c.as_ref())
}

// ── Scalar coercion + comparison helpers ─────────────────────────────────────────

/// Signature of the `arrow` scalar-vs-array comparison kernels in
/// [`arrow::compute::kernels::cmp`] (`eq`, `neq`, `gt`, `gt_eq`, `lt`, `lt_eq`).
///
/// Each takes the column [`Datum`](arrow_array::Datum) on the left and a scalar
/// [`Datum`](arrow_array::Datum) on the right and returns a row-wise
/// `BooleanArray`.
type CmpKernel = fn(
    &dyn arrow_array::Datum,
    &dyn arrow_array::Datum,
) -> std::result::Result<BooleanArray, ArrowError>;

/// Coerce a numeric [`ScalarValue`] to `i64` for an exact integer comparison.
///
/// Integer literals map directly; a *whole-valued* float literal (`1000.0`) is
/// coerced so that `id = 1000` and `id = 1000.0` behave identically against an
/// integer column.  Non-numeric scalars, and floats carrying a fractional part
/// (which cannot equal any integer and must be compared in float space to keep
/// `>` / `<` correct), return `None`.
fn scalar_as_i64(value: &ScalarValue) -> Option<i64> {
    match value {
        ScalarValue::Int32(v) => Some(i64::from(*v)),
        ScalarValue::Int64(v) => Some(*v),
        ScalarValue::Float32(v) if v.fract() == 0.0 => Some(*v as i64),
        ScalarValue::Float64(v) if v.fract() == 0.0 => Some(*v as i64),
        _ => None,
    }
}

/// Coerce a numeric [`ScalarValue`] to `f64`.  Non-numeric scalars return `None`.
fn scalar_as_f64(value: &ScalarValue) -> Option<f64> {
    match value {
        ScalarValue::Int32(v) => Some(f64::from(*v)),
        ScalarValue::Int64(v) => Some(*v as f64),
        ScalarValue::Float32(v) => Some(f64::from(*v)),
        ScalarValue::Float64(v) => Some(*v),
        _ => None,
    }
}

/// Compare an integer-typed column against an `i64` scalar, widening the column
/// to `Int64` first when it is a narrower integer type.
///
/// Widening is a plain element map — deliberately *not* arrow's heavyweight
/// `cast` kernel, which would pull the full cross-type cast machinery into the
/// wasm binary (~3 MB) for a rarely-taken path.  It is lossless for every
/// integer type except `UInt64` values above `i64::MAX`, which are vanishingly
/// rare in geoparquet attribute columns.
fn cmp_int_column(
    col: &dyn arrow_array::Array,
    iv: i64,
    kernel: CmpKernel,
) -> Result<BooleanArray> {
    use arrow_array::Int64Array;
    use arrow_array::cast::AsArray;
    use arrow_array::types::{
        Int8Type, Int16Type, Int32Type, Int64Type, UInt8Type, UInt16Type, UInt32Type, UInt64Type,
    };

    let scalar = Int64Array::new_scalar(iv);
    let dt = col.data_type();

    // Fast path: an `Int64` column compares directly (no allocation).
    if matches!(dt, DataType::Int64) {
        let arr = col
            .as_primitive_opt::<Int64Type>()
            .ok_or_else(|| GeoParquetError::type_mismatch("Int64", format!("{dt:?}")))?;
        return kernel(arr, &scalar).map_err(GeoParquetError::Arrow);
    }

    macro_rules! widen {
        ($t:ty, $conv:expr) => {{
            let arr = col
                .as_primitive_opt::<$t>()
                .ok_or_else(|| GeoParquetError::type_mismatch("integer", format!("{dt:?}")))?;
            arr.iter().map(|o| o.map($conv)).collect::<Int64Array>()
        }};
    }

    let widened: Int64Array = match dt {
        DataType::Int32 => widen!(Int32Type, i64::from),
        DataType::Int16 => widen!(Int16Type, i64::from),
        DataType::Int8 => widen!(Int8Type, i64::from),
        DataType::UInt32 => widen!(UInt32Type, i64::from),
        DataType::UInt16 => widen!(UInt16Type, i64::from),
        DataType::UInt8 => widen!(UInt8Type, i64::from),
        DataType::UInt64 => widen!(UInt64Type, |v| v as i64),
        other => {
            return Err(GeoParquetError::type_mismatch(
                "integer column",
                format!("{other:?}"),
            ));
        }
    };
    kernel(&widened, &scalar).map_err(GeoParquetError::Arrow)
}

/// Compare a numeric column against an `f64` scalar in `f64` space, widening the
/// column to `Float64` first with a plain element map (no arrow `cast`
/// machinery, keeping the wasm binary small).
///
/// Accepts any numeric column type: a `Float64` column compares directly, while
/// `Float32` and every integer width are widened.  This path serves both float
/// columns and the fractional-float-literal-vs-integer-column fallback in
/// [`compare_scalar`].
fn cmp_float_column(
    col: &dyn arrow_array::Array,
    fv: f64,
    kernel: CmpKernel,
) -> Result<BooleanArray> {
    use arrow_array::Float64Array;
    use arrow_array::cast::AsArray;
    use arrow_array::types::{
        Float32Type, Float64Type, Int8Type, Int16Type, Int32Type, Int64Type, UInt8Type, UInt16Type,
        UInt32Type, UInt64Type,
    };

    let scalar = Float64Array::new_scalar(fv);
    let dt = col.data_type();

    // Fast path: a `Float64` column compares directly (no allocation).
    if matches!(dt, DataType::Float64) {
        let arr = col
            .as_primitive_opt::<Float64Type>()
            .ok_or_else(|| GeoParquetError::type_mismatch("Float64", format!("{dt:?}")))?;
        return kernel(arr, &scalar).map_err(GeoParquetError::Arrow);
    }

    macro_rules! widen {
        ($t:ty, $conv:expr) => {{
            let arr = col
                .as_primitive_opt::<$t>()
                .ok_or_else(|| GeoParquetError::type_mismatch("numeric", format!("{dt:?}")))?;
            arr.iter().map(|o| o.map($conv)).collect::<Float64Array>()
        }};
    }

    let widened: Float64Array = match dt {
        DataType::Float32 => widen!(Float32Type, f64::from),
        DataType::Int64 => widen!(Int64Type, |v| v as f64),
        DataType::Int32 => widen!(Int32Type, f64::from),
        DataType::Int16 => widen!(Int16Type, f64::from),
        DataType::Int8 => widen!(Int8Type, f64::from),
        DataType::UInt64 => widen!(UInt64Type, |v| v as f64),
        DataType::UInt32 => widen!(UInt32Type, f64::from),
        DataType::UInt16 => widen!(UInt16Type, f64::from),
        DataType::UInt8 => widen!(UInt8Type, f64::from),
        other => {
            return Err(GeoParquetError::type_mismatch(
                "numeric column",
                format!("{other:?}"),
            ));
        }
    };
    kernel(&widened, &scalar).map_err(GeoParquetError::Arrow)
}

/// Compare every element of `col` against a single scalar `value` using the
/// arrow comparison `kernel`, coercing the literal to the column's type.
///
/// GeoParquet attribute filters are commonly authored with bare integer
/// literals (`area_in_meters > 1000`) even when the target column is `Float64`.
/// Arrow's comparison kernels require the scalar and array element types to
/// match exactly, so this bridges the gap by dispatching on the *column* type
/// (not the literal type):
///
/// * **Float column** (`Float32` / `Float64`) — any numeric literal is widened
///   to `f64` and compared in `f64` space.
/// * **Integer column** — an integer or whole-valued float literal is compared
///   exactly in `i64` space; a fractional-float literal falls back to an `f64`
///   comparison so `id > 3.5` is not silently truncated to `id > 3`.
/// * **Utf8 column** — a [`ScalarValue::Utf8`] literal is compared directly.
///
/// A non-numeric literal against a numeric column (or vice versa) yields a
/// [`GeoParquetError::TypeMismatch`].
fn compare_scalar(
    col: &dyn arrow_array::Array,
    value: &ScalarValue,
    kernel: CmpKernel,
) -> Result<BooleanArray> {
    use arrow_array::StringArray;

    let dt = col.data_type();

    if matches!(dt, DataType::Boolean) {
        return match value {
            ScalarValue::Bool(b) => {
                let arr = col
                    .as_any()
                    .downcast_ref::<BooleanArray>()
                    .ok_or_else(|| GeoParquetError::type_mismatch("Boolean", format!("{dt:?}")))?;
                kernel(arr, &BooleanArray::new_scalar(*b)).map_err(GeoParquetError::Arrow)
            }
            other => Err(GeoParquetError::type_mismatch(
                "Boolean",
                format!("{other:?}"),
            )),
        };
    }

    if matches!(dt, DataType::Utf8) {
        return match value {
            ScalarValue::Utf8(s) => {
                let arr = col
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .ok_or_else(|| GeoParquetError::type_mismatch("Utf8", format!("{dt:?}")))?;
                kernel(arr, &StringArray::new_scalar(s.as_str())).map_err(GeoParquetError::Arrow)
            }
            other => Err(GeoParquetError::type_mismatch("Utf8", format!("{other:?}"))),
        };
    }

    if dt.is_integer() {
        if let Some(iv) = scalar_as_i64(value) {
            return cmp_int_column(col, iv, kernel);
        }
        if let Some(fv) = scalar_as_f64(value) {
            // Fractional-float literal vs integer column → compare in f64 space.
            return cmp_float_column(col, fv, kernel);
        }
        return Err(GeoParquetError::type_mismatch(
            "numeric literal (Int64/Float64)",
            format!("{value:?}"),
        ));
    }

    if dt.is_floating() {
        if let Some(fv) = scalar_as_f64(value) {
            return cmp_float_column(col, fv, kernel);
        }
        return Err(GeoParquetError::type_mismatch(
            "numeric literal (Int64/Float64)",
            format!("{value:?}"),
        ));
    }

    Err(GeoParquetError::type_mismatch(
        "Int64/Float64/Utf8/Boolean column",
        format!("{dt:?}"),
    ))
}

/// Evaluate a per-row equality over a column array returning a `BooleanArray`.
fn eval_eq_array(col: &dyn arrow_array::Array, value: &ScalarValue) -> Result<BooleanArray> {
    use arrow::compute::kernels::cmp::eq;
    compare_scalar(col, value, eq)
}

/// Evaluate inclusive range `[lo, hi]` on an array.
///
/// Each bound is coerced to the column's type independently (see
/// [`compare_scalar`]), so a mixed-literal range such as `BETWEEN 1000 AND
/// 2500.5` against a `Float64` column, or integer bounds against a `Float64`
/// column, are all handled.
fn eval_range_array(
    col: &dyn arrow_array::Array,
    lo: &ScalarValue,
    hi: &ScalarValue,
) -> Result<BooleanArray> {
    use arrow::compute::and;
    use arrow::compute::kernels::cmp::{gt_eq, lt_eq};

    let ge = compare_scalar(col, lo, gt_eq)?;
    let le = compare_scalar(col, hi, lt_eq)?;
    and(&ge, &le).map_err(GeoParquetError::Arrow)
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

/// Evaluate a scalar comparison `col <op> value`, returning a `BooleanArray`.
fn eval_cmp_array(
    col: &dyn arrow_array::Array,
    op: CmpOp,
    value: &ScalarValue,
) -> Result<BooleanArray> {
    use arrow::compute::kernels::cmp::{gt, gt_eq, lt, lt_eq, neq};

    let kernel: CmpKernel = match op {
        CmpOp::Gt => gt,
        CmpOp::Ge => gt_eq,
        CmpOp::Lt => lt,
        CmpOp::Le => lt_eq,
        CmpOp::NotEq => neq,
    };
    compare_scalar(col, value, kernel)
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

struct CmpPredicate {
    col: String,
    op: CmpOp,
    value: ScalarValue,
    data_type: DataType,
    projection: ProjectionMask,
}

impl ArrowPredicate for CmpPredicate {
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
        eval_cmp_array(col, self.op, &self.value)
            .map_err(|e| ArrowError::ExternalError(Box::new(e)))
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

        let xmin = find_f64_array(batch, &self.xmin_col)?;
        let ymin = find_f64_array(batch, &self.ymin_col)?;
        let xmax = find_f64_array(batch, &self.xmax_col)?;
        let ymax = find_f64_array(batch, &self.ymax_col)?;

        // row_xmax >= qxmin
        let q_xmin = arrow_array::Float64Array::new_scalar(self.qxmin);
        let c1 = gt_eq(xmax, &q_xmin).map_err(GeoParquetError::Arrow)?;

        // row_xmin <= qxmax
        let q_xmax = arrow_array::Float64Array::new_scalar(self.qxmax);
        let c2 = lt_eq(xmin, &q_xmax).map_err(GeoParquetError::Arrow)?;

        // row_ymax >= qymin
        let q_ymin = arrow_array::Float64Array::new_scalar(self.qymin);
        let c3 = gt_eq(ymax, &q_ymin).map_err(GeoParquetError::Arrow)?;

        // row_ymin <= qymax
        let q_ymax = arrow_array::Float64Array::new_scalar(self.qymax);
        let c4 = lt_eq(ymin, &q_ymax).map_err(GeoParquetError::Arrow)?;

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

/// Locate a `Float64Array` leaf named `name` in `batch`.
///
/// Covering bbox columns may appear either as flat top-level columns (e.g.
/// `geometry_bbox_xmin`) or as fields inside a struct column (e.g. `bbox.xmin`,
/// the VIDA / GeoParquet 1.1 layout).  When a covering column is projected out
/// of a struct root, the reconstructed [`RecordBatch`] carries the struct as a
/// single top-level [`StructArray`] column, so a plain `column_by_name` lookup
/// on the leaf name misses.  This helper first tries the top level and then
/// descends one level into any struct column.
fn find_f64_array<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a arrow_array::Float64Array> {
    use arrow_array::{Float64Array, StructArray};

    if let Some(col) = batch.column_by_name(name)
        && let Some(arr) = col.as_any().downcast_ref::<Float64Array>()
    {
        return Ok(arr);
    }
    for col in batch.columns() {
        if let Some(st) = col.as_any().downcast_ref::<StructArray>()
            && let Some(child) = st.column_by_name(name)
            && let Some(arr) = child.as_any().downcast_ref::<Float64Array>()
        {
            return Ok(arr);
        }
    }
    Err(GeoParquetError::missing_field(name))
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
    fn test_eval_cmp_gt_int() {
        let batch = int_batch(&[100, 500_000, 1_000_000, 250_000]);
        let col = batch.column(0).as_ref();
        let result = eval_cmp_array(col, CmpOp::Gt, &ScalarValue::Int64(250_000)).expect("eval");
        assert!(!result.value(0));
        assert!(result.value(1));
        assert!(result.value(2));
        assert!(!result.value(3)); // 250000 > 250000 is false
    }

    #[test]
    fn test_eval_cmp_le_float() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "area",
            DataType::Float64,
            true,
        )]));
        let batch = RecordBatch::try_new(
            schema,
            vec![Arc::new(Float64Array::from(vec![10.0, 1000.0, 2500.5]))],
        )
        .expect("batch");
        let col = batch.column(0).as_ref();
        let result = eval_cmp_array(col, CmpOp::Le, &ScalarValue::Float64(1000.0)).expect("eval");
        assert!(result.value(0));
        assert!(result.value(1));
        assert!(!result.value(2));
    }

    #[test]
    fn test_eval_cmp_noteq_utf8() {
        let batch = string_batch(&["alpha", "beta", "alpha"]);
        let col = batch.column(0).as_ref();
        let result =
            eval_cmp_array(col, CmpOp::NotEq, &ScalarValue::Utf8("alpha".into())).expect("eval");
        assert!(!result.value(0));
        assert!(result.value(1));
        assert!(!result.value(2));
    }

    // ── Numeric literal ↔ column coercion (the `area_in_meters > 1000` defect) ───

    /// A `Float64Array` fixture: `area_in_meters`-like values.
    fn f64_array(values: &[f64]) -> Float64Array {
        Float64Array::from(values.to_vec())
    }

    /// Integer equality literal against a `Float64` column must coerce, not error.
    #[test]
    fn test_eval_eq_int_literal_vs_float_column() {
        let arr = f64_array(&[1000.0, 2000.0, 2000.0]);
        let result = eval_eq_array(&arr, &ScalarValue::Int64(2000)).expect("coerced eq");
        assert!(!result.value(0));
        assert!(result.value(1));
        assert!(result.value(2));
    }

    /// Every comparison operator must coerce an `Int64` literal to the `Float64`
    /// column instead of raising `Type mismatch: expected Int64, found Float64`.
    #[test]
    fn test_eval_cmp_int_literal_vs_float_column_all_ops() {
        let arr = f64_array(&[500.0, 1000.0, 1500.0]);

        let gt = eval_cmp_array(&arr, CmpOp::Gt, &ScalarValue::Int64(1000)).expect("gt");
        assert_eq!(
            (gt.value(0), gt.value(1), gt.value(2)),
            (false, false, true)
        );

        let ge = eval_cmp_array(&arr, CmpOp::Ge, &ScalarValue::Int64(1000)).expect("ge");
        assert_eq!((ge.value(0), ge.value(1), ge.value(2)), (false, true, true));

        let lt = eval_cmp_array(&arr, CmpOp::Lt, &ScalarValue::Int64(1000)).expect("lt");
        assert_eq!(
            (lt.value(0), lt.value(1), lt.value(2)),
            (true, false, false)
        );

        let le = eval_cmp_array(&arr, CmpOp::Le, &ScalarValue::Int64(1000)).expect("le");
        assert_eq!((le.value(0), le.value(1), le.value(2)), (true, true, false));

        let ne = eval_cmp_array(&arr, CmpOp::NotEq, &ScalarValue::Int64(1000)).expect("ne");
        assert_eq!((ne.value(0), ne.value(1), ne.value(2)), (true, false, true));
    }

    /// Integer range bounds against a `Float64` column coerce both ends.
    #[test]
    fn test_eval_range_int_literal_vs_float_column() {
        let arr = f64_array(&[999.5, 1000.0, 2000.0, 3000.0, 3000.5]);
        let result = eval_range_array(&arr, &ScalarValue::Int64(1000), &ScalarValue::Int64(3000))
            .expect("coerced range");
        assert!(!result.value(0)); // 999.5 < 1000
        assert!(result.value(1)); // 1000
        assert!(result.value(2)); // 2000
        assert!(result.value(3)); // 3000
        assert!(!result.value(4)); // 3000.5 > 3000
    }

    /// Mixed-type IN list (Int64 + Float64) against a `Float64` column.
    #[test]
    fn test_eval_in_mixed_literals_vs_float_column() {
        let arr = f64_array(&[1000.0, 2500.5, 9003.0, 42.0]);
        let values = vec![
            ScalarValue::Int64(1000),
            ScalarValue::Float64(2500.5),
            ScalarValue::Int64(9003),
        ];
        let result = eval_in_array(&arr, &values).expect("coerced in");
        assert!(result.value(0));
        assert!(result.value(1));
        assert!(result.value(2));
        assert!(!result.value(3));
    }

    /// The reverse direction: a whole-valued `Float64` literal against an `Int64`
    /// column coerces to exact integer comparison.
    #[test]
    fn test_eval_cmp_whole_float_literal_vs_int_column() {
        let arr = Int64Array::from(vec![1000i64, 2000, 3000]);
        let result = eval_cmp_array(&arr, CmpOp::Ge, &ScalarValue::Float64(2000.0)).expect("ge");
        assert!(!result.value(0));
        assert!(result.value(1));
        assert!(result.value(2));
    }

    /// A *fractional* float literal against an `Int64` column must compare in
    /// float space (not truncate to an integer bound): `col >= 3.5` excludes 3.
    #[test]
    fn test_eval_cmp_fractional_float_vs_int_column() {
        let arr = Int64Array::from(vec![3i64, 4]);
        let result = eval_cmp_array(&arr, CmpOp::Ge, &ScalarValue::Float64(3.5)).expect("ge");
        assert!(!result.value(0), "3 >= 3.5 must be false (no truncation)");
        assert!(result.value(1), "4 >= 3.5 must be true");
    }

    /// Int64 literal against a `Float32` column (narrower float) coerces via cast.
    #[test]
    fn test_eval_cmp_int_literal_vs_float32_column() {
        let arr = arrow_array::Float32Array::from(vec![500.0f32, 1000.0, 1500.0]);
        let result = eval_cmp_array(&arr, CmpOp::Gt, &ScalarValue::Int64(1000)).expect("gt");
        assert!(!result.value(0));
        assert!(!result.value(1));
        assert!(result.value(2));
    }

    /// Int64 literal against an `Int32` column (narrower int) coerces via cast.
    #[test]
    fn test_eval_cmp_int_literal_vs_int32_column() {
        let arr = arrow_array::Int32Array::from(vec![10i32, 100, 1000]);
        let result = eval_cmp_array(&arr, CmpOp::Ge, &ScalarValue::Int64(100)).expect("ge");
        assert!(!result.value(0));
        assert!(result.value(1));
        assert!(result.value(2));
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

    // ── Boolean column predicate evaluation ─────────────────────────────────────

    fn bool_array(values: &[bool]) -> BooleanArray {
        BooleanArray::from(values.to_vec())
    }

    /// `Eq` with a `Bool` literal selects the matching rows of a `Boolean` column.
    #[test]
    fn test_eval_eq_bool_true() {
        let arr = bool_array(&[true, false, true, false]);
        let result = eval_eq_array(&arr, &ScalarValue::Bool(true)).expect("bool eq");
        assert!(result.value(0));
        assert!(!result.value(1));
        assert!(result.value(2));
        assert!(!result.value(3));
    }

    /// `Eq` with `Bool(false)` selects the `false` rows.
    #[test]
    fn test_eval_eq_bool_false() {
        let arr = bool_array(&[true, false, true, false]);
        let result = eval_eq_array(&arr, &ScalarValue::Bool(false)).expect("bool eq");
        assert!(!result.value(0));
        assert!(result.value(1));
        assert!(!result.value(2));
        assert!(result.value(3));
    }

    /// `In` over a `Boolean` column routes through the same dispatch point.
    #[test]
    fn test_eval_in_bool() {
        let arr = bool_array(&[true, false, true]);
        let result = eval_in_array(&arr, &[ScalarValue::Bool(false)]).expect("bool in");
        assert!(!result.value(0));
        assert!(result.value(1));
        assert!(!result.value(2));
    }

    /// `Cmp` on booleans uses arrow's `false < true` ordering.
    #[test]
    fn test_eval_cmp_bool_ordering() {
        let arr = bool_array(&[false, true]);
        // col > false → only `true` matches.
        let gt = eval_cmp_array(&arr, CmpOp::Gt, &ScalarValue::Bool(false)).expect("bool gt");
        assert!(!gt.value(0));
        assert!(gt.value(1));
        // col >= false → both match.
        let ge = eval_cmp_array(&arr, CmpOp::Ge, &ScalarValue::Bool(false)).expect("bool ge");
        assert!(ge.value(0));
        assert!(ge.value(1));
    }

    /// A non-`Bool` literal against a `Boolean` column returns `TypeMismatch`
    /// (not a panic).
    #[test]
    fn test_bool_column_non_bool_literal_type_mismatch() {
        let arr = bool_array(&[true, false]);
        let err = eval_eq_array(&arr, &ScalarValue::Int64(1)).expect_err("must mismatch");
        assert!(matches!(err, GeoParquetError::TypeMismatch { .. }));
    }

    /// A `Bool` literal against a non-`Boolean` column returns `TypeMismatch`.
    #[test]
    fn test_bool_literal_non_bool_column_type_mismatch() {
        let arr = Int64Array::from(vec![1i64, 0]);
        let err = eval_eq_array(&arr, &ScalarValue::Bool(true)).expect_err("must mismatch");
        assert!(matches!(err, GeoParquetError::TypeMismatch { .. }));
    }
}
