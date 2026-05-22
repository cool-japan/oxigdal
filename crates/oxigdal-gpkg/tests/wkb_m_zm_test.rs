//! Integration tests for WKB M and ZM coordinate variant parsing and encoding.
//!
//! Covers ISO SQL/MM `2000 + base` (XYM) and `3000 + base` (XYZM) type codes
//! as well as the EWKB high-bit conventions used by PostGIS and other producers.

use oxigdal_gpkg::{
    CoordDim, GpkgBinaryParser, GpkgGeometry, Point4D, WKB_EWKB_M_HIGHBIT, WKB_EWKB_ZM_HIGHBIT,
    WKB_POINT_M, WKB_POINT_ZM,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helper: build a minimal little-endian WKB blob from type + raw f64 values.
// ─────────────────────────────────────────────────────────────────────────────

/// Construct a minimal LE WKB blob: byte-order marker (0x01), 4-byte LE type,
/// then the given coordinate f64 values packed as little-endian 8-byte words.
fn make_wkb_le(wkb_type: u32, coords: &[f64]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(5 + coords.len() * 8);
    buf.push(0x01); // little-endian marker
    buf.extend_from_slice(&wkb_type.to_le_bytes());
    for c in coords {
        buf.extend_from_slice(&c.to_le_bytes());
    }
    buf
}

/// Build a LE WKB blob for a LineString with an explicit point-count prefix,
/// followed by `n_pts × coords_per_pt` f64 values.
fn make_linestring_wkb_le(wkb_type: u32, points: &[Vec<f64>]) -> Vec<u8> {
    let n = points.len() as u32;
    let coords_per_pt = if !points.is_empty() {
        points[0].len()
    } else {
        2
    };
    let mut buf = Vec::with_capacity(5 + 4 + n as usize * coords_per_pt * 8);
    buf.push(0x01);
    buf.extend_from_slice(&wkb_type.to_le_bytes());
    buf.extend_from_slice(&n.to_le_bytes());
    for pt in points {
        for c in pt {
            buf.extend_from_slice(&c.to_le_bytes());
        }
    }
    buf
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — PointM round-trip via ISO type 2001
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wkb_point_m_round_trip() {
    let original = GpkgGeometry::PointM {
        x: 1.0,
        y: 2.0,
        m: 42.0,
    };

    // Encode using the writer.
    let wkb = GpkgBinaryParser::to_wkb(&original);

    // Verify the type code is WKB_POINT_M (2001) in little-endian position bytes 1..5.
    let type_code = u32::from_le_bytes([wkb[1], wkb[2], wkb[3], wkb[4]]);
    assert_eq!(
        type_code, WKB_POINT_M,
        "type code must be WKB_POINT_M = 2001"
    );

    // Round-trip decode: PartialEq on GpkgGeometry handles the comparison.
    let decoded = GpkgBinaryParser::parse_wkb(&wkb).expect("parse_wkb PointM");
    assert_eq!(
        decoded, original,
        "round-trip must reproduce PointM exactly"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — PointZM round-trip via ISO type 3001
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wkb_point_zm_round_trip() {
    let original = GpkgGeometry::PointZM(Point4D {
        x: 1.0,
        y: 2.0,
        z: Some(3.0),
        m: Some(4.0),
    });

    let wkb = GpkgBinaryParser::to_wkb(&original);

    let type_code = u32::from_le_bytes([wkb[1], wkb[2], wkb[3], wkb[4]]);
    assert_eq!(
        type_code, WKB_POINT_ZM,
        "type code must be WKB_POINT_ZM = 3001"
    );

    let decoded = GpkgBinaryParser::parse_wkb(&wkb).expect("parse_wkb PointZM");
    assert_eq!(
        decoded, original,
        "round-trip must reproduce PointZM exactly"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — LineStringM: 3 vertices each carrying (x, y, m)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wkb_linestring_m_parses_three_coords_per_vertex() {
    // Three vertices: (0, 0, 10), (1, 1, 20), (2, 2, 30)
    let pts: Vec<Vec<f64>> = vec![
        vec![0.0, 0.0, 10.0],
        vec![1.0, 1.0, 20.0],
        vec![2.0, 2.0, 30.0],
    ];
    // ISO type 2002 = WKB_LINESTRING_M
    let wkb = make_linestring_wkb_le(2002, &pts);

    let decoded = GpkgBinaryParser::parse_wkb(&wkb).expect("parse_wkb LineStringM");
    let expected = GpkgGeometry::LineStringM {
        coords: vec![(0.0, 0.0, 10.0), (1.0, 1.0, 20.0), (2.0, 2.0, 30.0)],
    };
    assert_eq!(
        decoded, expected,
        "LineStringM must decode 9 f64 values correctly"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — PolygonZM round-trip with outer ring + 1 hole, each vertex x/y/z/m
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wkb_polygon_zm_round_trip_with_hole() {
    // Outer ring: 5 vertices (closed square)
    let outer: Vec<Point4D> = vec![
        Point4D {
            x: 0.0,
            y: 0.0,
            z: Some(1.0),
            m: Some(0.1),
        },
        Point4D {
            x: 4.0,
            y: 0.0,
            z: Some(1.0),
            m: Some(0.2),
        },
        Point4D {
            x: 4.0,
            y: 4.0,
            z: Some(1.0),
            m: Some(0.3),
        },
        Point4D {
            x: 0.0,
            y: 4.0,
            z: Some(1.0),
            m: Some(0.4),
        },
        Point4D {
            x: 0.0,
            y: 0.0,
            z: Some(1.0),
            m: Some(0.1),
        }, // close
    ];
    // Inner ring (hole): 5 vertices
    let inner: Vec<Point4D> = vec![
        Point4D {
            x: 1.0,
            y: 1.0,
            z: Some(2.0),
            m: Some(0.5),
        },
        Point4D {
            x: 3.0,
            y: 1.0,
            z: Some(2.0),
            m: Some(0.6),
        },
        Point4D {
            x: 3.0,
            y: 3.0,
            z: Some(2.0),
            m: Some(0.7),
        },
        Point4D {
            x: 1.0,
            y: 3.0,
            z: Some(2.0),
            m: Some(0.8),
        },
        Point4D {
            x: 1.0,
            y: 1.0,
            z: Some(2.0),
            m: Some(0.5),
        }, // close
    ];

    let original = GpkgGeometry::PolygonZM {
        rings: vec![outer, inner],
    };

    let wkb = GpkgBinaryParser::to_wkb(&original);
    let decoded = GpkgBinaryParser::parse_wkb(&wkb).expect("parse_wkb PolygonZM");
    assert_eq!(
        decoded, original,
        "PolygonZM with hole must round-trip exactly"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — EWKB M high-bit (0x40000001) parses as PointM
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wkb_ewkb_m_highbit_flag_parses_as_m() {
    // WKB Point type with EWKB M high-bit set: 1 | 0x40000000 = 0x40000001
    let ewkb_point_m_type: u32 = 1 | WKB_EWKB_M_HIGHBIT;
    // Coordinates: x=7.0, y=8.0, m=99.0
    let wkb = make_wkb_le(ewkb_point_m_type, &[7.0, 8.0, 99.0]);

    let decoded = GpkgBinaryParser::parse_wkb(&wkb).expect("parse_wkb EWKB PointM");
    let expected = GpkgGeometry::PointM {
        x: 7.0,
        y: 8.0,
        m: 99.0,
    };
    assert_eq!(decoded, expected, "EWKB M high-bit must produce PointM");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — EWKB ZM combined high-bits (0xC0000001) parses as PointZM
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wkb_ewkb_zm_highbit_flag_parses_as_zm() {
    // WKB Point type with both EWKB Z and M high-bits: 1 | 0xC0000000
    let ewkb_point_zm_type: u32 = 1 | WKB_EWKB_ZM_HIGHBIT;
    // Coordinates: x=10.0, y=20.0, z=30.0, m=40.0
    let wkb = make_wkb_le(ewkb_point_zm_type, &[10.0, 20.0, 30.0, 40.0]);

    let decoded = GpkgBinaryParser::parse_wkb(&wkb).expect("parse_wkb EWKB PointZM");
    let expected = GpkgGeometry::PointZM(Point4D {
        x: 10.0,
        y: 20.0,
        z: Some(30.0),
        m: Some(40.0),
    });
    assert_eq!(decoded, expected, "EWKB ZM high-bits must produce PointZM");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — ISO 3000-offset type code 3001 (WKB_POINT_ZM) parses correctly
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wkb_iso_3000_offset_for_zm_parses_correctly() {
    // Directly supply WKB_POINT_ZM = 3001 as the type code.
    // Coordinates: x=5.5, y=6.6, z=7.7, m=8.8
    let wkb = make_wkb_le(WKB_POINT_ZM, &[5.5, 6.6, 7.7, 8.8]);

    let decoded = GpkgBinaryParser::parse_wkb(&wkb).expect("parse_wkb ISO WKB_POINT_ZM");
    let expected = GpkgGeometry::PointZM(Point4D {
        x: 5.5,
        y: 6.6,
        z: Some(7.7),
        m: Some(8.8),
    });
    assert_eq!(decoded, expected, "ISO type 3001 must produce PointZM");
}

// ─────────────────────────────────────────────────────────────────────────────
// CoordDim enum accessibility and distinctness
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_coord_dim_variants_are_distinct() {
    assert_ne!(CoordDim::XY, CoordDim::XYZ);
    assert_ne!(CoordDim::XYM, CoordDim::XYZM);
    assert_ne!(CoordDim::XYZ, CoordDim::XYZM);
    assert_ne!(CoordDim::XY, CoordDim::XYM);
}

// ─────────────────────────────────────────────────────────────────────────────
// Point4D projection helpers
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_point4d_projection_helpers() {
    let p = Point4D {
        x: 1.0,
        y: 2.0,
        z: Some(3.0),
        m: Some(4.0),
    };
    assert_eq!(p.to_xy(), (1.0, 2.0));
    assert_eq!(p.to_xyz(), (1.0, 2.0, 3.0));
}

#[test]
fn test_point4d_to_xyz_uses_zero_when_z_absent() {
    let p = Point4D {
        x: 5.0,
        y: 6.0,
        z: None,
        m: Some(99.0),
    };
    assert_eq!(p.to_xyz(), (5.0, 6.0, 0.0));
}

// ─────────────────────────────────────────────────────────────────────────────
// geometry_type() string checks for new variants
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_geometry_type_strings_for_m_and_zm_variants() {
    assert_eq!(
        GpkgGeometry::PointM {
            x: 0.0,
            y: 0.0,
            m: 0.0
        }
        .geometry_type(),
        "PointM"
    );
    assert_eq!(
        GpkgGeometry::LineStringM { coords: vec![] }.geometry_type(),
        "LineStringM"
    );
    assert_eq!(
        GpkgGeometry::PolygonM { rings: vec![] }.geometry_type(),
        "PolygonM"
    );
    assert_eq!(
        GpkgGeometry::MultiPointM { points: vec![] }.geometry_type(),
        "MultiPointM"
    );
    assert_eq!(
        GpkgGeometry::MultiLineStringM { lines: vec![] }.geometry_type(),
        "MultiLineStringM"
    );
    assert_eq!(
        GpkgGeometry::MultiPolygonM { polygons: vec![] }.geometry_type(),
        "MultiPolygonM"
    );
    assert_eq!(
        GpkgGeometry::PointZM(Point4D {
            x: 0.0,
            y: 0.0,
            z: None,
            m: None
        })
        .geometry_type(),
        "PointZM"
    );
    assert_eq!(
        GpkgGeometry::LineStringZM { coords: vec![] }.geometry_type(),
        "LineStringZM"
    );
    assert_eq!(
        GpkgGeometry::PolygonZM { rings: vec![] }.geometry_type(),
        "PolygonZM"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// point_count() for M/ZM variants
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_point_count_m_variants() {
    assert_eq!(
        GpkgGeometry::PointM {
            x: 0.0,
            y: 0.0,
            m: 1.0
        }
        .point_count(),
        1
    );
    assert_eq!(
        GpkgGeometry::LineStringM {
            coords: vec![(0.0, 0.0, 0.0), (1.0, 1.0, 1.0), (2.0, 2.0, 2.0)],
        }
        .point_count(),
        3
    );
}

#[test]
fn test_point_count_zm_variants() {
    assert_eq!(
        GpkgGeometry::PointZM(Point4D {
            x: 0.0,
            y: 0.0,
            z: Some(1.0),
            m: Some(2.0)
        })
        .point_count(),
        1
    );
    assert_eq!(
        GpkgGeometry::LineStringZM {
            coords: vec![
                Point4D {
                    x: 0.0,
                    y: 0.0,
                    z: Some(0.0),
                    m: Some(0.0)
                },
                Point4D {
                    x: 1.0,
                    y: 1.0,
                    z: Some(1.0),
                    m: Some(1.0)
                },
            ],
        }
        .point_count(),
        2
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// bbox() projects M / ZM to XY
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_bbox_for_point_m() {
    let bbox = GpkgGeometry::PointM {
        x: 3.0,
        y: 4.0,
        m: 99.0,
    }
    .bbox()
    .expect("PointM must have a bbox");
    assert_eq!(bbox, (3.0, 4.0, 3.0, 4.0));
}

#[test]
fn test_bbox_for_linestring_zm() {
    let bbox = GpkgGeometry::LineStringZM {
        coords: vec![
            Point4D {
                x: 1.0,
                y: 2.0,
                z: Some(10.0),
                m: Some(0.0),
            },
            Point4D {
                x: 3.0,
                y: 4.0,
                z: Some(20.0),
                m: Some(1.0),
            },
        ],
    }
    .bbox()
    .expect("LineStringZM must have a bbox");
    assert_eq!(bbox, (1.0, 2.0, 3.0, 4.0));
}

// ─────────────────────────────────────────────────────────────────────────────
// WKB MultiLineStringM round-trip
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wkb_multilinestring_m_round_trip() {
    let original = GpkgGeometry::MultiLineStringM {
        lines: vec![
            vec![(0.0, 0.0, 1.0), (1.0, 1.0, 2.0)],
            vec![(2.0, 2.0, 3.0), (3.0, 3.0, 4.0), (4.0, 4.0, 5.0)],
        ],
    };
    let wkb = GpkgBinaryParser::to_wkb(&original);
    let decoded = GpkgBinaryParser::parse_wkb(&wkb).expect("parse MultiLineStringM");
    assert_eq!(decoded, original);
}

// ─────────────────────────────────────────────────────────────────────────────
// WKB MultiPolygonZM round-trip
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wkb_multipolygon_zm_round_trip() {
    let make_p4 = |x: f64, y: f64| Point4D {
        x,
        y,
        z: Some(0.0),
        m: Some(1.0),
    };
    let ring1 = vec![
        make_p4(0.0, 0.0),
        make_p4(1.0, 0.0),
        make_p4(1.0, 1.0),
        make_p4(0.0, 0.0),
    ];
    let ring2 = vec![
        make_p4(2.0, 2.0),
        make_p4(3.0, 2.0),
        make_p4(3.0, 3.0),
        make_p4(2.0, 2.0),
    ];
    let original = GpkgGeometry::MultiPolygonZM {
        polygons: vec![vec![ring1], vec![ring2]],
    };
    let wkb = GpkgBinaryParser::to_wkb(&original);
    let decoded = GpkgBinaryParser::parse_wkb(&wkb).expect("parse MultiPolygonZM");
    assert_eq!(decoded, original);
}
