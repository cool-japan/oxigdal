//! Integration tests for `oxigeo_query::executor::spatial_funcs`.
//!
//! Covers the public dispatch entry point [`evaluate_spatial_function`] for
//! every ST_* function family plus one true end-to-end test that goes through
//! the SQL parser and the executor's filter pipeline.

#![allow(clippy::panic)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![allow(clippy::iter_cloned_collect)]

use oxigeo_query::Result;
use oxigeo_query::error::QueryError;
use oxigeo_query::executor::Executor;
use oxigeo_query::executor::evaluate_spatial_function;
use oxigeo_query::executor::filter::Value;
use oxigeo_query::executor::scan::{
    ColumnData, DataType, Field, MemoryDataSource, RecordBatch, Schema,
};
use oxigeo_query::parser::sql::parse_sql;
use std::sync::Arc;

// ── Convenience builders ────────────────────────────────────────────────────

fn wkt(s: &str) -> Value {
    Value::String(s.to_string())
}

fn float(v: f64) -> Value {
    Value::Float64(v)
}

fn expect_bool(v: Value) -> bool {
    match v {
        Value::Boolean(b) => b,
        other => panic!("expected Value::Boolean, got {:?}", other),
    }
}

fn expect_f64(v: Value) -> f64 {
    match v {
        Value::Float64(f) => f,
        other => panic!("expected Value::Float64, got {:?}", other),
    }
}

fn expect_geometry(v: Value) -> geo_types::Geometry<f64> {
    match v {
        Value::Geometry(g) => g,
        other => panic!("expected Value::Geometry, got {:?}", other),
    }
}

// ── Binary boolean predicates ───────────────────────────────────────────────

#[test]
fn test_st_intersects_two_overlapping_polygons_returns_true() {
    let a = wkt("POLYGON((0 0, 4 0, 4 4, 0 4, 0 0))");
    let b = wkt("POLYGON((2 2, 6 2, 6 6, 2 6, 2 2))");
    let v = evaluate_spatial_function("ST_Intersects", &[a, b], 2).expect("ok");
    assert!(expect_bool(v));
}

#[test]
fn test_st_intersects_disjoint_polygons_returns_false() {
    let a = wkt("POLYGON((0 0, 1 0, 1 1, 0 1, 0 0))");
    let b = wkt("POLYGON((10 10, 11 10, 11 11, 10 11, 10 10))");
    let v = evaluate_spatial_function("st_intersects", &[a, b], 2).expect("ok");
    assert!(!expect_bool(v));
}

#[test]
fn test_st_contains_polygon_point_inside() {
    let poly = wkt("POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))");
    let pt = wkt("POINT(5 5)");
    let v = evaluate_spatial_function("ST_Contains", &[poly, pt], 2).expect("ok");
    assert!(expect_bool(v));
}

#[test]
fn test_st_contains_polygon_point_outside_false() {
    let poly = wkt("POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))");
    let pt = wkt("POINT(100 100)");
    let v = evaluate_spatial_function("ST_Contains", &[poly, pt], 2).expect("ok");
    assert!(!expect_bool(v));
}

#[test]
fn test_st_within_swaps_arguments_of_contains() {
    // Point WITHIN Polygon should equal Polygon CONTAINS Point.
    let poly = wkt("POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))");
    let pt = wkt("POINT(5 5)");
    let v = evaluate_spatial_function("ST_Within", &[pt.clone(), poly.clone()], 2).expect("ok");
    assert!(expect_bool(v));
    // Outside should be false.
    let pt_out = wkt("POINT(100 100)");
    let v2 = evaluate_spatial_function("ST_Within", &[pt_out, poly], 2).expect("ok");
    assert!(!expect_bool(v2));
}

#[test]
fn test_st_disjoint_complements_intersects() {
    let a = wkt("POLYGON((0 0, 1 0, 1 1, 0 1, 0 0))");
    let b = wkt("POLYGON((10 10, 11 10, 11 11, 10 11, 10 10))");
    let v = evaluate_spatial_function("ST_Disjoint", &[a.clone(), b.clone()], 2).expect("ok");
    assert!(expect_bool(v));
    // Same polygons → not disjoint.
    let v2 = evaluate_spatial_function("ST_Disjoint", &[a.clone(), a.clone()], 2).expect("ok");
    assert!(!expect_bool(v2));
}

// ── Metric / distance functions ─────────────────────────────────────────────

#[test]
fn test_st_distance_two_points() {
    let a = wkt("POINT(0 0)");
    let b = wkt("POINT(3 4)");
    let v = evaluate_spatial_function("ST_Distance", &[a, b], 2).expect("ok");
    let d = expect_f64(v);
    assert!((d - 5.0).abs() < 1e-9, "expected 5, got {}", d);
}

#[test]
fn test_st_dwithin_returns_true_under_threshold() {
    let a = wkt("POINT(0 0)");
    let b = wkt("POINT(3 4)");
    let v = evaluate_spatial_function("ST_DWithin", &[a, b, float(10.0)], 2).expect("ok");
    assert!(expect_bool(v));
}

#[test]
fn test_st_dwithin_returns_false_over_threshold() {
    let a = wkt("POINT(0 0)");
    let b = wkt("POINT(3 4)");
    let v = evaluate_spatial_function("ST_DWithin", &[a, b, float(1.0)], 2).expect("ok");
    assert!(!expect_bool(v));
}

#[test]
fn test_st_area_unit_square_equals_one() {
    let sq = wkt("POLYGON((0 0, 1 0, 1 1, 0 1, 0 0))");
    let v = evaluate_spatial_function("ST_Area", &[sq], 2).expect("ok");
    let area = expect_f64(v);
    assert!((area - 1.0).abs() < 1e-9, "expected 1, got {}", area);
}

#[test]
fn test_st_length_unit_segment_equals_one() {
    let seg = wkt("LINESTRING(0 0, 1 0)");
    let v = evaluate_spatial_function("ST_Length", &[seg], 2).expect("ok");
    let len = expect_f64(v);
    assert!((len - 1.0).abs() < 1e-9, "expected 1, got {}", len);
}

// ── Constructors ────────────────────────────────────────────────────────────

#[test]
fn test_st_centroid_unit_square_at_half_half() {
    let sq = wkt("POLYGON((0 0, 1 0, 1 1, 0 1, 0 0))");
    let v = evaluate_spatial_function("ST_Centroid", &[sq], 2).expect("ok");
    let g = expect_geometry(v);
    if let geo_types::Geometry::Point(p) = g {
        let (x, y) = (p.x(), p.y());
        assert!((x - 0.5).abs() < 1e-9, "x = {}", x);
        assert!((y - 0.5).abs() < 1e-9, "y = {}", y);
    } else {
        panic!("expected Point centroid, got {:?}", g);
    }
}

#[test]
fn test_st_envelope_polygon_returns_aabb() {
    // Diamond shape; AABB should be the unit square (0,0)-(2,2).
    let diamond = wkt("POLYGON((1 0, 2 1, 1 2, 0 1, 1 0))");
    let v = evaluate_spatial_function("ST_Envelope", &[diamond], 2).expect("ok");
    let g = expect_geometry(v);
    if let geo_types::Geometry::Polygon(p) = g {
        let coords: Vec<_> = p.exterior().0.iter().copied().collect();
        // 5 coords = 4 corners + closing repeat
        assert_eq!(coords.len(), 5);
        let xs: Vec<f64> = coords.iter().map(|c| c.x).collect();
        let ys: Vec<f64> = coords.iter().map(|c| c.y).collect();
        let xmin = xs.iter().cloned().fold(f64::INFINITY, f64::min);
        let xmax = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let ymin = ys.iter().cloned().fold(f64::INFINITY, f64::min);
        let ymax = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        assert!((xmin - 0.0).abs() < 1e-9);
        assert!((xmax - 2.0).abs() < 1e-9);
        assert!((ymin - 0.0).abs() < 1e-9);
        assert!((ymax - 2.0).abs() < 1e-9);
    } else {
        panic!("expected Polygon envelope, got {:?}", g);
    }
}

// ── Error paths ─────────────────────────────────────────────────────────────

#[test]
fn test_evaluate_unknown_function_name_returns_typed_error() {
    let err = evaluate_spatial_function("ST_NotAReal_Func", &[], 2).unwrap_err();
    match err {
        QueryError::FunctionNotFound(name) => {
            assert!(
                name.to_uppercase().contains("ST_NOTAREAL_FUNC"),
                "unexpected name: {}",
                name
            );
        }
        other => panic!("expected FunctionNotFound, got {:?}", other),
    }
}

#[test]
fn test_evaluate_wrong_arity_returns_typed_error() {
    // ST_Intersects requires 2 args; supply 0 and 1.
    let err = evaluate_spatial_function("ST_Intersects", &[], 2).unwrap_err();
    assert!(matches!(err, QueryError::InvalidArgument(_)));

    let err = evaluate_spatial_function(
        "ST_Intersects",
        &[Value::String("POINT(0 0)".to_string())],
        2,
    )
    .unwrap_err();
    assert!(matches!(err, QueryError::InvalidArgument(_)));
}

// ── End-to-end via the SQL parser + executor pipeline ───────────────────────

fn make_geom_data() -> Result<(Arc<Schema>, Vec<RecordBatch>)> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id".to_string(), DataType::Int64, false),
        Field::new("geom".to_string(), DataType::String, false),
    ]));
    let columns = vec![
        ColumnData::Int64(vec![Some(1), Some(2), Some(3)]),
        ColumnData::String(vec![
            Some("POINT(0 0)".to_string()),     // inside test polygon
            Some("POINT(5 5)".to_string()),     // inside test polygon
            Some("POINT(100 100)".to_string()), // outside test polygon
        ]),
    ];
    let batch = RecordBatch::new(schema.clone(), columns, 3)?;
    Ok((schema, vec![batch]))
}

#[tokio::test]
async fn test_filter_st_intersects_via_executor_end_to_end() -> Result<()> {
    let (schema, batches) = make_geom_data()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut executor = Executor::new();
    executor.register_data_source("features".to_string(), source);

    // The literal polygon covers (-10..10) x (-10..10), so rows 1 and 2 match.
    let sql = "SELECT id FROM features \
               WHERE ST_Intersects(geom, 'POLYGON((-10 -10, 10 -10, 10 10, -10 10, -10 -10))')";
    let stmt = parse_sql(sql)?;
    let results = executor.execute(&stmt).await?;

    let total_rows: usize = results.iter().map(|b| b.num_rows).sum();
    assert_eq!(
        total_rows, 2,
        "expected 2 matching rows, got {}",
        total_rows
    );

    Ok(())
}
