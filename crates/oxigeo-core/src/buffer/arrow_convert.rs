// Allow unsafe blocks inherited from the parent module (low-level typed slice conversions)
#![allow(unsafe_code)]

//! Arrow [`RecordBatch`] ↔ [`RasterBuffer`] conversions
//!
//! This module provides [`TryFrom`] implementations for converting between
//! [`RasterBuffer`] and Apache Arrow [`RecordBatch`].
//!
//! # Feature gate
//!
//! These conversions are only available when the `arrow` Cargo feature is enabled.
//!
//! # Layout
//!
//! A `RecordBatch` produced from a `RasterBuffer` has exactly **one column** named
//! `"pixel_values"`, with `width * height` rows in row-major order (row 0 first).
//! The schema carries three metadata entries:
//!
//! | Key         | Value                        |
//! |-------------|------------------------------|
//! | `"width"`   | decimal string of pixel width |
//! | `"height"`  | decimal string of pixel height |
//! | `"data_type"` | [`RasterDataType::name`] string |
//!
//! # NoData handling
//!
//! If the source buffer carries a `NoData` value, pixels whose value equals the
//! no-data sentinel are emitted as Arrow nulls.  When converting from a
//! `RecordBatch` back to a `RasterBuffer`, nulls are silently replaced by the
//! zero representation of the target type; the output buffer carries
//! `NoDataValue::None`.
//!
//! # Unsupported types
//!
//! `CFloat32` and `CFloat64` have no standard Arrow equivalent.  Attempting to
//! convert a buffer of either type returns
//! [`OxiGeoError::NotSupported`].

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{
    Array, ArrayRef, Float32Array, Float64Array, Int8Array, Int16Array, Int32Array, Int64Array,
    RecordBatch, UInt8Array, UInt16Array, UInt32Array, UInt64Array,
};
use arrow_schema::{DataType, Field, Schema};

use super::{OxiGeoError, RasterBuffer, Result};
use crate::types::{NoDataValue, RasterDataType};

// ─── RasterBuffer → RecordBatch ──────────────────────────────────────────────

impl TryFrom<&RasterBuffer> for RecordBatch {
    type Error = OxiGeoError;

    /// Converts a [`RasterBuffer`] into an Arrow [`RecordBatch`].
    ///
    /// # Errors
    ///
    /// - [`OxiGeoError::NotSupported`] – if the buffer's data type is `CFloat32`
    ///   or `CFloat64`, which have no standard Arrow equivalent.
    /// - [`OxiGeoError::Internal`] – if the underlying Arrow builder rejects the
    ///   constructed schema or column (should not happen in practice).
    fn try_from(buf: &RasterBuffer) -> Result<Self> {
        let n = (buf.width() * buf.height()) as usize;
        let nodata = buf.nodata();

        /// Returns `true` when the given `f64` pixel value should be treated as
        /// null (i.e. it matches the buffer's no-data sentinel).
        #[inline]
        fn is_nd(nodata: NoDataValue, v: f64) -> bool {
            match nodata.as_f64() {
                None => false,
                Some(nd) => {
                    if nd.is_nan() && v.is_nan() {
                        true
                    } else {
                        (nd - v).abs() < f64::EPSILON
                    }
                }
            }
        }

        let (array, arrow_dt): (ArrayRef, DataType) = match buf.data_type() {
            RasterDataType::UInt8 => {
                let vals: &[u8] = buf.as_slice::<u8>().map_err(|e| OxiGeoError::Internal {
                    message: e.to_string(),
                })?;
                let arr: UInt8Array = vals
                    .iter()
                    .map(|&v| {
                        if is_nd(nodata, f64::from(v)) {
                            None
                        } else {
                            Some(v)
                        }
                    })
                    .collect();
                (Arc::new(arr), DataType::UInt8)
            }
            RasterDataType::Int8 => {
                let vals: &[i8] = buf.as_slice::<i8>().map_err(|e| OxiGeoError::Internal {
                    message: e.to_string(),
                })?;
                let arr: Int8Array = vals
                    .iter()
                    .map(|&v| {
                        if is_nd(nodata, f64::from(v)) {
                            None
                        } else {
                            Some(v)
                        }
                    })
                    .collect();
                (Arc::new(arr), DataType::Int8)
            }
            RasterDataType::UInt16 => {
                let vals: &[u16] = buf.as_slice::<u16>().map_err(|e| OxiGeoError::Internal {
                    message: e.to_string(),
                })?;
                let arr: UInt16Array = vals
                    .iter()
                    .map(|&v| {
                        if is_nd(nodata, f64::from(v)) {
                            None
                        } else {
                            Some(v)
                        }
                    })
                    .collect();
                (Arc::new(arr), DataType::UInt16)
            }
            RasterDataType::Int16 => {
                let vals: &[i16] = buf.as_slice::<i16>().map_err(|e| OxiGeoError::Internal {
                    message: e.to_string(),
                })?;
                let arr: Int16Array = vals
                    .iter()
                    .map(|&v| {
                        if is_nd(nodata, f64::from(v)) {
                            None
                        } else {
                            Some(v)
                        }
                    })
                    .collect();
                (Arc::new(arr), DataType::Int16)
            }
            RasterDataType::UInt32 => {
                let vals: &[u32] = buf.as_slice::<u32>().map_err(|e| OxiGeoError::Internal {
                    message: e.to_string(),
                })?;
                let arr: UInt32Array = vals
                    .iter()
                    .map(|&v| {
                        if is_nd(nodata, f64::from(v)) {
                            None
                        } else {
                            Some(v)
                        }
                    })
                    .collect();
                (Arc::new(arr), DataType::UInt32)
            }
            RasterDataType::Int32 => {
                let vals: &[i32] = buf.as_slice::<i32>().map_err(|e| OxiGeoError::Internal {
                    message: e.to_string(),
                })?;
                let arr: Int32Array = vals
                    .iter()
                    .map(|&v| {
                        if is_nd(nodata, f64::from(v)) {
                            None
                        } else {
                            Some(v)
                        }
                    })
                    .collect();
                (Arc::new(arr), DataType::Int32)
            }
            RasterDataType::UInt64 => {
                let vals: &[u64] = buf.as_slice::<u64>().map_err(|e| OxiGeoError::Internal {
                    message: e.to_string(),
                })?;
                let arr: UInt64Array = vals
                    .iter()
                    .map(|&v| {
                        // u64 → f64 may lose precision for very large values, but nodata
                        // matching uses the same cast so the comparison is consistent.
                        if is_nd(nodata, v as f64) {
                            None
                        } else {
                            Some(v)
                        }
                    })
                    .collect();
                (Arc::new(arr), DataType::UInt64)
            }
            RasterDataType::Int64 => {
                let vals: &[i64] = buf.as_slice::<i64>().map_err(|e| OxiGeoError::Internal {
                    message: e.to_string(),
                })?;
                let arr: Int64Array = vals
                    .iter()
                    .map(|&v| {
                        if is_nd(nodata, v as f64) {
                            None
                        } else {
                            Some(v)
                        }
                    })
                    .collect();
                (Arc::new(arr), DataType::Int64)
            }
            RasterDataType::Float32 => {
                let vals: &[f32] = buf.as_slice::<f32>().map_err(|e| OxiGeoError::Internal {
                    message: e.to_string(),
                })?;
                let arr: Float32Array = vals
                    .iter()
                    .map(|&v| {
                        if is_nd(nodata, f64::from(v)) {
                            None
                        } else {
                            Some(v)
                        }
                    })
                    .collect();
                (Arc::new(arr), DataType::Float32)
            }
            RasterDataType::Float64 => {
                let vals: &[f64] = buf.as_slice::<f64>().map_err(|e| OxiGeoError::Internal {
                    message: e.to_string(),
                })?;
                let arr: Float64Array = vals
                    .iter()
                    .map(|&v| if is_nd(nodata, v) { None } else { Some(v) })
                    .collect();
                (Arc::new(arr), DataType::Float64)
            }
            RasterDataType::CFloat32 | RasterDataType::CFloat64 => {
                return Err(OxiGeoError::NotSupported {
                    operation: format!(
                        "Arrow conversion of complex type {}",
                        buf.data_type().name()
                    ),
                });
            }
        };

        debug_assert_eq!(array.len(), n, "array length must equal pixel count");

        let mut metadata: HashMap<String, String> = HashMap::with_capacity(3);
        metadata.insert("width".to_string(), buf.width().to_string());
        metadata.insert("height".to_string(), buf.height().to_string());
        metadata.insert("data_type".to_string(), buf.data_type().name().to_string());

        let field = Field::new("pixel_values", arrow_dt, true);
        let schema = Arc::new(Schema::new_with_metadata(vec![field], metadata));

        RecordBatch::try_new(schema, vec![array]).map_err(|e| OxiGeoError::Internal {
            message: format!("Arrow RecordBatch construction failed: {e}"),
        })
    }
}

// ─── RecordBatch → RasterBuffer ──────────────────────────────────────────────

impl TryFrom<RecordBatch> for RasterBuffer {
    type Error = OxiGeoError;

    /// Converts an Arrow [`RecordBatch`] back into a [`RasterBuffer`].
    ///
    /// The `RecordBatch` must have been produced by the `TryFrom<&RasterBuffer>`
    /// impl (or be schema-compatible): exactly one column named `"pixel_values"`
    /// and schema metadata containing `"width"`, `"height"`, and `"data_type"`.
    ///
    /// Arrow nulls in the column are replaced by the zero representation of the
    /// target type.  The returned buffer carries [`NoDataValue::None`].
    ///
    /// # Errors
    ///
    /// - [`OxiGeoError::InvalidParameter`] – wrong number of columns, wrong
    ///   column name, or missing / malformed schema metadata.
    /// - [`OxiGeoError::Internal`] – unexpected Arrow type mismatch (should not
    ///   occur for batches produced by this module).
    fn try_from(batch: RecordBatch) -> Result<Self> {
        // ── Validate schema ──────────────────────────────────────────────────

        if batch.num_columns() != 1 {
            return Err(OxiGeoError::InvalidParameter {
                parameter: "batch",
                message: format!("Expected exactly 1 column, got {}", batch.num_columns()),
            });
        }

        let schema = batch.schema();
        let field = schema.field(0);
        if field.name() != "pixel_values" {
            return Err(OxiGeoError::InvalidParameter {
                parameter: "batch",
                message: format!(
                    "Expected column name 'pixel_values', got '{}'",
                    field.name()
                ),
            });
        }

        let meta = schema.metadata();

        let width: u64 = meta
            .get("width")
            .ok_or(OxiGeoError::InvalidParameter {
                parameter: "batch",
                message: "Schema metadata missing 'width' key".to_string(),
            })?
            .parse::<u64>()
            .map_err(|e| OxiGeoError::InvalidParameter {
                parameter: "batch",
                message: format!("Schema metadata 'width' is not a valid u64: {e}"),
            })?;

        let height: u64 = meta
            .get("height")
            .ok_or(OxiGeoError::InvalidParameter {
                parameter: "batch",
                message: "Schema metadata missing 'height' key".to_string(),
            })?
            .parse::<u64>()
            .map_err(|e| OxiGeoError::InvalidParameter {
                parameter: "batch",
                message: format!("Schema metadata 'height' is not a valid u64: {e}"),
            })?;

        let dt_name = meta.get("data_type").ok_or(OxiGeoError::InvalidParameter {
            parameter: "batch",
            message: "Schema metadata missing 'data_type' key".to_string(),
        })?;

        let data_type = parse_data_type(dt_name)?;

        // ── Convert column to bytes ───────────────────────────────────────────

        let column = batch.column(0);
        let bytes = arrow_column_to_bytes(column, data_type)?;

        RasterBuffer::new(bytes, width, height, data_type, NoDataValue::None)
    }
}

// ─── Private helpers ─────────────────────────────────────────────────────────

/// Parses a `RasterDataType` from its canonical name string.
fn parse_data_type(name: &str) -> Result<RasterDataType> {
    match name {
        "UInt8" => Ok(RasterDataType::UInt8),
        "Int8" => Ok(RasterDataType::Int8),
        "UInt16" => Ok(RasterDataType::UInt16),
        "Int16" => Ok(RasterDataType::Int16),
        "UInt32" => Ok(RasterDataType::UInt32),
        "Int32" => Ok(RasterDataType::Int32),
        "UInt64" => Ok(RasterDataType::UInt64),
        "Int64" => Ok(RasterDataType::Int64),
        "Float32" => Ok(RasterDataType::Float32),
        "Float64" => Ok(RasterDataType::Float64),
        "CFloat32" | "CFloat64" => Err(OxiGeoError::NotSupported {
            operation: format!("Arrow conversion of complex type {name}"),
        }),
        other => Err(OxiGeoError::InvalidParameter {
            parameter: "data_type",
            message: format!("Unknown data type '{other}' in schema metadata"),
        }),
    }
}

/// Converts an Arrow array column to a raw byte `Vec<u8>` suitable for
/// constructing a `RasterBuffer`.  Nulls are replaced by the zero bytes of the
/// element type.
fn arrow_column_to_bytes(column: &dyn Array, data_type: RasterDataType) -> Result<Vec<u8>> {
    macro_rules! downcast_to_bytes {
        ($ArrowArray:ty, $native:ty, $column:expr) => {{
            let arr = $column
                .as_any()
                .downcast_ref::<$ArrowArray>()
                .ok_or_else(|| OxiGeoError::Internal {
                    message: format!(
                        "Expected {} array, got {:?}",
                        stringify!($ArrowArray),
                        $column.data_type()
                    ),
                })?;
            let mut bytes = Vec::with_capacity(arr.len() * core::mem::size_of::<$native>());
            for i in 0..arr.len() {
                let v: $native = if arr.is_null(i) {
                    <$native as Default>::default()
                } else {
                    arr.value(i)
                };
                bytes.extend_from_slice(&v.to_ne_bytes());
            }
            Ok(bytes)
        }};
    }

    match data_type {
        RasterDataType::UInt8 => downcast_to_bytes!(UInt8Array, u8, column),
        RasterDataType::Int8 => downcast_to_bytes!(Int8Array, i8, column),
        RasterDataType::UInt16 => downcast_to_bytes!(UInt16Array, u16, column),
        RasterDataType::Int16 => downcast_to_bytes!(Int16Array, i16, column),
        RasterDataType::UInt32 => downcast_to_bytes!(UInt32Array, u32, column),
        RasterDataType::Int32 => downcast_to_bytes!(Int32Array, i32, column),
        RasterDataType::UInt64 => downcast_to_bytes!(UInt64Array, u64, column),
        RasterDataType::Int64 => downcast_to_bytes!(Int64Array, i64, column),
        RasterDataType::Float32 => downcast_to_bytes!(Float32Array, f32, column),
        RasterDataType::Float64 => downcast_to_bytes!(Float64Array, f64, column),
        RasterDataType::CFloat32 | RasterDataType::CFloat64 => Err(OxiGeoError::NotSupported {
            operation: format!("Arrow conversion of complex type {}", data_type.name()),
        }),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::buffer::RasterBuffer;
    use crate::types::{NoDataValue, RasterDataType};

    // Helper: create a simple 4×4 UInt8 buffer filled with sequential values 0..15
    fn make_u8_4x4() -> RasterBuffer {
        let data: Vec<u8> = (0u8..16).collect();
        RasterBuffer::new(data, 4, 4, RasterDataType::UInt8, NoDataValue::None)
            .expect("valid buffer")
    }

    // Helper: create a 2×3 Float32 buffer filled with distinct values
    fn make_f32_2x3() -> RasterBuffer {
        let data: Vec<f32> = (0..6).map(|i| i as f32 * 1.5_f32).collect();
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_ne_bytes()).collect();
        RasterBuffer::new(bytes, 2, 3, RasterDataType::Float32, NoDataValue::None)
            .expect("valid buffer")
    }

    // Helper: create a Float64 buffer with known values
    fn make_f64_2x2() -> RasterBuffer {
        let data: Vec<f64> = vec![1.1, 2.2, 3.3, 4.4];
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_ne_bytes()).collect();
        RasterBuffer::new(bytes, 2, 2, RasterDataType::Float64, NoDataValue::None)
            .expect("valid buffer")
    }

    // ── Forward conversion tests ──────────────────────────────────────────────

    /// TryFrom<&RasterBuffer> for RecordBatch: 4×4 UInt8 buffer
    #[test]
    fn test_raster_buffer_to_record_batch_u8() {
        let buf = make_u8_4x4();
        let batch = RecordBatch::try_from(&buf).expect("conversion should succeed");

        // Row count = width * height
        assert_eq!(batch.num_rows(), 16);
        assert_eq!(batch.num_columns(), 1);

        // Column type
        assert_eq!(batch.schema().field(0).data_type(), &DataType::UInt8);
        assert_eq!(batch.schema().field(0).name(), "pixel_values");

        // Spot-check values
        let col = batch
            .column(0)
            .as_any()
            .downcast_ref::<UInt8Array>()
            .expect("UInt8Array");
        for i in 0u8..16 {
            assert_eq!(col.value(i as usize), i, "value at index {i}");
            assert!(!col.is_null(i as usize));
        }
    }

    /// TryFrom<&RasterBuffer> for RecordBatch: schema metadata carries correct width/height
    #[test]
    fn test_raster_buffer_to_record_batch_f32_metadata() {
        let buf = make_f32_2x3();
        let batch = RecordBatch::try_from(&buf).expect("conversion should succeed");

        let meta = batch.schema().metadata().clone();
        assert_eq!(meta.get("width").map(String::as_str), Some("2"));
        assert_eq!(meta.get("height").map(String::as_str), Some("3"));
        assert_eq!(meta.get("data_type").map(String::as_str), Some("Float32"));

        // 2 * 3 rows
        assert_eq!(batch.num_rows(), 6);
        assert_eq!(batch.schema().field(0).data_type(), &DataType::Float32);
    }

    /// Roundtrip: RasterBuffer → RecordBatch → RasterBuffer preserves f64 pixels
    #[test]
    fn test_record_batch_roundtrip_f64() {
        let original = make_f64_2x2();
        let batch = RecordBatch::try_from(&original).expect("forward conversion");
        let recovered = RasterBuffer::try_from(batch).expect("reverse conversion");

        assert_eq!(recovered.width(), original.width());
        assert_eq!(recovered.height(), original.height());
        assert_eq!(recovered.data_type(), RasterDataType::Float64);

        for y in 0..original.height() {
            for x in 0..original.width() {
                let orig_val = original.get_pixel(x, y).expect("get_pixel original");
                let rcvd_val = recovered.get_pixel(x, y).expect("get_pixel recovered");
                assert!(
                    (orig_val - rcvd_val).abs() < f64::EPSILON,
                    "pixel ({x},{y}): expected {orig_val}, got {rcvd_val}"
                );
            }
        }
    }

    /// NoData pixels are emitted as Arrow nulls
    #[test]
    fn test_nodata_becomes_null() {
        // 3×1 Float32 buffer: values = [0.0, 1.5, 0.0], nodata = 0.0
        let data_f32: Vec<f32> = vec![0.0_f32, 1.5_f32, 0.0_f32];
        let bytes: Vec<u8> = data_f32.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let buf = RasterBuffer::new(
            bytes,
            3,
            1,
            RasterDataType::Float32,
            NoDataValue::Float(0.0),
        )
        .expect("valid buffer");

        let batch = RecordBatch::try_from(&buf).expect("conversion should succeed");
        let col = batch
            .column(0)
            .as_any()
            .downcast_ref::<Float32Array>()
            .expect("Float32Array");

        assert_eq!(col.len(), 3);
        assert!(col.is_null(0), "pixel 0 (nodata) should be null");
        assert!(!col.is_null(1), "pixel 1 (1.5) should not be null");
        assert!(col.is_null(2), "pixel 2 (nodata) should be null");
        assert!((col.value(1) - 1.5_f32).abs() < f32::EPSILON);
    }

    // ── Error-path tests ──────────────────────────────────────────────────────

    /// CFloat32 buffer → TryFrom returns NotSupported error
    #[test]
    fn test_complex_type_returns_error() {
        // CFloat32 is 8 bytes per pixel (real + imaginary, each f32)
        let buf = RasterBuffer::zeros(2, 2, RasterDataType::CFloat32);
        let result = RecordBatch::try_from(&buf);

        assert!(result.is_err(), "CFloat32 conversion should fail");
        let err = result.expect_err("expected error");
        assert!(
            matches!(err, OxiGeoError::NotSupported { .. }),
            "expected NotSupported, got {err:?}"
        );
    }

    /// CFloat64 buffer → TryFrom returns NotSupported error
    #[test]
    fn test_complex_type_cfloat64_returns_error() {
        let buf = RasterBuffer::zeros(2, 2, RasterDataType::CFloat64);
        let result = RecordBatch::try_from(&buf);

        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("expected error"),
            OxiGeoError::NotSupported { .. }
        ));
    }

    // ── Reverse conversion error tests ───────────────────────────────────────

    /// RecordBatch without required schema metadata → Err(InvalidParameter)
    #[test]
    fn test_record_batch_to_buffer_wrong_schema_missing_metadata() {
        // Build a minimal RecordBatch with no metadata
        let field = Field::new("pixel_values", DataType::UInt8, false);
        let schema = Arc::new(Schema::new(vec![field]));
        let array: ArrayRef = Arc::new(UInt8Array::from(vec![1u8, 2, 3, 4]));
        let batch = RecordBatch::try_new(schema, vec![array]).expect("valid RecordBatch for test");

        let result = RasterBuffer::try_from(batch);
        assert!(result.is_err(), "missing metadata should fail");
        assert!(
            matches!(
                result.expect_err("expected error"),
                OxiGeoError::InvalidParameter { .. }
            ),
            "expected InvalidParameter error"
        );
    }

    /// RecordBatch with wrong column name → Err(InvalidParameter)
    #[test]
    fn test_record_batch_to_buffer_wrong_column_name() {
        let mut metadata = HashMap::new();
        metadata.insert("width".to_string(), "2".to_string());
        metadata.insert("height".to_string(), "2".to_string());
        metadata.insert("data_type".to_string(), "UInt8".to_string());

        let field = Field::new("wrong_name", DataType::UInt8, false);
        let schema = Arc::new(Schema::new_with_metadata(vec![field], metadata));
        let array: ArrayRef = Arc::new(UInt8Array::from(vec![1u8, 2, 3, 4]));
        let batch = RecordBatch::try_new(schema, vec![array]).expect("valid RecordBatch for test");

        let result = RasterBuffer::try_from(batch);
        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("expected error"),
            OxiGeoError::InvalidParameter { .. }
        ));
    }

    /// RecordBatch with too many columns → Err(InvalidParameter)
    #[test]
    fn test_record_batch_to_buffer_too_many_columns() {
        let mut metadata = HashMap::new();
        metadata.insert("width".to_string(), "2".to_string());
        metadata.insert("height".to_string(), "2".to_string());
        metadata.insert("data_type".to_string(), "UInt8".to_string());

        let field1 = Field::new("pixel_values", DataType::UInt8, false);
        let field2 = Field::new("extra_column", DataType::UInt8, false);
        let schema = Arc::new(Schema::new_with_metadata(vec![field1, field2], metadata));
        let array1: ArrayRef = Arc::new(UInt8Array::from(vec![1u8, 2, 3, 4]));
        let array2: ArrayRef = Arc::new(UInt8Array::from(vec![5u8, 6, 7, 8]));
        let batch =
            RecordBatch::try_new(schema, vec![array1, array2]).expect("valid RecordBatch for test");

        let result = RasterBuffer::try_from(batch);
        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("expected error"),
            OxiGeoError::InvalidParameter { .. }
        ));
    }

    // ── Additional type coverage ──────────────────────────────────────────────

    /// Int16 buffer roundtrip
    #[test]
    fn test_roundtrip_i16() {
        let data: Vec<i16> = vec![-1000_i16, 0, 1000, i16::MAX];
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let buf = RasterBuffer::new(bytes, 2, 2, RasterDataType::Int16, NoDataValue::None)
            .expect("valid buffer");

        let batch = RecordBatch::try_from(&buf).expect("forward");
        let recovered = RasterBuffer::try_from(batch).expect("reverse");

        assert_eq!(recovered.data_type(), RasterDataType::Int16);
        for y in 0..2u64 {
            for x in 0..2u64 {
                let orig = buf.get_pixel(x, y).expect("orig pixel");
                let rcvd = recovered.get_pixel(x, y).expect("rcvd pixel");
                assert!((orig - rcvd).abs() < f64::EPSILON);
            }
        }
    }

    /// UInt64 buffer roundtrip
    #[test]
    fn test_roundtrip_u64() {
        let data: Vec<u64> = vec![0u64, 1, u64::MAX / 2, 1_000_000];
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let buf = RasterBuffer::new(bytes, 2, 2, RasterDataType::UInt64, NoDataValue::None)
            .expect("valid buffer");

        let batch = RecordBatch::try_from(&buf).expect("forward");
        let recovered = RasterBuffer::try_from(batch).expect("reverse");

        assert_eq!(recovered.data_type(), RasterDataType::UInt64);
        // Check raw bytes match (u64 is wider than f64 precision allows exact f64 comparison)
        assert_eq!(recovered.as_bytes(), buf.as_bytes());
    }

    /// Integer nodata (NoDataValue::Integer) is handled correctly
    #[test]
    fn test_integer_nodata_becomes_null() {
        // 1×4 Int32 buffer, nodata = -9999
        let data: Vec<i32> = vec![-9999_i32, 100, -9999, 200];
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let buf = RasterBuffer::new(
            bytes,
            4,
            1,
            RasterDataType::Int32,
            NoDataValue::Integer(-9999),
        )
        .expect("valid buffer");

        let batch = RecordBatch::try_from(&buf).expect("conversion");
        let col = batch
            .column(0)
            .as_any()
            .downcast_ref::<Int32Array>()
            .expect("Int32Array");

        assert!(col.is_null(0), "index 0 (-9999) should be null");
        assert!(!col.is_null(1), "index 1 (100) should not be null");
        assert!(col.is_null(2), "index 2 (-9999) should be null");
        assert!(!col.is_null(3), "index 3 (200) should not be null");
        assert_eq!(col.value(1), 100);
        assert_eq!(col.value(3), 200);
    }
}
