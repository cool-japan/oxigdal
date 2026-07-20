//! Integration tests for the GeoPackage file-integrity validator
//! (`oxigeo_gpkg::integrity`).
//!
//! These tests construct full GeoPackage byte buffers — using
//! [`GeoPackageBuilder`] where possible and lower-level B-tree page builders
//! when targeted corruption is needed — and run [`check_integrity`] /
//! [`check_integrity_strict`] against them, asserting that every category of
//! issue specified in OGC 12-128r19 §1.1.3 is detected.

#![allow(clippy::expect_used, clippy::panic)]

use oxigeo_gpkg::btree::encode_sqlite_varint;
use oxigeo_gpkg::{
    GPKG_APP_ID, GeoPackage, GeoPackageBuilder, IntegrityIssue, MIN_USER_VERSION, check_integrity,
    check_integrity_strict,
};

// ─────────────────────────────────────────────────────────────────────────────
// SQLite page helpers (mirrors patterns from extensions_test.rs)
// ─────────────────────────────────────────────────────────────────────────────

/// Build a minimal SQLite header (100 bytes) with the GeoPackage application_id
/// and a 1.3.0 `user_version` so the header-level checks pass by default.
fn make_sqlite_header(page_size_raw: u16, db_size_pages: u32) -> Vec<u8> {
    let mut data = vec![0u8; 100];
    data[..16].copy_from_slice(b"SQLite format 3\x00");
    data[16..18].copy_from_slice(&page_size_raw.to_be_bytes());
    // file format read/write versions, payload fractions, change counter (1)
    data[18] = 1;
    data[19] = 1;
    data[21] = 64;
    data[22] = 32;
    data[23] = 32;
    data[24..28].copy_from_slice(&1u32.to_be_bytes());
    data[28..32].copy_from_slice(&db_size_pages.to_be_bytes());
    data[40..44].copy_from_slice(&1u32.to_be_bytes());
    data[44] = 4; // schema format
    // text encoding = UTF-8 (1)
    data[56..60].copy_from_slice(&1u32.to_be_bytes());
    // user_version = 10_300 (1.3.0)
    data[60..64].copy_from_slice(&10_300u32.to_be_bytes());
    // GeoPackage application_id
    data[68..72].copy_from_slice(&0x4750_4B47u32.to_be_bytes());
    // version-valid-for + sqlite version
    data[92..96].copy_from_slice(&1u32.to_be_bytes());
    data[96..100].copy_from_slice(&3_040_001u32.to_be_bytes());
    data
}

fn write_sqlite_file_header(file_data: &mut [u8], page_size: u16, db_size_pages: u32) {
    let hdr = make_sqlite_header(page_size, db_size_pages);
    file_data[..100].copy_from_slice(&hdr);
}

/// Encode a SQLite record: header-length varint + serial-type varints + value bytes.
fn encode_record(fields: &[(u64, &[u8])]) -> Vec<u8> {
    let serial_type_varints: Vec<Vec<u8>> = fields
        .iter()
        .map(|(st, _)| encode_sqlite_varint(*st))
        .collect();
    let st_bytes: usize = serial_type_varints.iter().map(|v| v.len()).sum();
    let mut hdr_len = st_bytes + 1;
    let hdr_varint = encode_sqlite_varint(hdr_len as u64);
    if hdr_varint.len() != 1 {
        hdr_len = st_bytes + hdr_varint.len();
    }
    let hdr_varint = encode_sqlite_varint(hdr_len as u64);

    let mut out = Vec::new();
    out.extend_from_slice(&hdr_varint);
    for v in &serial_type_varints {
        out.extend_from_slice(v);
    }
    for (_, bytes) in fields {
        out.extend_from_slice(bytes);
    }
    out
}

/// Build a leaf table B-tree page (page-type 13) containing `cells`.
fn build_leaf_table_page(
    page_size: usize,
    cells: &[(i64, &[u8])],
    header_offset: usize,
) -> Vec<u8> {
    let mut page = vec![0u8; page_size];
    let cell_count = cells.len();
    let mut content_end = page_size;
    let mut cell_offsets: Vec<usize> = Vec::with_capacity(cell_count);

    for (rowid, payload) in cells {
        let pl_varint = encode_sqlite_varint(payload.len() as u64);
        let rid_varint = encode_sqlite_varint(*rowid as u64);
        let cell_size = pl_varint.len() + rid_varint.len() + payload.len();
        content_end -= cell_size;
        let start = content_end;
        cell_offsets.push(start);
        let mut pos = start;
        page[pos..pos + pl_varint.len()].copy_from_slice(&pl_varint);
        pos += pl_varint.len();
        page[pos..pos + rid_varint.len()].copy_from_slice(&rid_varint);
        pos += rid_varint.len();
        page[pos..pos + payload.len()].copy_from_slice(payload);
    }

    let hdr = header_offset;
    page[hdr] = 13; // leaf table B-tree
    page[hdr + 3] = ((cell_count >> 8) & 0xFF) as u8;
    page[hdr + 4] = (cell_count & 0xFF) as u8;
    let content_start = content_end as u16;
    page[hdr + 5] = ((content_start >> 8) & 0xFF) as u8;
    page[hdr + 6] = (content_start & 0xFF) as u8;
    let ptr_start = hdr + 8;
    for (i, offset) in cell_offsets.iter().enumerate() {
        let o = *offset as u16;
        page[ptr_start + i * 2] = ((o >> 8) & 0xFF) as u8;
        page[ptr_start + i * 2 + 1] = (o & 0xFF) as u8;
    }
    page
}

/// Compute the TEXT serial type for a byte-slice of length `len`:
/// serial_type = len * 2 + 13.
fn text_serial_type(len: usize) -> u64 {
    (len as u64) * 2 + 13
}

/// Build a sqlite_master record for a single table entry pointing to `rootpage`.
fn build_master_record(table_name: &str, rootpage: u8, sql: &str) -> Vec<u8> {
    let entry_type = b"table".to_vec();
    let name = table_name.as_bytes().to_vec();
    let tbl_name = table_name.as_bytes().to_vec();
    let sql_bytes = sql.as_bytes().to_vec();
    let rootpage_bytes = [rootpage];
    let fields: Vec<(u64, &[u8])> = vec![
        (text_serial_type(entry_type.len()), &entry_type),
        (text_serial_type(name.len()), &name),
        (text_serial_type(tbl_name.len()), &tbl_name),
        (1u64, &rootpage_bytes), // serial type 1 = i8 integer
        (text_serial_type(sql_bytes.len()), &sql_bytes),
    ];
    encode_record(&fields)
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: a minimal valid GeoPackage passes every check
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_integrity_minimal_valid_gpkg_passes() {
    let bytes = GeoPackageBuilder::new(4326)
        .build()
        .expect("builder must succeed");
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse builder output");

    let report = check_integrity(&gpkg);
    assert!(
        report.passed,
        "builder-produced GeoPackage should pass integrity but had issues: {:#?}",
        report.issues
    );
    assert!(
        report.issues.is_empty(),
        "no issues should be reported, found {}",
        report.issue_count()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: feature-table GeoPackage from the builder also passes
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_integrity_feature_table_gpkg_passes() {
    let bytes = GeoPackageBuilder::new(4326)
        .add_feature_table("cities", "POINT", vec![(1, 139.7, 35.7)])
        .build()
        .expect("builder");
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    let report = check_integrity(&gpkg);
    assert!(
        report.passed,
        "GeoPackage with feature table should pass; issues: {:#?}",
        report.issues
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: missing gpkg_spatial_ref_sys is detected as MissingRequiredTable
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_integrity_missing_spatial_ref_sys_fails() {
    // Build a GeoPackage with an EMPTY sqlite_master — no system tables at all.
    let page_size = 4096usize;
    let mut file = vec![0u8; page_size];
    let master_page = build_leaf_table_page(page_size, &[], 100);
    file[..page_size].copy_from_slice(&master_page);
    write_sqlite_file_header(&mut file, page_size as u16, 1);

    let gpkg = GeoPackage::from_bytes(file).expect("parse");
    let report = check_integrity(&gpkg);

    assert!(!report.passed, "missing SRS table must fail integrity");
    assert!(
        report.has_issue_of(|i| matches!(
            i,
            IntegrityIssue::MissingRequiredTable(name) if name == "gpkg_spatial_ref_sys"
        )),
        "expected MissingRequiredTable for gpkg_spatial_ref_sys, got {:#?}",
        report.issues
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: contents row pointing at a non-existent table is detected
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_integrity_contents_references_missing_table_detected() {
    // Build a 4-page file:
    //  - Page 1: sqlite_master containing gpkg_spatial_ref_sys (page 2) and
    //            gpkg_contents (page 3).  Notably it does NOT register a
    //            user table called "phantom_layer".
    //  - Page 2: gpkg_spatial_ref_sys with the three default SRS rows.
    //  - Page 3: gpkg_contents with a single row that points at
    //            "phantom_layer" — a deliberately dangling reference.
    //  - Page 4: padding.
    let page_size = 4096usize;
    let total_pages = 4usize;
    let mut file = vec![0u8; page_size * total_pages];

    // Page 2: gpkg_spatial_ref_sys with the three default SRS rows.
    let srs_rows = build_default_srs_rows();
    let srs_cells: Vec<(i64, &[u8])> = srs_rows
        .iter()
        .enumerate()
        .map(|(i, p)| (i as i64 + 1, p.as_slice()))
        .collect();
    let srs_page = build_leaf_table_page(page_size, &srs_cells, 0);
    file[page_size..page_size * 2].copy_from_slice(&srs_page);

    // Page 3: gpkg_contents row referencing "phantom_layer".
    let contents_payload = encode_contents_row("phantom_layer", "features", 4326);
    let contents_cells: Vec<(i64, &[u8])> = vec![(1, contents_payload.as_slice())];
    let contents_page = build_leaf_table_page(page_size, &contents_cells, 0);
    file[page_size * 2..page_size * 3].copy_from_slice(&contents_page);

    // Page 1: sqlite_master listing gpkg_spatial_ref_sys (page 2) and
    // gpkg_contents (page 3).
    let srs_master = build_master_record(
        "gpkg_spatial_ref_sys",
        2,
        "CREATE TABLE gpkg_spatial_ref_sys(\
            srs_name TEXT,srs_id INTEGER PRIMARY KEY,\
            organization TEXT,organization_coordsys_id INTEGER,\
            definition TEXT,description TEXT)",
    );
    let contents_master = build_master_record(
        "gpkg_contents",
        3,
        "CREATE TABLE gpkg_contents(\
            table_name TEXT PRIMARY KEY,data_type TEXT,identifier TEXT,\
            description TEXT,last_change DATETIME,\
            min_x DOUBLE,min_y DOUBLE,max_x DOUBLE,max_y DOUBLE,\
            srs_id INTEGER)",
    );
    let master_cells: Vec<(i64, &[u8])> =
        vec![(1, srs_master.as_slice()), (2, contents_master.as_slice())];
    let master_page = build_leaf_table_page(page_size, &master_cells, 100);
    file[..page_size].copy_from_slice(&master_page);

    write_sqlite_file_header(&mut file, page_size as u16, total_pages as u32);

    let gpkg = GeoPackage::from_bytes(file).expect("parse");
    let report = check_integrity(&gpkg);
    assert!(
        !report.passed,
        "dangling contents reference should fail integrity"
    );
    assert!(
        report.has_issue_of(|i| matches!(
            i,
            IntegrityIssue::ContentsRefsMissingTable { table_name }
                if table_name == "phantom_layer"
        )),
        "expected ContentsRefsMissingTable for phantom_layer; got {:#?}",
        report.issues
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5: geometry_columns with an unknown srs_id is detected
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_integrity_geometry_column_srs_id_missing_detected() {
    // Build a 5-page file:
    //  - Page 1: sqlite_master with SRS (p2), contents (p3), geom_cols (p4),
    //            the "cities" feature table (p5).
    //  - Page 2: SRS with the three default rows.
    //  - Page 3: contents row for "cities".
    //  - Page 4: geometry_columns row for "cities" with srs_id = 99999.
    //  - Page 5: empty feature table.
    let page_size = 4096usize;
    let total_pages = 5usize;
    let mut file = vec![0u8; page_size * total_pages];

    // Page 2: SRS rows.
    let srs_rows = build_default_srs_rows();
    let srs_cells: Vec<(i64, &[u8])> = srs_rows
        .iter()
        .enumerate()
        .map(|(i, p)| (i as i64 + 1, p.as_slice()))
        .collect();
    let srs_page = build_leaf_table_page(page_size, &srs_cells, 0);
    file[page_size..page_size * 2].copy_from_slice(&srs_page);

    // Page 3: contents row for "cities".
    let contents_payload = encode_contents_row("cities", "features", 4326);
    let contents_cells: Vec<(i64, &[u8])> = vec![(1, contents_payload.as_slice())];
    let contents_page = build_leaf_table_page(page_size, &contents_cells, 0);
    file[page_size * 2..page_size * 3].copy_from_slice(&contents_page);

    // Page 4: geometry_columns with srs_id = 99999 (not in spatial_ref_sys).
    let geom_payload = encode_geometry_columns_row("cities", "geom", "POINT", 99999);
    let geom_cells: Vec<(i64, &[u8])> = vec![(1, geom_payload.as_slice())];
    let geom_page = build_leaf_table_page(page_size, &geom_cells, 0);
    file[page_size * 3..page_size * 4].copy_from_slice(&geom_page);

    // Page 5: empty user feature table.
    let cities_page = build_leaf_table_page(page_size, &[], 0);
    file[page_size * 4..page_size * 5].copy_from_slice(&cities_page);

    // Page 1: sqlite_master.
    let master_cells_owned: Vec<Vec<u8>> = vec![
        build_master_record(
            "gpkg_spatial_ref_sys",
            2,
            "CREATE TABLE gpkg_spatial_ref_sys(srs_id INTEGER)",
        ),
        build_master_record(
            "gpkg_contents",
            3,
            "CREATE TABLE gpkg_contents(table_name TEXT)",
        ),
        build_master_record(
            "gpkg_geometry_columns",
            4,
            "CREATE TABLE gpkg_geometry_columns(table_name TEXT)",
        ),
        build_master_record("cities", 5, "CREATE TABLE cities(fid INTEGER PRIMARY KEY)"),
    ];
    let master_cells: Vec<(i64, &[u8])> = master_cells_owned
        .iter()
        .enumerate()
        .map(|(i, m)| (i as i64 + 1, m.as_slice()))
        .collect();
    let master_page = build_leaf_table_page(page_size, &master_cells, 100);
    file[..page_size].copy_from_slice(&master_page);

    write_sqlite_file_header(&mut file, page_size as u16, total_pages as u32);

    let gpkg = GeoPackage::from_bytes(file).expect("parse");
    let report = check_integrity(&gpkg);
    assert!(
        !report.passed,
        "geometry_columns with bad srs_id should fail integrity"
    );
    assert!(
        report.has_issue_of(|i| matches!(
            i,
            IntegrityIssue::GeometryColumnsRefsMissingSrs { srs_id, .. } if *srs_id == 99999
        )),
        "expected GeometryColumnsRefsMissingSrs with srs_id=99999, got {:#?}",
        report.issues
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6: AppIdMismatch — corrupt the application_id byte
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_integrity_app_id_mismatch_detected() {
    let mut bytes = GeoPackageBuilder::new(4326).build().expect("builder");
    // Overwrite the application_id at offset 68 with a non-GPKG value.
    bytes[68..72].copy_from_slice(&0x1234_5678u32.to_be_bytes());

    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");
    let report = check_integrity(&gpkg);

    assert!(!report.passed, "bad app_id must fail integrity");
    assert!(
        report.has_issue_of(|i| matches!(
            i,
            IntegrityIssue::AppIdMismatch { actual } if *actual == 0x1234_5678
        )),
        "expected AppIdMismatch with actual 0x12345678, got {:#?}",
        report.issues
    );
    // GPKG_APP_ID constant should be the canonical value.
    assert_eq!(GPKG_APP_ID, 0x4750_4B47);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7: UserVersionTooOld — set user_version to something before 1.3.0
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_integrity_user_version_too_old_detected() {
    let mut bytes = GeoPackageBuilder::new(4326).build().expect("builder");
    // Overwrite the user_version at offset 60 with 10_200 (1.2.0 → too old).
    bytes[60..64].copy_from_slice(&10_200u32.to_be_bytes());

    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");
    let report = check_integrity(&gpkg);

    assert!(!report.passed, "old user_version must fail integrity");
    assert!(
        report.has_issue_of(|i| matches!(
            i,
            IntegrityIssue::UserVersionTooOld { actual, minimum }
                if *actual == 10_200 && *minimum == MIN_USER_VERSION
        )),
        "expected UserVersionTooOld {{actual:10200, minimum:10300}}, got {:#?}",
        report.issues
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8: OrphanedExtensionRow — extension row referencing a non-existent table
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_integrity_orphaned_extension_row_detected() {
    // Build a 4-page file:
    //  - Page 1: sqlite_master with SRS (p2) and gpkg_extensions (p3) only.
    //  - Page 2: SRS with the three default rows (so the SRS check passes).
    //  - Page 3: gpkg_extensions with one row pointing at "ghost_table".
    //  - Page 4: padding.
    let page_size = 4096usize;
    let total_pages = 4usize;
    let mut file = vec![0u8; page_size * total_pages];

    // Page 2: SRS rows.
    let srs_rows = build_default_srs_rows();
    let srs_cells: Vec<(i64, &[u8])> = srs_rows
        .iter()
        .enumerate()
        .map(|(i, p)| (i as i64 + 1, p.as_slice()))
        .collect();
    let srs_page = build_leaf_table_page(page_size, &srs_cells, 0);
    file[page_size..page_size * 2].copy_from_slice(&srs_page);

    // Page 3: gpkg_extensions row.
    let ext_payload = encode_extensions_row(
        Some("ghost_table"),
        Some("geom"),
        "gpkg_rtree_index",
        "http://www.geopackage.org/spec/#extension_rtree",
        "read-write",
    );
    let ext_cells: Vec<(i64, &[u8])> = vec![(1, ext_payload.as_slice())];
    let ext_page = build_leaf_table_page(page_size, &ext_cells, 0);
    file[page_size * 2..page_size * 3].copy_from_slice(&ext_page);

    // Page 1: sqlite_master.
    let master_cells_owned: Vec<Vec<u8>> = vec![
        build_master_record(
            "gpkg_spatial_ref_sys",
            2,
            "CREATE TABLE gpkg_spatial_ref_sys(srs_id INTEGER)",
        ),
        build_master_record(
            "gpkg_extensions",
            3,
            "CREATE TABLE gpkg_extensions(table_name TEXT)",
        ),
    ];
    let master_cells: Vec<(i64, &[u8])> = master_cells_owned
        .iter()
        .enumerate()
        .map(|(i, m)| (i as i64 + 1, m.as_slice()))
        .collect();
    let master_page = build_leaf_table_page(page_size, &master_cells, 100);
    file[..page_size].copy_from_slice(&master_page);

    write_sqlite_file_header(&mut file, page_size as u16, total_pages as u32);

    let gpkg = GeoPackage::from_bytes(file).expect("parse");
    let report = check_integrity(&gpkg);

    assert!(!report.passed, "orphaned extension row must fail integrity");
    assert!(
        report.has_issue_of(|i| matches!(
            i,
            IntegrityIssue::OrphanedExtensionRow { table_name, extension_name }
                if table_name.as_deref() == Some("ghost_table")
                    && extension_name == "gpkg_rtree_index"
        )),
        "expected OrphanedExtensionRow for ghost_table; got {:#?}",
        report.issues
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9: IntegrityReport.passed mirrors issues.is_empty()
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_integrity_report_passed_flag_correctness() {
    // Valid file: passed = true.
    let bytes = GeoPackageBuilder::new(4326).build().expect("builder");
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");
    let report = check_integrity(&gpkg);
    assert_eq!(
        report.passed,
        report.issues.is_empty(),
        "passed flag must equal issues.is_empty()"
    );

    // Corrupted file: corrupt the app_id and ensure passed flips to false.
    let mut bad = GeoPackageBuilder::new(4326).build().expect("builder");
    bad[68..72].copy_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
    let gpkg_bad = GeoPackage::from_bytes(bad).expect("parse");
    let report_bad = check_integrity(&gpkg_bad);
    assert_eq!(
        report_bad.passed,
        report_bad.issues.is_empty(),
        "passed flag must equal issues.is_empty() for corrupted file"
    );
    assert!(!report_bad.passed);
    assert!(!report_bad.issues.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 10: check_integrity_strict returns Ok(()) for a clean GPKG
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_integrity_strict_returns_ok_for_valid() {
    let bytes = GeoPackageBuilder::new(4326).build().expect("builder");
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");
    let result = check_integrity_strict(&gpkg);
    assert!(
        result.is_ok(),
        "strict check on a valid GeoPackage must return Ok(()): {:#?}",
        result.err()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 11: check_integrity_strict returns Err with every issue
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_integrity_strict_returns_err_with_all_issues() {
    // Corrupt the GeoPackage at two header positions so the strict variant
    // sees multiple distinct issues at once.
    let mut bad = GeoPackageBuilder::new(4326).build().expect("builder");
    // Bad app_id → AppIdMismatch.
    bad[68..72].copy_from_slice(&0u32.to_be_bytes());
    // Bad user_version (5000 < 10_300) → UserVersionTooOld.
    bad[60..64].copy_from_slice(&5_000u32.to_be_bytes());

    let gpkg = GeoPackage::from_bytes(bad).expect("parse");
    let result = check_integrity_strict(&gpkg);
    let issues = result.expect_err("strict must return Err for corrupted file");

    // We expect at least the two header-level issues.
    let has_app_id = issues
        .iter()
        .any(|i| matches!(i, IntegrityIssue::AppIdMismatch { .. }));
    let has_user_version = issues
        .iter()
        .any(|i| matches!(i, IntegrityIssue::UserVersionTooOld { .. }));
    assert!(
        has_app_id,
        "expected AppIdMismatch in issues vec: {issues:#?}"
    );
    assert!(
        has_user_version,
        "expected UserVersionTooOld in issues vec: {issues:#?}"
    );

    // Same vec must equal check_integrity().issues.
    let report = check_integrity(&gpkg);
    assert_eq!(report.issues, issues);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 12: missing SRS rows in an otherwise valid table are detected
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_integrity_missing_required_srs_rows_detected() {
    // Build a 3-page file:
    //  - Page 1: sqlite_master with gpkg_spatial_ref_sys (page 2) only.
    //  - Page 2: SRS table with srs_id 4326 ONLY (-1 and 0 missing).
    //  - Page 3: padding.
    let page_size = 4096usize;
    let total_pages = 3usize;
    let mut file = vec![0u8; page_size * total_pages];

    let srs_row = encode_srs_row("WGS 84", 4326, "EPSG", 4326, "GEOGCS[\"WGS 84\"...]", "");
    let srs_cells: Vec<(i64, &[u8])> = vec![(1, srs_row.as_slice())];
    let srs_page = build_leaf_table_page(page_size, &srs_cells, 0);
    file[page_size..page_size * 2].copy_from_slice(&srs_page);

    let master = build_master_record(
        "gpkg_spatial_ref_sys",
        2,
        "CREATE TABLE gpkg_spatial_ref_sys(srs_id INTEGER)",
    );
    let master_cells: Vec<(i64, &[u8])> = vec![(1, master.as_slice())];
    let master_page = build_leaf_table_page(page_size, &master_cells, 100);
    file[..page_size].copy_from_slice(&master_page);

    write_sqlite_file_header(&mut file, page_size as u16, total_pages as u32);

    let gpkg = GeoPackage::from_bytes(file).expect("parse");
    let report = check_integrity(&gpkg);
    assert!(!report.passed, "missing required SRS rows must fail");

    let mut missing_codes: Vec<i32> = report
        .issues
        .iter()
        .filter_map(|i| match i {
            IntegrityIssue::MissingRequiredSrs { code } => Some(*code),
            _ => None,
        })
        .collect();
    missing_codes.sort();
    assert_eq!(
        missing_codes,
        vec![-1, 0],
        "expected -1 and 0 to be reported missing; got {:#?}",
        report.issues
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers: encode common system-table rows
// ─────────────────────────────────────────────────────────────────────────────

/// Encode an SRS row matching the canonical layout used by `load_contents`'s
/// sibling `gpkg_spatial_ref_sys` consumer.
fn encode_srs_row(
    srs_name: &str,
    srs_id: i32,
    organization: &str,
    org_id: i32,
    definition: &str,
    description: &str,
) -> Vec<u8> {
    let name_b = srs_name.as_bytes().to_vec();
    let org_b = organization.as_bytes().to_vec();
    let def_b = definition.as_bytes().to_vec();
    let desc_b = description.as_bytes().to_vec();

    // SRS IDs may be negative; use 3-byte signed int (serial type 3) when
    // it fits, otherwise 4-byte (serial type 4).  -1, 0, 4326 all fit in 3.
    let srs_id_st;
    let srs_id_bytes_vec: Vec<u8>;
    if (-(1 << 23)..(1 << 23)).contains(&srs_id) {
        // 3-byte big-endian signed
        let bytes = srs_id.to_be_bytes();
        srs_id_st = 3u64;
        srs_id_bytes_vec = bytes[1..4].to_vec();
    } else {
        srs_id_st = 4u64;
        srs_id_bytes_vec = srs_id.to_be_bytes().to_vec();
    }
    let org_id_st;
    let org_id_bytes_vec: Vec<u8>;
    if (-(1 << 23)..(1 << 23)).contains(&org_id) {
        let bytes = org_id.to_be_bytes();
        org_id_st = 3u64;
        org_id_bytes_vec = bytes[1..4].to_vec();
    } else {
        org_id_st = 4u64;
        org_id_bytes_vec = org_id.to_be_bytes().to_vec();
    }

    let fields: Vec<(u64, &[u8])> = vec![
        (text_serial_type(name_b.len()), &name_b),
        (srs_id_st, &srs_id_bytes_vec),
        (text_serial_type(org_b.len()), &org_b),
        (org_id_st, &org_id_bytes_vec),
        (text_serial_type(def_b.len()), &def_b),
        (text_serial_type(desc_b.len()), &desc_b),
    ];
    encode_record(&fields)
}

/// Encode a `gpkg_contents` row with the minimum fields the loader inspects.
///
/// Layout: `table_name, data_type, identifier, description, last_change,
///          min_x, min_y, max_x, max_y, srs_id`.
fn encode_contents_row(table_name: &str, data_type: &str, srs_id: i32) -> Vec<u8> {
    let tn = table_name.as_bytes().to_vec();
    let dt = data_type.as_bytes().to_vec();
    let id = table_name.as_bytes().to_vec();
    let desc = b"".to_vec();
    let last_change = b"2026-01-01T00:00:00.000Z".to_vec();

    // Bounding-box columns: all NULL (serial type 0).
    let zero: Vec<u8> = vec![];
    let srs_bytes = if (-(1 << 23)..(1 << 23)).contains(&srs_id) {
        srs_id.to_be_bytes()[1..4].to_vec()
    } else {
        srs_id.to_be_bytes().to_vec()
    };
    let srs_st = if (-(1 << 23)..(1 << 23)).contains(&srs_id) {
        3u64
    } else {
        4u64
    };

    let fields: Vec<(u64, &[u8])> = vec![
        (text_serial_type(tn.len()), &tn),
        (text_serial_type(dt.len()), &dt),
        (text_serial_type(id.len()), &id),
        (text_serial_type(desc.len()), &desc),
        (text_serial_type(last_change.len()), &last_change),
        (0, &zero), // min_x NULL
        (0, &zero), // min_y NULL
        (0, &zero), // max_x NULL
        (0, &zero), // max_y NULL
        (srs_st, &srs_bytes),
    ];
    encode_record(&fields)
}

/// Encode a `gpkg_geometry_columns` row.
///
/// Layout: `table_name, column_name, geometry_type_name, srs_id, z, m`.
fn encode_geometry_columns_row(
    table_name: &str,
    column_name: &str,
    geometry_type: &str,
    srs_id: i32,
) -> Vec<u8> {
    let tn = table_name.as_bytes().to_vec();
    let cn = column_name.as_bytes().to_vec();
    let gt = geometry_type.as_bytes().to_vec();

    // srs_id may be large (99_999).  Use 4-byte signed (serial type 4) for
    // values outside 3-byte range, 3-byte otherwise.
    let srs_st;
    let srs_bytes_vec: Vec<u8>;
    if (-(1 << 23)..(1 << 23)).contains(&srs_id) {
        srs_st = 3u64;
        srs_bytes_vec = srs_id.to_be_bytes()[1..4].to_vec();
    } else {
        srs_st = 4u64;
        srs_bytes_vec = srs_id.to_be_bytes().to_vec();
    }

    // z and m use serial type 8 = literal 0 (zero bytes occupied).
    let zero_lit: Vec<u8> = vec![];

    let fields: Vec<(u64, &[u8])> = vec![
        (text_serial_type(tn.len()), &tn),
        (text_serial_type(cn.len()), &cn),
        (text_serial_type(gt.len()), &gt),
        (srs_st, &srs_bytes_vec),
        (8, &zero_lit), // z = 0
        (8, &zero_lit), // m = 0
    ];
    encode_record(&fields)
}

/// Encode a `gpkg_extensions` row with optional nullable text columns.
fn encode_extensions_row(
    table_name: Option<&str>,
    column_name: Option<&str>,
    extension_name: &str,
    definition: &str,
    scope: &str,
) -> Vec<u8> {
    let tn_b: Vec<u8> = table_name
        .map(|s| s.as_bytes().to_vec())
        .unwrap_or_default();
    let cn_b: Vec<u8> = column_name
        .map(|s| s.as_bytes().to_vec())
        .unwrap_or_default();
    let en_b = extension_name.as_bytes().to_vec();
    let def_b = definition.as_bytes().to_vec();
    let scope_b = scope.as_bytes().to_vec();

    let tn_st = if table_name.is_some() {
        text_serial_type(tn_b.len())
    } else {
        0u64
    };
    let cn_st = if column_name.is_some() {
        text_serial_type(cn_b.len())
    } else {
        0u64
    };

    let fields: Vec<(u64, &[u8])> = vec![
        (tn_st, &tn_b),
        (cn_st, &cn_b),
        (text_serial_type(en_b.len()), &en_b),
        (text_serial_type(def_b.len()), &def_b),
        (text_serial_type(scope_b.len()), &scope_b),
    ];
    encode_record(&fields)
}

/// Build the encoded record payloads for the three default SRS rows.
fn build_default_srs_rows() -> Vec<Vec<u8>> {
    vec![
        encode_srs_row(
            "Undefined cartesian SRS",
            -1,
            "NONE",
            -1,
            "undefined",
            "undef cart",
        ),
        encode_srs_row(
            "Undefined geographic SRS",
            0,
            "NONE",
            0,
            "undefined",
            "undef geo",
        ),
        encode_srs_row(
            "WGS 84",
            4326,
            "EPSG",
            4326,
            "GEOGCS[\"WGS 84\"...]",
            "WGS 84",
        ),
    ]
}
