//! Integration tests for the `reproject` feature of `oxigdal-geojson-stream`.

#![allow(clippy::expect_used, clippy::panic)]

#[cfg(feature = "reproject")]
mod reproject_tests {
    use oxigdal_geojson_stream::parser::{FeatureCollection, GeoJsonParser};
    use oxigdal_geojson_stream::{
        GeoJsonCrs, GeoJsonDocument, GeoJsonFeature, GeoJsonGeometry, ReprojectOptions,
        Reprojector, extract_crs_from_geojson_value, parse_feature_collection_with_reprojection,
        write_feature_collection_with_reprojection,
    };

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn approx_eq(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    fn make_reproj(src: &str, dst: &str) -> Reprojector {
        Reprojector::new(ReprojectOptions {
            source_crs: src.to_string(),
            target_crs: dst.to_string(),
            ..Default::default()
        })
        .expect("reprojector ok")
    }

    fn wgs84_to_mercator() -> Reprojector {
        make_reproj("EPSG:4326", "EPSG:3857")
    }

    fn mercator_to_wgs84() -> Reprojector {
        make_reproj("EPSG:3857", "EPSG:4326")
    }

    // ── Test 1 ───────────────────────────────────────────────────────────────

    /// Default options have source == target == EPSG:4326; coords should be
    /// returned unchanged.
    #[test]
    fn test_reproject_options_default_is_identity() {
        let opts = ReprojectOptions::default();
        assert_eq!(opts.source_crs, "EPSG:4326");
        assert_eq!(opts.target_crs, "EPSG:4326");

        let r = Reprojector::new(opts).expect("ok");
        let out = r.reproject_2d([42.0, 13.0]).expect("ok");
        assert_eq!(out, [42.0, 13.0]);
    }

    // ── Test 2 ───────────────────────────────────────────────────────────────

    /// The geographic origin [0, 0] maps to [0, 0] in Web Mercator.
    #[test]
    fn test_reproject_position_wgs84_to_web_mercator_z0_origin() {
        let r = wgs84_to_mercator();
        let [x, y] = r.reproject_2d([0.0, 0.0]).expect("ok");
        assert!(approx_eq(x, 0.0, 1.0), "x={x}");
        assert!(approx_eq(y, 0.0, 1.0), "y={y}");
    }

    // ── Test 3 ───────────────────────────────────────────────────────────────

    /// The Z component of a 3-D position must pass through unchanged.
    #[test]
    fn test_reproject_position_preserves_z_coordinate() {
        let r = wgs84_to_mercator();
        let [_, _, z] = r.reproject_3d([10.0, 20.0, 100.0]).expect("ok");
        assert_eq!(z, 100.0);
    }

    // ── Test 4 ───────────────────────────────────────────────────────────────

    /// Round-trip: WGS 84 → Web Mercator → WGS 84 must restore original coords.
    #[test]
    fn test_reproject_geometry_point_roundtrip() {
        let orig_lon = 13.4050;
        let orig_lat = 52.5200;

        let mut geom = GeoJsonGeometry::Point([orig_lon, orig_lat]);
        wgs84_to_mercator()
            .reproject_geometry(&mut geom)
            .expect("forward");
        mercator_to_wgs84()
            .reproject_geometry(&mut geom)
            .expect("inverse");

        match geom {
            GeoJsonGeometry::Point([x, y]) => {
                assert!(approx_eq(x, orig_lon, 1e-6), "lon={x}");
                assert!(approx_eq(y, orig_lat, 1e-6), "lat={y}");
            }
            other => unreachable!("expected Point, got {:?}", other.geometry_type()),
        }
    }

    // ── Test 5 ───────────────────────────────────────────────────────────────

    /// Every vertex of a Polygon's exterior ring must be transformed.
    #[test]
    fn test_reproject_geometry_polygon_all_rings_transformed() {
        let ring = vec![
            [0.0_f64, 0.0_f64],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [0.0, 0.0],
        ];
        let original_ring = ring.clone();
        let mut geom = GeoJsonGeometry::Polygon(vec![ring]);

        wgs84_to_mercator()
            .reproject_geometry(&mut geom)
            .expect("ok");

        match geom {
            GeoJsonGeometry::Polygon(rings) => {
                assert_eq!(rings.len(), 1);
                for (orig, transformed) in original_ring.iter().zip(rings[0].iter()) {
                    let changed = !approx_eq(orig[0], transformed[0], 1e-6)
                        || !approx_eq(orig[1], transformed[1], 1e-6);
                    if orig[0].abs() > 0.5 || orig[1].abs() > 0.5 {
                        assert!(changed, "vertex should be transformed: orig={orig:?}");
                    }
                }
            }
            other => unreachable!("expected Polygon, got {}", other.geometry_type()),
        }
    }

    // ── Test 6 ───────────────────────────────────────────────────────────────

    /// MultiPolygon: every polygon's vertices must be transformed.
    #[test]
    fn test_reproject_geometry_multipolygon() {
        let poly1 = vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]];
        let poly2 = vec![vec![[2.0, 2.0], [3.0, 2.0], [3.0, 3.0], [2.0, 2.0]]];
        let mut geom = GeoJsonGeometry::MultiPolygon(vec![poly1, poly2]);

        wgs84_to_mercator()
            .reproject_geometry(&mut geom)
            .expect("ok");

        match geom {
            GeoJsonGeometry::MultiPolygon(polys) => {
                assert_eq!(polys.len(), 2);
                let [x, y] = polys[1][0][0];
                assert!(x.abs() > 100_000.0, "x should be in metres: {x}");
                assert!(y.abs() > 100_000.0, "y should be in metres: {y}");
            }
            other => unreachable!("expected MultiPolygon, got {}", other.geometry_type()),
        }
    }

    // ── Test 7 ───────────────────────────────────────────────────────────────

    /// GeometryCollection containing a Point and a LineString; both reprojected.
    #[test]
    fn test_reproject_geometry_geometrycollection_recursive() {
        let pt = GeoJsonGeometry::Point([10.0, 20.0]);
        let ls = GeoJsonGeometry::LineString(vec![[0.0, 0.0], [5.0, 5.0]]);
        let mut geom = GeoJsonGeometry::GeometryCollection(vec![pt, ls]);

        wgs84_to_mercator()
            .reproject_geometry(&mut geom)
            .expect("ok");

        match geom {
            GeoJsonGeometry::GeometryCollection(geoms) => {
                assert_eq!(geoms.len(), 2);
                match geoms[0] {
                    GeoJsonGeometry::Point([x, y]) => {
                        assert!(x.abs() > 1_000_000.0, "x={x}");
                        assert!(y.abs() > 1_000_000.0, "y={y}");
                    }
                    ref other => {
                        unreachable!(
                            "expected Point inside collection, got {}",
                            other.geometry_type()
                        )
                    }
                }
                match &geoms[1] {
                    GeoJsonGeometry::LineString(pts) => {
                        assert!(pts[1][0].abs() > 500_000.0);
                    }
                    other => {
                        unreachable!(
                            "expected LineString inside collection, got {}",
                            other.geometry_type()
                        )
                    }
                }
            }
            other => unreachable!("expected GeometryCollection, got {}", other.geometry_type()),
        }
    }

    // ── Test 8 ───────────────────────────────────────────────────────────────

    /// Properties must survive reprojection unchanged.
    #[test]
    fn test_reproject_feature_preserves_properties() {
        let props = serde_json::json!({ "name": "Test", "value": 42 });
        let mut feature = GeoJsonFeature {
            id: None,
            geometry: Some(GeoJsonGeometry::Point([10.0, 20.0])),
            properties: Some(props.clone()),
        };

        wgs84_to_mercator()
            .reproject_feature(&mut feature)
            .expect("ok");

        assert_eq!(feature.properties, Some(props));
        match feature.geometry {
            Some(GeoJsonGeometry::Point([x, y])) => {
                assert!(x.abs() > 1_000_000.0, "x={x}");
                assert!(y.abs() > 1_000_000.0, "y={y}");
            }
            other => unreachable!(
                "expected Point, got {:?}",
                other.as_ref().map(|g| g.geometry_type())
            ),
        }
    }

    // ── Test 9 ───────────────────────────────────────────────────────────────

    /// After reprojecting to EPSG:4326, the `crs` field must be cleared.
    #[test]
    fn test_reproject_feature_collection_clears_crs_member_when_target_wgs84() {
        let mut fc = FeatureCollection {
            features: vec![],
            bbox: None,
            bbox_3d: None,
            crs: Some(GeoJsonCrs::epsg3857()),
            name: None,
        };

        let r = make_reproj("EPSG:3857", "EPSG:4326");
        r.reproject_feature_collection(&mut fc).expect("ok");

        assert!(fc.crs.is_none(), "crs should be cleared for WGS84 target");
    }

    // ── Test 10 ──────────────────────────────────────────────────────────────

    /// Extract a named CRS from a `{"crs":{"type":"name","properties":{"name":"..."}}}` JSON.
    #[test]
    fn test_extract_crs_from_geojson_named_crs() {
        let json_str = r#"{"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"EPSG:3857"}},"features":[]}"#;
        let v: serde_json::Value = serde_json::from_str(json_str).expect("valid json");
        let crs = extract_crs_from_geojson_value(&v);
        assert_eq!(crs, Some("EPSG:3857".to_string()));
    }

    // ── Test 11 ──────────────────────────────────────────────────────────────

    /// Extract a URN-form CRS string unchanged.
    #[test]
    fn test_extract_crs_from_geojson_urn_form() {
        let json_str = r#"{"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::3857"}},"features":[]}"#;
        let v: serde_json::Value = serde_json::from_str(json_str).expect("valid json");
        let crs = extract_crs_from_geojson_value(&v);
        assert_eq!(crs, Some("urn:ogc:def:crs:EPSG::3857".to_string()));
    }

    // ── Test 12 ──────────────────────────────────────────────────────────────

    /// Parse a GeoJSON string declaring EPSG:3857 with a Point at origin;
    /// reprojecting to WGS 84 should give approximately [0, 0].
    #[test]
    fn test_parse_feature_collection_with_reprojection_from_epsg_3857() {
        let json_str = r#"{
            "type":"FeatureCollection",
            "crs":{"type":"name","properties":{"name":"EPSG:3857"}},
            "features":[{
                "type":"Feature",
                "geometry":{"type":"Point","coordinates":[0.0,0.0]},
                "properties":null
            }]
        }"#;

        let fc = parse_feature_collection_with_reprojection(json_str, "EPSG:4326").expect("ok");

        assert_eq!(fc.features.len(), 1);
        match fc.features[0].geometry {
            Some(GeoJsonGeometry::Point([x, y])) => {
                assert!(approx_eq(x, 0.0, 1e-6), "lon={x}");
                assert!(approx_eq(y, 0.0, 1e-6), "lat={y}");
            }
            ref other => unreachable!(
                "expected Point, got {:?}",
                other.as_ref().map(|g| g.geometry_type())
            ),
        }
    }

    // ── Test 13 ──────────────────────────────────────────────────────────────

    /// Write a WGS 84 fc with Point [10.0, 10.0] to EPSG:3857; the output JSON
    /// coordinates must differ from [10.0, 10.0].
    #[test]
    fn test_write_feature_collection_with_reprojection_to_epsg_3857() {
        let json_str = r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[10.0,10.0]},"properties":null}]}"#;
        let fc = match GeoJsonParser::new()
            .parse(json_str.as_bytes())
            .expect("parse ok")
        {
            GeoJsonDocument::FeatureCollection(fc) => fc,
            other => unreachable!("expected fc, got {}", other.document_type()),
        };

        let output = write_feature_collection_with_reprojection(&fc, "EPSG:4326", "EPSG:3857")
            .expect("write ok");

        assert!(
            !output.contains("10.000000,10.000000"),
            "coordinates should be in metres, not degrees: {output}"
        );
        let parsed: serde_json::Value = serde_json::from_str(&output).expect("valid json output");
        assert_eq!(parsed["type"].as_str(), Some("FeatureCollection"));
    }
}
