//! Extended integration tests for oxigdal-gpkg.
//!
//! Brings total test count to 50+.

use oxigdal_gpkg::{
    GeoPackage, GpkgContents, GpkgDataType, GpkgGeometryColumn, GpkgSrs, SqliteReader, TextEncoding,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a minimal valid SQLite header (100 bytes).
fn make_sqlite_header(
    page_size_raw: u16,
    db_size_pages: u32,
    text_enc: u32,
    user_version: u32,
    application_id: u32,
) -> Vec<u8> {
    let mut data = vec![0u8; 100];
    data[..16].copy_from_slice(b"SQLite format 3\x00");
    data[16..18].copy_from_slice(&page_size_raw.to_be_bytes());
    data[28..32].copy_from_slice(&db_size_pages.to_be_bytes());
    data[56..60].copy_from_slice(&text_enc.to_be_bytes());
    data[60..64].copy_from_slice(&user_version.to_be_bytes());
    data[68..72].copy_from_slice(&application_id.to_be_bytes());
    data
}

/// Build a multi-page SQLite file with valid header.
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

/// The GPKG application_id magic value.
const GPKG_APP_ID: u32 = 0x4750_4B47;

// ── SqliteReader — additional parsing tests ────────────────────────────────────

#[test]
fn test_schema_format_byte() {
    let mut data = vec![0u8; 4096];
    let header = make_sqlite_header(4096, 1, 1, 0, 0);
    data[..100].copy_from_slice(&header);
    // schema_format is at offset 44
    data[44] = 4;
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.schema_format, 4);
}

#[test]
fn test_user_version_roundtrip() {
    let user_ver: u32 = 0x0001_0300; // version 1.3.0 encoded
    let mut data = vec![0u8; 4096];
    let header = make_sqlite_header(4096, 1, 1, user_ver, 0);
    data[..100].copy_from_slice(&header);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.user_version, user_ver);
}

#[test]
fn test_application_id_nonzero_nonmagic() {
    let data = make_sqlite_file(4096, 1, 0x0000_0001);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.application_id, 1);
    assert!(!reader.header.is_geopackage());
}

#[test]
fn test_freelist_page_fields() {
    let mut data = vec![0u8; 4096];
    let mut header = make_sqlite_header(4096, 1, 1, 0, 0);
    // first_freelist_page at offset 32 (BE u32)
    header[32..36].copy_from_slice(&7u32.to_be_bytes());
    // freelist_page_count at offset 36 (BE u32)
    header[36..40].copy_from_slice(&3u32.to_be_bytes());
    data[..100].copy_from_slice(&header);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.first_freelist_page, 7);
    assert_eq!(reader.header.freelist_page_count, 3);
}

#[test]
fn test_default_cache_size_negative() {
    let mut data = vec![0u8; 4096];
    let mut header = make_sqlite_header(4096, 1, 1, 0, 0);
    let cache: i32 = -2000;
    header[48..52].copy_from_slice(&cache.to_be_bytes());
    data[..100].copy_from_slice(&header);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.default_cache_size, -2000);
}

#[test]
fn test_default_cache_size_positive() {
    let mut data = vec![0u8; 4096];
    let mut header = make_sqlite_header(4096, 1, 1, 0, 0);
    let cache: i32 = 5000;
    header[48..52].copy_from_slice(&cache.to_be_bytes());
    data[..100].copy_from_slice(&header);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.default_cache_size, 5000);
}

#[test]
fn test_page_size_512() {
    let data = make_sqlite_file(512, 4, 0);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.page_size, 512);
    assert_eq!(reader.page_count(), 4);
}

#[test]
fn test_page_size_32768() {
    let data = make_sqlite_file(32768, 1, 0);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.page_size, 32768);
}

#[test]
fn test_page2_contents() {
    // Build a 2-page file and write a sentinel byte on page 2
    let page_size = 4096usize;
    let mut data = vec![0u8; page_size * 2];
    let header = make_sqlite_header(4096, 2, 1, 0, 0);
    data[..100].copy_from_slice(&header);
    // Write sentinel at start of page 2 (byte offset 4096)
    data[4096] = 0xDE;
    let reader = SqliteReader::from_bytes(data).expect("valid");
    let page2 = reader.page(2).expect("page2 ok");
    assert_eq!(page2[0], 0xDE);
}

#[test]
fn test_is_valid_with_full_page() {
    let data = make_sqlite_file(1024, 2, 0);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert!(reader.is_valid());
}

#[test]
fn test_is_valid_insufficient_data() {
    // File is 100 bytes but page_size=4096 — no complete page
    let data = make_sqlite_file(4096, 0, 0);
    // data length = max(4096*0, 100) = 100
    assert_eq!(data.len(), 100);
    let reader = SqliteReader::from_bytes(data).expect("valid header parse");
    assert!(!reader.is_valid());
}

#[test]
fn test_schema_version_field() {
    let mut data = vec![0u8; 4096];
    let mut header = make_sqlite_header(4096, 1, 1, 0, 0);
    // schema_version (schema cookie) at offset 40 (BE u32)
    header[40..44].copy_from_slice(&42u32.to_be_bytes());
    data[..100].copy_from_slice(&header);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.schema_version, 42);
}

// ── GeoPackage wrapper — extended ─────────────────────────────────────────────

#[test]
fn test_geopackage_is_valid_with_gpkg_app_id() {
    let data = make_sqlite_file(4096, 2, GPKG_APP_ID);
    let gpkg = GeoPackage::from_bytes(data).expect("valid");
    assert!(gpkg.is_valid_gpkg());
}

#[test]
fn test_geopackage_is_valid_without_gpkg_app_id_but_full_page() {
    // is_valid_gpkg returns true if either condition holds —
    // a regular SQLite file with 0 app_id but is_valid() should also return true
    let data = make_sqlite_file(4096, 1, 0);
    let gpkg = GeoPackage::from_bytes(data).expect("valid");
    assert!(gpkg.is_valid_gpkg());
}

#[test]
fn test_geopackage_has_gpkg_application_id_false() {
    let data = make_sqlite_file(4096, 1, 0xCAFE_BABE);
    let gpkg = GeoPackage::from_bytes(data).expect("valid");
    assert!(!gpkg.has_gpkg_application_id());
}

#[test]
fn test_geopackage_page_size_1024() {
    let data = make_sqlite_file(1024, 4, GPKG_APP_ID);
    let gpkg = GeoPackage::from_bytes(data).expect("valid");
    assert_eq!(gpkg.page_size(), 1024);
}

#[test]
fn test_geopackage_page_count_from_header() {
    let data = make_sqlite_file(4096, 5, GPKG_APP_ID);
    let gpkg = GeoPackage::from_bytes(data).expect("valid");
    assert_eq!(gpkg.page_count(), 5);
}

#[test]
fn test_geopackage_contents_initially_empty() {
    let data = make_sqlite_file(4096, 1, GPKG_APP_ID);
    let gpkg = GeoPackage::from_bytes(data).expect("valid");
    assert!(gpkg.contents.is_empty());
}

// ── GpkgDataType — extended ────────────────────────────────────────────────────

#[test]
fn test_gpkg_data_type_features_as_str() {
    assert_eq!(GpkgDataType::Features.as_str(), "features");
}

#[test]
fn test_gpkg_data_type_tiles_as_str() {
    assert_eq!(GpkgDataType::Tiles.as_str(), "tiles");
}

#[test]
fn test_gpkg_data_type_attributes_as_str() {
    assert_eq!(GpkgDataType::Attributes.as_str(), "attributes");
}

#[test]
fn test_gpkg_data_type_parse_features() {
    assert_eq!(GpkgDataType::parse_type("features"), GpkgDataType::Features);
}

#[test]
fn test_gpkg_data_type_parse_tiles() {
    assert_eq!(GpkgDataType::parse_type("tiles"), GpkgDataType::Tiles);
}

#[test]
fn test_gpkg_data_type_parse_attributes() {
    assert_eq!(
        GpkgDataType::parse_type("attributes"),
        GpkgDataType::Attributes
    );
}

#[test]
fn test_gpkg_data_type_unknown_falls_back_to_features() {
    assert_eq!(GpkgDataType::parse_type("unknown"), GpkgDataType::Features);
    assert_eq!(GpkgDataType::parse_type(""), GpkgDataType::Features);
    assert_eq!(GpkgDataType::parse_type("TILES"), GpkgDataType::Features);
}

#[test]
fn test_gpkg_data_type_equality() {
    assert_eq!(GpkgDataType::Tiles, GpkgDataType::Tiles);
    assert_ne!(GpkgDataType::Features, GpkgDataType::Tiles);
}

// ── GpkgContents struct construction ──────────────────────────────────────────

#[test]
fn test_gpkg_contents_construction() {
    let c = GpkgContents {
        table_name: "my_layer".to_string(),
        data_type: GpkgDataType::Features,
        identifier: Some("My Layer".to_string()),
        description: Some("Test layer".to_string()),
        min_x: -180.0,
        min_y: -90.0,
        max_x: 180.0,
        max_y: 90.0,
        srs_id: 4326,
    };
    assert_eq!(c.table_name, "my_layer");
    assert_eq!(c.data_type, GpkgDataType::Features);
    assert_eq!(c.srs_id, 4326);
    assert!((c.min_x - (-180.0)).abs() < f64::EPSILON);
    assert!((c.max_y - 90.0).abs() < f64::EPSILON);
}

#[test]
fn test_gpkg_contents_optional_fields_none() {
    let c = GpkgContents {
        table_name: "bare".to_string(),
        data_type: GpkgDataType::Tiles,
        identifier: None,
        description: None,
        min_x: 0.0,
        min_y: 0.0,
        max_x: 1.0,
        max_y: 1.0,
        srs_id: 3857,
    };
    assert!(c.identifier.is_none());
    assert!(c.description.is_none());
}

// ── GpkgGeometryColumn struct ─────────────────────────────────────────────────

#[test]
fn test_gpkg_geometry_column_fields() {
    let gc = GpkgGeometryColumn {
        table_name: "buildings".to_string(),
        column_name: "geom".to_string(),
        geometry_type_name: "MULTIPOLYGON".to_string(),
        srs_id: 4326,
        z: 0,
        m: 0,
    };
    assert_eq!(gc.table_name, "buildings");
    assert_eq!(gc.geometry_type_name, "MULTIPOLYGON");
    assert_eq!(gc.z, 0);
    assert_eq!(gc.m, 0);
}

#[test]
fn test_gpkg_geometry_column_z_mandatory() {
    let gc = GpkgGeometryColumn {
        table_name: "pts".to_string(),
        column_name: "geom".to_string(),
        geometry_type_name: "POINT".to_string(),
        srs_id: 4326,
        z: 1, // mandatory
        m: 2, // optional
    };
    assert_eq!(gc.z, 1);
    assert_eq!(gc.m, 2);
}

// ── GpkgSrs struct ────────────────────────────────────────────────────────────

#[test]
fn test_gpkg_srs_fields() {
    let srs = GpkgSrs {
        srs_name: "WGS 84".to_string(),
        srs_id: 4326,
        organization: "EPSG".to_string(),
        organization_coordsys_id: 4326,
        definition: "GEOGCS[...]".to_string(),
        description: Some("World geodetic system 1984".to_string()),
    };
    assert_eq!(srs.srs_id, 4326);
    assert_eq!(srs.organization, "EPSG");
    assert!(srs.description.is_some());
}

#[test]
fn test_gpkg_srs_no_description() {
    let srs = GpkgSrs {
        srs_name: "Web Mercator".to_string(),
        srs_id: 3857,
        organization: "EPSG".to_string(),
        organization_coordsys_id: 3857,
        definition: "PROJCS[...]".to_string(),
        description: None,
    };
    assert_eq!(srs.srs_id, 3857);
    assert!(srs.description.is_none());
}

// ── TextEncoding variants ─────────────────────────────────────────────────────

#[test]
fn test_text_encoding_unknown_value_defaults_to_utf8() {
    // Value 0 and any other unknown value should fall back to Utf8
    let mut data = vec![0u8; 4096];
    let header = make_sqlite_header(4096, 1, 0, 0, 0); // text_enc = 0
    data[..100].copy_from_slice(&header);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.text_encoding, TextEncoding::Utf8);
}

#[test]
fn test_text_encoding_value_5_defaults_to_utf8() {
    let mut data = vec![0u8; 4096];
    let header = make_sqlite_header(4096, 1, 5, 0, 0); // unknown enc = 5
    data[..100].copy_from_slice(&header);
    let reader = SqliteReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.text_encoding, TextEncoding::Utf8);
}
