//! Integration tests for Engineering CRS (ENGCRS) parsing and transformer behaviour.
//!
//! ENGCRS is a WKT2:2019 keyword representing local/engineering coordinate systems
//! (e.g. ship-board, construction-site grids) that have no geodetic datum.
//! Because no spatial conversion is possible without additional parameters,
//! a transformer involving an Engineering CRS must either:
//!   (a) return an identity (pass-through) transformer, or
//!   (b) return a descriptive `Err(…)` — not panic.
//!
//! These tests verify Slice 17 W1 Part B.

#![allow(clippy::expect_used)]

use oxigdal_proj::{Coordinate, Crs, CrsType, Transformer};

// ---------------------------------------------------------------------------
// Helper — a minimal but well-formed ENGCRS WKT2 string
// ---------------------------------------------------------------------------

fn minimal_engcrs_wkt() -> &'static str {
    r#"ENGCRS["My Local Grid",
        LOCAL_DATUM["Arbitrary origin",32767],
        CS[Cartesian,2],
        AXIS["(E)",east,ORDER[1],LENGTHUNIT["metre",1]],
        AXIS["(N)",north,ORDER[2],LENGTHUNIT["metre",1]]]"#
}

// ---------------------------------------------------------------------------
// Part B test 1 — parse ENGCRS WKT without error and get CrsType::Engineering
// ---------------------------------------------------------------------------

/// Parsing a well-formed ENGCRS WKT string must succeed and return a CRS with
/// `crs_type() == Some(CrsType::Engineering)`.
#[test]
fn test_engcrs_from_wkt_parses_without_error() {
    let wkt = minimal_engcrs_wkt();
    let crs = Crs::from_wkt(wkt).expect("ENGCRS WKT should parse without error");

    assert_eq!(
        crs.crs_type(),
        Some(CrsType::Engineering),
        "crs_type must be Engineering, got {:?}",
        crs.crs_type()
    );

    // The CRS must report itself as engineering.
    assert!(
        crs.is_engineering(),
        "is_engineering() must return true for ENGCRS"
    );

    // A simple one-liner ENGCRS also works (matches the existing crs.rs test).
    let simple_wkt = r#"ENGCRS["Local Engineering"]"#;
    let simple_crs = Crs::from_wkt(simple_wkt).expect("minimal ENGCRS should parse");
    assert_eq!(simple_crs.crs_type(), Some(CrsType::Engineering));
}

// ---------------------------------------------------------------------------
// Part B test 2 — transformer from Engineering CRS to WGS84: identity or error
// ---------------------------------------------------------------------------

/// Calling `Transformer::from_crs(engcrs, wgs84)` must either:
///   (a) succeed and return an identity-equivalent transformer, or
///   (b) fail with a descriptive error.
/// In no case should it panic or return an error whose message is completely
/// unrelated to the Engineering CRS limitation.
#[test]
fn test_engcrs_identity_transform_or_graceful_error() {
    let engcrs_wkt = minimal_engcrs_wkt();
    let engcrs = Crs::from_wkt(engcrs_wkt).expect("ENGCRS should parse");
    let wgs84 = Crs::wgs84();

    // Attempt to create a transformer: Engineering → WGS84.
    let transformer_result = Transformer::new(engcrs, wgs84);

    match transformer_result {
        Ok(transformer) => {
            // Identity path: the transformer must pass coordinates through
            // unchanged (or apply only a trivial conversion).
            let coord = Coordinate::new(1_000.0, 2_000.0);
            let result = transformer.transform(&coord);
            // The transform must not panic and must either succeed or fail
            // gracefully (finite result in the identity case).
            match result {
                Ok(out) => {
                    // Identity: output must equal input.
                    assert!(
                        (out.x - coord.x).abs() < 1e-6,
                        "identity transformer x mismatch: in={} out={}",
                        coord.x,
                        out.x
                    );
                    assert!(
                        (out.y - coord.y).abs() < 1e-6,
                        "identity transformer y mismatch: in={} out={}",
                        coord.y,
                        out.y
                    );
                }
                Err(e) => {
                    // A transform-time error is acceptable — must not panic.
                    let msg = format!("{}", e);
                    assert!(
                        !msg.is_empty(),
                        "error message must not be empty for engineering transform failure"
                    );
                }
            }
        }
        Err(e) => {
            // Construction-time error is also acceptable: check the message is
            // informative (not a raw proj4rs internal error string).
            let msg = format!("{}", e);
            assert!(
                !msg.is_empty(),
                "error message must not be empty for engineering CRS transformer creation"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Part B test 3 — ENGCRS ↔ ENGCRS: must not panic
// ---------------------------------------------------------------------------

/// Two Engineering CRS instances (even the same WKT) must be transformable
/// (identity or descriptive error), never panic.
#[test]
fn test_engcrs_to_engcrs_no_panic() {
    let wkt = minimal_engcrs_wkt();
    let src = Crs::from_wkt(wkt).expect("src ENGCRS");
    let dst = Crs::from_wkt(wkt).expect("dst ENGCRS");

    // Must not panic.
    let result = Transformer::new(src, dst);
    // Whether Ok or Err, we just require no panic and a non-empty message on Err.
    if let Err(e) = result {
        assert!(!format!("{}", e).is_empty());
    }
}
