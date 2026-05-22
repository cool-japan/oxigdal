//! Tests for regex-based property filtering in oxigdal-geojson-stream.
#![allow(clippy::unwrap_used, clippy::panic, missing_docs)]

use oxigdal_geojson_stream::filter::{
    CompiledRegexFilter, FeatureFilter, FilterExpr, FilterOp, PropertyFilter,
};
use oxigdal_geojson_stream::types::{GeoJsonFeature, GeoJsonGeometry};
use serde_json::json;

// ─── helpers ─────────────────────────────────────────────────────────────────

fn feature_with_props(props: serde_json::Value) -> GeoJsonFeature {
    GeoJsonFeature {
        id: None,
        geometry: Some(GeoJsonGeometry::Point([10.0, 50.0])),
        properties: Some(props),
    }
}

fn make_map(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    match v {
        serde_json::Value::Object(m) => m,
        _ => panic!("expected object"),
    }
}

// ─── 1: FilterOp::MatchesRegex basic ─────────────────────────────────────────

#[test]
fn test_filterop_matches_regex_basic() {
    let pf = PropertyFilter {
        key: "name".to_string(),
        operator: FilterOp::MatchesRegex,
        value: json!("foo.*"),
    };

    // "foobar" matches "foo.*"
    let hit = json!({"name": "foobar"});
    assert!(pf.matches(&hit), "foobar should match foo.*");

    // "bar" does not match
    let miss = json!({"name": "bar"});
    assert!(!pf.matches(&miss), "bar should not match foo.*");
}

// ─── 2: FilterOp::NotMatchesRegex ────────────────────────────────────────────

#[test]
fn test_filterop_not_matches_regex() {
    let pf = PropertyFilter {
        key: "tag".to_string(),
        operator: FilterOp::NotMatchesRegex,
        value: json!("^test"),
    };

    // "test_value" starts with "test" → NotMatchesRegex → false
    let should_fail = json!({"tag": "test_value"});
    assert!(
        !pf.matches(&should_fail),
        "test_value should NOT pass NotMatchesRegex(^test)"
    );

    // "other" does not start with "test" → NotMatchesRegex → true
    let should_pass = json!({"tag": "other"});
    assert!(
        pf.matches(&should_pass),
        "other should pass NotMatchesRegex(^test)"
    );
}

// ─── 3: invalid regex returns false (no panic) ───────────────────────────────

#[test]
fn test_filterop_invalid_regex_returns_false() {
    let pf = PropertyFilter {
        key: "name".to_string(),
        operator: FilterOp::MatchesRegex,
        value: json!("[invalid"),
    };

    let props = json!({"name": "anything"});
    // Must not panic and must return false
    assert!(!pf.matches(&props), "invalid regex should return false");
}

// ─── 4: CompiledRegexFilter basic match ──────────────────────────────────────

#[test]
fn test_compiled_regex_filter_matches() {
    let f = CompiledRegexFilter::new("city", "^New", false).unwrap();

    let hit = make_map(json!({"city": "New York"}));
    assert!(f.evaluate(&hit), "New York should match ^New");

    let miss = make_map(json!({"city": "London"}));
    assert!(!f.evaluate(&miss), "London should not match ^New");
}

// ─── 5: case-insensitive regex via (?i) flag ─────────────────────────────────

#[test]
fn test_compiled_regex_filter_case_insensitive() {
    let f = CompiledRegexFilter::new("city", "(?i)london", false).unwrap();

    let hit = make_map(json!({"city": "LONDON"}));
    assert!(f.evaluate(&hit), "LONDON should match (?i)london");

    let also_hit = make_map(json!({"city": "London"}));
    assert!(f.evaluate(&also_hit), "London should match (?i)london");

    let miss = make_map(json!({"city": "Paris"}));
    assert!(!f.evaluate(&miss), "Paris should not match (?i)london");
}

// ─── 6: absent field semantics ───────────────────────────────────────────────

#[test]
fn test_compiled_regex_filter_absent_field() {
    let f_match = CompiledRegexFilter::new("missing_key", ".*", false).unwrap();
    let f_not = CompiledRegexFilter::new("missing_key", ".*", true).unwrap();

    let props = make_map(json!({"other": "value"}));

    // negate=false → absent field → false
    assert!(
        !f_match.evaluate(&props),
        "absent field with negate=false → false"
    );

    // negate=true → absent field → true
    assert!(
        f_not.evaluate(&props),
        "absent field with negate=true → true"
    );
}

// ─── 7: FeatureFilter::with_regex_filter builder ─────────────────────────────

#[test]
fn test_feature_filter_with_regex_builder() {
    let filter = FeatureFilter::new()
        .with_regex_filter("tag", "geo.*")
        .unwrap();

    let hit = feature_with_props(json!({"tag": "geospatial"}));
    assert!(filter.matches(&hit), "geospatial should match geo.*");

    let miss = feature_with_props(json!({"tag": "temporal"}));
    assert!(!filter.matches(&miss), "temporal should not match geo.*");
}

// ─── 8: combined bbox + not-regex filter ─────────────────────────────────────

#[test]
fn test_feature_filter_not_regex_builder() {
    // bbox covers [0..20, 40..60]; not-regex excludes "deprecated" tag prefix
    let filter = FeatureFilter::new()
        .with_bbox([0.0, 40.0, 20.0, 60.0])
        .with_not_regex_filter("status", "^deprecated")
        .unwrap();

    // inside bbox, status is "active" → passes both filters
    let good = feature_with_props(json!({"status": "active"}));
    assert!(filter.matches(&good), "active inside bbox should pass");

    // inside bbox, status is "deprecated_v1" → fails not-regex
    let stale = feature_with_props(json!({"status": "deprecated_v1"}));
    assert!(
        !filter.matches(&stale),
        "deprecated_v1 should fail not-regex filter"
    );

    // outside bbox (lon=50), status is "active" → fails bbox
    let outside = GeoJsonFeature {
        id: None,
        geometry: Some(GeoJsonGeometry::Point([50.0, 50.0])),
        properties: Some(json!({"status": "active"})),
    };
    assert!(
        !filter.matches(&outside),
        "feature outside bbox should fail"
    );
}

// ─── 9: integer property with regex → false (no panic) ───────────────────────

#[test]
fn test_regex_on_non_string_property() {
    let pf = PropertyFilter {
        key: "count".to_string(),
        operator: FilterOp::MatchesRegex,
        value: json!("\\d+"),
    };

    // "count" is an integer → not a string → should return false, never panic
    let props = json!({"count": 42});
    assert!(
        !pf.matches(&props),
        "integer property should return false for MatchesRegex"
    );

    // Same test via CompiledRegexFilter
    let f = CompiledRegexFilter::new("count", r"\d+", false).unwrap();
    let map = make_map(json!({"count": 42}));
    assert!(
        !f.evaluate(&map),
        "integer property via CompiledRegexFilter should return false"
    );
}

// ─── bonus: FilterExpr::CompiledRegex in expression trees ───────────────────

#[test]
fn test_filter_expr_compiled_regex_variant() {
    let rf = CompiledRegexFilter::new("name", "^[A-Z]", false).unwrap();
    let expr = FilterExpr::CompiledRegex(rf);

    // "Alpha" starts with uppercase → matches
    let hit = feature_with_props(json!({"name": "Alpha"}));
    assert!(expr.matches_feature(&hit), "Alpha should match ^[A-Z]");

    // "beta" starts with lowercase → no match
    let miss = feature_with_props(json!({"name": "beta"}));
    assert!(!expr.matches_feature(&miss), "beta should not match ^[A-Z]");
}

// ─── bonus: FilterExpr::CompiledRegex with no-properties feature ─────────────

#[test]
fn test_filter_expr_compiled_regex_no_properties() {
    let rf_match = CompiledRegexFilter::new("name", ".*", false).unwrap();
    let rf_not = CompiledRegexFilter::new("name", ".*", true).unwrap();

    let bare = GeoJsonFeature {
        id: None,
        geometry: Some(GeoJsonGeometry::Point([0.0, 0.0])),
        properties: None,
    };

    // negate=false, no properties → false
    assert!(
        !FilterExpr::CompiledRegex(rf_match).matches_feature(&bare),
        "no-properties + negate=false should be false"
    );

    // negate=true, no properties → true
    assert!(
        FilterExpr::CompiledRegex(rf_not).matches_feature(&bare),
        "no-properties + negate=true should be true"
    );
}
