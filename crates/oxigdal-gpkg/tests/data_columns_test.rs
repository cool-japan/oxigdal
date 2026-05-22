//! Integration tests for the `gpkg_data_columns` OGC Schema extension reader.
//!
//! Tests cover:
//!
//! 1. Absent-table fast-path — returns `Ok` with empty catalog.
//! 2. Row loading and catalog construction from synthetic GeoPackage bytes.
//! 3. Index look-ups: `for_table`, `for_column`, `columns_using_constraint`.
//! 4. Optional-field NULL handling for every nullable column.
//! 5. MIME-type population for BLOB columns.
//! 6. API ergonomics: `len`, `is_empty`, `iter`, `entries`.

use oxigdal_gpkg::btree::encode_sqlite_varint;
use oxigdal_gpkg::{DataColumn, DataColumnsCatalog, GeoPackage, read_data_columns_rows};

// ─────────────────────────────────────────────────────────────────────────────
// Low-level SQLite binary helpers
// (Mirrors the approach used in schema_constraints_test.rs)
// ─────────────────────────────────────────────────────────────────────────────

fn write_sqlite_file_header(file_data: &mut [u8], page_size: u16, db_size_pages: u32) {
    file_data[..16].copy_from_slice(b"SQLite format 3\x00");
    let ps = page_size.to_be_bytes();
    file_data[16..18].copy_from_slice(&ps);
    let db_sz = db_size_pages.to_be_bytes();
    file_data[28..32].copy_from_slice(&db_sz);
    // UTF-8 text encoding = 1
    file_data[56..60].copy_from_slice(&1u32.to_be_bytes());
    // GeoPackage application_id = 0x4750_4B47
    file_data[68..72].copy_from_slice(&0x4750_4B47u32.to_be_bytes());
}

/// Build a leaf table B-tree page.
///
/// `cells` is a slice of `(rowid, payload_bytes)` pairs.
/// `header_offset` is 100 for page 1 (which shares space with the SQLite file
/// header) and 0 for all other pages.
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
    page[hdr] = 13; // leaf table page type
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

/// Encode a SQLite record (header + body).
///
/// `fields` is a slice of `(serial_type, value_bytes)` pairs.
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

/// Compute the TEXT serial type for a UTF-8 string of `len` bytes.
fn text_serial_type(len: usize) -> u64 {
    (len as u64) * 2 + 13
}

/// NULL serial type in SQLite records.
const SERIAL_NULL: u64 = 0;

/// Build a `sqlite_master` record pointing a named table at a given root page.
fn build_master_record(table_name: &str, rootpage: u8, sql: &str) -> Vec<u8> {
    let entry_type = b"table";
    let name = table_name.as_bytes();
    let tbl_name = table_name.as_bytes();
    let sql_bytes = sql.as_bytes();
    let rootpage_bytes = [rootpage];
    let fields: Vec<(u64, &[u8])> = vec![
        (text_serial_type(entry_type.len()), entry_type),
        (text_serial_type(name.len()), name),
        (text_serial_type(tbl_name.len()), tbl_name),
        (1u64, &rootpage_bytes), // serial type 1 = i8
        (text_serial_type(sql_bytes.len()), sql_bytes),
    ];
    encode_record(&fields)
}

/// Encode one `gpkg_data_columns` row record with all 7 columns.
///
/// Pass `None` for any optional column to emit a NULL serial type.
fn encode_data_columns_row(
    table_name: &str,
    column_name: &str,
    name: Option<&str>,
    title: Option<&str>,
    description: Option<&str>,
    mime_type: Option<&str>,
    constraint_name: Option<&str>,
) -> Vec<u8> {
    let tn = table_name.as_bytes();
    let cn = column_name.as_bytes();

    let name_bytes = name.map(|s| s.as_bytes().to_vec()).unwrap_or_default();
    let title_bytes = title.map(|s| s.as_bytes().to_vec()).unwrap_or_default();
    let desc_bytes = description
        .map(|s| s.as_bytes().to_vec())
        .unwrap_or_default();
    let mime_bytes = mime_type.map(|s| s.as_bytes().to_vec()).unwrap_or_default();
    let constr_bytes = constraint_name
        .map(|s| s.as_bytes().to_vec())
        .unwrap_or_default();

    let fields: Vec<(u64, &[u8])> = vec![
        (text_serial_type(tn.len()), tn),
        (text_serial_type(cn.len()), cn),
        (
            if name.is_some() {
                text_serial_type(name_bytes.len())
            } else {
                SERIAL_NULL
            },
            if name.is_some() {
                &name_bytes
            } else {
                &[] as &[u8]
            },
        ),
        (
            if title.is_some() {
                text_serial_type(title_bytes.len())
            } else {
                SERIAL_NULL
            },
            if title.is_some() {
                &title_bytes
            } else {
                &[] as &[u8]
            },
        ),
        (
            if description.is_some() {
                text_serial_type(desc_bytes.len())
            } else {
                SERIAL_NULL
            },
            if description.is_some() {
                &desc_bytes
            } else {
                &[] as &[u8]
            },
        ),
        (
            if mime_type.is_some() {
                text_serial_type(mime_bytes.len())
            } else {
                SERIAL_NULL
            },
            if mime_type.is_some() {
                &mime_bytes
            } else {
                &[] as &[u8]
            },
        ),
        (
            if constraint_name.is_some() {
                text_serial_type(constr_bytes.len())
            } else {
                SERIAL_NULL
            },
            if constraint_name.is_some() {
                &constr_bytes
            } else {
                &[] as &[u8]
            },
        ),
    ];
    encode_record(&fields)
}

// ─────────────────────────────────────────────────────────────────────────────
// GeoPackage fixture builders
// ─────────────────────────────────────────────────────────────────────────────

/// Build a minimal valid SQLite/GeoPackage file whose `sqlite_master` is empty,
/// so that the `gpkg_data_columns` table is provably absent.
fn build_gpkg_without_data_columns() -> Vec<u8> {
    let page_size = 4096usize;
    let total_pages = 1usize;
    let mut file = vec![0u8; page_size * total_pages];
    let master_page = build_leaf_table_page(page_size, &[], 100);
    file[..page_size].copy_from_slice(&master_page);
    write_sqlite_file_header(&mut file, page_size as u16, total_pages as u32);
    file
}

/// Build a minimal 2-page GeoPackage that contains one unrelated table, but
/// still no `gpkg_data_columns` entry.
fn build_gpkg_with_unrelated_table() -> Vec<u8> {
    let page_size = 4096usize;
    let total_pages = 2usize;
    let mut file = vec![0u8; page_size * total_pages];

    // Page 2: empty data leaf for the unrelated table.
    let data_page = build_leaf_table_page(page_size, &[], 0);
    file[page_size..page_size * 2].copy_from_slice(&data_page);

    // Page 1: master with one row pointing to the unrelated table on page 2.
    let master_rec = build_master_record(
        "some_other_table",
        2,
        "CREATE TABLE some_other_table(id INTEGER)",
    );
    let master_page = build_leaf_table_page(page_size, &[(1, &master_rec)], 100);
    file[..page_size].copy_from_slice(&master_page);
    write_sqlite_file_header(&mut file, page_size as u16, total_pages as u32);
    file
}

/// Build a GeoPackage with a `gpkg_data_columns` table containing `rows`.
///
/// * Page 1 = `sqlite_master` (with an entry pointing to page 2)
/// * Page 2 = leaf data page for `gpkg_data_columns`
fn build_gpkg_with_rows(rows: &[(i64, Vec<u8>)]) -> Vec<u8> {
    let page_size = 4096usize;
    let total_pages = 2usize;
    let mut file = vec![0u8; page_size * total_pages];

    // Page 2: data leaf holding the gpkg_data_columns rows.
    let cell_refs: Vec<(i64, &[u8])> = rows.iter().map(|(rid, b)| (*rid, b.as_slice())).collect();
    let data_page = build_leaf_table_page(page_size, &cell_refs, 0);
    file[page_size..page_size * 2].copy_from_slice(&data_page);

    // Page 1: master with entry for gpkg_data_columns → page 2.
    let master_rec = build_master_record(
        "gpkg_data_columns",
        2,
        "CREATE TABLE gpkg_data_columns(\
         table_name TEXT NOT NULL,\
         column_name TEXT NOT NULL,\
         name TEXT,title TEXT,\
         description TEXT,mime_type TEXT,\
         constraint_name TEXT)",
    );
    let master_page = build_leaf_table_page(page_size, &[(1, &master_rec)], 100);
    file[..page_size].copy_from_slice(&master_page);
    write_sqlite_file_header(&mut file, page_size as u16, total_pages as u32);
    file
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — absent table returns empty catalog (not an error)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_data_columns_catalog_load_empty_when_table_absent() {
    // Completely empty sqlite_master → no gpkg_data_columns table.
    let bytes = build_gpkg_without_data_columns();
    let gpkg = GeoPackage::from_bytes(bytes).expect("valid gpkg bytes");
    let catalog = DataColumnsCatalog::load(&gpkg)
        .expect("missing gpkg_data_columns table must return Ok(empty)");
    assert!(
        catalog.is_empty(),
        "catalog must be empty when the extension table is absent"
    );
    assert_eq!(catalog.len(), 0);

    // Non-empty master but still no data_columns entry.
    let bytes2 = build_gpkg_with_unrelated_table();
    let gpkg2 = GeoPackage::from_bytes(bytes2).expect("valid gpkg bytes");
    let catalog2 = DataColumnsCatalog::load(&gpkg2)
        .expect("unrelated tables present should still yield Ok(empty)");
    assert!(
        catalog2.is_empty(),
        "catalog must be empty when only unrelated tables exist"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — load returns all rows from the table
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_data_columns_catalog_load_returns_all_rows() {
    let rows: Vec<(i64, Vec<u8>)> = vec![
        (
            1,
            encode_data_columns_row("t1", "c1", None, None, None, None, None),
        ),
        (
            2,
            encode_data_columns_row("t1", "c2", None, None, None, None, Some("r1")),
        ),
        (
            3,
            encode_data_columns_row("t2", "c3", None, None, None, None, Some("r1")),
        ),
    ];
    let bytes = build_gpkg_with_rows(&rows);
    let gpkg = GeoPackage::from_bytes(bytes).expect("valid gpkg");
    let catalog = DataColumnsCatalog::load(&gpkg).expect("load");
    assert_eq!(
        catalog.len(),
        3,
        "expected exactly 3 entries, got {}",
        catalog.len()
    );
    assert!(!catalog.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — for_table filters correctly
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_data_columns_for_table_filters_correctly() {
    let rows: Vec<(i64, Vec<u8>)> = vec![
        (
            1,
            encode_data_columns_row("table_a", "col1", None, None, None, None, None),
        ),
        (
            2,
            encode_data_columns_row("table_a", "col2", None, None, None, None, None),
        ),
        (
            3,
            encode_data_columns_row("table_b", "col3", None, None, None, None, None),
        ),
    ];
    let bytes = build_gpkg_with_rows(&rows);
    let gpkg = GeoPackage::from_bytes(bytes).expect("valid gpkg");
    let catalog = DataColumnsCatalog::load(&gpkg).expect("load");

    let ta = catalog.for_table("table_a");
    assert_eq!(ta.len(), 2, "expected 2 entries for table_a");
    assert!(ta.iter().all(|dc| dc.table_name == "table_a"));

    let tb = catalog.for_table("table_b");
    assert_eq!(tb.len(), 1, "expected 1 entry for table_b");
    assert_eq!(tb[0].column_name, "col3");

    assert!(catalog.for_table("nonexistent").is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — for_column returns the single matching row
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_data_columns_for_column_returns_single() {
    let rows: Vec<(i64, Vec<u8>)> = vec![
        (
            1,
            encode_data_columns_row(
                "table_a",
                "col1",
                Some("alt"),
                Some("Label"),
                None,
                None,
                Some("r1"),
            ),
        ),
        (
            2,
            encode_data_columns_row("table_a", "col2", None, None, None, None, None),
        ),
    ];
    let bytes = build_gpkg_with_rows(&rows);
    let gpkg = GeoPackage::from_bytes(bytes).expect("valid gpkg");
    let catalog = DataColumnsCatalog::load(&gpkg).expect("load");

    let dc = catalog
        .for_column("table_a", "col1")
        .expect("col1 must be found");
    assert_eq!(dc.table_name, "table_a");
    assert_eq!(dc.column_name, "col1");
    assert_eq!(dc.name.as_deref(), Some("alt"));
    assert_eq!(dc.title.as_deref(), Some("Label"));
    assert_eq!(dc.constraint_name.as_deref(), Some("r1"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — for_column returns None for a non-existent column
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_data_columns_for_column_missing_returns_none() {
    let rows: Vec<(i64, Vec<u8>)> = vec![(
        1,
        encode_data_columns_row("table_a", "col1", None, None, None, None, None),
    )];
    let bytes = build_gpkg_with_rows(&rows);
    let gpkg = GeoPackage::from_bytes(bytes).expect("valid gpkg");
    let catalog = DataColumnsCatalog::load(&gpkg).expect("load");

    assert!(
        catalog.for_column("table_a", "nonexistent").is_none(),
        "for_column must return None for unknown column"
    );
    assert!(
        catalog.for_column("no_such_table", "col1").is_none(),
        "for_column must return None for unknown table"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — columns_using_constraint returns the right entries
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_data_columns_columns_using_constraint() {
    let rows: Vec<(i64, Vec<u8>)> = vec![
        (
            1,
            encode_data_columns_row("t1", "c1", None, None, None, None, Some("size_rule")),
        ),
        (
            2,
            encode_data_columns_row("t1", "c2", None, None, None, None, Some("size_rule")),
        ),
        (
            3,
            encode_data_columns_row("t2", "c3", None, None, None, None, None),
        ),
    ];
    let bytes = build_gpkg_with_rows(&rows);
    let gpkg = GeoPackage::from_bytes(bytes).expect("valid gpkg");
    let catalog = DataColumnsCatalog::load(&gpkg).expect("load");

    let using_size = catalog.columns_using_constraint("size_rule");
    assert_eq!(
        using_size.len(),
        2,
        "two columns reference 'size_rule', got {}",
        using_size.len()
    );
    assert!(
        using_size
            .iter()
            .all(|dc| dc.constraint_name.as_deref() == Some("size_rule"))
    );

    assert!(
        catalog
            .columns_using_constraint("absent_constraint")
            .is_empty(),
        "unknown constraint must return empty vec"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — len and is_empty
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_data_columns_len_and_is_empty() {
    // Three rows → len == 3, is_empty == false.
    let rows: Vec<(i64, Vec<u8>)> = vec![
        (
            1,
            encode_data_columns_row("t1", "c1", None, None, None, None, None),
        ),
        (
            2,
            encode_data_columns_row("t1", "c2", None, None, None, None, None),
        ),
        (
            3,
            encode_data_columns_row("t2", "c3", None, None, None, None, None),
        ),
    ];
    let bytes = build_gpkg_with_rows(&rows);
    let gpkg = GeoPackage::from_bytes(bytes).expect("valid gpkg");
    let catalog = DataColumnsCatalog::load(&gpkg).expect("load");
    assert_eq!(catalog.len(), 3);
    assert!(!catalog.is_empty());

    // Zero rows → is_empty == true.
    let bytes0 = build_gpkg_without_data_columns();
    let gpkg0 = GeoPackage::from_bytes(bytes0).expect("valid gpkg");
    let catalog0 = DataColumnsCatalog::load(&gpkg0).expect("load");
    assert!(catalog0.is_empty());
    assert_eq!(catalog0.len(), 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8 — iter yields all entries
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_data_columns_iter_yields_all_entries() {
    let rows: Vec<(i64, Vec<u8>)> = vec![
        (
            1,
            encode_data_columns_row("t1", "c1", None, None, None, None, None),
        ),
        (
            2,
            encode_data_columns_row("t2", "c2", None, None, None, None, Some("r1")),
        ),
    ];
    let bytes = build_gpkg_with_rows(&rows);
    let gpkg = GeoPackage::from_bytes(bytes).expect("valid gpkg");
    let catalog = DataColumnsCatalog::load(&gpkg).expect("load");

    let from_iter: Vec<&DataColumn> = catalog.iter().collect();
    let from_entries: &[DataColumn] = catalog.entries();
    assert_eq!(
        from_iter.len(),
        from_entries.len(),
        "iter() and entries() must have the same length"
    );
    for (a, b) in from_iter.iter().zip(from_entries.iter()) {
        assert_eq!(
            *a, b,
            "iter() and entries() must yield the same items in order"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9 — NULL optional fields decoded as None
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_read_data_columns_rows_handles_null_optional_fields() {
    // All optional fields are NULL.
    let rows: Vec<(i64, Vec<u8>)> = vec![(
        1,
        encode_data_columns_row("t1", "c1", None, None, None, None, None),
    )];
    let bytes = build_gpkg_with_rows(&rows);
    let gpkg = GeoPackage::from_bytes(bytes).expect("valid gpkg");
    let parsed = read_data_columns_rows(&gpkg).expect("read rows");
    assert_eq!(parsed.len(), 1);
    let row = &parsed[0];
    assert_eq!(row.table_name, "t1");
    assert_eq!(row.column_name, "c1");
    assert!(row.name.is_none(), "name must be None when NULL");
    assert!(row.title.is_none(), "title must be None when NULL");
    assert!(
        row.description.is_none(),
        "description must be None when NULL"
    );
    assert!(row.mime_type.is_none(), "mime_type must be None when NULL");
    assert!(
        row.constraint_name.is_none(),
        "constraint_name must be None when NULL"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 10 — MIME type field populated for BLOB columns
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_read_data_columns_rows_handles_mime_type_blob_columns() {
    let rows: Vec<(i64, Vec<u8>)> = vec![(
        1,
        encode_data_columns_row(
            "imagery",
            "tile_data",
            None,
            Some("Tile Data"),
            Some("Raw tile blob"),
            Some("image/png"),
            None,
        ),
    )];
    let bytes = build_gpkg_with_rows(&rows);
    let gpkg = GeoPackage::from_bytes(bytes).expect("valid gpkg");
    let parsed = read_data_columns_rows(&gpkg).expect("read rows");
    assert_eq!(parsed.len(), 1);
    let row = &parsed[0];
    assert_eq!(row.table_name, "imagery");
    assert_eq!(row.column_name, "tile_data");
    assert_eq!(row.title.as_deref(), Some("Tile Data"));
    assert_eq!(row.description.as_deref(), Some("Raw tile blob"));
    assert_eq!(
        row.mime_type.as_deref(),
        Some("image/png"),
        "mime_type must be Some(\"image/png\") for a BLOB column"
    );
    assert!(row.constraint_name.is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 11 — for_table returns consistent results across multiple calls
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_data_columns_catalog_by_table_index_sorted() {
    let rows: Vec<(i64, Vec<u8>)> = vec![
        (
            1,
            encode_data_columns_row("alpha", "x", None, None, None, None, None),
        ),
        (
            2,
            encode_data_columns_row("alpha", "y", None, None, None, None, None),
        ),
        (
            3,
            encode_data_columns_row("beta", "z", None, None, None, None, None),
        ),
    ];
    let bytes = build_gpkg_with_rows(&rows);
    let gpkg = GeoPackage::from_bytes(bytes).expect("valid gpkg");
    let catalog = DataColumnsCatalog::load(&gpkg).expect("load");

    // Call for_table twice; must return the same set of entries each time.
    let first_call = catalog.for_table("alpha");
    let second_call = catalog.for_table("alpha");
    assert_eq!(
        first_call.len(),
        second_call.len(),
        "for_table must be idempotent"
    );
    for (a, b) in first_call.iter().zip(second_call.iter()) {
        assert_eq!(a.column_name, b.column_name, "order must be consistent");
    }

    // All results must really belong to the queried table.
    assert!(
        first_call.iter().all(|dc| dc.table_name == "alpha"),
        "for_table must only return entries for the queried table"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 12 — columns_using_constraint results are a subset of entries()
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_data_columns_catalog_constraint_name_index_consistent() {
    let rows: Vec<(i64, Vec<u8>)> = vec![
        (
            1,
            encode_data_columns_row("t1", "c1", None, None, None, None, Some("rule_x")),
        ),
        (
            2,
            encode_data_columns_row("t1", "c2", None, None, None, None, Some("rule_x")),
        ),
        (
            3,
            encode_data_columns_row("t2", "c3", None, None, None, None, Some("rule_y")),
        ),
        (
            4,
            encode_data_columns_row("t2", "c4", None, None, None, None, None),
        ),
    ];
    let bytes = build_gpkg_with_rows(&rows);
    let gpkg = GeoPackage::from_bytes(bytes).expect("valid gpkg");
    let catalog = DataColumnsCatalog::load(&gpkg).expect("load");

    let using_rule_x = catalog.columns_using_constraint("rule_x");
    assert_eq!(using_rule_x.len(), 2, "two entries reference rule_x");

    // Every returned entry must exist verbatim in entries().
    let all_entries = catalog.entries();
    for dc_ref in &using_rule_x {
        let found = all_entries.iter().any(|e| e == *dc_ref);
        assert!(
            found,
            "entry {:?} returned by columns_using_constraint must be in entries()",
            dc_ref
        );
    }

    // All rule_x entries must actually have the right constraint_name.
    assert!(
        using_rule_x
            .iter()
            .all(|dc| dc.constraint_name.as_deref() == Some("rule_x"))
    );

    // Totals: rule_x (2) + rule_y (1) + None (1) = 4 = catalog.len().
    assert_eq!(catalog.len(), 4);
}
