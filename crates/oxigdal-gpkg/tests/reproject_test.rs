//! Integration tests for [`oxigdal_gpkg::reproject::CrsReprojector`].
//!
//! All tests are gated behind the `reproject` feature so the default build of
//! `oxigdal-gpkg` neither pulls in `oxigdal-proj` nor runs these cases.

#![cfg(feature = "reproject")]
#![allow(clippy::expect_used)]

use std::collections::HashMap;

use oxigdal_gpkg::error::GpkgError;
use oxigdal_gpkg::reproject::CrsReprojector;
use oxigdal_gpkg::vector::feature::{FeatureRow, FeatureTable};
use oxigdal_gpkg::vector::types::{FieldValue, GpkgGeometry, Point4D};

// ─────────────────────────────────────────────────────────────────────────────
// Construction
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_reprojector_construct_4326_to_3857() {
    let r = CrsReprojector::new(4326, 3857).expect("construction should succeed");
    assert_eq!(r.src_epsg(), 4326);
    assert_eq!(r.dst_epsg(), 3857);
}

// ─────────────────────────────────────────────────────────────────────────────
// Point reprojection: known reference value
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_reprojector_point_4326_to_3857_matches_known_value() {
    // The origin in WGS84 lon/lat (0, 0) maps to (0, 0) in EPSG:3857.
    let r = CrsReprojector::new(4326, 3857).expect("construct");
    let origin = GpkgGeometry::Point { x: 0.0, y: 0.0 };
    let out = r.reproject_geometry(&origin).expect("reproject origin");

    assert!(
        matches!(out, GpkgGeometry::Point { .. }),
        "expected Point variant"
    );
    if let GpkgGeometry::Point { x, y } = out {
        assert!(x.abs() < 1.0, "x near 0; got {x}");
        assert!(y.abs() < 1.0, "y near 0; got {y}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Round-trip closure
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_reprojector_round_trip_4326_to_3857_to_4326_within_tolerance() {
    let fwd = CrsReprojector::new(4326, 3857).expect("forward");
    let inv = CrsReprojector::new(3857, 4326).expect("inverse");

    // Pick a point well inside the Web Mercator domain (latitude < 85°).
    let x0 = 12.34_f64;
    let y0 = 56.78_f64;
    let original = GpkgGeometry::Point { x: x0, y: y0 };

    let projected = fwd.reproject_geometry(&original).expect("forward project");
    let back = inv.reproject_geometry(&projected).expect("inverse project");

    assert!(
        matches!(back, GpkgGeometry::Point { .. }),
        "unexpected variant after round-trip"
    );
    if let GpkgGeometry::Point { x: x1, y: y1 } = back {
        let dx = (x1 - x0).abs();
        let dy = (y1 - y0).abs();
        // Round-trip through a metre-based CRS introduces a sub-microdegree
        // residue.  1e-6 degrees ≈ 11 cm at the equator, a comfortable
        // tolerance for the proj4rs implementation.
        assert!(dx < 1e-6, "dx={dx} exceeds 1e-6° tolerance");
        assert!(dy < 1e-6, "dy={dy} exceeds 1e-6° tolerance");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LineString vertex-count preservation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_reprojector_linestring_preserves_vertex_count() {
    let r = CrsReprojector::new(4326, 3857).expect("construct");
    let input_coords = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0), (4.0, 4.0)];
    let original_n = input_coords.len();
    let line = GpkgGeometry::LineString {
        coords: input_coords,
    };
    let out = r.reproject_geometry(&line).expect("reproject line");
    assert!(
        matches!(out, GpkgGeometry::LineString { .. }),
        "expected LineString variant"
    );
    if let GpkgGeometry::LineString { coords } = out {
        assert_eq!(coords.len(), original_n, "vertex count must be preserved");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Polygon with an interior ring (hole)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_reprojector_polygon_with_hole_reprojects_both_rings() {
    let r = CrsReprojector::new(4326, 3857).expect("construct");
    // Exterior: 5×5 square; hole: 1×1 square in the middle.  Five vertices
    // each (closed ring).
    let exterior = vec![(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0), (0.0, 0.0)];
    let hole = vec![(2.0, 2.0), (3.0, 2.0), (3.0, 3.0), (2.0, 3.0), (2.0, 2.0)];
    let exterior_n = exterior.len();
    let hole_n = hole.len();
    let poly = GpkgGeometry::Polygon {
        rings: vec![exterior, hole],
    };

    let out = r.reproject_geometry(&poly).expect("reproject polygon");
    assert!(
        matches!(out, GpkgGeometry::Polygon { .. }),
        "expected Polygon variant"
    );
    if let GpkgGeometry::Polygon { rings } = out {
        assert_eq!(rings.len(), 2, "must keep two rings");
        assert_eq!(rings[0].len(), exterior_n, "exterior count");
        assert_eq!(rings[1].len(), hole_n, "hole count");
        // Web Mercator x for lon=0 must remain near 0.
        assert!(rings[0][0].0.abs() < 1.0, "first vertex x near 0");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MultiPolygon multi-part reprojection
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_reprojector_multipolygon_each_part_reprojected() {
    let r = CrsReprojector::new(4326, 3857).expect("construct");
    let poly_a = vec![vec![
        (0.0, 0.0),
        (1.0, 0.0),
        (1.0, 1.0),
        (0.0, 1.0),
        (0.0, 0.0),
    ]];
    let poly_b = vec![vec![
        (10.0, 10.0),
        (11.0, 10.0),
        (11.0, 11.0),
        (10.0, 11.0),
        (10.0, 10.0),
    ]];
    let poly_a_vertex_count = poly_a[0].len();
    let poly_b_vertex_count = poly_b[0].len();
    let mp = GpkgGeometry::MultiPolygon {
        polygons: vec![poly_a, poly_b],
    };

    let out = r.reproject_geometry(&mp).expect("reproject multipolygon");
    assert!(
        matches!(out, GpkgGeometry::MultiPolygon { .. }),
        "expected MultiPolygon variant"
    );
    if let GpkgGeometry::MultiPolygon { polygons } = out {
        assert_eq!(polygons.len(), 2, "expected 2 parts");
        assert_eq!(polygons[0].len(), 1, "part A rings");
        assert_eq!(polygons[1].len(), 1, "part B rings");
        assert_eq!(
            polygons[0][0].len(),
            poly_a_vertex_count,
            "part A vertex count"
        );
        assert_eq!(
            polygons[1][0].len(),
            poly_b_vertex_count,
            "part B vertex count"
        );
        // Part B vertices must end up at a strictly larger easting than
        // part A vertices because lon=10° projects east of lon=0°.
        assert!(
            polygons[1][0][0].0 > polygons[0][0][0].0,
            "part B should lie east of part A in Mercator"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Z preservation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_reprojector_z_coords_preserved() {
    let r = CrsReprojector::new(4326, 3857).expect("construct");
    let pz = GpkgGeometry::PointZ {
        x: 1.0,
        y: 2.0,
        z: 100.0,
    };
    let out = r.reproject_geometry(&pz).expect("reproject PointZ");
    assert!(
        matches!(out, GpkgGeometry::PointZ { .. }),
        "expected PointZ variant"
    );
    if let GpkgGeometry::PointZ { z, .. } = out {
        assert!(
            (z - 100.0).abs() < 1e-12,
            "z must pass through unchanged; got {z}"
        );
    }

    // Also verify a Point4D (PointZM) keeps both Z and M.
    let pzm = GpkgGeometry::PointZM(Point4D {
        x: 3.0,
        y: 4.0,
        z: Some(42.0),
        m: Some(7.0),
    });
    let out2 = r.reproject_geometry(&pzm).expect("reproject PointZM");
    assert!(
        matches!(out2, GpkgGeometry::PointZM(_)),
        "expected PointZM variant"
    );
    if let GpkgGeometry::PointZM(p) = out2 {
        assert_eq!(p.z, Some(42.0), "Z must be preserved");
        assert_eq!(p.m, Some(7.0), "M must be preserved");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unknown EPSG → error
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_reprojector_unknown_epsg_returns_error() {
    let result = CrsReprojector::new(99999, 4326);
    assert!(
        result.is_err(),
        "EPSG 99999 must not resolve and must yield Err"
    );
    if let Err(err) = result {
        assert!(
            matches!(err, GpkgError::ReprojectionError(_)),
            "expected ReprojectionError variant, got {err:?}"
        );
        if let GpkgError::ReprojectionError(msg) = err {
            assert!(
                !msg.is_empty(),
                "ReprojectionError must carry a non-empty message"
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FeatureTable: SRS metadata updated
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_reproject_feature_table_updates_srs_metadata() {
    let mut table = FeatureTable::new("places", "geom");
    table.srs_id = Some(4326);

    // Two features with valid 4326 points.
    let mut fields_a: HashMap<String, FieldValue> = HashMap::new();
    fields_a.insert("name".into(), FieldValue::Text("origin".into()));
    table.add_feature(FeatureRow {
        fid: 1,
        geometry: Some(GpkgGeometry::Point { x: 0.0, y: 0.0 }),
        fields: fields_a,
    });

    let mut fields_b: HashMap<String, FieldValue> = HashMap::new();
    fields_b.insert("name".into(), FieldValue::Text("offset".into()));
    table.add_feature(FeatureRow {
        fid: 2,
        geometry: Some(GpkgGeometry::Point { x: 1.0, y: 1.0 }),
        fields: fields_b,
    });

    // A NULL-geometry feature must survive untouched.
    table.add_feature(FeatureRow {
        fid: 3,
        geometry: None,
        fields: HashMap::new(),
    });

    let r = CrsReprojector::new(4326, 3857).expect("construct");
    let out_table = r.reproject_feature_table(&table).expect("reproject table");

    assert_eq!(out_table.srs_id, Some(3857), "srs_id must be dst_epsg");
    assert_eq!(out_table.features.len(), table.features.len());
    assert_eq!(out_table.name, "places");
    assert_eq!(out_table.geometry_column, "geom");
    // FID 3 had no geometry — must remain None.
    assert!(
        out_table.features[2].geometry.is_none(),
        "NULL geometry must remain NULL"
    );
    // FID 1 (origin) must still be near (0, 0) in Web Mercator.
    assert!(
        matches!(
            out_table.features[0].geometry,
            Some(GpkgGeometry::Point { .. })
        ),
        "expected Point geometry at FID 1"
    );
    if let Some(GpkgGeometry::Point { x, y }) = out_table.features[0].geometry {
        assert!(
            x.abs() < 1.0 && y.abs() < 1.0,
            "origin should map near (0,0)"
        );
    }
}
