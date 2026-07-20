//! Integration tests for `gpkg_metadata` / `gpkg_metadata_reference` parsers.
//!
//! Covers OGC GeoPackage §10.8 (Table 16) and §10.8.5 (Table 18).
//!
//! All tests that need real binary SQLite data build it from scratch using the
//! same low-level helpers that `gpkg_test.rs` uses, so we have no external
//! file dependency and no `rusqlite` requirement.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::str::FromStr;

use oxigeo_gpkg::btree::encode_sqlite_varint;
use oxigeo_gpkg::{GeoPackage, GpkgMetadata, GpkgMetadataReference, MetadataScope, ReferenceScope};

// ─────────────────────────────────────────────────────────────────────────────
// Binary SQLite builder helpers (duplicated from gpkg_test.rs to keep each
// test file self-contained and to avoid a `tests/common/` module)
// ─────────────────────────────────────────────────────────────────────────────

/// Build a minimal valid SQLite file header (100 bytes).
fn make_sqlite_header(page_size_raw: u16, db_size_pages: u32, application_id: u32) -> Vec<u8> {
    let mut data = vec![0u8; 100];
    data[..16].copy_from_slice(b"SQLite format 3\x00");
    data[16..18].copy_from_slice(&page_size_raw.to_be_bytes());
    data[28..32].copy_from_slice(&db_size_pages.to_be_bytes());
    // text encoding = UTF-8 (1)
    data[56..60].copy_from_slice(&1u32.to_be_bytes());
    data[68..72].copy_from_slice(&application_id.to_be_bytes());
    data
}

/// Build a minimal SQLite file whose page 1 contains an empty but structurally
/// valid sqlite_master leaf table page (zero rows).
///
/// This is the correct "no user tables" baseline: the B-tree reader can parse
/// the page (page type byte = 13, cell count = 0) and will simply not find any
/// matching table, so `scan_table_by_name` returns `Ok(None)`.
fn make_minimal_sqlite(page_size: usize, n_pages: u32) -> Vec<u8> {
    let total = (page_size * n_pages as usize).max(page_size);
    let mut file = vec![0u8; total];

    // Page 1: empty sqlite_master leaf (no rows, no cells).
    // build_leaf_page with an empty cell list writes the 8-byte B-tree page
    // header at offset `header_offset=100` (because the first 100 bytes of
    // page 1 are the SQLite file header).
    let empty_master = build_leaf_page(page_size, &[], 100);
    file[..page_size].copy_from_slice(&empty_master);

    // Write the SQLite file header (100 bytes, overlapping the start of page 1).
    let hdr = make_sqlite_header(page_size as u16, n_pages, 0x4750_4B47);
    file[..100].copy_from_slice(&hdr);

    file
}

/// Encode a SQLite record: `[hdr_len varint][serial type varints...][values...]`.
///
/// Each element of `fields` is `(serial_type, value_bytes)`.  The header is
/// self-referential (its length includes the varint for the length itself), so
/// we compute it iteratively.
fn encode_record(fields: &[(u64, &[u8])]) -> Vec<u8> {
    let st_varints: Vec<Vec<u8>> = fields
        .iter()
        .map(|(st, _)| encode_sqlite_varint(*st))
        .collect();
    let st_bytes: usize = st_varints.iter().map(|v| v.len()).sum();

    // Self-referential header length: hdr_len includes its own varint encoding.
    let mut hdr_len = st_bytes + 1; // start: assume 1-byte hdr-len varint
    let hdr_varint = encode_sqlite_varint(hdr_len as u64);
    if hdr_varint.len() != 1 {
        hdr_len = st_bytes + hdr_varint.len();
    }
    let hdr_varint = encode_sqlite_varint(hdr_len as u64);

    let mut out = Vec::new();
    out.extend_from_slice(&hdr_varint);
    for v in &st_varints {
        out.extend_from_slice(v);
    }
    for (_, bytes) in fields {
        out.extend_from_slice(bytes);
    }
    out
}

/// Write a leaf table B-tree page.
///
/// `header_offset` is 100 for page 1 (overlaps with the SQLite file header)
/// and 0 for any subsequent page.
fn build_leaf_page(page_size: usize, cells: &[(i64, Vec<u8>)], header_offset: usize) -> Vec<u8> {
    let mut page = vec![0u8; page_size];
    let mut content_end = page_size;
    let mut cell_offsets: Vec<usize> = Vec::with_capacity(cells.len());

    for (rowid, payload) in cells {
        let pl_varint = encode_sqlite_varint(payload.len() as u64);
        let rid_varint = encode_sqlite_varint(*rowid as u64);
        let cell_size = pl_varint.len() + rid_varint.len() + payload.len();
        content_end -= cell_size;
        cell_offsets.push(content_end);

        let mut pos = content_end;
        page[pos..pos + pl_varint.len()].copy_from_slice(&pl_varint);
        pos += pl_varint.len();
        page[pos..pos + rid_varint.len()].copy_from_slice(&rid_varint);
        pos += rid_varint.len();
        page[pos..pos + payload.len()].copy_from_slice(payload);
    }

    let hdr = header_offset;
    let cell_count = cells.len();
    page[hdr] = 13; // leaf table page type
    page[hdr + 3] = ((cell_count >> 8) & 0xFF) as u8;
    page[hdr + 4] = (cell_count & 0xFF) as u8;
    let content_start = content_end as u16;
    page[hdr + 5] = (content_start >> 8) as u8;
    page[hdr + 6] = (content_start & 0xFF) as u8;

    // Cell pointer array (2 bytes per pointer, immediately after the 8-byte header)
    let ptr_base = hdr + 8;
    for (i, off) in cell_offsets.iter().enumerate() {
        let o = *off as u16;
        page[ptr_base + i * 2] = (o >> 8) as u8;
        page[ptr_base + i * 2 + 1] = (o & 0xFF) as u8;
    }

    page
}

/// Serial type for a UTF-8 string of `n` bytes: `n * 2 + 13`.
const fn text_serial(n: usize) -> u64 {
    n as u64 * 2 + 13
}

/// Serial type 1 = signed 8-bit integer.
const INTEGER_I8_SERIAL: u64 = 1;

/// Serial type 6 = signed 48-bit integer (fits any reasonable i64 in tests).
const INTEGER_I8_SERIAL_6: u64 = 6;

/// Serial type 0 = SQL NULL.
const NULL_SERIAL: u64 = 0;

/// Encode an `i64` value as a big-endian signed 8-byte integer (serial type 6).
fn encode_i64_be(v: i64) -> Vec<u8> {
    v.to_be_bytes().to_vec()
}

/// Encode a single-byte i8 as its raw byte (serial type 1).
fn encode_i8(v: i8) -> Vec<u8> {
    vec![v as u8]
}

// ─────────────────────────────────────────────────────────────────────────────
// Build a complete SQLite binary with gpkg_metadata and/or
// gpkg_metadata_reference tables populated with real rows.
// ─────────────────────────────────────────────────────────────────────────────

/// Append a `sqlite_master` entry record to `master_cells`.
///
/// Each call adds one row that describes a user table rooted at `root_page`.
fn append_master_entry(
    master_cells: &mut Vec<(i64, Vec<u8>)>,
    rowid: i64,
    table_name: &str,
    root_page: u8,
) {
    let entry_type = b"table".as_ref();
    let name = table_name.as_bytes();
    let tbl_name = table_name.as_bytes();
    let sql = format!("CREATE TABLE {table_name}(id INTEGER)").into_bytes();
    let rootpage_bytes = [root_page];

    let record = encode_record(&[
        (text_serial(entry_type.len()), entry_type),
        (text_serial(name.len()), name),
        (text_serial(tbl_name.len()), tbl_name),
        (INTEGER_I8_SERIAL, &rootpage_bytes),
        (text_serial(sql.len()), sql.as_slice()),
    ]);
    master_cells.push((rowid, record));
}

/// Build a complete 3-page SQLite file:
/// - Page 1: `sqlite_master` with two table entries (gpkg_metadata → page 2,
///   gpkg_metadata_reference → page 3)
/// - Page 2: `gpkg_metadata` with a single row
/// - Page 3: `gpkg_metadata_reference` with a single row
///
/// The metadata row encodes:
/// - id = 42
/// - md_scope = "dataset"
/// - md_standard_uri = "<http://www.isotc211.org/2005/gmd>"
/// - mime_type = "text/xml"
/// - metadata = "<metadata/>"
///
/// The metadata_reference row encodes:
/// - reference_scope = "table"
/// - table_name = "my_layer"
/// - column_name = NULL
/// - row_id_value = NULL
/// - timestamp = "2024-01-01T00:00:00.000Z"
/// - md_file_id = 42
/// - md_parent_id = NULL
fn build_gpkg_with_metadata_tables() -> Vec<u8> {
    let page_size: usize = 4096;
    let n_pages: u32 = 3;
    let mut file = vec![0u8; page_size * n_pages as usize];

    // ── Page 2: gpkg_metadata ────────────────────────────────────────────────
    let md_id: i8 = 42;
    let md_scope = b"dataset".as_ref();
    let md_uri = b"http://www.isotc211.org/2005/gmd".as_ref();
    let md_mime = b"text/xml".as_ref();
    let md_content = b"<metadata/>".as_ref();

    let md_record = encode_record(&[
        (INTEGER_I8_SERIAL, &[md_id as u8]),
        (text_serial(md_scope.len()), md_scope),
        (text_serial(md_uri.len()), md_uri),
        (text_serial(md_mime.len()), md_mime),
        (text_serial(md_content.len()), md_content),
    ]);
    let md_page = build_leaf_page(page_size, &[(1, md_record)], 0);
    file[page_size..page_size * 2].copy_from_slice(&md_page);

    // ── Page 3: gpkg_metadata_reference ─────────────────────────────────────
    let ref_scope = b"table".as_ref();
    let ref_table_name = b"my_layer".as_ref();
    let ref_timestamp = b"2024-01-01T00:00:00.000Z".as_ref();
    let ref_md_file_id: i8 = 42;

    let ref_record = encode_record(&[
        (text_serial(ref_scope.len()), ref_scope), // reference_scope
        (text_serial(ref_table_name.len()), ref_table_name), // table_name
        (NULL_SERIAL, &[]),                        // column_name (NULL)
        (NULL_SERIAL, &[]),                        // row_id_value (NULL)
        (text_serial(ref_timestamp.len()), ref_timestamp), // timestamp
        (INTEGER_I8_SERIAL, &[ref_md_file_id as u8]), // md_file_id
        (NULL_SERIAL, &[]),                        // md_parent_id (NULL)
    ]);
    let ref_page = build_leaf_page(page_size, &[(1, ref_record)], 0);
    file[page_size * 2..page_size * 3].copy_from_slice(&ref_page);

    // ── Page 1: sqlite_master ────────────────────────────────────────────────
    let mut master_cells: Vec<(i64, Vec<u8>)> = Vec::new();
    append_master_entry(&mut master_cells, 1, "gpkg_metadata", 2);
    append_master_entry(&mut master_cells, 2, "gpkg_metadata_reference", 3);

    let master_page = build_leaf_page(page_size, &master_cells, 100);
    file[..page_size].copy_from_slice(&master_page);

    // Write the SQLite file header into page 1.
    let hdr = make_sqlite_header(page_size as u16, n_pages, 0x4750_4B47);
    file[..100].copy_from_slice(&hdr);

    file
}

/// Build a GPKG with a `gpkg_metadata` table containing a row that has
/// `md_scope = "row/col"` (intentionally invalid for MetadataScope, maps to
/// Undefined) and uses a 6-byte integer serial type for `id`.
fn build_gpkg_with_multirow_metadata() -> Vec<u8> {
    let page_size: usize = 4096;
    let n_pages: u32 = 3;
    let mut file = vec![0u8; page_size * n_pages as usize];

    // Row 1: id=1, scope=featureType, standard=urn:ogc:def:crs, mime=text/plain, metadata=<a/>
    let r1_id = encode_i8(1);
    let r1_scope = b"featureType".as_ref();
    let r1_uri = b"urn:ogc:def:crs".as_ref();
    let r1_mime = b"text/plain".as_ref();
    let r1_content = b"<a/>".as_ref();
    let r1 = encode_record(&[
        (INTEGER_I8_SERIAL, &r1_id),
        (text_serial(r1_scope.len()), r1_scope),
        (text_serial(r1_uri.len()), r1_uri),
        (text_serial(r1_mime.len()), r1_mime),
        (text_serial(r1_content.len()), r1_content),
    ]);

    // Row 2: id=999 (i64), scope=service, others
    let r2_id = encode_i64_be(999_i64);
    let r2_scope = b"service".as_ref();
    let r2_uri = b"http://example.com".as_ref();
    let r2_mime = b"application/json".as_ref();
    let r2_content = b"{}".as_ref();
    let r2 = encode_record(&[
        (INTEGER_I8_SERIAL_6, &r2_id),
        (text_serial(r2_scope.len()), r2_scope),
        (text_serial(r2_uri.len()), r2_uri),
        (text_serial(r2_mime.len()), r2_mime),
        (text_serial(r2_content.len()), r2_content),
    ]);

    let md_cells = vec![(1_i64, r1), (2_i64, r2)];
    let md_page = build_leaf_page(page_size, &md_cells, 0);
    file[page_size..page_size * 2].copy_from_slice(&md_page);

    // Page 3: gpkg_metadata_reference with two rows
    // Row 1: row/col scope with all optional fields set
    let ts = b"2025-06-15T12:00:00.000Z".as_ref();
    let rref1_scope = b"row/col".as_ref();
    let rref1_table = b"features".as_ref();
    let rref1_col = b"geom".as_ref();
    let rref1_row_id = encode_i8(7);
    let rref1_file_id = encode_i8(1);
    let rref1_parent_id = encode_i8(0); // parent id = 0

    let rref1 = encode_record(&[
        (text_serial(rref1_scope.len()), rref1_scope),
        (text_serial(rref1_table.len()), rref1_table),
        (text_serial(rref1_col.len()), rref1_col),
        (INTEGER_I8_SERIAL, &rref1_row_id),
        (text_serial(ts.len()), ts),
        (INTEGER_I8_SERIAL, &rref1_file_id),
        (INTEGER_I8_SERIAL, &rref1_parent_id),
    ]);

    // Row 2: geopackage scope (all nullable columns are NULL)
    let rref2_scope = b"geopackage".as_ref();
    let rref2_ts = b"2025-01-01T00:00:00.000Z".as_ref();
    let rref2_file_id = encode_i64_be(999_i64);

    let rref2 = encode_record(&[
        (text_serial(rref2_scope.len()), rref2_scope),
        (NULL_SERIAL, &[]),
        (NULL_SERIAL, &[]),
        (NULL_SERIAL, &[]),
        (text_serial(rref2_ts.len()), rref2_ts),
        (INTEGER_I8_SERIAL_6, &rref2_file_id),
        (NULL_SERIAL, &[]),
    ]);

    let ref_cells = vec![(1_i64, rref1), (2_i64, rref2)];
    let ref_page = build_leaf_page(page_size, &ref_cells, 0);
    file[page_size * 2..page_size * 3].copy_from_slice(&ref_page);

    // sqlite_master on page 1
    let mut master_cells: Vec<(i64, Vec<u8>)> = Vec::new();
    append_master_entry(&mut master_cells, 1, "gpkg_metadata", 2);
    append_master_entry(&mut master_cells, 2, "gpkg_metadata_reference", 3);

    let master_page = build_leaf_page(page_size, &master_cells, 100);
    file[..page_size].copy_from_slice(&master_page);
    let hdr = make_sqlite_header(page_size as u16, n_pages, 0x4750_4B47);
    file[..100].copy_from_slice(&hdr);

    file
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — load_metadata() returns Ok([]) when table is absent
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_metadata_empty_when_table_absent() {
    // A minimal GPKG with no user tables at all.
    let data = make_minimal_sqlite(4096, 2);
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");
    let rows = gpkg.load_metadata().expect("load ok");
    assert!(
        rows.is_empty(),
        "expected empty vec when gpkg_metadata is absent, got {rows:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — load_metadata_references() returns Ok([]) when table is absent
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_metadata_reference_empty_when_table_absent() {
    let data = make_minimal_sqlite(4096, 2);
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");
    let rows = gpkg.load_metadata_references().expect("load ok");
    assert!(
        rows.is_empty(),
        "expected empty vec when gpkg_metadata_reference is absent, got {rows:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — MetadataScope::from_str for known variants
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_metadata_scope_from_str_all_variants() {
    assert_eq!(
        MetadataScope::from_str("undefined").unwrap(),
        MetadataScope::Undefined
    );
    assert_eq!(
        MetadataScope::from_str("fieldSession").unwrap(),
        MetadataScope::FieldSession
    );
    assert_eq!(
        MetadataScope::from_str("collectionSession").unwrap(),
        MetadataScope::CollectionSession
    );
    assert_eq!(
        MetadataScope::from_str("series").unwrap(),
        MetadataScope::Series
    );
    assert_eq!(
        MetadataScope::from_str("dataset").unwrap(),
        MetadataScope::Dataset
    );
    assert_eq!(
        MetadataScope::from_str("featureType").unwrap(),
        MetadataScope::FeatureType
    );
    assert_eq!(
        MetadataScope::from_str("feature").unwrap(),
        MetadataScope::Feature
    );
    assert_eq!(
        MetadataScope::from_str("attributeType").unwrap(),
        MetadataScope::AttributeType
    );
    assert_eq!(
        MetadataScope::from_str("attribute").unwrap(),
        MetadataScope::Attribute
    );
    assert_eq!(
        MetadataScope::from_str("tile").unwrap(),
        MetadataScope::Tile
    );
    assert_eq!(
        MetadataScope::from_str("model").unwrap(),
        MetadataScope::Model
    );
    assert_eq!(
        MetadataScope::from_str("catalog").unwrap(),
        MetadataScope::Catalog
    );
    assert_eq!(
        MetadataScope::from_str("schema").unwrap(),
        MetadataScope::Schema
    );
    assert_eq!(
        MetadataScope::from_str("taxonomy").unwrap(),
        MetadataScope::Taxonomy
    );
    assert_eq!(
        MetadataScope::from_str("software").unwrap(),
        MetadataScope::Software
    );
    assert_eq!(
        MetadataScope::from_str("service").unwrap(),
        MetadataScope::Service
    );
    assert_eq!(
        MetadataScope::from_str("collectionHardware").unwrap(),
        MetadataScope::CollectionHardware
    );
    assert_eq!(
        MetadataScope::from_str("nonGeographicDataset").unwrap(),
        MetadataScope::NonGeographicDataset
    );
    assert_eq!(
        MetadataScope::from_str("dimensionGroup").unwrap(),
        MetadataScope::DimensionGroup
    );
    // Unknown string → Undefined
    assert_eq!(
        MetadataScope::from_str("").unwrap(),
        MetadataScope::Undefined
    );
    assert_eq!(
        MetadataScope::from_str("DATASET").unwrap(),
        MetadataScope::Undefined
    );
    assert_eq!(
        MetadataScope::from_str("completely_unknown").unwrap(),
        MetadataScope::Undefined
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — MetadataScope from_str → as_str roundtrip
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_metadata_scope_roundtrip() {
    let cases = [
        "undefined",
        "fieldSession",
        "collectionSession",
        "series",
        "dataset",
        "featureType",
        "feature",
        "attributeType",
        "attribute",
        "tile",
        "model",
        "catalog",
        "schema",
        "taxonomy",
        "software",
        "service",
        "collectionHardware",
        "nonGeographicDataset",
        "dimensionGroup",
    ];
    for s in cases {
        let scope = MetadataScope::from_str(s).unwrap();
        assert_eq!(
            scope.as_str(),
            s,
            "roundtrip failed for {s:?}: from_str gave {scope:?} whose as_str is {:?}",
            scope.as_str()
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — ReferenceScope::from_str for all variants
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_reference_scope_from_str_variants() {
    assert_eq!(
        ReferenceScope::from_str("geopackage").unwrap(),
        ReferenceScope::GeoPackage
    );
    assert_eq!(
        ReferenceScope::from_str("table").unwrap(),
        ReferenceScope::Table
    );
    assert_eq!(
        ReferenceScope::from_str("column").unwrap(),
        ReferenceScope::Column
    );
    assert_eq!(
        ReferenceScope::from_str("row").unwrap(),
        ReferenceScope::Row
    );
    assert_eq!(
        ReferenceScope::from_str("row/col").unwrap(),
        ReferenceScope::RowCol
    );
    // Unknown → GeoPackage
    assert_eq!(
        ReferenceScope::from_str("").unwrap(),
        ReferenceScope::GeoPackage
    );
    assert_eq!(
        ReferenceScope::from_str("GEOPACKAGE").unwrap(),
        ReferenceScope::GeoPackage
    );
    assert_eq!(
        ReferenceScope::from_str("unknown").unwrap(),
        ReferenceScope::GeoPackage
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — ReferenceScope from_str → as_str roundtrip
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_reference_scope_roundtrip() {
    let cases = ["geopackage", "table", "column", "row", "row/col"];
    for s in cases {
        let scope = ReferenceScope::from_str(s).unwrap();
        assert_eq!(
            scope.as_str(),
            s,
            "roundtrip failed for {s:?}: from_str gave {scope:?} whose as_str is {:?}",
            scope.as_str()
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — corrupted file: Err propagated (non-table-absent error)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_metadata_gpkg_error_on_corrupt_data() {
    // Feed 200 bytes that are clearly not a valid SQLite file.
    // SqliteReader::from_bytes should fail immediately.
    let corrupt: Vec<u8> = (0u8..200u8).collect();
    let result = GeoPackage::from_bytes(corrupt);
    assert!(result.is_err(), "expected error for corrupt data, got Ok");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8 — load_metadata() parses a real gpkg_metadata row correctly
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_metadata_parses_single_row() {
    let data = build_gpkg_with_metadata_tables();
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");

    let rows = gpkg.load_metadata().expect("load_metadata ok");
    assert_eq!(rows.len(), 1, "expected 1 metadata row, got {}", rows.len());

    let row = &rows[0];
    assert_eq!(row.id, 42);
    assert_eq!(row.md_scope, MetadataScope::Dataset);
    assert_eq!(row.md_standard_uri, "http://www.isotc211.org/2005/gmd");
    assert_eq!(row.mime_type, "text/xml");
    assert_eq!(row.metadata, "<metadata/>");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9 — load_metadata_references() parses a real row correctly
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_metadata_references_parses_single_row() {
    let data = build_gpkg_with_metadata_tables();
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");

    let rows = gpkg
        .load_metadata_references()
        .expect("load_metadata_references ok");
    assert_eq!(
        rows.len(),
        1,
        "expected 1 reference row, got {}",
        rows.len()
    );

    let row = &rows[0];
    assert_eq!(row.reference_scope, ReferenceScope::Table);
    assert_eq!(row.table_name.as_deref(), Some("my_layer"));
    assert!(row.column_name.is_none(), "column_name should be None");
    assert!(row.row_id_value.is_none(), "row_id_value should be None");
    assert_eq!(row.timestamp, "2024-01-01T00:00:00.000Z");
    assert_eq!(row.md_file_id, 42);
    assert!(row.md_parent_id.is_none(), "md_parent_id should be None");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 10 — load_metadata() with multiple rows and various integer encodings
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_metadata_parses_multiple_rows() {
    let data = build_gpkg_with_multirow_metadata();
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");

    let rows = gpkg.load_metadata().expect("load_metadata ok");
    assert_eq!(
        rows.len(),
        2,
        "expected 2 metadata rows, got {}",
        rows.len()
    );

    // Row 1: featureType scope, id = 1 (i8 encoding)
    assert_eq!(rows[0].id, 1);
    assert_eq!(rows[0].md_scope, MetadataScope::FeatureType);
    assert_eq!(rows[0].md_standard_uri, "urn:ogc:def:crs");
    assert_eq!(rows[0].mime_type, "text/plain");
    assert_eq!(rows[0].metadata, "<a/>");

    // Row 2: service scope, id = 999 (i64 encoding)
    assert_eq!(rows[1].id, 999);
    assert_eq!(rows[1].md_scope, MetadataScope::Service);
    assert_eq!(rows[1].md_standard_uri, "http://example.com");
    assert_eq!(rows[1].mime_type, "application/json");
    assert_eq!(rows[1].metadata, "{}");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 11 — load_metadata_references() with multiple rows including all-null
//            optional fields and the row/col scope
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_metadata_references_parses_multiple_rows() {
    let data = build_gpkg_with_multirow_metadata();
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");

    let rows = gpkg
        .load_metadata_references()
        .expect("load_metadata_references ok");
    assert_eq!(
        rows.len(),
        2,
        "expected 2 reference rows, got {}",
        rows.len()
    );

    // Row 1: row/col scope with optional fields set
    let r1 = &rows[0];
    assert_eq!(r1.reference_scope, ReferenceScope::RowCol);
    assert_eq!(r1.table_name.as_deref(), Some("features"));
    assert_eq!(r1.column_name.as_deref(), Some("geom"));
    assert_eq!(r1.row_id_value, Some(7));
    assert_eq!(r1.timestamp, "2025-06-15T12:00:00.000Z");
    assert_eq!(r1.md_file_id, 1);
    assert_eq!(r1.md_parent_id, Some(0));

    // Row 2: geopackage scope with all nullable fields NULL
    let r2 = &rows[1];
    assert_eq!(r2.reference_scope, ReferenceScope::GeoPackage);
    assert!(r2.table_name.is_none());
    assert!(r2.column_name.is_none());
    assert!(r2.row_id_value.is_none());
    assert_eq!(r2.timestamp, "2025-01-01T00:00:00.000Z");
    assert_eq!(r2.md_file_id, 999);
    assert!(r2.md_parent_id.is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 12 — GpkgMetadata struct equality and Clone
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_gpkg_metadata_equality_and_clone() {
    let m = GpkgMetadata {
        id: 1,
        md_scope: MetadataScope::Dataset,
        md_standard_uri: "http://example.com".to_string(),
        mime_type: "text/xml".to_string(),
        metadata: "<x/>".to_string(),
    };
    let m2 = m.clone();
    assert_eq!(m, m2);
    assert_ne!(
        m,
        GpkgMetadata {
            id: 2,
            ..m2.clone()
        }
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 13 — GpkgMetadataReference struct equality and Clone
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_gpkg_metadata_reference_equality_and_clone() {
    let r = GpkgMetadataReference {
        reference_scope: ReferenceScope::Row,
        table_name: Some("t".to_string()),
        column_name: None,
        row_id_value: Some(99),
        timestamp: "2024-06-01T00:00:00.000Z".to_string(),
        md_file_id: 7,
        md_parent_id: None,
    };
    let r2 = r.clone();
    assert_eq!(r, r2);
    assert_ne!(
        r,
        GpkgMetadataReference {
            md_file_id: 8,
            ..r2.clone()
        }
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 14 — MetadataScope Debug representation contains expected text
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_metadata_scope_debug_format() {
    let s = format!("{:?}", MetadataScope::FeatureType);
    assert!(
        s.contains("FeatureType"),
        "Debug output {s:?} should contain 'FeatureType'"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 15 — ReferenceScope Debug representation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_reference_scope_debug_format() {
    let s = format!("{:?}", ReferenceScope::RowCol);
    assert!(
        s.contains("RowCol"),
        "Debug output {s:?} should contain 'RowCol'"
    );
}
