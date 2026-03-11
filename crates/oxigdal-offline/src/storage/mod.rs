//! Storage backends for offline data

use crate::error::Result;
use crate::types::{Operation, Record, RecordId};
use async_trait::async_trait;

#[cfg(feature = "native")]
pub mod sqlite;

#[cfg(all(feature = "wasm", not(feature = "native")))]
pub mod indexeddb;

/// Trait for storage backends
///
/// Note: Send + Sync bounds are only required for native platforms.
/// WASM platforms are single-threaded and don't need these bounds.
#[async_trait(?Send)]
#[cfg(all(feature = "native", not(feature = "wasm")))]
pub trait StorageBackend: Send + Sync {
    /// Initialize the storage backend
    async fn initialize(&mut self) -> Result<()>;

    /// Store a record
    async fn put_record(&mut self, record: &Record) -> Result<()>;

    /// Retrieve a record by key
    async fn get_record(&self, key: &str) -> Result<Option<Record>>;

    /// Retrieve a record by ID
    async fn get_record_by_id(&self, id: &RecordId) -> Result<Option<Record>>;

    /// Delete a record
    async fn delete_record(&mut self, key: &str) -> Result<()>;

    /// List all records
    async fn list_records(&self) -> Result<Vec<Record>>;

    /// Count total records
    async fn count_records(&self) -> Result<usize>;

    /// Clear all records
    async fn clear_records(&mut self) -> Result<()>;

    /// Store an operation in the sync queue
    async fn enqueue_operation(&mut self, operation: &Operation) -> Result<()>;

    /// Retrieve pending operations
    async fn get_pending_operations(&self, limit: usize) -> Result<Vec<Operation>>;

    /// Remove an operation from the queue
    async fn dequeue_operation(&mut self, operation_id: &crate::types::OperationId) -> Result<()>;

    /// Update an operation (e.g., increment retry count)
    async fn update_operation(&mut self, operation: &Operation) -> Result<()>;

    /// Count pending operations
    async fn count_pending_operations(&self) -> Result<usize>;

    /// Clear all operations
    async fn clear_operations(&mut self) -> Result<()>;

    /// Get storage statistics
    async fn get_statistics(&self) -> Result<StorageStatistics>;

    /// Compact/optimize storage
    async fn compact(&mut self) -> Result<()>;
}

/// Trait for storage backends (WASM version without Send + Sync)
/// Only used when wasm feature is enabled but native is not
#[async_trait(?Send)]
#[cfg(all(feature = "wasm", not(feature = "native")))]
pub trait StorageBackend {
    /// Initialize the storage backend
    async fn initialize(&mut self) -> Result<()>;

    /// Store a record
    async fn put_record(&mut self, record: &Record) -> Result<()>;

    /// Retrieve a record by key
    async fn get_record(&self, key: &str) -> Result<Option<Record>>;

    /// Retrieve a record by ID
    async fn get_record_by_id(&self, id: &RecordId) -> Result<Option<Record>>;

    /// Delete a record
    async fn delete_record(&mut self, key: &str) -> Result<()>;

    /// List all records
    async fn list_records(&self) -> Result<Vec<Record>>;

    /// Count total records
    async fn count_records(&self) -> Result<usize>;

    /// Clear all records
    async fn clear_records(&mut self) -> Result<()>;

    /// Store an operation in the sync queue
    async fn enqueue_operation(&mut self, operation: &Operation) -> Result<()>;

    /// Retrieve pending operations
    async fn get_pending_operations(&self, limit: usize) -> Result<Vec<Operation>>;

    /// Remove an operation from the queue
    async fn dequeue_operation(&mut self, operation_id: &crate::types::OperationId) -> Result<()>;

    /// Update an operation (e.g., increment retry count)
    async fn update_operation(&mut self, operation: &Operation) -> Result<()>;

    /// Count pending operations
    async fn count_pending_operations(&self) -> Result<usize>;

    /// Clear all operations
    async fn clear_operations(&mut self) -> Result<()>;

    /// Get storage statistics
    async fn get_statistics(&self) -> Result<StorageStatistics>;

    /// Compact/optimize storage
    async fn compact(&mut self) -> Result<()>;
}

/// Trait for storage backends (both features enabled - use native with Send + Sync)
/// When both wasm and native features are enabled, prefer native threading model
#[async_trait(?Send)]
#[cfg(all(feature = "wasm", feature = "native"))]
pub trait StorageBackend: Send + Sync {
    /// Initialize the storage backend
    async fn initialize(&mut self) -> Result<()>;

    /// Store a record
    async fn put_record(&mut self, record: &Record) -> Result<()>;

    /// Retrieve a record by key
    async fn get_record(&self, key: &str) -> Result<Option<Record>>;

    /// Retrieve a record by ID
    async fn get_record_by_id(&self, id: &RecordId) -> Result<Option<Record>>;

    /// Delete a record
    async fn delete_record(&mut self, key: &str) -> Result<()>;

    /// List all records
    async fn list_records(&self) -> Result<Vec<Record>>;

    /// Count total records
    async fn count_records(&self) -> Result<usize>;

    /// Clear all records
    async fn clear_records(&mut self) -> Result<()>;

    /// Store an operation in the sync queue
    async fn enqueue_operation(&mut self, operation: &Operation) -> Result<()>;

    /// Retrieve pending operations
    async fn get_pending_operations(&self, limit: usize) -> Result<Vec<Operation>>;

    /// Remove an operation from the queue
    async fn dequeue_operation(&mut self, operation_id: &crate::types::OperationId) -> Result<()>;

    /// Update an operation (e.g., increment retry count)
    async fn update_operation(&mut self, operation: &Operation) -> Result<()>;

    /// Count pending operations
    async fn count_pending_operations(&self) -> Result<usize>;

    /// Clear all operations
    async fn clear_operations(&mut self) -> Result<()>;

    /// Get storage statistics
    async fn get_statistics(&self) -> Result<StorageStatistics>;

    /// Compact/optimize storage
    async fn compact(&mut self) -> Result<()>;
}

/// Storage statistics
#[derive(Debug, Clone)]
pub struct StorageStatistics {
    /// Total number of records
    pub record_count: usize,
    /// Total size of records (bytes)
    pub record_size_bytes: u64,
    /// Number of pending operations
    pub pending_operations: usize,
    /// Total size of operations (bytes)
    pub operations_size_bytes: u64,
    /// Storage backend type
    pub backend_type: String,
    /// Additional backend-specific metrics
    pub custom_metrics: Vec<(String, String)>,
}

impl StorageStatistics {
    /// Create empty statistics
    pub fn empty(backend_type: String) -> Self {
        Self {
            record_count: 0,
            record_size_bytes: 0,
            pending_operations: 0,
            operations_size_bytes: 0,
            backend_type,
            custom_metrics: Vec::new(),
        }
    }

    /// Add a custom metric
    pub fn add_metric(&mut self, key: String, value: String) {
        self.custom_metrics.push((key, value));
    }
}

/// Compression utilities for storage
pub mod compression {
    use crate::error::{Error, Result};
    use bytes::Bytes;

    /// Compress data if it exceeds threshold
    pub fn compress_if_needed(data: &[u8], threshold: usize) -> Result<(Bytes, bool)> {
        if data.len() < threshold {
            return Ok((Bytes::copy_from_slice(data), false));
        }

        // Use DEFLATE compression
        let compressed =
            oxiarc_deflate::deflate(data, 6).map_err(|e| Error::storage(e.to_string()))?;

        // Only use compression if it actually reduces size
        if compressed.len() < data.len() {
            Ok((Bytes::from(compressed), true))
        } else {
            Ok((Bytes::copy_from_slice(data), false))
        }
    }

    /// Decompress data if needed
    pub fn decompress_if_needed(data: &[u8], compressed: bool) -> Result<Bytes> {
        if !compressed {
            return Ok(Bytes::copy_from_slice(data));
        }

        // Decompress using DEFLATE
        oxiarc_deflate::inflate(data)
            .map(Bytes::from)
            .map_err(|e| Error::storage(format!("Decompression failed: {}", e)))
    }
}

/// Serialization utilities for storage
pub mod serialization {
    use crate::error::{Error, Result};
    use crate::types::{
        Operation, OperationId, OperationType, Record, RecordId, RecordMetadata, RecordSource,
        SyncStatus, Version,
    };
    use bytes::Bytes;
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    /// Format version for serialization
    /// Increment this when making breaking changes to the format
    const FORMAT_VERSION: u8 = 1;

    /// Minimum header size for version detection
    const MIN_HEADER_SIZE: usize = 1;

    /// Helper to read a u32 length from a slice
    fn read_u32_len(data: &[u8], offset: &mut usize) -> Result<usize> {
        if *offset + 4 > data.len() {
            return Err(Error::deserialization(
                "unexpected end of data while reading length",
            ));
        }
        let bytes: [u8; 4] = data[*offset..*offset + 4]
            .try_into()
            .map_err(|_| Error::deserialization("failed to read u32 length"))?;
        *offset += 4;
        Ok(u32::from_le_bytes(bytes) as usize)
    }

    /// Helper to read a u64 from a slice
    fn read_u64(data: &[u8], offset: &mut usize) -> Result<u64> {
        if *offset + 8 > data.len() {
            return Err(Error::deserialization(
                "unexpected end of data while reading u64",
            ));
        }
        let bytes: [u8; 8] = data[*offset..*offset + 8]
            .try_into()
            .map_err(|_| Error::deserialization("failed to read u64"))?;
        *offset += 8;
        Ok(u64::from_le_bytes(bytes))
    }

    /// Helper to read an i64 from a slice
    fn read_i64(data: &[u8], offset: &mut usize) -> Result<i64> {
        if *offset + 8 > data.len() {
            return Err(Error::deserialization(
                "unexpected end of data while reading i64",
            ));
        }
        let bytes: [u8; 8] = data[*offset..*offset + 8]
            .try_into()
            .map_err(|_| Error::deserialization("failed to read i64"))?;
        *offset += 8;
        Ok(i64::from_le_bytes(bytes))
    }

    /// Helper to read a UUID from a slice
    fn read_uuid(data: &[u8], offset: &mut usize) -> Result<Uuid> {
        if *offset + 16 > data.len() {
            return Err(Error::deserialization(
                "unexpected end of data while reading UUID",
            ));
        }
        let bytes: [u8; 16] = data[*offset..*offset + 16]
            .try_into()
            .map_err(|_| Error::deserialization("failed to read UUID bytes"))?;
        *offset += 16;
        Ok(Uuid::from_bytes(bytes))
    }

    /// Helper to read a length-prefixed string from a slice
    fn read_string(data: &[u8], offset: &mut usize) -> Result<String> {
        let len = read_u32_len(data, offset)?;

        // Validate string length to prevent DoS attacks
        const MAX_STRING_LEN: usize = 1024 * 1024; // 1MB max
        if len > MAX_STRING_LEN {
            return Err(Error::deserialization(format!(
                "string length {} exceeds maximum {}",
                len, MAX_STRING_LEN
            )));
        }

        if *offset + len > data.len() {
            return Err(Error::deserialization(format!(
                "unexpected end of data while reading string: need {} bytes at offset {}, have {}",
                len,
                *offset,
                data.len()
            )));
        }
        let string_bytes = &data[*offset..*offset + len];
        *offset += len;
        String::from_utf8(string_bytes.to_vec())
            .map_err(|e| Error::deserialization(format!("invalid UTF-8 string: {}", e)))
    }

    /// Helper to read length-prefixed bytes from a slice
    fn read_bytes(data: &[u8], offset: &mut usize) -> Result<Bytes> {
        let len = read_u32_len(data, offset)?;

        // Validate data length to prevent DoS attacks
        const MAX_DATA_LEN: usize = 100 * 1024 * 1024; // 100MB max
        if len > MAX_DATA_LEN {
            return Err(Error::deserialization(format!(
                "data length {} exceeds maximum {}",
                len, MAX_DATA_LEN
            )));
        }

        if *offset + len > data.len() {
            return Err(Error::deserialization(format!(
                "unexpected end of data while reading bytes: need {} bytes at offset {}, have {}",
                len,
                *offset,
                data.len()
            )));
        }
        let bytes = Bytes::copy_from_slice(&data[*offset..*offset + len]);
        *offset += len;
        Ok(bytes)
    }

    /// Helper to read a u8 from a slice
    fn read_u8(data: &[u8], offset: &mut usize) -> Result<u8> {
        if *offset >= data.len() {
            return Err(Error::deserialization(
                "unexpected end of data while reading u8",
            ));
        }
        let value = data[*offset];
        *offset += 1;
        Ok(value)
    }

    /// Helper to read a u32 from a slice
    fn read_u32(data: &[u8], offset: &mut usize) -> Result<u32> {
        if *offset + 4 > data.len() {
            return Err(Error::deserialization(
                "unexpected end of data while reading u32",
            ));
        }
        let bytes: [u8; 4] = data[*offset..*offset + 4]
            .try_into()
            .map_err(|_| Error::deserialization("failed to read u32"))?;
        *offset += 4;
        Ok(u32::from_le_bytes(bytes))
    }

    /// Serialize a record to bytes with versioning support
    pub fn serialize_record(record: &Record) -> Result<Bytes> {
        let mut buf = Vec::new();

        // Write format version (enables future format changes)
        buf.push(FORMAT_VERSION);

        // Serialize ID
        buf.extend_from_slice(record.id.as_uuid().as_bytes());

        // Serialize key (length-prefixed)
        let key_bytes = record.key.as_bytes();
        buf.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(key_bytes);

        // Serialize data (length-prefixed)
        buf.extend_from_slice(&(record.data.len() as u32).to_le_bytes());
        buf.extend_from_slice(&record.data);

        // Serialize version
        buf.extend_from_slice(&record.version.value().to_le_bytes());

        // Serialize timestamps
        buf.extend_from_slice(&record.created_at.timestamp().to_le_bytes());
        buf.extend_from_slice(&record.updated_at.timestamp().to_le_bytes());

        // Serialize deleted flag
        buf.push(if record.deleted { 1 } else { 0 });

        // Serialize metadata (v1 format)
        serialize_metadata(&mut buf, &record.metadata)?;

        Ok(Bytes::from(buf))
    }

    /// Serialize record metadata to bytes
    fn serialize_metadata(buf: &mut Vec<u8>, metadata: &RecordMetadata) -> Result<()> {
        // Serialize tags (length-prefixed array of strings)
        buf.extend_from_slice(&(metadata.tags.len() as u32).to_le_bytes());
        for tag in &metadata.tags {
            let tag_bytes = tag.as_bytes();
            buf.extend_from_slice(&(tag_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(tag_bytes);
        }

        // Serialize attributes (length-prefixed array of key-value pairs)
        buf.extend_from_slice(&(metadata.attributes.len() as u32).to_le_bytes());
        for (key, value) in &metadata.attributes {
            let key_bytes = key.as_bytes();
            buf.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(key_bytes);
            let value_bytes = value.as_bytes();
            buf.extend_from_slice(&(value_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(value_bytes);
        }

        // Serialize source
        let source_byte = match metadata.source {
            RecordSource::Local => 0u8,
            RecordSource::Remote => 1u8,
            RecordSource::Merged => 2u8,
        };
        buf.push(source_byte);

        // Serialize sync status
        let status_byte = match metadata.sync_status {
            SyncStatus::Pending => 0u8,
            SyncStatus::Syncing => 1u8,
            SyncStatus::Synced => 2u8,
            SyncStatus::Failed => 3u8,
            SyncStatus::Conflict => 4u8,
        };
        buf.push(status_byte);

        Ok(())
    }

    /// Deserialize record metadata from bytes
    fn deserialize_metadata(data: &[u8], offset: &mut usize) -> Result<RecordMetadata> {
        // Read tags
        let tags_len = read_u32_len(data, offset)?;

        // Validate tags count to prevent DoS
        const MAX_TAGS: usize = 10000;
        if tags_len > MAX_TAGS {
            return Err(Error::deserialization(format!(
                "tags count {} exceeds maximum {}",
                tags_len, MAX_TAGS
            )));
        }

        let mut tags = Vec::with_capacity(tags_len);
        for _ in 0..tags_len {
            tags.push(read_string(data, offset)?);
        }

        // Read attributes
        let attrs_len = read_u32_len(data, offset)?;

        // Validate attributes count
        const MAX_ATTRS: usize = 10000;
        if attrs_len > MAX_ATTRS {
            return Err(Error::deserialization(format!(
                "attributes count {} exceeds maximum {}",
                attrs_len, MAX_ATTRS
            )));
        }

        let mut attributes = Vec::with_capacity(attrs_len);
        for _ in 0..attrs_len {
            let key = read_string(data, offset)?;
            let value = read_string(data, offset)?;
            attributes.push((key, value));
        }

        // Read source
        let source_byte = read_u8(data, offset)?;
        let source = match source_byte {
            0 => RecordSource::Local,
            1 => RecordSource::Remote,
            2 => RecordSource::Merged,
            v => {
                return Err(Error::deserialization(format!(
                    "invalid RecordSource value: {}",
                    v
                )));
            }
        };

        // Read sync status
        let status_byte = read_u8(data, offset)?;
        let sync_status = match status_byte {
            0 => SyncStatus::Pending,
            1 => SyncStatus::Syncing,
            2 => SyncStatus::Synced,
            3 => SyncStatus::Failed,
            4 => SyncStatus::Conflict,
            v => {
                return Err(Error::deserialization(format!(
                    "invalid SyncStatus value: {}",
                    v
                )));
            }
        };

        Ok(RecordMetadata {
            tags,
            attributes,
            source,
            sync_status,
        })
    }

    /// Deserialize a record from bytes with version handling
    pub fn deserialize_record(data: &[u8]) -> Result<Record> {
        // Check minimum size for version detection
        if data.len() < MIN_HEADER_SIZE {
            return Err(Error::deserialization(
                "data too short: missing version header",
            ));
        }

        let mut offset = 0;

        // Read format version
        let version = read_u8(data, &mut offset)?;

        match version {
            0 => deserialize_record_v0(data, &mut offset),
            1 => deserialize_record_v1(data, &mut offset),
            v => Err(Error::deserialization(format!(
                "unsupported record format version: {} (current: {})",
                v, FORMAT_VERSION
            ))),
        }
    }

    /// Deserialize record in legacy v0 format (without version header and metadata)
    /// This handles data serialized before version headers were added
    fn deserialize_record_v0(data: &[u8], offset: &mut usize) -> Result<Record> {
        // V0 format starts directly with UUID (no version byte was written)
        // We need to reset offset since v0 didn't have a version byte
        *offset = 0;

        // Minimum size for v0: UUID(16) + key_len(4) + data_len(4) + version(8) +
        // created_at(8) + updated_at(8) + deleted(1) = 49 bytes
        if data.len() < 49 {
            return Err(Error::deserialization(format!(
                "data too short for v0 record: {} < 49 bytes",
                data.len()
            )));
        }

        // Read UUID
        let uuid = read_uuid(data, offset)?;
        let id = RecordId::from_uuid(uuid);

        // Read key
        let key = read_string(data, offset)?;

        // Read data
        let record_data = read_bytes(data, offset)?;

        // Read version
        let version_value = read_u64(data, offset)?;
        let version = Version::from_u64(version_value);

        // Read timestamps
        let created_ts = read_i64(data, offset)?;
        let updated_ts = read_i64(data, offset)?;

        // Convert timestamps safely
        let created_at = Utc.timestamp_opt(created_ts, 0).single().ok_or_else(|| {
            Error::deserialization(format!("invalid created_at timestamp: {}", created_ts))
        })?;
        let updated_at = Utc.timestamp_opt(updated_ts, 0).single().ok_or_else(|| {
            Error::deserialization(format!("invalid updated_at timestamp: {}", updated_ts))
        })?;

        // Read deleted flag
        let deleted_byte = read_u8(data, offset)?;
        let deleted = match deleted_byte {
            0 => false,
            1 => true,
            v => {
                return Err(Error::deserialization(format!(
                    "invalid deleted flag: {}",
                    v
                )));
            }
        };

        // V0 doesn't have metadata, use defaults
        let metadata = RecordMetadata::default();

        Ok(Record {
            id,
            key,
            data: record_data,
            version,
            created_at,
            updated_at,
            deleted,
            metadata,
        })
    }

    /// Deserialize record in v1 format (with metadata)
    fn deserialize_record_v1(data: &[u8], offset: &mut usize) -> Result<Record> {
        // Minimum size for v1: version(1) + UUID(16) + key_len(4) + data_len(4) +
        // version(8) + created_at(8) + updated_at(8) + deleted(1) +
        // tags_len(4) + attrs_len(4) + source(1) + sync_status(1) = 60 bytes
        if data.len() < 60 {
            return Err(Error::deserialization(format!(
                "data too short for v1 record: {} < 60 bytes",
                data.len()
            )));
        }

        // Read UUID
        let uuid = read_uuid(data, offset)?;
        let id = RecordId::from_uuid(uuid);

        // Read key
        let key = read_string(data, offset)?;

        // Read data
        let record_data = read_bytes(data, offset)?;

        // Read version
        let version_value = read_u64(data, offset)?;
        let version = Version::from_u64(version_value);

        // Read timestamps
        let created_ts = read_i64(data, offset)?;
        let updated_ts = read_i64(data, offset)?;

        // Convert timestamps safely
        let created_at = Utc.timestamp_opt(created_ts, 0).single().ok_or_else(|| {
            Error::deserialization(format!("invalid created_at timestamp: {}", created_ts))
        })?;
        let updated_at = Utc.timestamp_opt(updated_ts, 0).single().ok_or_else(|| {
            Error::deserialization(format!("invalid updated_at timestamp: {}", updated_ts))
        })?;

        // Validate timestamp consistency
        if updated_at < created_at {
            return Err(Error::deserialization(
                "updated_at timestamp is before created_at",
            ));
        }

        // Read deleted flag
        let deleted_byte = read_u8(data, offset)?;
        let deleted = match deleted_byte {
            0 => false,
            1 => true,
            v => {
                return Err(Error::deserialization(format!(
                    "invalid deleted flag: {}",
                    v
                )));
            }
        };

        // Read metadata
        let metadata = deserialize_metadata(data, offset)?;

        Ok(Record {
            id,
            key,
            data: record_data,
            version,
            created_at,
            updated_at,
            deleted,
            metadata,
        })
    }

    /// Serialize an operation to bytes with versioning support
    pub fn serialize_operation(operation: &Operation) -> Result<Bytes> {
        let mut buf = Vec::new();

        // Write format version
        buf.push(FORMAT_VERSION);

        // Serialize ID
        buf.extend_from_slice(operation.id.as_uuid().as_bytes());

        // Serialize operation type
        let op_type = match operation.operation_type {
            OperationType::Insert => 0u8,
            OperationType::Update => 1u8,
            OperationType::Delete => 2u8,
        };
        buf.push(op_type);

        // Serialize record ID
        buf.extend_from_slice(operation.record_id.as_uuid().as_bytes());

        // Serialize key (length-prefixed)
        let key_bytes = operation.key.as_bytes();
        buf.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(key_bytes);

        // Serialize payload (length-prefixed)
        buf.extend_from_slice(&(operation.payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&operation.payload);

        // Serialize versions
        buf.extend_from_slice(&operation.base_version.value().to_le_bytes());
        buf.extend_from_slice(&operation.target_version.value().to_le_bytes());

        // Serialize timestamp
        buf.extend_from_slice(&operation.created_at.timestamp().to_le_bytes());

        // Serialize retry count
        buf.extend_from_slice(&(operation.retry_count as u32).to_le_bytes());

        // Serialize priority
        buf.push(operation.priority);

        // Serialize last_retry (Option<DateTime<Utc>>)
        match &operation.last_retry {
            Some(dt) => {
                buf.push(1); // Some indicator
                buf.extend_from_slice(&dt.timestamp().to_le_bytes());
            }
            None => {
                buf.push(0); // None indicator
            }
        }

        Ok(Bytes::from(buf))
    }

    /// Deserialize an operation from bytes with version handling
    pub fn deserialize_operation(data: &[u8]) -> Result<Operation> {
        // Check minimum size for version detection
        if data.len() < MIN_HEADER_SIZE {
            return Err(Error::deserialization(
                "data too short: missing version header",
            ));
        }

        let mut offset = 0;

        // Read format version
        let version = read_u8(data, &mut offset)?;

        match version {
            0 => deserialize_operation_v0(data, &mut offset),
            1 => deserialize_operation_v1(data, &mut offset),
            v => Err(Error::deserialization(format!(
                "unsupported operation format version: {} (current: {})",
                v, FORMAT_VERSION
            ))),
        }
    }

    /// Deserialize operation in legacy v0 format
    fn deserialize_operation_v0(data: &[u8], offset: &mut usize) -> Result<Operation> {
        // V0 format didn't have version byte, reset offset
        *offset = 0;

        // Minimum size for v0 operation
        if data.len() < 66 {
            return Err(Error::deserialization(format!(
                "data too short for v0 operation: {} < 66 bytes",
                data.len()
            )));
        }

        // Read operation ID
        let uuid = read_uuid(data, offset)?;
        let id = OperationId::from_uuid(uuid);

        // Read operation type
        let op_type_byte = read_u8(data, offset)?;
        let operation_type = match op_type_byte {
            0 => OperationType::Insert,
            1 => OperationType::Update,
            2 => OperationType::Delete,
            v => {
                return Err(Error::deserialization(format!(
                    "invalid operation type: {}",
                    v
                )));
            }
        };

        // Read record ID
        let record_uuid = read_uuid(data, offset)?;
        let record_id = RecordId::from_uuid(record_uuid);

        // Read key
        let key = read_string(data, offset)?;

        // Read payload
        let payload = read_bytes(data, offset)?;

        // Read versions
        let base_version_value = read_u64(data, offset)?;
        let base_version = Version::from_u64(base_version_value);

        let target_version_value = read_u64(data, offset)?;
        let target_version = Version::from_u64(target_version_value);

        // Read created_at timestamp
        let created_ts = read_i64(data, offset)?;
        let created_at = Utc.timestamp_opt(created_ts, 0).single().ok_or_else(|| {
            Error::deserialization(format!("invalid created_at timestamp: {}", created_ts))
        })?;

        // Read retry count
        let retry_count = read_u32(data, offset)? as usize;

        // Read priority
        let priority = read_u8(data, offset)?;

        // V0 doesn't have last_retry
        let last_retry = None;

        Ok(Operation {
            id,
            operation_type,
            record_id,
            key,
            payload,
            base_version,
            target_version,
            created_at,
            retry_count,
            last_retry,
            priority,
        })
    }

    /// Deserialize operation in v1 format (with last_retry)
    fn deserialize_operation_v1(data: &[u8], offset: &mut usize) -> Result<Operation> {
        // Minimum size for v1 operation
        if data.len() < 67 {
            return Err(Error::deserialization(format!(
                "data too short for v1 operation: {} < 67 bytes",
                data.len()
            )));
        }

        // Read operation ID
        let uuid = read_uuid(data, offset)?;
        let id = OperationId::from_uuid(uuid);

        // Read operation type
        let op_type_byte = read_u8(data, offset)?;
        let operation_type = match op_type_byte {
            0 => OperationType::Insert,
            1 => OperationType::Update,
            2 => OperationType::Delete,
            v => {
                return Err(Error::deserialization(format!(
                    "invalid operation type: {}",
                    v
                )));
            }
        };

        // Read record ID
        let record_uuid = read_uuid(data, offset)?;
        let record_id = RecordId::from_uuid(record_uuid);

        // Read key
        let key = read_string(data, offset)?;

        // Validate key is not empty for insert/update operations
        if key.is_empty() && operation_type != OperationType::Delete {
            return Err(Error::deserialization(
                "key cannot be empty for insert/update operations",
            ));
        }

        // Read payload
        let payload = read_bytes(data, offset)?;

        // Read versions
        let base_version_value = read_u64(data, offset)?;
        let base_version = Version::from_u64(base_version_value);

        let target_version_value = read_u64(data, offset)?;
        let target_version = Version::from_u64(target_version_value);

        // Validate version consistency
        if operation_type == OperationType::Insert && base_version_value != 0 {
            return Err(Error::deserialization(
                "insert operation should have base_version 0",
            ));
        }

        // Read created_at timestamp
        let created_ts = read_i64(data, offset)?;
        let created_at = Utc.timestamp_opt(created_ts, 0).single().ok_or_else(|| {
            Error::deserialization(format!("invalid created_at timestamp: {}", created_ts))
        })?;

        // Read retry count
        let retry_count = read_u32(data, offset)? as usize;

        // Read priority
        let priority = read_u8(data, offset)?;

        // Read last_retry option
        let has_last_retry = read_u8(data, offset)?;
        let last_retry = match has_last_retry {
            0 => None,
            1 => {
                let last_retry_ts = read_i64(data, offset)?;
                Some(
                    Utc.timestamp_opt(last_retry_ts, 0)
                        .single()
                        .ok_or_else(|| {
                            Error::deserialization(format!(
                                "invalid last_retry timestamp: {}",
                                last_retry_ts
                            ))
                        })?,
                )
            }
            v => {
                return Err(Error::deserialization(format!(
                    "invalid last_retry indicator: {}",
                    v
                )));
            }
        };

        // Validate last_retry consistency with retry_count
        if retry_count > 0 && last_retry.is_none() {
            // This is a warning condition but not an error - could be v0 migrated data
            tracing::debug!(
                "operation has retry_count {} but no last_retry timestamp",
                retry_count
            );
        }

        Ok(Operation {
            id,
            operation_type,
            record_id,
            key,
            payload,
            base_version,
            target_version,
            created_at,
            retry_count,
            last_retry,
            priority,
        })
    }

    /// Detect the format version from serialized data
    pub fn detect_format_version(data: &[u8]) -> Result<u8> {
        if data.is_empty() {
            return Err(Error::deserialization("empty data"));
        }

        let first_byte = data[0];

        // V0 format started with UUID (first byte is random)
        // V1+ format starts with version number (0-255)
        // We use heuristics: if first byte is 0 or 1, it's likely a version number
        // If it's a higher value, it's likely UUID's first byte from v0
        // This is imperfect but provides reasonable backwards compatibility
        if first_byte <= FORMAT_VERSION {
            Ok(first_byte)
        } else {
            // Assume v0 (legacy format without version header)
            Ok(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_statistics() {
        let mut stats = StorageStatistics::empty("test".to_string());
        assert_eq!(stats.record_count, 0);
        assert_eq!(stats.backend_type, "test");

        stats.add_metric("test_key".to_string(), "test_value".to_string());
        assert_eq!(stats.custom_metrics.len(), 1);
    }

    #[test]
    fn test_compression_threshold() {
        let data = b"small";
        let result = compression::compress_if_needed(data, 1024);
        assert!(result.is_ok());
        let (compressed, was_compressed) = result.expect("failed");
        assert!(!was_compressed);
        assert_eq!(compressed.as_ref(), data);
    }

    #[test]
    fn test_compression_with_large_data() {
        // Create compressible data
        let data = "Hello world! ".repeat(1000);
        let data_bytes = data.as_bytes();

        let result = compression::compress_if_needed(data_bytes, 100);
        assert!(result.is_ok());
        let (compressed, was_compressed) = result.expect("failed");
        assert!(was_compressed);
        // Compressed size should be significantly smaller
        assert!(compressed.len() < data_bytes.len());
    }

    #[test]
    fn test_compression_round_trip() {
        // Create test data
        let original = "This is test data that will be compressed! ".repeat(100);
        let original_bytes = original.as_bytes();

        // Compress
        let (compressed, was_compressed) =
            compression::compress_if_needed(original_bytes, 100).expect("compression failed");
        assert!(was_compressed);

        // Decompress
        let decompressed =
            compression::decompress_if_needed(&compressed, true).expect("decompression failed");

        // Should match original
        assert_eq!(decompressed.as_ref(), original_bytes);
    }

    #[test]
    fn test_decompression_uncompressed_data() {
        let data = b"uncompressed data";
        let result = compression::decompress_if_needed(data, false);
        assert!(result.is_ok());
        let decompressed = result.expect("failed");
        assert_eq!(decompressed.as_ref(), data);
    }

    #[test]
    fn test_compression_random_data() {
        // Random data typically doesn't compress well
        let data: Vec<u8> = (0..1024).map(|i| (i * 7 % 256) as u8).collect();

        let result = compression::compress_if_needed(&data, 100);
        assert!(result.is_ok());
        let (compressed, was_compressed) = result.expect("failed");

        // Either compressed or returned as-is if compression didn't help
        if was_compressed {
            // Should still decompress correctly
            let decompressed =
                compression::decompress_if_needed(&compressed, true).expect("decompression failed");
            assert_eq!(decompressed.as_ref(), data.as_slice());
        } else {
            // Should be original data
            assert_eq!(compressed.as_ref(), data.as_slice());
        }
    }

    #[test]
    fn test_compression_empty_data() {
        let data = b"";
        let result = compression::compress_if_needed(data, 100);
        assert!(result.is_ok());
        let (compressed, was_compressed) = result.expect("failed");
        assert!(!was_compressed);
        assert_eq!(compressed.len(), 0);
    }

    // Serialization/Deserialization tests
    mod serialization_tests {
        use super::*;
        use crate::types::{
            Operation, OperationType, Record, RecordMetadata, RecordSource, SyncStatus, Version,
        };
        use bytes::Bytes;

        #[test]
        fn test_record_serialization_roundtrip() {
            let record = Record::new("test_key".to_string(), Bytes::from("test data"));
            let serialized = serialization::serialize_record(&record);
            assert!(serialized.is_ok());

            let bytes = serialized.expect("serialization should succeed");
            let deserialized = serialization::deserialize_record(&bytes);
            assert!(deserialized.is_ok());

            let restored = deserialized.expect("deserialization should succeed");
            assert_eq!(restored.key, record.key);
            assert_eq!(restored.data, record.data);
            assert_eq!(restored.version.value(), record.version.value());
            assert_eq!(restored.deleted, record.deleted);
        }

        #[test]
        fn test_record_with_metadata_roundtrip() {
            let mut record = Record::new("key_with_meta".to_string(), Bytes::from("payload"));
            record.metadata = RecordMetadata {
                tags: vec!["tag1".to_string(), "tag2".to_string()],
                attributes: vec![
                    ("attr1".to_string(), "value1".to_string()),
                    ("attr2".to_string(), "value2".to_string()),
                ],
                source: RecordSource::Remote,
                sync_status: SyncStatus::Synced,
            };

            let serialized = serialization::serialize_record(&record);
            assert!(serialized.is_ok());

            let bytes = serialized.expect("serialization should succeed");
            let deserialized = serialization::deserialize_record(&bytes);
            assert!(deserialized.is_ok());

            let restored = deserialized.expect("deserialization should succeed");
            assert_eq!(restored.metadata.tags, record.metadata.tags);
            assert_eq!(restored.metadata.attributes, record.metadata.attributes);
            assert_eq!(restored.metadata.source, RecordSource::Remote);
            assert_eq!(restored.metadata.sync_status, SyncStatus::Synced);
        }

        #[test]
        fn test_record_with_empty_data() {
            let record = Record::new("empty".to_string(), Bytes::new());
            let serialized = serialization::serialize_record(&record);
            assert!(serialized.is_ok());

            let bytes = serialized.expect("serialization should succeed");
            let deserialized = serialization::deserialize_record(&bytes);
            assert!(deserialized.is_ok());

            let restored = deserialized.expect("deserialization should succeed");
            assert_eq!(restored.data.len(), 0);
        }

        #[test]
        fn test_record_with_large_data() {
            let large_data = vec![0xABu8; 10000];
            let record = Record::new("large".to_string(), Bytes::from(large_data.clone()));
            let serialized = serialization::serialize_record(&record);
            assert!(serialized.is_ok());

            let bytes = serialized.expect("serialization should succeed");
            let deserialized = serialization::deserialize_record(&bytes);
            assert!(deserialized.is_ok());

            let restored = deserialized.expect("deserialization should succeed");
            assert_eq!(restored.data.as_ref(), large_data.as_slice());
        }

        #[test]
        fn test_record_deleted_flag() {
            let mut record = Record::new("deleted_test".to_string(), Bytes::from("data"));
            record.mark_deleted();

            let serialized = serialization::serialize_record(&record);
            assert!(serialized.is_ok());

            let bytes = serialized.expect("serialization should succeed");
            let deserialized = serialization::deserialize_record(&bytes);
            assert!(deserialized.is_ok());

            let restored = deserialized.expect("deserialization should succeed");
            assert!(restored.deleted);
            assert!(restored.version.value() > 0);
        }

        #[test]
        fn test_operation_insert_roundtrip() {
            let record = Record::new("op_test".to_string(), Bytes::from("insert data"));
            let operation = Operation::insert(&record);

            let serialized = serialization::serialize_operation(&operation);
            assert!(serialized.is_ok());

            let bytes = serialized.expect("serialization should succeed");
            let deserialized = serialization::deserialize_operation(&bytes);
            assert!(deserialized.is_ok());

            let restored = deserialized.expect("deserialization should succeed");
            assert_eq!(restored.operation_type, OperationType::Insert);
            assert_eq!(restored.key, operation.key);
            assert_eq!(restored.payload, operation.payload);
            assert_eq!(restored.base_version.value(), 0);
        }

        #[test]
        fn test_operation_update_roundtrip() {
            let mut record = Record::new("op_update".to_string(), Bytes::from("updated data"));
            let old_version = record.version;
            record.update(Bytes::from("new data"));
            let operation = Operation::update(&record, old_version);

            let serialized = serialization::serialize_operation(&operation);
            assert!(serialized.is_ok());

            let bytes = serialized.expect("serialization should succeed");
            let deserialized = serialization::deserialize_operation(&bytes);
            assert!(deserialized.is_ok());

            let restored = deserialized.expect("deserialization should succeed");
            assert_eq!(restored.operation_type, OperationType::Update);
            assert_eq!(restored.base_version.value(), old_version.value());
        }

        #[test]
        fn test_operation_delete_roundtrip() {
            let record = Record::new("op_delete".to_string(), Bytes::from("to delete"));
            let operation = Operation::delete(&record);

            let serialized = serialization::serialize_operation(&operation);
            assert!(serialized.is_ok());

            let bytes = serialized.expect("serialization should succeed");
            let deserialized = serialization::deserialize_operation(&bytes);
            assert!(deserialized.is_ok());

            let restored = deserialized.expect("deserialization should succeed");
            assert_eq!(restored.operation_type, OperationType::Delete);
            assert!(restored.payload.is_empty());
        }

        #[test]
        fn test_operation_with_retries() {
            let record = Record::new("op_retry".to_string(), Bytes::from("retry data"));
            let mut operation = Operation::insert(&record);
            operation.increment_retry();
            operation.increment_retry();
            operation.priority = 10;

            let serialized = serialization::serialize_operation(&operation);
            assert!(serialized.is_ok());

            let bytes = serialized.expect("serialization should succeed");
            let deserialized = serialization::deserialize_operation(&bytes);
            assert!(deserialized.is_ok());

            let restored = deserialized.expect("deserialization should succeed");
            assert_eq!(restored.retry_count, 2);
            assert_eq!(restored.priority, 10);
            assert!(restored.last_retry.is_some());
        }

        #[test]
        fn test_deserialization_empty_data_error() {
            let result = serialization::deserialize_record(&[]);
            assert!(result.is_err());

            let result = serialization::deserialize_operation(&[]);
            assert!(result.is_err());
        }

        #[test]
        fn test_deserialization_too_short_data_error() {
            let short_data = [1, 2, 3];
            let result = serialization::deserialize_record(&short_data);
            assert!(result.is_err());

            let result = serialization::deserialize_operation(&short_data);
            assert!(result.is_err());
        }

        #[test]
        fn test_deserialization_invalid_version_error() {
            // Create data with an invalid version number (255)
            let mut data = vec![255u8];
            data.extend_from_slice(&[0u8; 100]);
            let result = serialization::deserialize_record(&data);
            assert!(result.is_err());
        }

        #[test]
        fn test_record_unicode_key() {
            let record = Record::new("unicode_key".to_string(), Bytes::from("data"));
            let serialized = serialization::serialize_record(&record);
            assert!(serialized.is_ok());

            let bytes = serialized.expect("serialization should succeed");
            let deserialized = serialization::deserialize_record(&bytes);
            assert!(deserialized.is_ok());

            let restored = deserialized.expect("deserialization should succeed");
            assert_eq!(restored.key, record.key);
        }

        #[test]
        fn test_format_version_detection() {
            // Version 1 data (starts with 1)
            let v1_data = [1u8, 0, 0, 0];
            let version = serialization::detect_format_version(&v1_data);
            assert!(version.is_ok());
            assert_eq!(version.expect("should detect version"), 1);

            // Higher byte (likely v0 UUID start)
            let v0_data = [0xABu8, 0xCD, 0xEF];
            let version = serialization::detect_format_version(&v0_data);
            assert!(version.is_ok());
            assert_eq!(version.expect("should detect version"), 0);

            // Empty data
            let empty: [u8; 0] = [];
            let version = serialization::detect_format_version(&empty);
            assert!(version.is_err());
        }

        #[test]
        fn test_all_sync_status_values() {
            let statuses = [
                SyncStatus::Pending,
                SyncStatus::Syncing,
                SyncStatus::Synced,
                SyncStatus::Failed,
                SyncStatus::Conflict,
            ];

            for status in statuses {
                let mut record = Record::new("status_test".to_string(), Bytes::from("data"));
                record.metadata.sync_status = status;

                let serialized = serialization::serialize_record(&record);
                assert!(serialized.is_ok());

                let bytes = serialized.expect("serialization should succeed");
                let deserialized = serialization::deserialize_record(&bytes);
                assert!(deserialized.is_ok());

                let restored = deserialized.expect("deserialization should succeed");
                assert_eq!(restored.metadata.sync_status, status);
            }
        }

        #[test]
        fn test_all_record_source_values() {
            let sources = [
                RecordSource::Local,
                RecordSource::Remote,
                RecordSource::Merged,
            ];

            for source in sources {
                let mut record = Record::new("source_test".to_string(), Bytes::from("data"));
                record.metadata.source = source;

                let serialized = serialization::serialize_record(&record);
                assert!(serialized.is_ok());

                let bytes = serialized.expect("serialization should succeed");
                let deserialized = serialization::deserialize_record(&bytes);
                assert!(deserialized.is_ok());

                let restored = deserialized.expect("deserialization should succeed");
                assert_eq!(restored.metadata.source, source);
            }
        }

        #[test]
        fn test_all_operation_types() {
            let record = Record::new("op_type_test".to_string(), Bytes::from("data"));

            let operations = [
                Operation::insert(&record),
                Operation::update(&record, Version::zero()),
                Operation::delete(&record),
            ];

            let expected_types = [
                OperationType::Insert,
                OperationType::Update,
                OperationType::Delete,
            ];

            for (op, expected_type) in operations.iter().zip(expected_types.iter()) {
                let serialized = serialization::serialize_operation(op);
                assert!(serialized.is_ok());

                let bytes = serialized.expect("serialization should succeed");
                let deserialized = serialization::deserialize_operation(&bytes);
                assert!(deserialized.is_ok());

                let restored = deserialized.expect("deserialization should succeed");
                assert_eq!(restored.operation_type, *expected_type);
            }
        }
    }
}
