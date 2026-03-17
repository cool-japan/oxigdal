//! OGC API - Features Part 1 (Core) and Part 2 (CRS) implementation
//!
//! Implements the OGC API - Features standard for serving geospatial features
//! via a RESTful API with GeoJSON responses.
//!
//! # Part 1 — Core
//! - Landing page, conformance, collections, items, and single feature endpoints
//! - Filtering by bbox, datetime, and CQL2 expressions
//! - Pagination with next/prev links
//!
//! # Part 2 — CRS
//! - CRS negotiation via `crs` and `bbox-crs` query parameters
//! - CRS transformation for bbox queries
//! - Per-collection CRS list and storage CRS

mod cql;
mod crs;
mod error;
mod query;
mod server;
mod types;

pub use cql::*;
pub use crs::*;
pub use error::*;
pub use query::*;
pub use server::*;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── ConformanceClasses ────────────────────────────────────────────────────

    #[test]
    fn test_core_has_four_uris() {
        let cc = ConformanceClasses::ogc_features_core();
        assert_eq!(cc.conforms_to.len(), 4);
    }

    #[test]
    fn test_core_contains_expected_uris() {
        let cc = ConformanceClasses::ogc_features_core();
        assert!(
            cc.conforms_to.contains(
                &"http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/core".to_string()
            )
        );
        assert!(cc.conforms_to.contains(
            &"http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/geojson".to_string()
        ));
    }

    #[test]
    fn test_with_crs_adds_crs_uri() {
        let cc = ConformanceClasses::with_crs();
        assert_eq!(cc.conforms_to.len(), 5);
        assert!(
            cc.conforms_to.contains(
                &"http://www.opengis.net/spec/ogcapi-features-2/1.0/conf/crs".to_string()
            )
        );
    }

    // ── DateTimeFilter ────────────────────────────────────────────────────────

    #[test]
    fn test_datetime_instant() {
        let dt = DateTimeFilter::parse("2021-04-22T00:00:00Z");
        assert!(dt.is_ok());
        let dt = dt.expect("should parse");
        assert_eq!(
            dt,
            DateTimeFilter::Instant("2021-04-22T00:00:00Z".to_string())
        );
    }

    #[test]
    fn test_datetime_open_start() {
        let dt = DateTimeFilter::parse("../2021-01-01T00:00:00Z");
        assert!(dt.is_ok());
        let dt = dt.expect("should parse");
        assert_eq!(
            dt,
            DateTimeFilter::Interval(None, Some("2021-01-01T00:00:00Z".to_string()))
        );
    }

    #[test]
    fn test_datetime_open_end() {
        let dt = DateTimeFilter::parse("2021-01-01T00:00:00Z/..");
        assert!(dt.is_ok());
        let dt = dt.expect("should parse");
        assert_eq!(
            dt,
            DateTimeFilter::Interval(Some("2021-01-01T00:00:00Z".to_string()), None)
        );
    }

    #[test]
    fn test_datetime_closed_interval() {
        let dt = DateTimeFilter::parse("2021-01-01T00:00:00Z/2021-12-31T23:59:59Z");
        assert!(dt.is_ok());
        let dt = dt.expect("should parse");
        assert_eq!(
            dt,
            DateTimeFilter::Interval(
                Some("2021-01-01T00:00:00Z".to_string()),
                Some("2021-12-31T23:59:59Z".to_string())
            )
        );
    }

    #[test]
    fn test_datetime_empty_is_error() {
        assert!(DateTimeFilter::parse("").is_err());
    }

    // ── CrsTransform ─────────────────────────────────────────────────────────

    #[test]
    fn test_crs84_identity() {
        let bbox = [-10.0, 40.0, 10.0, 60.0];
        let result = CrsTransform::bbox_to_wgs84(bbox, CRS84_URI);
        assert!(result.is_ok());
        assert_eq!(result.expect("should transform"), bbox);
    }

    #[test]
    fn test_epsg4326_identity() {
        let bbox = [0.0, 50.0, 5.0, 55.0];
        let result = CrsTransform::bbox_to_wgs84(bbox, EPSG4326_URI);
        assert!(result.is_ok());
        assert_eq!(result.expect("should transform"), bbox);
    }

    #[test]
    fn test_epsg4258_identity() {
        let bbox = [5.0, 47.0, 15.0, 55.0];
        let result = CrsTransform::bbox_to_wgs84(bbox, EPSG4258_URI);
        assert!(result.is_ok());
        assert_eq!(result.expect("should transform"), bbox);
    }

    #[test]
    fn test_epsg3857_inverse_origin() {
        // (0, 0) in EPSG:3857 → (0.0, 0.0) in WGS84
        let bbox = [0.0, 0.0, 0.0, 0.0];
        let result = CrsTransform::bbox_to_wgs84(bbox, EPSG3857_URI);
        assert!(result.is_ok());
        let result = result.expect("should transform");
        assert!((result[0]).abs() < 1e-9);
        assert!((result[1]).abs() < 1e-9);
    }

    #[test]
    fn test_epsg3857_inverse_known_point() {
        // London roughly: 0°W, 51.5°N → EPSG:3857 ~(0, 6_711_000)
        let x = 0.0_f64;
        let y = 6_711_000.0_f64;
        let bbox = [x, y, x, y];
        let result = CrsTransform::bbox_to_wgs84(bbox, EPSG3857_URI);
        assert!(result.is_ok());
        let result = result.expect("should transform");
        // longitude should be ~0
        assert!((result[0]).abs() < 0.1);
        // latitude should be ~51-52
        assert!(result[1] > 50.0 && result[1] < 55.0);
    }

    #[test]
    fn test_unknown_crs_returns_error() {
        let bbox = [0.0, 0.0, 1.0, 1.0];
        assert!(CrsTransform::bbox_to_wgs84(bbox, "EPSG:9999").is_err());
    }

    #[test]
    fn test_supported_crs_uris_count() {
        assert_eq!(CrsTransform::supported_crs_uris().len(), 6);
    }

    #[test]
    fn test_crs_is_supported() {
        assert!(CrsTransform::is_supported(CRS84_URI));
        assert!(CrsTransform::is_supported(EPSG3857_URI));
        assert!(!CrsTransform::is_supported("EPSG:9999"));
    }

    // ── FeaturesServer ────────────────────────────────────────────────────────

    fn make_server() -> FeaturesServer {
        let mut server = FeaturesServer::new("Test API", "https://example.com/ogcapi");
        let col = Collection::new("buildings");
        server.add_collection(col);
        server
    }

    #[test]
    fn test_landing_page_title() {
        let server = make_server();
        let lp = server.landing_page();
        assert_eq!(lp.title, "Test API");
    }

    #[test]
    fn test_landing_page_has_four_links() {
        let server = make_server();
        let lp = server.landing_page();
        assert_eq!(lp.links.len(), 4);
    }

    #[test]
    fn test_landing_page_has_self_link() {
        let server = make_server();
        let lp = server.landing_page();
        assert!(lp.links.iter().any(|l| l.rel == "self"));
    }

    #[test]
    fn test_landing_page_has_conformance_link() {
        let server = make_server();
        let lp = server.landing_page();
        assert!(lp.links.iter().any(|l| l.rel == "conformance"));
    }

    #[test]
    fn test_conformance_includes_crs() {
        let server = make_server();
        let cc = server.conformance();
        assert!(
            cc.conforms_to.contains(
                &"http://www.opengis.net/spec/ogcapi-features-2/1.0/conf/crs".to_string()
            )
        );
    }

    #[test]
    fn test_add_and_get_collection() {
        let mut server = FeaturesServer::new("API", "https://example.com");
        let col = Collection::new("rivers");
        server.add_collection(col);
        let found = server.get_collection("rivers");
        assert!(found.is_some());
        assert_eq!(found.expect("should find rivers").id, "rivers");
    }

    #[test]
    fn test_get_collection_missing_returns_none() {
        let server = make_server();
        assert!(server.get_collection("does-not-exist").is_none());
    }

    #[test]
    fn test_list_collections_count() {
        let server = make_server();
        let cols = server.list_collections();
        assert_eq!(cols.collections.len(), 1);
    }

    #[test]
    fn test_list_collections_has_self_link() {
        let server = make_server();
        let cols = server.list_collections();
        assert!(cols.links.iter().any(|l| l.rel == "self"));
    }

    // ── build_items_response ──────────────────────────────────────────────────

    fn make_features(n: usize) -> Vec<Feature> {
        (0..n)
            .map(|i| {
                let mut f = Feature::new();
                f.id = Some(FeatureId::Integer(i as i64));
                f
            })
            .collect()
    }

    #[test]
    fn test_build_items_number_returned() {
        let server = make_server();
        let features = make_features(5);
        let params = QueryParams::default();
        let fc = server.build_items_response("buildings", features, &params, None);
        assert!(fc.is_ok());
        let fc = fc.expect("should build");
        assert_eq!(fc.number_returned, Some(5));
    }

    #[test]
    fn test_build_items_number_matched_from_total() {
        let server = make_server();
        let features = make_features(3);
        let params = QueryParams::default();
        let fc = server.build_items_response("buildings", features, &params, Some(100));
        assert!(fc.is_ok());
        let fc = fc.expect("should build");
        assert_eq!(fc.number_matched, Some(100));
    }

    #[test]
    fn test_build_items_pagination_limit() {
        let server = make_server();
        let features = make_features(25);
        let params = QueryParams {
            limit: Some(10),
            ..Default::default()
        };
        let fc = server.build_items_response("buildings", features, &params, None);
        assert!(fc.is_ok());
        let fc = fc.expect("should build");
        assert_eq!(fc.features.len(), 10);
        assert_eq!(fc.number_returned, Some(10));
    }

    #[test]
    fn test_build_items_pagination_next_link() {
        let server = make_server();
        let features = make_features(25);
        let params = QueryParams {
            limit: Some(10),
            ..Default::default()
        };
        let fc = server.build_items_response("buildings", features, &params, None);
        assert!(fc.is_ok());
        let fc = fc.expect("should build");
        let links = fc.links.as_ref().expect("should have links");
        assert!(links.iter().any(|l| l.rel == "next"));
    }

    #[test]
    fn test_build_items_no_next_on_last_page() {
        let server = make_server();
        let features = make_features(5);
        let params = QueryParams {
            limit: Some(10),
            ..Default::default()
        };
        let fc = server.build_items_response("buildings", features, &params, None);
        assert!(fc.is_ok());
        let fc = fc.expect("should build");
        let links = fc.links.as_ref().expect("should have links");
        assert!(!links.iter().any(|l| l.rel == "next"));
    }

    #[test]
    fn test_build_items_prev_link_on_second_page() {
        let server = make_server();
        let features = make_features(25);
        let params = QueryParams {
            limit: Some(10),
            offset: Some(10),
            ..Default::default()
        };
        let fc = server.build_items_response("buildings", features, &params, None);
        assert!(fc.is_ok());
        let fc = fc.expect("should build");
        let links = fc.links.as_ref().expect("should have links");
        assert!(links.iter().any(|l| l.rel == "prev"));
    }

    #[test]
    fn test_build_items_no_prev_on_first_page() {
        let server = make_server();
        let features = make_features(25);
        let params = QueryParams {
            limit: Some(10),
            offset: Some(0),
            ..Default::default()
        };
        let fc = server.build_items_response("buildings", features, &params, None);
        assert!(fc.is_ok());
        let fc = fc.expect("should build");
        let links = fc.links.as_ref().expect("should have links");
        assert!(!links.iter().any(|l| l.rel == "prev"));
    }

    #[test]
    fn test_build_items_collection_not_found() {
        let server = make_server();
        let params = QueryParams::default();
        let result = server.build_items_response("nonexistent", vec![], &params, None);
        assert!(matches!(result, Err(FeaturesError::CollectionNotFound(_))));
    }

    #[test]
    fn test_build_items_limit_exceeded() {
        let server = make_server();
        let params = QueryParams {
            limit: Some(MAX_LIMIT + 1),
            ..Default::default()
        };
        let result = server.build_items_response("buildings", vec![], &params, None);
        assert!(matches!(result, Err(FeaturesError::LimitExceeded { .. })));
    }

    #[test]
    fn test_build_items_timestamp_present() {
        let server = make_server();
        let params = QueryParams::default();
        let fc = server.build_items_response("buildings", vec![], &params, None);
        assert!(fc.is_ok());
        let fc = fc.expect("should build");
        assert!(fc.time_stamp.is_some());
    }

    // ── QueryParams ───────────────────────────────────────────────────────────

    #[test]
    fn test_query_params_default_limit() {
        let p = QueryParams::default();
        assert_eq!(p.effective_limit(), 10);
    }

    #[test]
    fn test_query_params_default_offset() {
        let p = QueryParams::default();
        assert_eq!(p.effective_offset(), 0);
    }

    #[test]
    fn test_query_params_custom_limit() {
        let p = QueryParams {
            limit: Some(50),
            ..Default::default()
        };
        assert_eq!(p.effective_limit(), 50);
    }

    #[test]
    fn test_query_params_bbox() {
        let p = QueryParams {
            bbox: Some([-10.0, 40.0, 10.0, 60.0]),
            ..Default::default()
        };
        assert!(p.bbox.is_some());
    }

    #[test]
    fn test_query_params_crs() {
        let p = QueryParams {
            crs: Some(EPSG3857_URI.to_string()),
            ..Default::default()
        };
        assert_eq!(p.crs.as_deref(), Some(EPSG3857_URI));
    }

    // ── Collection ────────────────────────────────────────────────────────────

    #[test]
    fn test_collection_default_item_type() {
        let col = Collection::new("parcels");
        assert_eq!(col.item_type.as_deref(), Some("feature"));
    }

    #[test]
    fn test_collection_with_crs_list() {
        let mut col = Collection::new("roads");
        col.crs = CrsTransform::supported_crs_uris();
        assert_eq!(col.crs.len(), 6);
    }

    #[test]
    fn test_collection_storage_crs() {
        let mut col = Collection::new("tiles");
        col.storage_crs = Some(EPSG3857_URI.to_string());
        assert_eq!(col.storage_crs.as_deref(), Some(EPSG3857_URI));
    }

    // ── FeatureCollection serialisation ──────────────────────────────────────

    #[test]
    fn test_feature_collection_type_field() {
        let fc = FeatureCollection::new();
        let v = serde_json::to_value(&fc);
        assert!(v.is_ok());
        let v = v.expect("should serialize");
        assert_eq!(v["type"], "FeatureCollection");
        // Must NOT have "type_" key
        assert!(v.get("type_").is_none());
    }

    #[test]
    fn test_feature_collection_deserialise() {
        let json = r#"{"type":"FeatureCollection","features":[]}"#;
        let fc: Result<FeatureCollection, _> = serde_json::from_str(json);
        assert!(fc.is_ok());
        let fc = fc.expect("should deserialize");
        assert_eq!(fc.type_, "FeatureCollection");
        assert!(fc.features.is_empty());
    }

    #[test]
    fn test_feature_collection_number_matched() {
        let mut fc = FeatureCollection::new();
        fc.number_matched = Some(42);
        let v = serde_json::to_value(&fc);
        assert!(v.is_ok());
        let v = v.expect("should serialize");
        assert_eq!(v["numberMatched"], 42);
    }

    // ── Feature serialisation ─────────────────────────────────────────────────

    #[test]
    fn test_feature_type_field() {
        let f = Feature::new();
        let v = serde_json::to_value(&f);
        assert!(v.is_ok());
        let v = v.expect("should serialize");
        assert_eq!(v["type"], "Feature");
        assert!(v.get("type_").is_none());
    }

    #[test]
    fn test_feature_string_id() {
        let mut f = Feature::new();
        f.id = Some(FeatureId::String("abc".to_string()));
        let v = serde_json::to_value(&f);
        assert!(v.is_ok());
        let v = v.expect("should serialize");
        assert_eq!(v["id"], "abc");
    }

    #[test]
    fn test_feature_integer_id() {
        let mut f = Feature::new();
        f.id = Some(FeatureId::Integer(42));
        let v = serde_json::to_value(&f);
        assert!(v.is_ok());
        let v = v.expect("should serialize");
        assert_eq!(v["id"], 42);
    }

    #[test]
    fn test_feature_null_geometry() {
        let f = Feature::new();
        let v = serde_json::to_value(&f);
        assert!(v.is_ok());
        let v = v.expect("should serialize");
        assert_eq!(v["geometry"], serde_json::Value::Null);
    }

    // ── Link serialisation ────────────────────────────────────────────────────

    #[test]
    fn test_link_type_key_not_type_underscore() {
        let link = Link::new("https://example.com", "self").with_type("application/json");
        let v = serde_json::to_value(&link);
        assert!(v.is_ok());
        let v = v.expect("should serialize");
        assert_eq!(v["type"], "application/json");
        assert!(v.get("type_").is_none());
    }

    #[test]
    fn test_link_optional_fields_absent_when_none() {
        let link = Link::new("https://example.com", "self");
        let v = serde_json::to_value(&link);
        assert!(v.is_ok());
        let v = v.expect("should serialize");
        assert!(v.get("title").is_none());
        assert!(v.get("hreflang").is_none());
    }

    // ── Extent ────────────────────────────────────────────────────────────────

    #[test]
    fn test_extent_spatial_and_temporal() {
        let extent = Extent {
            spatial: Some(SpatialExtent {
                bbox: vec![[-180.0, -90.0, 180.0, 90.0]],
                crs: None,
            }),
            temporal: Some(TemporalExtent {
                interval: vec![[Some("2020-01-01T00:00:00Z".to_string()), None]],
                trs: None,
            }),
        };
        assert!(extent.spatial.is_some());
        assert!(extent.temporal.is_some());
    }

    #[test]
    fn test_extent_serialise() {
        let extent = Extent {
            spatial: Some(SpatialExtent {
                bbox: vec![[0.0, 0.0, 1.0, 1.0]],
                crs: Some(CRS84_URI.to_string()),
            }),
            temporal: None,
        };
        let v = serde_json::to_value(&extent);
        assert!(v.is_ok());
        let v = v.expect("should serialize");
        assert!(v["spatial"].is_object());
        assert!(v.get("temporal").is_none());
    }

    // ── CqlParser::parse ──────────────────────────────────────────────────────

    #[test]
    fn test_cql_parse_eq_string() {
        let expr = CqlParser::parse("name = 'London'");
        assert!(expr.is_ok());
        let expr = expr.expect("should parse");
        assert_eq!(
            expr,
            CqlExpr::Eq {
                property: "name".to_string(),
                value: CqlValue::String("London".to_string())
            }
        );
    }

    #[test]
    fn test_cql_parse_eq_number() {
        let expr = CqlParser::parse("code = 42");
        assert!(expr.is_ok());
        let expr = expr.expect("should parse");
        assert_eq!(
            expr,
            CqlExpr::Eq {
                property: "code".to_string(),
                value: CqlValue::Number(42.0)
            }
        );
    }

    #[test]
    fn test_cql_parse_gt() {
        let expr = CqlParser::parse("population > 1000000");
        assert!(expr.is_ok());
        let expr = expr.expect("should parse");
        assert_eq!(
            expr,
            CqlExpr::Gt {
                property: "population".to_string(),
                value: 1_000_000.0
            }
        );
    }

    #[test]
    fn test_cql_parse_lt() {
        let expr = CqlParser::parse("elevation < 500");
        assert!(expr.is_ok());
        let expr = expr.expect("should parse");
        assert_eq!(
            expr,
            CqlExpr::Lt {
                property: "elevation".to_string(),
                value: 500.0
            }
        );
    }

    #[test]
    fn test_cql_parse_gte() {
        let expr = CqlParser::parse("score >= 7");
        assert!(expr.is_ok());
        let expr = expr.expect("should parse");
        assert_eq!(
            expr,
            CqlExpr::Gte {
                property: "score".to_string(),
                value: 7.0
            }
        );
    }

    #[test]
    fn test_cql_parse_lte() {
        let expr = CqlParser::parse("rank <= 3");
        assert!(expr.is_ok());
        let expr = expr.expect("should parse");
        assert_eq!(
            expr,
            CqlExpr::Lte {
                property: "rank".to_string(),
                value: 3.0
            }
        );
    }

    #[test]
    fn test_cql_parse_like() {
        let expr = CqlParser::parse("name LIKE '%city%'");
        assert!(expr.is_ok());
        let expr = expr.expect("should parse");
        assert_eq!(
            expr,
            CqlExpr::Like {
                property: "name".to_string(),
                pattern: "%city%".to_string()
            }
        );
    }

    #[test]
    fn test_cql_parse_between() {
        let expr = CqlParser::parse("age BETWEEN 18 AND 65");
        assert!(expr.is_ok());
        let expr = expr.expect("should parse");
        assert_eq!(
            expr,
            CqlExpr::Between {
                property: "age".to_string(),
                low: 18.0,
                high: 65.0
            }
        );
    }

    #[test]
    fn test_cql_parse_and() {
        let expr = CqlParser::parse("a > 5 AND b < 10");
        assert!(expr.is_ok());
        let expr = expr.expect("should parse");
        assert!(matches!(expr, CqlExpr::And(_, _)));
    }

    #[test]
    fn test_cql_parse_or() {
        let expr = CqlParser::parse("x = 1 OR y = 2");
        assert!(expr.is_ok());
        let expr = expr.expect("should parse");
        assert!(matches!(expr, CqlExpr::Or(_, _)));
    }

    #[test]
    fn test_cql_parse_not() {
        let expr = CqlParser::parse("NOT (active = TRUE)");
        assert!(expr.is_ok());
        let expr = expr.expect("should parse");
        assert!(matches!(expr, CqlExpr::Not(_)));
    }

    // ── CqlParser::evaluate ───────────────────────────────────────────────────

    #[test]
    fn test_eval_eq_string_match() {
        let expr = CqlParser::parse("city = 'London'").expect("should parse");
        let props = json!({"city": "London"});
        assert!(CqlParser::evaluate(&expr, &props));
    }

    #[test]
    fn test_eval_eq_string_no_match() {
        let expr = CqlParser::parse("city = 'Paris'").expect("should parse");
        let props = json!({"city": "London"});
        assert!(!CqlParser::evaluate(&expr, &props));
    }

    #[test]
    fn test_eval_gt_match() {
        let expr = CqlParser::parse("pop > 100").expect("should parse");
        let props = json!({"pop": 200});
        assert!(CqlParser::evaluate(&expr, &props));
    }

    #[test]
    fn test_eval_gt_no_match() {
        let expr = CqlParser::parse("pop > 300").expect("should parse");
        let props = json!({"pop": 200});
        assert!(!CqlParser::evaluate(&expr, &props));
    }

    #[test]
    fn test_eval_lt_match() {
        let expr = CqlParser::parse("temp < 10").expect("should parse");
        let props = json!({"temp": 5});
        assert!(CqlParser::evaluate(&expr, &props));
    }

    #[test]
    fn test_eval_between_match() {
        let expr = CqlParser::parse("age BETWEEN 18 AND 65").expect("should parse");
        let props = json!({"age": 30});
        assert!(CqlParser::evaluate(&expr, &props));
    }

    #[test]
    fn test_eval_between_no_match() {
        let expr = CqlParser::parse("age BETWEEN 18 AND 65").expect("should parse");
        let props = json!({"age": 10});
        assert!(!CqlParser::evaluate(&expr, &props));
    }

    #[test]
    fn test_eval_like_match() {
        let expr = CqlParser::parse("name LIKE '%city%'").expect("should parse");
        let props = json!({"name": "New York city"});
        assert!(CqlParser::evaluate(&expr, &props));
    }

    #[test]
    fn test_eval_like_no_match() {
        let expr = CqlParser::parse("name LIKE 'Big%'").expect("should parse");
        let props = json!({"name": "Small town"});
        assert!(!CqlParser::evaluate(&expr, &props));
    }

    #[test]
    fn test_eval_and_both_true() {
        let expr = CqlParser::parse("a > 5 AND b < 10").expect("should parse");
        let props = json!({"a": 7, "b": 8});
        assert!(CqlParser::evaluate(&expr, &props));
    }

    #[test]
    fn test_eval_and_one_false() {
        let expr = CqlParser::parse("a > 5 AND b < 10").expect("should parse");
        let props = json!({"a": 7, "b": 15});
        assert!(!CqlParser::evaluate(&expr, &props));
    }

    #[test]
    fn test_eval_or_one_true() {
        let expr = CqlParser::parse("x = 1 OR y = 2").expect("should parse");
        let props = json!({"x": 99, "y": 2});
        assert!(CqlParser::evaluate(&expr, &props));
    }

    #[test]
    fn test_eval_or_both_false() {
        let expr = CqlParser::parse("x = 1 OR y = 2").expect("should parse");
        let props = json!({"x": 99, "y": 99});
        assert!(!CqlParser::evaluate(&expr, &props));
    }

    #[test]
    fn test_eval_not_inverts() {
        let expr = CqlParser::parse("NOT (active = TRUE)").expect("should parse");
        let props = json!({"active": false});
        assert!(CqlParser::evaluate(&expr, &props));
    }

    #[test]
    fn test_eval_not_active_false() {
        let expr = CqlParser::parse("NOT (active = TRUE)").expect("should parse");
        let props = json!({"active": true});
        assert!(!CqlParser::evaluate(&expr, &props));
    }

    // ── FeaturesError ─────────────────────────────────────────────────────────

    #[test]
    fn test_error_collection_not_found() {
        let e = FeaturesError::CollectionNotFound("foo".to_string());
        assert!(e.to_string().contains("foo"));
    }

    #[test]
    fn test_error_invalid_bbox() {
        let e = FeaturesError::InvalidBbox("too many coords".to_string());
        assert!(e.to_string().contains("Invalid bbox"));
    }

    #[test]
    fn test_error_invalid_datetime() {
        let e = FeaturesError::InvalidDatetime("bad".to_string());
        assert!(e.to_string().contains("Invalid datetime"));
    }

    #[test]
    fn test_error_invalid_crs() {
        let e = FeaturesError::InvalidCrs("EPSG:9999".to_string());
        assert!(e.to_string().contains("Invalid CRS"));
    }

    #[test]
    fn test_error_limit_exceeded() {
        let e = FeaturesError::LimitExceeded {
            requested: 20_000,
            max: MAX_LIMIT,
        };
        assert!(e.to_string().contains("20000"));
    }

    #[test]
    fn test_error_serde_from() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json");
        assert!(json_err.is_err());
        let json_err = json_err.expect_err("should fail");
        let e: FeaturesError = json_err.into();
        assert!(matches!(e, FeaturesError::SerdeError(_)));
    }
}
