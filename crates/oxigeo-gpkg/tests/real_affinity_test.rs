//! Integration test for issue 9: restoring SQLite REAL type affinity.
//!
//! Real SQLite stores an integral value written to a REAL column using an
//! INTEGER serial type (a space optimisation), so a raw B-tree reader that
//! ignores column affinity sees a REAL column mixing `Integer` and `Float`
//! cells (e.g. `40` alongside `40.5`). [`GeoPackage::scan_table_by_name_typed`]
//! restores that by consulting the column's declared type from
//! `sqlite_master.sql`; [`GeoPackage::scan_table_by_name`] is unaffected and
//! keeps returning the raw, serial-type-literal value.
//!
//! The fixture is a genuine SQLite database file, generated with Python's
//! `sqlite3` standard-library module (a binding to the real, C-based SQLite
//! engine) so the on-disk bytes reflect actual SQLite storage behavior
//! rather than this crate's own (pure-Rust) writer. If `python3` — or its
//! `sqlite3` module — is unavailable in the environment, the test skips
//! itself gracefully with an `eprintln!`, mirroring the fork-unavailable
//! skip in `tests/locking_test.rs`.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use oxigeo_gpkg::{CellValue, GeoPackage};

// ─────────────────────────────────────────────────────────────────────────────
// Fixture generation
// ─────────────────────────────────────────────────────────────────────────────

/// The table built by the fixture:
///
/// ```sql
/// CREATE TABLE real_test (
///     val REAL,
///     int_col INTEGER,
///     txt_col TEXT,
///     "name ja" TEXT
/// )
/// ```
///
/// Row 1: `val = 40` (an integral value — real SQLite will store this using
/// an INTEGER serial type). Row 2: `val = 40.5` (not integral — always
/// stored using the float serial type, so it already round-trips correctly
/// even without affinity restoration). Both rows carry `int_col` (a genuine
/// INTEGER column, which must NOT be touched by affinity restoration) and a
/// quoted `"name ja"` column (exercises the quote-aware DDL parser).
const TABLE_NAME: &str = "real_test";

/// A temp-file fixture path, unique per process and per call so concurrent
/// nextest retries never collide. Removed on drop (best effort), mirroring
/// `tests/incremental_mutation_test.rs`'s `TempPath`.
struct FixturePath(PathBuf);

impl std::ops::Deref for FixturePath {
    type Target = std::path::Path;

    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for FixturePath {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for FixturePath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn fixture_path() -> FixturePath {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    FixturePath(std::env::temp_dir().join(format!("oxigeo_gpkg_real_affinity_{pid}_{seq}.sqlite")))
}

/// Build the REAL-affinity fixture at `path` using real SQLite (via
/// Python's `sqlite3` stdlib module).
///
/// Returns `Ok(())` on success. Returns `Err(reason)` — with a
/// human-readable explanation suitable for an `eprintln!` skip notice — when
/// `python3` cannot be spawned, or when the script it runs exits non-zero
/// (e.g. `python3` present but its `sqlite3` module missing).
fn build_real_sqlite_fixture(path: &std::path::Path) -> Result<(), String> {
    let path_str = path.to_string_lossy();
    let script = format!(
        r#"
import sqlite3
con = sqlite3.connect({path_str:?})
cur = con.cursor()
cur.execute(
    'CREATE TABLE {table} ('
    'val REAL, '
    'int_col INTEGER, '
    'txt_col TEXT, '
    '"name ja" TEXT'
    ')'
)
cur.execute(
    'INSERT INTO {table} (val, int_col, txt_col, "name ja") VALUES (?, ?, ?, ?)',
    (40, 100, 'hello', 'aiu'),
)
cur.execute(
    'INSERT INTO {table} (val, int_col, txt_col, "name ja") VALUES (?, ?, ?, ?)',
    (40.5, 200, 'world', 'eo'),
)
con.commit()
con.close()
"#,
        table = TABLE_NAME,
    );

    let output = match Command::new("python3").arg("-c").arg(&script).output() {
        Ok(output) => output,
        Err(e) => return Err(format!("python3 not spawnable: {e}")),
    };

    if !output.status.success() {
        return Err(format!(
            "python3 fixture script failed (status {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

/// Build the fixture and open it as a [`GeoPackage`], or return `None`
/// (after an `eprintln!` skip notice) when the fixture cannot be built.
fn open_fixture() -> Option<(FixturePath, GeoPackage)> {
    let path = fixture_path();
    if let Err(reason) = build_real_sqlite_fixture(&path) {
        eprintln!("skipping real_affinity_test: could not build a real-SQLite fixture ({reason})");
        return None;
    }
    let bytes = std::fs::read(&path).expect("read generated fixture");
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse generated fixture as SQLite");
    Some((path, gpkg))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn typed_scan_restores_real_affinity_untyped_scan_unchanged() {
    let Some((_path, gpkg)) = open_fixture() else {
        return; // python3 / sqlite3 unavailable: skip gracefully.
    };

    // ── Untyped scan: unchanged, serial-type-literal behavior ──────────────
    let untyped_rows = gpkg
        .scan_table_by_name(TABLE_NAME)
        .expect("untyped scan")
        .expect("table present");
    assert_eq!(untyped_rows.len(), 2, "expected two fixture rows");

    // Row 1's `val = 40` was written as an integral REAL value; real SQLite
    // stores it using an INTEGER serial type, so the untyped scan — which
    // does not consult column affinity — must see it as `Integer(40)`. If
    // this assertion ever fails, real SQLite's storage behavior has changed
    // and the premise of this fixture no longer holds.
    assert_eq!(
        untyped_rows[0].1[0],
        CellValue::Integer(40),
        "untyped scan must report the raw serial-type-literal value \
         (Integer) for an integral value stored in a REAL column"
    );
    // Row 2's `val = 40.5` is never integral, so it is always stored with
    // the float serial type — already correct without any restoration.
    assert_eq!(untyped_rows[1].1[0], CellValue::Float(40.5));
    // The genuine INTEGER column is untouched either way.
    assert_eq!(untyped_rows[0].1[1], CellValue::Integer(100));
    assert_eq!(untyped_rows[1].1[1], CellValue::Integer(200));

    // ── Typed scan: REAL affinity restored ──────────────────────────────────
    let typed_rows = gpkg
        .scan_table_by_name_typed(TABLE_NAME)
        .expect("typed scan")
        .expect("table present");
    assert_eq!(typed_rows.len(), 2);

    // Both values in the REAL column now come back as Float, regardless of
    // which serial type SQLite chose to store them with.
    assert_eq!(typed_rows[0].1[0], CellValue::Float(40.0));
    assert_eq!(typed_rows[1].1[0], CellValue::Float(40.5));

    // The INTEGER column must NOT be affected by REAL-affinity restoration.
    assert_eq!(typed_rows[0].1[1], CellValue::Integer(100));
    assert_eq!(typed_rows[1].1[1], CellValue::Integer(200));

    // TEXT columns (including the quoted `"name ja"` identifier) are passed
    // through unchanged and stay positionally aligned.
    assert_eq!(typed_rows[0].1[2], CellValue::Text("hello".to_string()));
    assert_eq!(typed_rows[1].1[2], CellValue::Text("world".to_string()));
    assert_eq!(typed_rows[0].1[3], CellValue::Text("aiu".to_string()));
    assert_eq!(typed_rows[1].1[3], CellValue::Text("eo".to_string()));

    // rowids must be unaffected by affinity restoration.
    assert_eq!(typed_rows[0].0, untyped_rows[0].0);
    assert_eq!(typed_rows[1].0, untyped_rows[1].0);
}

#[test]
fn typed_scan_missing_table_returns_none() {
    let Some((_path, gpkg)) = open_fixture() else {
        return; // python3 / sqlite3 unavailable: skip gracefully.
    };

    assert!(
        gpkg.scan_table_by_name_typed("does_not_exist")
            .expect("scan should not error for a missing table")
            .is_none()
    );
}

/// Build a second, independent fixture whose `AUTOINCREMENT` table causes
/// real SQLite to also create the system table `sqlite_sequence(name, seq)`
/// — a real-world table declared with **no column types at all**
/// (`CREATE TABLE sqlite_sequence(name,seq)`), exercising the typeless-column
/// path of the declared-type parser against genuine SQLite-authored DDL
/// rather than a hand-written unit-test string.
fn build_autoincrement_fixture(path: &std::path::Path) -> Result<(), String> {
    let path_str = path.to_string_lossy();
    let script = format!(
        r#"
import sqlite3
con = sqlite3.connect({path_str:?})
cur = con.cursor()
cur.execute('CREATE TABLE with_seq (id INTEGER PRIMARY KEY AUTOINCREMENT, val REAL)')
cur.execute('INSERT INTO with_seq (val) VALUES (1)')
cur.execute('INSERT INTO with_seq (val) VALUES (2.5)')
con.commit()
con.close()
"#,
    );

    let output = match Command::new("python3").arg("-c").arg(&script).output() {
        Ok(output) => output,
        Err(e) => return Err(format!("python3 not spawnable: {e}")),
    };

    if !output.status.success() {
        return Err(format!(
            "python3 fixture script failed (status {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

/// `scan_table_by_name_typed` must never differ from `scan_table_by_name`
/// for a table it leaves untouched — in particular `sqlite_sequence`, whose
/// two columns are both declared with no type at all. This is the strongest
/// available check that "typed scan only ever changes REAL-affinity
/// columns" holds against bytes written by real SQLite, not just against the
/// synthetic DDL strings used in the `btree::affinity` unit tests.
#[test]
fn typed_scan_matches_untyped_for_typeless_system_table() {
    let path = fixture_path();
    if let Err(reason) = build_autoincrement_fixture(&path) {
        eprintln!("skipping real_affinity_test: could not build a real-SQLite fixture ({reason})");
        return; // python3 / sqlite3 unavailable: skip gracefully.
    }
    let bytes = std::fs::read(&path).expect("read generated fixture");
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse generated fixture as SQLite");

    let untyped = gpkg
        .scan_table_by_name("sqlite_sequence")
        .expect("untyped scan")
        .expect("sqlite_sequence present (AUTOINCREMENT table was created)");
    let typed = gpkg
        .scan_table_by_name_typed("sqlite_sequence")
        .expect("typed scan")
        .expect("sqlite_sequence present (AUTOINCREMENT table was created)");

    assert_eq!(
        typed, untyped,
        "a table with no declared column types must scan identically \
         whether or not affinity restoration is requested"
    );
    // Sanity: this really did exercise a non-empty, typeless-column table.
    assert!(!untyped.is_empty());
    assert_eq!(untyped[0].1[0], CellValue::Text("with_seq".to_string()));
}
