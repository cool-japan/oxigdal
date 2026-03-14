//! Integration tests for oxigdal-gpkg.

use oxigdal_gpkg::{GeoPackage, GpkgDataType, SqliteReader, TextEncoding};

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
    assert_eq!(GpkgDataType::parse_type("attributes").as_str(), "attributes");
    // Unknown falls back to Features
    assert_eq!(GpkgDataType::parse_type("other"), GpkgDataType::Features);
}
