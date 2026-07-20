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

use std::sync::Mutex;

use oxigeo_proj::epsg::{EpsgDatabase, populate_from_proj_db};
use oxigeo_proj::{ProjDb, default_proj_db_paths};
use oxisql_core::ToSqlValue;
use oxisql_sqlite_compat::blocking::SqliteConnectionBlocking;

/// Mutex to serialise tests that mutate process-wide environment variables.
///
/// Tests that call `std::env::set_var` / `remove_var` must hold this lock for
/// the full duration of their env-mutation window to avoid data races with
/// other tests running in parallel.
static ENV_MUTEX: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

fn make_fixture_conn() -> SqliteConnectionBlocking {
    let conn = SqliteConnectionBlocking::open_memory().expect("in-memory SQLite");
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

fn insert_row(
    conn: &SqliteConnectionBlocking,
    auth: &str,
    code: i64,
    name: &str,
    crs_type: &str,
    deprecated: i64,
    area: Option<&str>,
) {
    let area_string: Option<String> = area.map(|s| s.to_owned());
    conn.execute(
        "INSERT INTO crs_view (auth_name, code, name, type, deprecated, area) \
         VALUES ($1, $2, $3, $4, $5, $6)",
        &[
            &auth as &dyn ToSqlValue,
            &code as &dyn ToSqlValue,
            &name as &dyn ToSqlValue,
            &crs_type as &dyn ToSqlValue,
            &deprecated as &dyn ToSqlValue,
            &area_string as &dyn ToSqlValue,
        ],
    )
    .expect("insert row");
}

// ---------------------------------------------------------------------------
// Tests
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

#[test]
fn test_default_proj_db_paths_honors_env_var() {
    let _guard = ENV_MUTEX.lock().expect("env mutex");
    let prev = std::env::var("PROJ_DATA").ok();
    let tmp_proj_data = std::env::temp_dir().join("test_proj_data");
    unsafe { std::env::set_var("PROJ_DATA", &tmp_proj_data) };
    let paths = default_proj_db_paths();
    unsafe {
        match prev {
            Some(val) => std::env::set_var("PROJ_DATA", val),
            None => std::env::remove_var("PROJ_DATA"),
        }
    }
    assert!(!paths.is_empty());
    assert_eq!(paths[0], tmp_proj_data.join("proj.db"));
}

#[test]
fn test_open_nonexistent_path_returns_error() {
    let result = ProjDb::open("/nonexistent/path/proj.db");
    assert!(
        result.is_err(),
        "expected Err for non-existent path, got Ok"
    );
}

#[test]
fn test_open_first_available_returns_none_when_no_paths_exist() {
    let _guard = ENV_MUTEX.lock().expect("env mutex");
    let prev_data = std::env::var("PROJ_DATA").ok();
    let prev_lib = std::env::var("PROJ_LIB").ok();
    unsafe {
        std::env::remove_var("PROJ_DATA");
        std::env::remove_var("PROJ_LIB");
    }
    let result = ProjDb::open_first_available();
    unsafe {
        if let Some(v) = prev_data {
            std::env::set_var("PROJ_DATA", v);
        }
        if let Some(v) = prev_lib {
            std::env::set_var("PROJ_LIB", v);
        }
    }
    assert!(
        result.is_ok(),
        "open_first_available should not error when no DB found"
    );
}

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

#[test]
fn test_projdb_lookup_unknown_code_returns_none() {
    let conn = make_fixture_conn();
    let db = ProjDb::from_conn(conn);
    let result = db.lookup_epsg(999_999).expect("lookup should not error");
    assert!(result.is_none(), "expected None for unknown code 999999");
}

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

#[test]
fn test_projdb_list_epsg_codes_returns_sorted() {
    let conn = make_fixture_conn();
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
    let mut db = EpsgDatabase::new();
    db.remove_definition(4269);
    assert!(!db.contains(4269), "pre-condition: 4269 not in db");
    populate_from_proj_db(&mut db, &proj_db).expect("populate should not error");
    assert!(db.contains(4269), "EPSG 4269 should have been inserted");
}

#[test]
fn test_populate_from_proj_db_preserves_builtins_priority() {
    use oxigeo_proj::epsg::{CrsType, EpsgDefinition};
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
        "built-in must not be overwritten"
    );
}

#[test]
fn test_populate_from_proj_db_returns_insert_count() {
    use oxigeo_proj::epsg::{CrsType, EpsgDefinition};
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
    db.remove_definition(4269);
    db.remove_definition(3857);
    let count = populate_from_proj_db(&mut db, &proj_db).expect("populate should not error");
    assert_eq!(
        count, 2,
        "expected exactly 2 new insertions (4269 and 3857)"
    );
}

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

#[test]
fn test_projdb_entry_deprecated_flag_parsed() {
    let conn = make_fixture_conn();
    insert_row(
        &conn,
        "EPSG",
        4001,
        "Unknown datum based on Airy 1830 ellipsoid",
        "geographic 2D CRS",
        1,
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
