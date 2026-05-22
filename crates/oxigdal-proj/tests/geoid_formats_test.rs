//! Integration tests for the EGM geoid distribution-format parsers
//! (Slice 23 W1).
//!
//! Covers the EGM96 ASCII (`WW15MGH.GRD`) header/body parser, the EGM2008
//! 2.5′ self-consistent binary container, file-input round-trips via temp
//! files, and the conservative `load_geoid_auto` format sniffer.
//!
//! Per project policy these tests use `std::env::temp_dir()` with
//! uniquely-named scratch files rather than an external tempfile crate.

#![cfg(feature = "std")]
#![allow(clippy::expect_used)]

use std::io::Write;

use oxigdal_proj::{
    Egm2008BinaryHeader, GeoidModel, load_geoid_auto, parse_egm96_ascii_file,
    parse_egm96_ascii_header, parse_egm96_ascii_str, parse_egm2008_binary_25,
    parse_egm2008_binary_25_file, parse_egm2008_binary_25_header,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Writes `bytes` to a uniquely-named file inside `std::env::temp_dir()` and
/// returns its path.  The caller is responsible for removing it.
fn write_tempfile(name: &str, ext: &str, bytes: &[u8]) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    path.push(format!("oxigdal_geoidfmt_{name}_{pid}_{nanos}.{ext}"));
    let mut f = std::fs::File::create(&path).expect("create tempfile");
    f.write_all(bytes).expect("write tempfile");
    f.flush().expect("flush tempfile");
    path
}

/// Serialises an EGM2008 binary container: 8-byte little-endian header
/// (`n_lat`, `n_lon`) followed by row-major little-endian `f32` body.
fn encode_egm2008_binary(n_lat: u32, n_lon: u32, heights: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + heights.len() * 4);
    out.extend_from_slice(&n_lat.to_le_bytes());
    out.extend_from_slice(&n_lon.to_le_bytes());
    for v in heights {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// 1–6: EGM96 ASCII
// ---------------------------------------------------------------------------

#[test]
fn test_parse_egm96_ascii_header_canonical_form() {
    // Canonical WW15MGH.GRD header: south north west east dlat dlon.
    let header = parse_egm96_ascii_header("-90.0 90.0 0.0 360.0 0.25 0.25")
        .expect("canonical header parses");
    assert!((header.lat_min - -90.0).abs() < f64::EPSILON);
    assert!((header.lat_max - 90.0).abs() < f64::EPSILON);
    assert!((header.lon_min - 0.0).abs() < f64::EPSILON);
    assert!((header.lon_max - 360.0).abs() < f64::EPSILON);
    assert!((header.lat_step - 0.25).abs() < f64::EPSILON);
    assert!((header.lon_step - 0.25).abs() < f64::EPSILON);
    // 180 / 0.25 + 1 = 721 rows; 360 / 0.25 + 1 = 1441 cols (the real
    // WW15MGH.GRD dimensions).
    assert_eq!(header.n_lat, 721);
    assert_eq!(header.n_lon, 1441);
}

#[test]
fn test_parse_egm96_ascii_header_rejects_truncated() {
    // Only five numbers — missing dlon.
    let result = parse_egm96_ascii_header("-90.0 90.0 0.0 360.0 0.25");
    let err = result.expect_err("truncated header must error");
    let msg = format!("{err}");
    assert!(
        msg.to_lowercase().contains("truncat") || msg.to_lowercase().contains("missing"),
        "error should describe the truncation: {msg}"
    );
}

#[test]
fn test_parse_egm96_ascii_str_3x3_round_trip() {
    // 3×3 grid spanning lat [-1,1], lon [-1,1] at 1° spacing.
    let text = "\
-1.0 1.0 -1.0 1.0 1.0 1.0
0.0 1.0 2.0
3.0 4.0 5.0
6.0 7.0 8.0
";
    let grid = parse_egm96_ascii_str(text).expect("3x3 grid parses");
    assert_eq!(grid.model, GeoidModel::Egm96);
    assert_eq!(grid.n_lat, 3);
    assert_eq!(grid.n_lon, 3);
    assert_eq!(grid.heights_m.len(), 9);
    // Row-major: row 0 (lat_min = -1) is [0,1,2]; node (i=1,j=1) is centre = 4.
    assert!((grid.height_at_node_m(0, 0).expect("node") - 0.0).abs() < 1e-6);
    assert!((grid.height_at_node_m(1, 1).expect("node") - 4.0).abs() < 1e-6);
    assert!((grid.height_at_node_m(2, 2).expect("node") - 8.0).abs() < 1e-6);
    assert!((grid.lat_step_deg - 1.0).abs() < f64::EPSILON);
    assert!((grid.lon_step_deg - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_parse_egm96_ascii_str_count_mismatch_returns_error() {
    // Header declares 3×3 = 9 values but only 8 are provided.
    let text = "\
-1.0 1.0 -1.0 1.0 1.0 1.0
0 1 2
3 4 5
6 7
";
    let err = parse_egm96_ascii_str(text).expect_err("count mismatch must error");
    let msg = format!("{err}");
    assert!(
        msg.contains('8') && msg.contains('9'),
        "error should report 8 actual vs 9 expected: {msg}"
    );
}

#[test]
fn test_parse_egm96_ascii_str_handles_extra_whitespace() {
    // Leading/trailing blank lines, irregular interior spacing and tabs.
    let text =
        "\n   \n   -1.0   1.0\t-1.0  1.0   1.0   1.0   \n\n  0   1\t2  \n 3 4 5 \n\t6 7 8\n\n";
    let grid = parse_egm96_ascii_str(text).expect("whitespace-tolerant parse");
    assert_eq!(grid.n_lat, 3);
    assert_eq!(grid.n_lon, 3);
    assert_eq!(grid.heights_m.len(), 9);
    assert!((grid.height_at_node_m(2, 2).expect("node") - 8.0).abs() < 1e-6);
}

#[test]
fn test_parse_egm96_ascii_file_via_tempfile() {
    let text = "\
-2.0 2.0 -2.0 2.0 2.0 2.0
10 11 12
13 14 15
16 17 18
";
    let path = write_tempfile("egm96", "grd", text.as_bytes());
    let result = parse_egm96_ascii_file(&path);
    let _ = std::fs::remove_file(&path);
    let grid = result.expect("file parse succeeds");
    assert_eq!(grid.n_lat, 3);
    assert_eq!(grid.n_lon, 3);
    assert!((grid.height_at_node_m(0, 0).expect("node") - 10.0).abs() < 1e-6);
    assert!((grid.height_at_node_m(2, 1).expect("node") - 17.0).abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// 7–11: EGM2008 binary
// ---------------------------------------------------------------------------

#[test]
fn test_parse_egm2008_binary_25_header_canonical_dimensions() {
    // Canonical 2.5′ grid: 4321 × 8640.
    let bytes = encode_egm2008_binary(4321, 8640, &[]);
    let header = parse_egm2008_binary_25_header(&bytes).expect("canonical header parses");
    assert_eq!(
        header,
        Egm2008BinaryHeader {
            n_lat: 4321,
            n_lon: 8640
        }
    );
}

#[test]
fn test_parse_egm2008_binary_25_header_rejects_zero_dim() {
    // n_lon = 0 must be rejected.
    let bytes = encode_egm2008_binary(4321, 0, &[]);
    let err = parse_egm2008_binary_25_header(&bytes).expect_err("zero dimension must error");
    let msg = format!("{err}");
    assert!(
        msg.to_lowercase().contains("non-zero") || msg.to_lowercase().contains("zero"),
        "error should mention the zero dimension: {msg}"
    );
}

#[test]
fn test_parse_egm2008_binary_25_3x3_synthetic_grid() {
    let heights: Vec<f32> = (0..9).map(|i| i as f32 * 0.5).collect();
    let bytes = encode_egm2008_binary(3, 3, &heights);
    let grid = parse_egm2008_binary_25(&bytes).expect("3x3 binary grid parses");
    assert_eq!(grid.model, GeoidModel::Egm2008);
    assert_eq!(grid.n_lat, 3);
    assert_eq!(grid.n_lon, 3);
    assert_eq!(grid.heights_m.len(), 9);
    // Geometry: 180/2 = 90° lat step, 360/3 = 120° lon step, origin (-90,-180).
    assert!((grid.lat_step_deg - 90.0).abs() < 1e-12);
    assert!((grid.lon_step_deg - 120.0).abs() < 1e-12);
    assert!((grid.lat_min_deg - -90.0).abs() < f64::EPSILON);
    assert!((grid.lon_min_deg - -180.0).abs() < f64::EPSILON);
    // Body preserved row-major.
    assert!((grid.height_at_node_m(2, 2).expect("node") - 4.0).abs() < 1e-6);
}

#[test]
fn test_parse_egm2008_binary_25_size_mismatch_returns_error() {
    // Header says 3×3 (needs 8 + 36 = 44 bytes) but supply only 8 + 20 bytes.
    let mut bytes = encode_egm2008_binary(3, 3, &[]);
    bytes.extend_from_slice(&[0u8; 20]);
    let err = parse_egm2008_binary_25(&bytes).expect_err("size mismatch must error");
    let msg = format!("{err}");
    assert!(
        msg.contains("44") && msg.contains("28"),
        "error should report 28 actual vs 44 expected bytes: {msg}"
    );
}

#[test]
fn test_parse_egm2008_binary_25_round_trip_height_lookup() {
    // Build a small but non-trivial 5×8 grid, serialise, parse, and confirm a
    // sampled height matches the value we stored at that exact node.
    let n_lat = 5u32;
    let n_lon = 8u32;
    let mut heights: Vec<f32> = Vec::with_capacity((n_lat * n_lon) as usize);
    for i in 0..n_lat {
        for j in 0..n_lon {
            heights.push((i as f32) * 10.0 + (j as f32));
        }
    }
    let bytes = encode_egm2008_binary(n_lat, n_lon, &heights);
    let grid = parse_egm2008_binary_25(&bytes).expect("5x8 grid parses");

    // lat_step = 180/4 = 45°, lon_step = 360/8 = 45°, origin (-90, -180).
    // Sample exactly at node (i=2, j=3): lat = -90 + 2*45 = 0°,
    // lon = -180 + 3*45 = -45°.  Stored value = 2*10 + 3 = 23.
    let lat = -90.0 + 2.0 * 45.0;
    let lon = -180.0 + 3.0 * 45.0;
    let sampled = grid.geoid_height_m(lat, lon);
    let stored = grid.height_at_node_m(2, 3).expect("node in bounds");
    assert!(
        (stored - 23.0).abs() < 1e-6,
        "stored node value should be 23, got {stored}"
    );
    assert!(
        (sampled - 23.0).abs() < 1e-4,
        "height sampled at node (0°,-45°) should equal stored 23, got {sampled}"
    );

    // Round-trip through a temp file too.
    let path = write_tempfile("egm2008", "bin", &bytes);
    let from_file = parse_egm2008_binary_25_file(&path);
    let _ = std::fs::remove_file(&path);
    let grid2 = from_file.expect("file parse succeeds");
    assert_eq!(grid2.heights_m, grid.heights_m);
}

// ---------------------------------------------------------------------------
// 12–13: auto-detection
// ---------------------------------------------------------------------------

#[test]
fn test_load_geoid_auto_detects_egm96_ascii() {
    let text = "\
-1.0 1.0 -1.0 1.0 1.0 1.0
0 1 2
3 4 5
6 7 8
";
    let path = write_tempfile("auto_ascii", "grd", text.as_bytes());
    let result = load_geoid_auto(&path);
    let _ = std::fs::remove_file(&path);
    let grid = result.expect("auto-detect ASCII");
    assert_eq!(grid.model, GeoidModel::Egm96);
    assert_eq!(grid.n_lat, 3);
    assert_eq!(grid.n_lon, 3);
}

#[test]
fn test_load_geoid_auto_detects_egm2008_binary() {
    let heights: Vec<f32> = (0..9).map(|i| i as f32).collect();
    let bytes = encode_egm2008_binary(3, 3, &heights);
    let path = write_tempfile("auto_bin", "bin", &bytes);
    let result = load_geoid_auto(&path);
    let _ = std::fs::remove_file(&path);
    let grid = result.expect("auto-detect binary");
    assert_eq!(grid.model, GeoidModel::Egm2008);
    assert_eq!(grid.n_lat, 3);
    assert_eq!(grid.n_lon, 3);
}
