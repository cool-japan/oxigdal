//! Integration tests for PMTiles tile-set diffing.
//!
//! Tests exercise the public API in `oxigdal_pmtiles::diff`:
//! - [`diff_archives`]: full per-tile diff with ordered Vecs
//! - [`diff_archives_summary`]: compact count-only summary

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use oxigdal_pmtiles::{
    DiffSummary, PmTilesBuilder, TileChange, TileType, diff_archives, diff_archives_summary,
    zxy_to_tile_id,
};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Build a minimal valid PMTiles v3 archive from the given set of tiles.
///
/// Zoom range is derived from the tile list; falls back to `(0, 0)` for an
/// empty list so the archive header is still valid.
fn build_archive(tiles: &[(u8, u32, u32, &[u8])]) -> Vec<u8> {
    let min_z = tiles.iter().map(|t| t.0).min().unwrap_or(0);
    let max_z = tiles.iter().map(|t| t.0).max().unwrap_or(0);
    let mut builder = PmTilesBuilder::new(TileType::Png, min_z, max_z);
    for &(z, x, y, data) in tiles {
        builder.add_tile(z, x, y, data).expect("add_tile ok");
    }
    builder.build().expect("build ok")
}

// ---------------------------------------------------------------------------
// Test 1 — same archive vs. itself reports zero changes
// ---------------------------------------------------------------------------

#[test]
fn test_diff_identical_archives_no_changes() {
    let archive = build_archive(&[
        (0, 0, 0, b"z0"),
        (1, 0, 0, b"z1-0-0"),
        (1, 1, 0, b"z1-1-0"),
        (1, 0, 1, b"z1-0-1"),
        (1, 1, 1, b"z1-1-1"),
    ]);

    let report = diff_archives(&archive, &archive).expect("diff ok");

    assert!(report.added.is_empty(), "no tiles should be added");
    assert!(report.removed.is_empty(), "no tiles should be removed");
    assert!(report.changed.is_empty(), "no tiles should be changed");
    assert!(report.unchanged_count > 0, "unchanged_count should be > 0");
    assert_eq!(report.unchanged_count, 5);
    assert!(report.is_empty());
    assert_eq!(report.total_changes(), 0);
}

// ---------------------------------------------------------------------------
// Test 2 — two empty archives diff to all-zero report
// ---------------------------------------------------------------------------

#[test]
fn test_diff_empty_vs_empty_archives() {
    let empty = build_archive(&[]);
    let report = diff_archives(&empty, &empty).expect("diff ok");

    assert!(report.added.is_empty());
    assert!(report.removed.is_empty());
    assert!(report.changed.is_empty());
    assert_eq!(report.unchanged_count, 0);
    assert_eq!(report.total_changes(), 0);
    assert!(report.is_empty());
}

// ---------------------------------------------------------------------------
// Test 3 — adding a tile in the new archive is reported as Added
// ---------------------------------------------------------------------------

#[test]
fn test_diff_added_tile_in_new() {
    let old_bytes = build_archive(&[(0, 0, 0, b"only")]);
    let new_bytes = build_archive(&[(0, 0, 0, b"only"), (1, 0, 0, b"extra")]);

    let report = diff_archives(&old_bytes, &new_bytes).expect("diff ok");

    assert_eq!(report.added.len(), 1, "exactly one tile should be added");
    assert!(report.removed.is_empty());
    assert!(report.changed.is_empty());
    assert_eq!(report.unchanged_count, 1, "the z0 tile is unchanged");

    let expected_added_id = zxy_to_tile_id(1, 0, 0).expect("tile_id");
    let change = &report.added[0];
    assert!(
        matches!(change, TileChange::Added { .. }),
        "expected Added variant, got {change:?}"
    );
    if let TileChange::Added {
        tile_id,
        z,
        x,
        y,
        new_bytes,
    } = change
    {
        assert_eq!(*tile_id, expected_added_id);
        assert_eq!(*z, 1);
        assert_eq!(*x, 0);
        assert_eq!(*y, 0);
        assert_eq!(*new_bytes, b"extra".len());
    }
}

// ---------------------------------------------------------------------------
// Test 4 — removing a tile in the new archive is reported as Removed
// ---------------------------------------------------------------------------

#[test]
fn test_diff_removed_tile_in_old() {
    let old_bytes = build_archive(&[(0, 0, 0, b"only"), (1, 0, 0, b"gone")]);
    let new_bytes = build_archive(&[(0, 0, 0, b"only")]);

    let report = diff_archives(&old_bytes, &new_bytes).expect("diff ok");

    assert_eq!(
        report.removed.len(),
        1,
        "exactly one tile should be removed"
    );
    assert!(report.added.is_empty());
    assert!(report.changed.is_empty());
    assert_eq!(report.unchanged_count, 1);

    let expected_removed_id = zxy_to_tile_id(1, 0, 0).expect("tile_id");
    let change = &report.removed[0];
    assert!(
        matches!(change, TileChange::Removed { .. }),
        "expected Removed variant, got {change:?}"
    );
    if let TileChange::Removed {
        tile_id,
        z,
        x,
        y,
        old_bytes,
    } = change
    {
        assert_eq!(*tile_id, expected_removed_id);
        assert_eq!(*z, 1);
        assert_eq!(*x, 0);
        assert_eq!(*y, 0);
        assert_eq!(*old_bytes, b"gone".len());
    }
}

// ---------------------------------------------------------------------------
// Test 5 — same tile_id with different bytes is reported as Changed
// ---------------------------------------------------------------------------

#[test]
fn test_diff_changed_tile_content() {
    let old_bytes = build_archive(&[(0, 0, 0, b"version-one")]);
    let new_bytes = build_archive(&[(0, 0, 0, b"version-two-bigger")]);

    let report = diff_archives(&old_bytes, &new_bytes).expect("diff ok");

    assert_eq!(
        report.changed.len(),
        1,
        "exactly one tile should be changed"
    );
    assert!(report.added.is_empty());
    assert!(report.removed.is_empty());
    assert_eq!(report.unchanged_count, 0);

    let change = &report.changed[0];
    assert!(
        matches!(change, TileChange::Changed { .. }),
        "expected Changed variant, got {change:?}"
    );
    if let TileChange::Changed {
        tile_id,
        z,
        x,
        y,
        old_bytes: ob,
        new_bytes: nb,
    } = change
    {
        assert_eq!(*tile_id, 0);
        assert_eq!((*z, *x, *y), (0, 0, 0));
        assert_eq!(*ob, b"version-one".len());
        assert_eq!(*nb, b"version-two-bigger".len());
    }

    // changed_byte_delta = new - old
    let expected_delta = b"version-two-bigger".len() as i64 - b"version-one".len() as i64;
    assert_eq!(report.changed_byte_delta(), expected_delta);
}

// ---------------------------------------------------------------------------
// Test 6 — byte tallies for added/removed match the inserted payloads
// ---------------------------------------------------------------------------

#[test]
fn test_diff_summary_byte_tallies() {
    // 2 tiles added (10 + 20 = 30 bytes), 1 tile removed (5 bytes).
    let old_bytes = build_archive(&[(0, 0, 0, b"keep!"), (1, 0, 0, b"drop!")]);
    let new_bytes = build_archive(&[
        (0, 0, 0, b"keep!"),
        (1, 1, 0, &[0u8; 10]),
        (1, 1, 1, &[1u8; 20]),
    ]);

    let report = diff_archives(&old_bytes, &new_bytes).expect("diff ok");
    assert_eq!(report.added.len(), 2);
    assert_eq!(report.removed.len(), 1);

    assert_eq!(report.total_added_bytes(), 30);
    assert_eq!(report.total_removed_bytes(), 5);
}

// ---------------------------------------------------------------------------
// Test 7 — archives covering disjoint zoom levels report all tiles added
// ---------------------------------------------------------------------------

#[test]
fn test_diff_archives_at_different_zoom_levels() {
    // Old archive: 4 tiles at z=2.  New archive: 4 tiles at z=3 (disjoint IDs).
    let old_tiles: Vec<(u8, u32, u32, &[u8])> = vec![
        (2, 0, 0, b"a"),
        (2, 1, 0, b"b"),
        (2, 0, 1, b"c"),
        (2, 1, 1, b"d"),
    ];
    let new_tiles: Vec<(u8, u32, u32, &[u8])> = vec![
        (3, 0, 0, b"w"),
        (3, 1, 0, b"x"),
        (3, 0, 1, b"y"),
        (3, 1, 1, b"z"),
    ];

    let old_bytes = build_archive(&old_tiles);
    let new_bytes = build_archive(&new_tiles);

    let report = diff_archives(&old_bytes, &new_bytes).expect("diff ok");

    assert_eq!(report.added.len(), 4, "all 4 new-archive tiles are added");
    assert_eq!(
        report.removed.len(),
        4,
        "all 4 old-archive tiles are removed"
    );
    assert!(report.changed.is_empty());
    assert_eq!(report.unchanged_count, 0);

    // Every Added variant must come from zoom 3, every Removed from zoom 2.
    for change in &report.added {
        assert_eq!(change.zoom(), 3);
    }
    for change in &report.removed {
        assert_eq!(change.zoom(), 2);
    }
}

// ---------------------------------------------------------------------------
// Test 8 — total_changes() equals added + removed + changed
// ---------------------------------------------------------------------------

#[test]
fn test_diff_report_total_changes_counts() {
    // Old: 3 tiles.  New: keeps 1, changes 1, removes 1, adds 2 → 4 changes.
    let old_bytes = build_archive(&[
        (1, 0, 0, b"keep"),
        (1, 1, 0, b"to-change"),
        (1, 0, 1, b"to-remove"),
    ]);
    let new_bytes = build_archive(&[
        (1, 0, 0, b"keep"),
        (1, 1, 0, b"changed!"),
        (1, 1, 1, b"new-1"),
        (2, 0, 0, b"new-2"),
    ]);

    let report = diff_archives(&old_bytes, &new_bytes).expect("diff ok");

    assert_eq!(report.added.len(), 2);
    assert_eq!(report.removed.len(), 1);
    assert_eq!(report.changed.len(), 1);
    assert_eq!(report.unchanged_count, 1);

    let expected = report.added.len() + report.removed.len() + report.changed.len();
    assert_eq!(report.total_changes(), expected);
    assert_eq!(report.total_changes(), 4);
    assert!(!report.is_empty());
}

// ---------------------------------------------------------------------------
// Test 9 — unchanged_count counts only tiles present in both with same hash
// ---------------------------------------------------------------------------

#[test]
fn test_diff_unchanged_count_correctness() {
    // 5 tiles identical between old and new; 1 tile changes content.
    let old_tiles: Vec<(u8, u32, u32, Vec<u8>)> = vec![
        (1, 0, 0, vec![1u8; 16]),
        (1, 1, 0, vec![2u8; 16]),
        (1, 0, 1, vec![3u8; 16]),
        (1, 1, 1, vec![4u8; 16]),
        (2, 0, 0, vec![5u8; 16]),
        // changing tile
        (2, 1, 0, vec![6u8; 16]),
    ];
    let new_tiles: Vec<(u8, u32, u32, Vec<u8>)> = vec![
        (1, 0, 0, vec![1u8; 16]),
        (1, 1, 0, vec![2u8; 16]),
        (1, 0, 1, vec![3u8; 16]),
        (1, 1, 1, vec![4u8; 16]),
        (2, 0, 0, vec![5u8; 16]),
        // same id, different bytes
        (2, 1, 0, vec![99u8; 32]),
    ];

    let old_refs: Vec<(u8, u32, u32, &[u8])> = old_tiles
        .iter()
        .map(|(z, x, y, d)| (*z, *x, *y, d.as_slice()))
        .collect();
    let new_refs: Vec<(u8, u32, u32, &[u8])> = new_tiles
        .iter()
        .map(|(z, x, y, d)| (*z, *x, *y, d.as_slice()))
        .collect();

    let old_bytes = build_archive(&old_refs);
    let new_bytes = build_archive(&new_refs);

    let report = diff_archives(&old_bytes, &new_bytes).expect("diff ok");

    assert_eq!(report.unchanged_count, 5, "exactly 5 tiles are unchanged");
    assert_eq!(report.changed.len(), 1, "exactly 1 tile changes content");
    assert!(report.added.is_empty());
    assert!(report.removed.is_empty());
}

// ---------------------------------------------------------------------------
// Test 10 — output Vecs are sorted by ascending tile_id
// ---------------------------------------------------------------------------

#[test]
fn test_diff_archives_results_sorted_by_tile_id() {
    // Construct a workload that touches many tile IDs across multiple zoom
    // levels so that each Vec ends up with > 2 entries to sort.
    let old_bytes = build_archive(&[
        (0, 0, 0, b"old-z0"),
        (1, 0, 0, b"removed-A"),
        (1, 1, 1, b"removed-B"),
        (2, 0, 0, b"change-me-1"),
        (2, 1, 1, b"change-me-2"),
    ]);
    let new_bytes = build_archive(&[
        (0, 0, 0, b"old-z0"),
        // removed (1,0,0) and (1,1,1)
        (1, 0, 1, b"added-A"),
        (1, 1, 0, b"added-B"),
        // changed
        (2, 0, 0, b"change-me-1-NEW"),
        (2, 1, 1, b"change-me-2-NEW"),
    ]);

    let report = diff_archives(&old_bytes, &new_bytes).expect("diff ok");

    for window in report.added.windows(2) {
        assert!(
            window[0].tile_id() <= window[1].tile_id(),
            "added Vec must be sorted by tile_id"
        );
    }
    for window in report.removed.windows(2) {
        assert!(
            window[0].tile_id() <= window[1].tile_id(),
            "removed Vec must be sorted by tile_id"
        );
    }
    for window in report.changed.windows(2) {
        assert!(
            window[0].tile_id() <= window[1].tile_id(),
            "changed Vec must be sorted by tile_id"
        );
    }

    // Verify the diff was non-trivial so the sort assertions had work to do.
    assert!(report.added.len() >= 2);
    assert!(report.removed.len() >= 2);
    assert!(report.changed.len() >= 2);
}

// ---------------------------------------------------------------------------
// Test 11 — diff_archives_summary mirrors the full report's counts
// ---------------------------------------------------------------------------

#[test]
fn test_diff_archives_summary_matches_full_report() {
    let old_bytes = build_archive(&[
        (1, 0, 0, b"keep"),
        (1, 1, 0, b"change"),
        (1, 0, 1, b"removed"),
    ]);
    let new_bytes = build_archive(&[
        (1, 0, 0, b"keep"),
        (1, 1, 0, b"CHANGED!"),
        (1, 1, 1, b"new"),
    ]);

    let summary = diff_archives_summary(&old_bytes, &new_bytes).expect("summary ok");
    let report = diff_archives(&old_bytes, &new_bytes).expect("report ok");

    assert_eq!(
        summary,
        DiffSummary {
            added: report.added.len() as u64,
            removed: report.removed.len() as u64,
            changed: report.changed.len() as u64,
            unchanged: report.unchanged_count,
        }
    );

    assert_eq!(summary.total_changes(), report.total_changes() as u64);
    assert_eq!(
        summary.total_tiles(),
        summary.added + summary.removed + summary.changed + summary.unchanged
    );
}

// ---------------------------------------------------------------------------
// Test 12 — invalid input is reported as an error (no panic)
// ---------------------------------------------------------------------------

#[test]
fn test_diff_invalid_archive_returns_error() {
    let valid = build_archive(&[(0, 0, 0, b"ok")]);
    let garbage = vec![0u8; 32];

    assert!(diff_archives(&garbage, &valid).is_err());
    assert!(diff_archives(&valid, &garbage).is_err());
    assert!(diff_archives_summary(&garbage, &valid).is_err());
}
