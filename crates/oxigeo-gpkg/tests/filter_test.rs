//! Integration tests for the attribute WHERE filter with SQL-like predicate pushdown.
//!
//! Tests cover:
//! * Individual comparison operators (Eq, Ne, Lt, Lte, Gt, Gte)
//! * LIKE / NOT LIKE with `%` and `_` wildcards
//! * IS NULL / IS NOT NULL
//! * Boolean combinators: AND, OR, NOT
//! * In-process GeoPackage scan with filter and filtered pagination
//! * Out-of-range column index behaviour

#![allow(clippy::expect_used, clippy::panic)]

use oxigeo_gpkg::{CellValue, FilterExpr, GeoPackage, GeoPackageBuilder, evaluate_filter};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Build a small in-memory GeoPackage with a "cities" feature table that has
/// three point features.  Used for scan-level integration tests.
fn build_cities_gpkg() -> Vec<u8> {
    GeoPackageBuilder::new(4326)
        .add_feature_table(
            "cities",
            "POINT",
            vec![
                (1, 139.7, 35.7), // Tokyo
                (2, -74.0, 40.7), // New York
                (3, 2.35, 48.85), // Paris
            ],
        )
        .build()
        .expect("build cities gpkg")
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — col_eq: integer match and non-match
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_filter_eq_integer_matches_and_rejects() {
    let row = vec![CellValue::Integer(42), CellValue::Text("hello".into())];

    let matching = FilterExpr::col_eq(0, CellValue::Integer(42));
    assert!(evaluate_filter(&matching, &row), "should match Integer(42)");

    let rejecting = FilterExpr::col_eq(0, CellValue::Integer(99));
    assert!(
        !evaluate_filter(&rejecting, &row),
        "should reject Integer(99)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — col_ne: text non-equality filter
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_filter_ne_text_excludes_matching_value() {
    let row = vec![CellValue::Text("alice".into())];

    let ne_alice = FilterExpr::col_ne(0, CellValue::Text("alice".into()));
    assert!(
        !evaluate_filter(&ne_alice, &row),
        "alice <> alice should be false"
    );

    let ne_bob = FilterExpr::col_ne(0, CellValue::Text("bob".into()));
    assert!(
        evaluate_filter(&ne_bob, &row),
        "alice <> bob should be true"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — col_gte AND col_lt: numeric range
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_filter_lt_gte_numeric_range() {
    // Expression: col[0] >= 10 AND col[0] < 20
    let expr = FilterExpr::and(
        FilterExpr::col_gte(0, CellValue::Integer(10)),
        FilterExpr::col_lt(0, CellValue::Integer(20)),
    );

    // Value in range [10, 20)
    let row_in = vec![CellValue::Integer(15)];
    assert!(evaluate_filter(&expr, &row_in), "15 is in [10, 20)");

    // Value below range
    let row_low = vec![CellValue::Integer(5)];
    assert!(!evaluate_filter(&expr, &row_low), "5 is below [10, 20)");

    // Value exactly at boundary (20 is excluded)
    let row_boundary = vec![CellValue::Integer(20)];
    assert!(
        !evaluate_filter(&expr, &row_boundary),
        "20 is excluded by < 20"
    );

    // Value exactly at lower boundary
    let row_lo_edge = vec![CellValue::Integer(10)];
    assert!(
        evaluate_filter(&expr, &row_lo_edge),
        "10 is included by >= 10"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — col_like: % wildcard matches any suffix
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_filter_like_percent_matches_any_suffix() {
    let row = vec![CellValue::Text("hello world".into())];

    let prefix_match = FilterExpr::col_like(0, "hello%");
    assert!(
        evaluate_filter(&prefix_match, &row),
        "hello% should match 'hello world'"
    );

    let suffix_match = FilterExpr::col_like(0, "%world");
    assert!(
        evaluate_filter(&suffix_match, &row),
        "%world should match 'hello world'"
    );

    let no_match = FilterExpr::col_like(0, "goodbye%");
    assert!(
        !evaluate_filter(&no_match, &row),
        "goodbye% should not match 'hello world'"
    );

    // % alone matches any string
    let any_match = FilterExpr::col_like(0, "%");
    assert!(
        evaluate_filter(&any_match, &row),
        "% should match any non-empty string"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — col_like: _ wildcard matches exactly one character
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_filter_like_underscore_matches_one_char() {
    let row = vec![CellValue::Text("abc".into())];

    let one_char = FilterExpr::col_like(0, "a_c");
    assert!(evaluate_filter(&one_char, &row), "a_c should match 'abc'");

    // Two characters between a and c — should NOT match
    let two_chars = FilterExpr::col_like(0, "a__c");
    assert!(
        !evaluate_filter(&two_chars, &row),
        "a__c should not match 'abc'"
    );

    // Short string — _ requires at least one char
    let short = vec![CellValue::Text("ac".into())];
    assert!(
        !evaluate_filter(&one_char, &short),
        "a_c should not match 'ac'"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — col_is_null and col_is_not_null
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_filter_is_null_matches_null_only() {
    let row_null = vec![CellValue::Null];
    let row_int = vec![CellValue::Integer(0)];
    let row_empty_text = vec![CellValue::Text(String::new())];

    let is_null = FilterExpr::col_is_null(0);
    assert!(
        evaluate_filter(&is_null, &row_null),
        "IS NULL should match CellValue::Null"
    );
    assert!(
        !evaluate_filter(&is_null, &row_int),
        "IS NULL should not match Integer(0)"
    );
    assert!(
        !evaluate_filter(&is_null, &row_empty_text),
        "IS NULL should not match empty text"
    );

    let is_not_null = FilterExpr::col_is_not_null(0);
    assert!(
        !evaluate_filter(&is_not_null, &row_null),
        "IS NOT NULL should reject CellValue::Null"
    );
    assert!(
        evaluate_filter(&is_not_null, &row_int),
        "IS NOT NULL should match Integer(0)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — FilterExpr::and combines two predicates
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_filter_and_combines_two_predicates() {
    let row = vec![CellValue::Integer(5), CellValue::Text("foo".into())];

    // Both predicates true → AND is true
    let both_true = FilterExpr::and(
        FilterExpr::col_eq(0, CellValue::Integer(5)),
        FilterExpr::col_eq(1, CellValue::Text("foo".into())),
    );
    assert!(
        evaluate_filter(&both_true, &row),
        "5='foo': AND should be true"
    );

    // First predicate false → AND is false (short-circuit)
    let first_false = FilterExpr::and(
        FilterExpr::col_eq(0, CellValue::Integer(99)),
        FilterExpr::col_eq(1, CellValue::Text("foo".into())),
    );
    assert!(
        !evaluate_filter(&first_false, &row),
        "99='foo': AND should be false"
    );

    // Second predicate false → AND is false
    let second_false = FilterExpr::and(
        FilterExpr::col_eq(0, CellValue::Integer(5)),
        FilterExpr::col_eq(1, CellValue::Text("bar".into())),
    );
    assert!(
        !evaluate_filter(&second_false, &row),
        "5='bar': AND should be false"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8 — FilterExpr::or combines two predicates
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_filter_or_combines_two_predicates() {
    let row = vec![CellValue::Integer(5)];

    // First branch matches → OR is true
    let first_true = FilterExpr::or(
        FilterExpr::col_eq(0, CellValue::Integer(5)),
        FilterExpr::col_eq(0, CellValue::Integer(99)),
    );
    assert!(
        evaluate_filter(&first_true, &row),
        "5 OR 99: first branch matches → true"
    );

    // Second branch matches → OR is true
    let second_true = FilterExpr::or(
        FilterExpr::col_eq(0, CellValue::Integer(99)),
        FilterExpr::col_eq(0, CellValue::Integer(5)),
    );
    assert!(
        evaluate_filter(&second_true, &row),
        "99 OR 5: second branch matches → true"
    );

    // Neither branch matches → OR is false
    let none_true = FilterExpr::or(
        FilterExpr::col_eq(0, CellValue::Integer(1)),
        FilterExpr::col_eq(0, CellValue::Integer(2)),
    );
    assert!(
        !evaluate_filter(&none_true, &row),
        "1 OR 2: neither matches 5 → false"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9 — FilterExpr::not inverts the result
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_filter_not_inverts_result() {
    let row = vec![CellValue::Integer(5)];

    // NOT (col = 99) → true, because col is 5
    let not_99 = FilterExpr::not(FilterExpr::col_eq(0, CellValue::Integer(99)));
    assert!(evaluate_filter(&not_99, &row), "NOT(5=99) should be true");

    // NOT (col = 5) → false, because col IS 5
    let not_5 = FilterExpr::not(FilterExpr::col_eq(0, CellValue::Integer(5)));
    assert!(!evaluate_filter(&not_5, &row), "NOT(5=5) should be false");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 10 — out-of-range column index treated as NULL
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_filter_out_of_range_column_treated_as_null() {
    let row = vec![CellValue::Integer(1)]; // only 1 column

    // Column index 5 is out of range → IS NULL should return true
    let is_null_oob = FilterExpr::col_is_null(5);
    assert!(
        evaluate_filter(&is_null_oob, &row),
        "out-of-range column should be treated as NULL"
    );

    // col[5] = 42 should return false (NULL comparison)
    let eq_oob = FilterExpr::col_eq(5, CellValue::Integer(42));
    assert!(
        !evaluate_filter(&eq_oob, &row),
        "comparison with missing column should be false"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 11 — scan_table_filtered via GeoPackage wrapper
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_scan_table_filtered_returns_matching_rows() {
    use oxigeo_gpkg::filter::FilterExpr as FE;

    let bytes = build_cities_gpkg();
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse gpkg");

    // The "cities" feature table has columns: fid INTEGER, geom BLOB.
    // Columns in the raw B-tree record: [geom BLOB, fid INTEGER] order depends
    // on how the writer encodes them.  We do a full scan first to discover
    // what columns look like, then apply a filter that must return results.

    // Full scan to understand the row structure
    let all_rows = gpkg
        .scan_table_by_name("cities")
        .expect("scan_table_by_name")
        .expect("cities table missing");

    assert!(!all_rows.is_empty(), "cities table should have rows");

    // Apply a trivially-true IS NOT NULL filter on column 0 — should return all rows.
    let expr = FE::col_is_not_null(0);
    let filtered = gpkg
        .scan_table_filtered("cities", &expr)
        .expect("scan_table_filtered")
        .expect("cities table missing");

    assert_eq!(
        filtered.len(),
        all_rows.len(),
        "IS NOT NULL on col[0] should return all rows"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 12 — scan_table_filtered_paginated: post-filter offset and limit
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_scan_table_filtered_paginated_post_filter_offset_limit() {
    use oxigeo_gpkg::filter::FilterExpr as FE;

    let bytes = build_cities_gpkg();
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse gpkg");

    // IS NOT NULL on col[0] is always true, so all 3 rows match.
    // With offset=1 and limit=1 we should get exactly 1 row.
    let expr = FE::col_is_not_null(0);

    let page = gpkg
        .scan_table_filtered_paginated("cities", &expr, 1, 1)
        .expect("scan_table_filtered_paginated")
        .expect("cities table missing");

    assert_eq!(
        page.len(),
        1,
        "offset=1 limit=1 should return exactly 1 row"
    );

    // limit=0 should return empty regardless of filter
    let empty = gpkg
        .scan_table_filtered_paginated("cities", &expr, 0, 0)
        .expect("scan_table_filtered_paginated limit=0")
        .expect("cities table missing");
    assert!(empty.is_empty(), "limit=0 should return empty");

    // Offset beyond all rows should return empty
    let beyond = gpkg
        .scan_table_filtered_paginated("cities", &expr, 999, 10)
        .expect("scan_table_filtered_paginated beyond")
        .expect("cities table missing");
    assert!(
        beyond.is_empty(),
        "offset beyond row count should return empty"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 13 — NULL = NULL is FALSE (SQL three-valued logic)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_filter_null_eq_null_is_false() {
    let row = vec![CellValue::Null];
    // NULL = NULL should be false in SQL semantics
    let expr = FilterExpr::col_eq(0, CellValue::Null);
    assert!(
        !evaluate_filter(&expr, &row),
        "NULL = NULL must be false (SQL semantics)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 14 — cross-type numeric comparison (Integer vs Float)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_filter_cross_type_numeric_comparison() {
    let row = vec![CellValue::Float(3.5)];

    // Integer(3) < Float(3.5) → true via f64 promotion
    let lt = FilterExpr::col_gt(0, CellValue::Integer(3));
    assert!(
        evaluate_filter(&lt, &row),
        "Float(3.5) > Integer(3) should be true via f64 promotion"
    );

    // Integer(4) > Float(3.5) → col is 3.5, Integer(4) > 3.5 → so col < 4
    let col_lt_4 = FilterExpr::col_lt(0, CellValue::Integer(4));
    assert!(
        evaluate_filter(&col_lt_4, &row),
        "Float(3.5) < Integer(4) should be true"
    );
}
