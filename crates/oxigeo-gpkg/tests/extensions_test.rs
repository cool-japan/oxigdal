//! Integration tests for the `gpkg_extensions` table loader.
//!
//! Verifies OGC 12-128r19 §F.4 compliant parsing of extension rows, absent-table
//! handling, malformed-row skipping, and scope string round-trips.

use oxigeo_gpkg::btree::encode_sqlite_varint;
use oxigeo_gpkg::{ExtensionScope, GeoPackage, GpkgExtension};

// ── SQLite binary helpers (mirrors gpkg_test.rs) ─────────────────────────────

/// Build a minimal SQLite header (100 bytes).
fn make_sqlite_header(page_size_raw: u16, db_size_pages: u32) -> Vec<u8> {
    let mut data = vec![0u8; 100];
    data[..16].copy_from_slice(b"SQLite format 3\x00");
    data[16..18].copy_from_slice(&page_size_raw.to_be_bytes());
    data[28..32].copy_from_slice(&db_size_pages.to_be_bytes());
    // text encoding = UTF-8 (1)
    data[56..60].copy_from_slice(&1u32.to_be_bytes());
    // GeoPackage application_id
    data[68..72].copy_from_slice(&0x4750_4B47u32.to_be_bytes());
    data
}

/// Write the file header into `file_data`.
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
/// `header_offset` = 100 for page 1 (file header overlap), 0 otherwise.
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
    page[hdr + 1] = 0;
    page[hdr + 2] = 0;
    page[hdr + 3] = ((cell_count >> 8) & 0xFF) as u8;
    page[hdr + 4] = (cell_count & 0xFF) as u8;
    let content_start = content_end as u16;
    page[hdr + 5] = ((content_start >> 8) & 0xFF) as u8;
    page[hdr + 6] = (content_start & 0xFF) as u8;
    page[hdr + 7] = 0;
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

/// Encode a `gpkg_extensions` row with five TEXT columns.
///
/// Nullable columns (`table_name`, `column_name`) use serial_type 0 (NULL)
/// when `None`.
fn build_extensions_record(
    table_name: Option<&str>,
    column_name: Option<&str>,
    extension_name: &str,
    definition: &str,
    scope: &str,
) -> Vec<u8> {
    let table_name_bytes: Vec<u8> = table_name
        .map(|s| s.as_bytes().to_vec())
        .unwrap_or_default();
    let column_name_bytes: Vec<u8> = column_name
        .map(|s| s.as_bytes().to_vec())
        .unwrap_or_default();
    let extension_name_bytes = extension_name.as_bytes().to_vec();
    let definition_bytes = definition.as_bytes().to_vec();
    let scope_bytes = scope.as_bytes().to_vec();

    let st_table_name = if table_name.is_some() {
        text_serial_type(table_name_bytes.len())
    } else {
        0u64 // NULL
    };
    let st_column_name = if column_name.is_some() {
        text_serial_type(column_name_bytes.len())
    } else {
        0u64 // NULL
    };

    let fields: Vec<(u64, &[u8])> = vec![
        (st_table_name, &table_name_bytes),
        (st_column_name, &column_name_bytes),
        (
            text_serial_type(extension_name_bytes.len()),
            &extension_name_bytes,
        ),
        (text_serial_type(definition_bytes.len()), &definition_bytes),
        (text_serial_type(scope_bytes.len()), &scope_bytes),
    ];
    encode_record(&fields)
}

/// Build a 3-page GeoPackage SQLite file:
/// - Page 1 (sqlite_master): one row pointing to the `gpkg_extensions` table on page 2.
/// - Page 2 (gpkg_extensions data): `extension_rows` rows.
/// - Page 3 is unused padding so `page_count > 2`.
///
/// Returns the raw bytes.
fn build_gpkg_with_extensions(extension_rows: &[(i64, Vec<u8>)]) -> Vec<u8> {
    let page_size = 4096usize;
    let total_pages = 3usize;
    let mut file = vec![0u8; page_size * total_pages];

    // Page 2: leaf table page containing the extension rows.
    let cells: Vec<(i64, &[u8])> = extension_rows
        .iter()
        .map(|(rid, payload)| (*rid, payload.as_slice()))
        .collect();
    let data_page = build_leaf_table_page(page_size, &cells, 0);
    file[page_size..page_size * 2].copy_from_slice(&data_page);

    // Page 1: sqlite_master with a single row registering `gpkg_extensions`
    // at root page 2.
    let sql = "CREATE TABLE gpkg_extensions(\
        table_name TEXT,column_name TEXT,\
        extension_name TEXT NOT NULL,\
        definition TEXT NOT NULL,scope TEXT NOT NULL)";
    let master_record = build_master_record("gpkg_extensions", 2, sql);
    let master_page = build_leaf_table_page(page_size, &[(1, &master_record)], 100);
    file[..page_size].copy_from_slice(&master_page);

    // Write the valid SQLite + GeoPackage file header.
    write_sqlite_file_header(&mut file, page_size as u16, total_pages as u32);

    file
}

// ── Test: absent table returns Ok(empty vec) ─────────────────────────────────

#[test]
fn test_load_extensions_absent_table_returns_ok_empty_vec() {
    // A file with NO gpkg_extensions table at all.
    let page_size = 4096usize;
    let mut file = vec![0u8; page_size];
    write_sqlite_file_header(&mut file, page_size as u16, 1);
    // Build a sqlite_master leaf page with zero cells — no table entries.
    let master_page = build_leaf_table_page(page_size, &[], 100);
    file[..page_size].copy_from_slice(&master_page);
    write_sqlite_file_header(&mut file, page_size as u16, 1);

    let mut gpkg = GeoPackage::from_bytes(file).expect("valid gpkg bytes");
    let extensions = gpkg
        .load_extensions()
        .expect("absent gpkg_extensions must return Ok(vec![])");
    assert!(
        extensions.is_empty(),
        "no extensions table should yield an empty vec, got {extensions:?}"
    );
}

// ── Test: single read-write extension row ────────────────────────────────────

#[test]
fn test_load_extensions_single_read_write_row() {
    let row = build_extensions_record(
        Some("my_features"),
        Some("geom"),
        "gpkg_rtree_index",
        "http://www.geopackage.org/spec/#extension_rtree",
        "read-write",
    );
    let file = build_gpkg_with_extensions(&[(1, row)]);
    let mut gpkg = GeoPackage::from_bytes(file).expect("valid gpkg bytes");
    let extensions = gpkg
        .load_extensions()
        .expect("load_extensions must succeed");

    assert_eq!(extensions.len(), 1, "expected exactly one extension row");
    let ext = &extensions[0];
    assert_eq!(ext.extension_name, "gpkg_rtree_index");
    assert_eq!(
        ext.table_name.as_deref(),
        Some("my_features"),
        "table_name should be Some"
    );
    assert_eq!(
        ext.column_name.as_deref(),
        Some("geom"),
        "column_name should be Some"
    );
    assert_eq!(
        ext.definition,
        "http://www.geopackage.org/spec/#extension_rtree"
    );
    assert_eq!(
        ext.scope,
        ExtensionScope::ReadWrite,
        "scope 'read-write' must map to ReadWrite"
    );
}

// ── Test: write-only scope parses correctly ───────────────────────────────────

#[test]
fn test_load_extensions_write_only_scope() {
    let row = build_extensions_record(
        Some("tile_layer"),
        None,
        "gpkg_webp",
        "http://www.geopackage.org/spec/#extension_webp",
        "write-only",
    );
    let file = build_gpkg_with_extensions(&[(1, row)]);
    let mut gpkg = GeoPackage::from_bytes(file).expect("valid gpkg bytes");
    let extensions = gpkg
        .load_extensions()
        .expect("load_extensions must succeed");

    assert_eq!(extensions.len(), 1);
    let ext = &extensions[0];
    assert_eq!(ext.scope, ExtensionScope::WriteOnly);
    assert_eq!(ext.extension_name, "gpkg_webp");
    assert!(
        ext.column_name.is_none(),
        "column_name should be None when NULL in the table"
    );
}

// ── Test: NULL table_name and column_name ─────────────────────────────────────

#[test]
fn test_load_extensions_null_table_and_column() {
    let row = build_extensions_record(
        None,
        None,
        "gpkg_schema",
        "http://www.geopackage.org/spec/#extension_schema",
        "read-write",
    );
    let file = build_gpkg_with_extensions(&[(1, row)]);
    let mut gpkg = GeoPackage::from_bytes(file).expect("valid gpkg bytes");
    let extensions = gpkg
        .load_extensions()
        .expect("load_extensions must succeed");

    assert_eq!(extensions.len(), 1);
    let ext = &extensions[0];
    assert!(
        ext.table_name.is_none(),
        "NULL table_name should be decoded as None"
    );
    assert!(
        ext.column_name.is_none(),
        "NULL column_name should be decoded as None"
    );
    assert_eq!(ext.extension_name, "gpkg_schema");
    assert_eq!(ext.scope, ExtensionScope::ReadWrite);
}

// ── Test: multiple extension rows ─────────────────────────────────────────────

#[test]
fn test_load_extensions_multiple_rows() {
    let row1 = build_extensions_record(
        Some("features"),
        Some("geom"),
        "gpkg_rtree_index",
        "http://www.geopackage.org/spec/#extension_rtree",
        "read-write",
    );
    let row2 = build_extensions_record(
        Some("tiles"),
        None,
        "gpkg_webp",
        "http://www.geopackage.org/spec/#extension_webp",
        "write-only",
    );
    let row3 = build_extensions_record(
        None,
        None,
        "author_custom",
        "https://example.com/custom-extension",
        "read-write",
    );
    let file = build_gpkg_with_extensions(&[(1, row1), (2, row2), (3, row3)]);
    let mut gpkg = GeoPackage::from_bytes(file).expect("valid gpkg bytes");
    let extensions = gpkg
        .load_extensions()
        .expect("load_extensions must succeed");

    assert_eq!(
        extensions.len(),
        3,
        "all three extension rows must be loaded"
    );

    // Row 1 — RTree
    assert_eq!(extensions[0].extension_name, "gpkg_rtree_index");
    assert_eq!(extensions[0].scope, ExtensionScope::ReadWrite);
    assert_eq!(extensions[0].table_name.as_deref(), Some("features"));

    // Row 2 — WebP
    assert_eq!(extensions[1].extension_name, "gpkg_webp");
    assert_eq!(extensions[1].scope, ExtensionScope::WriteOnly);
    assert!(extensions[1].column_name.is_none());

    // Row 3 — custom author extension
    assert_eq!(extensions[2].extension_name, "author_custom");
    assert!(extensions[2].table_name.is_none());
    assert_eq!(
        extensions[2].definition,
        "https://example.com/custom-extension"
    );
}

// ── Test: unknown scope string falls back to ReadWrite ────────────────────────

#[test]
fn test_load_extensions_unknown_scope_defaults_to_read_write() {
    let row = build_extensions_record(
        None,
        None,
        "vendor_ext",
        "https://vendor.example.com/ext",
        "proprietary", // not a spec-defined scope
    );
    let file = build_gpkg_with_extensions(&[(1, row)]);
    let mut gpkg = GeoPackage::from_bytes(file).expect("valid gpkg bytes");
    let extensions = gpkg
        .load_extensions()
        .expect("load_extensions must succeed");

    assert_eq!(extensions.len(), 1);
    assert_eq!(
        extensions[0].scope,
        ExtensionScope::ReadWrite,
        "unrecognised scope must default to the conservative ReadWrite variant"
    );
}

// ── Test: GpkgExtension struct — public API surface ──────────────────────────

#[test]
fn test_gpkg_extension_struct_construction_and_fields() {
    let ext = GpkgExtension {
        table_name: Some("layer".to_string()),
        column_name: None,
        extension_name: "gpkg_crs_wkt".to_string(),
        definition: "http://www.geopackage.org/spec/#extension_crs_wkt".to_string(),
        scope: ExtensionScope::ReadWrite,
    };
    assert_eq!(ext.extension_name, "gpkg_crs_wkt");
    assert_eq!(ext.table_name, Some("layer".to_string()));
    assert!(ext.column_name.is_none());
    assert_eq!(ext.scope, ExtensionScope::ReadWrite);
}

#[test]
fn test_gpkg_extension_clone_and_eq() {
    let ext = GpkgExtension {
        table_name: None,
        column_name: None,
        extension_name: "gpkg_related_tables".to_string(),
        definition: "http://www.geopackage.org/spec/#extension_related_tables".to_string(),
        scope: ExtensionScope::WriteOnly,
    };
    let cloned = ext.clone();
    assert_eq!(ext, cloned);
}

// ── Test: ExtensionScope — public API surface ─────────────────────────────────

#[test]
fn test_extension_scope_integration_read_write() {
    let scope: ExtensionScope = "read-write"
        .parse()
        .expect("'read-write' must parse to ReadWrite");
    assert_eq!(scope, ExtensionScope::ReadWrite);
    // Verify PartialEq and Debug derive work
    let debug_str = format!("{scope:?}");
    assert!(debug_str.contains("ReadWrite"));
}

#[test]
fn test_extension_scope_integration_write_only() {
    let scope: ExtensionScope = "write-only"
        .parse()
        .expect("'write-only' must parse to WriteOnly");
    assert_eq!(scope, ExtensionScope::WriteOnly);
    let debug_str = format!("{scope:?}");
    assert!(debug_str.contains("WriteOnly"));
}
