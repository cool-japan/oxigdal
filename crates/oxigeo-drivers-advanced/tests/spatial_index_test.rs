//! Integration tests for GeoPackage R-tree spatial index building.
//!
//! Verifies that `RTreeIndex::build` correctly populates the in-memory R-tree
//! from real WKB geometry blobs stored in a SQLite database.

#![cfg(feature = "geopackage")]

use oxigeo_drivers_advanced::gpkg::{ConnectionMode, GpkgConnection, RTreeIndex, SpatialIndex};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ── Geometry blob helpers ────────────────────────────────────────────────────

/// Build a minimal GPKG point blob with XY envelope (envelope flag = 1).
///
/// Layout (OGC GeoPackage §2.1.3):
///   'G','P', version(0x00), flags(0x03: LE + envelope=1)
///   srs_id(u32 LE)
///   minx, maxx, miny, maxy  (f64 LE — all equal for a point)
///   WKB Point body: byte_order(1=LE) + type(u32=1) + x(f64) + y(f64)
fn make_gpkg_point_blob_with_envelope(x: f64, y: f64) -> Vec<u8> {
    // flags: bit0=1 (LE), bits1-3 = 001 (envelope flag = 1) → 0x03
    let mut blob = vec![b'G', b'P', 0x00u8, 0x03u8];
    blob.extend_from_slice(&4326u32.to_le_bytes()); // srs_id
    // XY envelope: minx, maxx, miny, maxy (point → min == max)
    blob.extend_from_slice(&x.to_le_bytes());
    blob.extend_from_slice(&x.to_le_bytes());
    blob.extend_from_slice(&y.to_le_bytes());
    blob.extend_from_slice(&y.to_le_bytes());
    // WKB Point
    blob.extend_from_slice(&[1u8]); // LE byte order
    blob.extend_from_slice(&1u32.to_le_bytes()); // type = Point
    blob.extend_from_slice(&x.to_le_bytes());
    blob.extend_from_slice(&y.to_le_bytes());
    blob
}

/// Build a GPKG point blob with *no* envelope (flag = 0); triggers WKB scan.
fn make_gpkg_point_blob_no_envelope(x: f64, y: f64) -> Vec<u8> {
    // flags: bit0=1 (LE), bits1-3 = 000 (no envelope) → 0x01
    let mut blob = vec![b'G', b'P', 0x00u8, 0x01u8];
    blob.extend_from_slice(&4326u32.to_le_bytes()); // srs_id
    // WKB Point directly follows the header
    blob.extend_from_slice(&[1u8]); // LE
    blob.extend_from_slice(&1u32.to_le_bytes()); // type = Point
    blob.extend_from_slice(&x.to_le_bytes());
    blob.extend_from_slice(&y.to_le_bytes());
    blob
}

// ── Helper: create and populate a temp SQLite feature table ─────────────────

/// Open (or create) a file-based SQLite database at `path`, create a minimal
/// `features (fid INTEGER PRIMARY KEY, geom BLOB)` table, and insert `rows`.
fn populate_test_db(
    path: &std::path::Path,
    rows: &[(i64, Vec<u8>)],
) -> Result<(), Box<dyn std::error::Error>> {
    use oxisql_core::ToSqlValue;
    use oxisql_sqlite_compat::blocking::SqliteConnectionBlocking;

    let path_str = path.to_str().ok_or("path contains non-UTF-8 characters")?;
    let conn = SqliteConnectionBlocking::open(path_str)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS features (fid INTEGER PRIMARY KEY, geom BLOB);",
    )?;
    for (fid, blob) in rows {
        conn.execute(
            "INSERT INTO features (fid, geom) VALUES ($1, $2)",
            &[fid as &dyn ToSqlValue, blob as &dyn ToSqlValue],
        )?;
    }
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// Verifies that the GPKG blob helpers produce non-empty byte slices.
#[test]
fn test_blob_helpers_produce_non_empty_output() {
    let blob_env = make_gpkg_point_blob_with_envelope(10.0, 20.0);
    assert!(!blob_env.is_empty());

    let blob_no_env = make_gpkg_point_blob_no_envelope(10.0, 20.0);
    assert!(!blob_no_env.is_empty());
}

/// Main integration test: `build` must populate the R-tree from real WKB blobs.
#[test]
fn test_rtree_build_populates_from_db() -> TestResult {
    let tmp = std::env::temp_dir().join("test_gpkg_rtree_build.gpkg");
    let _ = std::fs::remove_file(&tmp);

    let pts: &[(i64, f64, f64)] = &[(1, 10.0, 20.0), (2, 50.0, 60.0), (3, -10.0, -20.0)];
    let rows: Vec<(i64, Vec<u8>)> = pts
        .iter()
        .map(|(fid, x, y)| (*fid, make_gpkg_point_blob_with_envelope(*x, *y)))
        .collect();
    populate_test_db(&tmp, &rows)?;

    let gpkg_conn = GpkgConnection::open(&tmp, ConnectionMode::ReadOnly)?;
    let mut index = RTreeIndex::new();
    index.build(&gpkg_conn, "features")?;

    assert_eq!(index.len(), 3, "all 3 features must be indexed");

    // Query a bbox that covers the first two points but not the third
    let results = index.query(5.0, 15.0, 55.0, 65.0);
    assert!(results.contains(&1), "feature at (10,20) must be found");
    assert!(results.contains(&2), "feature at (50,60) must be found");
    assert!(
        !results.contains(&3),
        "feature at (-10,-20) must NOT be found"
    );

    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

/// `build` with no-envelope blobs must fall back to WKB scanning and still
/// insert the correct bounding boxes.
#[test]
fn test_rtree_build_wkb_scan_path() -> TestResult {
    let tmp = std::env::temp_dir().join("test_gpkg_rtree_wkb.gpkg");
    let _ = std::fs::remove_file(&tmp);

    let rows: Vec<(i64, Vec<u8>)> = vec![
        (1, make_gpkg_point_blob_no_envelope(7.0, 8.0)),
        (2, make_gpkg_point_blob_no_envelope(100.0, 200.0)),
    ];
    populate_test_db(&tmp, &rows)?;

    let gpkg_conn = GpkgConnection::open(&tmp, ConnectionMode::ReadOnly)?;
    let mut index = RTreeIndex::new();
    index.build(&gpkg_conn, "features")?;

    assert_eq!(index.len(), 2, "both features must be indexed via WKB scan");

    let results = index.query(0.0, 0.0, 10.0, 10.0);
    assert!(results.contains(&1), "feature at (7,8) should be found");
    assert!(
        !results.contains(&2),
        "feature at (100,200) should NOT be found"
    );

    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

/// Corrupt geometry blobs must be silently skipped; valid ones must be indexed.
#[test]
fn test_rtree_build_skips_corrupt_geometry() -> TestResult {
    let tmp = std::env::temp_dir().join("test_gpkg_rtree_corrupt.gpkg");
    let _ = std::fs::remove_file(&tmp);

    let rows: Vec<(i64, Vec<u8>)> = vec![
        (1, make_gpkg_point_blob_with_envelope(5.0, 5.0)),
        // Random bytes — definitely not a valid GPKG geometry blob
        (2, vec![0xFFu8, 0xFE, 0x00, 0x01, 0x02]),
    ];
    populate_test_db(&tmp, &rows)?;

    let gpkg_conn = GpkgConnection::open(&tmp, ConnectionMode::ReadOnly)?;
    let mut index = RTreeIndex::new();
    // Must not return an error — corrupt rows are warned and skipped
    index.build(&gpkg_conn, "features")?;

    assert_eq!(
        index.len(),
        1,
        "only the valid feature should be in the index"
    );

    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

/// After `build`, insert/query/remove must still work correctly.
#[test]
fn test_rtree_insert_query_remove_preserved() {
    let mut index = RTreeIndex::new();
    index.insert(1, 0.0, 0.0, 10.0, 10.0);
    index.insert(2, 5.0, 5.0, 15.0, 15.0);

    let results = index.query(0.0, 0.0, 12.0, 12.0);
    assert!(!results.is_empty(), "query must return at least one result");

    let removed = index.remove(1);
    assert!(removed, "remove(1) should return true");
    assert_eq!(index.len(), 1, "index should have one entry left");

    let not_found = index.remove(99);
    assert!(!not_found, "remove(99) should return false");
}

/// `build` called twice must rebuild from scratch (no duplicate entries).
#[test]
fn test_rtree_build_clears_existing_index() -> TestResult {
    let tmp = std::env::temp_dir().join("test_gpkg_rtree_rebuild.gpkg");
    let _ = std::fs::remove_file(&tmp);

    let rows: Vec<(i64, Vec<u8>)> = vec![(1, make_gpkg_point_blob_with_envelope(1.0, 1.0))];
    populate_test_db(&tmp, &rows)?;

    let gpkg_conn = GpkgConnection::open(&tmp, ConnectionMode::ReadOnly)?;
    let mut index = RTreeIndex::new();
    index.build(&gpkg_conn, "features")?;
    assert_eq!(index.len(), 1, "first build should index 1 feature");

    // Second build must clear the index first, so no duplicate entries
    index.build(&gpkg_conn, "features")?;
    assert_eq!(
        index.len(),
        1,
        "second build must not create duplicate entries"
    );

    let _ = std::fs::remove_file(&tmp);
    Ok(())
}
