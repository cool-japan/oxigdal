//! Audit log queries.

use crate::audit::{AuditEventType, AuditLogEntry, AuditResult, storage::StorageQuery};

/// Audit query builder.
pub struct AuditQuery {
    query: StorageQuery,
}

impl AuditQuery {
    /// Create new audit query.
    pub fn new() -> Self {
        Self {
            query: StorageQuery::new(),
        }
    }

    /// Filter by subject.
    pub fn subject(mut self, subject: String) -> Self {
        self.query = self.query.with_subject(subject);
        self
    }

    /// Filter by resource.
    pub fn resource(mut self, resource: String) -> Self {
        self.query = self.query.with_resource(resource);
        self
    }

    /// Filter by event type.
    pub fn event_type(mut self, event_type: AuditEventType) -> Self {
        self.query = self.query.with_event_type(event_type);
        self
    }

    /// Filter by result.
    pub fn result(mut self, result: AuditResult) -> Self {
        self.query = self.query.with_result(result);
        self
    }

    /// Set limit.
    pub fn limit(mut self, limit: usize) -> Self {
        self.query = self.query.with_limit(limit);
        self
    }

    /// Build the storage query.
    pub fn build(self) -> StorageQuery {
        self.query
    }
}

impl Default for AuditQuery {
    fn default() -> Self {
        Self::new()
    }
}

/// Audit report generator.
pub struct AuditReport {
    entries: Vec<AuditLogEntry>,
}

impl AuditReport {
    /// Create new audit report.
    pub fn new(entries: Vec<AuditLogEntry>) -> Self {
        Self { entries }
    }

    /// Get total count.
    pub fn total_count(&self) -> usize {
        self.entries.len()
    }

    /// Get success count.
    pub fn success_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.result == AuditResult::Success)
            .count()
    }

    /// Get failure count.
    pub fn failure_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.result == AuditResult::Failure)
            .count()
    }

    /// Get denied count.
    pub fn denied_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.result == AuditResult::Denied)
            .count()
    }

    /// Group by event type.
    pub fn by_event_type(&self) -> std::collections::HashMap<AuditEventType, usize> {
        let mut map = std::collections::HashMap::new();
        for entry in &self.entries {
            *map.entry(entry.event_type).or_insert(0) += 1;
        }
        map
    }

    /// Group by subject.
    pub fn by_subject(&self) -> std::collections::HashMap<String, usize> {
        let mut map = std::collections::HashMap::new();
        for entry in &self.entries {
            if let Some(ref subject) = entry.subject {
                *map.entry(subject.clone()).or_insert(0) += 1;
            }
        }
        map
    }

    /// Generate summary text.
    pub fn summary(&self) -> String {
        format!(
            "Audit Report:\n\
             Total Events: {}\n\
             Success: {}\n\
             Failure: {}\n\
             Denied: {}",
            self.total_count(),
            self.success_count(),
            self.failure_count(),
            self.denied_count()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_query_builder() {
        let query = AuditQuery::new()
            .subject("user-123".to_string())
            .event_type(AuditEventType::Authentication)
            .result(AuditResult::Success)
            .limit(10)
            .build();

        assert_eq!(query.subject, Some("user-123".to_string()));
        assert_eq!(query.event_type, Some(AuditEventType::Authentication));
        assert_eq!(query.limit, Some(10));
    }

    #[test]
    fn test_audit_report() {
        let entry1 = AuditLogEntry::new(AuditEventType::Authentication, AuditResult::Success);
        let entry2 = AuditLogEntry::new(AuditEventType::Authentication, AuditResult::Failure);
        let entry3 = AuditLogEntry::new(AuditEventType::DataAccess, AuditResult::Denied);

        let report = AuditReport::new(vec![entry1, entry2, entry3]);

        assert_eq!(report.total_count(), 3);
        assert_eq!(report.success_count(), 1);
        assert_eq!(report.failure_count(), 1);
        assert_eq!(report.denied_count(), 1);

        let by_type = report.by_event_type();
        assert_eq!(by_type.get(&AuditEventType::Authentication), Some(&2));
        assert_eq!(by_type.get(&AuditEventType::DataAccess), Some(&1));
    }
}
