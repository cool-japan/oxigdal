//! GeoPackage database connection management.

use crate::error::{Error, Result};
use oxisql_core::ToSqlValue;
use oxisql_sqlite_compat::blocking::{SqliteBlockingTransaction, SqliteConnectionBlocking};
use std::path::Path;

/// Connection mode for GeoPackage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMode {
    /// Read-only access
    ReadOnly,
    /// Read-write access
    ReadWrite,
    /// Create new database
    Create,
}

/// GeoPackage database connection wrapper.
pub struct GpkgConnection {
    conn: SqliteConnectionBlocking,
    mode: ConnectionMode,
}

impl GpkgConnection {
    /// Open an existing GeoPackage.
    pub fn open<P: AsRef<Path>>(path: P, mode: ConnectionMode) -> Result<Self> {
        if mode == ConnectionMode::Create {
            return Err(Error::geopackage("Use create() to create new GeoPackage"));
        }

        let path_str = path
            .as_ref()
            .to_str()
            .ok_or_else(|| Error::geopackage("path contains non-UTF-8 characters"))?;

        let conn = SqliteConnectionBlocking::open(path_str)?;

        // Enable foreign keys (best-effort; Limbo may not support this pragma)
        let _ = conn.execute_batch("PRAGMA foreign_keys = ON;");

        // Set journal mode to WAL for better concurrency (best-effort; Limbo defaults to WAL internally)
        if mode == ConnectionMode::ReadWrite {
            let _ = conn.execute_batch("PRAGMA journal_mode = WAL;");
        }

        Ok(Self { conn, mode })
    }

    /// Create a new GeoPackage file.
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_str = path
            .as_ref()
            .to_str()
            .ok_or_else(|| Error::geopackage("path contains non-UTF-8 characters"))?;

        let conn = SqliteConnectionBlocking::open(path_str)?;

        // Enable foreign keys (best-effort; Limbo may not support this pragma)
        let _ = conn.execute_batch("PRAGMA foreign_keys = ON;");

        // Set journal mode to WAL (best-effort; Limbo defaults to WAL internally)
        let _ = conn.execute_batch("PRAGMA journal_mode = WAL;");

        // Set the GeoPackage application ID ('GPKG') and user_version
        // (MMMmmmPP, i.e. 10300 = 1.3.0) per OGC GeoPackage spec §1.1.1.1.1
        // Requirement 2, so that `GpkgDatabase::version()` can later read
        // this same on-disk marker back via `PRAGMA application_id`/
        // `PRAGMA user_version` instead of guessing.
        conn.execute_batch("PRAGMA application_id = 0x47504B47;")?;
        conn.execute_batch("PRAGMA user_version = 10300;")?;

        Ok(Self {
            conn,
            mode: ConnectionMode::ReadWrite,
        })
    }

    /// Get underlying SQLite connection.
    pub fn connection(&self) -> &SqliteConnectionBlocking {
        &self.conn
    }

    /// Execute a query.
    pub fn execute(&self, sql: &str, params: &[&dyn ToSqlValue]) -> Result<u64> {
        Ok(self.conn.execute(sql, params)?)
    }

    /// Execute a batch of SQL statements.
    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        self.conn.execute_batch(sql)?;
        Ok(())
    }

    /// Begin a transaction.
    pub fn transaction(&self) -> Result<SqliteBlockingTransaction<'_>> {
        Ok(self.conn.transaction()?)
    }

    /// List tables by type.
    pub fn list_tables(&self, table_type: super::schema::TableType) -> Result<Vec<String>> {
        let type_str = table_type.as_str().to_string();
        let rows = self.conn.query(
            "SELECT table_name FROM gpkg_contents WHERE data_type = $1 ORDER BY table_name",
            &[&type_str as &dyn ToSqlValue],
        )?;

        rows.into_iter()
            .map(|row| {
                row.try_get_by_index::<String>(0)
                    .map_err(|e| Error::geopackage(format!("column 0: {e}")))
            })
            .collect()
    }

    /// Query a single integer scalar, e.g. a `PRAGMA` value such as
    /// `PRAGMA application_id;` or `PRAGMA user_version;`.
    pub fn query_scalar_i64(&self, sql: &str) -> Result<i64> {
        let rows = self.conn.query(sql, &[])?;
        let row = rows
            .into_iter()
            .next()
            .ok_or_else(|| Error::geopackage(format!("{sql}: no row returned")))?;
        row.try_get_by_index(0)
            .map_err(|e| Error::geopackage(format!("column 0: {e}")))
    }

    /// Check if table exists.
    pub fn table_exists(&self, table_name: &str) -> Result<bool> {
        let table_name_owned = table_name.to_string();
        let rows = self.conn.query(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=$1",
            &[&table_name_owned as &dyn ToSqlValue],
        )?;
        let row = rows
            .into_iter()
            .next()
            .ok_or_else(|| Error::geopackage("table_exists: no row returned"))?;
        let count: i64 = row
            .try_get_by_index(0)
            .map_err(|e| Error::geopackage(format!("column 0: {e}")))?;
        Ok(count > 0)
    }

    /// Flush changes to disk.
    pub fn flush(&mut self) -> Result<()> {
        if self.mode != ConnectionMode::ReadOnly {
            // Checkpoint: Limbo only supports Passive mode; ignore unsupported modes
            let _ = self.conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
        }
        Ok(())
    }

    /// Vacuum database.
    pub fn vacuum(&mut self) -> Result<()> {
        if self.mode == ConnectionMode::ReadOnly {
            return Err(Error::geopackage("Cannot vacuum read-only database"));
        }
        self.conn.execute_batch("VACUUM;")?;
        Ok(())
    }

    /// Check database integrity.
    pub fn check_integrity(&self) -> Result<bool> {
        let rows = self.conn.query("PRAGMA integrity_check;", &[])?;
        let row = rows
            .into_iter()
            .next()
            .ok_or_else(|| Error::geopackage("integrity_check: no row returned"))?;
        let result: String = row
            .try_get_by_index(0)
            .map_err(|e| Error::geopackage(format!("column 0: {e}")))?;
        Ok(result == "ok")
    }

    /// Get connection mode.
    pub fn mode(&self) -> ConnectionMode {
        self.mode
    }

    /// Check if connection is read-only.
    pub fn is_readonly(&self) -> bool {
        self.mode == ConnectionMode::ReadOnly
    }

    /// Get database size in bytes.
    pub fn size(&self) -> Result<i64> {
        let rows = self.conn.query("PRAGMA page_count;", &[])?;
        let row = rows
            .into_iter()
            .next()
            .ok_or_else(|| Error::geopackage("page_count: no row returned"))?;
        let page_count: i64 = row
            .try_get_by_index(0)
            .map_err(|e| Error::geopackage(format!("column 0: {e}")))?;

        let rows = self.conn.query("PRAGMA page_size;", &[])?;
        let row = rows
            .into_iter()
            .next()
            .ok_or_else(|| Error::geopackage("page_size: no row returned"))?;
        let page_size: i64 = row
            .try_get_by_index(0)
            .map_err(|e| Error::geopackage(format!("column 0: {e}")))?;

        Ok(page_count * page_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_connection_creation() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(Error::from)?;
        let conn = GpkgConnection::create(temp_file.path())?;
        assert_eq!(conn.mode(), ConnectionMode::ReadWrite);
        assert!(!conn.is_readonly());
        Ok(())
    }

    #[test]
    fn test_connection_integrity() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(Error::from)?;
        let conn = GpkgConnection::create(temp_file.path())?;
        assert!(conn.check_integrity()?);
        Ok(())
    }

    #[test]
    fn test_connection_size() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(Error::from)?;
        let conn = GpkgConnection::create(temp_file.path())?;
        let size = conn.size()?;
        assert!(size > 0);
        Ok(())
    }

    #[test]
    fn test_table_exists() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(Error::from)?;
        let conn = GpkgConnection::create(temp_file.path())?;
        assert!(!conn.table_exists("nonexistent")?);
        Ok(())
    }

    #[test]
    fn test_create_sets_gpkg_application_id_and_user_version() -> Result<()> {
        let temp_file = NamedTempFile::new().map_err(Error::from)?;
        let conn = GpkgConnection::create(temp_file.path())?;

        let application_id = conn.query_scalar_i64("PRAGMA application_id;")?;
        assert_eq!(application_id, 0x4750_4B47); // 'GPKG'

        let user_version = conn.query_scalar_i64("PRAGMA user_version;")?;
        assert_eq!(user_version, 10300); // 1.3.0

        Ok(())
    }
}
