//! Tests for coordinate-precision truncation in `GeoJsonParser`.

#![allow(clippy::panic)] // test module — panics are acceptable assertions

use oxigeo_geojson_stream::{GeoJsonDocument, GeoJsonGeometry, GeoJsonParser};

// ─── Helper ─────────────────────────────────────────────────────────────────

fn parser_with_precision(decimals: u8) -> GeoJsonParser {
    GeoJsonParser::new().with_coordinate_precision(decimals)
}

// ─── 1. Default (no truncation) preserves full f64 precision ────────────────

#[test]
fn test_parse_default_no_truncation_preserves_15_digits() {
    // 1.234567890123456 has 15 significant decimal digits — must not be changed
    let json = br#"{"type":"Point","coordinates":[1.234567890123456,9.876543210987654]}"#;
    let parser = GeoJsonParser::new(); // no precision set
    let doc = parser.parse(json).expect("valid GeoJSON");
    let geom = match doc {
        GeoJsonDocument::Geometry(g) => g,
        _ => unreachable!("expected Geometry document"),
    };
    if let GeoJsonGeometry::Point([x, y]) = geom {
        assert_eq!(x, 1.234_567_890_123_456_f64);
        assert_eq!(y, 9.876_543_210_987_654_f64);
    } else {
        unreachable!("expected Point geometry");
    }
}

// ─── 2. Precision 6 truncates to micro-degree resolution ────────────────────

#[test]
fn test_parse_precision_6_truncates_to_micro_degrees() {
    // 13.400000001 rounded to 6 decimals is 13.4
    let json = br#"{"type":"Point","coordinates":[13.400000001,52.500000009]}"#;
    let doc = parser_with_precision(6).parse(json).expect("valid");
    let geom = match doc {
        GeoJsonDocument::Geometry(g) => g,
        _ => unreachable!("expected Geometry document"),
    };
    if let GeoJsonGeometry::Point([x, y]) = geom {
        assert_eq!(x, 13.4_f64);
        assert_eq!(y, 52.5_f64);
    } else {
        unreachable!("expected Point geometry");
    }
}

// ─── 3. Precision 0 truncates to integer (rounds half-up per f64::round) ────

#[test]
fn test_parse_precision_0_truncates_to_integer() {
    // 1.9 rounds to 2.0, -0.4 rounds to -0.0 == 0.0
    let json = br#"{"type":"Point","coordinates":[1.9,-0.4]}"#;
    let doc = parser_with_precision(0).parse(json).expect("valid");
    let geom = match doc {
        GeoJsonDocument::Geometry(g) => g,
        _ => unreachable!("expected Geometry document"),
    };
    if let GeoJsonGeometry::Point([x, y]) = geom {
        assert_eq!(x, 2.0_f64);
        assert_eq!(y, 0.0_f64);
    } else {
        unreachable!("expected Point geometry");
    }
}

// ─── 4. Precision applies to polygon rings (exterior + hole) ─────────────────

#[test]
fn test_parse_precision_applies_to_polygon_holes() {
    // Exterior ring and a hole — all coords truncated to 2 decimal places.
    // 0.001 → 0.0, 10.009 → 10.01, 2.005 → ~2.0 or 2.01, 4.004 → 4.0
    let json = br#"{
        "type": "Polygon",
        "coordinates": [
            [[0.001,0.001],[10.009,0.001],[10.009,10.009],[0.001,10.009],[0.001,0.001]],
            [[2.005,2.005],[4.004,2.005],[4.004,4.004],[2.005,4.004],[2.005,2.005]]
        ]
    }"#;
    let doc = parser_with_precision(2).parse(json).expect("valid");
    let geom = match doc {
        GeoJsonDocument::Geometry(g) => g,
        _ => unreachable!("expected Geometry document"),
    };
    if let GeoJsonGeometry::Polygon(rings) = geom {
        assert_eq!(rings.len(), 2, "exterior + 1 hole");
        // Exterior: 0.001 → 0.0, 10.009 → 10.01
        assert_eq!(rings[0][0], [0.0_f64, 0.0_f64]);
        assert_eq!(rings[0][1], [10.01_f64, 0.0_f64]);
        // Hole: 4.004 → 4.0
        assert_eq!(rings[1][2][0], 4.0_f64);
        assert_eq!(rings[1][2][1], 4.0_f64);
    } else {
        unreachable!("expected Polygon geometry");
    }
}

// ─── 5. Precision applies to MultiPolygon ───────────────────────────────────

#[test]
fn test_parse_precision_applies_to_multipolygon() {
    let json = br#"{
        "type": "MultiPolygon",
        "coordinates": [
            [[[0.12345,0.98765],[1.12345,0.98765],[1.12345,1.98765],[0.12345,1.98765],[0.12345,0.98765]]],
            [[[5.55555,5.55555],[6.55555,5.55555],[6.55555,6.55555],[5.55555,6.55555],[5.55555,5.55555]]]
        ]
    }"#;
    let doc = parser_with_precision(4).parse(json).expect("valid");
    let geom = match doc {
        GeoJsonDocument::Geometry(g) => g,
        _ => unreachable!("expected Geometry document"),
    };
    if let GeoJsonGeometry::MultiPolygon(polys) = geom {
        assert_eq!(polys.len(), 2);
        // 0.12345 at precision 4 → 0.1235
        assert_eq!(polys[0][0][0], [0.1235_f64, 0.9877_f64]);
        // 5.55555 at precision 4 → 5.5556
        assert_eq!(polys[1][0][0], [5.5556_f64, 5.5556_f64]);
    } else {
        unreachable!("expected MultiPolygon geometry");
    }
}

// ─── 6. Non-finite guard: precision set, finite coords do not panic ──────────

#[test]
fn test_parse_precision_preserves_nan_and_inf_passthrough() {
    // NaN/Inf are not valid JSON numbers, so we cannot test them end-to-end
    // through the parser.  Instead we verify that:
    //   a) parsing with a precision set on finite coords works correctly, and
    //   b) parsing with the maximum precision (15) does not panic.
    let json = br#"{"type":"Point","coordinates":[0.123456789,0.987654321]}"#;
    let doc = parser_with_precision(5).parse(json).expect("valid");
    let geom = match doc {
        GeoJsonDocument::Geometry(g) => g,
        _ => unreachable!("expected Geometry document"),
    };
    if let GeoJsonGeometry::Point([x, y]) = geom {
        assert!((x - 0.12346_f64).abs() < 1e-9, "x={x}");
        assert!((y - 0.98765_f64).abs() < 1e-9, "y={y}");
    } else {
        unreachable!("expected Point geometry");
    }

    // Maximum precision (15) must not panic
    let json_big = br#"{"type":"Point","coordinates":[1.0,2.0]}"#;
    parser_with_precision(15)
        .parse(json_big)
        .expect("precision=15 must not panic");
}

// ─── 7. Z coordinates are truncated too ─────────────────────────────────────

#[test]
fn test_parse_precision_z_coordinates_truncated_too() {
    // PointZ: x, y, z all truncated to 3 decimal places.
    // 13.123456 → 13.123, 52.654321 → 52.654, 100.9876543 → 100.988
    let json = br#"{"type":"Point","coordinates":[13.123456,52.654321,100.9876543]}"#;
    let doc = parser_with_precision(3).parse(json).expect("valid");
    let geom = match doc {
        GeoJsonDocument::Geometry(g) => g,
        _ => unreachable!("expected Geometry document"),
    };
    if let GeoJsonGeometry::PointZ([x, y, z]) = geom {
        assert_eq!(x, 13.123_f64);
        assert_eq!(y, 52.654_f64);
        assert_eq!(z, 100.988_f64);
    } else {
        unreachable!("expected PointZ geometry");
    }
}

// ─── 8. GeometryCollection recurses and truncates all member coords ──────────

#[test]
fn test_parse_precision_geometrycollection_recurses_into_members() {
    let json = br#"{
        "type": "GeometryCollection",
        "geometries": [
            {"type": "Point", "coordinates": [10.123456, 20.654321]},
            {"type": "LineString", "coordinates": [
                [1.111111, 2.222222],
                [3.333333, 4.444444]
            ]}
        ]
    }"#;
    let doc = parser_with_precision(2).parse(json).expect("valid");
    let geom = match doc {
        GeoJsonDocument::Geometry(g) => g,
        _ => unreachable!("expected Geometry document"),
    };
    if let GeoJsonGeometry::GeometryCollection(members) = geom {
        assert_eq!(members.len(), 2);
        // Point: 10.123456 → 10.12, 20.654321 → 20.65
        if let GeoJsonGeometry::Point([x, y]) = members[0] {
            assert_eq!(x, 10.12_f64);
            assert_eq!(y, 20.65_f64);
        } else {
            unreachable!("member[0] expected Point");
        }
        // LineString: 1.111111→1.11, 2.222222→2.22, 3.333333→3.33, 4.444444→4.44
        if let GeoJsonGeometry::LineString(ref pts) = members[1] {
            assert_eq!(pts[0], [1.11_f64, 2.22_f64]);
            assert_eq!(pts[1], [3.33_f64, 4.44_f64]);
        } else {
            unreachable!("member[1] expected LineString");
        }
    } else {
        unreachable!("expected GeometryCollection");
    }
}
