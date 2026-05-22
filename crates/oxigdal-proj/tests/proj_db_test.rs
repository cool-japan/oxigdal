//! Integration tests for the PROJ.db SQLite reader.
//!
//! All tests are gated on the `proj-db` feature.  They use in-memory SQLite
//! databases with a fixture `crs_view` table so they do not depend on a real
//! system PROJ.db installation.

#![cfg(feature = "proj-db")]
#![allow(clippy::expect_used)]
// env::set_var / remove_var are technically unsafe in Rust 2024 (data race
// risk in multi-threaded processes); our tests are single-threaded.
#![allow(unsafe_code)]

use std::path::PathBuf;

use oxigdal_proj::epsg::{EpsgDatabase, populate_from_proj_db};
use oxigdal_proj::{ProjDb, default_proj_db_paths};
use rusqlite::Connection;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Creates an in-memory SQLite connection with a `crs_view` table that mimics
/// the schema used by the module.
fn make_fixture_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory SQLite");
    conn.execute_batch(
        "CREATE TABLE crs_view (
             auth_name TEXT NOT NULL,
             code      INTEGER NOT NULL,
             name      TEXT NOT NULL,
             type      TEXT NOT NULL,
             deprecated INTEGER NOT NULL DEFAULT 0,
             area      TEXT
         );",
    )
    .expect("create crs_view");
    conn
}

/// Inserts a minimal EPSG row into `crs_view`.
fn insert_row(
    conn: &Connection,
    auth: &str,
    code: i64,
    name: &str,
    crs_type: &str,
    deprecated: i64,
    area: Option<&str>,
) {
    conn.execute(
        "INSERT INTO crs_view (auth_name, code, name, type, deprecated, area) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![auth, code, name, crs_type, deprecated, area],
    )
    .expect("insert row");
}

// ---------------------------------------------------------------------------
// 1. default_proj_db_paths returns a non-empty list
// ---------------------------------------------------------------------------

#[test]
fn test_default_proj_db_paths_returns_non_empty_list() {
    let paths = default_proj_db_paths();
    assert!(
        paths.len() >= 3,
        "expected at least 3 candidate paths, got {}",
        paths.len()
    );
}

// ---------------------------------------------------------------------------
// 2. $PROJ_DATA env-var is honoured
// ---------------------------------------------------------------------------

#[test]
fn test_default_proj_db_paths_honors_env_var() {
    // Stash existing value
    let prev = std::env::var("PROJ_DATA").ok();

    // SAFETY: single-threaded test; no concurrent access to the env
    unsafe { std::env::set_var("PROJ_DATA", "/tmp/test_proj_data") };
    let paths = default_proj_db_paths();

    // Restore
    // SAFETY: same test, no concurrent env access
    unsafe {
        match prev {
            Some(val) => std::env::set_var("PROJ_DATA", val),
            None => std::env::remove_var("PROJ_DATA"),
        }
    }

    assert!(
        !paths.is_empty(),
        "paths should not be empty after setting PROJ_DATA"
    );
    assert_eq!(
        paths[0],
        PathBuf::from("/tmp/test_proj_data/proj.db"),
        "first path should honour PROJ_DATA"
    );
}

// ---------------------------------------------------------------------------
// 3. open() on a non-existent path returns an error
// ---------------------------------------------------------------------------

#[test]
fn test_open_nonexistent_path_returns_error() {
    let result = ProjDb::open("/nonexistent/path/proj.db");
    assert!(
        result.is_err(),
        "expected Err for non-existent path, got Ok"
    );
}

// ---------------------------------------------------------------------------
// 4. open_first_available() returns Ok when no known paths exist
// ---------------------------------------------------------------------------

#[test]
fn test_open_first_available_returns_none_when_no_paths_exist() {
    // Remove env-var overrides so we fall through to hard-coded system paths.
    let prev_data = std::env::var("PROJ_DATA").ok();
    let prev_lib = std::env::var("PROJ_LIB").ok();
    // SAFETY: single-threaded test; no concurrent access to the env
    unsafe {
        std::env::remove_var("PROJ_DATA");
        std::env::remove_var("PROJ_LIB");
    }

    let result = ProjDb::open_first_available();

    // Restore
    // SAFETY: same test, no concurrent env access
    unsafe {
        if let Some(v) = prev_data {
            std::env::set_var("PROJ_DATA", v);
        }
        if let Some(v) = prev_lib {
            std::env::set_var("PROJ_LIB", v);
        }
    }

    // Either Ok(None) (no system proj.db) or Ok(Some(_)) (proj.db installed)
    // is acceptable; we just must not panic or return an internal error.
    assert!(
        result.is_ok(),
        "open_first_available should not propagate an error when no DB is found"
    );
}

// ---------------------------------------------------------------------------
// 5. lookup_epsg 4326 from fixture
// ---------------------------------------------------------------------------

#[test]
fn test_projdb_lookup_epsg_4326_from_fixture() {
    let conn = make_fixture_conn();
    insert_row(
        &conn,
        "EPSG",
        4326,
        "WGS 84",
        "geographic 2D CRS",
        0,
        Some("World"),
    );

    let db = ProjDb::from_conn(conn);
    let entry = db
        .lookup_epsg(4326)
        .expect("lookup should not error")
        .expect("EPSG 4326 should be present");

    assert_eq!(entry.name, "WGS 84");
    assert_eq!(entry.code, 4326);
    assert!(!entry.deprecated);
}

// ---------------------------------------------------------------------------
// 6. lookup_epsg unknown code returns None
// ---------------------------------------------------------------------------

#[test]
fn test_projdb_lookup_unknown_code_returns_none() {
    let conn = make_fixture_conn();
    let db = ProjDb::from_conn(conn);
    let result = db.lookup_epsg(999_999).expect("lookup should not error");
    assert!(result.is_none(), "expected None for unknown code 999999");
}

// ---------------------------------------------------------------------------
// 7. count_epsg_codes matches fixture size
// ---------------------------------------------------------------------------

#[test]
fn test_projdb_count_epsg_codes_matches_fixture_size() {
    let conn = make_fixture_conn();
    insert_row(&conn, "EPSG", 4326, "WGS 84", "geographic 2D CRS", 0, None);
    insert_row(&conn, "EPSG", 4269, "NAD83", "geographic 2D CRS", 0, None);
    insert_row(
        &conn,
        "EPSG",
        3857,
        "Web Mercator",
        "projected CRS",
        0,
        None,
    );
    insert_row(&conn, "EPSG", 32601, "UTM 1N", "projected CRS", 0, None);
    insert_row(&conn, "EPSG", 32701, "UTM 1S", "projected CRS", 0, None);

    let db = ProjDb::from_conn(conn);
    let count = db.count_epsg_codes().expect("count should not error");
    assert_eq!(count, 5, "expected 5 EPSG codes in fixture");
}

// ---------------------------------------------------------------------------
// 8. list_epsg_codes returns codes in ascending order
// ---------------------------------------------------------------------------

#[test]
fn test_projdb_list_epsg_codes_returns_sorted() {
    let conn = make_fixture_conn();
    // Insert deliberately out of order
    insert_row(&conn, "EPSG", 4326, "WGS 84", "geographic 2D CRS", 0, None);
    insert_row(&conn, "EPSG", 4267, "NAD27", "geographic 2D CRS", 0, None);
    insert_row(&conn, "EPSG", 4269, "NAD83", "geographic 2D CRS", 0, None);

    let db = ProjDb::from_conn(conn);
    let codes = db.list_epsg_codes(None).expect("list should not error");
    assert_eq!(
        codes,
        vec![4267, 4269, 4326],
        "codes must be sorted ascending"
    );
}

// ---------------------------------------------------------------------------
// 9. populate_from_proj_db inserts new codes
// ---------------------------------------------------------------------------

#[test]
fn test_populate_from_proj_db_inserts_new_codes() {
    let conn = make_fixture_conn();
    insert_row(
        &conn,
        "EPSG",
        4269,
        "NAD83",
        "geographic 2D CRS",
        0,
        Some("North America"),
    );

    let proj_db = ProjDb::from_conn(conn);

    // Create a fresh database that does NOT contain EPSG 4269
    let mut db = EpsgDatabase::new();
    // Remove 4269 if the built-ins happen to include it
    db.remove_definition(4269);
    assert!(!db.contains(4269), "pre-condition: 4269 not in db");

    populate_from_proj_db(&mut db, &proj_db).expect("populate should not error");

    assert!(db.contains(4269), "EPSG 4269 should have been inserted");
}

// ---------------------------------------------------------------------------
// 10. Built-in entries are never overwritten (priority check)
// ---------------------------------------------------------------------------

#[test]
fn test_populate_from_proj_db_preserves_builtins_priority() {
    use oxigdal_proj::epsg::{CrsType, EpsgDefinition};

    let conn = make_fixture_conn();
    insert_row(
        &conn,
        "EPSG",
        4326,
        "WGS 84 - from proj.db",
        "geographic 2D CRS",
        0,
        None,
    );

    let proj_db = ProjDb::from_conn(conn);

    let mut db = EpsgDatabase::new();
    // Plant a sentinel in place of EPSG 4326 using the public API
    db.add_definition(EpsgDefinition {
        code: 4326,
        name: "built-in sentinel".to_owned(),
        proj_string: "+proj=longlat +datum=WGS84 +no_defs".to_owned(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "World".to_owned(),
        unit: "degree".to_owned(),
        datum: "WGS84".to_owned(),
    });

    populate_from_proj_db(&mut db, &proj_db).expect("populate should not error");

    let entry = db.lookup(4326).expect("4326 must exist");
    assert_eq!(
        entry.name, "built-in sentinel",
        "built-in entry must not be overwritten by proj_db"
    );
}

// ---------------------------------------------------------------------------
// 11. populate_from_proj_db returns the count of actually-inserted entries
// ---------------------------------------------------------------------------

#[test]
fn test_populate_from_proj_db_returns_insert_count() {
    use oxigdal_proj::epsg::{CrsType, EpsgDefinition};

    let conn = make_fixture_conn();
    insert_row(&conn, "EPSG", 4326, "WGS 84", "geographic 2D CRS", 0, None);
    insert_row(&conn, "EPSG", 4269, "NAD83", "geographic 2D CRS", 0, None);
    insert_row(
        &conn,
        "EPSG",
        3857,
        "Web Mercator",
        "projected CRS",
        0,
        None,
    );

    let proj_db = ProjDb::from_conn(conn);

    let mut db = EpsgDatabase::new();
    // Pre-load 4326 with a sentinel so it won't be re-inserted (it already exists as built-in)
    db.add_definition(EpsgDefinition {
        code: 4326,
        name: "pre-existing".to_owned(),
        proj_string: "+proj=longlat +datum=WGS84 +no_defs".to_owned(),
        wkt: None,
        crs_type: CrsType::Geographic,
        area_of_use: "World".to_owned(),
        unit: "degree".to_owned(),
        datum: "WGS84".to_owned(),
    });
    // Remove 4269 and 3857 if the built-ins include them so we can count inserts cleanly
    db.remove_definition(4269);
    db.remove_definition(3857);

    let count = populate_from_proj_db(&mut db, &proj_db).expect("populate should not error");

    assert_eq!(
        count, 2,
        "expected exactly 2 new insertions (4269 and 3857)"
    );
}

// ---------------------------------------------------------------------------
// 12. lookup_authority with ESRI codes
// ---------------------------------------------------------------------------

#[test]
fn test_projdb_lookup_authority_esri_code() {
    let conn = make_fixture_conn();
    insert_row(&conn, "EPSG", 4326, "WGS 84", "geographic 2D CRS", 0, None);
    insert_row(
        &conn,
        "ESRI",
        102100,
        "Web Mercator Aux",
        "projected CRS",
        0,
        None,
    );

    let db = ProjDb::from_conn(conn);

    let entry = db
        .lookup_authority("ESRI", 102_100)
        .expect("lookup should not error")
        .expect("ESRI:102100 should be found");

    assert_eq!(entry.code, 102_100);
    assert_eq!(entry.name, "Web Mercator Aux");
}

// ---------------------------------------------------------------------------
// 13. deprecated flag is parsed correctly
// ---------------------------------------------------------------------------

#[test]
fn test_projdb_entry_deprecated_flag_parsed() {
    let conn = make_fixture_conn();
    insert_row(
        &conn,
        "EPSG",
        4001,
        "Unknown datum based on Airy 1830 ellipsoid",
        "geographic 2D CRS",
        1, // deprecated = true
        None,
    );

    let db = ProjDb::from_conn(conn);
    let entry = db
        .lookup_epsg(4001)
        .expect("lookup should not error")
        .expect("EPSG 4001 should be present");

    assert!(
        entry.deprecated,
        "entry with deprecated=1 must have deprecated==true"
    );
}
