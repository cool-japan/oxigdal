//! Concrete Arrow-compute implementations of distributed task operations.
//!
//! These functions perform the *real* work behind [`crate::task::TaskOperation`]
//! variants that can be expressed purely in terms of an Arrow [`RecordBatch`]:
//! row filtering, normalized-difference spectral indices, and spatial bounding-box
//! clipping. Operations that require the raster/CRS engines (reproject, resample,
//! convolve) are intentionally *not* silently passed through — the worker returns a
//! typed [`crate::error::DistributedError::OperationNotImplemented`] for those so a
//! skipped operation can never be reported to the coordinator as a completed one.

use crate::error::{DistributedError, Result};
use arrow::array::{
    Array, ArrayRef, BooleanArray, BooleanBuilder, Float64Array, Float64Builder, StringArray,
};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use std::sync::Arc;

/// Comparison operator parsed out of a filter predicate string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Return `true` for the numeric Arrow data types we can coerce to `Float64`.
fn is_numeric(dt: &DataType) -> bool {
    matches!(
        dt,
        DataType::Int8
            | DataType::Int16
            | DataType::Int32
            | DataType::Int64
            | DataType::UInt8
            | DataType::UInt16
            | DataType::UInt32
            | DataType::UInt64
            | DataType::Float16
            | DataType::Float32
            | DataType::Float64
    )
}

/// Parse a simple `column <op> literal` predicate.
///
/// Supports the operators `>=`, `<=`, `!=`, `<>`, `==`, `>`, `<`, `=`. Two-character
/// operators are matched before single-character ones so that `>=` is not mistaken
/// for `>`.
fn parse_predicate(expr: &str) -> Result<(String, CmpOp, String)> {
    let trimmed = expr.trim();

    // Two-character operators must be tested first.
    const TWO_CHAR: [(&str, CmpOp); 5] = [
        (">=", CmpOp::Ge),
        ("<=", CmpOp::Le),
        ("!=", CmpOp::Ne),
        ("<>", CmpOp::Ne),
        ("==", CmpOp::Eq),
    ];
    for (sym, op) in TWO_CHAR {
        if let Some(idx) = trimmed.find(sym) {
            let (lhs, rest) = trimmed.split_at(idx);
            let rhs = &rest[sym.len()..];
            return finish_predicate(lhs, op, rhs);
        }
    }

    const ONE_CHAR: [(&str, CmpOp); 3] = [(">", CmpOp::Gt), ("<", CmpOp::Lt), ("=", CmpOp::Eq)];
    for (sym, op) in ONE_CHAR {
        if let Some(idx) = trimmed.find(sym) {
            let (lhs, rest) = trimmed.split_at(idx);
            let rhs = &rest[sym.len()..];
            return finish_predicate(lhs, op, rhs);
        }
    }

    Err(DistributedError::invalid_operation(format!(
        "Unsupported filter expression: '{}' (expected 'column <op> value')",
        expr
    )))
}

fn finish_predicate(lhs: &str, op: CmpOp, rhs: &str) -> Result<(String, CmpOp, String)> {
    let column = lhs.trim();
    let literal = rhs.trim();
    if column.is_empty() || literal.is_empty() {
        return Err(DistributedError::invalid_operation(
            "Filter expression must have a column on the left and a value on the right",
        ));
    }
    Ok((column.to_string(), op, literal.to_string()))
}

fn cmp_f64(op: CmpOp, a: f64, b: f64) -> bool {
    match op {
        CmpOp::Eq => a == b,
        CmpOp::Ne => a != b,
        CmpOp::Lt => a < b,
        CmpOp::Le => a <= b,
        CmpOp::Gt => a > b,
        CmpOp::Ge => a >= b,
    }
}

fn cmp_str(op: CmpOp, a: &str, b: &str) -> bool {
    match op {
        CmpOp::Eq => a == b,
        CmpOp::Ne => a != b,
        CmpOp::Lt => a < b,
        CmpOp::Le => a <= b,
        CmpOp::Gt => a > b,
        CmpOp::Ge => a >= b,
    }
}

/// Cast an arbitrary numeric column to a `Float64Array`.
fn column_as_f64(column: &ArrayRef) -> Result<Float64Array> {
    let casted = arrow::compute::cast(column, &DataType::Float64)?;
    casted
        .as_any()
        .downcast_ref::<Float64Array>()
        .cloned()
        .ok_or_else(|| DistributedError::arrow("Failed to cast column to Float64"))
}

/// Apply a filter predicate to `batch`, returning only the rows that match.
///
/// The predicate is a `column <op> literal` string. Numeric columns are compared
/// after coercion to `f64`; UTF-8 columns are compared lexically (the literal may be
/// wrapped in single or double quotes). Null values never match.
pub fn apply_filter(batch: &RecordBatch, expression: &str) -> Result<RecordBatch> {
    let (name, op, literal) = parse_predicate(expression)?;

    let column = batch.column_by_name(&name).ok_or_else(|| {
        DistributedError::invalid_operation(format!("Filter references unknown column '{}'", name))
    })?;

    let mask = if is_numeric(column.data_type()) {
        let value: f64 = literal.parse().map_err(|_| {
            DistributedError::invalid_operation(format!(
                "Filter literal '{}' is not a valid number for numeric column '{}'",
                literal, name
            ))
        })?;
        let values = column_as_f64(column)?;
        let mut builder = BooleanBuilder::with_capacity(values.len());
        for i in 0..values.len() {
            if values.is_null(i) {
                builder.append_value(false);
            } else {
                builder.append_value(cmp_f64(op, values.value(i), value));
            }
        }
        builder.finish()
    } else if matches!(column.data_type(), DataType::Utf8 | DataType::LargeUtf8) {
        let literal = literal.trim_matches(|c| c == '\'' || c == '"').to_string();
        let casted = arrow::compute::cast(column, &DataType::Utf8)?;
        let values = casted
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| DistributedError::arrow("Failed to cast column to Utf8"))?;
        let mut builder = BooleanBuilder::with_capacity(values.len());
        for i in 0..values.len() {
            if values.is_null(i) {
                builder.append_value(false);
            } else {
                builder.append_value(cmp_str(op, values.value(i), &literal));
            }
        }
        builder.finish()
    } else {
        return Err(DistributedError::invalid_operation(format!(
            "Filter does not support column '{}' of type {:?}",
            name,
            column.data_type()
        )));
    };

    let filtered = arrow::compute::filter_record_batch(batch, &mask)?;
    Ok(filtered)
}

/// Compute a two-band normalized-difference spectral index (NDVI, NDWI, NDBI, ...).
///
/// `bands` must contain exactly two column indices `[a, b]`; the result is
/// `(col[b] - col[a]) / (col[b] + col[a])`, matching the NDVI convention of passing
/// `[red, nir]`. A new `Float64` column named after `index_type` is appended to the
/// batch. Rows with a zero denominator or a null input yield a null result.
pub fn calculate_index(
    batch: &RecordBatch,
    index_type: &str,
    bands: &[usize],
) -> Result<RecordBatch> {
    if bands.len() != 2 {
        return Err(DistributedError::invalid_operation(format!(
            "Index '{}' requires exactly 2 band column indices, got {}",
            index_type,
            bands.len()
        )));
    }

    let num_columns = batch.num_columns();
    for &b in bands {
        if b >= num_columns {
            return Err(DistributedError::invalid_operation(format!(
                "Band index {} is out of range (batch has {} columns)",
                b, num_columns
            )));
        }
    }

    let a = column_as_f64(batch.column(bands[0]))?;
    let b = column_as_f64(batch.column(bands[1]))?;

    let len = batch.num_rows();
    let mut builder = Float64Builder::with_capacity(len);
    for i in 0..len {
        if a.is_null(i) || b.is_null(i) {
            builder.append_null();
            continue;
        }
        let va = a.value(i);
        let vb = b.value(i);
        let denom = vb + va;
        if denom == 0.0 {
            builder.append_null();
        } else {
            builder.append_value((vb - va) / denom);
        }
    }
    let index_column = builder.finish();

    let mut fields: Vec<Arc<Field>> = batch.schema().fields().iter().cloned().collect();
    fields.push(Arc::new(Field::new(index_type, DataType::Float64, true)));
    let new_schema = Arc::new(Schema::new(fields));

    let mut columns = batch.columns().to_vec();
    columns.push(Arc::new(index_column) as ArrayRef);

    let result = RecordBatch::try_new(new_schema, columns)?;
    Ok(result)
}

/// Candidate coordinate column-name pairs, tried case-insensitively in order.
const XY_CANDIDATES: [(&str, &str); 3] = [("x", "y"), ("longitude", "latitude"), ("lon", "lat")];

/// Locate the `(x, y)` coordinate columns of `batch`, returning them coerced to
/// `Float64`.
fn find_xy(batch: &RecordBatch) -> Result<(Float64Array, Float64Array)> {
    let schema = batch.schema();
    for (xn, yn) in XY_CANDIDATES {
        let x_idx = schema
            .fields()
            .iter()
            .position(|f| f.name().eq_ignore_ascii_case(xn));
        let y_idx = schema
            .fields()
            .iter()
            .position(|f| f.name().eq_ignore_ascii_case(yn));
        if let (Some(xi), Some(yi)) = (x_idx, y_idx) {
            let x = column_as_f64(batch.column(xi))?;
            let y = column_as_f64(batch.column(yi))?;
            return Ok((x, y));
        }
    }
    Err(DistributedError::invalid_operation(
        "Clip requires coordinate columns (x/y, longitude/latitude, or lon/lat)",
    ))
}

/// Clip `batch` to a bounding box, keeping only rows whose `(x, y)` coordinate falls
/// within `[min_x, max_x] x [min_y, max_y]` (inclusive). Rows with a null coordinate
/// are dropped.
pub fn clip_bbox(
    batch: &RecordBatch,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
) -> Result<RecordBatch> {
    let (x, y) = find_xy(batch)?;

    let len = batch.num_rows();
    let mut builder = BooleanBuilder::with_capacity(len);
    for i in 0..len {
        if x.is_null(i) || y.is_null(i) {
            builder.append_value(false);
            continue;
        }
        let xi = x.value(i);
        let yi = y.value(i);
        let inside = xi >= min_x && xi <= max_x && yi >= min_y && yi <= max_y;
        builder.append_value(inside);
    }
    let mask: BooleanArray = builder.finish();

    let clipped = arrow::compute::filter_record_batch(batch, &mask)?;
    Ok(clipped)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use arrow::array::{Float64Array, Int32Array, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};

    fn int_batch() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "value",
            DataType::Int32,
            false,
        )]));
        let array = Int32Array::from(vec![1, 2, 3, 4, 5]);
        RecordBatch::try_new(schema, vec![Arc::new(array)]).unwrap()
    }

    #[test]
    fn test_parse_predicate_ops() {
        assert_eq!(
            parse_predicate("value >= 2").unwrap(),
            ("value".to_string(), CmpOp::Ge, "2".to_string())
        );
        assert_eq!(
            parse_predicate("name == 'foo'").unwrap(),
            ("name".to_string(), CmpOp::Eq, "'foo'".to_string())
        );
        assert_eq!(
            parse_predicate("a>1").unwrap(),
            ("a".to_string(), CmpOp::Gt, "1".to_string())
        );
        assert!(parse_predicate("nonsense").is_err());
        assert!(parse_predicate("value > ").is_err());
    }

    #[test]
    fn test_filter_reduces_rows() {
        let batch = int_batch();
        let out = apply_filter(&batch, "value > 2").unwrap();
        assert_eq!(out.num_rows(), 3);
        let col = out.column(0).as_any().downcast_ref::<Int32Array>().unwrap();
        assert_eq!(col.values(), &[3, 4, 5]);
    }

    #[test]
    fn test_filter_equality_and_range() {
        let batch = int_batch();
        assert_eq!(apply_filter(&batch, "value == 3").unwrap().num_rows(), 1);
        assert_eq!(apply_filter(&batch, "value <= 2").unwrap().num_rows(), 2);
        assert_eq!(apply_filter(&batch, "value != 5").unwrap().num_rows(), 4);
    }

    #[test]
    fn test_filter_string_column() {
        let schema = Arc::new(Schema::new(vec![Field::new("name", DataType::Utf8, false)]));
        let array = StringArray::from(vec!["a", "b", "c", "b"]);
        let batch = RecordBatch::try_new(schema, vec![Arc::new(array)]).unwrap();
        let out = apply_filter(&batch, "name == 'b'").unwrap();
        assert_eq!(out.num_rows(), 2);
    }

    #[test]
    fn test_filter_unknown_column_errors() {
        let batch = int_batch();
        assert!(apply_filter(&batch, "missing > 1").is_err());
    }

    #[test]
    fn test_filter_non_numeric_literal_errors() {
        let batch = int_batch();
        assert!(apply_filter(&batch, "value > abc").is_err());
    }

    #[test]
    fn test_calculate_ndvi() {
        // red = [1, 2], nir = [3, 6] -> ndvi = (nir-red)/(nir+red) = [0.5, 0.5]
        let schema = Arc::new(Schema::new(vec![
            Field::new("red", DataType::Float64, false),
            Field::new("nir", DataType::Float64, false),
        ]));
        let red = Float64Array::from(vec![1.0, 2.0]);
        let nir = Float64Array::from(vec![3.0, 6.0]);
        let batch = RecordBatch::try_new(schema, vec![Arc::new(red), Arc::new(nir)]).unwrap();

        let out = calculate_index(&batch, "NDVI", &[0, 1]).unwrap();
        assert_eq!(out.num_columns(), 3);
        assert_eq!(out.schema().field(2).name(), "NDVI");
        let ndvi = out
            .column(2)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((ndvi.value(0) - 0.5).abs() < 1e-12);
        assert!((ndvi.value(1) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn test_calculate_index_zero_denominator_null() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("a", DataType::Float64, false),
            Field::new("b", DataType::Float64, false),
        ]));
        let a = Float64Array::from(vec![0.0]);
        let b = Float64Array::from(vec![0.0]);
        let batch = RecordBatch::try_new(schema, vec![Arc::new(a), Arc::new(b)]).unwrap();
        let out = calculate_index(&batch, "ND", &[0, 1]).unwrap();
        let col = out
            .column(2)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!(col.is_null(0));
    }

    #[test]
    fn test_calculate_index_bad_band_count() {
        let batch = int_batch();
        assert!(calculate_index(&batch, "NDVI", &[0]).is_err());
    }

    #[test]
    fn test_calculate_index_out_of_range() {
        let batch = int_batch();
        assert!(calculate_index(&batch, "NDVI", &[0, 9]).is_err());
    }

    #[test]
    fn test_clip_bbox() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("x", DataType::Float64, false),
            Field::new("y", DataType::Float64, false),
        ]));
        let x = Float64Array::from(vec![0.0, 5.0, 10.0, 15.0]);
        let y = Float64Array::from(vec![0.0, 5.0, 10.0, 15.0]);
        let batch = RecordBatch::try_new(schema, vec![Arc::new(x), Arc::new(y)]).unwrap();

        let out = clip_bbox(&batch, 1.0, 1.0, 11.0, 11.0).unwrap();
        assert_eq!(out.num_rows(), 2); // (5,5) and (10,10)
    }

    #[test]
    fn test_clip_missing_columns_errors() {
        let batch = int_batch();
        assert!(clip_bbox(&batch, 0.0, 0.0, 1.0, 1.0).is_err());
    }
}
