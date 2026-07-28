//! SQLite storage backend for native platforms.
//!
//! Uses the Pure-Rust [`oxisql_sqlite_compat::SqliteConnection`] engine
//! (no C/FFI, no `libsqlite3`).  All database calls use `.await` directly
//! against the async OxiSQL engine — no internal blocking runtime is needed.
//!
//! # Notes on cosmetic stats
//!
//! `PRAGMA page_count` and `PRAGMA page_size` are not yet reliably supported
//! by the OxiSQL engine.  The `get_statistics` implementation therefore reports
//! those two metrics as `"0"` (cosmetic omission, not a correctness issue).
//!
//! `VACUUM` is not yet supported either; `compact` is a no-op with a comment.
//!
//! # Parameter placeholders
//!
//! OxiSQL uses `$1`, `$2`, … positional placeholders.  All SQL in this module
//! uses the `$N` form.

use crate::error::{Error, Result};
use crate::storage::{StorageBackend, StorageStatistics};
use crate::types::{Operation, OperationId, Record, RecordId};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use oxisql_core::{Connection, ToSqlValue, Value};
use oxisql_sqlite_compat::SqliteConnection;
use std::path::Path;

// ---------------------------------------------------------------------------
// Error mapping helper
// ---------------------------------------------------------------------------

fn db_err(e: impl std::fmt::Display) -> Error {
    Error::Database(e.to_string())
}

// ---------------------------------------------------------------------------
// Compact binary encoding for `RecordMetadata.tags` / `.attributes`
//
// Neither `Vec<String>` nor `Vec<(String, String)>` derive `serde::Serialize`
// (this crate's `serde` support is an optional feature, and the native
// SQLite backend must work without it), so these are hand-rolled
// length-prefixed binary encodings stored in dedicated BLOB columns. This
// avoids any ambiguity from separator characters that could legitimately
// appear inside a tag or attribute value.
// ---------------------------------------------------------------------------

fn encode_tags(tags: &[String]) -> Vec<u8> {
    let mut buf = Vec::new();
    for tag in tags {
        let bytes = tag.as_bytes();
        buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(bytes);
    }
    buf
}

fn read_length_prefixed_string(blob: &[u8], pos: &mut usize) -> Result<String> {
    if *pos + 4 > blob.len() {
        return Err(Error::Database(
            "corrupt metadata blob: truncated length prefix".to_string(),
        ));
    }
    let len =
        u32::from_le_bytes([blob[*pos], blob[*pos + 1], blob[*pos + 2], blob[*pos + 3]]) as usize;
    *pos += 4;
    if *pos + len > blob.len() {
        return Err(Error::Database(
            "corrupt metadata blob: truncated string data".to_string(),
        ));
    }
    let s = String::from_utf8(blob[*pos..*pos + len].to_vec())
        .map_err(|e| Error::Database(format!("corrupt metadata blob: invalid utf8: {e}")))?;
    *pos += len;
    Ok(s)
}

fn decode_tags(blob: &[u8]) -> Result<Vec<String>> {
    let mut tags = Vec::new();
    let mut pos = 0usize;
    while pos < blob.len() {
        tags.push(read_length_prefixed_string(blob, &mut pos)?);
    }
    Ok(tags)
}

fn encode_attributes(attrs: &[(String, String)]) -> Vec<u8> {
    let mut buf = Vec::new();
    for (key, value) in attrs {
        let kb = key.as_bytes();
        let vb = value.as_bytes();
        buf.extend_from_slice(&(kb.len() as u32).to_le_bytes());
        buf.extend_from_slice(kb);
        buf.extend_from_slice(&(vb.len() as u32).to_le_bytes());
        buf.extend_from_slice(vb);
    }
    buf
}

fn decode_attributes(blob: &[u8]) -> Result<Vec<(String, String)>> {
    let mut attrs = Vec::new();
    let mut pos = 0usize;
    while pos < blob.len() {
        let key = read_length_prefixed_string(blob, &mut pos)?;
        let value = read_length_prefixed_string(blob, &mut pos)?;
        attrs.push((key, value));
    }
    Ok(attrs)
}

// ---------------------------------------------------------------------------
// SqliteBackend
// ---------------------------------------------------------------------------

/// Pure-Rust SQLite storage backend backed by OxiSQL.
pub struct SqliteBackend {
    conn: SqliteConnection,
}

impl SqliteBackend {
    /// Create a new SQLite backend at the given file path.
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_str = path.as_ref().to_string_lossy().into_owned();
        let conn = SqliteConnection::open(&path_str).await.map_err(db_err)?;
        Ok(Self { conn })
    }

    /// Create an in-memory SQLite backend (useful in tests).
    pub async fn in_memory() -> Result<Self> {
        let conn = SqliteConnection::open_memory().await.map_err(db_err)?;
        Ok(Self { conn })
    }

    // ---- Internal helpers ---------------------------------------------------

    async fn exec(&self, sql: &str, params: &[&dyn ToSqlValue]) -> Result<u64> {
        self.conn.execute(sql, params).await.map_err(db_err)
    }

    async fn exec_batch(&self, sql: &str) -> Result<()> {
        self.conn.execute_batch(sql).await.map_err(db_err)?;
        Ok(())
    }

    async fn query(&self, sql: &str, params: &[&dyn ToSqlValue]) -> Result<Vec<oxisql_core::Row>> {
        self.conn.query(sql, params).await.map_err(db_err)
    }

    async fn query_count(&self, sql: &str, params: &[&dyn ToSqlValue]) -> Result<i64> {
        let rows = self.query(sql, params).await?;
        match rows.first().and_then(|r| r.get_by_index(0)) {
            Some(Value::I64(n)) => Ok(*n),
            Some(Value::Null) | None => Ok(0),
            Some(other) => Err(Error::Database(format!(
                "query_count: unexpected value type {}",
                other.type_name()
            ))),
        }
    }

    // ---- Schema creation ----------------------------------------------------

    async fn create_schema(&self) -> Result<()> {
        // Records table
        self.exec_batch(
            "CREATE TABLE IF NOT EXISTS records (
                id TEXT PRIMARY KEY,
                key TEXT NOT NULL UNIQUE,
                data BLOB NOT NULL,
                version INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                deleted INTEGER NOT NULL,
                source INTEGER NOT NULL,
                sync_status INTEGER NOT NULL,
                tags BLOB NOT NULL,
                attributes BLOB NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_records_key ON records(key);

            CREATE INDEX IF NOT EXISTS idx_records_sync_status ON records(sync_status);

            CREATE TABLE IF NOT EXISTS operations (
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
            );

            CREATE INDEX IF NOT EXISTS idx_operations_priority
             ON operations(priority, created_at);",
        )
        .await
    }

    // ---- Row mapping helpers ------------------------------------------------

    /// Map an OxiSQL `Row` to a [`Record`].
    fn row_to_record(row: &oxisql_core::Row) -> Result<Record> {
        // Expected column order: id(0), key(1), data(2), version(3),
        //   created_at(4), updated_at(5), deleted(6), source(7),
        //   sync_status(8), tags(9), attributes(10)
        let id_str = match row.get_by_index(0) {
            Some(Value::Text(s)) => s.clone(),
            other => {
                return Err(Error::Database(format!(
                    "record.id: expected TEXT, got {:?}",
                    other
                )));
            }
        };
        let key = match row.get_by_index(1) {
            Some(Value::Text(s)) => s.clone(),
            other => {
                return Err(Error::Database(format!(
                    "record.key: expected TEXT, got {:?}",
                    other
                )));
            }
        };
        let data: Vec<u8> = match row.get_by_index(2) {
            Some(Value::Blob(b)) => b.clone(),
            other => {
                return Err(Error::Database(format!(
                    "record.data: expected BLOB, got {:?}",
                    other
                )));
            }
        };
        let version: i64 = match row.get_by_index(3) {
            Some(Value::I64(n)) => *n,
            other => {
                return Err(Error::Database(format!(
                    "record.version: expected I64, got {:?}",
                    other
                )));
            }
        };
        let created_at: i64 = match row.get_by_index(4) {
            Some(Value::I64(n)) => *n,
            other => {
                return Err(Error::Database(format!(
                    "record.created_at: expected I64, got {:?}",
                    other
                )));
            }
        };
        let updated_at: i64 = match row.get_by_index(5) {
            Some(Value::I64(n)) => *n,
            other => {
                return Err(Error::Database(format!(
                    "record.updated_at: expected I64, got {:?}",
                    other
                )));
            }
        };
        let deleted: i64 = match row.get_by_index(6) {
            Some(Value::I64(n)) => *n,
            other => {
                return Err(Error::Database(format!(
                    "record.deleted: expected I64, got {:?}",
                    other
                )));
            }
        };
        let source_code: i64 = match row.get_by_index(7) {
            Some(Value::I64(n)) => *n,
            other => {
                return Err(Error::Database(format!(
                    "record.source: expected I64, got {:?}",
                    other
                )));
            }
        };
        let sync_status_code: i64 = match row.get_by_index(8) {
            Some(Value::I64(n)) => *n,
            other => {
                return Err(Error::Database(format!(
                    "record.sync_status: expected I64, got {:?}",
                    other
                )));
            }
        };
        let tags_blob: Vec<u8> = match row.get_by_index(9) {
            Some(Value::Blob(b)) => b.clone(),
            other => {
                return Err(Error::Database(format!(
                    "record.tags: expected BLOB, got {:?}",
                    other
                )));
            }
        };
        let attributes_blob: Vec<u8> = match row.get_by_index(10) {
            Some(Value::Blob(b)) => b.clone(),
            other => {
                return Err(Error::Database(format!(
                    "record.attributes: expected BLOB, got {:?}",
                    other
                )));
            }
        };

        let id = RecordId::parse(&id_str).map_err(|e| Error::Database(e.to_string()))?;

        let created = Utc.timestamp_opt(created_at, 0).single().ok_or_else(|| {
            Error::Database(format!("invalid created_at timestamp: {created_at}"))
        })?;
        let updated = Utc.timestamp_opt(updated_at, 0).single().ok_or_else(|| {
            Error::Database(format!("invalid updated_at timestamp: {updated_at}"))
        })?;

        let metadata = crate::types::RecordMetadata {
            tags: decode_tags(&tags_blob)?,
            attributes: decode_attributes(&attributes_blob)?,
            source: crate::types::RecordSource::from_code(source_code),
            sync_status: crate::types::SyncStatus::from_code(sync_status_code),
        };

        Ok(Record {
            id,
            key,
            data: bytes::Bytes::from(data),
            version: crate::types::Version::from_u64(version as u64),
            created_at: created,
            updated_at: updated,
            deleted: deleted != 0,
            metadata,
        })
    }

    /// Map an OxiSQL `Row` to an [`Operation`].
    fn row_to_operation(row: &oxisql_core::Row) -> Result<Operation> {
        // Expected column order: id(0), operation_type(1), record_id(2),
        //   key(3), payload(4), base_version(5), target_version(6),
        //   created_at(7), retry_count(8), last_retry(9), priority(10)
        let id_str = match row.get_by_index(0) {
            Some(Value::Text(s)) => s.clone(),
            other => {
                return Err(Error::Database(format!(
                    "operation.id: expected TEXT, got {:?}",
                    other
                )));
            }
        };
        let op_type: i64 = match row.get_by_index(1) {
            Some(Value::I64(n)) => *n,
            other => {
                return Err(Error::Database(format!(
                    "operation.operation_type: expected I64, got {:?}",
                    other
                )));
            }
        };
        let record_id_str = match row.get_by_index(2) {
            Some(Value::Text(s)) => s.clone(),
            other => {
                return Err(Error::Database(format!(
                    "operation.record_id: expected TEXT, got {:?}",
                    other
                )));
            }
        };
        let key = match row.get_by_index(3) {
            Some(Value::Text(s)) => s.clone(),
            other => {
                return Err(Error::Database(format!(
                    "operation.key: expected TEXT, got {:?}",
                    other
                )));
            }
        };
        let payload: Vec<u8> = match row.get_by_index(4) {
            Some(Value::Blob(b)) => b.clone(),
            other => {
                return Err(Error::Database(format!(
                    "operation.payload: expected BLOB, got {:?}",
                    other
                )));
            }
        };
        let base_version: i64 = match row.get_by_index(5) {
            Some(Value::I64(n)) => *n,
            other => {
                return Err(Error::Database(format!(
                    "operation.base_version: expected I64, got {:?}",
                    other
                )));
            }
        };
        let target_version: i64 = match row.get_by_index(6) {
            Some(Value::I64(n)) => *n,
            other => {
                return Err(Error::Database(format!(
                    "operation.target_version: expected I64, got {:?}",
                    other
                )));
            }
        };
        let created_at_ts: i64 = match row.get_by_index(7) {
            Some(Value::I64(n)) => *n,
            other => {
                return Err(Error::Database(format!(
                    "operation.created_at: expected I64, got {:?}",
                    other
                )));
            }
        };
        let retry_count: i64 = match row.get_by_index(8) {
            Some(Value::I64(n)) => *n,
            other => {
                return Err(Error::Database(format!(
                    "operation.retry_count: expected I64, got {:?}",
                    other
                )));
            }
        };
        let last_retry_ts: Option<i64> = match row.get_by_index(9) {
            Some(Value::I64(n)) => Some(*n),
            Some(Value::Null) | None => None,
            other => {
                return Err(Error::Database(format!(
                    "operation.last_retry: expected I64 or NULL, got {:?}",
                    other
                )));
            }
        };
        let priority: i64 = match row.get_by_index(10) {
            Some(Value::I64(n)) => *n,
            other => {
                return Err(Error::Database(format!(
                    "operation.priority: expected I64, got {:?}",
                    other
                )));
            }
        };

        let id = OperationId::parse(&id_str).map_err(|e| Error::Database(e.to_string()))?;
        let record_id =
            RecordId::parse(&record_id_str).map_err(|e| Error::Database(e.to_string()))?;

        let operation_type = match op_type {
            0 => crate::types::OperationType::Insert,
            1 => crate::types::OperationType::Update,
            2 => crate::types::OperationType::Delete,
            other => {
                return Err(Error::Database(format!(
                    "unknown operation_type code: {other}"
                )));
            }
        };

        let created = Utc
            .timestamp_opt(created_at_ts, 0)
            .single()
            .ok_or_else(|| {
                Error::Database(format!("invalid created_at timestamp: {created_at_ts}"))
            })?;

        let last_retry = last_retry_ts.and_then(|ts| Utc.timestamp_opt(ts, 0).single());

        Ok(Operation {
            id,
            operation_type,
            record_id,
            key,
            payload: bytes::Bytes::from(payload),
            base_version: crate::types::Version::from_u64(base_version as u64),
            target_version: crate::types::Version::from_u64(target_version as u64),
            created_at: created,
            retry_count: retry_count as usize,
            last_retry,
            priority: priority as u8,
        })
    }
}

#[async_trait(?Send)]
impl StorageBackend for SqliteBackend {
    async fn initialize(&mut self) -> Result<()> {
        self.create_schema().await
    }

    async fn put_record(&mut self, record: &Record) -> Result<()> {
        let id_s = record.id.to_string();
        let key_s: &str = &record.key;
        let data_vec: Vec<u8> = record.data.to_vec();
        let version_i = record.version.value() as i64;
        let created_i = record.created_at.timestamp();
        let updated_i = record.updated_at.timestamp();
        let deleted_i: i64 = if record.deleted { 1 } else { 0 };
        let source_i: i64 = record.metadata.source.to_code();
        let sync_status_i: i64 = record.metadata.sync_status.to_code();
        let tags_blob = encode_tags(&record.metadata.tags);
        let attributes_blob = encode_attributes(&record.metadata.attributes);
        let id_ref: &str = &id_s;

        self.exec(
            "INSERT OR REPLACE INTO records
             (id, key, data, version, created_at, updated_at, deleted, source, sync_status,
              tags, attributes)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            &[
                &id_ref,
                &key_s,
                &data_vec,
                &version_i,
                &created_i,
                &updated_i,
                &deleted_i,
                &source_i,
                &sync_status_i,
                &tags_blob,
                &attributes_blob,
            ],
        )
        .await?;

        Ok(())
    }

    async fn get_record(&self, key: &str) -> Result<Option<Record>> {
        let rows = self
            .query(
                "SELECT id, key, data, version, created_at, updated_at, deleted,
                        source, sync_status, tags, attributes
             FROM records WHERE key = $1 AND deleted = 0",
                &[&key],
            )
            .await?;

        if rows.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self::row_to_record(&rows[0])?))
    }

    async fn get_record_by_id(&self, id: &RecordId) -> Result<Option<Record>> {
        let id_s = id.to_string();
        let id_ref: &str = &id_s;
        let rows = self
            .query(
                "SELECT id, key, data, version, created_at, updated_at, deleted,
                        source, sync_status, tags, attributes
             FROM records WHERE id = $1 AND deleted = 0",
                &[&id_ref],
            )
            .await?;

        if rows.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self::row_to_record(&rows[0])?))
    }

    async fn delete_record(&mut self, key: &str) -> Result<()> {
        let now_i = Utc::now().timestamp();
        self.exec(
            "UPDATE records SET deleted = 1, updated_at = $1 WHERE key = $2",
            &[&now_i, &key],
        )
        .await?;
        Ok(())
    }

    async fn list_records(&self) -> Result<Vec<Record>> {
        let rows = self
            .query(
                "SELECT id, key, data, version, created_at, updated_at, deleted,
                        source, sync_status, tags, attributes
             FROM records WHERE deleted = 0 ORDER BY updated_at DESC",
                &[],
            )
            .await?;

        rows.iter().map(Self::row_to_record).collect()
    }

    async fn count_records(&self) -> Result<usize> {
        let n = self
            .query_count("SELECT COUNT(*) FROM records WHERE deleted = 0", &[])
            .await?;
        Ok(n as usize)
    }

    async fn clear_records(&mut self) -> Result<()> {
        self.exec("DELETE FROM records", &[]).await?;
        Ok(())
    }

    async fn enqueue_operation(&mut self, operation: &Operation) -> Result<()> {
        let id_s = operation.id.to_string();
        let op_type_i: i64 = match operation.operation_type {
            crate::types::OperationType::Insert => 0,
            crate::types::OperationType::Update => 1,
            crate::types::OperationType::Delete => 2,
        };
        let record_id_s = operation.record_id.to_string();
        let key_s: &str = &operation.key;
        let payload_vec: Vec<u8> = operation.payload.to_vec();
        let base_v_i = operation.base_version.value() as i64;
        let target_v_i = operation.target_version.value() as i64;
        let created_i = operation.created_at.timestamp();
        let retry_i = operation.retry_count as i64;
        let last_retry_opt: Option<i64> = operation.last_retry.map(|dt| dt.timestamp());
        let priority_i = operation.priority as i64;
        let id_ref: &str = &id_s;
        let record_id_ref: &str = &record_id_s;

        // last_retry may be NULL; OxiSQL does not directly support Option<i64>
        // binding, so we use two code paths.
        if let Some(last_retry_ts) = last_retry_opt {
            self.exec(
                "INSERT OR REPLACE INTO operations
                 (id, operation_type, record_id, key, payload, base_version, target_version,
                  created_at, retry_count, last_retry, priority)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
                &[
                    &id_ref,
                    &op_type_i,
                    &record_id_ref,
                    &key_s,
                    &payload_vec,
                    &base_v_i,
                    &target_v_i,
                    &created_i,
                    &retry_i,
                    &last_retry_ts,
                    &priority_i,
                ],
            )
            .await?;
        } else {
            self.exec(
                "INSERT OR REPLACE INTO operations
                 (id, operation_type, record_id, key, payload, base_version, target_version,
                  created_at, retry_count, last_retry, priority)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, $10)",
                &[
                    &id_ref,
                    &op_type_i,
                    &record_id_ref,
                    &key_s,
                    &payload_vec,
                    &base_v_i,
                    &target_v_i,
                    &created_i,
                    &retry_i,
                    &priority_i,
                ],
            )
            .await?;
        }

        Ok(())
    }

    async fn get_pending_operations(&self, limit: usize) -> Result<Vec<Operation>> {
        // CRITICAL: parameterized LIMIT is not supported by the compat layer.
        // The limit value is a checked usize (not user-controlled SQL), so
        // formatting it directly into the SQL string is injection-safe.
        let sql = format!(
            "SELECT id, operation_type, record_id, key, payload, base_version,
                    target_version, created_at, retry_count, last_retry, priority
             FROM operations
             ORDER BY priority DESC, created_at ASC
             LIMIT {limit}"
        );

        let rows = self.query(&sql, &[]).await?;
        rows.iter().map(Self::row_to_operation).collect()
    }

    async fn dequeue_operation(&mut self, operation_id: &OperationId) -> Result<()> {
        let id_s = operation_id.to_string();
        let id_ref: &str = &id_s;
        self.exec("DELETE FROM operations WHERE id = $1", &[&id_ref])
            .await?;
        Ok(())
    }

    async fn update_operation(&mut self, operation: &Operation) -> Result<()> {
        let retry_i = operation.retry_count as i64;
        let id_s = operation.id.to_string();
        let id_ref: &str = &id_s;

        if let Some(last_retry) = operation.last_retry {
            let ts = last_retry.timestamp();
            self.exec(
                "UPDATE operations SET retry_count = $1, last_retry = $2 WHERE id = $3",
                &[&retry_i, &ts, &id_ref],
            )
            .await?;
        } else {
            self.exec(
                "UPDATE operations SET retry_count = $1, last_retry = NULL WHERE id = $2",
                &[&retry_i, &id_ref],
            )
            .await?;
        }
        Ok(())
    }

    async fn count_pending_operations(&self) -> Result<usize> {
        let n = self
            .query_count("SELECT COUNT(*) FROM operations", &[])
            .await?;
        Ok(n as usize)
    }

    async fn clear_operations(&mut self) -> Result<()> {
        self.exec("DELETE FROM operations", &[]).await?;
        Ok(())
    }

    async fn get_statistics(&self) -> Result<StorageStatistics> {
        let record_count = self
            .query_count("SELECT COUNT(*) FROM records WHERE deleted = 0", &[])
            .await? as usize;

        let record_size = {
            let rows = self
                .query(
                    "SELECT COALESCE(SUM(LENGTH(data)), 0) FROM records WHERE deleted = 0",
                    &[],
                )
                .await?;
            match rows.first().and_then(|r| r.get_by_index(0)) {
                Some(Value::I64(n)) => *n as u64,
                _ => 0u64,
            }
        };

        let pending_operations = self
            .query_count("SELECT COUNT(*) FROM operations", &[])
            .await? as usize;

        let operations_size = {
            let rows = self
                .query(
                    "SELECT COALESCE(SUM(LENGTH(payload)), 0) FROM operations",
                    &[],
                )
                .await?;
            match rows.first().and_then(|r| r.get_by_index(0)) {
                Some(Value::I64(n)) => *n as u64,
                _ => 0u64,
            }
        };

        let mut stats = StorageStatistics {
            record_count,
            record_size_bytes: record_size,
            pending_operations,
            operations_size_bytes: operations_size,
            backend_type: "SQLite (OxiSQL)".to_string(),
            custom_metrics: Vec::new(),
        };

        // PRAGMA page_count / page_size are not yet reliably supported by the
        // OxiSQL engine; we report them as "0" (cosmetic omission only).
        stats.add_metric("page_count".to_string(), "0".to_string());
        stats.add_metric("page_size".to_string(), "0".to_string());
        stats.add_metric("database_size".to_string(), "0".to_string());

        Ok(stats)
    }

    async fn compact(&mut self) -> Result<()> {
        // VACUUM is not yet supported by the OxiSQL engine; this is a no-op.
        // The WAL is checkpointed automatically by the engine on close.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[tokio::test]
    async fn test_sqlite_backend() {
        let mut backend = SqliteBackend::in_memory()
            .await
            .expect("failed to create backend");
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
        let mut backend = SqliteBackend::in_memory()
            .await
            .expect("failed to create backend");
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

    /// Regression test: full `RecordMetadata` (tags, attributes, source,
    /// sync_status) must survive a put/get round trip through
    /// `SqliteBackend`, not silently reset to defaults.
    #[tokio::test]
    async fn test_put_get_record_preserves_full_metadata() {
        let mut backend = SqliteBackend::in_memory()
            .await
            .expect("failed to create backend");
        backend.initialize().await.expect("failed to initialize");

        let mut record = Record::new("meta_key".to_string(), Bytes::from("meta_data"));
        record.metadata.tags = vec!["important".to_string(), "geo:tokyo".to_string()];
        record.metadata.attributes = vec![
            ("owner".to_string(), "kitasan".to_string()),
            ("region".to_string(), "ap-northeast-1".to_string()),
        ];
        record.metadata.source = crate::types::RecordSource::Remote;
        record.metadata.sync_status = crate::types::SyncStatus::Synced;

        backend
            .put_record(&record)
            .await
            .expect("failed to put record");

        let retrieved = backend
            .get_record("meta_key")
            .await
            .expect("failed to get record")
            .expect("record must exist");

        assert_eq!(retrieved.metadata.tags, record.metadata.tags);
        assert_eq!(retrieved.metadata.attributes, record.metadata.attributes);
        assert_eq!(
            retrieved.metadata.source,
            crate::types::RecordSource::Remote
        );
        assert_eq!(
            retrieved.metadata.sync_status,
            crate::types::SyncStatus::Synced
        );

        // Also verify via `get_record_by_id` and `list_records`, which share
        // the same row-mapping path.
        let by_id = backend
            .get_record_by_id(&record.id)
            .await
            .expect("failed to get by id")
            .expect("record must exist");
        assert_eq!(by_id.metadata.tags, record.metadata.tags);
        assert_eq!(by_id.metadata.sync_status, crate::types::SyncStatus::Synced);

        let all = backend.list_records().await.expect("failed to list");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].metadata.attributes, record.metadata.attributes);
    }

    /// Tags/attributes containing characters that could be mistaken for a
    /// naive separator (colons, commas, empty strings) must still round-trip
    /// exactly through the length-prefixed binary encoding.
    #[tokio::test]
    async fn test_put_get_record_metadata_with_tricky_characters() {
        let mut backend = SqliteBackend::in_memory()
            .await
            .expect("failed to create backend");
        backend.initialize().await.expect("failed to initialize");

        let mut record = Record::new("tricky_key".to_string(), Bytes::from("data"));
        record.metadata.tags = vec![
            String::new(),
            "a,b:c=d".to_string(),
            "emoji-\u{1F600}".to_string(),
        ];
        record.metadata.attributes = vec![
            ("".to_string(), "".to_string()),
            (
                "key:with:colons".to_string(),
                "value=with=equals".to_string(),
            ),
        ];

        backend
            .put_record(&record)
            .await
            .expect("failed to put record");

        let retrieved = backend
            .get_record("tricky_key")
            .await
            .expect("failed to get record")
            .expect("record must exist");

        assert_eq!(retrieved.metadata.tags, record.metadata.tags);
        assert_eq!(retrieved.metadata.attributes, record.metadata.attributes);
    }
}
