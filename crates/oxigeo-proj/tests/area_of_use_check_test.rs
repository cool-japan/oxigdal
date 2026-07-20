//! Integration tests for opt-in area-of-use validation on `Transformer`.
//!
//! Covers the three [`AreaOfUseCheck`] modes (`Off`, `Warn`, `Strict`), the
//! antimeridian-aware [`AreaOfUse::contains`] semantics, and the pass-through
//! behaviour for CRS without a registered area-of-use entry.

#![allow(clippy::expect_used)]

use oxigeo_proj::{
    AreaOfUse, AreaOfUseCheck, Coordinate, Crs, Error, Transformer, area_of_use_for_epsg,
};

// -----------------------------------------------------------------------------
// `AreaOfUse::contains` semantics
// -----------------------------------------------------------------------------

#[test]
fn test_aou_contains_point_inside_bbox() {
    // Japan: west=122.38, south=17.09, east=157.65, north=46.05.
    let area = AreaOfUse::new(122.38, 17.09, 157.65, 46.05, "Japan");
    assert!(
        area.contains(139.69, 35.69),
        "Tokyo should be inside Japan AoU"
    );
    // Boundary points are inclusive.
    assert!(area.contains(122.38, 17.09));
    assert!(area.contains(157.65, 46.05));
}

#[test]
fn test_aou_contains_antimeridian_crossing() {
    // NZGD2000 (EPSG:4167) wraps the antimeridian: west=160.6, east=-171.2.
    // The valid lon set is [160.6, 180] ∪ [-180, -171.2].
    let area = AreaOfUse::new(160.6, -55.95, -171.2, -25.88, "New Zealand");

    // Eastern wraparound half (lon > 160.6, ≤ 180).
    assert!(area.contains(174.77, -41.29), "Wellington should be inside");
    assert!(
        area.contains(180.0, -40.0),
        "antimeridian itself should be inside"
    );

    // Western wraparound half (lon ≥ -180, ≤ -171.2).
    assert!(area.contains(-175.0, -40.0));
    assert!(area.contains(-180.0, -40.0));

    // Outside the wraparound region.
    assert!(!area.contains(0.0, -40.0));
    assert!(
        !area.contains(-150.0, -40.0),
        "lon between east and west should be outside"
    );
    assert!(
        !area.contains(159.0, -40.0),
        "lon just west of `west` should be outside"
    );

    // Out-of-lat band must always be false even within the wraparound lon range.
    assert!(!area.contains(174.77, 10.0));
}

#[test]
fn test_aou_contains_outside_returns_false() {
    let area = AreaOfUse::new(-10.0, 30.0, 10.0, 50.0, "Test");
    // Lon outside.
    assert!(!area.contains(-20.0, 40.0));
    assert!(!area.contains(20.0, 40.0));
    // Lat outside.
    assert!(!area.contains(0.0, 20.0));
    assert!(!area.contains(0.0, 60.0));
    // Both outside.
    assert!(!area.contains(-20.0, 20.0));
}

// -----------------------------------------------------------------------------
// `Transformer` × `AreaOfUseCheck`
// -----------------------------------------------------------------------------

/// Helper: build a same-CRS transformer for JGD2011 (Japan, EPSG:6668).  Using
/// a same-CRS pair avoids any proj4rs work and isolates the area-of-use logic.
fn jgd2011_transformer() -> Transformer {
    Transformer::from_epsg(6668, 6668).expect("same-CRS transformer should build")
}

#[test]
fn test_transformer_aou_off_passes_outside_silently() {
    // Default mode is Off — an out-of-Japan coordinate must transform cleanly
    // and produce no warning.
    let tf = jgd2011_transformer().with_area_of_use_check(AreaOfUseCheck::Off);
    assert_eq!(tf.area_of_use_check(), AreaOfUseCheck::Off);

    let outside = Coordinate::new(0.0, 0.0); // Gulf of Guinea — not Japan.
    let out = tf.transform(&outside).expect("Off mode must not error");
    assert_eq!(out, outside, "no-op same-CRS transform should round-trip");
    assert!(
        tf.last_warning().is_none(),
        "Off mode must not record a warning"
    );
}

#[test]
fn test_transformer_aou_warn_records_warning() {
    let tf = jgd2011_transformer().with_area_of_use_check(AreaOfUseCheck::Warn);
    assert_eq!(tf.area_of_use_check(), AreaOfUseCheck::Warn);
    assert!(
        tf.last_warning().is_none(),
        "no warning before any transform call"
    );

    let outside = Coordinate::new(0.0, 0.0);
    let out = tf.transform(&outside).expect("Warn mode must not error");
    assert_eq!(out, outside);

    let w = tf.last_warning().expect("Warn mode must record a warning");
    assert_eq!(w.lon, 0.0);
    assert_eq!(w.lat, 0.0);
    assert_eq!(w.epsg, 6668);
    // Bounds should match the registered Japan AoU.
    let expected = area_of_use_for_epsg(6668).expect("JGD2011 has an AoU");
    assert_eq!(w.west, expected.west);
    assert_eq!(w.south, expected.south);
    assert_eq!(w.east, expected.east);
    assert_eq!(w.north, expected.north);

    // Clearing the warning works.
    tf.clear_warning();
    assert!(tf.last_warning().is_none());
}

#[test]
fn test_transformer_aou_strict_returns_error() {
    let tf = jgd2011_transformer().with_area_of_use_check(AreaOfUseCheck::Strict);
    assert_eq!(tf.area_of_use_check(), AreaOfUseCheck::Strict);

    let outside = Coordinate::new(0.0, 0.0);
    let err = tf
        .transform(&outside)
        .expect_err("Strict mode must reject out-of-area points");
    let expected = area_of_use_for_epsg(6668).expect("JGD2011 has an AoU");
    assert!(
        matches!(
            err,
            Error::OutsideAreaOfUse {
                lon,
                lat,
                epsg: 6668,
                west,
                south,
                east,
                north,
            } if lon == 0.0
                && lat == 0.0
                && (west - expected.west).abs() < f64::EPSILON
                && (south - expected.south).abs() < f64::EPSILON
                && (east - expected.east).abs() < f64::EPSILON
                && (north - expected.north).abs() < f64::EPSILON
        ),
        "expected OutsideAreaOfUse with Japan bounds, got: {:?}",
        err
    );

    // A point *inside* Japan must succeed even in Strict mode.
    let tokyo = Coordinate::new(139.69, 35.69);
    let out = tf.transform(&tokyo).expect("Tokyo must pass Strict mode");
    assert_eq!(out, tokyo);
}

#[test]
fn test_transformer_aou_unknown_epsg_passes_through() {
    // Custom CRS has no EPSG code → all modes pass through silently.
    let custom = Crs::custom("Custom", "+proj=longlat +datum=WGS84 +no_defs");
    let outside = Coordinate::new(500.0, 500.0); // would be out-of-area for any real CRS.

    for mode in [
        AreaOfUseCheck::Off,
        AreaOfUseCheck::Warn,
        AreaOfUseCheck::Strict,
    ] {
        let tf = Transformer::new(custom.clone(), custom.clone())
            .expect("same-CRS transformer should build")
            .with_area_of_use_check(mode);

        let result = tf.transform(&outside);
        assert!(
            result.is_ok(),
            "custom-CRS transform must succeed in mode {:?}: {:?}",
            mode,
            result
        );
        let out = result.expect("checked is_ok above");
        assert_eq!(
            out, outside,
            "no-op transform should round-trip in mode {:?}",
            mode
        );
        assert!(
            tf.last_warning().is_none(),
            "no warning expected for unknown EPSG in mode {:?}",
            mode
        );
    }
}

#[test]
fn test_transformer_aou_global_crs_always_passes() {
    // EPSG:4326 has the world bbox — every finite lon/lat must pass.
    let tf = Transformer::from_epsg(4326, 4326)
        .expect("WGS84 same-CRS transformer")
        .with_area_of_use_check(AreaOfUseCheck::Strict);

    for coord in [
        Coordinate::new(0.0, 0.0),
        Coordinate::new(180.0, 90.0),
        Coordinate::new(-180.0, -90.0),
        Coordinate::new(139.69, 35.69),
        Coordinate::new(-122.4194, 37.7749),
    ] {
        let result = tf.transform(&coord);
        assert!(
            result.is_ok(),
            "WGS84 must accept {:?}: {:?}",
            coord,
            result
        );
        let out = result.expect("checked is_ok above");
        assert_eq!(out, coord);
        assert!(tf.last_warning().is_none(), "WGS84 must never warn");
    }
}

#[test]
fn test_transformer_aou_builder_resets_warning() {
    // Calling `with_area_of_use_check` again must clear any prior warning so
    // that test fixtures are easy to reset.
    let tf = jgd2011_transformer().with_area_of_use_check(AreaOfUseCheck::Warn);
    let _ = tf.transform(&Coordinate::new(0.0, 0.0)).expect("Warn ok");
    assert!(tf.last_warning().is_some());

    let tf = tf.with_area_of_use_check(AreaOfUseCheck::Off);
    assert!(
        tf.last_warning().is_none(),
        "builder must reset warning state"
    );
}

#[test]
fn test_transformer_aou_batch_strict_rejects_first_offender() {
    let tf = jgd2011_transformer().with_area_of_use_check(AreaOfUseCheck::Strict);
    let coords = [
        Coordinate::new(139.69, 35.69), // Tokyo — ok
        Coordinate::new(0.0, 0.0),      // out of Japan
        Coordinate::new(140.0, 40.0),   // ok
    ];
    let err = tf
        .transform_batch(&coords)
        .expect_err("batch must reject the out-of-area entry");
    assert!(matches!(err, Error::OutsideAreaOfUse { lon, lat, .. } if lon == 0.0 && lat == 0.0));
}
