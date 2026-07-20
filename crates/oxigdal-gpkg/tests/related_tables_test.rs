//! Integration tests for the GPKG Related Tables Extension parser.
//!
//! Verifies `gpkg_relations` table loading (`GeoPackage::load_relations`) and
//! mapping-table loading (`GeoPackage::load_mapping_table`) against hand-crafted
//! binary SQLite files, following the same zero-dependency pattern used in
//! `metadata_test.rs` and `extensions_test.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use oxigdal_gpkg::btree::encode_sqlite_varint;
use oxigdal_gpkg::{GeoPackage, GpkgRelation, MappingRow, RelationType};

// ─────────────────────────────────────────────────────────────────────────────
// SQLite binary builder helpers (mirrors metadata_test.rs, kept self-contained)
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

/// Build a single-page SQLite file whose page-1 is an empty sqlite_master
/// leaf table page (no rows, no user tables).
fn make_minimal_sqlite(page_size: usize, n_pages: u32) -> Vec<u8> {
    let total = (page_size * n_pages as usize).max(page_size);
    let mut file = vec![0u8; total];

    let empty_master = build_leaf_page(page_size, &[], 100);
    file[..page_size].copy_from_slice(&empty_master);

    let hdr = make_sqlite_header(page_size as u16, n_pages, 0x4750_4B47);
    file[..100].copy_from_slice(&hdr);

    file
}

/// Encode a SQLite record: `[hdr_len varint][serial type varints…][values…]`.
///
/// Each `(serial_type, value_bytes)` element provides one column.
fn encode_record(fields: &[(u64, &[u8])]) -> Vec<u8> {
    let st_varints: Vec<Vec<u8>> = fields
        .iter()
        .map(|(st, _)| encode_sqlite_varint(*st))
        .collect();
    let st_bytes: usize = st_varints.iter().map(|v| v.len()).sum();

    // Self-referential header length includes its own varint encoding.
    let mut hdr_len = st_bytes + 1;
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

/// Write a leaf table B-tree page (page type 0x0D).
///
/// `header_offset` must be 100 for page 1 and 0 for all other pages.
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

    // Cell pointer array (2 bytes per pointer, immediately after the 8-byte
    // B-tree page header).
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

/// SQLite serial type 1 — signed 8-bit integer (1 byte).
const INTEGER_I8_SERIAL: u64 = 1;

/// Encode an `i64` as a big-endian signed 8-byte integer (serial type 6).
fn encode_i64_be(v: i64) -> Vec<u8> {
    v.to_be_bytes().to_vec()
}

/// Append a `sqlite_master` "table" entry to `master_cells`.
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
    let rootpage_b = [root_page];

    let record = encode_record(&[
        (text_serial(entry_type.len()), entry_type),
        (text_serial(name.len()), name),
        (text_serial(tbl_name.len()), tbl_name),
        (INTEGER_I8_SERIAL, &rootpage_b),
        (text_serial(sql.len()), sql.as_slice()),
    ]);
    master_cells.push((rowid, record));
}

// ─────────────────────────────────────────────────────────────────────────────
// Build a SQLite binary with gpkg_relations populated
// ─────────────────────────────────────────────────────────────────────────────

/// Build a 4-page GeoPackage binary that contains:
///
/// - Page 1: `sqlite_master` (two entries: `gpkg_relations` → page 2,
///   `features_media` mapping table → page 3; page 4 is just padding).
/// - Page 2: `gpkg_relations` — one row describing a media relationship.
/// - Page 3: `features_media` mapping table — two mapping rows.
///
/// The `gpkg_relations` row encodes:
/// - id = 1
/// - base_table_name = "buildings"
/// - base_primary_column = "id"
/// - related_table_name = "attachments"
/// - related_primary_column = "id"
/// - relation_name = "media"
/// - mapping_table_name = "buildings_media"
fn build_gpkg_with_relations() -> Vec<u8> {
    let page_size: usize = 4096;
    let n_pages: u32 = 4;
    let mut file = vec![0u8; page_size * n_pages as usize];

    // ── Page 2: gpkg_relations — one row ────────────────────────────────────
    let rel_id: i8 = 1;
    let base_tbl = b"buildings".as_ref();
    let base_pk = b"id".as_ref();
    let rel_tbl = b"attachments".as_ref();
    let rel_pk = b"id".as_ref();
    let rel_name = b"media".as_ref();
    let mapping_tbl = b"buildings_media".as_ref();

    let relations_record = encode_record(&[
        (INTEGER_I8_SERIAL, &[rel_id as u8]),          // id
        (text_serial(base_tbl.len()), base_tbl),       // base_table_name
        (text_serial(base_pk.len()), base_pk),         // base_primary_column
        (text_serial(rel_tbl.len()), rel_tbl),         // related_table_name
        (text_serial(rel_pk.len()), rel_pk),           // related_primary_column
        (text_serial(rel_name.len()), rel_name),       // relation_name
        (text_serial(mapping_tbl.len()), mapping_tbl), // mapping_table_name
    ]);
    let relations_page = build_leaf_page(page_size, &[(1, relations_record)], 0);
    file[page_size..page_size * 2].copy_from_slice(&relations_page);

    // ── Page 3: buildings_media mapping table — two rows ─────────────────────
    // Row 1: id=1, base_id=10, related_id=100
    let map1 = encode_record(&[
        (INTEGER_I8_SERIAL, &[1u8]),  // id
        (INTEGER_I8_SERIAL, &[10u8]), // base_id
        (1u64, &[100u8]),             // related_id (serial type 1 = i8)
    ]);
    // Row 2: id=2, base_id=20, related_id=200 — use i64 serial (type 6) for variety
    let base_id2 = encode_i64_be(20);
    let rel_id2 = encode_i64_be(200);
    let map2 = encode_record(&[
        (INTEGER_I8_SERIAL, &[2u8]), // id
        (6u64, base_id2.as_slice()), // base_id (i64)
        (6u64, rel_id2.as_slice()),  // related_id (i64)
    ]);
    let mapping_page = build_leaf_page(page_size, &[(1, map1), (2, map2)], 0);
    file[page_size * 2..page_size * 3].copy_from_slice(&mapping_page);

    // ── Page 1: sqlite_master ────────────────────────────────────────────────
    let mut master_cells: Vec<(i64, Vec<u8>)> = Vec::new();
    append_master_entry(&mut master_cells, 1, "gpkg_relations", 2);
    append_master_entry(&mut master_cells, 2, "buildings_media", 3);

    let master_page = build_leaf_page(page_size, &master_cells, 100);
    file[..page_size].copy_from_slice(&master_page);

    let hdr = make_sqlite_header(page_size as u16, n_pages, 0x4750_4B47);
    file[..100].copy_from_slice(&hdr);

    file
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — load_relations() returns Ok([]) when table is absent
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_relations_empty_when_table_absent() {
    let data = make_minimal_sqlite(4096, 2);
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");
    let rels = gpkg.load_relations().expect("load_relations ok");
    assert!(
        rels.is_empty(),
        "expected empty vec when gpkg_relations is absent, got {rels:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — load_mapping_table() returns Ok([]) when table is absent
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_mapping_table_empty_when_absent() {
    let data = make_minimal_sqlite(4096, 2);
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");
    let rows = gpkg
        .load_mapping_table("nonexistent_mapping_table")
        .expect("load_mapping_table ok");
    assert!(
        rows.is_empty(),
        "expected empty vec for absent mapping table, got {rows:?}"
    );
}

// Helper: parse a RelationType from a string (infallible FromStr)
fn parse_rt(s: &str) -> RelationType {
    s.parse().unwrap()
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — RelationType::from_str for all five known variants
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_relation_type_from_str_all_variants() {
    assert_eq!(parse_rt("media"), RelationType::Media);
    assert_eq!(
        parse_rt("simple_attributes"),
        RelationType::SimpleAttributes
    );
    assert_eq!(parse_rt("related_features"), RelationType::RelatedFeatures);
    assert_eq!(
        parse_rt("features"),
        RelationType::RelatedFeatures,
        "legacy alias 'features' should also map to RelatedFeatures"
    );
    assert_eq!(parse_rt("tiles"), RelationType::Tiles);
    assert_eq!(parse_rt("attributes"), RelationType::Attributes);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — RelationType::Other for an unknown string
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_relation_type_other_variant() {
    let rt = parse_rt("custom");
    assert_eq!(
        rt,
        RelationType::Other("custom".to_owned()),
        "unknown string must produce RelationType::Other"
    );
    assert_eq!(
        rt.as_str(),
        "custom",
        "as_str() must return the raw string for Other"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — from_str → as_str roundtrip for all canonical variants
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_relation_type_roundtrip() {
    let cases = [
        "media",
        "simple_attributes",
        "related_features",
        "tiles",
        "attributes",
    ];
    for s in cases {
        let rt = parse_rt(s);
        assert_eq!(
            rt.as_str(),
            s,
            "roundtrip failed for {s:?}: parse gave {rt:?}, as_str returned {:?}",
            rt.as_str()
        );
    }

    // Other variant roundtrip
    let raw = "org_special_relation";
    let rt = parse_rt(raw);
    assert_eq!(rt.as_str(), raw);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — GpkgRelation Clone + PartialEq
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_gpkg_relation_struct_clone() {
    let rel = GpkgRelation {
        id: 3,
        base_table_name: "features".to_owned(),
        base_primary_column: "fid".to_owned(),
        related_table_name: "documents".to_owned(),
        related_primary_column: "doc_id".to_owned(),
        relation_name: RelationType::Media,
        mapping_table_name: "features_documents".to_owned(),
    };
    let cloned = rel.clone();
    assert_eq!(rel, cloned, "cloned GpkgRelation must equal the original");

    // Ensure inequality when a field differs
    let mut different = cloned.clone();
    different.id = 99;
    assert_ne!(rel, different);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — MappingRow Clone + PartialEq
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_mapping_row_struct_clone() {
    let row = MappingRow {
        id: 5,
        base_id: 100,
        related_id: 200,
    };
    let cloned = row.clone();
    assert_eq!(row, cloned, "cloned MappingRow must equal the original");

    let mut different = cloned.clone();
    different.base_id = 999;
    assert_ne!(row, different);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8 — load_relations() parses a synthesised gpkg_relations row correctly
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_relations_with_synthesized_data() {
    let data = build_gpkg_with_relations();
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");

    let rels = gpkg.load_relations().expect("load_relations ok");
    assert_eq!(rels.len(), 1, "expected 1 relation row, got {}", rels.len());

    let rel = &rels[0];
    assert_eq!(rel.id, 1);
    assert_eq!(rel.base_table_name, "buildings");
    assert_eq!(rel.base_primary_column, "id");
    assert_eq!(rel.related_table_name, "attachments");
    assert_eq!(rel.related_primary_column, "id");
    assert_eq!(rel.relation_name, RelationType::Media);
    assert_eq!(rel.mapping_table_name, "buildings_media");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9 — load_mapping_table() parses synthesised mapping rows correctly
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_mapping_table_with_synthesized_data() {
    let data = build_gpkg_with_relations();
    let gpkg = GeoPackage::from_bytes(data).expect("valid gpkg");

    let rows = gpkg
        .load_mapping_table("buildings_media")
        .expect("load_mapping_table ok");
    assert_eq!(rows.len(), 2, "expected 2 mapping rows, got {}", rows.len());

    // Row 1: id=1, base_id=10, related_id=100
    assert_eq!(rows[0].id, 1);
    assert_eq!(rows[0].base_id, 10);
    assert_eq!(rows[0].related_id, 100);

    // Row 2: id=2, base_id=20, related_id=200
    assert_eq!(rows[1].id, 2);
    assert_eq!(rows[1].base_id, 20);
    assert_eq!(rows[1].related_id, 200);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 10 — Debug representations contain expected text
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_relation_type_debug_format() {
    assert!(format!("{:?}", RelationType::Media).contains("Media"));
    assert!(format!("{:?}", RelationType::SimpleAttributes).contains("SimpleAttributes"));
    assert!(format!("{:?}", RelationType::RelatedFeatures).contains("RelatedFeatures"));
    assert!(format!("{:?}", RelationType::Tiles).contains("Tiles"));
    assert!(format!("{:?}", RelationType::Attributes).contains("Attributes"));
    let dbg = format!("{:?}", RelationType::Other("my_ext".to_owned()));
    assert!(dbg.contains("Other") && dbg.contains("my_ext"));
}

#[test]
fn test_gpkg_relation_debug_format() {
    let rel = GpkgRelation {
        id: 42,
        base_table_name: "base".to_owned(),
        base_primary_column: "id".to_owned(),
        related_table_name: "related".to_owned(),
        related_primary_column: "id".to_owned(),
        relation_name: RelationType::SimpleAttributes,
        mapping_table_name: "base_related".to_owned(),
    };
    let dbg = format!("{rel:?}");
    assert!(dbg.contains("base"));
    assert!(dbg.contains("related"));
    assert!(dbg.contains("SimpleAttributes"));
}

#[test]
fn test_mapping_row_debug_format() {
    let row = MappingRow {
        id: 7,
        base_id: 11,
        related_id: 22,
    };
    let dbg = format!("{row:?}");
    assert!(dbg.contains("base_id"));
    assert!(dbg.contains("related_id"));
}
