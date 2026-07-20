//! Integration tests for oxigeo-gpkg.

use oxigeo_gpkg::btree::encode_sqlite_varint;
use oxigeo_gpkg::{CellValue, GeoPackage, GpkgDataType, MasterEntry, SqliteReader, TextEncoding};

// ── Helper ────────────────────────────────────────────────────────────────────

/// Build a minimal valid SQLite header (100 bytes) with caller-controlled fields.
fn make_sqlite_header(
    page_size_raw: u16,
    db_size_pages: u32,
    text_enc: u32,
    user_version: u32,
    application_id: u32,
) -> Vec<u8> {
    let mut data = vec![0u8; 100];
    // Magic: "SQLite format 3\0"
    data[..16].copy_from_slice(b"SQLite format 3\x00");
    // Page size (offset 16, 2 bytes BE)
    data[16..18].copy_from_slice(&page_size_raw.to_be_bytes());
    // db_size_pages (offset 28, 4 bytes BE)
    data[28..32].copy_from_slice(&db_size_pages.to_be_bytes());
    // text encoding (offset 56, 4 bytes BE)
    data[56..60].copy_from_slice(&text_enc.to_be_bytes());
    // user_version (offset 60, 4 bytes BE)
    data[60..64].copy_from_slice(&user_version.to_be_bytes());
    // application_id (offset 68, 4 bytes BE)
    data[68..72].copy_from_slice(&application_id.to_be_bytes());
    data
}

/// Build a byte buffer of `pages` pages of `page_size` bytes each, with a
/// valid SQLite header in the first 100 bytes.
fn make_sqlite_file(page_size: u16, pages: u32, application_id: u32) -> Vec<u8> {
    let actual_size = if page_size == 1 {
        65536usize
    } else {
        page_size as usize
    };
    let total = actual_size * pages as usize;
    let mut data = vec![0u8; total.max(100)];
    let header = make_sqlite_header(page_size, pages, 1, 0, application_id);
    data[..100].copy_from_slice(&header);
    data
}

// ── Test 1: valid magic bytes → Ok ───────────────────────────────────────────

#[test]
fn test_valid_magic_ok() {
    let data = make_sqlite_file(4096, 1, 0);
    assert!(SqliteReader::from_bytes(data).is_ok());
}

// ── Test 2: short data → error ────────────────────────────────────────────────

#[test]
fn test_short_data_error() {
    let data = vec![0u8; 50];
    let err = SqliteReader::from_bytes(data);
    assert!(err.is_err());
}

// ── Test 3: wrong magic → error ───────────────────────────────────────────────

#[test]
fn test_wrong_magic_error() {
    let mut data = vec![0u8; 200];
    data[..4].copy_from_slice(b"NOTQ");
    let err = SqliteReader::from_bytes(data);
    assert!(err.is_err());
}

// ── Test 4: page_size raw=1 → 65536 ──────────────────────────────────────────

#[test]
fn test_page_size_one_means_65536() {
    // page_size_raw=1 means 65536 per the SQLite spec
    let data = make_sqlite_file(1, 1, 0);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.page_size, 65536);
}

// ── Test 5: page_size raw=4096 → 4096 ────────────────────────────────────────

#[test]
fn test_page_size_4096() {
    let data = make_sqlite_file(4096, 1, 0);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.page_size, 4096);
}

// ── Test 6: page_count from data length when db_size_pages=0 ─────────────────

#[test]
fn test_page_count_from_data_length() {
    // db_size_pages = 0 → infer from data.len() / page_size
    let page_size = 4096u16;
    let n_pages = 3u32;
    let mut data = vec![0u8; page_size as usize * n_pages as usize];
    let header = make_sqlite_header(page_size, 0 /*db_size_pages=0*/, 1, 0, 0);
    data[..100].copy_from_slice(&header);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.page_count(), n_pages);
}

// ── Test 7: page_count from header when db_size_pages > 0 ────────────────────

#[test]
fn test_page_count_from_header() {
    let data = make_sqlite_file(4096, 7, 0);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.page_count(), 7);
}

// ── Test 8: is_geopackage() with correct application_id ──────────────────────

#[test]
fn test_is_geopackage_true() {
    let data = make_sqlite_file(4096, 1, 0x4750_4B47);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert!(reader.header.is_geopackage());
}

// ── Test 9: is_geopackage() false with wrong application_id ──────────────────

#[test]
fn test_is_geopackage_false() {
    let data = make_sqlite_file(4096, 1, 0xDEAD_BEEF);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert!(!reader.header.is_geopackage());
}

// ── Test 10: text encoding UTF-8 (value 1) ────────────────────────────────────

#[test]
fn test_text_encoding_utf8() {
    let header = make_sqlite_header(4096, 1, 1, 0, 0);
    let mut data = vec![0u8; 4096];
    data[..100].copy_from_slice(&header);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.text_encoding, TextEncoding::Utf8);
}

// ── Test 11: text encoding UTF-16 LE (value 2) ───────────────────────────────

#[test]
fn test_text_encoding_utf16le() {
    let header = make_sqlite_header(4096, 1, 2, 0, 0);
    let mut data = vec![0u8; 4096];
    data[..100].copy_from_slice(&header);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.text_encoding, TextEncoding::Utf16Le);
}

// ── Test 12: text encoding UTF-16 BE (value 3) ───────────────────────────────

#[test]
fn test_text_encoding_utf16be() {
    let header = make_sqlite_header(4096, 1, 3, 0, 0);
    let mut data = vec![0u8; 4096];
    data[..100].copy_from_slice(&header);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.text_encoding, TextEncoding::Utf16Be);
}

// ── Test 13: page() accesses valid first page ─────────────────────────────────

#[test]
fn test_page_access_valid() {
    let data = make_sqlite_file(4096, 2, 0);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    let page1 = reader.page(1);
    assert!(page1.is_ok());
    assert_eq!(page1.expect("page1").len(), 4096);
}

// ── Test 14: page() out of range → error ─────────────────────────────────────

#[test]
fn test_page_out_of_range_error() {
    let data = make_sqlite_file(4096, 1, 0);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert!(reader.page(2).is_err()); // only 1 page
}

// ── Test 15: page(0) → error ─────────────────────────────────────────────────

#[test]
fn test_page_zero_error() {
    let data = make_sqlite_file(4096, 1, 0);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert!(reader.page(0).is_err());
}

// ── Extra: GeoPackage wrapper ─────────────────────────────────────────────────

#[test]
fn test_geopackage_from_bytes() {
    let data = make_sqlite_file(4096, 2, 0x4750_4B47);
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");
    assert!(gpkg.has_gpkg_application_id());
    assert_eq!(gpkg.page_size(), 4096);
    assert_eq!(gpkg.page_count(), 2);
}

#[test]
fn test_gpkg_data_type_round_trip() {
    assert_eq!(GpkgDataType::parse_type("features").as_str(), "features");
    assert_eq!(GpkgDataType::parse_type("tiles").as_str(), "tiles");
    assert_eq!(
        GpkgDataType::parse_type("attributes").as_str(),
        "attributes"
    );
    // Unknown falls back to Features
    assert_eq!(GpkgDataType::parse_type("other"), GpkgDataType::Features);
}

// ─────────────────────────────────────────────────────────────────────────────
// B-tree traversal / sqlite_master / table scan integration tests
// ─────────────────────────────────────────────────────────────────────────────

/// Build a leaf table B-tree page with the given rowid-payload cells.
///
/// `header_offset` must be `100` for page 1 (which shares its first 100 bytes
/// with the SQLite file header) and `0` for any other page.
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

    // 8-byte leaf page header
    let hdr = header_offset;
    page[hdr] = 13; // leaf table
    page[hdr + 1] = 0;
    page[hdr + 2] = 0;
    page[hdr + 3] = ((cell_count >> 8) & 0xFF) as u8;
    page[hdr + 4] = (cell_count & 0xFF) as u8;
    let content_start = content_end as u16;
    page[hdr + 5] = ((content_start >> 8) & 0xFF) as u8;
    page[hdr + 6] = (content_start & 0xFF) as u8;
    page[hdr + 7] = 0;

    // cell pointer array
    let ptr_start = hdr + 8;
    for (i, offset) in cell_offsets.iter().enumerate() {
        let o = *offset as u16;
        page[ptr_start + i * 2] = ((o >> 8) & 0xFF) as u8;
        page[ptr_start + i * 2 + 1] = (o & 0xFF) as u8;
    }

    page
}

/// Encode a SQLite record body: `[header_len varint, serial types varints..., values...]`.
/// `rows` is a vector of `(serial_type, value_bytes)` tuples.
fn encode_record(fields: &[(u64, &[u8])]) -> Vec<u8> {
    // Compute header length: sum of lengths of all serial-type varints, plus the
    // length of the header-length varint itself (self-referential, so iterate).
    let serial_type_varints: Vec<Vec<u8>> = fields
        .iter()
        .map(|(st, _)| encode_sqlite_varint(*st))
        .collect();
    let st_bytes: usize = serial_type_varints.iter().map(|v| v.len()).sum();

    let mut hdr_len = st_bytes + 1; // assume 1-byte header-length varint
    // If st_bytes + its own varint length overflows single-byte, re-compute.
    // For our small test payloads, st_bytes is small enough that 1 byte suffices.
    let hdr_varint = encode_sqlite_varint(hdr_len as u64);
    if hdr_varint.len() != 1 {
        // Recompute including the header-length varint itself in the total.
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

/// Write the SQLite file header magic/page-size/db-size into `file_data`.
fn write_sqlite_file_header(file_data: &mut [u8], page_size: u16, db_size_pages: u32) {
    file_data[..16].copy_from_slice(b"SQLite format 3\x00");
    file_data[16..18].copy_from_slice(&page_size.to_be_bytes());
    file_data[28..32].copy_from_slice(&db_size_pages.to_be_bytes());
    // text encoding = UTF-8 (1)
    file_data[56..60].copy_from_slice(&1u32.to_be_bytes());
}

/// Build a minimal single-page SQLite database with a `sqlite_master` row that
/// describes a `"table"` named `table_name` rooted at page 2. Page 2 contains
/// a leaf table page with one row (rowid=1, one integer-100 column).
fn build_minimal_gpkg_with_table(table_name: &str) -> Vec<u8> {
    let page_size = 4096usize;
    let mut file = vec![0u8; page_size * 2];

    // Page 2: leaf table page with a single row (rowid=1, column = integer 100).
    // Record: serial_type=1 (i8), body = 100.
    let row_record = encode_record(&[(1u64, &[100u8])]);
    let leaf_page_2 = build_leaf_table_page(page_size, &[(1, &row_record)], 0);
    file[page_size..page_size * 2].copy_from_slice(&leaf_page_2);

    // Page 1: sqlite_master leaf with one row that references page 2.
    let entry_type = b"table".to_vec();
    let name = table_name.as_bytes().to_vec();
    let tbl_name = table_name.as_bytes().to_vec();
    let sql = format!("CREATE TABLE {table_name}(id INTEGER)").into_bytes();

    let st_type = (entry_type.len() as u64) * 2 + 13;
    let st_name = (name.len() as u64) * 2 + 13;
    let st_tbl_name = (tbl_name.len() as u64) * 2 + 13;
    let st_rootpage = 1u64; // serial type 1 (i8) — fits since rootpage is 2
    let st_sql = (sql.len() as u64) * 2 + 13;

    let rootpage_bytes = [2u8]; // i8 = 2
    let fields: Vec<(u64, &[u8])> = vec![
        (st_type, &entry_type),
        (st_name, &name),
        (st_tbl_name, &tbl_name),
        (st_rootpage, &rootpage_bytes),
        (st_sql, &sql),
    ];
    let master_record = encode_record(&fields);

    let master_page_1 = build_leaf_table_page(page_size, &[(1, &master_record)], 100);
    file[..page_size].copy_from_slice(&master_page_1);

    // Write the SQLite file header on page 1.
    write_sqlite_file_header(&mut file, page_size as u16, 2);

    file
}

#[test]
fn test_scan_sqlite_master_single_table() {
    let file = build_minimal_gpkg_with_table("my_features");
    let gpkg = GeoPackage::from_bytes(file).expect("valid gpkg");
    let entries: Vec<MasterEntry> = gpkg.scan_sqlite_master().expect("scan master");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].entry_type, "table");
    assert_eq!(entries[0].name, "my_features");
    assert_eq!(entries[0].tbl_name, "my_features");
    assert_eq!(entries[0].rootpage, 2);
    assert!(entries[0].sql.starts_with("CREATE TABLE"));
}

#[test]
fn test_scan_table_by_name_returns_rows() {
    let file = build_minimal_gpkg_with_table("widgets");
    let gpkg = GeoPackage::from_bytes(file).expect("valid gpkg");

    let rows = gpkg
        .scan_table_by_name("widgets")
        .expect("scan")
        .expect("table exists");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, 1); // rowid
    assert_eq!(rows[0].1.len(), 1);
    assert_eq!(rows[0].1[0], CellValue::Integer(100));
}

#[test]
fn test_scan_table_by_name_missing_returns_none() {
    let file = build_minimal_gpkg_with_table("widgets");
    let gpkg = GeoPackage::from_bytes(file).expect("valid gpkg");
    let missing = gpkg.scan_table_by_name("does_not_exist").expect("scan");
    assert!(missing.is_none());
}

#[test]
fn test_scan_table_by_root_page_direct() {
    // Build a 2-page file and scan page 2 directly — no sqlite_master lookup.
    let file = build_minimal_gpkg_with_table("dummy");
    let gpkg = GeoPackage::from_bytes(file).expect("valid gpkg");
    let rows = gpkg.scan_table(2).expect("scan page 2");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1[0], CellValue::Integer(100));
}
