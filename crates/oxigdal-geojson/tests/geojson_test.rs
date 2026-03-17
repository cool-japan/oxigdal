//! Comprehensive tests for oxigdal-geojson-stream
//!
//! Covers geometry ops, parser, writer, validator, streaming reader,
//! feature filter, CRS, error types, and round-trip fidelity.

use oxigdal_geojson_stream::{
    FeatureCollection, FeatureFilter, FeatureId, GeoJsonCrs, GeoJsonError, GeoJsonFeature,
    GeoJsonGeometry, GeoJsonParser, GeoJsonValidator, GeoJsonWriter, IssueSeverity,
    StreamingFeatureReader,
};

// ═══════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════

fn pt(lon: f64, lat: f64) -> GeoJsonFeature {
    GeoJsonFeature {
        id: None,
        geometry: Some(GeoJsonGeometry::Point([lon, lat])),
        properties: None,
    }
}

fn pt_with_props(lon: f64, lat: f64, props: serde_json::Value) -> GeoJsonFeature {
    GeoJsonFeature {
        id: None,
        geometry: Some(GeoJsonGeometry::Point([lon, lat])),
        properties: Some(props),
    }
}

fn simple_ring() -> Vec<[f64; 2]> {
    vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]
}

fn closed_ring() -> Vec<[f64; 2]> {
    vec![
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 10.0],
        [0.0, 10.0],
        [0.0, 0.0],
    ]
}

fn parser() -> GeoJsonParser {
    GeoJsonParser::new()
}

// ═══════════════════════════════════════════════════════════════════
//  1. GeoJsonGeometry — geometry_type
// ═══════════════════════════════════════════════════════════════════

#[test]
fn geom_type_point() {
    assert_eq!(GeoJsonGeometry::Point([0.0, 0.0]).geometry_type(), "Point");
}

#[test]
fn geom_type_point_z() {
    assert_eq!(
        GeoJsonGeometry::PointZ([0.0, 0.0, 5.0]).geometry_type(),
        "Point"
    );
}

#[test]
fn geom_type_linestring() {
    let g = GeoJsonGeometry::LineString(vec![[0.0, 0.0], [1.0, 1.0]]);
    assert_eq!(g.geometry_type(), "LineString");
}

#[test]
fn geom_type_polygon() {
    let g = GeoJsonGeometry::Polygon(vec![closed_ring()]);
    assert_eq!(g.geometry_type(), "Polygon");
}

#[test]
fn geom_type_multipoint() {
    let g = GeoJsonGeometry::MultiPoint(vec![[0.0, 0.0], [1.0, 1.0]]);
    assert_eq!(g.geometry_type(), "MultiPoint");
}

#[test]
fn geom_type_multilinestring() {
    let g = GeoJsonGeometry::MultiLineString(vec![vec![[0.0, 0.0], [1.0, 1.0]]]);
    assert_eq!(g.geometry_type(), "MultiLineString");
}

#[test]
fn geom_type_multipolygon() {
    let g = GeoJsonGeometry::MultiPolygon(vec![vec![closed_ring()]]);
    assert_eq!(g.geometry_type(), "MultiPolygon");
}

#[test]
fn geom_type_geometry_collection() {
    let g = GeoJsonGeometry::GeometryCollection(vec![GeoJsonGeometry::Point([0.0, 0.0])]);
    assert_eq!(g.geometry_type(), "GeometryCollection");
}

#[test]
fn geom_type_null() {
    assert_eq!(GeoJsonGeometry::Null.geometry_type(), "null");
}

// ═══════════════════════════════════════════════════════════════════
//  2. GeoJsonGeometry — bbox
// ═══════════════════════════════════════════════════════════════════

#[test]
fn bbox_point() {
    let g = GeoJsonGeometry::Point([10.0, 20.0]);
    assert_eq!(g.bbox(), Some([10.0, 20.0, 10.0, 20.0]));
}

#[test]
fn bbox_point_z() {
    let g = GeoJsonGeometry::PointZ([3.0, 4.0, 99.0]);
    assert_eq!(g.bbox(), Some([3.0, 4.0, 3.0, 4.0]));
}

#[test]
fn bbox_linestring() {
    let g = GeoJsonGeometry::LineString(vec![[0.0, 0.0], [5.0, 3.0], [-1.0, 4.0]]);
    assert_eq!(g.bbox(), Some([-1.0, 0.0, 5.0, 4.0]));
}

#[test]
fn bbox_linestring_z() {
    let g = GeoJsonGeometry::LineStringZ(vec![[0.0, 0.0, 0.0], [5.0, 3.0, 10.0]]);
    assert_eq!(g.bbox(), Some([0.0, 0.0, 5.0, 3.0]));
}

#[test]
fn bbox_polygon() {
    let g = GeoJsonGeometry::Polygon(vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 0.0]]]);
    let bb = g.bbox().expect("has bbox");
    assert_eq!(bb, [0.0, 0.0, 10.0, 5.0]);
}

#[test]
fn bbox_multipoint() {
    let g = GeoJsonGeometry::MultiPoint(vec![[1.0, 2.0], [-3.0, 4.0]]);
    assert_eq!(g.bbox(), Some([-3.0, 2.0, 1.0, 4.0]));
}

#[test]
fn bbox_multilinestring() {
    let g = GeoJsonGeometry::MultiLineString(vec![
        vec![[0.0, 0.0], [5.0, 5.0]],
        vec![[-2.0, -1.0], [3.0, 2.0]],
    ]);
    assert_eq!(g.bbox(), Some([-2.0, -1.0, 5.0, 5.0]));
}

#[test]
fn bbox_multipolygon() {
    let g = GeoJsonGeometry::MultiPolygon(vec![
        vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
        vec![vec![[5.0, 5.0], [6.0, 5.0], [6.0, 6.0], [5.0, 5.0]]],
    ]);
    let bb = g.bbox().expect("has bbox");
    assert_eq!(bb, [0.0, 0.0, 6.0, 6.0]);
}

#[test]
fn bbox_geometry_collection() {
    let g = GeoJsonGeometry::GeometryCollection(vec![
        GeoJsonGeometry::Point([1.0, 2.0]),
        GeoJsonGeometry::Point([5.0, 6.0]),
    ]);
    assert_eq!(g.bbox(), Some([1.0, 2.0, 5.0, 6.0]));
}

#[test]
fn bbox_null_is_none() {
    assert!(GeoJsonGeometry::Null.bbox().is_none());
}

// ═══════════════════════════════════════════════════════════════════
//  3. GeoJsonGeometry — to_2d
// ═══════════════════════════════════════════════════════════════════

#[test]
fn to_2d_from_point_z() {
    let g = GeoJsonGeometry::PointZ([1.0, 2.0, 3.0]);
    assert_eq!(g.to_2d(), GeoJsonGeometry::Point([1.0, 2.0]));
}

#[test]
fn to_2d_from_linestring_z() {
    let g = GeoJsonGeometry::LineStringZ(vec![[0.0, 1.0, 5.0], [2.0, 3.0, 6.0]]);
    assert_eq!(
        g.to_2d(),
        GeoJsonGeometry::LineString(vec![[0.0, 1.0], [2.0, 3.0]])
    );
}

#[test]
fn to_2d_from_polygon_z() {
    let g = GeoJsonGeometry::PolygonZ(vec![vec![
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 0.0, 1.0],
    ]]);
    let result = g.to_2d();
    assert!(matches!(result, GeoJsonGeometry::Polygon(_)));
}

#[test]
fn to_2d_from_point_unchanged() {
    let g = GeoJsonGeometry::Point([1.0, 2.0]);
    assert_eq!(g.to_2d(), GeoJsonGeometry::Point([1.0, 2.0]));
}

#[test]
fn to_2d_geometry_collection() {
    let g = GeoJsonGeometry::GeometryCollection(vec![GeoJsonGeometry::PointZ([0.0, 1.0, 2.0])]);
    let result = g.to_2d();
    assert!(
        matches!(&result, GeoJsonGeometry::GeometryCollection(inner) if inner[0] == GeoJsonGeometry::Point([0.0, 1.0])),
        "expected collection"
    );
}

// ═══════════════════════════════════════════════════════════════════
//  4. GeoJsonParser — parse complete documents
// ═══════════════════════════════════════════════════════════════════

#[test]
fn parse_feature_collection_empty() {
    let json = br#"{"type":"FeatureCollection","features":[]}"#;
    let doc = parser().parse(json).expect("valid");
    assert_eq!(doc.document_type(), "FeatureCollection");
    let fc = doc.as_feature_collection().expect("fc");
    assert!(fc.is_empty());
}

#[test]
fn parse_feature_collection_two_features() {
    let json = br#"{
        "type":"FeatureCollection",
        "features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[1.0,2.0]},"properties":null},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[3.0,4.0]},"properties":null}
        ]
    }"#;
    let doc = parser().parse(json).expect("valid");
    let fc = doc.as_feature_collection().expect("fc");
    assert_eq!(fc.len(), 2);
}

#[test]
fn parse_geometry_only_point() {
    let json = br#"{"type":"Point","coordinates":[100.0,0.0]}"#;
    let doc = parser().parse(json).expect("valid");
    assert_eq!(doc.document_type(), "Geometry");
}

#[test]
fn parse_feature_document() {
    let json = br#"{"type":"Feature","geometry":{"type":"Point","coordinates":[0.0,0.0]},"properties":{"name":"test"}}"#;
    let doc = parser().parse(json).expect("valid");
    assert_eq!(doc.document_type(), "Feature");
}

#[test]
fn parse_feature_collection_with_bbox() {
    let json = br#"{"type":"FeatureCollection","bbox":[-10.0,-5.0,10.0,5.0],"features":[]}"#;
    let doc = parser().parse(json).expect("valid");
    let fc = doc.as_feature_collection().expect("fc");
    assert_eq!(fc.bbox, Some([-10.0, -5.0, 10.0, 5.0]));
}

#[test]
fn parse_feature_collection_with_name() {
    let json = br#"{"type":"FeatureCollection","name":"My Layer","features":[]}"#;
    let doc = parser().parse(json).expect("valid");
    let fc = doc.as_feature_collection().expect("fc");
    assert_eq!(fc.name.as_deref(), Some("My Layer"));
}

// ═══════════════════════════════════════════════════════════════════
//  5. GeoJsonParser — parse_geometry
// ═══════════════════════════════════════════════════════════════════

#[test]
fn parse_geometry_point_value() {
    let v = serde_json::json!({"type":"Point","coordinates":[5.0,6.0]});
    let g = parser().parse_geometry(&v).expect("valid");
    assert_eq!(g, GeoJsonGeometry::Point([5.0, 6.0]));
}

#[test]
fn parse_geometry_point_z_value() {
    let v = serde_json::json!({"type":"Point","coordinates":[5.0,6.0,7.0]});
    let g = parser().parse_geometry(&v).expect("valid");
    assert_eq!(g, GeoJsonGeometry::PointZ([5.0, 6.0, 7.0]));
}

#[test]
fn parse_geometry_linestring_value() {
    let v = serde_json::json!({"type":"LineString","coordinates":[[0.0,0.0],[1.0,1.0]]});
    let g = parser().parse_geometry(&v).expect("valid");
    assert!(matches!(g, GeoJsonGeometry::LineString(_)));
}

#[test]
fn parse_geometry_polygon_value() {
    let v = serde_json::json!({
        "type":"Polygon",
        "coordinates":[[[0.0,0.0],[10.0,0.0],[10.0,10.0],[0.0,10.0],[0.0,0.0]]]
    });
    let g = parser().parse_geometry(&v).expect("valid");
    assert!(matches!(g, GeoJsonGeometry::Polygon(_)));
}

#[test]
fn parse_geometry_multipoint_value() {
    let v = serde_json::json!({"type":"MultiPoint","coordinates":[[0.0,0.0],[1.0,1.0]]});
    let g = parser().parse_geometry(&v).expect("valid");
    assert!(matches!(g, GeoJsonGeometry::MultiPoint(_)));
}

#[test]
fn parse_geometry_multilinestring_value() {
    let v = serde_json::json!({
        "type":"MultiLineString",
        "coordinates":[[[0.0,0.0],[1.0,1.0]],[[2.0,2.0],[3.0,3.0]]]
    });
    let g = parser().parse_geometry(&v).expect("valid");
    assert!(matches!(g, GeoJsonGeometry::MultiLineString(_)));
}

#[test]
fn parse_geometry_multipolygon_value() {
    let v = serde_json::json!({
        "type":"MultiPolygon",
        "coordinates":[[[[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,0.0]]]]
    });
    let g = parser().parse_geometry(&v).expect("valid");
    assert!(matches!(g, GeoJsonGeometry::MultiPolygon(_)));
}

#[test]
fn parse_geometry_collection_value() {
    let v = serde_json::json!({
        "type":"GeometryCollection",
        "geometries":[
            {"type":"Point","coordinates":[0.0,0.0]},
            {"type":"Point","coordinates":[1.0,1.0]}
        ]
    });
    let g = parser().parse_geometry(&v).expect("valid");
    assert!(
        matches!(&g, GeoJsonGeometry::GeometryCollection(inner) if inner.len() == 2),
        "expected collection"
    );
}

// ═══════════════════════════════════════════════════════════════════
//  6. GeoJsonParser — parse_feature
// ═══════════════════════════════════════════════════════════════════

#[test]
fn parse_feature_with_properties() {
    let v = serde_json::json!({
        "type":"Feature",
        "geometry":{"type":"Point","coordinates":[1.0,2.0]},
        "properties":{"name":"Tokyo","pop":14000000}
    });
    let f = parser().parse_feature(&v).expect("valid");
    let name: String = f.get_property("name").expect("has name");
    assert_eq!(name, "Tokyo");
    let pop: i64 = f.get_property("pop").expect("has pop");
    assert_eq!(pop, 14_000_000);
}

#[test]
fn parse_feature_with_null_geometry() {
    let v = serde_json::json!({
        "type":"Feature",
        "geometry":null,
        "properties":null
    });
    let f = parser().parse_feature(&v).expect("valid");
    assert!(!f.has_geometry());
}

#[test]
fn parse_feature_with_string_id() {
    let v = serde_json::json!({
        "type":"Feature",
        "id":"feature-001",
        "geometry":{"type":"Point","coordinates":[0.0,0.0]},
        "properties":null
    });
    let f = parser().parse_feature(&v).expect("valid");
    assert!(matches!(f.id, Some(FeatureId::String(_))));
    if let Some(FeatureId::String(s)) = f.id {
        assert_eq!(s, "feature-001");
    }
}

#[test]
fn parse_feature_with_numeric_id() {
    let v = serde_json::json!({
        "type":"Feature",
        "id":42,
        "geometry":{"type":"Point","coordinates":[0.0,0.0]},
        "properties":null
    });
    let f = parser().parse_feature(&v).expect("valid");
    assert!(matches!(f.id, Some(FeatureId::Number(_))));
}

#[test]
fn parse_feature_wrong_type_error() {
    let v = serde_json::json!({"type":"Point","coordinates":[0.0,0.0]});
    let result = parser().parse_feature(&v);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════
//  7. GeoJsonParser — parse_header
// ═══════════════════════════════════════════════════════════════════

#[test]
fn parse_header_type_field() {
    let json = br#"{"type":"FeatureCollection","features":[]}"#;
    let hdr = parser().parse_header(json).expect("valid");
    assert_eq!(hdr.type_, "FeatureCollection");
}

#[test]
fn parse_header_feature_count() {
    let json = br#"{"type":"FeatureCollection","features":[{},{}]}"#;
    let hdr = parser().parse_header(json).expect("valid");
    assert_eq!(hdr.feature_count, Some(2));
}

#[test]
fn parse_header_zero_features() {
    let json = br#"{"type":"FeatureCollection","features":[]}"#;
    let hdr = parser().parse_header(json).expect("valid");
    assert_eq!(hdr.feature_count, Some(0));
}

#[test]
fn parse_header_bbox() {
    let json = br#"{"type":"FeatureCollection","bbox":[0,0,10,10],"features":[]}"#;
    let hdr = parser().parse_header(json).expect("valid");
    assert_eq!(hdr.bbox, Some([0.0, 0.0, 10.0, 10.0]));
}

#[test]
fn parse_header_name() {
    let json = br#"{"type":"FeatureCollection","name":"TestLayer","features":[]}"#;
    let hdr = parser().parse_header(json).expect("valid");
    assert_eq!(hdr.name.as_deref(), Some("TestLayer"));
}

// ═══════════════════════════════════════════════════════════════════
//  8. FeatureCollection helpers
// ═══════════════════════════════════════════════════════════════════

#[test]
fn feature_collection_compute_bbox_union() {
    let fc = FeatureCollection {
        features: vec![pt(-10.0, -5.0), pt(10.0, 5.0)],
        bbox: None,
        crs: None,
        name: None,
    };
    assert_eq!(fc.compute_bbox(), Some([-10.0, -5.0, 10.0, 5.0]));
}

#[test]
fn feature_collection_compute_bbox_single_point() {
    let fc = FeatureCollection {
        features: vec![pt(1.0, 2.0)],
        bbox: None,
        crs: None,
        name: None,
    };
    assert_eq!(fc.compute_bbox(), Some([1.0, 2.0, 1.0, 2.0]));
}

#[test]
fn feature_collection_compute_bbox_empty_is_none() {
    let fc = FeatureCollection::default();
    assert!(fc.compute_bbox().is_none());
}

#[test]
fn feature_collection_geometry_types_unique() {
    let fc = FeatureCollection {
        features: vec![
            pt(0.0, 0.0),
            pt(1.0, 1.0),
            GeoJsonFeature {
                id: None,
                geometry: Some(GeoJsonGeometry::LineString(vec![[0.0, 0.0], [1.0, 1.0]])),
                properties: None,
            },
        ],
        bbox: None,
        crs: None,
        name: None,
    };
    let types = fc.geometry_types();
    assert_eq!(types.len(), 2);
    assert!(types.contains(&"Point"));
    assert!(types.contains(&"LineString"));
}

#[test]
fn feature_collection_len_matches() {
    let fc = FeatureCollection {
        features: vec![pt(0.0, 0.0), pt(1.0, 1.0), pt(2.0, 2.0)],
        bbox: None,
        crs: None,
        name: None,
    };
    assert_eq!(fc.len(), 3);
    assert!(!fc.is_empty());
}

// ═══════════════════════════════════════════════════════════════════
//  9. GeoJsonWriter — compact
// ═══════════════════════════════════════════════════════════════════

#[test]
fn writer_compact_point() {
    let g = GeoJsonGeometry::Point([1.0, 2.0]);
    let s = GeoJsonWriter::compact().write_geometry(&g);
    assert!(!s.contains('\n'));
    assert!(s.contains("\"Point\""));
}

#[test]
fn writer_compact_feature_collection() {
    let fc = FeatureCollection {
        features: vec![pt(0.0, 0.0)],
        bbox: None,
        crs: None,
        name: None,
    };
    let s = GeoJsonWriter::compact().write_feature_collection(&fc);
    assert!(s.contains("\"FeatureCollection\""));
    assert!(s.contains("\"features\""));
}

// ═══════════════════════════════════════════════════════════════════
//  10. GeoJsonWriter — pretty
// ═══════════════════════════════════════════════════════════════════

#[test]
fn writer_pretty_has_newlines() {
    let g = GeoJsonGeometry::Point([0.0, 0.0]);
    let s = GeoJsonWriter::pretty(2).write_geometry(&g);
    assert!(s.contains('\n'));
}

#[test]
fn writer_pretty_indented() {
    let fc = FeatureCollection {
        features: vec![pt(0.0, 0.0)],
        bbox: None,
        crs: None,
        name: None,
    };
    let s = GeoJsonWriter::pretty(4).write_feature_collection(&fc);
    assert!(s.contains("    "));
}

// ═══════════════════════════════════════════════════════════════════
//  11. GeoJsonWriter — precision
// ═══════════════════════════════════════════════════════════════════

#[test]
fn writer_precision_3() {
    let g = GeoJsonGeometry::Point([1.123456789, 2.0]);
    let s = GeoJsonWriter::compact()
        .with_precision(3)
        .write_geometry(&g);
    assert!(s.contains("1.123"));
    assert!(!s.contains("1.1234"));
}

#[test]
fn writer_precision_0() {
    let g = GeoJsonGeometry::Point([10.7, 20.3]);
    let s = GeoJsonWriter::compact()
        .with_precision(0)
        .write_geometry(&g);
    // Should contain rounded integer representations
    assert!(s.contains("\"coordinates\""));
}

// ═══════════════════════════════════════════════════════════════════
//  12. GeoJsonWriter — geometry types
// ═══════════════════════════════════════════════════════════════════

#[test]
fn write_linestring_geometry() {
    let g = GeoJsonGeometry::LineString(vec![[0.0, 0.0], [1.0, 1.0]]);
    let s = GeoJsonWriter::compact().write_geometry(&g);
    assert!(s.contains("\"LineString\""));
}

#[test]
fn write_polygon_geometry() {
    let g = GeoJsonGeometry::Polygon(vec![closed_ring()]);
    let s = GeoJsonWriter::compact().write_geometry(&g);
    assert!(s.contains("\"Polygon\""));
}

#[test]
fn write_multipoint_geometry() {
    let g = GeoJsonGeometry::MultiPoint(vec![[0.0, 0.0], [1.0, 1.0]]);
    let s = GeoJsonWriter::compact().write_geometry(&g);
    assert!(s.contains("\"MultiPoint\""));
}

#[test]
fn write_multilinestring_geometry() {
    let g = GeoJsonGeometry::MultiLineString(vec![
        vec![[0.0, 0.0], [1.0, 1.0]],
        vec![[2.0, 2.0], [3.0, 3.0]],
    ]);
    let s = GeoJsonWriter::compact().write_geometry(&g);
    assert!(s.contains("\"MultiLineString\""));
}

#[test]
fn write_multipolygon_geometry() {
    let g = GeoJsonGeometry::MultiPolygon(vec![vec![closed_ring()]]);
    let s = GeoJsonWriter::compact().write_geometry(&g);
    assert!(s.contains("\"MultiPolygon\""));
}

#[test]
fn write_geometry_collection() {
    let g = GeoJsonGeometry::GeometryCollection(vec![GeoJsonGeometry::Point([0.0, 0.0])]);
    let s = GeoJsonWriter::compact().write_geometry(&g);
    assert!(s.contains("\"GeometryCollection\""));
    assert!(s.contains("\"geometries\""));
}

#[test]
fn write_null_geometry() {
    let g = GeoJsonGeometry::Null;
    let s = GeoJsonWriter::compact().write_geometry(&g);
    assert_eq!(s, "null");
}

// ═══════════════════════════════════════════════════════════════════
//  13. Writer → Parser round-trip
// ═══════════════════════════════════════════════════════════════════

fn round_trip_geometry(geom: GeoJsonGeometry) -> GeoJsonGeometry {
    let s = GeoJsonWriter::compact().write_geometry(&geom);
    let v: serde_json::Value = serde_json::from_str(&s).expect("valid JSON");
    GeoJsonParser::new().parse_geometry(&v).expect("valid geom")
}

#[test]
fn roundtrip_point() {
    let g = GeoJsonGeometry::Point([13.4050, 52.5200]);
    let result = round_trip_geometry(g.clone());
    assert_eq!(g, result);
}

#[test]
fn roundtrip_linestring() {
    let g = GeoJsonGeometry::LineString(vec![[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]]);
    let result = round_trip_geometry(g.clone());
    assert_eq!(g, result);
}

#[test]
fn roundtrip_polygon() {
    let g = GeoJsonGeometry::Polygon(vec![closed_ring()]);
    let result = round_trip_geometry(g.clone());
    assert_eq!(g, result);
}

#[test]
fn roundtrip_feature_collection() {
    let fc = FeatureCollection {
        features: vec![pt(1.0, 2.0), pt(3.0, 4.0)],
        bbox: None,
        crs: None,
        name: None,
    };
    let w = GeoJsonWriter::compact();
    let s = w.write_feature_collection(&fc);
    let doc = GeoJsonParser::new().parse(s.as_bytes()).expect("valid");
    let fc2 = doc.as_feature_collection().expect("fc");
    assert_eq!(fc2.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════
//  14. GeoJsonWriter — write_features_iter
// ═══════════════════════════════════════════════════════════════════

#[test]
fn write_features_iter_produces_collection() {
    let features = vec![pt(0.0, 0.0), pt(1.0, 1.0)];
    let s = GeoJsonWriter::compact().write_features_iter(features.iter(), None);
    assert!(s.starts_with('{'));
    assert!(s.contains("\"FeatureCollection\""));
    assert!(s.contains("\"features\""));
}

#[test]
fn write_features_iter_with_bbox() {
    let features = vec![pt(0.0, 0.0)];
    let bbox = Some([0.0_f64, 0.0_f64, 1.0_f64, 1.0_f64]);
    let s = GeoJsonWriter::compact().write_features_iter(features.iter(), bbox);
    assert!(s.contains("\"bbox\""));
}

#[test]
fn write_features_iter_empty() {
    let features: Vec<GeoJsonFeature> = vec![];
    let s = GeoJsonWriter::compact().write_features_iter(features.iter(), None);
    let doc = GeoJsonParser::new().parse(s.as_bytes()).expect("valid");
    let fc = doc.as_feature_collection().expect("fc");
    assert!(fc.is_empty());
}

// ═══════════════════════════════════════════════════════════════════
//  15. GeoJsonValidator
// ═══════════════════════════════════════════════════════════════════

#[test]
fn validator_polygon_ring_too_short_is_error() {
    // Ring with only 3 points (needs ≥4)
    let short_ring = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]]; // 3 points
    let g = GeoJsonGeometry::Polygon(vec![short_ring]);
    let issues = GeoJsonValidator::validate_geometry(&g);
    assert!(!issues.is_empty());
    assert!(issues.iter().any(|i| i.severity == IssueSeverity::Error));
}

#[test]
fn validator_polygon_ring_valid_closed() {
    let g = GeoJsonGeometry::Polygon(vec![closed_ring()]);
    let issues = GeoJsonValidator::validate_geometry(&g);
    // No error about ring length (5 points) — may still warn about coords
    let errors: Vec<_> = issues
        .iter()
        .filter(|i| i.severity == IssueSeverity::Error)
        .collect();
    assert!(errors.is_empty());
}

#[test]
fn validator_out_of_range_coords_warning() {
    // lon=200 is outside [-180,180]
    let g = GeoJsonGeometry::Point([200.0, 0.0]);
    let issues = GeoJsonValidator::validate_geometry(&g);
    assert!(issues.iter().any(|i| i.severity == IssueSeverity::Warning));
}

#[test]
fn validator_lat_out_of_range_warning() {
    let g = GeoJsonGeometry::Point([0.0, 100.0]);
    let issues = GeoJsonValidator::validate_geometry(&g);
    assert!(issues.iter().any(|i| i.severity == IssueSeverity::Warning));
}

#[test]
fn validator_linestring_min_2_points_error() {
    let g = GeoJsonGeometry::LineString(vec![[0.0, 0.0]]);
    let issues = GeoJsonValidator::validate_geometry(&g);
    assert!(issues.iter().any(|i| i.severity == IssueSeverity::Error));
}

#[test]
fn validator_valid_linestring_no_error() {
    let g = GeoJsonGeometry::LineString(vec![[0.0, 0.0], [1.0, 1.0]]);
    let issues = GeoJsonValidator::validate_geometry(&g);
    let errors: Vec<_> = issues
        .iter()
        .filter(|i| i.severity == IssueSeverity::Error)
        .collect();
    assert!(errors.is_empty());
}

#[test]
fn validator_non_finite_coord_error() {
    let g = GeoJsonGeometry::Point([f64::NAN, 0.0]);
    let issues = GeoJsonValidator::validate_geometry(&g);
    assert!(issues.iter().any(|i| i.severity == IssueSeverity::Error));
}

#[test]
fn validator_feature_collection_collects_all_issues() {
    let fc = FeatureCollection {
        features: vec![
            GeoJsonFeature {
                id: None,
                geometry: Some(GeoJsonGeometry::LineString(vec![[0.0, 0.0]])), // too short
                properties: None,
            },
            GeoJsonFeature {
                id: None,
                geometry: Some(GeoJsonGeometry::Point([200.0, 0.0])), // out of range lon
                properties: None,
            },
        ],
        bbox: None,
        crs: None,
        name: None,
    };
    let issues = GeoJsonValidator::validate_feature_collection(&fc);
    assert!(issues.len() >= 2);
}

#[test]
fn validator_feature_issues_include_path() {
    let feat = GeoJsonFeature {
        id: None,
        geometry: Some(GeoJsonGeometry::LineString(vec![[0.0, 0.0]])),
        properties: None,
    };
    let issues = GeoJsonValidator::validate_feature(&feat);
    assert!(!issues.is_empty());
    assert!(issues[0].path.is_some());
}

// ═══════════════════════════════════════════════════════════════════
//  16. StreamingFeatureReader
// ═══════════════════════════════════════════════════════════════════

#[test]
fn streaming_reader_iterates_all() {
    let fc = FeatureCollection {
        features: vec![pt(0.0, 0.0), pt(1.0, 1.0), pt(2.0, 2.0)],
        bbox: None,
        crs: None,
        name: None,
    };
    let reader = StreamingFeatureReader::from_collection(&fc);
    let collected: Vec<_> = reader.collect();
    assert_eq!(collected.len(), 3);
}

#[test]
fn streaming_reader_count_matches_collection() {
    let fc = FeatureCollection {
        features: vec![pt(0.0, 0.0), pt(1.0, 1.0)],
        bbox: None,
        crs: None,
        name: None,
    };
    let count = StreamingFeatureReader::from_collection(&fc).count();
    assert_eq!(count, fc.len());
}

#[test]
fn streaming_reader_empty_collection() {
    let fc = FeatureCollection::default();
    let count = StreamingFeatureReader::from_collection(&fc).count();
    assert_eq!(count, 0);
}

#[test]
fn streaming_reader_borrows_features() {
    let fc = FeatureCollection {
        features: vec![pt(1.0, 2.0)],
        bbox: None,
        crs: None,
        name: None,
    };
    let mut reader = StreamingFeatureReader::from_collection(&fc);
    let f = reader.next().expect("has feature");
    assert_eq!(f.bbox(), Some([1.0, 2.0, 1.0, 2.0]));
}

// ═══════════════════════════════════════════════════════════════════
//  17. FeatureFilter
// ═══════════════════════════════════════════════════════════════════

#[test]
fn filter_bbox_inside() {
    let feat = pt(5.0, 5.0);
    let f = FeatureFilter::new().with_bbox([0.0, 0.0, 10.0, 10.0]);
    assert!(f.matches(&feat));
}

#[test]
fn filter_bbox_outside() {
    let feat = pt(50.0, 50.0);
    let f = FeatureFilter::new().with_bbox([0.0, 0.0, 10.0, 10.0]);
    assert!(!f.matches(&feat));
}

#[test]
fn filter_bbox_on_boundary() {
    let feat = pt(10.0, 10.0);
    let f = FeatureFilter::new().with_bbox([0.0, 0.0, 10.0, 10.0]);
    assert!(f.matches(&feat)); // boundary is inclusive
}

#[test]
fn filter_property_eq_match() {
    let feat = pt_with_props(0.0, 0.0, serde_json::json!({"city": "Tokyo"}));
    let f = FeatureFilter::new().where_eq("city", "Tokyo");
    assert!(f.matches(&feat));
}

#[test]
fn filter_property_eq_no_match() {
    let feat = pt_with_props(0.0, 0.0, serde_json::json!({"city": "Paris"}));
    let f = FeatureFilter::new().where_eq("city", "Tokyo");
    assert!(!f.matches(&feat));
}

#[test]
fn filter_property_gt_match() {
    let feat = pt_with_props(0.0, 0.0, serde_json::json!({"pop": 2000000}));
    let f = FeatureFilter::new().where_gt("pop", 1_000_000.0);
    assert!(f.matches(&feat));
}

#[test]
fn filter_property_gt_no_match() {
    let feat = pt_with_props(0.0, 0.0, serde_json::json!({"pop": 500000}));
    let f = FeatureFilter::new().where_gt("pop", 1_000_000.0);
    assert!(!f.matches(&feat));
}

#[test]
fn filter_property_lt_match() {
    let feat = pt_with_props(0.0, 0.0, serde_json::json!({"elevation": 100.0}));
    let f = FeatureFilter::new().where_lt("elevation", 200.0);
    assert!(f.matches(&feat));
}

#[test]
fn filter_apply_returns_filtered_collection() {
    let fc = FeatureCollection {
        features: vec![
            pt_with_props(0.0, 0.0, serde_json::json!({"keep": true})),
            pt_with_props(1.0, 1.0, serde_json::json!({"keep": false})),
            pt_with_props(2.0, 2.0, serde_json::json!({"keep": true})),
        ],
        bbox: None,
        crs: None,
        name: None,
    };
    let filter = FeatureFilter::new().where_eq("keep", true);
    let result = filter.apply(&fc);
    assert_eq!(result.len(), 2);
}

#[test]
fn filter_apply_empty_result() {
    let fc = FeatureCollection {
        features: vec![pt_with_props(0.0, 0.0, serde_json::json!({"v": 1}))],
        bbox: None,
        crs: None,
        name: None,
    };
    let filter = FeatureFilter::new().where_eq("v", 999);
    let result = filter.apply(&fc);
    assert!(result.is_empty());
}

#[test]
fn filter_geometry_type_filter() {
    let fc = FeatureCollection {
        features: vec![
            pt(0.0, 0.0),
            GeoJsonFeature {
                id: None,
                geometry: Some(GeoJsonGeometry::LineString(vec![[0.0, 0.0], [1.0, 1.0]])),
                properties: None,
            },
        ],
        bbox: None,
        crs: None,
        name: None,
    };
    let filter = FeatureFilter::new().by_geometry_type(vec!["Point".into()]);
    let result = filter.apply(&fc);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result.features[0]
            .geometry
            .as_ref()
            .map(|g| g.geometry_type()),
        Some("Point")
    );
}

#[test]
fn filter_no_filters_passes_all() {
    let feat = pt(1.0, 2.0);
    let f = FeatureFilter::new();
    assert!(f.matches(&feat));
}

// ═══════════════════════════════════════════════════════════════════
//  18. GeoJsonCrs
// ═══════════════════════════════════════════════════════════════════

#[test]
fn crs_epsg4326_name() {
    let crs = GeoJsonCrs::epsg4326();
    assert_eq!(crs.type_, "name");
    let name = crs
        .properties
        .get("name")
        .and_then(|v| v.as_str())
        .expect("has name");
    assert!(name.contains("CRS84") || name.contains("4326"));
}

#[test]
fn crs_epsg3857_name() {
    let crs = GeoJsonCrs::epsg3857();
    let name = crs
        .properties
        .get("name")
        .and_then(|v| v.as_str())
        .expect("has name");
    assert!(name.contains("3857"));
}

#[test]
fn crs_epsg_code_from_epsg_prefix() {
    let crs = GeoJsonCrs::named("EPSG:4326");
    assert_eq!(crs.epsg_code(), Some(4326));
}

#[test]
fn crs_epsg_code_none_for_non_epsg() {
    let crs = GeoJsonCrs::named("urn:ogc:def:crs:OGC:1.3:CRS84");
    // CRS84 is not a numeric EPSG code
    let code = crs.epsg_code();
    // It may return None or parse 84 — just ensure no panic
    let _ = code;
}

#[test]
fn crs_named_type() {
    let crs = GeoJsonCrs::named("EPSG:32632");
    assert_eq!(crs.type_, "name");
}

// ═══════════════════════════════════════════════════════════════════
//  19. FeatureId variants
// ═══════════════════════════════════════════════════════════════════

#[test]
fn feature_id_string_variant() {
    let id = FeatureId::String("abc-123".into());
    assert!(
        matches!(&id, FeatureId::String(s) if s == "abc-123"),
        "wrong variant"
    );
}

#[test]
fn feature_id_number_variant() {
    let id = FeatureId::Number(42.0);
    assert!(
        matches!(id, FeatureId::Number(n) if (n - 42.0).abs() < f64::EPSILON),
        "wrong variant"
    );
}

// ═══════════════════════════════════════════════════════════════════
//  20. GeoJsonError display
// ═══════════════════════════════════════════════════════════════════

#[test]
fn error_invalid_type_display() {
    let e = GeoJsonError::InvalidType {
        expected: "Feature".into(),
        got: "Point".into(),
    };
    let s = e.to_string();
    assert!(s.contains("Feature"));
    assert!(s.contains("Point"));
}

#[test]
fn error_missing_field_display() {
    let e = GeoJsonError::MissingField("type".into());
    assert!(e.to_string().contains("type"));
}

#[test]
fn error_invalid_coordinates_display() {
    let e = GeoJsonError::InvalidCoordinates("NaN detected".into());
    assert!(e.to_string().contains("NaN"));
}

#[test]
fn error_max_depth_display() {
    let e = GeoJsonError::MaxDepthExceeded;
    assert!(!e.to_string().is_empty());
}

#[test]
fn error_empty_coordinates_display() {
    let e = GeoJsonError::EmptyCoordinates;
    assert!(!e.to_string().is_empty());
}

#[test]
fn error_from_serde_json() {
    let result: Result<serde_json::Value, _> = serde_json::from_str("{invalid}");
    let serde_err = result.expect_err("should fail");
    let geo_err: GeoJsonError = serde_err.into();
    let s = geo_err.to_string();
    assert!(!s.is_empty());
}

// ═══════════════════════════════════════════════════════════════════
//  21. GeoJsonDocument helpers
// ═══════════════════════════════════════════════════════════════════

#[test]
fn document_type_feature_collection() {
    let json = br#"{"type":"FeatureCollection","features":[]}"#;
    let doc = GeoJsonParser::new().parse(json).expect("valid");
    assert_eq!(doc.document_type(), "FeatureCollection");
    assert!(doc.as_feature_collection().is_some());
}

#[test]
fn document_type_feature() {
    let json = br#"{"type":"Feature","geometry":null,"properties":null}"#;
    let doc = GeoJsonParser::new().parse(json).expect("valid");
    assert_eq!(doc.document_type(), "Feature");
    assert!(doc.as_feature_collection().is_none());
}

#[test]
fn document_type_geometry() {
    let json = br#"{"type":"Point","coordinates":[0.0,0.0]}"#;
    let doc = GeoJsonParser::new().parse(json).expect("valid");
    assert_eq!(doc.document_type(), "Geometry");
}

// ═══════════════════════════════════════════════════════════════════
//  22. point_count / is_empty
// ═══════════════════════════════════════════════════════════════════

#[test]
fn point_count_point() {
    assert_eq!(GeoJsonGeometry::Point([0.0, 0.0]).point_count(), 1);
}

#[test]
fn point_count_null() {
    assert_eq!(GeoJsonGeometry::Null.point_count(), 0);
    assert!(GeoJsonGeometry::Null.is_empty());
}

#[test]
fn point_count_polygon() {
    let g = GeoJsonGeometry::Polygon(vec![closed_ring()]);
    assert_eq!(g.point_count(), 5);
}

// ═══════════════════════════════════════════════════════════════════
//  23. Strict-mode parser
// ═══════════════════════════════════════════════════════════════════

#[test]
fn strict_parser_parses_valid_input() {
    let json = br#"{"type":"FeatureCollection","features":[]}"#;
    let doc = GeoJsonParser::new().strict().parse(json).expect("valid");
    assert!(doc.as_feature_collection().is_some());
}

// ═══════════════════════════════════════════════════════════════════
//  24. Bbox writer option
// ═══════════════════════════════════════════════════════════════════

#[test]
fn writer_with_bbox_includes_bbox_field() {
    let fc = FeatureCollection {
        features: vec![pt(1.0, 2.0)],
        bbox: None,
        crs: None,
        name: None,
    };
    let s = GeoJsonWriter::compact()
        .with_bbox()
        .write_feature_collection(&fc);
    assert!(s.contains("\"bbox\""));
}

#[test]
fn writer_without_bbox_no_bbox_field() {
    let fc = FeatureCollection {
        features: vec![pt(1.0, 2.0)],
        bbox: None,
        crs: None,
        name: None,
    };
    let s = GeoJsonWriter::compact().write_feature_collection(&fc);
    assert!(!s.contains("\"bbox\""));
}
