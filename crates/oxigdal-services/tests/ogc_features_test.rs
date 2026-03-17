//! Integration tests for the OGC API - Features Part 1 & Part 2 module

use oxigdal_services::ogc_features::{
    Collection, ConformanceClasses, CqlExpr, CqlParser, CqlValue, CrsTransform, DateTimeFilter,
    Extent, Feature, FeatureCollection, FeatureId, FeaturesError, FeaturesServer, FilterLang, Link,
    MAX_LIMIT, QueryParams, SpatialExtent, TemporalExtent,
};
use serde_json::json;

// ─── constants (copied from production code for use in tests) ────────────────
const CRS84_URI: &str = "http://www.opengis.net/def/crs/OGC/1.3/CRS84";
const EPSG4326_URI: &str = "http://www.opengis.net/def/crs/EPSG/0/4326";
const EPSG3857_URI: &str = "http://www.opengis.net/def/crs/EPSG/0/3857";
const EPSG4258_URI: &str = "http://www.opengis.net/def/crs/EPSG/0/4258";
const EPSG25832_URI: &str = "http://www.opengis.net/def/crs/EPSG/0/25832";
const EPSG25833_URI: &str = "http://www.opengis.net/def/crs/EPSG/0/25833";

// ─── helpers ─────────────────────────────────────────────────────────────────

fn make_server() -> FeaturesServer {
    let mut s = FeaturesServer::new("Integration Test API", "https://test.example.com/ogc");
    s.add_collection(Collection::new("buildings"));
    s.add_collection(Collection::new("roads"));
    s
}

fn make_features(n: usize) -> Vec<Feature> {
    (0..n)
        .map(|i| {
            let mut f = Feature::new();
            f.id = Some(FeatureId::Integer(i as i64));
            f.properties = Some(json!({"index": i, "name": format!("Feature {i}")}));
            f
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// ConformanceClasses
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_conformance_core_has_four_uris() {
    let cc = ConformanceClasses::ogc_features_core();
    assert_eq!(cc.conforms_to.len(), 4);
}

#[test]
fn ext_conformance_core_contains_core_uri() {
    let cc = ConformanceClasses::ogc_features_core();
    assert!(
        cc.conforms_to
            .contains(&"http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/core".to_string())
    );
}

#[test]
fn ext_conformance_core_contains_oas30() {
    let cc = ConformanceClasses::ogc_features_core();
    assert!(
        cc.conforms_to
            .contains(&"http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/oas30".to_string())
    );
}

#[test]
fn ext_conformance_core_contains_html() {
    let cc = ConformanceClasses::ogc_features_core();
    assert!(
        cc.conforms_to
            .contains(&"http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/html".to_string())
    );
}

#[test]
fn ext_conformance_core_contains_geojson() {
    let cc = ConformanceClasses::ogc_features_core();
    assert!(
        cc.conforms_to.contains(
            &"http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/geojson".to_string()
        )
    );
}

#[test]
fn ext_conformance_with_crs_has_five_uris() {
    let cc = ConformanceClasses::with_crs();
    assert_eq!(cc.conforms_to.len(), 5);
}

#[test]
fn ext_conformance_with_crs_contains_crs_uri() {
    let cc = ConformanceClasses::with_crs();
    assert!(
        cc.conforms_to
            .contains(&"http://www.opengis.net/spec/ogcapi-features-2/1.0/conf/crs".to_string())
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// DateTimeFilter::parse
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_datetime_instant() {
    let f = DateTimeFilter::parse("2021-04-22T00:00:00Z").expect("parse instant datetime");
    assert_eq!(
        f,
        DateTimeFilter::Instant("2021-04-22T00:00:00Z".to_string())
    );
}

#[test]
fn ext_datetime_open_start_interval() {
    let f = DateTimeFilter::parse("../2021-12-31T23:59:59Z").expect("parse open-start interval");
    assert_eq!(
        f,
        DateTimeFilter::Interval(None, Some("2021-12-31T23:59:59Z".to_string()))
    );
}

#[test]
fn ext_datetime_open_end_interval() {
    let f = DateTimeFilter::parse("2021-01-01T00:00:00Z/..").expect("parse open-end interval");
    assert_eq!(
        f,
        DateTimeFilter::Interval(Some("2021-01-01T00:00:00Z".to_string()), None)
    );
}

#[test]
fn ext_datetime_closed_interval() {
    let f = DateTimeFilter::parse("2021-01-01T00:00:00Z/2021-12-31T23:59:59Z")
        .expect("parse closed interval");
    assert_eq!(
        f,
        DateTimeFilter::Interval(
            Some("2021-01-01T00:00:00Z".to_string()),
            Some("2021-12-31T23:59:59Z".to_string())
        )
    );
}

#[test]
fn ext_datetime_empty_string_is_error() {
    assert!(DateTimeFilter::parse("").is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// CrsTransform::bbox_to_wgs84
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_crs84_bbox_identity() {
    let bbox = [-10.0, 40.0, 10.0, 60.0];
    let result = CrsTransform::bbox_to_wgs84(bbox, CRS84_URI).expect("CRS84 bbox transform");
    assert_eq!(result, bbox);
}

#[test]
fn ext_epsg4326_bbox_identity() {
    let bbox = [5.0, 50.0, 15.0, 55.0];
    let result = CrsTransform::bbox_to_wgs84(bbox, EPSG4326_URI).expect("EPSG4326 bbox transform");
    assert_eq!(result, bbox);
}

#[test]
fn ext_epsg4258_bbox_identity() {
    let bbox = [6.0, 47.0, 14.0, 55.0];
    let result = CrsTransform::bbox_to_wgs84(bbox, EPSG4258_URI).expect("EPSG4258 bbox transform");
    assert_eq!(result, bbox);
}

#[test]
fn ext_epsg3857_origin_maps_to_zero() {
    let bbox = [0.0, 0.0, 0.0, 0.0];
    let result =
        CrsTransform::bbox_to_wgs84(bbox, EPSG3857_URI).expect("EPSG3857 origin transform");
    assert!((result[0]).abs() < 1e-9, "lon should be ~0");
    assert!((result[1]).abs() < 1e-9, "lat should be ~0");
}

#[test]
fn ext_epsg3857_inverse_london() {
    // roughly 0°E / 51.5°N in EPSG:3857
    let bbox = [0.0, 6_711_000.0, 0.0, 6_711_000.0];
    let result =
        CrsTransform::bbox_to_wgs84(bbox, EPSG3857_URI).expect("EPSG3857 London inverse transform");
    assert!((result[0]).abs() < 0.5);
    assert!(result[1] > 50.0 && result[1] < 54.0);
}

#[test]
fn ext_unknown_crs_error() {
    let result = CrsTransform::bbox_to_wgs84([0.0, 0.0, 1.0, 1.0], "EPSG:9999");
    assert!(result.is_err());
    assert!(matches!(
        result.expect_err("unknown CRS should error"),
        FeaturesError::InvalidCrs(_)
    ));
}

#[test]
fn ext_supported_crs_uris_includes_all_six() {
    let uris = CrsTransform::supported_crs_uris();
    assert_eq!(uris.len(), 6);
    assert!(uris.contains(&CRS84_URI.to_string()));
    assert!(uris.contains(&EPSG4326_URI.to_string()));
    assert!(uris.contains(&EPSG3857_URI.to_string()));
    assert!(uris.contains(&EPSG4258_URI.to_string()));
    assert!(uris.contains(&EPSG25832_URI.to_string()));
    assert!(uris.contains(&EPSG25833_URI.to_string()));
}

// ═══════════════════════════════════════════════════════════════════════════════
// FeaturesServer — landing page
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_landing_page_title() {
    let s = make_server();
    assert_eq!(s.landing_page().title, "Integration Test API");
}

#[test]
fn ext_landing_page_four_links() {
    let s = make_server();
    assert_eq!(s.landing_page().links.len(), 4);
}

#[test]
fn ext_landing_page_self_link() {
    let s = make_server();
    assert!(s.landing_page().links.iter().any(|l| l.rel == "self"));
}

#[test]
fn ext_landing_page_conformance_link() {
    let s = make_server();
    assert!(
        s.landing_page()
            .links
            .iter()
            .any(|l| l.rel == "conformance")
    );
}

#[test]
fn ext_landing_page_service_desc_link() {
    let s = make_server();
    assert!(
        s.landing_page()
            .links
            .iter()
            .any(|l| l.rel == "service-desc")
    );
}

#[test]
fn ext_landing_page_data_link() {
    let s = make_server();
    assert!(s.landing_page().links.iter().any(|l| l.rel == "data"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// FeaturesServer — conformance
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_server_conformance_includes_crs() {
    let s = make_server();
    assert!(
        s.conformance()
            .conforms_to
            .contains(&"http://www.opengis.net/spec/ogcapi-features-2/1.0/conf/crs".to_string())
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// FeaturesServer — collection CRUD
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_add_and_get_collection() {
    let mut s = FeaturesServer::new("T", "https://x.com");
    s.add_collection(Collection::new("parcels"));
    assert_eq!(
        s.get_collection("parcels")
            .expect("parcels collection should exist")
            .id,
        "parcels"
    );
}

#[test]
fn ext_get_missing_collection_none() {
    let s = make_server();
    assert!(s.get_collection("nonexistent").is_none());
}

#[test]
fn ext_list_collections_count() {
    let s = make_server();
    assert_eq!(s.list_collections().collections.len(), 2);
}

#[test]
fn ext_list_collections_self_link() {
    let s = make_server();
    assert!(s.list_collections().links.iter().any(|l| l.rel == "self"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// FeaturesServer::build_items_response — pagination
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_items_number_returned_exact() {
    let s = make_server();
    let fc = s
        .build_items_response("buildings", make_features(7), &QueryParams::default(), None)
        .expect("build items response");
    assert_eq!(fc.number_returned, Some(7));
}

#[test]
fn ext_items_number_matched_override() {
    let s = make_server();
    let fc = s
        .build_items_response(
            "buildings",
            make_features(5),
            &QueryParams::default(),
            Some(500),
        )
        .expect("build items response with number_matched");
    assert_eq!(fc.number_matched, Some(500));
}

#[test]
fn ext_items_limit_slices_features() {
    let s = make_server();
    let params = QueryParams {
        limit: Some(5),
        ..Default::default()
    };
    let fc = s
        .build_items_response("buildings", make_features(20), &params, None)
        .expect("build items response with limit");
    assert_eq!(fc.features.len(), 5);
}

#[test]
fn ext_items_next_link_present_when_more() {
    let s = make_server();
    let params = QueryParams {
        limit: Some(5),
        ..Default::default()
    };
    let fc = s
        .build_items_response("buildings", make_features(20), &params, None)
        .expect("build items response with limit");
    assert!(
        fc.links
            .as_ref()
            .expect("links should be present")
            .iter()
            .any(|l| l.rel == "next")
    );
}

#[test]
fn ext_items_no_next_link_on_last_page() {
    let s = make_server();
    let params = QueryParams {
        limit: Some(10),
        ..Default::default()
    };
    let fc = s
        .build_items_response("buildings", make_features(5), &params, None)
        .expect("build items response");
    assert!(
        !fc.links
            .as_ref()
            .expect("links should be present")
            .iter()
            .any(|l| l.rel == "next")
    );
}

#[test]
fn ext_items_prev_link_on_page_2() {
    let s = make_server();
    let params = QueryParams {
        limit: Some(5),
        offset: Some(5),
        ..Default::default()
    };
    let fc = s
        .build_items_response("buildings", make_features(20), &params, None)
        .expect("build items response with limit");
    assert!(
        fc.links
            .as_ref()
            .expect("links should be present")
            .iter()
            .any(|l| l.rel == "prev")
    );
}

#[test]
fn ext_items_no_prev_on_page_1() {
    let s = make_server();
    let params = QueryParams::default();
    let fc = s
        .build_items_response("buildings", make_features(5), &params, None)
        .expect("build items response");
    assert!(
        !fc.links
            .as_ref()
            .expect("links should be present")
            .iter()
            .any(|l| l.rel == "prev")
    );
}

#[test]
fn ext_items_collection_not_found_error() {
    let s = make_server();
    let result = s.build_items_response("ghost", vec![], &QueryParams::default(), None);
    assert!(matches!(result, Err(FeaturesError::CollectionNotFound(_))));
}

#[test]
fn ext_items_limit_exceeded_error() {
    let s = make_server();
    let params = QueryParams {
        limit: Some(MAX_LIMIT + 1),
        ..Default::default()
    };
    let result = s.build_items_response("buildings", vec![], &params, None);
    assert!(matches!(result, Err(FeaturesError::LimitExceeded { .. })));
}

#[test]
fn ext_items_timestamp_is_set() {
    let s = make_server();
    let fc = s
        .build_items_response("buildings", vec![], &QueryParams::default(), None)
        .expect("build empty items response");
    assert!(fc.time_stamp.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════════
// QueryParams
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_query_default_limit_10() {
    assert_eq!(QueryParams::default().effective_limit(), 10);
}

#[test]
fn ext_query_default_offset_0() {
    assert_eq!(QueryParams::default().effective_offset(), 0);
}

#[test]
fn ext_query_custom_limit() {
    let p = QueryParams {
        limit: Some(50),
        ..Default::default()
    };
    assert_eq!(p.effective_limit(), 50);
}

#[test]
fn ext_query_bbox_field() {
    let p = QueryParams {
        bbox: Some([-5.0, 40.0, 5.0, 50.0]),
        ..Default::default()
    };
    assert!(p.bbox.is_some());
}

#[test]
fn ext_query_filter_lang_cql2text() {
    let p = QueryParams {
        filter_lang: Some(FilterLang::Cql2Text),
        ..Default::default()
    };
    assert_eq!(p.filter_lang, Some(FilterLang::Cql2Text));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Collection — Part 2 CRS fields
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_collection_default_item_type_feature() {
    assert_eq!(Collection::new("x").item_type.as_deref(), Some("feature"));
}

#[test]
fn ext_collection_crs_list_part2() {
    let mut col = Collection::new("grid");
    col.crs = CrsTransform::supported_crs_uris();
    assert_eq!(col.crs.len(), 6);
    assert!(col.crs.contains(&CRS84_URI.to_string()));
}

#[test]
fn ext_collection_storage_crs() {
    let mut col = Collection::new("raster");
    col.storage_crs = Some(EPSG3857_URI.to_string());
    assert_eq!(col.storage_crs.as_deref(), Some(EPSG3857_URI));
}

// ═══════════════════════════════════════════════════════════════════════════════
// FeatureCollection — GeoJSON serialisation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_fc_type_key_is_type_not_type_underscore() {
    let v = serde_json::to_value(FeatureCollection::new()).expect("serialize FeatureCollection");
    assert_eq!(v["type"], "FeatureCollection");
    assert!(v.get("type_").is_none());
}

#[test]
fn ext_fc_number_matched_camel_case() {
    let mut fc = FeatureCollection::new();
    fc.number_matched = Some(99);
    let v = serde_json::to_value(&fc).expect("serialize FeatureCollection with numberMatched");
    assert_eq!(v["numberMatched"], 99);
    assert!(v.get("number_matched").is_none());
}

#[test]
fn ext_fc_deserialise_geojson() {
    let raw = r#"{"type":"FeatureCollection","features":[]}"#;
    let fc: FeatureCollection = serde_json::from_str(raw).expect("deserialize FeatureCollection");
    assert_eq!(fc.type_, "FeatureCollection");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Feature — GeoJSON serialisation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_feature_type_key() {
    let v = serde_json::to_value(Feature::new()).expect("serialize Feature");
    assert_eq!(v["type"], "Feature");
    assert!(v.get("type_").is_none());
}

#[test]
fn ext_feature_string_id_serialises_as_string() {
    let mut f = Feature::new();
    f.id = Some(FeatureId::String("abc-123".to_string()));
    let v = serde_json::to_value(&f).expect("serialize Feature with id");
    assert_eq!(v["id"], "abc-123");
}

#[test]
fn ext_feature_integer_id_serialises_as_number() {
    let mut f = Feature::new();
    f.id = Some(FeatureId::Integer(42));
    let v = serde_json::to_value(&f).expect("serialize Feature with id");
    assert_eq!(v["id"], 42);
}

#[test]
fn ext_feature_null_geometry() {
    let v = serde_json::to_value(Feature::new()).expect("serialize Feature");
    assert_eq!(v["geometry"], serde_json::Value::Null);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Link serialisation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_link_type_key_not_type_underscore() {
    let l = Link::new("https://x.com", "self").with_type("application/json");
    let v = serde_json::to_value(&l).expect("serialize Link");
    assert_eq!(v["type"], "application/json");
    assert!(v.get("type_").is_none());
}

#[test]
fn ext_link_optional_fields_absent_when_none() {
    let l = Link::new("https://x.com", "self");
    let v = serde_json::to_value(&l).expect("serialize Link");
    assert!(v.get("title").is_none());
    assert!(v.get("hreflang").is_none());
}

#[test]
fn ext_link_title_present_when_set() {
    let l = Link::new("https://x.com", "self").with_title("Root");
    let v = serde_json::to_value(&l).expect("serialize Link");
    assert_eq!(v["title"], "Root");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Extent
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_extent_with_spatial_and_temporal() {
    let e = Extent {
        spatial: Some(SpatialExtent {
            bbox: vec![[-180.0, -90.0, 180.0, 90.0]],
            crs: None,
        }),
        temporal: Some(TemporalExtent {
            interval: vec![[Some("2000-01-01T00:00:00Z".to_string()), None]],
            trs: None,
        }),
    };
    assert!(e.spatial.is_some());
    assert!(e.temporal.is_some());
}

#[test]
fn ext_extent_spatial_only_no_temporal_key() {
    let e = Extent {
        spatial: Some(SpatialExtent {
            bbox: vec![[0.0, 0.0, 1.0, 1.0]],
            crs: None,
        }),
        temporal: None,
    };
    let v = serde_json::to_value(&e).expect("serialize Extent");
    assert!(v.get("temporal").is_none());
    assert!(v["spatial"].is_object());
}

// ═══════════════════════════════════════════════════════════════════════════════
// CqlParser — parse
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_cql_eq_string() {
    let e = CqlParser::parse("name = 'London'").expect("parse CQL eq string");
    assert_eq!(
        e,
        CqlExpr::Eq {
            property: "name".to_string(),
            value: CqlValue::String("London".to_string())
        }
    );
}

#[test]
fn ext_cql_gt() {
    let e = CqlParser::parse("population > 1000000").expect("parse CQL gt");
    assert_eq!(
        e,
        CqlExpr::Gt {
            property: "population".to_string(),
            value: 1_000_000.0
        }
    );
}

#[test]
fn ext_cql_lt() {
    let e = CqlParser::parse("depth < 200").expect("parse CQL lt");
    assert_eq!(
        e,
        CqlExpr::Lt {
            property: "depth".to_string(),
            value: 200.0
        }
    );
}

#[test]
fn ext_cql_gte() {
    let e = CqlParser::parse("score >= 90").expect("parse CQL gte");
    assert_eq!(
        e,
        CqlExpr::Gte {
            property: "score".to_string(),
            value: 90.0
        }
    );
}

#[test]
fn ext_cql_lte() {
    let e = CqlParser::parse("rank <= 5").expect("parse CQL lte");
    assert_eq!(
        e,
        CqlExpr::Lte {
            property: "rank".to_string(),
            value: 5.0
        }
    );
}

#[test]
fn ext_cql_like() {
    let e = CqlParser::parse("name LIKE '%city%'").expect("parse CQL like");
    assert_eq!(
        e,
        CqlExpr::Like {
            property: "name".to_string(),
            pattern: "%city%".to_string()
        }
    );
}

#[test]
fn ext_cql_between() {
    let e = CqlParser::parse("age BETWEEN 18 AND 65").expect("parse CQL between");
    assert_eq!(
        e,
        CqlExpr::Between {
            property: "age".to_string(),
            low: 18.0,
            high: 65.0
        }
    );
}

#[test]
fn ext_cql_and() {
    let e = CqlParser::parse("a > 5 AND b < 10").expect("parse CQL and");
    assert!(matches!(e, CqlExpr::And(_, _)));
}

#[test]
fn ext_cql_or() {
    let e = CqlParser::parse("x = 1 OR y = 2").expect("parse CQL or");
    assert!(matches!(e, CqlExpr::Or(_, _)));
}

#[test]
fn ext_cql_not() {
    let e = CqlParser::parse("NOT (active = TRUE)").expect("parse CQL not");
    assert!(matches!(e, CqlExpr::Not(_)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// CqlParser — evaluate
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_eval_eq_match() {
    let e = CqlParser::parse("city = 'Tokyo'").expect("parse CQL city eq");
    assert!(CqlParser::evaluate(&e, &json!({"city": "Tokyo"})));
}

#[test]
fn ext_eval_eq_no_match() {
    let e = CqlParser::parse("city = 'Tokyo'").expect("parse CQL city eq");
    assert!(!CqlParser::evaluate(&e, &json!({"city": "Berlin"})));
}

#[test]
fn ext_eval_lt_match() {
    let e = CqlParser::parse("temp < 0").expect("parse CQL temp lt");
    assert!(CqlParser::evaluate(&e, &json!({"temp": -5})));
}

#[test]
fn ext_eval_gt_match() {
    let e = CqlParser::parse("pop > 1000").expect("parse CQL pop gt");
    assert!(CqlParser::evaluate(&e, &json!({"pop": 5000})));
}

#[test]
fn ext_eval_like_wildcard_match() {
    let e = CqlParser::parse("name LIKE '%burg'").expect("parse CQL like burg");
    assert!(CqlParser::evaluate(&e, &json!({"name": "Hamburg"})));
}

#[test]
fn ext_eval_like_no_match() {
    let e = CqlParser::parse("name LIKE 'Big%'").expect("parse CQL like Big");
    assert!(!CqlParser::evaluate(&e, &json!({"name": "Small"})));
}

#[test]
fn ext_eval_between_match() {
    let e = CqlParser::parse("age BETWEEN 18 AND 65").expect("parse CQL between");
    assert!(CqlParser::evaluate(&e, &json!({"age": 30})));
}

#[test]
fn ext_eval_between_boundary_low() {
    let e = CqlParser::parse("age BETWEEN 18 AND 65").expect("parse CQL between");
    assert!(CqlParser::evaluate(&e, &json!({"age": 18})));
}

#[test]
fn ext_eval_between_boundary_high() {
    let e = CqlParser::parse("age BETWEEN 18 AND 65").expect("parse CQL between");
    assert!(CqlParser::evaluate(&e, &json!({"age": 65})));
}

#[test]
fn ext_eval_between_out_of_range() {
    let e = CqlParser::parse("age BETWEEN 18 AND 65").expect("parse CQL between");
    assert!(!CqlParser::evaluate(&e, &json!({"age": 10})));
}

#[test]
fn ext_eval_and_both_true() {
    let e = CqlParser::parse("a > 5 AND b < 10").expect("parse CQL and");
    assert!(CqlParser::evaluate(&e, &json!({"a": 8, "b": 3})));
}

#[test]
fn ext_eval_and_second_false() {
    let e = CqlParser::parse("a > 5 AND b < 10").expect("parse CQL and");
    assert!(!CqlParser::evaluate(&e, &json!({"a": 8, "b": 20})));
}

#[test]
fn ext_eval_or_first_true() {
    let e = CqlParser::parse("x = 1 OR y = 2").expect("parse CQL or");
    assert!(CqlParser::evaluate(&e, &json!({"x": 1, "y": 99})));
}

#[test]
fn ext_eval_or_both_false() {
    let e = CqlParser::parse("x = 1 OR y = 2").expect("parse CQL or");
    assert!(!CqlParser::evaluate(&e, &json!({"x": 9, "y": 9})));
}

#[test]
fn ext_eval_not_true_becomes_false() {
    let e = CqlParser::parse("NOT (active = TRUE)").expect("parse CQL not");
    assert!(!CqlParser::evaluate(&e, &json!({"active": true})));
}

#[test]
fn ext_eval_not_false_becomes_true() {
    let e = CqlParser::parse("NOT (active = TRUE)").expect("parse CQL not");
    assert!(CqlParser::evaluate(&e, &json!({"active": false})));
}

// ═══════════════════════════════════════════════════════════════════════════════
// FeaturesError variants
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ext_err_collection_not_found_msg() {
    let e = FeaturesError::CollectionNotFound("x".to_string());
    assert!(e.to_string().contains("x"));
}

#[test]
fn ext_err_invalid_bbox_msg() {
    let e = FeaturesError::InvalidBbox("bad".to_string());
    assert!(e.to_string().contains("Invalid bbox"));
}

#[test]
fn ext_err_invalid_datetime_msg() {
    let e = FeaturesError::InvalidDatetime("bad".to_string());
    assert!(e.to_string().contains("Invalid datetime"));
}

#[test]
fn ext_err_invalid_crs_msg() {
    let e = FeaturesError::InvalidCrs("x".to_string());
    assert!(e.to_string().contains("Invalid CRS"));
}

#[test]
fn ext_err_limit_exceeded_msg() {
    let e = FeaturesError::LimitExceeded {
        requested: 99_999,
        max: MAX_LIMIT,
    };
    assert!(e.to_string().contains("99999"));
}

#[test]
fn ext_err_serde_from_conversion() {
    let json_err =
        serde_json::from_str::<serde_json::Value>("!!").expect_err("invalid JSON should error");
    let e: FeaturesError = json_err.into();
    assert!(matches!(e, FeaturesError::SerdeError(_)));
}
