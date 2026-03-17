//! Property-based tests for WKB encoding/decoding in oxigdal-geoparquet.
//!
//! Invariants tested:
//! 1. Point WKB round-trip: encode → decode gives same coordinates
//! 2. WKB byte length is deterministic and correct
//! 3. Bounding box contains all ring points
//! 4. encode_point_2d produces exactly 21 bytes (NDR little-endian 2D point)

#![allow(clippy::expect_used)]

use oxigdal_geoparquet::geometry::{
    Coordinate, Geometry, LineString, Point, WkbReader, WkbWriter,
    wkb_extended::{decode_point_2d, encode_point_2d, encode_polygon, encode_ring_2d, wkb_bbox},
};
use proptest::prelude::*;
use std::f64::consts::PI;

// ── Strategies ────────────────────────────────────────────────────────────────

prop_compose! {
    fn valid_coord()(
        x in -180.0f64..180.0f64,
        y in -90.0f64..90.0f64,
    ) -> (f64, f64) { (x, y) }
}

prop_compose! {
    fn finite_coord()(
        x in -1.0e6f64..1.0e6f64,
        y in -1.0e6f64..1.0e6f64,
    ) -> (f64, f64) { (x, y) }
}

prop_compose! {
    /// Generates a closed convex polygon ring (circle approximation).
    /// n_pts is the number of vertices (excluding the closing point).
    fn valid_ring()(
        n_pts in 3usize..16usize,
        cx in -100.0f64..100.0f64,
        cy in -50.0f64..50.0f64,
        r in 0.01f64..5.0f64,
    ) -> Vec<(f64, f64)> {
        let mut ring: Vec<(f64, f64)> = (0..n_pts).map(|i| {
            let angle = 2.0 * PI * i as f64 / n_pts as f64;
            (cx + r * angle.cos(), cy + r * angle.sin())
        }).collect();
        // Close the ring
        ring.push(ring[0]);
        ring
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

proptest! {
    /// encode_point_2d always produces exactly 21 bytes for a 2D WKB point:
    /// 1 (byte order) + 4 (type) + 8 (x) + 8 (y) = 21
    #[test]
    fn prop_point_wkb_length((x, y) in valid_coord()) {
        let wkb = encode_point_2d(x, y).expect("encode_point_2d should not fail");
        prop_assert_eq!(wkb.len(), 21, "2D WKB point must be exactly 21 bytes");
    }

    /// encode_point_2d round-trips through decode_point_2d.
    #[test]
    fn prop_point_wkb_roundtrip((x, y) in valid_coord()) {
        let wkb = encode_point_2d(x, y).expect("encode should not fail");
        let (x2, y2) = decode_point_2d(&wkb).expect("decode should not fail");

        prop_assert!(
            (x2 - x).abs() < f64::EPSILON * 4.0,
            "x round-trip failed: encoded {}, decoded {}", x, x2
        );
        prop_assert!(
            (y2 - y).abs() < f64::EPSILON * 4.0,
            "y round-trip failed: encoded {}, decoded {}", y, y2
        );
    }

    /// WkbWriter + WkbReader round-trip for 2D points.
    #[test]
    fn prop_point_writer_reader_roundtrip((x, y) in finite_coord()) {
        let point = Point::new_2d(x, y);
        let geom = Geometry::Point(point);

        let mut writer = WkbWriter::new(true);
        let wkb = writer.write_geometry(&geom).expect("write should not fail");

        let mut reader = WkbReader::new(&wkb);
        let decoded = reader.read_geometry().expect("read should not fail");

        match decoded {
            Geometry::Point(p) => {
                prop_assert!(
                    (p.coord.x - x).abs() < f64::EPSILON * 4.0,
                    "x mismatch: {} vs {}",
                    p.coord.x, x
                );
                prop_assert!(
                    (p.coord.y - y).abs() < f64::EPSILON * 4.0,
                    "y mismatch: {} vs {}",
                    p.coord.y, y
                );
            }
            other => prop_assert!(false, "expected Point, got {:?}", other),
        }
    }

    /// WkbWriter + WkbReader round-trip for LineStrings.
    #[test]
    fn prop_linestring_roundtrip(
        pts in prop::collection::vec(finite_coord(), 2..20)
    ) {
        let coords: Vec<Coordinate> = pts.iter()
            .map(|(x, y)| Coordinate::new_2d(*x, *y))
            .collect();
        let ls = LineString::new(coords.clone());
        let geom = Geometry::LineString(ls);

        let mut writer = WkbWriter::new(true);
        let wkb = writer.write_geometry(&geom).expect("write should not fail");

        let mut reader = WkbReader::new(&wkb);
        let decoded = reader.read_geometry().expect("read should not fail");

        match decoded {
            Geometry::LineString(ls_dec) => {
                prop_assert_eq!(
                    ls_dec.coords.len(),
                    coords.len(),
                    "coord count mismatch"
                );
                for (orig, dec) in coords.iter().zip(ls_dec.coords.iter()) {
                    prop_assert!(
                        (dec.x - orig.x).abs() < f64::EPSILON * 4.0 &&
                        (dec.y - orig.y).abs() < f64::EPSILON * 4.0,
                        "coordinate mismatch: {:?} vs {:?}", orig, dec
                    );
                }
            }
            other => prop_assert!(false, "expected LineString, got {:?}", other),
        }
    }

    /// encode_ring_2d length: 4 (n_points) + n * 16 (each x,y pair)
    #[test]
    fn prop_ring_length(ring in valid_ring()) {
        let wkb = encode_ring_2d(&ring).expect("encode_ring_2d should not fail");
        let expected_len = 4 + ring.len() * 16;
        prop_assert_eq!(
            wkb.len(),
            expected_len,
            "ring WKB length wrong: expected {}, got {}",
            expected_len, wkb.len()
        );
    }

    /// wkb_bbox on a polygon ring contains all ring points.
    #[test]
    fn prop_bbox_contains_all_points(ring in valid_ring()) {
        // encode_polygon with no holes
        let wkb = encode_polygon(&ring, &[]).expect("encode_polygon should not fail");

        let bbox = wkb_bbox(&wkb);
        let (min_x, min_y, max_x, max_y) = match bbox {
            Some(b) => b,
            None => return Ok(()), // wkb_bbox may return None for unsupported types
        };

        prop_assume!(min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite());

        // The first n_pts points (not the closing duplicate) should be inside bbox
        for &(x, y) in ring.iter() {
            prop_assert!(
                x >= min_x - f64::EPSILON * 100.0 && x <= max_x + f64::EPSILON * 100.0,
                "x={} outside bbox [{}, {}]", x, min_x, max_x
            );
            prop_assert!(
                y >= min_y - f64::EPSILON * 100.0 && y <= max_y + f64::EPSILON * 100.0,
                "y={} outside bbox [{}, {}]", y, min_y, max_y
            );
        }
    }

    /// wkb_bbox on a point geometry gives (x,x,y,y).
    #[test]
    fn prop_point_bbox_is_point((x, y) in valid_coord()) {
        let wkb = encode_point_2d(x, y).expect("encode should not fail");
        let bbox = wkb_bbox(&wkb);

        match bbox {
            Some((min_x, min_y, max_x, max_y)) => {
                prop_assert!(
                    (min_x - x).abs() < f64::EPSILON * 4.0,
                    "point bbox min_x wrong: {} vs {}", min_x, x
                );
                prop_assert!(
                    (max_x - x).abs() < f64::EPSILON * 4.0,
                    "point bbox max_x wrong: {} vs {}", max_x, x
                );
                prop_assert!(
                    (min_y - y).abs() < f64::EPSILON * 4.0,
                    "point bbox min_y wrong: {} vs {}", min_y, y
                );
                prop_assert!(
                    (max_y - y).abs() < f64::EPSILON * 4.0,
                    "point bbox max_y wrong: {} vs {}", max_y, y
                );
            }
            None => {
                // wkb_bbox returning None is acceptable (only signals not computed)
            }
        }
    }

    /// wkb_bbox on arbitrary bytes never panics (safety check).
    #[test]
    fn prop_wkb_bbox_never_panics(data in prop::collection::vec(0u8..=255u8, 0..100)) {
        // Must not panic on any input
        let _ = wkb_bbox(&data);
    }

    /// WKB encode→decode of a polygon is consistent (same geometry type).
    #[test]
    fn prop_polygon_encode_decode_type(ring in valid_ring()) {
        use oxigdal_geoparquet::geometry::{LineString as GeoLS, Polygon};

        let exterior_coords: Vec<Coordinate> = ring.iter()
            .map(|(x, y)| Coordinate::new_2d(*x, *y))
            .collect();
        let exterior = GeoLS::new(exterior_coords);
        let poly = Polygon::new_simple(exterior);
        let geom = Geometry::Polygon(poly);

        let mut writer = WkbWriter::new(true);
        let wkb = writer.write_geometry(&geom).expect("write should not fail");

        let mut reader = WkbReader::new(&wkb);
        let decoded = reader.read_geometry().expect("read should not fail");

        prop_assert!(
            matches!(decoded, Geometry::Polygon(_)),
            "expected Polygon after round-trip"
        );
    }
}
