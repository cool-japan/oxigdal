//! Integration tests for OGC GeoPackage Extension §F.7 — Tiled Gridded Coverage.
//!
//! Tests are split into two groups:
//! 1. **Pure-Rust function tests** — exercise `unscale_value`,
//!    `unscale_tile_buffer_u16`, `unscale_tile_buffer_i16`, and the two enum
//!    parsers without touching any database I/O.
//! 2. **DB integration tests** — build a minimal SQLite binary in-memory and
//!    verify that `load_gridded_coverages` / `load_gridded_tile_ancillary` behave
//!    correctly when the coverage extension tables are absent.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::panic
)]

use oxigdal_gpkg::{
    CoverageDatatype, GeoPackage, GpkgError, GridCellEncoding, GriddedCoverage,
    TileGriddedAncillary, load_gridded_coverages, load_gridded_tile_ancillary,
    unscale_tile_buffer_i16, unscale_tile_buffer_u16, unscale_value,
};

// ─────────────────────────────────────────────────────────────────────────────
// Low-level SQLite binary builder helpers
// (Self-contained; mirrors the pattern used in metadata_test.rs)
// ─────────────────────────────────────────────────────────────────────────────

use oxigdal_gpkg::btree::encode_sqlite_varint;

/// Build a minimal valid SQLite file header (100 bytes).
fn make_sqlite_header(page_size: u16, db_size_pages: u32) -> Vec<u8> {
    let mut h = vec![0u8; 100];
    h[..16].copy_from_slice(b"SQLite format 3\x00");
    h[16..18].copy_from_slice(&page_size.to_be_bytes());
    h[28..32].copy_from_slice(&db_size_pages.to_be_bytes());
    // text encoding = UTF-8 (1)
    h[56..60].copy_from_slice(&1u32.to_be_bytes());
    // application_id = "GPKG"
    h[68..72].copy_from_slice(&0x4750_4B47u32.to_be_bytes());
    h
}

/// Encode a SQLite record from `(serial_type, value_bytes)` pairs.
fn encode_record_raw(fields: &[(u64, &[u8])]) -> Vec<u8> {
    let st_varints: Vec<Vec<u8>> = fields
        .iter()
        .map(|(st, _)| encode_sqlite_varint(*st))
        .collect();
    let st_bytes: usize = st_varints.iter().map(|v| v.len()).sum();

    let mut hdr_len = st_bytes + 1;
    let hdr_v = encode_sqlite_varint(hdr_len as u64);
    if hdr_v.len() != 1 {
        hdr_len = st_bytes + hdr_v.len();
    }
    let hdr_v = encode_sqlite_varint(hdr_len as u64);

    let mut out = Vec::new();
    out.extend_from_slice(&hdr_v);
    for v in &st_varints {
        out.extend_from_slice(v);
    }
    for (_, bytes) in fields {
        out.extend_from_slice(bytes);
    }
    out
}

/// Text serial type for a string of `n` bytes.
const fn text_serial(n: usize) -> u64 {
    n as u64 * 2 + 13
}

/// Write a leaf table B-tree page.
///
/// `header_offset` is 100 for page 1, 0 for any other page.
fn build_leaf_page(page_size: usize, cells: &[(i64, Vec<u8>)], header_offset: usize) -> Vec<u8> {
    let mut page = vec![0u8; page_size];
    let mut content_end = page_size;
    let mut cell_offsets: Vec<usize> = Vec::with_capacity(cells.len());

    for (rowid, payload) in cells {
        let pl_v = encode_sqlite_varint(payload.len() as u64);
        let rid_v = encode_sqlite_varint(*rowid as u64);
        let cell_size = pl_v.len() + rid_v.len() + payload.len();
        content_end -= cell_size;
        cell_offsets.push(content_end);

        let mut pos = content_end;
        page[pos..pos + pl_v.len()].copy_from_slice(&pl_v);
        pos += pl_v.len();
        page[pos..pos + rid_v.len()].copy_from_slice(&rid_v);
        pos += rid_v.len();
        page[pos..pos + payload.len()].copy_from_slice(payload);
    }

    let hdr = header_offset;
    let cell_count = cells.len();
    page[hdr] = 13; // leaf table page type
    page[hdr + 3] = ((cell_count >> 8) & 0xFF) as u8;
    page[hdr + 4] = (cell_count & 0xFF) as u8;
    let cs = content_end as u16;
    page[hdr + 5] = (cs >> 8) as u8;
    page[hdr + 6] = (cs & 0xFF) as u8;
    let ptr_base = hdr + 8;
    for (i, off) in cell_offsets.iter().enumerate() {
        let o = *off as u16;
        page[ptr_base + i * 2] = (o >> 8) as u8;
        page[ptr_base + i * 2 + 1] = (o & 0xFF) as u8;
    }
    page
}

/// Build a minimal GeoPackage file with no user tables (only empty sqlite_master).
///
/// `load_gridded_coverages` on this file should return `Ok(vec![])`.
fn make_empty_gpkg() -> Vec<u8> {
    let page_size = 4096usize;
    let n_pages = 1u32;
    let mut file = vec![0u8; page_size * n_pages as usize];

    // Page 1: empty sqlite_master leaf (zero rows).
    let empty_master = build_leaf_page(page_size, &[], 100);
    file[..page_size].copy_from_slice(&empty_master);

    // Write the SQLite file header over the first 100 bytes of page 1.
    let hdr = make_sqlite_header(page_size as u16, n_pages);
    file[..100].copy_from_slice(&hdr);

    file
}

/// Append a sqlite_master row describing a user table.
fn append_master_entry(
    master_cells: &mut Vec<(i64, Vec<u8>)>,
    rowid: i64,
    table_name: &str,
    root_page: u8,
) {
    let entry_type = b"table";
    let name = table_name.as_bytes();
    let sql = format!("CREATE TABLE {table_name}(id INTEGER)").into_bytes();
    let rootpage_bytes = [root_page];

    let record = encode_record_raw(&[
        (text_serial(entry_type.len()), entry_type.as_ref()),
        (text_serial(name.len()), name),
        (text_serial(name.len()), name),
        (1u64, &rootpage_bytes), // INTEGER i8
        (text_serial(sql.len()), sql.as_slice()),
    ]);
    master_cells.push((rowid, record));
}

/// Build a GeoPackage that contains both coverage extension tables with one
/// data row each.
///
/// Coverage ancillary row:
/// - id=1, tile_matrix_set_name="dem", datatype="integer", scale=0.01,
///   offset=-100.0, precision=1.0, data_null=NULL, grid_cell_encoding="grid-value-is-center",
///   uom="metre", field_name="Height", quantity_definition=NULL
///
/// Tile ancillary row:
/// - id=1, tpudt_name="dem", tpudt_id=7, scale=1.0, offset=0.0,
///   min=NULL, max=NULL, mean=NULL, std_dev=NULL
fn make_gpkg_with_coverage_tables() -> Vec<u8> {
    let page_size = 4096usize;
    let n_pages = 3u32;
    let mut file = vec![0u8; page_size * n_pages as usize];

    // ── Page 2: gpkg_2d_gridded_coverage_ancillary ───────────────────────────
    let id_bytes = [1u8]; // i8 = 1
    let tms_name = b"dem";
    let datatype = b"integer";
    let scale_bytes = 0.01f64.to_be_bytes();
    let offset_bytes = (-100.0f64).to_be_bytes();
    let precision_bytes = 1.0f64.to_be_bytes();
    let encoding = b"grid-value-is-center";
    let uom = b"metre";
    let field_name = b"Height";

    let cov_record = encode_record_raw(&[
        (1u64, &id_bytes),                           // id  INTEGER i8
        (text_serial(tms_name.len()), tms_name),     // tile_matrix_set_name TEXT
        (text_serial(datatype.len()), datatype),     // datatype TEXT
        (7u64, &scale_bytes),                        // scale REAL (serial type 7)
        (7u64, &offset_bytes),                       // offset REAL
        (7u64, &precision_bytes),                    // precision REAL
        (0u64, &[]),                                 // data_null NULL
        (text_serial(encoding.len()), encoding),     // grid_cell_encoding TEXT
        (text_serial(uom.len()), uom),               // uom TEXT
        (text_serial(field_name.len()), field_name), // field_name TEXT
        (0u64, &[]),                                 // quantity_definition NULL
    ]);
    let cov_page = build_leaf_page(page_size, &[(1, cov_record)], 0);
    file[page_size..page_size * 2].copy_from_slice(&cov_page);

    // ── Page 3: gpkg_2d_gridded_tile_ancillary ───────────────────────────────
    let ta_id_bytes = [1u8];
    let tpudt_name = b"dem";
    let tpudt_id_bytes = [7u8]; // i8 = 7
    let ta_scale = 1.0f64.to_be_bytes();
    let ta_offset = 0.0f64.to_be_bytes();

    let ta_record = encode_record_raw(&[
        (1u64, &ta_id_bytes),                        // id INTEGER i8
        (text_serial(tpudt_name.len()), tpudt_name), // tpudt_name TEXT
        (1u64, &tpudt_id_bytes),                     // tpudt_id INTEGER i8
        (7u64, &ta_scale),                           // scale REAL
        (7u64, &ta_offset),                          // offset REAL
        (0u64, &[]),                                 // min NULL
        (0u64, &[]),                                 // max NULL
        (0u64, &[]),                                 // mean NULL
        (0u64, &[]),                                 // std_dev NULL
    ]);
    let ta_page = build_leaf_page(page_size, &[(1, ta_record)], 0);
    file[page_size * 2..page_size * 3].copy_from_slice(&ta_page);

    // ── Page 1: sqlite_master ────────────────────────────────────────────────
    let mut master_cells: Vec<(i64, Vec<u8>)> = Vec::new();
    append_master_entry(
        &mut master_cells,
        1,
        "gpkg_2d_gridded_coverage_ancillary",
        2,
    );
    append_master_entry(&mut master_cells, 2, "gpkg_2d_gridded_tile_ancillary", 3);

    let master_page = build_leaf_page(page_size, &master_cells, 100);
    file[..page_size].copy_from_slice(&master_page);

    let hdr = make_sqlite_header(page_size as u16, n_pages);
    file[..100].copy_from_slice(&hdr);

    file
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper: build GriddedCoverage structs for pure tests
// ─────────────────────────────────────────────────────────────────────────────

fn make_coverage(scale: f64, offset: f64, data_null: Option<f64>) -> GriddedCoverage {
    GriddedCoverage {
        table_name: "dem".to_string(),
        datatype: CoverageDatatype::Integer,
        scale,
        offset,
        precision: 1.0,
        data_null,
        grid_cell_encoding: GridCellEncoding::Grid,
        uom: None,
        field_name: "Height".to_string(),
        quantity_definition: None,
    }
}

fn make_tile_ancillary(scale: f64, offset: f64) -> TileGriddedAncillary {
    TileGriddedAncillary {
        id: 1,
        tpudt_id: 42,
        scale,
        offset,
        min: None,
        max: None,
        mean: None,
        std_dev: None,
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Pure-Rust function tests (no database I/O)
// ═════════════════════════════════════════════════════════════════════════════

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — CoverageDatatype parses "integer" and "float"
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_coverage_datatype_parses_integer_and_float() {
    assert_eq!(
        "integer".parse::<CoverageDatatype>().expect("integer"),
        CoverageDatatype::Integer
    );
    assert_eq!(
        "float".parse::<CoverageDatatype>().expect("float"),
        CoverageDatatype::Float
    );
    // Round-trip via as_str
    assert_eq!(CoverageDatatype::Integer.as_str(), "integer");
    assert_eq!(CoverageDatatype::Float.as_str(), "float");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — CoverageDatatype::from_str("raster") → Err(InvalidCoverageDatatype)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_coverage_datatype_invalid_returns_error() {
    let err = "raster".parse::<CoverageDatatype>().unwrap_err();
    assert!(
        matches!(err, GpkgError::InvalidCoverageDatatype(ref s) if s == "raster"),
        "unexpected error variant: {err:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — GridCellEncoding parses all three canonical strings
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_grid_cell_encoding_parses_three_variants() {
    assert_eq!(
        "grid-value-is-center"
            .parse::<GridCellEncoding>()
            .expect("center"),
        GridCellEncoding::Grid
    );
    assert_eq!(
        "grid-value-is-area"
            .parse::<GridCellEncoding>()
            .expect("area"),
        GridCellEncoding::PixelIsArea
    );
    assert_eq!(
        "grid-value-is-corner"
            .parse::<GridCellEncoding>()
            .expect("corner"),
        GridCellEncoding::PixelIsPoint
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — unscale_value with scale=1.0 offset=0.0 is a passthrough
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_unscale_value_default_scale_one_offset_zero_passthrough() {
    let cov = make_coverage(1.0, 0.0, None);
    let phys = unscale_value(42.0, &cov, None);
    assert!((phys - 42.0).abs() < 1e-10, "expected 42.0, got {phys}");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — unscale_value with scale=0.1, offset=-100.0: raw=1000.0 → phys=0.0
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_unscale_value_with_scale_and_offset_applies_correctly() {
    let cov = make_coverage(0.1, -100.0, None);
    let phys = unscale_value(1000.0, &cov, None);
    assert!(phys.abs() < 1e-10, "expected 0.0, got {phys}");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — tile_ancillary overrides coverage scale
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_unscale_value_tile_ancillary_overrides_coverage_scale() {
    // coverage: scale=1.0, offset=0.0; tile: scale=2.0, offset=5.0
    // raw=10.0 → phys = 10*2 + 5 = 25.0
    let cov = make_coverage(1.0, 0.0, None);
    let ta = make_tile_ancillary(2.0, 5.0);
    let phys = unscale_value(10.0, &cov, Some(&ta));
    assert!((phys - 25.0).abs() < 1e-10, "expected 25.0, got {phys}");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — raw == data_null → NAN
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_unscale_value_data_null_returns_nan() {
    // data_null sentinel = 0.0; raw=0.0 → NAN
    let cov = make_coverage(1.0, 0.0, Some(0.0));
    let result = unscale_value(0.0, &cov, None);
    assert!(result.is_nan(), "expected NAN, got {result}");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8 — unscale_tile_buffer_u16: identity transform
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_unscale_tile_buffer_u16_round_trip_preserves_values() {
    let cov = make_coverage(1.0, 0.0, None);
    let raw = [0u16, 1u16, 65535u16];
    let out = unscale_tile_buffer_u16(&raw, &cov, None);

    assert_eq!(out.len(), 3);
    assert!((out[0] - 0.0).abs() < 1e-10, "index 0: got {}", out[0]);
    assert!((out[1] - 1.0).abs() < 1e-10, "index 1: got {}", out[1]);
    assert!((out[2] - 65535.0).abs() < 1e-10, "index 2: got {}", out[2]);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9 — unscale_tile_buffer_i16: negative elevations preserved
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_unscale_tile_buffer_i16_negative_elevations_preserved() {
    let cov = make_coverage(1.0, 0.0, None);
    let raw = [-100i16, 0i16, 100i16];
    let out = unscale_tile_buffer_i16(&raw, &cov, None);

    assert_eq!(out.len(), 3);
    assert!((out[0] - (-100.0)).abs() < 1e-10, "index 0: got {}", out[0]);
    assert!((out[1] - 0.0).abs() < 1e-10, "index 1: got {}", out[1]);
    assert!((out[2] - 100.0).abs() < 1e-10, "index 2: got {}", out[2]);
}

// ═════════════════════════════════════════════════════════════════════════════
// DB integration tests
// ═════════════════════════════════════════════════════════════════════════════

// ─────────────────────────────────────────────────────────────────────────────
// Test 10 — load_gridded_coverages on a standard GPKG returns Ok(vec![])
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_gridded_coverage_load_empty_returns_empty_vec() {
    // A GPKG that has no coverage extension tables at all.
    let data = make_empty_gpkg();
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");

    let coverages = load_gridded_coverages(&gpkg).expect("load ok");
    assert!(
        coverages.is_empty(),
        "expected empty vec when coverage tables absent, got {coverages:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 11 — load_gridded_tile_ancillary on a standard GPKG returns Ok(vec![])
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_gridded_tile_ancillary_load_empty_returns_empty_vec() {
    let data = make_empty_gpkg();
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");

    let rows = load_gridded_tile_ancillary(&gpkg, "dem").expect("load ok");
    assert!(
        rows.is_empty(),
        "expected empty vec when tile ancillary table absent, got {rows:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 12 — load_gridded_coverages parses a real ancillary row
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_gridded_coverages_parses_single_row() {
    let data = make_gpkg_with_coverage_tables();
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");

    let coverages = load_gridded_coverages(&gpkg).expect("load ok");
    assert_eq!(
        coverages.len(),
        1,
        "expected 1 coverage row, got {}",
        coverages.len()
    );

    let cov = &coverages[0];
    assert_eq!(cov.table_name, "dem");
    assert_eq!(cov.datatype, CoverageDatatype::Integer);
    assert!((cov.scale - 0.01).abs() < 1e-10, "scale: got {}", cov.scale);
    assert!(
        (cov.offset - (-100.0)).abs() < 1e-10,
        "offset: got {}",
        cov.offset
    );
    assert_eq!(cov.grid_cell_encoding, GridCellEncoding::Grid);
    assert_eq!(cov.uom.as_deref(), Some("metre"));
    assert_eq!(cov.field_name, "Height");
    assert!(cov.data_null.is_none(), "data_null should be None");
    assert!(
        cov.quantity_definition.is_none(),
        "quantity_definition should be None"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 13 — load_gridded_tile_ancillary filters by tpudt_name
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_gridded_tile_ancillary_filters_by_table_name() {
    let data = make_gpkg_with_coverage_tables();
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");

    // "dem" has one row.
    let rows = load_gridded_tile_ancillary(&gpkg, "dem").expect("load ok");
    assert_eq!(
        rows.len(),
        1,
        "expected 1 tile ancillary row, got {}",
        rows.len()
    );

    let row = &rows[0];
    assert_eq!(row.id, 1);
    assert_eq!(row.tpudt_id, 7);
    assert!((row.scale - 1.0).abs() < 1e-10, "scale: got {}", row.scale);
    assert!(
        (row.offset - 0.0).abs() < 1e-10,
        "offset: got {}",
        row.offset
    );

    // Requesting a non-existent table returns empty.
    let other = load_gridded_tile_ancillary(&gpkg, "nonexistent").expect("load ok");
    assert!(
        other.is_empty(),
        "expected no rows for non-existent table, got {other:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 14 — unscale_value end-to-end with DB-loaded coverage
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_unscale_value_end_to_end_with_loaded_coverage() {
    let data = make_gpkg_with_coverage_tables();
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");

    let coverages = load_gridded_coverages(&gpkg).expect("load_coverages ok");
    let cov = &coverages[0]; // scale=0.01, offset=-100.0

    // raw=10000 → phys = 10000 * 0.01 + (-100) = 0.0
    let phys = unscale_value(10000.0, cov, None);
    assert!(
        phys.abs() < 1e-8,
        "expected ~0.0 from raw=10000 with DB coverage, got {phys}"
    );

    // raw=15000 → phys = 150 - 100 = 50.0
    let phys2 = unscale_value(15000.0, cov, None);
    assert!(
        (phys2 - 50.0).abs() < 1e-8,
        "expected ~50.0 from raw=15000, got {phys2}"
    );
}
