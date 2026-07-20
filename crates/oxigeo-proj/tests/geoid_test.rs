//! Integration tests for the geoid model (Slice 14 W1).
//!
//! Covers synthetic-grid construction, bilinear interpolation behaviour,
//! file round-trip via a temp file, and end-to-end height conversion when a
//! geoid is attached to a [`Transformer`].

#![allow(clippy::expect_used)]

use std::io::Write;
use std::sync::Arc;

use oxigeo_proj::geoid::VerticalDatumKind;
use oxigeo_proj::{
    Coordinate3D, Crs, GeoidGrid, GeoidModel, Transformer, classify_vertical_datum, load_egm_grid,
    synthetic_grid, synthetic_height_m,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Writes `bytes` to a uniquely-named file inside `std::env::temp_dir()`
/// and returns its path.  Used by [`test_load_egm_grid_round_trip_via_tempfile`]
/// and [`test_load_egm_grid_size_mismatch_errors`].
fn write_tempfile(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    path.push(format!("oxigeo_geoid_{name}_{pid}_{nanos}.bin"));
    let mut f = std::fs::File::create(&path).expect("create tempfile");
    f.write_all(bytes).expect("write tempfile");
    f.flush().expect("flush tempfile");
    path
}

/// Builds an in-memory little-endian f32 byte buffer from a `Vec<f32>`.
fn encode_le_f32(values: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Builds a compound CRS with the named horizontal (EPSG:4326) and a vertical
/// CRS parsed from `vert_wkt`.
fn make_compound_with_vert(vert_wkt: &str) -> Crs {
    let horiz = Crs::wgs84();
    let vert = Crs::from_wkt(vert_wkt).expect("vertical WKT must parse");
    Crs::compound(horiz, vert).expect("compound must build")
}

// ---------------------------------------------------------------------------
// Tests — synthetic grid dimensions and node values
// ---------------------------------------------------------------------------

#[test]
fn test_geoid_synthetic_grid_has_expected_dims() {
    // 2° step, -90..=90 → 91 rows; -180..178 → 180 cols (longitude wraps at 180).
    let g = synthetic_grid(GeoidModel::Egm96);
    assert_eq!(g.n_lat, 91, "should be 91 latitude rows");
    assert_eq!(g.n_lon, 180, "should be 180 longitude columns");
    assert_eq!(g.heights_m.len(), 91 * 180);
    assert!((g.lat_step_deg - 2.0).abs() < f64::EPSILON);
    assert!((g.lon_step_deg - 2.0).abs() < f64::EPSILON);
    assert!((g.lat_min_deg - -90.0).abs() < f64::EPSILON);
    assert!((g.lon_min_deg - -180.0).abs() < f64::EPSILON);
}

#[test]
fn test_geoid_height_at_grid_node_exact() {
    // Querying exactly at a grid node must return the stored value
    // (no interpolation artefacts).  Use (lat=30°, lon=60°) which falls
    // on a node for the 2° synthetic grid.
    let g = synthetic_grid(GeoidModel::Egm96);
    let lat = 30.0_f64;
    let lon = 60.0_f64;
    let value = g.geoid_height_m(lat, lon);
    let expected = synthetic_height_m(lat, lon);
    assert!(
        (value - expected).abs() < 1e-3,
        "query at grid node should match stored value (got {value}, expected {expected})"
    );
}

// ---------------------------------------------------------------------------
// Tests — bilinear interpolation behaviour
// ---------------------------------------------------------------------------

#[test]
fn test_geoid_height_bilinear_interpolates_midpoint() {
    // For a smooth field, the value midway between four nodes should be very
    // close to the average of the four neighbours, with tiny departure caused
    // by the curvature of the analytic field over the cell.
    let g = synthetic_grid(GeoidModel::Egm96);

    // Cell with corners at (lat=30°, lon=60°), (30°, 62°), (32°, 60°), (32°, 62°).
    // Midpoint: (31°, 61°).
    let n00 = synthetic_height_m(30.0, 60.0);
    let n01 = synthetic_height_m(30.0, 62.0);
    let n10 = synthetic_height_m(32.0, 60.0);
    let n11 = synthetic_height_m(32.0, 62.0);
    let bilinear_expected = 0.25_f64 * (n00 + n01 + n10 + n11);
    let queried = g.geoid_height_m(31.0, 61.0);
    // Bilinear over a smooth sinusoidal field should track the average closely.
    assert!(
        (queried - bilinear_expected).abs() < 1e-2,
        "midpoint interpolation: got {queried}, expected ~{bilinear_expected}"
    );
}

#[test]
fn test_geoid_longitude_wraps_at_antimeridian() {
    // For a global grid (n_lon * lon_step = 360°), querying at +180° and
    // -180° must yield identical heights (longitude is wrapped modulo 360°).
    let g = synthetic_grid(GeoidModel::Egm96);
    let v_pos = g.geoid_height_m(20.0, 180.0);
    let v_neg = g.geoid_height_m(20.0, -180.0);
    assert!(
        (v_pos - v_neg).abs() < 1e-6,
        "longitude wrap mismatch: +180° → {v_pos}, -180° → {v_neg}"
    );
}

#[test]
fn test_geoid_latitude_clamps_at_pole() {
    // Querying north of +90° must clamp to the northernmost row (lat=+90°).
    let g = synthetic_grid(GeoidModel::Egm96);
    let v_91 = g.geoid_height_m(91.0, 0.0);
    let v_90 = g.geoid_height_m(90.0, 0.0);
    assert!(
        (v_91 - v_90).abs() < 1e-6,
        "latitude clamp at pole: 91° → {v_91}, 90° → {v_90}"
    );

    let v_minus_91 = g.geoid_height_m(-91.0, 0.0);
    let v_minus_90 = g.geoid_height_m(-90.0, 0.0);
    assert!(
        (v_minus_91 - v_minus_90).abs() < 1e-6,
        "latitude clamp at south pole: -91° → {v_minus_91}, -90° → {v_minus_90}"
    );
}

// ---------------------------------------------------------------------------
// Tests — height conversion round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_orthometric_to_ellipsoidal_round_trip() {
    let g = synthetic_grid(GeoidModel::Egm2008);
    for &lat in &[-45.0_f64, 0.0, 23.5, 45.0, 60.0] {
        for &lon in &[-120.0_f64, 0.0, 17.0, 90.0] {
            let h0 = 250.123;
            let h_ellip = g.orthometric_to_ellipsoidal(lat, lon, h0);
            let h_back = g.ellipsoidal_to_orthometric(lat, lon, h_ellip);
            assert!(
                (h_back - h0).abs() < 1e-9,
                "round-trip mismatch at ({lat}, {lon}): {h0} -> {h_ellip} -> {h_back}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests — file I/O
// ---------------------------------------------------------------------------

#[test]
fn test_load_egm_grid_size_mismatch_errors() {
    // Write 7 bytes — definitely not a multiple of 4 nor 2x2x4=16 expected.
    let bytes = vec![0u8; 7];
    let path = write_tempfile("too_small", &bytes);
    let result = load_egm_grid(
        &path,
        GeoidModel::Egm96,
        -90.0,
        -180.0,
        2.0,
        2.0,
        2,
        2, // declared 2x2 = 16 bytes
    );
    let _ = std::fs::remove_file(&path);
    let err = result.expect_err("size-mismatched load must fail");
    let msg = format!("{err}");
    assert!(
        msg.to_lowercase().contains("geoid"),
        "error message should mention geoid: {msg}"
    );
}

#[test]
fn test_load_egm_grid_round_trip_via_tempfile() {
    // Build a known small grid (3x4) with deterministic values, write it,
    // load it back, and assert every cell matches.
    let n_lat = 3_usize;
    let n_lon = 4_usize;
    let values: Vec<f32> = (0..(n_lat * n_lon))
        .map(|i| (i as f32) * 0.5 - 1.0)
        .collect();
    let bytes = encode_le_f32(&values);
    let path = write_tempfile("round_trip", &bytes);
    let grid = load_egm_grid(
        &path,
        GeoidModel::Egm2008,
        20.0,
        100.0,
        0.5,
        0.5,
        n_lat,
        n_lon,
    )
    .expect("load must succeed");
    let _ = std::fs::remove_file(&path);

    assert_eq!(grid.model, GeoidModel::Egm2008);
    assert_eq!(grid.n_lat, n_lat);
    assert_eq!(grid.n_lon, n_lon);
    assert_eq!(grid.heights_m.len(), n_lat * n_lon);
    for (i, expected) in values.iter().enumerate() {
        assert!(
            (grid.heights_m[i] - expected).abs() < 1e-7,
            "cell {i}: got {got}, expected {expected}",
            got = grid.heights_m[i],
        );
    }
}

// ---------------------------------------------------------------------------
// Tests — Transformer integration
// ---------------------------------------------------------------------------

#[test]
fn test_transformer_compound_with_geoid_applies_undulation() {
    // Compound CRS source: WGS84 horizontal + EGM96 orthometric height.
    // Compound CRS target: WGS84 horizontal + WGS 84 ellipsoidal height.
    // Attaching a geoid must cause `transform_3d` to apply the undulation
    // correction `h_ellip = h_ortho + N`.
    let src =
        make_compound_with_vert(r#"VERTCRS["EGM96 height",VDATUM["EGM96 geoid"],UNIT["metre",1]]"#);
    let dst = make_compound_with_vert(
        r#"VERTCRS["WGS 84 ellipsoidal height",VDATUM["WGS_1984"],UNIT["metre",1]]"#,
    );

    let grid = Arc::new(synthetic_grid(GeoidModel::Egm96));
    let transformer = Transformer::new(src, dst)
        .expect("compound transformer")
        .with_geoid(grid.clone());

    // Tokyo: lat=35.6895°N, lon=139.6917°E.
    let input = Coordinate3D::new(139.6917, 35.6895, 100.0);
    let undulation = grid.geoid_height_m(input.y, input.x);
    let output = transformer
        .transform_3d(&input)
        .expect("transform must succeed");

    // Horizontal pair is unchanged (same EPSG:4326 horizontal CRS).
    assert!((output.x - input.x).abs() < 1e-9);
    assert!((output.y - input.y).abs() < 1e-9);
    // Vertical: should add undulation.
    let expected_z = input.z + undulation;
    assert!(
        (output.z - expected_z).abs() < 1e-6,
        "expected z = {expected_z} (= {input_z} + {undulation}), got {got}",
        input_z = input.z,
        got = output.z,
    );

    // Reverse direction must subtract the undulation.
    let dst2 =
        make_compound_with_vert(r#"VERTCRS["EGM96 height",VDATUM["EGM96 geoid"],UNIT["metre",1]]"#);
    let src2 = make_compound_with_vert(
        r#"VERTCRS["WGS 84 ellipsoidal height",VDATUM["WGS_1984"],UNIT["metre",1]]"#,
    );
    let t2 = Transformer::new(src2, dst2)
        .expect("compound transformer rev")
        .with_geoid(grid.clone());
    let out2 = t2.transform_3d(&input).expect("ok");
    let expected_z2 = input.z - undulation;
    assert!(
        (out2.z - expected_z2).abs() < 1e-6,
        "ellipsoidal → orthometric: expected {expected_z2}, got {got}",
        got = out2.z,
    );
}

#[test]
fn test_transformer_no_geoid_falls_back_silently() {
    // Without `with_geoid`, the transform should NOT fail — it should fall
    // through silently (back-compat with pre-Slice-14 behaviour).
    let src =
        make_compound_with_vert(r#"VERTCRS["EGM96 height",VDATUM["EGM96 geoid"],UNIT["metre",1]]"#);
    let dst = make_compound_with_vert(
        r#"VERTCRS["WGS 84 ellipsoidal height",VDATUM["WGS_1984"],UNIT["metre",1]]"#,
    );

    let transformer = Transformer::new(src, dst).expect("compound transformer");
    assert!(
        transformer.geoid().is_none(),
        "geoid must be None by default"
    );

    let input = Coordinate3D::new(0.0, 0.0, 42.0);
    let out = transformer
        .transform_3d(&input)
        .expect("transform must succeed without geoid");

    // No undulation applied — z passes through unchanged.
    assert!((out.x - input.x).abs() < 1e-9);
    assert!((out.y - input.y).abs() < 1e-9);
    assert!(
        (out.z - input.z).abs() < 1e-9,
        "z should pass through unchanged when no geoid is attached"
    );
}

#[test]
fn test_classify_vertical_datum_recognises_known_strings() {
    // Sanity-check the classifier used by the Transformer dispatch.
    assert_eq!(
        classify_vertical_datum("EGM96 height"),
        VerticalDatumKind::Orthometric
    );
    assert_eq!(
        classify_vertical_datum("EGM2008 height"),
        VerticalDatumKind::Orthometric
    );
    assert_eq!(
        classify_vertical_datum("NAVD88 height"),
        VerticalDatumKind::Orthometric
    );
    assert_eq!(
        classify_vertical_datum("WGS 84 ellipsoidal height"),
        VerticalDatumKind::Ellipsoidal
    );
    assert_eq!(
        classify_vertical_datum("something else entirely"),
        VerticalDatumKind::Unknown
    );
}

#[test]
fn test_geoid_grid_height_at_node_oob_returns_none() {
    let g: GeoidGrid = synthetic_grid(GeoidModel::Egm96);
    assert!(g.height_at_node_m(g.n_lat, 0).is_none());
    assert!(g.height_at_node_m(0, g.n_lon).is_none());
    assert!(g.height_at_node_m(g.n_lat, g.n_lon).is_none());
    // In-bounds returns Some
    assert!(g.height_at_node_m(0, 0).is_some());
}
