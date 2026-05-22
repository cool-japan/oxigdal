//! Integration tests for the GeoPackage ↔ GeoJSON bidirectional conversion.

use std::collections::HashMap;

use oxigdal_geojson_stream::{FeatureCollection, FeatureId, GeoJsonFeature, GeoJsonGeometry};
use oxigdal_gpkg::{
    FeatureRow, FeatureTable, FieldDefinition, FieldType, FieldValue, GpkgGeometry,
    feature_table_from_geojson, feature_table_to_geojson, geojson_geom_to_gpkg,
    gpkg_geom_to_geojson,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helper builders
// ─────────────────────────────────────────────────────────────────────────────

fn empty_feature_collection() -> FeatureCollection {
    FeatureCollection {
        features: Vec::new(),
        bbox: None,
        bbox_3d: None,
        crs: None,
        name: None,
    }
}

fn make_feature_collection(features: Vec<GeoJsonFeature>) -> FeatureCollection {
    FeatureCollection {
        features,
        bbox: None,
        bbox_3d: None,
        crs: None,
        name: None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — round-trip Point FeatureTable
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_round_trip_point_feature_table() {
    // Build a small FeatureTable with two Point features
    let mut table = FeatureTable::new("points", "geom");
    table.schema = vec![FieldDefinition {
        name: "label".into(),
        field_type: FieldType::Text,
        not_null: false,
        primary_key: false,
        default_value: None,
    }];

    let mut fields1 = HashMap::new();
    fields1.insert("label".to_string(), FieldValue::Text("alpha".into()));
    table.features.push(FeatureRow {
        fid: 1,
        geometry: Some(GpkgGeometry::Point { x: 10.0, y: 20.0 }),
        fields: fields1,
    });

    let mut fields2 = HashMap::new();
    fields2.insert("label".to_string(), FieldValue::Text("beta".into()));
    table.features.push(FeatureRow {
        fid: 2,
        geometry: Some(GpkgGeometry::Point { x: -5.5, y: 45.0 }),
        fields: fields2,
    });

    // to_geojson
    let fc = feature_table_to_geojson(&table).expect("to_geojson must succeed");
    assert_eq!(fc.features.len(), 2);
    assert_eq!(
        fc.features[0].geometry,
        Some(GeoJsonGeometry::Point([10.0, 20.0]))
    );
    assert_eq!(
        fc.features[1].geometry,
        Some(GeoJsonGeometry::Point([-5.5, 45.0]))
    );

    // from_geojson → round-trip
    let table2 =
        feature_table_from_geojson(&fc, "points", "geom").expect("from_geojson must succeed");
    assert_eq!(table2.features.len(), 2);
    assert_eq!(
        table2.features[0].geometry,
        Some(GpkgGeometry::Point { x: 10.0, y: 20.0 })
    );
    assert_eq!(table2.features[0].fid, 1);
    assert_eq!(
        table2.features[1].geometry,
        Some(GpkgGeometry::Point { x: -5.5, y: 45.0 })
    );
    assert_eq!(table2.features[1].fid, 2);

    // Labels survive
    assert_eq!(
        table2.features[0].fields.get("label").cloned(),
        Some(FieldValue::Text("alpha".into()))
    );
    assert_eq!(
        table2.features[1].fields.get("label").cloned(),
        Some(FieldValue::Text("beta".into()))
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — round-trip LineString FeatureTable
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_round_trip_linestring_feature_table() {
    let mut table = FeatureTable::new("roads", "geom");
    let mut fields = HashMap::new();
    fields.insert("road_id".to_string(), FieldValue::Integer(42));
    table.features.push(FeatureRow {
        fid: 1,
        geometry: Some(GpkgGeometry::LineString {
            coords: vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)],
        }),
        fields,
    });

    let fc = feature_table_to_geojson(&table).expect("to_geojson");
    assert_eq!(
        fc.features[0].geometry,
        Some(GeoJsonGeometry::LineString(vec![
            [0.0, 0.0],
            [1.0, 1.0],
            [2.0, 0.0]
        ]))
    );

    let table2 = feature_table_from_geojson(&fc, "roads", "geom").expect("from_geojson");
    assert_eq!(
        table2.features[0].geometry,
        Some(GpkgGeometry::LineString {
            coords: vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)]
        })
    );
    assert_eq!(
        table2.features[0].fields.get("road_id").cloned(),
        Some(FieldValue::Integer(42))
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — round-trip Polygon with holes
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_round_trip_polygon_with_holes() {
    let exterior = vec![
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
        (0.0, 0.0),
    ];
    let hole = vec![(2.0, 2.0), (4.0, 2.0), (4.0, 4.0), (2.0, 4.0), (2.0, 2.0)];

    let mut table = FeatureTable::new("parcels", "geom");
    table.features.push(FeatureRow {
        fid: 7,
        geometry: Some(GpkgGeometry::Polygon {
            rings: vec![exterior.clone(), hole.clone()],
        }),
        fields: HashMap::new(),
    });

    let fc = feature_table_to_geojson(&table).expect("to_geojson");
    let gj_geom = fc.features[0].geometry.clone().expect("geometry present");

    let gj_rings = match &gj_geom {
        GeoJsonGeometry::Polygon(rings) => rings,
        other => unreachable!("expected Polygon, got {other:?}"),
    };
    assert_eq!(gj_rings.len(), 2, "should have exterior + 1 hole");
    assert_eq!(gj_rings[0].len(), 5);
    assert_eq!(gj_rings[1].len(), 5);

    let table2 = feature_table_from_geojson(&fc, "parcels", "geom").expect("from_geojson");
    let gpkg_rings = match &table2.features[0].geometry {
        Some(GpkgGeometry::Polygon { rings }) => rings,
        other => unreachable!("expected GpkgGeometry::Polygon, got {other:?}"),
    };
    assert_eq!(gpkg_rings.len(), 2);
    assert_eq!(gpkg_rings[0], exterior);
    assert_eq!(gpkg_rings[1], hole);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — all attribute types survive round-trip
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_round_trip_attributes_int_real_text_null() {
    let mut fields = HashMap::new();
    fields.insert("count".to_string(), FieldValue::Integer(99));
    fields.insert("score".to_string(), FieldValue::Real(1.234_567_89));
    fields.insert("name".to_string(), FieldValue::Text("hello".into()));
    fields.insert("missing".to_string(), FieldValue::Null);

    let mut table = FeatureTable::new("data", "geom");
    table.schema = vec![
        FieldDefinition {
            name: "count".into(),
            field_type: FieldType::Integer,
            not_null: false,
            primary_key: false,
            default_value: None,
        },
        FieldDefinition {
            name: "score".into(),
            field_type: FieldType::Real,
            not_null: false,
            primary_key: false,
            default_value: None,
        },
        FieldDefinition {
            name: "name".into(),
            field_type: FieldType::Text,
            not_null: false,
            primary_key: false,
            default_value: None,
        },
        FieldDefinition {
            name: "missing".into(),
            field_type: FieldType::Text,
            not_null: false,
            primary_key: false,
            default_value: None,
        },
    ];
    table.features.push(FeatureRow {
        fid: 1,
        geometry: Some(GpkgGeometry::Point { x: 0.0, y: 0.0 }),
        fields,
    });

    let fc = feature_table_to_geojson(&table).expect("to_geojson");
    let table2 = feature_table_from_geojson(&fc, "data", "geom").expect("from_geojson");
    let row = &table2.features[0];

    assert_eq!(row.fields.get("count"), Some(&FieldValue::Integer(99)));
    assert_eq!(
        row.fields.get("score"),
        Some(&FieldValue::Real(1.234_567_89))
    );
    assert_eq!(
        row.fields.get("name"),
        Some(&FieldValue::Text("hello".into()))
    );
    assert_eq!(row.fields.get("missing"), Some(&FieldValue::Null));
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — sequential FIDs assigned when id is absent
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_from_geojson_assigns_sequential_fids_when_missing() {
    let features = vec![
        GeoJsonFeature {
            id: None,
            geometry: Some(GeoJsonGeometry::Point([1.0, 1.0])),
            properties: None,
        },
        GeoJsonFeature {
            id: None,
            geometry: Some(GeoJsonGeometry::Point([2.0, 2.0])),
            properties: None,
        },
        GeoJsonFeature {
            id: None,
            geometry: Some(GeoJsonGeometry::Point([3.0, 3.0])),
            properties: None,
        },
    ];
    let fc = make_feature_collection(features);
    let table = feature_table_from_geojson(&fc, "seq", "geom").expect("from_geojson");

    assert_eq!(table.features[0].fid, 1);
    assert_eq!(table.features[1].fid, 2);
    assert_eq!(table.features[2].fid, 3);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — numeric feature id is used as FID
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_from_geojson_uses_numeric_feature_id_when_present() {
    let features = vec![GeoJsonFeature {
        id: Some(FeatureId::Number(42.0)),
        geometry: Some(GeoJsonGeometry::Point([0.0, 0.0])),
        properties: None,
    }];
    let fc = make_feature_collection(features);
    let table = feature_table_from_geojson(&fc, "fid_test", "geom").expect("from_geojson");

    assert_eq!(table.features[0].fid, 42);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — null geometry maps to GeoJSON null
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_to_geojson_null_geometry_maps_to_geojson_null() {
    let mut table = FeatureTable::new("nullgeo", "geom");
    table.features.push(FeatureRow {
        fid: 1,
        geometry: None, // null geometry
        fields: HashMap::new(),
    });

    let fc = feature_table_to_geojson(&table).expect("to_geojson");
    assert_eq!(
        fc.features[0].geometry,
        Some(GeoJsonGeometry::Null),
        "None geometry must become Some(Null) in GeoJSON"
    );

    // Round-trip back: Null → Empty → None
    let table2 = feature_table_from_geojson(&fc, "nullgeo", "geom").expect("from_geojson");
    assert!(
        table2.features[0].geometry.is_none(),
        "Null GeoJSON geometry must come back as None after round-trip"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8 — SRS metadata is NOT present in GeoJSON output
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_to_geojson_drops_srs_metadata() {
    let mut table = FeatureTable::new("spatial", "geom");
    table.srs_id = Some(4326); // EPSG:4326
    table.features.push(FeatureRow {
        fid: 1,
        geometry: Some(GpkgGeometry::Point { x: 10.0, y: 50.0 }),
        fields: HashMap::new(),
    });

    let fc = feature_table_to_geojson(&table).expect("to_geojson");

    // RFC 7946 compliant: no CRS object
    assert!(
        fc.crs.is_none(),
        "SRS metadata must not leak into GeoJSON output (RFC 7946)"
    );

    // Also verify the geometry itself is correct despite the SRS
    assert_eq!(
        fc.features[0].geometry,
        Some(GeoJsonGeometry::Point([10.0, 50.0]))
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Additional geometry conversion tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_gpkg_geom_to_geojson_point_z() {
    let g = GpkgGeometry::PointZ {
        x: 1.0,
        y: 2.0,
        z: 100.0,
    };
    let gj = gpkg_geom_to_geojson(&g).expect("PointZ conversion");
    assert_eq!(gj, GeoJsonGeometry::PointZ([1.0, 2.0, 100.0]));
}

#[test]
fn test_geojson_geom_to_gpkg_linestring() {
    let gj = GeoJsonGeometry::LineString(vec![[0.0, 0.0], [1.0, 1.0]]);
    let g = geojson_geom_to_gpkg(&gj).expect("LineString conversion");
    assert_eq!(
        g,
        GpkgGeometry::LineString {
            coords: vec![(0.0, 0.0), (1.0, 1.0)]
        }
    );
}

#[test]
fn test_multipolygon_z_round_trip() {
    let ring: Vec<(f64, f64, f64)> = vec![
        (0.0, 0.0, 1.0),
        (1.0, 0.0, 2.0),
        (1.0, 1.0, 3.0),
        (0.0, 0.0, 1.0),
    ];
    let g = GpkgGeometry::MultiPolygonZ {
        polygons: vec![vec![ring.clone()]],
    };
    let gj = gpkg_geom_to_geojson(&g).expect("MultiPolygonZ");
    let back = geojson_geom_to_gpkg(&gj).expect("back");
    assert_eq!(
        back,
        GpkgGeometry::MultiPolygonZ {
            polygons: vec![vec![ring]]
        }
    );
}

#[test]
fn test_empty_feature_table_produces_empty_feature_collection() {
    let table = FeatureTable::new("empty", "geom");
    let fc = feature_table_to_geojson(&table).expect("empty table");
    assert!(fc.features.is_empty());
}

#[test]
fn test_from_empty_feature_collection() {
    let fc = empty_feature_collection();
    let table = feature_table_from_geojson(&fc, "t", "g").expect("empty fc");
    assert!(table.features.is_empty());
    assert!(table.schema.is_empty());
}
