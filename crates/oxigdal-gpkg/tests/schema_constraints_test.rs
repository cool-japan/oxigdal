//! Integration tests for the `gpkg_data_column_constraints` loader and
//! [`ConstraintValidator`] (OGC GeoPackage Encoding Standard §F.5).
//!
//! Tests cover three layers:
//!
//! 1. Loading rows from a synthetic GeoPackage byte stream — verifies the
//!    absent-table fast path returns `Ok(vec![])`.
//! 2. Validator behaviour for `range`, `enum`, and `glob` constraint kinds.
//! 3. Public API ergonomics — dispatch by name, diagnostic content of
//!    [`ConstraintViolation`].

use oxigdal_gpkg::btree::encode_sqlite_varint;
use oxigdal_gpkg::{
    CellValue, ConstraintType, ConstraintValidator, DataColumnConstraint, GeoPackage,
    load_data_column_constraints,
};

// ─────────────────────────────────────────────────────────────────────────────
// SQLite binary helpers (mirrors extensions_test.rs)
// ─────────────────────────────────────────────────────────────────────────────

fn make_sqlite_header(page_size_raw: u16, db_size_pages: u32) -> Vec<u8> {
    let mut data = vec![0u8; 100];
    data[..16].copy_from_slice(b"SQLite format 3\x00");
    data[16..18].copy_from_slice(&page_size_raw.to_be_bytes());
    data[28..32].copy_from_slice(&db_size_pages.to_be_bytes());
    data[56..60].copy_from_slice(&1u32.to_be_bytes()); // UTF-8 text encoding
    data[68..72].copy_from_slice(&0x4750_4B47u32.to_be_bytes()); // GPKG magic
    data
}

fn write_sqlite_file_header(file_data: &mut [u8], page_size: u16, db_size_pages: u32) {
    let hdr = make_sqlite_header(page_size, db_size_pages);
    file_data[..100].copy_from_slice(&hdr);
}

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
    page[hdr] = 13;
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

fn text_serial_type(len: usize) -> u64 {
    (len as u64) * 2 + 13
}

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
        (1u64, &rootpage_bytes), // serial type 1 = i8
        (text_serial_type(sql_bytes.len()), &sql_bytes),
    ];
    encode_record(&fields)
}

/// Build a 1-page SQLite GeoPackage file where page 1 is sqlite_master with no
/// entries — so the constraints table is unambiguously absent.
fn build_gpkg_without_constraints() -> Vec<u8> {
    let page_size = 4096usize;
    let total_pages = 1usize;
    let mut file = vec![0u8; page_size * total_pages];
    let master_page = build_leaf_table_page(page_size, &[], 100);
    file[..page_size].copy_from_slice(&master_page);
    write_sqlite_file_header(&mut file, page_size as u16, total_pages as u32);
    file
}

/// Build a 2-page SQLite file whose sqlite_master holds a single unrelated
/// table entry — used to confirm `load_data_column_constraints` returns
/// `Ok(vec![])` even when other tables exist.
fn build_gpkg_with_unrelated_table() -> Vec<u8> {
    let page_size = 4096usize;
    let total_pages = 2usize;
    let mut file = vec![0u8; page_size * total_pages];

    // Page 2: empty data leaf for the unrelated table.
    let data_page = build_leaf_table_page(page_size, &[], 0);
    file[page_size..page_size * 2].copy_from_slice(&data_page);

    // Page 1: master with one row pointing to the unrelated table on page 2.
    let master_record = build_master_record(
        "some_other_table",
        2,
        "CREATE TABLE some_other_table(id INTEGER)",
    );
    let master_page = build_leaf_table_page(page_size, &[(1, &master_record)], 100);
    file[..page_size].copy_from_slice(&master_page);
    write_sqlite_file_header(&mut file, page_size as u16, total_pages as u32);
    file
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_constraints_empty_when_table_absent() {
    let bytes = build_gpkg_without_constraints();
    let gpkg = GeoPackage::from_bytes(bytes).expect("valid gpkg bytes");
    let constraints = load_data_column_constraints(&gpkg)
        .expect("missing constraints table must return Ok(vec![])");
    assert!(
        constraints.is_empty(),
        "absent gpkg_data_column_constraints table must yield empty Vec, got {constraints:?}"
    );

    // Also verify the same fast-path holds when the master is non-empty but
    // contains no entry for `gpkg_data_column_constraints` specifically.
    let bytes_other = build_gpkg_with_unrelated_table();
    let gpkg_other = GeoPackage::from_bytes(bytes_other).expect("valid gpkg bytes");
    let constraints_other = load_data_column_constraints(&gpkg_other)
        .expect("unrelated tables present should still yield Ok(vec![])");
    assert!(
        constraints_other.is_empty(),
        "unrelated tables present must still yield empty constraints, got {constraints_other:?}"
    );
}

#[test]
fn test_constraints_range_inclusive_bounds_accepts_edge() {
    let validator = ConstraintValidator::new(vec![DataColumnConstraint {
        constraint_name: "percent".into(),
        constraint_type: ConstraintType::Range,
        value: None,
        min: Some(0.0),
        min_is_inclusive: Some(true),
        max: Some(100.0),
        max_is_inclusive: Some(true),
        description: Some("percentage 0..100 inclusive".into()),
    }]);

    assert!(
        validator
            .validate("percent", &CellValue::Integer(0))
            .is_ok(),
        "inclusive min should accept the lower edge (0)"
    );
    assert!(
        validator
            .validate("percent", &CellValue::Integer(100))
            .is_ok(),
        "inclusive max should accept the upper edge (100)"
    );
    assert!(
        validator
            .validate("percent", &CellValue::Float(50.5))
            .is_ok(),
        "midpoint value must pass"
    );
}

#[test]
fn test_constraints_range_exclusive_bounds_rejects_edge() {
    let validator = ConstraintValidator::new(vec![DataColumnConstraint {
        constraint_name: "positive".into(),
        constraint_type: ConstraintType::Range,
        value: None,
        min: Some(0.0),
        min_is_inclusive: Some(false),
        max: None,
        max_is_inclusive: None,
        description: None,
    }]);

    let err = validator
        .validate("positive", &CellValue::Integer(0))
        .expect_err("exclusive min must reject the edge value (0)");
    assert_eq!(err.constraint_name, "positive");
    assert_eq!(err.constraint_type, ConstraintType::Range);
    assert!(
        err.reason.to_lowercase().contains("below"),
        "violation reason should mention 'below', got: {}",
        err.reason
    );
    assert!(
        err.reason.contains("exclusive"),
        "violation reason should mention exclusivity, got: {}",
        err.reason
    );

    assert!(
        validator
            .validate("positive", &CellValue::Integer(1))
            .is_ok(),
        "value just above the exclusive min should pass"
    );
}

#[test]
fn test_constraints_enum_accepts_listed_value() {
    let validator = ConstraintValidator::new(vec![
        DataColumnConstraint {
            constraint_name: "category".into(),
            constraint_type: ConstraintType::Enum,
            value: Some("a".into()),
            min: None,
            min_is_inclusive: None,
            max: None,
            max_is_inclusive: None,
            description: None,
        },
        DataColumnConstraint {
            constraint_name: "category".into(),
            constraint_type: ConstraintType::Enum,
            value: Some("b".into()),
            min: None,
            min_is_inclusive: None,
            max: None,
            max_is_inclusive: None,
            description: None,
        },
        DataColumnConstraint {
            constraint_name: "category".into(),
            constraint_type: ConstraintType::Enum,
            value: Some("c".into()),
            min: None,
            min_is_inclusive: None,
            max: None,
            max_is_inclusive: None,
            description: None,
        },
    ]);

    assert!(
        validator
            .validate("category", &CellValue::Text("b".into()))
            .is_ok(),
        "value 'b' is listed; must pass"
    );
    assert!(
        validator
            .validate("category", &CellValue::Text("a".into()))
            .is_ok()
    );
    assert!(
        validator
            .validate("category", &CellValue::Text("c".into()))
            .is_ok()
    );
}

#[test]
fn test_constraints_enum_rejects_unlisted_value() {
    let validator = ConstraintValidator::new(vec![
        DataColumnConstraint {
            constraint_name: "category".into(),
            constraint_type: ConstraintType::Enum,
            value: Some("red".into()),
            min: None,
            min_is_inclusive: None,
            max: None,
            max_is_inclusive: None,
            description: None,
        },
        DataColumnConstraint {
            constraint_name: "category".into(),
            constraint_type: ConstraintType::Enum,
            value: Some("green".into()),
            min: None,
            min_is_inclusive: None,
            max: None,
            max_is_inclusive: None,
            description: None,
        },
    ]);

    let err = validator
        .validate("category", &CellValue::Text("blue".into()))
        .expect_err("'blue' is not in the enum and must be rejected");
    assert_eq!(err.constraint_name, "category");
    assert_eq!(err.constraint_type, ConstraintType::Enum);
    assert_eq!(err.actual_value, "blue");
    assert!(
        err.reason.contains("red") && err.reason.contains("green"),
        "diagnostic should list the allowed values, got: {}",
        err.reason
    );
}

#[test]
fn test_constraints_glob_star_wildcard() {
    let validator = ConstraintValidator::new(vec![DataColumnConstraint {
        constraint_name: "prefix".into(),
        constraint_type: ConstraintType::Glob,
        value: Some("foo*".into()),
        min: None,
        min_is_inclusive: None,
        max: None,
        max_is_inclusive: None,
        description: None,
    }]);

    assert!(
        validator
            .validate("prefix", &CellValue::Text("foobar".into()))
            .is_ok(),
        "'foobar' must match the glob 'foo*'"
    );
    assert!(
        validator
            .validate("prefix", &CellValue::Text("foo".into()))
            .is_ok(),
        "'foo' (zero suffix) must match the glob 'foo*'"
    );

    let err = validator
        .validate("prefix", &CellValue::Text("barfoo".into()))
        .expect_err("'barfoo' does not start with 'foo' and must fail");
    assert_eq!(err.constraint_type, ConstraintType::Glob);
    assert!(err.reason.contains("foo*"));
}

#[test]
fn test_constraints_glob_question_wildcard() {
    let validator = ConstraintValidator::new(vec![DataColumnConstraint {
        constraint_name: "triple".into(),
        constraint_type: ConstraintType::Glob,
        value: Some("a?c".into()),
        min: None,
        min_is_inclusive: None,
        max: None,
        max_is_inclusive: None,
        description: None,
    }]);

    assert!(
        validator
            .validate("triple", &CellValue::Text("abc".into()))
            .is_ok(),
        "'abc' is a 3-char match for 'a?c'"
    );

    let err_short = validator
        .validate("triple", &CellValue::Text("ac".into()))
        .expect_err("'ac' is 2 chars and cannot match 'a?c'");
    assert_eq!(err_short.constraint_type, ConstraintType::Glob);

    let err_long = validator
        .validate("triple", &CellValue::Text("abbc".into()))
        .expect_err("'abbc' is 4 chars and cannot match 'a?c'");
    assert_eq!(err_long.constraint_type, ConstraintType::Glob);
}

#[test]
fn test_constraints_validator_dispatches_by_name() {
    let validator = ConstraintValidator::new(vec![
        DataColumnConstraint {
            constraint_name: "score".into(),
            constraint_type: ConstraintType::Range,
            value: None,
            min: Some(0.0),
            min_is_inclusive: Some(true),
            max: Some(10.0),
            max_is_inclusive: Some(true),
            description: None,
        },
        DataColumnConstraint {
            constraint_name: "color".into(),
            constraint_type: ConstraintType::Enum,
            value: Some("red".into()),
            min: None,
            min_is_inclusive: None,
            max: None,
            max_is_inclusive: None,
            description: None,
        },
        DataColumnConstraint {
            constraint_name: "tag".into(),
            constraint_type: ConstraintType::Glob,
            value: Some("tag_*".into()),
            min: None,
            min_is_inclusive: None,
            max: None,
            max_is_inclusive: None,
            description: None,
        },
    ]);

    // Each name routes to the right validator kind.
    assert!(validator.validate("score", &CellValue::Integer(5)).is_ok());
    assert!(
        validator
            .validate("color", &CellValue::Text("red".into()))
            .is_ok()
    );
    assert!(
        validator
            .validate("tag", &CellValue::Text("tag_42".into()))
            .is_ok()
    );

    // And each name correctly reports the kind it dispatches to on failure.
    let score_err = validator
        .validate("score", &CellValue::Integer(11))
        .expect_err("11 is out of range for 'score'");
    assert_eq!(score_err.constraint_type, ConstraintType::Range);

    let color_err = validator
        .validate("color", &CellValue::Text("blue".into()))
        .expect_err("'blue' is not in enum 'color'");
    assert_eq!(color_err.constraint_type, ConstraintType::Enum);

    let tag_err = validator
        .validate("tag", &CellValue::Text("xyz".into()))
        .expect_err("'xyz' does not match glob 'tag_*'");
    assert_eq!(tag_err.constraint_type, ConstraintType::Glob);

    assert_eq!(validator.len(), 3, "three distinct constraint names");
    assert!(validator.contains("score"));
    assert!(validator.contains("color"));
    assert!(validator.contains("tag"));
    assert!(!validator.contains("missing"));
}

#[test]
fn test_constraints_violation_carries_reason() {
    let validator = ConstraintValidator::new(vec![DataColumnConstraint {
        constraint_name: "bounded".into(),
        constraint_type: ConstraintType::Range,
        value: None,
        min: Some(1.0),
        min_is_inclusive: Some(true),
        max: Some(5.0),
        max_is_inclusive: Some(true),
        description: None,
    }]);

    let err = validator
        .validate("bounded", &CellValue::Integer(99))
        .expect_err("99 is above the range and must fail");

    assert!(
        !err.reason.is_empty(),
        "violation must carry a non-empty reason string"
    );
    assert!(
        !err.actual_value.is_empty(),
        "violation must record the offending value"
    );
    assert_eq!(err.constraint_name, "bounded");
    assert_eq!(err.constraint_type, ConstraintType::Range);
    assert!(
        err.reason.contains("above") && err.reason.contains("max"),
        "diagnostic should mention 'above' and 'max', got: {}",
        err.reason
    );
}
