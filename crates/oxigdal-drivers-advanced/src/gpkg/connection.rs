//! GeoPackage database connection management.

use crate::error::{Error, Result};
use rusqlite::{Connection, OpenFlags, Transaction};
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
    conn: Connection,
    mode: ConnectionMode,
}

impl GpkgConnection {
    /// Open an existing GeoPackage.
    pub fn open<P: AsRef<Path>>(path: P, mode: ConnectionMode) -> Result<Self> {
        let flags = match mode {
            ConnectionMode::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
            ConnectionMode::ReadWrite => {
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX
            }
            ConnectionMode::Create => {
                return Err(Error::geopackage("Use create() to create new GeoPackage"));
            }
        };

        let conn = Connection::open_with_flags(path, flags)?;

        // Enable foreign keys
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        // Set journal mode to WAL for better concurrency
        if mode == ConnectionMode::ReadWrite {
            conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        }

        Ok(Self { conn, mode })
    }

    /// Create a new GeoPackage file.
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Enable foreign keys
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        // Set journal mode to WAL
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;

        // Set application ID for GeoPackage
        conn.execute_batch("PRAGMA application_id = 0x47503130;")?;

        Ok(Self {
            conn,
            mode: ConnectionMode::ReadWrite,
        })
    }

    /// Get underlying SQLite connection.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Execute a query.
    pub fn execute(&self, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Result<usize> {
        Ok(self.conn.execute(sql, params)?)
    }

    /// Execute a batch of SQL statements.
    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        Ok(self.conn.execute_batch(sql)?)
    }

    /// Begin a transaction.
    pub fn transaction(&mut self) -> Result<Transaction<'_>> {
        Ok(self.conn.transaction()?)
    }

    /// List tables by type.
    pub fn list_tables(&self, table_type: super::schema::TableType) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT table_name FROM gpkg_contents WHERE data_type = ?1 ORDER BY table_name",
        )?;

        let type_str = table_type.as_str();
        let tables: rusqlite::Result<Vec<String>> =
            stmt.query_map([type_str], |row| row.get(0))?.collect();

        Ok(tables?)
    }

    /// Check if table exists.
    pub fn table_exists(&self, table_name: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table_name],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Flush changes to disk.
    pub fn flush(&mut self) -> Result<()> {
        // Checkpoint the WAL file
        if self.mode != ConnectionMode::ReadOnly {
            self.conn.execute_batch("PRAGMA wal_checkpoint(FULL);")?;
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
        let result: String = self
            .conn
            .query_row("PRAGMA integrity_check;", [], |row| row.get(0))?;
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
        let page_count: i64 = self
            .conn
            .query_row("PRAGMA page_count;", [], |row| row.get(0))?;
        let page_size: i64 = self
            .conn
            .query_row("PRAGMA page_size;", [], |row| row.get(0))?;
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
}
