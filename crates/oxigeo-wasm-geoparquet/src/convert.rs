//! Arrow `RecordBatch` → GeoJSON `FeatureCollection` conversion.
//!
//! Decodes each row's WKB geometry via `oxigeo-geoparquet`'s
//! [`WkbReader`], maps the seven [`Geometry`] variants onto
//! `oxigeo-geojson-stream` [`GeoJsonGeometry`] values, carries projected
//! attribute columns (`Float64` / `Utf8` / `Int64` / `Int32` / `Boolean` /
//! `Date32` / `Timestamp`) as feature properties and accumulates the match
//! count and total `area_in_meters`.  Output is written with the compact
//! GeoJSON writer at coordinate precision 6.
//!
//! This module is intentionally cross-platform (it links no `web_sys` /
//! `wasm_bindgen` surface) so the parity test can exercise it — and the whole
//! plan → fetch → decode pipeline — natively.
//!
//! Implemented by WP C4 (GeoParquet Live lane); stub created by WP W0.

// The public surface here is consumed by the wasm-only `session` bindings
// (also WP C4); on a native lib build without the parity test it looks unused.
#![allow(dead_code)]

use arrow_array::{
    Array, BinaryArray, BooleanArray, Date32Array, Float64Array, Int32Array, Int64Array,
    RecordBatch, StringArray, TimestampMicrosecondArray, TimestampMillisecondArray,
    TimestampNanosecondArray, TimestampSecondArray,
};
use arrow_schema::DataType;
use oxigeo_geojson_stream::{GeoJsonFeature, GeoJsonGeometry, GeoJsonWriter};
use oxigeo_geoparquet::geometry::{Coordinate, Geometry, WkbReader};
use serde_json::{Map, Value};

use crate::error::GpqLiveError;

/// Coordinate precision (decimal places) for emitted GeoJSON — 6 places is
/// ~0.1 m at the equator, ample for building-footprint display.
pub const GEOJSON_PRECISION: usize = 6;

/// The result of converting a query's record batches to GeoJSON.
///
/// `geojson` is a complete `FeatureCollection` document; `matched` counts the
/// emitted features (rows with a decodable geometry); `total_area_m2` sums the
/// `area_in_meters` attribute over those features (0.0 when the column is
/// absent from the projection).
#[derive(Debug, Clone)]
pub struct QueryOutput {
    /// Serialized GeoJSON `FeatureCollection`.
    pub geojson: String,
    /// Number of features written.
    pub matched: usize,
    /// Sum of the area attribute over all matched features, in square metres.
    pub total_area_m2: f64,
}

/// Convert a slice of Arrow [`RecordBatch`]es to a GeoJSON `FeatureCollection`.
///
/// * `batches` — the (already filtered) record batches from pushdown execution.
/// * `geometry_column` — name of the WKB geometry column to decode.
/// * `area_column` — name of the numeric area column to total (e.g.
///   `area_in_meters`); when it is absent from a batch the running sum is left
///   unchanged.
///
/// Every non-geometry column whose Arrow type is `Float64`, `Utf8`, `Int64`,
/// `Int32`, `Boolean`, `Date32`, or `Timestamp` (any unit) becomes a feature
/// property; other column types (struct `bbox`, binary, list, …) are skipped.
/// Rows whose geometry is null are dropped rather than emitted with a null
/// geometry.
///
/// # Errors
///
/// Returns [`GpqLiveError`] if the geometry column is missing, is not a
/// `BinaryArray`, or contains WKB that fails to decode.
pub fn record_batches_to_geojson(
    batches: &[RecordBatch],
    geometry_column: &str,
    area_column: &str,
) -> Result<QueryOutput, GpqLiveError> {
    let mut features: Vec<GeoJsonFeature> = Vec::new();
    let mut total_area_m2 = 0.0f64;

    for batch in batches {
        let geom_col = batch
            .column_by_name(geometry_column)
            .ok_or_else(|| missing_column(geometry_column))?;
        let binary = geom_col
            .as_any()
            .downcast_ref::<BinaryArray>()
            .ok_or_else(|| {
                GpqLiveError::Parquet(parquet::errors::ParquetError::General(format!(
                    "geometry column '{geometry_column}' is not a BinaryArray (got {:?})",
                    geom_col.data_type()
                )))
            })?;

        // Pre-resolve the property source columns once per batch.
        let prop_cols = property_columns(batch, geometry_column);
        let area_values = batch
            .column_by_name(area_column)
            .and_then(|c| c.as_any().downcast_ref::<Float64Array>());

        for row in 0..binary.len() {
            if binary.is_null(row) {
                continue;
            }
            let wkb = binary.value(row);
            let geometry = WkbReader::new(wkb).read_geometry()?;
            let gj = geometry_to_geojson(&geometry);

            let properties = build_properties(&prop_cols, row);

            if let Some(area) = area_values
                && !area.is_null(row)
            {
                total_area_m2 += area.value(row);
            }

            features.push(GeoJsonFeature {
                id: None,
                geometry: Some(gj),
                properties: Some(Value::Object(properties)),
            });
        }
    }

    let matched = features.len();
    let geojson = GeoJsonWriter::compact()
        .with_precision(GEOJSON_PRECISION)
        .write_features_iter(features.iter(), None);

    Ok(QueryOutput {
        geojson,
        matched,
        total_area_m2,
    })
}

/// A property-source column resolved to a typed accessor for fast per-row reads.
///
/// The column name is owned so the accessors do not borrow the batch's schema
/// (which `RecordBatch::schema` hands back as a temporary `Arc` clone).
enum PropColumn<'a> {
    /// A `Float64` numeric column.
    Float(String, &'a Float64Array),
    /// A `Utf8` string column.
    Text(String, &'a StringArray),
    /// An `Int64` integer column (e.g. population counts, feature IDs).
    Int64(String, &'a Int64Array),
    /// An `Int32` integer column.
    Int32(String, &'a Int32Array),
    /// A `Boolean` flag column.
    Bool(String, &'a BooleanArray),
    /// A `Date32` (days-since-epoch) column, rendered as `YYYY-MM-DD`.
    Date32(String, &'a Date32Array),
    /// A `Timestamp(Second, _)` column, rendered as an ISO-8601 datetime.
    TimestampSecond(String, &'a TimestampSecondArray),
    /// A `Timestamp(Millisecond, _)` column, rendered as an ISO-8601 datetime.
    TimestampMillisecond(String, &'a TimestampMillisecondArray),
    /// A `Timestamp(Microsecond, _)` column, rendered as an ISO-8601 datetime.
    TimestampMicrosecond(String, &'a TimestampMicrosecondArray),
    /// A `Timestamp(Nanosecond, _)` column, rendered as an ISO-8601 datetime.
    TimestampNanosecond(String, &'a TimestampNanosecondArray),
}

/// Resolve the batch's non-geometry `Float64` / `Utf8` / `Int64` / `Int32` /
/// `Boolean` / `Date32` / `Timestamp` columns to typed accessors, preserving
/// schema order. Other Arrow types (struct `bbox`, binary, list, …) are
/// intentionally skipped — there is no lossless, self-describing GeoJSON
/// property representation for them without a richer schema on the JS side.
fn property_columns<'a>(batch: &'a RecordBatch, geometry_column: &str) -> Vec<PropColumn<'a>> {
    let mut cols = Vec::new();
    for (idx, field) in batch.schema_ref().fields().iter().enumerate() {
        let name = field.name();
        if name == geometry_column {
            continue;
        }
        let array = batch.column(idx);
        if let Some(f) = array.as_any().downcast_ref::<Float64Array>() {
            cols.push(PropColumn::Float(name.clone(), f));
        } else if let Some(s) = array.as_any().downcast_ref::<StringArray>() {
            cols.push(PropColumn::Text(name.clone(), s));
        } else if let Some(i) = array.as_any().downcast_ref::<Int64Array>() {
            cols.push(PropColumn::Int64(name.clone(), i));
        } else if let Some(i) = array.as_any().downcast_ref::<Int32Array>() {
            cols.push(PropColumn::Int32(name.clone(), i));
        } else if let Some(b) = array.as_any().downcast_ref::<BooleanArray>() {
            cols.push(PropColumn::Bool(name.clone(), b));
        } else if let Some(d) = array.as_any().downcast_ref::<Date32Array>() {
            cols.push(PropColumn::Date32(name.clone(), d));
        } else if matches!(array.data_type(), DataType::Timestamp(_, _)) {
            if let Some(t) = array.as_any().downcast_ref::<TimestampSecondArray>() {
                cols.push(PropColumn::TimestampSecond(name.clone(), t));
            } else if let Some(t) = array.as_any().downcast_ref::<TimestampMillisecondArray>() {
                cols.push(PropColumn::TimestampMillisecond(name.clone(), t));
            } else if let Some(t) = array.as_any().downcast_ref::<TimestampMicrosecondArray>() {
                cols.push(PropColumn::TimestampMicrosecond(name.clone(), t));
            } else if let Some(t) = array.as_any().downcast_ref::<TimestampNanosecondArray>() {
                cols.push(PropColumn::TimestampNanosecond(name.clone(), t));
            }
        }
    }
    cols
}

/// Build the JSON property object for a single row.
fn build_properties(cols: &[PropColumn<'_>], row: usize) -> Map<String, Value> {
    let mut map = Map::new();
    for col in cols {
        match col {
            PropColumn::Float(name, arr) => {
                if arr.is_null(row) {
                    map.insert(name.clone(), Value::Null);
                } else if let Some(n) = serde_json::Number::from_f64(arr.value(row)) {
                    map.insert(name.clone(), Value::Number(n));
                }
            }
            PropColumn::Text(name, arr) => {
                if arr.is_null(row) {
                    map.insert(name.clone(), Value::Null);
                } else {
                    map.insert(name.clone(), Value::String(arr.value(row).to_string()));
                }
            }
            PropColumn::Int64(name, arr) => {
                if arr.is_null(row) {
                    map.insert(name.clone(), Value::Null);
                } else {
                    map.insert(name.clone(), Value::Number(arr.value(row).into()));
                }
            }
            PropColumn::Int32(name, arr) => {
                if arr.is_null(row) {
                    map.insert(name.clone(), Value::Null);
                } else {
                    map.insert(name.clone(), Value::Number(arr.value(row).into()));
                }
            }
            PropColumn::Bool(name, arr) => {
                if arr.is_null(row) {
                    map.insert(name.clone(), Value::Null);
                } else {
                    map.insert(name.clone(), Value::Bool(arr.value(row)));
                }
            }
            PropColumn::Date32(name, arr) => {
                let value = if arr.is_null(row) {
                    None
                } else {
                    arr.value_as_date(row).map(|d| d.to_string())
                };
                insert_string_or_null(&mut map, name, value);
            }
            PropColumn::TimestampSecond(name, arr) => {
                insert_timestamp(&mut map, name, arr.is_null(row), || {
                    arr.value_as_datetime(row)
                });
            }
            PropColumn::TimestampMillisecond(name, arr) => {
                insert_timestamp(&mut map, name, arr.is_null(row), || {
                    arr.value_as_datetime(row)
                });
            }
            PropColumn::TimestampMicrosecond(name, arr) => {
                insert_timestamp(&mut map, name, arr.is_null(row), || {
                    arr.value_as_datetime(row)
                });
            }
            PropColumn::TimestampNanosecond(name, arr) => {
                insert_timestamp(&mut map, name, arr.is_null(row), || {
                    arr.value_as_datetime(row)
                });
            }
        }
    }
    map
}

/// Inserts `value` as a JSON string, or `Null` when the row was null or (for
/// an out-of-range day count) the day-count-to-date conversion failed.
fn insert_string_or_null(map: &mut Map<String, Value>, name: &str, value: Option<String>) {
    let json = match value {
        Some(s) => Value::String(s),
        None => Value::Null,
    };
    map.insert(name.to_string(), json);
}

/// Inserts a `Timestamp` value as an ISO-8601 datetime string
/// (`YYYY-MM-DDTHH:MM:SS.fff`), or `Null` when the row is null or (for an
/// out-of-range instant) undecodable. `datetime` is only invoked when the row
/// is not null, since it downcasts a raw integer that may be meaningless for
/// a null slot.
fn insert_timestamp(
    map: &mut Map<String, Value>,
    name: &str,
    is_null: bool,
    datetime: impl FnOnce() -> Option<chrono::NaiveDateTime>,
) {
    let value = if is_null {
        None
    } else {
        datetime().map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
    };
    insert_string_or_null(map, name, value);
}

/// Build a [`GpqLiveError`] for a missing column.
fn missing_column(name: &str) -> GpqLiveError {
    GpqLiveError::Parquet(parquet::errors::ParquetError::General(format!(
        "column '{name}' not found in record batch"
    )))
}

/// Project a [`Coordinate`] to a 2-D `[x, y]` GeoJSON position.
#[inline]
fn xy(c: &Coordinate) -> [f64; 2] {
    [c.x, c.y]
}

/// Map a [`LineString`]-shaped coordinate list to a GeoJSON ring / line.
fn coords_2d(coords: &[Coordinate]) -> Vec<[f64; 2]> {
    coords.iter().map(xy).collect()
}

/// Convert an `oxigeo-geoparquet` [`Geometry`] to a 2-D [`GeoJsonGeometry`].
///
/// Z / M components are projected away: the browser demo renders 2-D and
/// GeoJSON positions are emitted as `[x, y]` regardless of the source
/// dimensionality.  All seven OGC Simple-Feature variants are handled.
pub fn geometry_to_geojson(geometry: &Geometry) -> GeoJsonGeometry {
    match geometry {
        Geometry::Point(p) => GeoJsonGeometry::Point(xy(&p.coord)),
        Geometry::LineString(ls) => GeoJsonGeometry::LineString(coords_2d(&ls.coords)),
        Geometry::Polygon(poly) => GeoJsonGeometry::Polygon(polygon_rings(poly)),
        Geometry::MultiPoint(mp) => {
            GeoJsonGeometry::MultiPoint(mp.points.iter().map(|p| xy(&p.coord)).collect())
        }
        Geometry::MultiLineString(mls) => GeoJsonGeometry::MultiLineString(
            mls.linestrings
                .iter()
                .map(|l| coords_2d(&l.coords))
                .collect(),
        ),
        Geometry::MultiPolygon(mpoly) => {
            GeoJsonGeometry::MultiPolygon(mpoly.polygons.iter().map(polygon_rings).collect())
        }
        Geometry::GeometryCollection(gc) => GeoJsonGeometry::GeometryCollection(
            gc.geometries.iter().map(geometry_to_geojson).collect(),
        ),
    }
}

/// Flatten a polygon (exterior + interior rings) to GeoJSON ring arrays.
fn polygon_rings(poly: &oxigeo_geoparquet::geometry::Polygon) -> Vec<Vec<[f64; 2]>> {
    let mut rings = Vec::with_capacity(1 + poly.interiors.len());
    rings.push(coords_2d(&poly.exterior.coords));
    for hole in &poly.interiors {
        rings.push(coords_2d(&hole.coords));
    }
    rings
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use oxigeo_geoparquet::geometry::{
        Geometry, LineString, MultiPoint, Point, Polygon, WkbWriter,
    };

    fn wkb(geom: &Geometry) -> Vec<u8> {
        WkbWriter::new(true)
            .write_geometry(geom)
            .expect("wkb encode")
    }

    // ── geometry_to_geojson variant coverage ─────────────────────────────────

    #[test]
    fn point_maps_to_geojson_point() {
        let g = Geometry::Point(Point::new_2d(139.7, 35.68));
        match geometry_to_geojson(&g) {
            GeoJsonGeometry::Point([x, y]) => {
                assert!((x - 139.7).abs() < 1e-9 && (y - 35.68).abs() < 1e-9);
            }
            other => panic!("expected Point, got {other:?}"),
        }
    }

    #[test]
    fn point_z_projects_to_2d() {
        let g = Geometry::Point(Point::new_3d(1.0, 2.0, 99.0));
        assert!(matches!(
            geometry_to_geojson(&g),
            GeoJsonGeometry::Point([_, _])
        ));
    }

    #[test]
    fn linestring_maps_all_vertices() {
        let ls = LineString::new(vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 0.0),
        ]);
        match geometry_to_geojson(&Geometry::LineString(ls)) {
            GeoJsonGeometry::LineString(pts) => assert_eq!(pts.len(), 3),
            other => panic!("expected LineString, got {other:?}"),
        }
    }

    #[test]
    fn polygon_carries_exterior_and_holes() {
        let ext = LineString::new(vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(0.0, 4.0),
            Coordinate::new_2d(0.0, 0.0),
        ]);
        let hole = LineString::new(vec![
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 1.0),
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(1.0, 1.0),
        ]);
        let poly = Polygon::new(ext, vec![hole]);
        match geometry_to_geojson(&Geometry::Polygon(poly)) {
            GeoJsonGeometry::Polygon(rings) => {
                assert_eq!(rings.len(), 2);
                assert_eq!(rings[0].len(), 5);
                assert_eq!(rings[1].len(), 4);
            }
            other => panic!("expected Polygon, got {other:?}"),
        }
    }

    #[test]
    fn multipoint_maps_all_points() {
        let mp = MultiPoint::new(vec![Point::new_2d(0.0, 0.0), Point::new_2d(1.0, 1.0)]);
        match geometry_to_geojson(&Geometry::MultiPoint(mp)) {
            GeoJsonGeometry::MultiPoint(pts) => assert_eq!(pts.len(), 2),
            other => panic!("expected MultiPoint, got {other:?}"),
        }
    }

    // ── record_batches_to_geojson ────────────────────────────────────────────

    fn build_batch(points: &[(f64, f64)], areas: &[f64], names: &[&str]) -> RecordBatch {
        use std::sync::Arc;
        let raw: Vec<Vec<u8>> = points
            .iter()
            .map(|(x, y)| wkb(&Geometry::Point(Point::new_2d(*x, *y))))
            .collect();
        let geom_refs: Vec<Option<&[u8]>> = raw.iter().map(|v| Some(v.as_slice())).collect();
        let geom = Arc::new(BinaryArray::from(geom_refs)) as arrow_array::ArrayRef;
        let area = Arc::new(Float64Array::from(areas.to_vec())) as arrow_array::ArrayRef;
        let name = Arc::new(StringArray::from(names.to_vec())) as arrow_array::ArrayRef;
        RecordBatch::try_from_iter_with_nullable(vec![
            ("geometry", geom, true),
            ("area_in_meters", area, false),
            ("name", name, false),
        ])
        .expect("batch")
    }

    #[test]
    fn converts_batch_to_feature_collection() {
        let batch = build_batch(
            &[(139.70, 35.68), (139.71, 35.69)],
            &[100.0, 250.0],
            &["a", "b"],
        );
        let out = record_batches_to_geojson(&[batch], "geometry", "area_in_meters").unwrap();
        assert_eq!(out.matched, 2);
        assert!((out.total_area_m2 - 350.0).abs() < 1e-6);
        assert!(out.geojson.contains("\"FeatureCollection\""));
        assert!(out.geojson.contains("\"area_in_meters\""));
        assert!(out.geojson.contains("\"name\":\"a\""));
        // Geometry column must NOT leak into properties.
        assert!(!out.geojson.contains("\"geometry\":\""));
    }

    #[test]
    fn int_bool_date_and_timestamp_columns_survive_as_properties() {
        use std::sync::Arc;

        use chrono::NaiveDate;

        let raw = wkb(&Geometry::Point(Point::new_2d(139.0, 35.0)));
        let geom = Arc::new(BinaryArray::from(vec![Some(raw.as_slice())])) as arrow_array::ArrayRef;
        let area = Arc::new(Float64Array::from(vec![42.0])) as arrow_array::ArrayRef;
        let population =
            Arc::new(Int64Array::from(vec![1_234_567_890_123i64])) as arrow_array::ArrayRef;
        let small_count = Arc::new(Int32Array::from(vec![7i32])) as arrow_array::ArrayRef;
        let active = Arc::new(BooleanArray::from(vec![true])) as arrow_array::ArrayRef;

        let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch date");
        let built_on_date = NaiveDate::from_ymd_opt(2023, 1, 15).expect("built-on date");
        let built_on_days = built_on_date.signed_duration_since(epoch).num_days() as i32;
        let built_on = Arc::new(Date32Array::from(vec![built_on_days])) as arrow_array::ArrayRef;

        let surveyed_dt = built_on_date
            .and_hms_opt(12, 30, 0)
            .expect("built-on time")
            .and_utc();
        let surveyed_at = Arc::new(TimestampMicrosecondArray::from(vec![
            surveyed_dt.timestamp_micros(),
        ])) as arrow_array::ArrayRef;

        let batch = RecordBatch::try_from_iter_with_nullable(vec![
            ("geometry", geom, true),
            ("area_in_meters", area, false),
            ("population", population, false),
            ("small_count", small_count, false),
            ("active", active, false),
            ("built_on", built_on, false),
            ("surveyed_at", surveyed_at, false),
        ])
        .expect("batch");

        let out = record_batches_to_geojson(&[batch], "geometry", "area_in_meters").unwrap();
        assert_eq!(out.matched, 1);
        assert!(
            out.geojson.contains("\"population\":1234567890123"),
            "{}",
            out.geojson
        );
        assert!(out.geojson.contains("\"small_count\":7"), "{}", out.geojson);
        assert!(out.geojson.contains("\"active\":true"), "{}", out.geojson);
        assert!(
            out.geojson.contains("\"built_on\":\"2023-01-15\""),
            "{}",
            out.geojson
        );
        assert!(
            out.geojson
                .contains("\"surveyed_at\":\"2023-01-15T12:30:00"),
            "{}",
            out.geojson
        );
    }

    #[test]
    fn null_attribute_columns_render_as_json_null() {
        use std::sync::Arc;

        let raw = wkb(&Geometry::Point(Point::new_2d(1.0, 2.0)));
        let geom = Arc::new(BinaryArray::from(vec![Some(raw.as_slice())])) as arrow_array::ArrayRef;
        let area = Arc::new(Float64Array::from(vec![1.0])) as arrow_array::ArrayRef;
        let population: arrow_array::ArrayRef = Arc::new(Int64Array::from(vec![None]));
        let active: arrow_array::ArrayRef = Arc::new(BooleanArray::from(vec![None]));

        let batch = RecordBatch::try_from_iter_with_nullable(vec![
            ("geometry", geom, true),
            ("area_in_meters", area, false),
            ("population", population, true),
            ("active", active, true),
        ])
        .expect("batch");

        let out = record_batches_to_geojson(&[batch], "geometry", "area_in_meters").unwrap();
        assert_eq!(out.matched, 1);
        assert!(
            out.geojson.contains("\"population\":null"),
            "{}",
            out.geojson
        );
        assert!(out.geojson.contains("\"active\":null"), "{}", out.geojson);
    }

    #[test]
    fn precision_is_six_decimal_places() {
        let batch = build_batch(&[(139.123456789, 35.987654321)], &[1.0], &["x"]);
        let out = record_batches_to_geojson(&[batch], "geometry", "area_in_meters").unwrap();
        // 6-dp rounding: 139.123457, and no 7th-place digit sequence present.
        assert!(out.geojson.contains("139.123457"), "{}", out.geojson);
        assert!(!out.geojson.contains("139.1234567"));
    }

    #[test]
    fn null_geometry_rows_are_skipped() {
        use std::sync::Arc;
        let raw = wkb(&Geometry::Point(Point::new_2d(1.0, 2.0)));
        let geom_refs: Vec<Option<&[u8]>> = vec![Some(raw.as_slice()), None];
        let geom = Arc::new(BinaryArray::from(geom_refs)) as arrow_array::ArrayRef;
        let area = Arc::new(Float64Array::from(vec![10.0, 20.0])) as arrow_array::ArrayRef;
        let batch = RecordBatch::try_from_iter_with_nullable(vec![
            ("geometry", geom, true),
            ("area_in_meters", area, false),
        ])
        .expect("batch");
        let out = record_batches_to_geojson(&[batch], "geometry", "area_in_meters").unwrap();
        // Only the non-null geometry row is emitted; area sums only that row.
        assert_eq!(out.matched, 1);
        assert!((out.total_area_m2 - 10.0).abs() < 1e-6);
    }

    #[test]
    fn missing_area_column_yields_zero_total() {
        use std::sync::Arc;
        let raw = wkb(&Geometry::Point(Point::new_2d(1.0, 2.0)));
        let geom = Arc::new(BinaryArray::from(vec![Some(raw.as_slice())])) as arrow_array::ArrayRef;
        let batch = RecordBatch::try_from_iter_with_nullable(vec![("geometry", geom, true)])
            .expect("batch");
        let out = record_batches_to_geojson(&[batch], "geometry", "area_in_meters").unwrap();
        assert_eq!(out.matched, 1);
        assert_eq!(out.total_area_m2, 0.0);
    }

    #[test]
    fn missing_geometry_column_errors() {
        use std::sync::Arc;
        let area = Arc::new(Float64Array::from(vec![1.0])) as arrow_array::ArrayRef;
        let batch = RecordBatch::try_from_iter_with_nullable(vec![("area_in_meters", area, false)])
            .expect("batch");
        let err = record_batches_to_geojson(&[batch], "geometry", "area_in_meters").unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    // ── MANDATORY parity test ────────────────────────────────────────────────
    //
    // Verifies the remote pipeline core: metadata-only `plan_pushdown` selects
    // row-group column-chunk byte ranges; those exact bytes are sliced out of a
    // real Parquet file into a `SparseChunkReader`; `execute_pushdown` over the
    // sparse reader produces results **identical** to a full-file
    // `GeoParquetReader::open(...).read_pushdown()`.  This is the native stand-in
    // for the browser's fetch → decode path (`session.rs` is wasm-only).

    use std::sync::Arc;

    use arrow_array::ArrayRef;
    use bytes::Bytes;
    use oxigeo_geoparquet::GeoParquetReader;
    use oxigeo_geoparquet::metadata::{Crs, GeoParquetMetadata, GeometryColumnMetadata};
    use oxigeo_geoparquet::plan::plan_pushdown;
    use oxigeo_geoparquet::predicate::{AttributeFilter, CmpOp, ScalarValue};
    use oxigeo_geoparquet::pushdown::execute_pushdown;
    use parquet::arrow::ArrowWriter;
    use parquet::arrow::arrow_reader::{ArrowReaderMetadata, ParquetRecordBatchReaderBuilder};
    use parquet::basic::Compression;
    use parquet::file::properties::WriterProperties;

    use crate::sparse::{Segment, SparseChunkReader};

    const GEOM: &str = "geometry";

    /// The VIDA-mirroring `geo` metadata (WKB, WGS84).  No `covering` object —
    /// exercising the WKB post-filter fallback path in `execute_pushdown`.
    fn geo_meta() -> GeoParquetMetadata {
        let col = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());
        let mut geo = GeoParquetMetadata::new(GEOM);
        geo.add_column(GEOM, col);
        geo
    }

    /// One row group of `n` points at `(cx, cy)`, area starting at `area_base`.
    fn zone_batch(
        template: &arrow_array::RecordBatch,
        cx: f64,
        cy: f64,
        n: usize,
        area_base: f64,
        name: &str,
    ) -> arrow_array::RecordBatch {
        let raw: Vec<Vec<u8>> = (0..n)
            .map(|_| wkb(&Geometry::Point(Point::new_2d(cx, cy))))
            .collect();
        let geom_refs: Vec<Option<&[u8]>> = raw.iter().map(|v| Some(v.as_slice())).collect();
        let geom = Arc::new(BinaryArray::from(geom_refs)) as ArrayRef;
        let areas: Vec<f64> = (0..n).map(|i| area_base + i as f64).collect();
        let area = Arc::new(Float64Array::from(areas)) as ArrayRef;
        let conf = Arc::new(Float64Array::from(vec![0.9; n])) as ArrayRef;
        let names = Arc::new(StringArray::from(vec![name; n])) as ArrayRef;
        arrow_array::RecordBatch::try_new(template.schema(), vec![geom, area, conf, names])
            .expect("zone batch")
    }

    #[test]
    fn parity_sparse_equals_full_read_pushdown() {
        // ── Build a 3-row-group SNAPPY fixture with `geo` metadata ───────────
        let dir = std::env::temp_dir();
        let path = dir.join(format!("oxigeo_c4_parity_{}.parquet", std::process::id()));

        // Template batch → schema; then stamp the `geo` metadata onto it.
        let g0 = wkb(&Geometry::Point(Point::new_2d(0.0, 0.0)));
        let mut template = arrow_array::RecordBatch::try_from_iter_with_nullable(vec![
            (
                GEOM,
                Arc::new(BinaryArray::from(vec![Some(g0.as_slice())])) as ArrayRef,
                true,
            ),
            (
                "area_in_meters",
                Arc::new(Float64Array::from(vec![1.0])) as ArrayRef,
                false,
            ),
            (
                "confidence",
                Arc::new(Float64Array::from(vec![0.9])) as ArrayRef,
                false,
            ),
            (
                "name",
                Arc::new(StringArray::from(vec!["seed"])) as ArrayRef,
                false,
            ),
        ])
        .expect("template");
        template
            .schema_metadata_mut()
            .insert("geo".to_string(), geo_meta().to_json().expect("geo json"));

        let rg0 = zone_batch(&template, 139.70, 35.68, 5, 100.0, "zone_a");
        let rg1 = zone_batch(&template, 139.90, 35.90, 5, 5000.0, "zone_b");
        let rg2 = zone_batch(&template, 140.10, 36.10, 5, 9000.0, "zone_c");

        {
            let file = std::fs::File::create(&path).expect("create fixture");
            let props = WriterProperties::builder()
                .set_compression(Compression::SNAPPY)
                .build();
            let mut writer =
                ArrowWriter::try_new(file, template.schema(), Some(props)).expect("writer");
            for batch in [&rg0, &rg1, &rg2] {
                writer.write(batch).expect("write");
                writer.flush().expect("flush"); // one row group per batch
            }
            writer.close().expect("close");
        }

        // Query: box covering zone_a + zone_b, area > 3000 (drops zone_a rows;
        // zone_c is spatially outside the box).
        let bbox = Some((139.6, 35.6, 139.95, 35.95));
        let filters = vec![AttributeFilter::Cmp {
            col: "area_in_meters".into(),
            op: CmpOp::Gt,
            value: ScalarValue::Float64(3000.0),
        }];
        let output_columns: Vec<String> = ["geometry", "area_in_meters", "confidence", "name"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        // ── Sparse path: plan → slice real bytes → execute_pushdown ──────────
        let file_bytes = Bytes::from(std::fs::read(&path).expect("read fixture"));
        let builder =
            ParquetRecordBatchReaderBuilder::try_new(file_bytes.clone()).expect("builder");
        let parquet_meta = builder.metadata().clone();
        let arrow_meta = ArrowReaderMetadata::try_new(parquet_meta.clone(), Default::default())
            .expect("arrow meta");
        let geo = geo_meta();

        let plan = plan_pushdown(&parquet_meta, &geo, GEOM, bbox, &filters, &output_columns)
            .expect("plan");

        // zone_a (RG0) must be pruned by attribute statistics (area max < 3000).
        assert!(
            !plan.row_groups.contains(&0),
            "RG0 should be pruned by area stats, survivors={:?}",
            plan.row_groups
        );
        assert!(plan.row_groups.contains(&1), "RG1 must survive");

        let segments: Vec<Segment> = plan
            .ranges
            .iter()
            .map(|r| {
                let start = r.start as usize;
                let end = start + r.length as usize;
                Segment {
                    start: r.start,
                    data: file_bytes.slice(start..end),
                }
            })
            .collect();
        let sparse = SparseChunkReader::new(file_bytes.len() as u64, segments);

        let sparse_batches = execute_pushdown(
            sparse,
            arrow_meta,
            &geo,
            GEOM,
            bbox,
            &filters,
            plan.row_groups.clone(),
            None,
            None,
        )
        .expect("sparse execute");

        // ── Full path: GeoParquetReader::read_pushdown ───────────────────────
        let full = GeoParquetReader::open(&path).expect("open");
        let full_batches = full
            .with_bbox_filter((139.6, 35.6, 139.95, 35.95))
            .with_attribute_filters(filters.clone())
            .read_pushdown()
            .expect("full read_pushdown");

        // ── Compare via GeoJSON conversion ───────────────────────────────────
        let sparse_out =
            record_batches_to_geojson(&sparse_batches, GEOM, "area_in_meters").expect("sparse gj");
        let full_out =
            record_batches_to_geojson(&full_batches, GEOM, "area_in_meters").expect("full gj");

        assert_eq!(
            sparse_out.matched, full_out.matched,
            "matched count differs (sparse={}, full={})",
            sparse_out.matched, full_out.matched
        );
        assert!(sparse_out.matched > 0, "query should match some rows");
        assert!(
            (sparse_out.total_area_m2 - full_out.total_area_m2).abs() < 1e-6,
            "total area differs"
        );
        assert_eq!(
            sparse_out.geojson, full_out.geojson,
            "GeoJSON output differs between sparse and full read"
        );

        let _ = std::fs::remove_file(&path);
    }
}
