//! Audit log storage.

use crate::audit::AuditLogEntry;
use crate::error::Result;
use dashmap::DashMap;
use std::sync::Arc;

/// Audit log storage trait.
pub trait AuditStorage: Send + Sync {
    /// Store an audit log entry.
    fn store(&self, entry: AuditLogEntry) -> Result<()>;

    /// Query audit logs.
    fn query(&self, query: &StorageQuery) -> Result<Vec<AuditLogEntry>>;

    /// Count audit logs matching query.
    fn count(&self, query: &StorageQuery) -> Result<usize>;
}

/// In-memory audit storage.
pub struct InMemoryAuditStorage {
    entries: Arc<DashMap<String, AuditLogEntry>>,
}

impl InMemoryAuditStorage {
    /// Create new in-memory storage.
    pub fn new() -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
        }
    }

    /// Get number of stored entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if storage is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all entries.
    pub fn clear(&self) {
        self.entries.clear();
    }
}

impl Default for InMemoryAuditStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditStorage for InMemoryAuditStorage {
    fn store(&self, entry: AuditLogEntry) -> Result<()> {
        self.entries.insert(entry.id.clone(), entry);
        Ok(())
    }

    fn query(&self, query: &StorageQuery) -> Result<Vec<AuditLogEntry>> {
        let mut results: Vec<AuditLogEntry> = self
            .entries
            .iter()
            .map(|e| e.value().clone())
            .filter(|entry| query.matches(entry))
            .collect();

        // Sort by timestamp (most recent first)
        results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        // Apply limit
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }

        Ok(results)
    }

    fn count(&self, query: &StorageQuery) -> Result<usize> {
        Ok(self
            .entries
            .iter()
            .filter(|e| query.matches(e.value()))
            .count())
    }
}

/// Storage query.
#[derive(Debug, Clone, Default)]
pub struct StorageQuery {
    /// Filter by subject.
    pub subject: Option<String>,
    /// Filter by resource.
    pub resource: Option<String>,
    /// Filter by event type.
    pub event_type: Option<crate::audit::AuditEventType>,
    /// Filter by result.
    pub result: Option<crate::audit::AuditResult>,
    /// Filter by tenant ID.
    pub tenant_id: Option<String>,
    /// Time range start.
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
    /// Time range end.
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
    /// Maximum number of results.
    pub limit: Option<usize>,
}

impl StorageQuery {
    /// Create new storage query.
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by subject.
    pub fn with_subject(mut self, subject: String) -> Self {
        self.subject = Some(subject);
        self
    }

    /// Filter by resource.
    pub fn with_resource(mut self, resource: String) -> Self {
        self.resource = Some(resource);
        self
    }

    /// Filter by event type.
    pub fn with_event_type(mut self, event_type: crate::audit::AuditEventType) -> Self {
        self.event_type = Some(event_type);
        self
    }

    /// Filter by result.
    pub fn with_result(mut self, result: crate::audit::AuditResult) -> Self {
        self.result = Some(result);
        self
    }

    /// Filter by tenant ID.
    pub fn with_tenant_id(mut self, tenant_id: String) -> Self {
        self.tenant_id = Some(tenant_id);
        self
    }

    /// Set time range.
    pub fn with_time_range(
        mut self,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        self.start_time = Some(start);
        self.end_time = Some(end);
        self
    }

    /// Set limit.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Check if entry matches query.
    pub fn matches(&self, entry: &AuditLogEntry) -> bool {
        if let Some(ref subject) = self.subject {
            if entry.subject.as_ref() != Some(subject) {
                return false;
            }
        }

        if let Some(ref resource) = self.resource {
            if entry.resource.as_ref() != Some(resource) {
                return false;
            }
        }

        if let Some(event_type) = self.event_type {
            if entry.event_type != event_type {
                return false;
            }
        }

        if let Some(result) = self.result {
            if entry.result != result {
                return false;
            }
        }

        if let Some(ref tenant_id) = self.tenant_id {
            if entry.tenant_id.as_ref() != Some(tenant_id) {
                return false;
            }
        }

        if let Some(start) = self.start_time {
            if entry.timestamp < start {
                return false;
            }
        }

        if let Some(end) = self.end_time {
            if entry.timestamp > end {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{AuditEventType, AuditResult};

    #[test]
    fn test_in_memory_storage() {
        let storage = InMemoryAuditStorage::new();

        let entry = AuditLogEntry::new(AuditEventType::Authentication, AuditResult::Success)
            .with_subject("user-123".to_string());

        storage.store(entry.clone()).expect("Failed to store");
        assert_eq!(storage.len(), 1);

        let query = StorageQuery::new().with_subject("user-123".to_string());
        let results = storage.query(&query).expect("Query failed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, entry.id);
    }

    #[test]
    fn test_storage_query() {
        let storage = InMemoryAuditStorage::new();

        let entry1 = AuditLogEntry::new(AuditEventType::Authentication, AuditResult::Success)
            .with_subject("user-1".to_string());
        let entry2 = AuditLogEntry::new(AuditEventType::DataAccess, AuditResult::Success)
            .with_subject("user-2".to_string());

        storage.store(entry1).expect("Failed to store");
        storage.store(entry2).expect("Failed to store");

        let query = StorageQuery::new().with_event_type(AuditEventType::Authentication);
        let results = storage.query(&query).expect("Query failed");
        assert_eq!(results.len(), 1);
    }
}
