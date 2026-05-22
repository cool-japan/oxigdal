//! Public API for exposing per-column / per-row-group Parquet statistics.
//!
//! Parquet stores min, max, null count, and (optionally) distinct count for
//! every column in every row group, in the file footer.  This module surfaces
//! that data through a typed [`ColumnStatistics`] struct so callers can do
//! pre-scan filtering and analytics without re-reading the file.
//!
//! # Stability
//!
//! Stats may be absent for some columns — particularly geometry columns
//! (whose WKB blobs have no meaningful min/max) and string columns from
//! writers that disable stats by default.  The extractor never panics on
//! missing or unsupported stats; instead it returns
//! [`ScalarValue::Other`] with the debug string of the underlying value so
//! the user can decide what to do.

use crate::covering::BboxColumns;
use crate::error::Result;
use crate::predicate::ScalarValue;
use arrow_schema::Schema;
use parquet::file::metadata::ParquetMetaData;
use parquet::file::statistics::Statistics;

/// Per-row-group / per-column Parquet statistics.
///
/// One [`ColumnStatistics`] is produced per (row group, column) pair when
/// stats are present in the file footer.  The same column is described by
/// (potentially many) different `ColumnStatistics` instances — one per row
/// group — because Parquet stats are per-row-group.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnStatistics {
    /// Column name (the leaf name from the Parquet schema).
    pub name: String,
    /// String representation of the Parquet physical type (e.g. `"INT64"`,
    /// `"BYTE_ARRAY"`).  Useful for debug output and disambiguating
    /// `ScalarValue::Other`.
    pub parquet_type: String,
    /// Minimum value across the row group.  `ScalarValue::Other(_)` when the
    /// physical type is not natively represented in [`ScalarValue`].
    pub min: ScalarValue,
    /// Maximum value across the row group.
    pub max: ScalarValue,
    /// Number of null values in this column for this row group.
    pub null_count: u64,
    /// Number of distinct values, when the writer recorded that — almost
    /// always `None` in practice (most writers omit it).
    pub distinct_count: Option<u64>,
}

/// Extract per-row-group, per-column statistics from a parsed Parquet file's
/// metadata.
///
/// Returned shape: `outer[row_group_idx][column_idx]`.  When stats are absent
/// for a column in a particular row group, that entry is omitted (so the
/// inner Vec may be shorter than `schema.fields().len()`).
///
/// # Errors
///
/// This function never errors — bad/unsupported Parquet types degrade to
/// `ScalarValue::Other(_)` rather than failing the whole extraction.  The
/// `Result` return is reserved for future error paths (e.g. structural
/// integrity checks) so callers can stay forward-compatible.
pub fn extract_column_statistics(
    metadata: &ParquetMetaData,
    _schema: &Schema,
) -> Result<Vec<Vec<ColumnStatistics>>> {
    let parquet_schema = metadata.file_metadata().schema_descr();
    let mut out = Vec::with_capacity(metadata.num_row_groups());
    for rg_idx in 0..metadata.num_row_groups() {
        let rg = metadata.row_group(rg_idx);
        let mut rg_stats: Vec<ColumnStatistics> = Vec::new();
        for col_idx in 0..rg.num_columns() {
            let col_chunk = rg.column(col_idx);
            let Some(stats) = col_chunk.statistics() else {
                continue;
            };
            // Use the leaf path's last component as the column name —
            // matches what users see in `RecordBatch::column_by_name`.
            let leaf_name = parquet_schema
                .column(col_idx)
                .path()
                .parts()
                .last()
                .cloned()
                .unwrap_or_else(|| parquet_schema.column(col_idx).name().to_string());
            let parquet_type = format!("{:?}", parquet_schema.column(col_idx).physical_type());
            let (min, max) = scalar_pair(stats);
            let null_count = stats.null_count_opt().unwrap_or(0);
            let distinct_count = stats.distinct_count_opt();
            rg_stats.push(ColumnStatistics {
                name: leaf_name,
                parquet_type,
                min,
                max,
                null_count,
                distinct_count,
            });
        }
        out.push(rg_stats);
    }
    Ok(out)
}

/// Convert Parquet column statistics to a `(min, max)` pair of `ScalarValue`.
///
/// Mapping:
/// * `Int32` → [`ScalarValue::Int32`]
/// * `Int64` → [`ScalarValue::Int64`]
/// * `Int96` → [`ScalarValue::Other`] (timestamp-only, deprecated)
/// * `Float` → [`ScalarValue::Float32`]
/// * `Double` → [`ScalarValue::Float64`]
/// * `Boolean` → [`ScalarValue::Bool`]
/// * `ByteArray` → [`ScalarValue::Utf8`] when valid UTF-8, else
///   [`ScalarValue::Binary`]
/// * `FixedLenByteArray` → [`ScalarValue::Binary`]
fn scalar_pair(stats: &Statistics) -> (ScalarValue, ScalarValue) {
    fn opt_or_other<T: std::fmt::Debug>(
        opt: Option<&T>,
        build: impl Fn(&T) -> ScalarValue,
    ) -> ScalarValue {
        match opt {
            Some(v) => build(v),
            None => ScalarValue::Other("none".to_string()),
        }
    }

    match stats {
        Statistics::Int32(typed) => (
            opt_or_other(typed.min_opt(), |v| ScalarValue::Int32(*v)),
            opt_or_other(typed.max_opt(), |v| ScalarValue::Int32(*v)),
        ),
        Statistics::Int64(typed) => (
            opt_or_other(typed.min_opt(), |v| ScalarValue::Int64(*v)),
            opt_or_other(typed.max_opt(), |v| ScalarValue::Int64(*v)),
        ),
        Statistics::Float(typed) => (
            opt_or_other(typed.min_opt(), |v| ScalarValue::Float32(*v)),
            opt_or_other(typed.max_opt(), |v| ScalarValue::Float32(*v)),
        ),
        Statistics::Double(typed) => (
            opt_or_other(typed.min_opt(), |v| ScalarValue::Float64(*v)),
            opt_or_other(typed.max_opt(), |v| ScalarValue::Float64(*v)),
        ),
        Statistics::Boolean(typed) => (
            opt_or_other(typed.min_opt(), |v| ScalarValue::Bool(*v)),
            opt_or_other(typed.max_opt(), |v| ScalarValue::Bool(*v)),
        ),
        Statistics::ByteArray(typed) => (
            opt_or_other(typed.min_opt(), byte_array_to_scalar),
            opt_or_other(typed.max_opt(), byte_array_to_scalar),
        ),
        Statistics::FixedLenByteArray(typed) => (
            opt_or_other(typed.min_opt(), |v| ScalarValue::Binary(v.data().to_vec())),
            opt_or_other(typed.max_opt(), |v| ScalarValue::Binary(v.data().to_vec())),
        ),
        Statistics::Int96(typed) => (
            opt_or_other(typed.min_opt(), |v| ScalarValue::Other(format!("{v:?}"))),
            opt_or_other(typed.max_opt(), |v| ScalarValue::Other(format!("{v:?}"))),
        ),
    }
}

/// Convert a Parquet `ByteArray` to either a [`ScalarValue::Utf8`] (when it's
/// valid UTF-8) or a [`ScalarValue::Binary`] otherwise.
///
/// Most string-typed Parquet columns end up as `ByteArray` physically — the
/// logical type tells us "this is UTF-8" but stats don't carry the logical
/// type.  Falling back to UTF-8 sniffing keeps the surface ergonomic.
fn byte_array_to_scalar(b: &parquet::data_type::ByteArray) -> ScalarValue {
    let bytes = b.data();
    match std::str::from_utf8(bytes) {
        Ok(s) => ScalarValue::Utf8(s.to_string()),
        Err(_) => ScalarValue::Binary(bytes.to_vec()),
    }
}

/// Returns `true` when the geometry column is "covered" by spec-shape bbox
/// columns and therefore *can* expose meaningful per-row-group stats.
///
/// Used by `GeoParquetReader::column_statistics` to decide whether to return
/// `None` for geometry-column stat queries (the WKB blob's own min/max being
/// meaningless).
pub(crate) fn geometry_has_meaningful_stats(metadata: &ParquetMetaData, geom_col: &str) -> bool {
    let schema = metadata.file_metadata().schema_descr();
    BboxColumns::detect(schema, geom_col).is_some()
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use arrow_array::{BinaryArray, Float64Array, Int64Array, RecordBatch, StringArray};
    use arrow_schema::{DataType, Field, Schema as ArrowSchema};
    use parquet::arrow::ArrowWriter;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use parquet::file::properties::WriterProperties;
    use std::fs::File;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "oxigdal_geoparquet_stats_{}_{}_{}.parquet",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        p
    }

    fn write_simple_parquet(
        path: &std::path::Path,
        schema: Arc<ArrowSchema>,
        batches: Vec<RecordBatch>,
        row_groups: usize,
    ) {
        let _ = std::fs::remove_file(path);
        let file = File::create(path).expect("create");
        let _ = row_groups; // each `write` call below produces one row group.
        // Use a large row count so each write+flush produces one row group;
        // the write+flush pattern below is what governs row-group boundaries
        // in tests.
        let props = WriterProperties::builder().build();
        let mut writer = ArrowWriter::try_new(file, schema, Some(props)).expect("writer");
        for batch in batches {
            writer.write(&batch).expect("write");
            writer.flush().expect("flush");
        }
        writer.close().expect("close");
    }

    fn read_metadata(path: &std::path::Path) -> Arc<ParquetMetaData> {
        let file = File::open(path).expect("open");
        let builder = ParquetRecordBatchReaderBuilder::try_new(file).expect("build");
        builder.metadata().clone()
    }

    #[test]
    fn test_stats_int64_min_max() {
        let path = temp_path("int64_min_max");
        let schema = Arc::new(ArrowSchema::new(vec![Field::new(
            "population",
            DataType::Int64,
            true,
        )]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(Int64Array::from(vec![100, 250, 500_000]))],
        )
        .expect("batch");
        write_simple_parquet(&path, schema.clone(), vec![batch], 1);

        let meta = read_metadata(&path);
        let groups = extract_column_statistics(&meta, schema.as_ref()).expect("stats");
        assert_eq!(groups.len(), 1, "one row group");
        assert!(!groups[0].is_empty(), "should have at least one column");

        let pop_stats = groups[0]
            .iter()
            .find(|c| c.name == "population")
            .expect("population stats");
        match (&pop_stats.min, &pop_stats.max) {
            (ScalarValue::Int64(min), ScalarValue::Int64(max)) => {
                assert_eq!(*min, 100);
                assert_eq!(*max, 500_000);
            }
            other => panic!("expected Int64/Int64, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_stats_string_min_max() {
        let path = temp_path("string_min_max");
        let schema = Arc::new(ArrowSchema::new(vec![Field::new(
            "name",
            DataType::Utf8,
            true,
        )]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(StringArray::from(vec!["alpha", "beta", "delta"]))],
        )
        .expect("batch");
        write_simple_parquet(&path, schema.clone(), vec![batch], 1);

        let meta = read_metadata(&path);
        let groups = extract_column_statistics(&meta, schema.as_ref()).expect("stats");
        let s = groups[0]
            .iter()
            .find(|c| c.name == "name")
            .expect("name stats");
        match (&s.min, &s.max) {
            (ScalarValue::Utf8(min), ScalarValue::Utf8(max)) => {
                assert_eq!(min, "alpha");
                assert_eq!(max, "delta");
            }
            other => panic!("expected Utf8 min/max, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_stats_null_counts_per_row_group() {
        let path = temp_path("null_counts");
        let schema = Arc::new(ArrowSchema::new(vec![Field::new(
            "value",
            DataType::Int64,
            true,
        )]));
        // Two batches written separately to produce two row groups.
        let b1 = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(Int64Array::from(vec![Some(1), None, Some(3)]))],
        )
        .expect("b1");
        let b2 = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(Int64Array::from(vec![Some(10), Some(20)]))],
        )
        .expect("b2");
        write_simple_parquet(&path, schema.clone(), vec![b1, b2], 2);

        let meta = read_metadata(&path);
        let groups = extract_column_statistics(&meta, schema.as_ref()).expect("stats");
        assert_eq!(groups.len(), 2, "two row groups");

        let rg0 = groups[0]
            .iter()
            .find(|c| c.name == "value")
            .expect("rg0 value");
        assert_eq!(rg0.null_count, 1, "rg0 has one null");

        let rg1 = groups[1]
            .iter()
            .find(|c| c.name == "value")
            .expect("rg1 value");
        assert_eq!(rg1.null_count, 0, "rg1 has zero nulls");
        let _ = std::fs::remove_file(&path);
    }

    /// When stats are absent for a column (because the writer disabled
    /// statistics output), the public `column_statistics` API on
    /// `GeoParquetReader` must return `None` cleanly — no panic, no
    /// hard error.
    #[test]
    fn test_stats_missing_returns_none() {
        use crate::GeoParquetReader;
        use crate::arrow_ext::add_geoparquet_metadata;
        use crate::metadata::{Crs, GeoParquetMetadata, GeometryColumnMetadata};

        let path = temp_path("no_stats");
        // Embed valid geo metadata so GeoParquetReader::open() accepts the
        // file: a single geometry column plus an unrelated `blob` column.
        let raw_schema = ArrowSchema::new(vec![
            Field::new("geometry", DataType::Binary, true),
            Field::new("blob", DataType::Binary, true),
        ]);
        let mut geo_meta = GeoParquetMetadata::new("geometry");
        geo_meta.add_column(
            "geometry",
            GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84()),
        );
        let json = geo_meta.to_json().expect("geo to_json");
        let schema = Arc::new(add_geoparquet_metadata(raw_schema, json).expect("attach geo meta"));

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(BinaryArray::from_vec(vec![
                    b"\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00".as_ref(),
                    b"\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00".as_ref(),
                ])),
                Arc::new(BinaryArray::from_vec(vec![
                    b"\x00\x01\x02".as_ref(),
                    b"\x03\x04\x05".as_ref(),
                ])),
            ],
        )
        .expect("batch");

        // Disable statistics output for this writer.
        let _ = std::fs::remove_file(&path);
        let file = File::create(&path).expect("create");
        let props = WriterProperties::builder()
            .set_statistics_enabled(parquet::file::properties::EnabledStatistics::None)
            .build();
        let mut writer = ArrowWriter::try_new(file, schema.clone(), Some(props)).expect("writer");
        writer.write(&batch).expect("write");
        writer.close().expect("close");

        // Public API contract: no panic, returns None for the stats-less
        // `blob` column.
        let reader = GeoParquetReader::open(&path).expect("open");
        let blob_stats = reader.column_statistics("blob");
        assert!(
            blob_stats.is_none(),
            "stats-less column must return None; got {blob_stats:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_stats_by_column_name_filtering() {
        let path = temp_path("by_col_name");
        let schema = Arc::new(ArrowSchema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("score", DataType::Float64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Int64Array::from(vec![1i64, 2, 3])),
                Arc::new(Float64Array::from(vec![0.1, 0.5, 0.9])),
            ],
        )
        .expect("batch");
        write_simple_parquet(&path, schema.clone(), vec![batch], 1);

        let meta = read_metadata(&path);
        let groups = extract_column_statistics(&meta, schema.as_ref()).expect("stats");
        let rg = &groups[0];
        let id = rg.iter().find(|c| c.name == "id").expect("id stats");
        let score = rg.iter().find(|c| c.name == "score").expect("score stats");
        match &id.min {
            ScalarValue::Int64(v) => assert_eq!(*v, 1),
            other => panic!("expected Int64 min, got {other:?}"),
        }
        match &score.max {
            ScalarValue::Float64(v) => assert!(*v - 0.9 < 1e-12),
            other => panic!("expected Float64 max, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    /// Public-API contract: when a file has a WKB geometry column but no
    /// `covering.bbox` columns, `column_statistics("geometry")` returns
    /// `None` because the WKB blob has no meaningful min/max.
    #[test]
    fn test_stats_geometry_column_returns_none_without_bbox_columns() {
        use crate::GeoParquetReader;
        use crate::arrow_ext::add_geoparquet_metadata;
        use crate::metadata::{Crs, GeoParquetMetadata, GeometryColumnMetadata};

        let path = temp_path("geom_no_bbox");
        // Build a real GeoParquet file with the `geo` JSON metadata.
        let raw_schema = ArrowSchema::new(vec![Field::new("geometry", DataType::Binary, true)]);
        let mut geo_meta = GeoParquetMetadata::new("geometry");
        geo_meta.add_column(
            "geometry",
            GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84()),
        );
        let json = geo_meta.to_json().expect("geo to_json");
        let schema = Arc::new(add_geoparquet_metadata(raw_schema, json).expect("attach geo meta"));

        // Realistic LE WKB Point(1.0, 2.0): 0x01 + type=1 + x=1.0 + y=2.0
        let mut wkb_pt = vec![0x01u8];
        wkb_pt.extend_from_slice(&1u32.to_le_bytes());
        wkb_pt.extend_from_slice(&1.0f64.to_le_bytes());
        wkb_pt.extend_from_slice(&2.0f64.to_le_bytes());
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(BinaryArray::from_vec(vec![wkb_pt.as_slice()]))],
        )
        .expect("batch");
        write_simple_parquet(&path, schema.clone(), vec![batch], 1);

        // Public-API contract.
        let reader = GeoParquetReader::open(&path).expect("open");
        let stats = reader.column_statistics("geometry");
        assert!(
            stats.is_none(),
            "without covering.bbox, GeoParquetReader::column_statistics(\"geometry\") must return None; got {stats:?}"
        );
        let _ = std::fs::remove_file(&path);
    }
}
