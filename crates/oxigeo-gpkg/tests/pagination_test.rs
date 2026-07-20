//! Integration tests for cursor-based B-tree pagination (feature-table pagination).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use oxigeo_gpkg::{
    CellValue, GeoPackage, GeoPackageBuilder, count_table_rows, scan_table, scan_table_paginated,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Build a GeoPackage with a single feature table called `"pts"` containing
/// `n` point features at coordinates `(i as f64, 0.0)` for i in 1..=n.
fn build_gpkg_with_n_points(n: usize) -> Vec<u8> {
    let points: Vec<(i64, f64, f64)> = (1..=n).map(|i| (i as i64, i as f64, 0.0_f64)).collect();
    GeoPackageBuilder::new(4326)
        .add_feature_table("pts", "POINT", points)
        .build()
        .expect("build gpkg")
}

/// Return `(root_page, page_size)` for the table `table_name` in the given
/// raw GPKG bytes by scanning `sqlite_master`.
fn find_root_page(data: &[u8], table_name: &str) -> (u32, usize) {
    let gpkg = GeoPackage::from_bytes(data.to_vec()).expect("parse gpkg");
    let page_size = gpkg.page_size() as usize;
    let master = gpkg.scan_sqlite_master().expect("scan sqlite_master");
    for entry in &master {
        if entry.entry_type == "table" && entry.name == table_name {
            return (entry.rootpage, page_size);
        }
    }
    panic!("Table '{table_name}' not found in sqlite_master");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — offset=0, limit larger than table → all rows returned
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_paginate_offset_zero_limit_all() {
    let data = build_gpkg_with_n_points(5);
    let (root, page_size) = find_root_page(&data, "pts");

    let page = scan_table_paginated(&data, root, page_size, 0, 100).expect("paginate");
    assert_eq!(page.len(), 5, "expected all 5 rows, got {}", page.len());
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — offset=0, limit=3 → first 3 rows only
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_paginate_offset_zero_limit_exact() {
    let data = build_gpkg_with_n_points(5);
    let (root, page_size) = find_root_page(&data, "pts");

    let page = scan_table_paginated(&data, root, page_size, 0, 3).expect("paginate");
    assert_eq!(page.len(), 3, "expected 3 rows, got {}", page.len());

    // The first 3 rows must be a prefix of the full scan.
    let full = scan_table(&data, root, page_size).expect("full scan");
    for i in 0..3 {
        assert_eq!(
            page[i].0, full[i].0,
            "rowid mismatch at index {i}: page={}, full={}",
            page[i].0, full[i].0
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — offset beyond end of table → empty result
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_paginate_offset_beyond_end() {
    let data = build_gpkg_with_n_points(5);
    let (root, page_size) = find_root_page(&data, "pts");

    let page = scan_table_paginated(&data, root, page_size, 10, 5).expect("paginate");
    assert!(
        page.is_empty(),
        "expected empty vec for offset beyond end, got {} rows",
        page.len()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — offset=2, limit=3 → rows at index 2, 3, 4
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_paginate_offset_partial() {
    let data = build_gpkg_with_n_points(5);
    let (root, page_size) = find_root_page(&data, "pts");

    let full = scan_table(&data, root, page_size).expect("full scan");
    let page = scan_table_paginated(&data, root, page_size, 2, 3).expect("paginate");

    assert_eq!(page.len(), 3, "expected 3 rows, got {}", page.len());
    for i in 0..3 {
        assert_eq!(
            page[i].0,
            full[i + 2].0,
            "rowid mismatch at slot {i}: page={}, full[{}]={}",
            page[i].0,
            i + 2,
            full[i + 2].0
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — limit=0 → always empty, no error
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_paginate_limit_zero() {
    let data = build_gpkg_with_n_points(5);
    let (root, page_size) = find_root_page(&data, "pts");

    let page = scan_table_paginated(&data, root, page_size, 0, 0).expect("paginate");
    assert!(page.is_empty(), "limit=0 must return empty vec");

    // Also test with non-zero offset.
    let page2 = scan_table_paginated(&data, root, page_size, 3, 0).expect("paginate");
    assert!(
        page2.is_empty(),
        "limit=0 must return empty vec (with offset)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — 50-row table, page through 10 at a time → union covers all rows
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_paginate_full_pages() {
    let data = build_gpkg_with_n_points(50);
    let (root, page_size) = find_root_page(&data, "pts");

    let full = scan_table(&data, root, page_size).expect("full scan");
    assert_eq!(full.len(), 50, "expected 50 rows in full scan");

    let mut all_paged: Vec<(i64, Vec<CellValue>)> = Vec::new();
    let page_sz = 10_usize;
    let mut off = 0_usize;
    loop {
        let page = scan_table_paginated(&data, root, page_size, off, page_sz).expect("paginate");
        if page.is_empty() {
            break;
        }
        all_paged.extend(page);
        off += page_sz;
    }

    assert_eq!(
        all_paged.len(),
        50,
        "paging through 10-at-a-time should cover all 50 rows"
    );

    // Verify the rowids match the full scan exactly.
    for (idx, (paged_rowid, full_rowid)) in all_paged
        .iter()
        .map(|(r, _)| r)
        .zip(full.iter().map(|(r, _)| r))
        .enumerate()
    {
        assert_eq!(
            paged_rowid, full_rowid,
            "rowid mismatch at position {idx}: paged={paged_rowid}, full={full_rowid}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — last partial page: offset=45, limit=10 on 50-row table → 5 rows
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_paginate_last_page() {
    let data = build_gpkg_with_n_points(50);
    let (root, page_size) = find_root_page(&data, "pts");

    let page = scan_table_paginated(&data, root, page_size, 45, 10).expect("paginate");
    assert_eq!(
        page.len(),
        5,
        "last page should have 5 rows (50-45=5), got {}",
        page.len()
    );

    // Verify those 5 rows are the tail of the full scan.
    let full = scan_table(&data, root, page_size).expect("full scan");
    for i in 0..5 {
        assert_eq!(
            page[i].0,
            full[45 + i].0,
            "rowid mismatch at last-page slot {i}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8 — count_table_rows matches scan_table().len()
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_count_table_rows_matches_scan() {
    let data = build_gpkg_with_n_points(17);
    let (root, page_size) = find_root_page(&data, "pts");

    let full = scan_table(&data, root, page_size).expect("full scan");
    let count = count_table_rows(&data, root, page_size).expect("count");

    assert_eq!(
        count,
        full.len() as u64,
        "count_table_rows({count}) != scan_table().len({})",
        full.len()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9 — count on an empty (zero-row) table → 0
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_count_table_rows_empty_table() {
    // Build a GeoPackage with no feature table; use gpkg_contents as a zero-
    // row-ish table proxy — but actually the system tables have rows.
    // Instead build a table with 0 points explicitly.
    let data = GeoPackageBuilder::new(4326)
        .add_feature_table("empty_pts", "POINT", Vec::<(i64, f64, f64)>::new())
        .build()
        .expect("build");

    let (root, page_size) = find_root_page(&data, "empty_pts");

    let count = count_table_rows(&data, root, page_size).expect("count");
    assert_eq!(count, 0, "expected 0 rows for empty table, got {count}");

    // Also verify via paginate.
    let page = scan_table_paginated(&data, root, page_size, 0, 100).expect("paginate");
    assert!(
        page.is_empty(),
        "paginate on empty table must return empty vec"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 10 — rowids are non-decreasing across paginated results
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_paginate_rows_in_ascending_rowid_order() {
    let data = build_gpkg_with_n_points(50);
    let (root, page_size) = find_root_page(&data, "pts");

    let page = scan_table_paginated(&data, root, page_size, 0, 50).expect("paginate");
    assert!(!page.is_empty(), "expected rows");

    let mut prev = i64::MIN;
    for (rowid, _) in &page {
        assert!(
            *rowid >= prev,
            "rowids must be non-decreasing: prev={prev}, current={rowid}"
        );
        prev = *rowid;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 11 — GeoPackage::scan_table_paginated matches btree::scan_table_paginated
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_paginate_gpkg_method_matches_btree_direct() {
    let data = build_gpkg_with_n_points(20);
    let (root, page_size) = find_root_page(&data, "pts");

    // Direct btree call.
    let direct = scan_table_paginated(&data, root, page_size, 0, 5).expect("direct");

    // GeoPackage method call.
    let gpkg = GeoPackage::from_bytes(data).expect("parse");
    let via_gpkg = gpkg
        .scan_table_paginated("pts", 0, 5)
        .expect("gpkg method")
        .expect("table found");

    assert_eq!(
        direct.len(),
        via_gpkg.len(),
        "row count mismatch: direct={}, gpkg={}",
        direct.len(),
        via_gpkg.len()
    );

    for (idx, ((r_d, _), (r_g, _))) in direct.iter().zip(via_gpkg.iter()).enumerate() {
        assert_eq!(
            r_d, r_g,
            "rowid mismatch at index {idx}: direct={r_d}, gpkg={r_g}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 12 — count_table_rows via GeoPackage method
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_count_table_rows_gpkg_method() {
    let data = build_gpkg_with_n_points(33);
    let gpkg = GeoPackage::from_bytes(data).expect("parse");

    let count = gpkg
        .count_table_rows("pts")
        .expect("count method")
        .expect("table found");
    assert_eq!(
        count, 33,
        "expected 33 rows via GeoPackage::count_table_rows"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 13 — scan_table_paginated matches scan_table for every (offset, limit)
//            combination on a small table (exhaustive correctness check)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_paginate_exhaustive_correctness() {
    let data = build_gpkg_with_n_points(8);
    let (root, page_size) = find_root_page(&data, "pts");

    let full = scan_table(&data, root, page_size).expect("full scan");
    let n = full.len();

    for offset in 0..=(n + 2) {
        for limit in 0..=(n + 2) {
            let page =
                scan_table_paginated(&data, root, page_size, offset, limit).expect("paginate");

            // Expected: full[offset..offset+limit] clamped to table size.
            let start = offset.min(n);
            let end = (offset + limit).min(n);
            let expected = &full[start..end];

            assert_eq!(
                page.len(),
                expected.len(),
                "length mismatch for offset={offset} limit={limit}: \
                 got {}, expected {}",
                page.len(),
                expected.len()
            );

            for (slot, ((r_page, _), (r_exp, _))) in page.iter().zip(expected.iter()).enumerate() {
                assert_eq!(
                    r_page, r_exp,
                    "rowid mismatch at slot {slot} for offset={offset} limit={limit}: \
                     page={r_page}, expected={r_exp}"
                );
            }
        }
    }
}
