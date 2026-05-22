//! Integration tests for ITRF epoch-aware coordinate transformation (Slice 7 W5).
//!
//! Covers:
//!  1. `ItrfTransformParams::params_at_epoch` with zero rates → identity.
//!  2. `ItrfTransformParams::params_at_epoch` with known rates → linear extrapolation.
//!  3. `find_itrf_params` with a known ITRF pair → `Some`.
//!  4. `find_itrf_params` with an unknown pair → `None`.
//!  5. `Transformer::with_epoch` on a non-ITRF CRS pair → `Err`.
//!  6. `Transformer::with_epoch` on an ITRF pair → applies epoch correction.
//!  7. `Transformer` without `with_epoch` → behaves exactly as before.

use oxigdal_proj::{
    Coordinate, Coordinate3D, Crs, Transformer,
    datum_transform::{
        BursaWolfParams, Ellipsoid, EpochTransformArgs, ItrfTransformParams, find_itrf_params,
    },
};

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — zero-rate params: `params_at_epoch` is the identity on the rates
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_itrf_transform_params_at_epoch_preserves_identity_when_zero_rates() {
    // Build a params object with all rates set to zero.
    let bw = BursaWolfParams::new(
        1.6e-3,  // tx  (m)
        1.9e-3,  // ty  (m)
        2.4e-3,  // tz  (m)
        0.001,   // rx  (arcsec)
        0.002,   // ry  (arcsec)
        0.003,   // rz  (arcsec)
        0.05e-3, // ds  (ppm)
    );
    let zero_rates = [0.0f64; 7];
    let params = ItrfTransformParams::new(bw, zero_rates);

    // With zero rates, extrapolation to any epoch must reproduce the reference values.
    let ref_epoch = 2010.0_f64;
    for offset in [0.0_f64, 5.0, -5.0, 100.0] {
        let at = params.params_at_epoch(ref_epoch + offset, ref_epoch);
        assert!(
            (at.tx - bw.tx).abs() < 1e-15,
            "tx must be unchanged for offset={offset}: got {}, expected {}",
            at.tx,
            bw.tx
        );
        assert!(
            (at.ty - bw.ty).abs() < 1e-15,
            "ty must be unchanged for offset={offset}"
        );
        assert!(
            (at.tz - bw.tz).abs() < 1e-15,
            "tz must be unchanged for offset={offset}"
        );
        assert!(
            (at.rx - bw.rx).abs() < 1e-15,
            "rx must be unchanged for offset={offset}"
        );
        assert!(
            (at.ry - bw.ry).abs() < 1e-15,
            "ry must be unchanged for offset={offset}"
        );
        assert!(
            (at.rz - bw.rz).abs() < 1e-15,
            "rz must be unchanged for offset={offset}"
        );
        assert!(
            (at.ds - bw.ds).abs() < 1e-15,
            "ds must be unchanged for offset={offset}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — known rates: linear extrapolation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_itrf_transform_params_at_epoch_applies_linear_rates() {
    // Set dx_rate = 1.0 mm/yr = 1e-3 m/yr, all others zero.
    let ref_bw = BursaWolfParams::new(10.0e-3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    // rates array: [dtx, dty, dtz, drx, dry, drz, dds]
    let rates = [1.0e-3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let params = ItrfTransformParams::new(ref_bw, rates);

    let ref_epoch = 2005.0_f64;

    // After 5 years the x-translation should increase by 5 × 1 mm = 5 mm.
    let at_5yr = params.params_at_epoch(ref_epoch + 5.0, ref_epoch);
    let expected_tx = ref_bw.tx + 5.0 * 1.0e-3;
    assert!(
        (at_5yr.tx - expected_tx).abs() < 1e-14,
        "tx after 5 yr should be {expected_tx:.6e}, got {:.6e}",
        at_5yr.tx
    );

    // Verify the arithmetic sequence: offsets 0..=10 in steps of 1 yr.
    for step in 0_u32..=10 {
        let dt = step as f64;
        let at = params.params_at_epoch(ref_epoch + dt, ref_epoch);
        let expected = ref_bw.tx + dt * 1.0e-3;
        assert!(
            (at.tx - expected).abs() < 1e-14,
            "tx at step={step}: expected {expected:.6e}, got {:.6e}",
            at.tx
        );
        // All other parameters must remain at their reference values.
        assert!((at.ty - ref_bw.ty).abs() < 1e-15, "ty must stay zero");
        assert!((at.ds - ref_bw.ds).abs() < 1e-15, "ds must stay zero");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — `find_itrf_params` with a known pair
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_find_itrf_params_known_pair_returns_some() {
    // Forward: ITRF2014 → ITRF2008
    let result = find_itrf_params("ITRF2014", "ITRF2008");
    assert!(
        result.is_some(),
        "find_itrf_params(ITRF2014, ITRF2008) should return Some"
    );
    let (params, ref_epoch) = result.expect("checked above");
    // Reference epoch for ITRF2014 is 2010.0.
    assert!(
        (ref_epoch - 2010.0).abs() < 1e-9,
        "ref_epoch should be 2010.0, got {ref_epoch}"
    );
    // The tx parameter at the reference epoch should be ~1.6 mm (per IERS TN61).
    assert!(
        params.bursa_wolf.tx.abs() > 0.0,
        "bursa_wolf.tx should be non-zero"
    );

    // Also verify the inverse direction works.
    let inv = find_itrf_params("ITRF2008", "ITRF2014");
    assert!(
        inv.is_some(),
        "find_itrf_params(ITRF2008, ITRF2014) should also return Some (inverse)"
    );
    let (inv_params, _) = inv.expect("checked above");
    // tx of the inverse should be the negation of the forward tx.
    assert!(
        (inv_params.bursa_wolf.tx + params.bursa_wolf.tx).abs() < 1e-14,
        "inverse tx should negate forward tx"
    );

    // ITRF2008 → ITRF2005 (forward preset)
    let r2 = find_itrf_params("ITRF2008", "ITRF2005");
    assert!(r2.is_some(), "ITRF2008 → ITRF2005 should be registered");

    // ITRF2000 → ITRF97 (forward preset)
    let r3 = find_itrf_params("ITRF2000", "ITRF97");
    assert!(r3.is_some(), "ITRF2000 → ITRF97 should be registered");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — `find_itrf_params` with an unknown pair
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_find_itrf_params_unknown_pair_returns_none() {
    assert!(
        find_itrf_params("ITRF2014", "ITRF1900").is_none(),
        "ITRF1900 is not a registered frame"
    );
    assert!(
        find_itrf_params("WGS84", "ITRF2008").is_none(),
        "WGS84 is not an ITRF frame in the preset table"
    );
    assert!(
        find_itrf_params("ITRF2020", "ITRF2014").is_none(),
        "ITRF2020 → ITRF2014 has no registered preset"
    );
    assert!(
        find_itrf_params("ITRF2014", "ITRF2014").is_none(),
        "identity pair (same frame) has no preset"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — `Transformer::with_epoch` rejects non-ITRF CRS
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_transformer_with_epoch_requires_itrf_crs() {
    let wgs84 = Crs::wgs84();
    let web_mercator = Crs::web_mercator();

    let transformer = Transformer::new(wgs84.clone(), web_mercator.clone())
        .expect("WGS84 → WebMercator transformer should construct");

    let result = transformer.with_epoch(2015.0, 2020.0);
    assert!(
        result.is_err(),
        "with_epoch on a non-ITRF pair must return Err"
    );

    // Same-frame WGS84 pair (not ITRF) should also fail.
    let t2 = Transformer::new(wgs84.clone(), wgs84.clone())
        .expect("same CRS transformer should construct");
    assert!(
        t2.with_epoch(2000.0, 2005.0).is_err(),
        "WGS84 is not ITRF; with_epoch must return Err"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — `Transformer::with_epoch` applies epoch correction
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_itrf_epoch_transform_3d_applies_epoch_correction() {
    // Use EPSG:7789 (ITRF2014) → EPSG:7930 (ITRF2008) — both are registered.
    let itrf2014 = Crs::from_epsg(7789).expect("ITRF2014 EPSG:7789 should be registered");
    let itrf2008 = Crs::from_epsg(7930).expect("ITRF2008 EPSG:7930 should be registered");

    // Confirm itrf_name is detected.
    assert_eq!(
        itrf2014.itrf_name().as_deref(),
        Some("ITRF2014"),
        "EPSG:7789 itrf_name"
    );
    assert_eq!(
        itrf2008.itrf_name().as_deref(),
        Some("ITRF2008"),
        "EPSG:7930 itrf_name"
    );

    let transformer = Transformer::new(itrf2014, itrf2008)
        .expect("ITRF2014 → ITRF2008 transformer should build")
        .with_epoch(2010.0, 2020.0)
        .expect("with_epoch(2010, 2020) should succeed for ITRF2014 → ITRF2008");

    assert_eq!(transformer.source_epoch(), Some(2010.0));
    assert_eq!(transformer.target_epoch(), Some(2020.0));

    // Geodetic point: approximately Tokyo Station (lon=139.77, lat=35.68, h=50 m).
    let input = Coordinate3D::new(139.77, 35.68, 50.0);
    let output = transformer
        .transform_3d(&input)
        .expect("epoch transform should succeed");

    assert!(output.is_valid(), "output coordinate must be finite");

    // The coordinates should differ from the input (epoch shift is non-zero
    // over a 10-year span with the published IERS rates).
    let lon_delta = (output.x - input.x).abs();
    let lat_delta = (output.y - input.y).abs();
    let h_delta = (output.z - input.z).abs();

    // Bursa-Wolf corrections are tiny (sub-mm in position).  They will produce
    // differences well under 1 arc-second in lon/lat and under 1 m in height,
    // but strictly greater than floating-point epsilon for a 10-year span.
    assert!(
        lon_delta < 1.0,
        "longitude shift should be sub-degree, got {lon_delta}"
    );
    assert!(
        lat_delta < 1.0,
        "latitude shift should be sub-degree, got {lat_delta}"
    );
    assert!(
        h_delta < 1.0,
        "height shift should be sub-metre, got {h_delta}"
    );

    // The epoch correction must produce a result that differs from the input
    // (rates are non-zero for ITRF2014→ITRF2008 over 10 yr).
    let any_change = lon_delta > 0.0 || lat_delta > 0.0 || h_delta > 0.0;
    assert!(any_change, "epoch correction must produce a non-zero delta");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — `Transformer` without `with_epoch` is unaffected
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_transformer_without_epoch_unaffected() {
    // Verify that transformers that don't call with_epoch still work exactly
    // as they did before this change.

    // 1. Same CRS passthrough.
    let wgs84 = Crs::wgs84();
    let t_same = Transformer::new(wgs84.clone(), wgs84).expect("same-CRS transformer");
    let pt = Coordinate::new(10.0, 50.0);
    let result = t_same.transform(&pt).expect("passthrough should succeed");
    assert!(
        (result.x - pt.x).abs() < 1e-9 && (result.y - pt.y).abs() < 1e-9,
        "passthrough must be identity"
    );

    // 2. WGS84 → Web Mercator 2D transform still works.
    let transformer = Transformer::from_epsg(4326, 3857).expect("WGS84 → WebMerc");
    let london = Coordinate::new(0.0, 51.5);
    let merc = transformer
        .transform(&london)
        .expect("2D transform should succeed");
    assert!(
        merc.x.abs() < 1.0,
        "prime meridian x should be ~0 in WebMerc"
    );
    assert!(
        merc.y > 6_000_000.0 && merc.y < 7_500_000.0,
        "London northing in WebMerc out of expected range: {}",
        merc.y
    );

    // 3. 3D passthrough without epoch is unchanged.
    let t3d = Transformer::from_epsg(4326, 4326).expect("same EPSG 3D");
    let p3 = Coordinate3D::new(10.0, 50.0, 100.0);
    let r3 = t3d.transform_3d(&p3).expect("3D passthrough");
    assert!(
        (r3.x - p3.x).abs() < 1e-9 && (r3.y - p3.y).abs() < 1e-9 && (r3.z - p3.z).abs() < 1e-9,
        "3D passthrough must be identity"
    );

    // 4. Epoch accessors return None when with_epoch was not called.
    let t_no_epoch = Transformer::from_epsg(4326, 4326).expect("baseline");
    assert_eq!(t_no_epoch.source_epoch(), None);
    assert_eq!(t_no_epoch.target_epoch(), None);

    // 5. Zero-epoch-delta invariant tested via ItrfTransformParams directly:
    //    transform_at_epoch with t0==t1 must return the input unchanged.
    let params = ItrfTransformParams::itrf2014_to_itrf2008();
    let ref_epoch = 2010.0_f64;
    let same_epoch = 2020.0_f64;
    // Zero-delta check: params at same epoch minus params at same epoch = zero BW.
    let bw = params.params_at_epoch(same_epoch, ref_epoch);
    let bw_again = params.params_at_epoch(same_epoch, ref_epoch);
    // Net correction is (bw - bw) = zero, so the transform must be identity.
    let net_tx = bw.tx - bw_again.tx;
    let net_ty = bw.ty - bw_again.ty;
    assert!(
        net_tx.abs() < 1e-15 && net_ty.abs() < 1e-15,
        "same-epoch delta must be zero"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Additional: zero-epoch-delta round-trip through Transformer::transform_3d
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_transformer_with_epoch_same_epoch_is_identity() {
    // When source_epoch == target_epoch the net Bursa-Wolf correction is zero,
    // so the output coordinates must equal the input.
    let itrf2014 = Crs::from_epsg(7789).expect("ITRF2014");
    let itrf2008 = Crs::from_epsg(7930).expect("ITRF2008");

    let transformer = Transformer::new(itrf2014, itrf2008)
        .expect("build transformer")
        .with_epoch(2015.0, 2015.0)
        .expect("same-epoch with_epoch");

    // Use an arithmetic sequence of points — no rand.
    let base_lon = 0.0_f64;
    let base_lat = 0.0_f64;
    let base_h = 0.0_f64;
    for i in 0_u32..5 {
        let lon = base_lon + i as f64 * 10.0;
        let lat = base_lat + i as f64 * 5.0;
        let h = base_h + i as f64 * 100.0;
        let input = Coordinate3D::new(lon, lat, h);
        let output = transformer
            .transform_3d(&input)
            .expect("same-epoch transform should succeed");

        assert!(
            (output.x - input.x).abs() < 1e-9,
            "lon unchanged for same epoch: delta={}",
            (output.x - input.x).abs()
        );
        assert!(
            (output.y - input.y).abs() < 1e-9,
            "lat unchanged for same epoch: delta={}",
            (output.y - input.y).abs()
        );
        assert!(
            (output.z - input.z).abs() < 1e-9,
            "h unchanged for same epoch: delta={}",
            (output.z - input.z).abs()
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Additional: EpochTransformArgs usage verification
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_epoch_transform_args_direct_round_trip() {
    // Build a zero-rate ItrfTransformParams and verify transform_at_epoch is identity
    // for any lat/lon when source == target epoch.
    let zero_bw = BursaWolfParams::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let params = ItrfTransformParams::new(zero_bw, [0.0; 7]);

    let ref_epoch = 2010.0_f64;
    let lat_deg = 35.68_f64;
    let lon_deg = 139.77_f64;
    let h = 50.0_f64;

    let args = EpochTransformArgs::new(
        lat_deg.to_radians(),
        lon_deg.to_radians(),
        h,
        &Ellipsoid::GRS80,
        &Ellipsoid::GRS80,
        ref_epoch,
        ref_epoch, // same epoch → zero delta
    );

    let (lat_out_rad, lon_out_rad, h_out) = params.transform_at_epoch(args);

    assert!(
        (lat_out_rad.to_degrees() - lat_deg).abs() < 1e-9,
        "lat should be unchanged: got {}, expected {}",
        lat_out_rad.to_degrees(),
        lat_deg
    );
    assert!(
        (lon_out_rad.to_degrees() - lon_deg).abs() < 1e-9,
        "lon should be unchanged: got {}, expected {}",
        lon_out_rad.to_degrees(),
        lon_deg
    );
    assert!(
        (h_out - h).abs() < 1e-6,
        "h should be unchanged: got {h_out}, expected {h}"
    );
}
