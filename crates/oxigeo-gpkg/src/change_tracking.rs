//! Trigger-based change tracking for GeoPackage tables.
//!
//! This module installs SQLite `AFTER INSERT / UPDATE / DELETE` triggers on
//! user-specified feature tables and records every modification in a central
//! `gpkg_changes` log table.  The log can be queried in ascending order of
//! insertion so that consumers can replay only the events that occurred after a
//! known checkpoint (identified by the last-seen row `id`).
//!
//! # Feature flag
//!
//! All items in this module are compiled only when the `change-tracking` Cargo
//! feature is enabled.  The feature activates the `oxisql-sqlite-compat`
//! dependency which provides a pure-Rust blocking SQLite API.
//!
//! # SQL-injection mitigation
//!
//! Table and column names are embedded directly into trigger DDL strings because
//! SQLite does not support binding identifiers as parameters.  Every identifier
//! accepted by the public API is validated against the regex
//! `^[A-Za-z_][A-Za-z0-9_]*$` before any SQL is constructed.  Callers that
//! supply names which do not match this pattern receive a
//! [`GpkgError::ChangeTrackingError`] instead of reaching the database.

#[cfg(feature = "change-tracking")]
use std::path::Path;

#[cfg(feature = "change-tracking")]
use oxisql_core::{Row, ToSqlValue};

#[cfg(feature = "change-tracking")]
use oxisql_sqlite_compat::blocking::SqliteConnectionBlocking;

#[cfg(feature = "change-tracking")]
use crate::error::GpkgError;

// ─── identifier validation ────────────────────────────────────────────────────

/// Returns `Ok(())` when `name` is a safe SQL identifier, or a
/// [`GpkgError::ChangeTrackingError`] when it is not.
///
/// Accepted pattern: `^[A-Za-z_][A-Za-z0-9_]*$`.
#[cfg(feature = "change-tracking")]
fn validate_identifier(name: &str) -> Result<(), GpkgError> {
    if name.is_empty() {
        return Err(GpkgError::ChangeTrackingError(
            "identifier must not be empty".to_owned(),
        ));
    }
    let mut chars = name.chars();
    // Safety: we already checked `name.is_empty()` above, so `next()` returns Some.
    let first = match chars.next() {
        Some(c) => c,
        None => {
            return Err(GpkgError::ChangeTrackingError(
                "identifier must not be empty".to_owned(),
            ));
        }
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return Err(GpkgError::ChangeTrackingError(format!(
            "identifier '{name}' must start with a letter or underscore"
        )));
    }
    for ch in chars {
        if !ch.is_ascii_alphanumeric() && ch != '_' {
            return Err(GpkgError::ChangeTrackingError(format!(
                "identifier '{name}' contains invalid character '{ch}'"
            )));
        }
    }
    Ok(())
}

// ─── row helper ──────────────────────────────────────────────────────────────

/// Parse a single row from the `gpkg_changes` table into a [`ChangeLogEntry`].
#[cfg(feature = "change-tracking")]
fn parse_change_log_row(row: &Row) -> Result<ChangeLogEntry, GpkgError> {
    let id = row
        .try_get_by_index::<i64>(0)
        .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;
    let table_name = row
        .try_get_by_index::<String>(1)
        .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;
    let op_int = row
        .try_get_by_index::<i64>(2)
        .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;
    let feature_id = row
        .try_get_by_index::<i64>(3)
        .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;
    let committed_at = row
        .try_get_by_index::<String>(4)
        .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;
    let operation = ChangeOperation::from_int(op_int)?;
    Ok(ChangeLogEntry {
        id,
        table_name,
        operation,
        feature_id,
        committed_at,
    })
}

// ─── ChangeOperation ─────────────────────────────────────────────────────────

/// The kind of DML operation that produced a [`ChangeLogEntry`].
#[cfg(feature = "change-tracking")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeOperation {
    /// A row was inserted (`INSERT`).
    Insert = 1,
    /// An existing row was modified (`UPDATE`).
    Update = 2,
    /// A row was removed (`DELETE`).
    Delete = 3,
}

#[cfg(feature = "change-tracking")]
impl ChangeOperation {
    /// Convert an integer stored in `gpkg_changes.operation` back to the typed
    /// enum variant.
    ///
    /// Returns [`GpkgError::ChangeTrackingError`] for any value other than
    /// `1`, `2`, or `3`.
    pub fn from_int(n: i64) -> Result<Self, GpkgError> {
        match n {
            1 => Ok(Self::Insert),
            2 => Ok(Self::Update),
            3 => Ok(Self::Delete),
            other => Err(GpkgError::ChangeTrackingError(format!(
                "unknown operation code {other}"
            ))),
        }
    }

    /// Return the integer code stored in the `gpkg_changes` table for this
    /// operation variant.
    pub fn as_int(self) -> i64 {
        self as i64
    }
}

// ─── ChangeLogEntry ──────────────────────────────────────────────────────────

/// A single row read back from the `gpkg_changes` log table.
#[cfg(feature = "change-tracking")]
#[derive(Debug, Clone)]
pub struct ChangeLogEntry {
    /// Auto-incremented primary key — monotonically increasing per database.
    pub id: i64,
    /// Name of the feature table that was modified.
    pub table_name: String,
    /// The kind of change that occurred.
    pub operation: ChangeOperation,
    /// Value of the feature-id column at the time of the change.
    pub feature_id: i64,
    /// ISO-8601 UTC timestamp produced by SQLite `datetime('now')`.
    pub committed_at: String,
}

// ─── ChangeTracker ───────────────────────────────────────────────────────────

/// Manages trigger-based change tracking for one GeoPackage (SQLite) database.
///
/// The tracker owns the [`SqliteConnectionBlocking`] so that all trigger DDL and
/// DML share the same connection handle — this is critical for in-memory
/// databases where each connection sees a separate database.
#[cfg(feature = "change-tracking")]
pub struct ChangeTracker {
    conn: SqliteConnectionBlocking,
}

#[cfg(feature = "change-tracking")]
impl ChangeTracker {
    // ── constructors ─────────────────────────────────────────────────────────

    /// Open (or create) a GeoPackage database at the given file-system path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, GpkgError> {
        let path_str = path
            .as_ref()
            .to_str()
            .ok_or_else(|| GpkgError::ChangeTrackingError("path is not valid UTF-8".to_owned()))?;
        let conn = SqliteConnectionBlocking::open(path_str)
            .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;
        Ok(Self { conn })
    }

    /// Create a transient in-memory SQLite database.
    ///
    /// All DML and queries must go through the same [`ChangeTracker`] instance
    /// because an in-memory database is private to its connection.
    pub fn open_in_memory() -> Result<Self, GpkgError> {
        let conn = SqliteConnectionBlocking::open_memory()
            .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;
        Ok(Self { conn })
    }

    // ── accessor ─────────────────────────────────────────────────────────────

    /// Return a shared reference to the underlying [`SqliteConnectionBlocking`].
    ///
    /// Useful in tests to perform DML (INSERT / UPDATE / DELETE) on the same
    /// connection that owns the triggers, which is mandatory when the database
    /// is in-memory.
    pub fn connection(&self) -> &SqliteConnectionBlocking {
        &self.conn
    }

    // ── schema helpers ───────────────────────────────────────────────────────

    /// Create the `gpkg_changes` log table if it does not already exist.
    ///
    /// The table schema is:
    ///
    /// ```sql
    /// CREATE TABLE IF NOT EXISTS gpkg_changes (
    ///     id           INTEGER PRIMARY KEY AUTOINCREMENT,
    ///     table_name   TEXT    NOT NULL,
    ///     operation    INTEGER NOT NULL,
    ///     feature_id   INTEGER NOT NULL,
    ///     committed_at TEXT    NOT NULL DEFAULT (datetime('now'))
    /// )
    /// ```
    pub fn create_changes_table(&self) -> Result<(), GpkgError> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS gpkg_changes (
                    id           INTEGER PRIMARY KEY AUTOINCREMENT,
                    table_name   TEXT    NOT NULL,
                    operation    INTEGER NOT NULL,
                    feature_id   INTEGER NOT NULL,
                    committed_at TEXT    NOT NULL DEFAULT (datetime('now'))
                );",
            )
            .map(|_| ())
            .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))
    }

    // ── trigger management ───────────────────────────────────────────────────

    /// Install `AFTER INSERT`, `AFTER UPDATE`, and `AFTER DELETE` triggers on
    /// `table` that record each change to `gpkg_changes`.
    ///
    /// `fid_column` is the name of the integer primary-key column that uniquely
    /// identifies each feature row (commonly `fid` or `id`).
    ///
    /// Both `table` and `fid_column` are validated as safe SQL identifiers
    /// before any SQL is constructed.  The `gpkg_changes` table is created
    /// automatically if it does not exist yet.
    pub fn enable_tracking(&self, table: &str, fid_column: &str) -> Result<(), GpkgError> {
        validate_identifier(table)?;
        validate_identifier(fid_column)?;

        self.create_changes_table()?;

        // Each trigger is issued as a separate `execute` call because the
        // oxisqlite `execute_batch` statement splitter splits on every `;`,
        // including those inside `BEGIN ... END` trigger bodies, which would
        // corrupt the DDL.  Trigger names and table / column references are
        // embedded directly (no parameter binding for identifiers); both inputs
        // have been validated above.
        let insert_ddl = format!(
            "CREATE TRIGGER IF NOT EXISTS gpkg_track_{table}_insert \
             AFTER INSERT ON {table} \
             BEGIN \
                 INSERT INTO gpkg_changes (table_name, operation, feature_id) \
                 VALUES ('{table}', 1, NEW.{fid_column}); \
             END"
        );
        let update_ddl = format!(
            "CREATE TRIGGER IF NOT EXISTS gpkg_track_{table}_update \
             AFTER UPDATE ON {table} \
             BEGIN \
                 INSERT INTO gpkg_changes (table_name, operation, feature_id) \
                 VALUES ('{table}', 2, NEW.{fid_column}); \
             END"
        );
        let delete_ddl = format!(
            "CREATE TRIGGER IF NOT EXISTS gpkg_track_{table}_delete \
             AFTER DELETE ON {table} \
             BEGIN \
                 INSERT INTO gpkg_changes (table_name, operation, feature_id) \
                 VALUES ('{table}', 3, OLD.{fid_column}); \
             END"
        );

        self.conn
            .execute(&insert_ddl, &[])
            .map(|_| ())
            .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;
        self.conn
            .execute(&update_ddl, &[])
            .map(|_| ())
            .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;
        self.conn
            .execute(&delete_ddl, &[])
            .map(|_| ())
            .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))
    }

    /// Remove the three tracking triggers previously installed by
    /// [`enable_tracking`](Self::enable_tracking) for `table`.
    ///
    /// This is a no-op when the triggers do not exist (`DROP TRIGGER IF
    /// EXISTS`).
    pub fn disable_tracking(&self, table: &str) -> Result<(), GpkgError> {
        validate_identifier(table)?;

        // Issue each DROP separately to avoid the execute_batch semicolon-
        // splitting issue with multi-statement batches.
        for suffix in ["insert", "update", "delete"] {
            let ddl = format!("DROP TRIGGER IF EXISTS gpkg_track_{table}_{suffix}");
            self.conn
                .execute(&ddl, &[])
                .map(|_| ())
                .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;
        }
        Ok(())
    }

    /// Return `true` when the three tracking triggers for `table` are
    /// currently installed, `false` otherwise.
    pub fn is_tracking(&self, table: &str) -> Result<bool, GpkgError> {
        validate_identifier(table)?;

        // Only check for the INSERT trigger as a proxy for all three; they are
        // always created / dropped together.
        let trigger_name = format!("gpkg_track_{table}_insert");

        let rows = self
            .conn
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name=$1",
                &[&trigger_name as &dyn ToSqlValue],
            )
            .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;

        let count = rows
            .first()
            .ok_or_else(|| {
                GpkgError::ChangeTrackingError("no row returned from COUNT(*) query".to_owned())
            })?
            .try_get_by_index::<i64>(0)
            .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;

        Ok(count > 0)
    }

    // ── change log queries ───────────────────────────────────────────────────

    /// Retrieve every change log entry for `table` in ascending order of `id`.
    pub fn get_all_changes(&self, table: &str) -> Result<Vec<ChangeLogEntry>, GpkgError> {
        let rows = self
            .conn
            .query(
                "SELECT id, table_name, operation, feature_id, committed_at
                 FROM gpkg_changes
                 WHERE table_name = $1
                 ORDER BY id ASC",
                &[&table as &dyn ToSqlValue],
            )
            .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;

        rows.iter().map(parse_change_log_row).collect()
    }

    /// Retrieve change log entries for `table` whose `id` is strictly greater
    /// than `since_id`, in ascending order.
    ///
    /// This enables incremental polling: callers record the `id` of the last
    /// entry they processed and pass it here on the next call.
    pub fn get_changes_since(
        &self,
        table: &str,
        since_id: i64,
    ) -> Result<Vec<ChangeLogEntry>, GpkgError> {
        let rows = self
            .conn
            .query(
                "SELECT id, table_name, operation, feature_id, committed_at
                 FROM gpkg_changes
                 WHERE table_name = $1 AND id > $2
                 ORDER BY id ASC",
                &[&table as &dyn ToSqlValue, &since_id as &dyn ToSqlValue],
            )
            .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;

        rows.iter().map(parse_change_log_row).collect()
    }

    // ── housekeeping ─────────────────────────────────────────────────────────

    /// Delete all change log entries for `table` and return the number of rows
    /// removed.
    pub fn clear_changes(&self, table: &str) -> Result<usize, GpkgError> {
        let rows_affected = self
            .conn
            .execute(
                "DELETE FROM gpkg_changes WHERE table_name = $1",
                &[&table as &dyn ToSqlValue],
            )
            .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;
        Ok(rows_affected as usize)
    }

    /// Delete every row from `gpkg_changes` regardless of table name and
    /// return the total number of rows removed.
    pub fn clear_all_changes(&self) -> Result<usize, GpkgError> {
        let rows_affected = self
            .conn
            .execute("DELETE FROM gpkg_changes", &[])
            .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;
        Ok(rows_affected as usize)
    }

    /// Return the names of all feature tables that currently have tracking
    /// triggers installed.
    ///
    /// The list is derived by querying `sqlite_master` for triggers whose
    /// names match the pattern `gpkg_track_%_insert` and stripping the
    /// surrounding prefix / suffix.
    pub fn tracked_tables(&self) -> Result<Vec<String>, GpkgError> {
        let rows = self
            .conn
            .query(
                "SELECT name FROM sqlite_master
                 WHERE type = 'trigger' AND name LIKE 'gpkg_track_%_insert'",
                &[],
            )
            .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;

        rows.iter()
            .map(|row| {
                let trigger_name = row
                    .try_get_by_index::<String>(0)
                    .map_err(|e| GpkgError::ChangeTrackingError(e.to_string()))?;
                // Strip "gpkg_track_" prefix (11 chars) and "_insert" suffix (7 chars).
                let inner = trigger_name
                    .strip_prefix("gpkg_track_")
                    .and_then(|s| s.strip_suffix("_insert"))
                    .ok_or_else(|| {
                        GpkgError::ChangeTrackingError(format!(
                            "unexpected trigger name format: {trigger_name}"
                        ))
                    })?;
                Ok(inner.to_owned())
            })
            .collect()
    }
}
