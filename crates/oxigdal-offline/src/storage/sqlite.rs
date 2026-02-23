//! SQLite storage backend for native platforms

use crate::error::Result;
use crate::storage::{StorageBackend, StorageStatistics};
use crate::types::{Operation, OperationId, Record, RecordId};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::Arc;

/// SQLite storage backend
pub struct SqliteBackend {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteBackend {
    /// Create a new SQLite backend
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Create an in-memory SQLite backend
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Create database schema
    fn create_schema(&self) -> Result<()> {
        let conn = self.conn.lock();

        // Records table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS records (
                id TEXT PRIMARY KEY,
                key TEXT NOT NULL UNIQUE,
                data BLOB NOT NULL,
                version INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                deleted INTEGER NOT NULL,
                source INTEGER NOT NULL,
                sync_status INTEGER NOT NULL
            )",
            [],
        )?;

        // Index on key for faster lookups
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_records_key ON records(key)",
            [],
        )?;

        // Index on sync_status for filtering
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_records_sync_status ON records(sync_status)",
            [],
        )?;

        // Operations table (sync queue)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS operations (
                id TEXT PRIMARY KEY,
                operation_type INTEGER NOT NULL,
                record_id TEXT NOT NULL,
                key TEXT NOT NULL,
                payload BLOB NOT NULL,
                base_version INTEGER NOT NULL,
                target_version INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                retry_count INTEGER NOT NULL,
                last_retry INTEGER,
                priority INTEGER NOT NULL
            )",
            [],
        )?;

        // Index on priority for efficient queue operations
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_operations_priority
             ON operations(priority DESC, created_at ASC)",
            [],
        )?;

        Ok(())
    }

    /// Convert record to database row
    fn record_to_params(record: &Record) -> Vec<Box<dyn rusqlite::ToSql>> {
        vec![
            Box::new(record.id.to_string()),
            Box::new(record.key.clone()),
            Box::new(record.data.to_vec()),
            Box::new(record.version.value() as i64),
            Box::new(record.created_at.timestamp()),
            Box::new(record.updated_at.timestamp()),
            Box::new(if record.deleted { 1 } else { 0 }),
            Box::new(0i32), // source
            Box::new(0i32), // sync_status
        ]
    }

    /// Parse record from database row
    fn parse_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<Record> {
        let id_str: String = row.get(0)?;
        let key: String = row.get(1)?;
        let data: Vec<u8> = row.get(2)?;
        let version: i64 = row.get(3)?;
        let created_at: i64 = row.get(4)?;
        let updated_at: i64 = row.get(5)?;
        let deleted: i32 = row.get(6)?;

        let id = RecordId::parse(&id_str)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

        Ok(Record {
            id,
            key,
            data: bytes::Bytes::from(data),
            version: crate::types::Version::from_u64(version as u64),
            created_at: Utc.timestamp_opt(created_at, 0).single().ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(
                    4,
                    "created_at".to_string(),
                    rusqlite::types::Type::Integer,
                )
            })?,
            updated_at: Utc.timestamp_opt(updated_at, 0).single().ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(
                    5,
                    "updated_at".to_string(),
                    rusqlite::types::Type::Integer,
                )
            })?,
            deleted: deleted != 0,
            metadata: crate::types::RecordMetadata::default(),
        })
    }

    /// Convert operation to database parameters
    fn operation_to_params(operation: &Operation) -> Vec<Box<dyn rusqlite::ToSql>> {
        let op_type = match operation.operation_type {
            crate::types::OperationType::Insert => 0i32,
            crate::types::OperationType::Update => 1i32,
            crate::types::OperationType::Delete => 2i32,
        };

        let last_retry = operation.last_retry.map(|dt| dt.timestamp());

        vec![
            Box::new(operation.id.to_string()),
            Box::new(op_type),
            Box::new(operation.record_id.to_string()),
            Box::new(operation.key.clone()),
            Box::new(operation.payload.to_vec()),
            Box::new(operation.base_version.value() as i64),
            Box::new(operation.target_version.value() as i64),
            Box::new(operation.created_at.timestamp()),
            Box::new(operation.retry_count as i64),
            Box::new(last_retry),
            Box::new(operation.priority as i32),
        ]
    }

    /// Parse operation from database row
    fn parse_operation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Operation> {
        let id_str: String = row.get(0)?;
        let op_type: i32 = row.get(1)?;
        let record_id_str: String = row.get(2)?;
        let key: String = row.get(3)?;
        let payload: Vec<u8> = row.get(4)?;
        let base_version: i64 = row.get(5)?;
        let target_version: i64 = row.get(6)?;
        let created_at: i64 = row.get(7)?;
        let retry_count: i64 = row.get(8)?;
        let last_retry: Option<i64> = row.get(9)?;
        let priority: i32 = row.get(10)?;

        let id = OperationId::parse(&id_str)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let record_id = RecordId::parse(&record_id_str)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

        let operation_type = match op_type {
            0 => crate::types::OperationType::Insert,
            1 => crate::types::OperationType::Update,
            2 => crate::types::OperationType::Delete,
            _ => {
                return Err(rusqlite::Error::InvalidColumnType(
                    1,
                    "operation_type".to_string(),
                    rusqlite::types::Type::Integer,
                ));
            }
        };

        let last_retry_dt = last_retry.and_then(|ts| Utc.timestamp_opt(ts, 0).single());

        Ok(Operation {
            id,
            operation_type,
            record_id,
            key,
            payload: bytes::Bytes::from(payload),
            base_version: crate::types::Version::from_u64(base_version as u64),
            target_version: crate::types::Version::from_u64(target_version as u64),
            created_at: Utc.timestamp_opt(created_at, 0).single().ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(
                    7,
                    "created_at".to_string(),
                    rusqlite::types::Type::Integer,
                )
            })?,
            retry_count: retry_count as usize,
            last_retry: last_retry_dt,
            priority: priority as u8,
        })
    }
}

#[async_trait(?Send)]
impl StorageBackend for SqliteBackend {
    async fn initialize(&mut self) -> Result<()> {
        self.create_schema()
    }

    async fn put_record(&mut self, record: &Record) -> Result<()> {
        let conn = self.conn.lock();
        let params = Self::record_to_params(record);

        conn.execute(
            "INSERT OR REPLACE INTO records
             (id, key, data, version, created_at, updated_at, deleted, source, sync_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
        )?;

        Ok(())
    }

    async fn get_record(&self, key: &str) -> Result<Option<Record>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, key, data, version, created_at, updated_at, deleted
             FROM records WHERE key = ?1 AND deleted = 0",
        )?;

        let record = stmt
            .query_row(params![key], Self::parse_record)
            .optional()?;

        Ok(record)
    }

    async fn get_record_by_id(&self, id: &RecordId) -> Result<Option<Record>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, key, data, version, created_at, updated_at, deleted
             FROM records WHERE id = ?1 AND deleted = 0",
        )?;

        let record = stmt
            .query_row(params![id.to_string()], Self::parse_record)
            .optional()?;

        Ok(record)
    }

    async fn delete_record(&mut self, key: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE records SET deleted = 1, updated_at = ?1 WHERE key = ?2",
            params![Utc::now().timestamp(), key],
        )?;

        Ok(())
    }

    async fn list_records(&self) -> Result<Vec<Record>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, key, data, version, created_at, updated_at, deleted
             FROM records WHERE deleted = 0 ORDER BY updated_at DESC",
        )?;

        let records = stmt
            .query_map([], Self::parse_record)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(records)
    }

    async fn count_records(&self) -> Result<usize> {
        let conn = self.conn.lock();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM records WHERE deleted = 0",
            [],
            |row| row.get(0),
        )?;

        Ok(count as usize)
    }

    async fn clear_records(&mut self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM records", [])?;
        Ok(())
    }

    async fn enqueue_operation(&mut self, operation: &Operation) -> Result<()> {
        let conn = self.conn.lock();
        let params = Self::operation_to_params(operation);

        conn.execute(
            "INSERT OR REPLACE INTO operations
             (id, operation_type, record_id, key, payload, base_version, target_version,
              created_at, retry_count, last_retry, priority)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
        )?;

        Ok(())
    }

    async fn get_pending_operations(&self, limit: usize) -> Result<Vec<Operation>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, operation_type, record_id, key, payload, base_version,
                    target_version, created_at, retry_count, last_retry, priority
             FROM operations
             ORDER BY priority DESC, created_at ASC
             LIMIT ?1",
        )?;

        let operations = stmt
            .query_map(params![limit as i64], Self::parse_operation)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(operations)
    }

    async fn dequeue_operation(&mut self, operation_id: &OperationId) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM operations WHERE id = ?1",
            params![operation_id.to_string()],
        )?;

        Ok(())
    }

    async fn update_operation(&mut self, operation: &Operation) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE operations SET retry_count = ?1, last_retry = ?2 WHERE id = ?3",
            params![
                operation.retry_count as i64,
                operation.last_retry.map(|dt| dt.timestamp()),
                operation.id.to_string()
            ],
        )?;

        Ok(())
    }

    async fn count_pending_operations(&self) -> Result<usize> {
        let conn = self.conn.lock();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM operations", [], |row| row.get(0))?;

        Ok(count as usize)
    }

    async fn clear_operations(&mut self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM operations", [])?;
        Ok(())
    }

    async fn get_statistics(&self) -> Result<StorageStatistics> {
        let conn = self.conn.lock();

        let record_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM records WHERE deleted = 0",
            [],
            |row| row.get(0),
        )?;

        let record_size: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(data)), 0) FROM records WHERE deleted = 0",
            [],
            |row| row.get(0),
        )?;

        let pending_operations: i64 =
            conn.query_row("SELECT COUNT(*) FROM operations", [], |row| row.get(0))?;

        let operations_size: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(payload)), 0) FROM operations",
            [],
            |row| row.get(0),
        )?;

        let mut stats = StorageStatistics {
            record_count: record_count as usize,
            record_size_bytes: record_size as u64,
            pending_operations: pending_operations as usize,
            operations_size_bytes: operations_size as u64,
            backend_type: "SQLite".to_string(),
            custom_metrics: Vec::new(),
        };

        // Add SQLite-specific metrics
        let page_count: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let page_size: i64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;

        stats.add_metric("page_count".to_string(), page_count.to_string());
        stats.add_metric("page_size".to_string(), page_size.to_string());
        stats.add_metric(
            "database_size".to_string(),
            (page_count * page_size).to_string(),
        );

        Ok(stats)
    }

    async fn compact(&mut self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("VACUUM", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[tokio::test]
    async fn test_sqlite_backend() {
        let mut backend = SqliteBackend::in_memory().expect("failed to create backend");
        backend.initialize().await.expect("failed to initialize");

        // Test record operations
        let record = Record::new("test_key".to_string(), Bytes::from("test_data"));
        backend
            .put_record(&record)
            .await
            .expect("failed to put record");

        let retrieved = backend
            .get_record("test_key")
            .await
            .expect("failed to get record");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.as_ref().expect("no record").key, "test_key");

        let count = backend.count_records().await.expect("failed to count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_operation_queue() {
        let mut backend = SqliteBackend::in_memory().expect("failed to create backend");
        backend.initialize().await.expect("failed to initialize");

        let record = Record::new("test".to_string(), Bytes::from("data"));
        let operation = Operation::insert(&record);

        backend
            .enqueue_operation(&operation)
            .await
            .expect("failed to enqueue");

        let pending = backend
            .get_pending_operations(10)
            .await
            .expect("failed to get pending");
        assert_eq!(pending.len(), 1);

        backend
            .dequeue_operation(&operation.id)
            .await
            .expect("failed to dequeue");

        let pending = backend
            .get_pending_operations(10)
            .await
            .expect("failed to get pending");
        assert_eq!(pending.len(), 0);
    }
}
