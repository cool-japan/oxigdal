//! Core types for offline data management

use bytes::Bytes;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Unique identifier for a record
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecordId(Uuid);

impl RecordId {
    /// Create a new random record ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create a record ID from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get the UUID
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Parse from string
    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl Default for RecordId {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Display for RecordId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for an operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationId(Uuid);

impl OperationId {
    /// Create a new random operation ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create an operation ID from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get the UUID
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Parse from string
    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Display for OperationId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Version number for optimistic concurrency control
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(u64);

impl Version {
    /// Create a new version with value 0
    pub fn zero() -> Self {
        Self(0)
    }

    /// Create a version from a u64
    pub fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// Get the version value
    pub fn value(&self) -> u64 {
        self.0
    }

    /// Increment the version
    pub fn increment(&mut self) {
        self.0 = self.0.saturating_add(1);
    }

    /// Get the next version
    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl Default for Version {
    fn default() -> Self {
        Self::zero()
    }
}

impl core::fmt::Display for Version {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A record in the offline storage
#[derive(Debug, Clone)]
pub struct Record {
    /// Unique identifier
    pub id: RecordId,
    /// Record key (for lookup)
    pub key: String,
    /// Record data
    pub data: Bytes,
    /// Version number
    pub version: Version,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last modification timestamp
    pub updated_at: DateTime<Utc>,
    /// Deletion flag
    pub deleted: bool,
    /// Metadata
    pub metadata: RecordMetadata,
}

impl Record {
    /// Create a new record
    pub fn new(key: String, data: Bytes) -> Self {
        let now = Utc::now();
        Self {
            id: RecordId::new(),
            key,
            data,
            version: Version::zero(),
            created_at: now,
            updated_at: now,
            deleted: false,
            metadata: RecordMetadata::default(),
        }
    }

    /// Update the record data
    pub fn update(&mut self, data: Bytes) {
        self.data = data;
        self.version.increment();
        self.updated_at = Utc::now();
    }

    /// Mark as deleted
    pub fn mark_deleted(&mut self) {
        self.deleted = true;
        self.version.increment();
        self.updated_at = Utc::now();
    }

    /// Check if the record is newer than another
    pub fn is_newer_than(&self, other: &Self) -> bool {
        self.version > other.version
            || (self.version == other.version && self.updated_at > other.updated_at)
    }
}

/// Metadata for a record
#[derive(Debug, Clone, Default)]
pub struct RecordMetadata {
    /// Tags associated with the record
    pub tags: Vec<String>,
    /// Custom attributes
    pub attributes: Vec<(String, String)>,
    /// Source of the record (local, remote, sync)
    pub source: RecordSource,
    /// Sync status
    pub sync_status: SyncStatus,
}

/// Source of a record
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordSource {
    /// Created locally
    #[default]
    Local,
    /// Received from remote
    Remote,
    /// Result of sync/merge
    Merged,
}

/// Sync status of a record
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncStatus {
    /// Not yet synced
    #[default]
    Pending,
    /// Currently syncing
    Syncing,
    /// Successfully synced
    Synced,
    /// Sync failed
    Failed,
    /// Conflict detected
    Conflict,
}

/// An operation to be synced
#[derive(Debug, Clone)]
pub struct Operation {
    /// Unique identifier
    pub id: OperationId,
    /// Type of operation
    pub operation_type: OperationType,
    /// Record ID affected
    pub record_id: RecordId,
    /// Record key
    pub key: String,
    /// Operation payload
    pub payload: Bytes,
    /// Version before operation
    pub base_version: Version,
    /// Version after operation
    pub target_version: Version,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Number of retry attempts
    pub retry_count: usize,
    /// Last retry timestamp
    pub last_retry: Option<DateTime<Utc>>,
    /// Priority (higher = more urgent)
    pub priority: u8,
}

impl Operation {
    /// Create a new insert operation
    pub fn insert(record: &Record) -> Self {
        Self {
            id: OperationId::new(),
            operation_type: OperationType::Insert,
            record_id: record.id,
            key: record.key.clone(),
            payload: record.data.clone(),
            base_version: Version::zero(),
            target_version: record.version,
            created_at: Utc::now(),
            retry_count: 0,
            last_retry: None,
            priority: 5,
        }
    }

    /// Create a new update operation
    pub fn update(record: &Record, old_version: Version) -> Self {
        Self {
            id: OperationId::new(),
            operation_type: OperationType::Update,
            record_id: record.id,
            key: record.key.clone(),
            payload: record.data.clone(),
            base_version: old_version,
            target_version: record.version,
            created_at: Utc::now(),
            retry_count: 0,
            last_retry: None,
            priority: 5,
        }
    }

    /// Create a new delete operation
    pub fn delete(record: &Record) -> Self {
        Self {
            id: OperationId::new(),
            operation_type: OperationType::Delete,
            record_id: record.id,
            key: record.key.clone(),
            payload: Bytes::new(),
            base_version: record.version,
            target_version: record.version.next(),
            created_at: Utc::now(),
            retry_count: 0,
            last_retry: None,
            priority: 5,
        }
    }

    /// Increment retry count
    pub fn increment_retry(&mut self) {
        self.retry_count = self.retry_count.saturating_add(1);
        self.last_retry = Some(Utc::now());
    }
}

/// Type of operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationType {
    /// Insert a new record
    Insert,
    /// Update an existing record
    Update,
    /// Delete a record
    Delete,
}

impl core::fmt::Display for OperationType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Insert => write!(f, "INSERT"),
            Self::Update => write!(f, "UPDATE"),
            Self::Delete => write!(f, "DELETE"),
        }
    }
}

/// Conflict between local and remote records
#[derive(Debug, Clone)]
pub struct Conflict {
    /// Local record
    pub local: Record,
    /// Remote record
    pub remote: Record,
    /// Common ancestor (if available)
    pub base: Option<Record>,
    /// Conflict type
    pub conflict_type: ConflictType,
}

impl Conflict {
    /// Create a new conflict
    pub fn new(local: Record, remote: Record, base: Option<Record>) -> Self {
        let conflict_type = Self::detect_type(&local, &remote, base.as_ref());
        Self {
            local,
            remote,
            base,
            conflict_type,
        }
    }

    /// Detect conflict type
    fn detect_type(local: &Record, remote: &Record, base: Option<&Record>) -> ConflictType {
        if local.deleted && remote.deleted {
            ConflictType::DeleteDelete
        } else if local.deleted {
            ConflictType::DeleteUpdate
        } else if remote.deleted {
            ConflictType::UpdateDelete
        } else if base.is_some() {
            ConflictType::UpdateUpdate
        } else {
            ConflictType::InsertInsert
        }
    }
}

/// Type of conflict
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictType {
    /// Both sides inserted the same key
    InsertInsert,
    /// Both sides updated the record
    UpdateUpdate,
    /// One side deleted, other updated
    DeleteUpdate,
    /// One side updated, other deleted
    UpdateDelete,
    /// Both sides deleted
    DeleteDelete,
}

impl core::fmt::Display for ConflictType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InsertInsert => write!(f, "INSERT-INSERT"),
            Self::UpdateUpdate => write!(f, "UPDATE-UPDATE"),
            Self::DeleteUpdate => write!(f, "DELETE-UPDATE"),
            Self::UpdateDelete => write!(f, "UPDATE-DELETE"),
            Self::DeleteDelete => write!(f, "DELETE-DELETE"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_id_roundtrip() {
        let id = RecordId::new();
        let s = id.to_string();
        let parsed = RecordId::parse(&s);
        assert!(parsed.is_ok());
        assert_eq!(id, parsed.expect("failed to parse"));
    }

    #[test]
    fn test_version_increment() {
        let mut v = Version::zero();
        assert_eq!(v.value(), 0);
        v.increment();
        assert_eq!(v.value(), 1);
        let next = v.next();
        assert_eq!(next.value(), 2);
        assert_eq!(v.value(), 1);
    }

    #[test]
    fn test_record_update() {
        let mut record = Record::new("test".to_string(), Bytes::from("data1"));
        let v0 = record.version;
        record.update(Bytes::from("data2"));
        assert_eq!(record.version, v0.next());
        assert_eq!(record.data, Bytes::from("data2"));
    }

    #[test]
    fn test_operation_creation() {
        let record = Record::new("test".to_string(), Bytes::from("data"));
        let op = Operation::insert(&record);
        assert_eq!(op.operation_type, OperationType::Insert);
        assert_eq!(op.retry_count, 0);
        assert!(op.last_retry.is_none());
    }

    #[test]
    fn test_conflict_type_detection() {
        let local = Record::new("test".to_string(), Bytes::from("local"));
        let remote = Record::new("test".to_string(), Bytes::from("remote"));
        let conflict = Conflict::new(local, remote, None);
        assert_eq!(conflict.conflict_type, ConflictType::InsertInsert);
    }
}
