//! Integration tests for property schema inference.

#![allow(clippy::unwrap_used, missing_docs)]

use oxigdal_geojson_stream::{
    FeatureCollection, GeoJsonFeature, InferredType, infer_schema, infer_schema_from_collection,
    infer_schema_slice,
};
use serde_json::json;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build a `GeoJsonFeature` with the given JSON object as properties.
fn feature(props: serde_json::Value) -> GeoJsonFeature {
    GeoJsonFeature {
        id: None,
        geometry: None,
        properties: Some(props),
    }
}

/// Build a `GeoJsonFeature` with no properties at all.
fn feature_no_props() -> GeoJsonFeature {
    GeoJsonFeature {
        id: None,
        geometry: None,
        properties: None,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// Empty feature collection → empty schema, feature_count = 0.
#[test]
fn test_infer_empty_collection() {
    let schema = infer_schema(std::iter::empty::<&GeoJsonFeature>());
    assert_eq!(schema.feature_count, 0);
    assert!(schema.fields.is_empty());
    assert_eq!(schema.null_properties_count, 0);
}

/// All features have field "name" as a string → InferredType::String, nullable = false.
#[test]
fn test_infer_single_string_field() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "name": "Alice" })),
        feature(json!({ "name": "Bob" })),
        feature(json!({ "name": "Charlie" })),
    ];
    let schema = infer_schema(feats.iter());
    assert_eq!(schema.feature_count, 3);

    let name_field = schema.fields.get("name").expect("field 'name' must exist");
    assert_eq!(name_field.inferred_type, InferredType::String);
    assert!(!name_field.nullable, "name should not be nullable");
    assert_eq!(name_field.non_null_count, 3);
    assert!((name_field.fill_rate() - 1.0).abs() < f64::EPSILON);
}

/// Some features missing "name" → nullable = true.
#[test]
fn test_infer_string_with_nullable() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "name": "Alice" })),
        feature(json!({})),               // name absent
        feature(json!({ "name": null })), // name explicit null
    ];
    let schema = infer_schema(feats.iter());

    let name_field = schema.fields.get("name").expect("field 'name' must exist");
    assert_eq!(name_field.inferred_type, InferredType::String);
    assert!(name_field.nullable, "name should be nullable");
    assert_eq!(name_field.non_null_count, 1);
}

/// All values are integers → InferredType::Integer.
#[test]
fn test_infer_integer_field() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "count": 1 })),
        feature(json!({ "count": 42 })),
        feature(json!({ "count": -7 })),
    ];
    let schema = infer_schema(feats.iter());

    let field = schema
        .fields
        .get("count")
        .expect("field 'count' must exist");
    assert_eq!(field.inferred_type, InferredType::Integer);
    assert!(!field.nullable);
    assert_eq!(field.non_null_count, 3);
}

/// Mix of integer and float values → InferredType::Number (float widens integer).
#[test]
fn test_infer_number_field_mixed_int_float() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "val": 1 })),    // i64 → Integer
        feature(json!({ "val": 1.75 })), // f64 → Number (not a well-known constant)
        feature(json!({ "val": 100 })),  // i64 → Integer
    ];
    let schema = infer_schema(feats.iter());

    let field = schema.fields.get("val").expect("field 'val' must exist");
    assert_eq!(
        field.inferred_type,
        InferredType::Number,
        "integer + float should coerce to Number"
    );
    assert_eq!(field.non_null_count, 3);
}

/// All boolean values → InferredType::Boolean.
#[test]
fn test_infer_boolean_field() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "active": true })),
        feature(json!({ "active": false })),
        feature(json!({ "active": true })),
    ];
    let schema = infer_schema(feats.iter());

    let field = schema
        .fields
        .get("active")
        .expect("field 'active' must exist");
    assert_eq!(field.inferred_type, InferredType::Boolean);
    assert!(!field.nullable);
}

/// All null values → InferredType::Null (field is nullable).
#[test]
fn test_infer_null_only_field() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "code": null })),
        feature(json!({ "code": null })),
    ];
    let schema = infer_schema(feats.iter());

    let field = schema.fields.get("code").expect("field 'code' must exist");
    assert_eq!(field.inferred_type, InferredType::Null);
    assert!(field.nullable, "null-only field should be nullable");
    assert_eq!(field.non_null_count, 0);
}

/// String and number values in the same field → InferredType::Mixed.
#[test]
fn test_infer_mixed_type_field() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "tag": "hello" })),
        feature(json!({ "tag": 42 })),
        feature(json!({ "tag": "world" })),
    ];
    let schema = infer_schema(feats.iter());

    let field = schema.fields.get("tag").expect("field 'tag' must exist");
    assert_eq!(field.inferred_type, InferredType::Mixed);
}

/// 3 of 5 features have "score" → fill_rate ≈ 0.6.
#[test]
fn test_infer_fill_rate_partial() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "score": 10 })),
        feature(json!({ "score": 20 })),
        feature(json!({})), // absent
        feature(json!({ "score": 30 })),
        feature(json!({})), // absent
    ];
    let schema = infer_schema(feats.iter());
    assert_eq!(schema.feature_count, 5);

    let field = schema
        .fields
        .get("score")
        .expect("field 'score' must exist");
    assert_eq!(field.non_null_count, 3);
    let expected = 3.0_f64 / 5.0;
    assert!(
        (field.fill_rate() - expected).abs() < 1e-10,
        "fill_rate should be ~0.6 but got {}",
        field.fill_rate()
    );
}

/// All fields present in all features → is_fully_populated() = true.
#[test]
fn test_infer_is_fully_populated() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "a": 1, "b": "x" })),
        feature(json!({ "a": 2, "b": "y" })),
        feature(json!({ "a": 3, "b": "z" })),
    ];
    let schema = infer_schema(feats.iter());
    assert!(
        schema.is_fully_populated(),
        "schema should be fully populated"
    );
}

/// Field with fill_rate < 0.5 appears in sparse_fields(0.5).
#[test]
fn test_infer_sparse_fields() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "common": 1, "rare": "r" })),
        feature(json!({ "common": 2 })),
        feature(json!({ "common": 3 })),
        feature(json!({ "common": 4 })),
        feature(json!({ "common": 5 })),
    ];
    let schema = infer_schema(feats.iter());

    let sparse = schema.sparse_fields(0.5);
    assert!(
        sparse.contains(&"rare"),
        "rare (fill_rate=0.2) should appear in sparse_fields(0.5)"
    );
    assert!(
        !sparse.contains(&"common"),
        "common (fill_rate=1.0) should NOT appear in sparse_fields(0.5)"
    );
}

/// String length stats are populated correctly.
#[test]
fn test_infer_string_len_stats() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "label": "ab" })),          // len=2
        feature(json!({ "label": "hello world" })), // len=11
        feature(json!({ "label": "xyz" })),         // len=3
    ];
    let schema = infer_schema(feats.iter());

    let field = schema
        .fields
        .get("label")
        .expect("field 'label' must exist");
    assert_eq!(field.inferred_type, InferredType::String);
    assert_eq!(field.min_string_len, Some(2), "min string len should be 2");
    assert_eq!(
        field.max_string_len,
        Some(11),
        "max string len should be 11"
    );
}

/// Numeric min/max stats are populated correctly.
#[test]
fn test_infer_numeric_stats() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "temperature": -5.5 })),
        feature(json!({ "temperature": 23.1 })),
        feature(json!({ "temperature": 0.0 })),
        feature(json!({ "temperature": 100.0 })),
    ];
    let schema = infer_schema(feats.iter());

    let field = schema
        .fields
        .get("temperature")
        .expect("field 'temperature' must exist");
    assert_eq!(field.inferred_type, InferredType::Number);

    let min_val = field.min_numeric.expect("min_numeric should be Some");
    let max_val = field.max_numeric.expect("max_numeric should be Some");
    assert!((min_val - (-5.5)).abs() < 1e-10, "min should be -5.5");
    assert!((max_val - 100.0).abs() < 1e-10, "max should be 100.0");
}

/// infer_schema_from_collection works on a FeatureCollection.
#[test]
fn test_infer_from_collection() {
    let fc = FeatureCollection {
        features: vec![
            feature(json!({ "id": 1, "city": "Tokyo" })),
            feature(json!({ "id": 2, "city": "Osaka" })),
            feature(json!({ "id": 3, "city": "Kyoto" })),
        ],
        bbox: None,
        bbox_3d: None,
        crs: None,
        name: None,
    };

    let schema = infer_schema_from_collection(&fc);
    assert_eq!(schema.feature_count, 3);
    assert_eq!(schema.null_properties_count, 0);

    let id_field = schema.fields.get("id").expect("field 'id' must exist");
    assert_eq!(id_field.inferred_type, InferredType::Integer);

    let city_field = schema.fields.get("city").expect("field 'city' must exist");
    assert_eq!(city_field.inferred_type, InferredType::String);

    assert!(schema.is_fully_populated());
}

/// Features with no properties increment null_properties_count and absent
/// fields are marked nullable when discovered later.
#[test]
fn test_infer_null_properties_count() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "x": 1 })),
        feature_no_props(),
        feature_no_props(),
    ];
    let schema = infer_schema(feats.iter());
    assert_eq!(schema.feature_count, 3);
    assert_eq!(schema.null_properties_count, 2);

    // Field "x" still has total_count = 3 but fill_rate = 1/3.
    let field = schema.fields.get("x").expect("field 'x' must exist");
    assert_eq!(field.non_null_count, 1);
    assert!((field.fill_rate() - (1.0 / 3.0)).abs() < 1e-10);
}

/// infer_schema_slice accepts a slice of references directly.
#[test]
fn test_infer_schema_slice_direct() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "flag": true })),
        feature(json!({ "flag": false })),
    ];
    let refs: Vec<&GeoJsonFeature> = feats.iter().collect();
    let schema = infer_schema_slice(&refs);

    let field = schema.fields.get("flag").expect("field 'flag' must exist");
    assert_eq!(field.inferred_type, InferredType::Boolean);
    assert_eq!(field.total_count, 2);
    assert_eq!(field.non_null_count, 2);
}

/// Array and Object type values are detected correctly.
#[test]
fn test_infer_array_and_object_types() {
    let feats: Vec<GeoJsonFeature> = vec![
        feature(json!({ "tags": ["a", "b"], "meta": { "key": 1 } })),
        feature(json!({ "tags": ["c"],      "meta": { "key": 2 } })),
    ];
    let schema = infer_schema(feats.iter());

    let tags = schema.fields.get("tags").expect("field 'tags' must exist");
    assert_eq!(tags.inferred_type, InferredType::Array);

    let meta = schema.fields.get("meta").expect("field 'meta' must exist");
    assert_eq!(meta.inferred_type, InferredType::Object);
}

/// field_names() returns sorted keys.
#[test]
fn test_field_names_sorted() {
    let feats: Vec<GeoJsonFeature> = vec![feature(json!({ "zebra": 1, "apple": 2, "mango": 3 }))];
    let schema = infer_schema(feats.iter());
    let names = schema.field_names();
    assert_eq!(names, vec!["apple", "mango", "zebra"]);
}
