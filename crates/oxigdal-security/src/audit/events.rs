//! Audit event definitions.

use crate::audit::{AuditEventType, AuditLogEntry, AuditResult};
use serde::{Deserialize, Serialize};

/// Authentication event builder.
pub struct AuthenticationEvent {
    subject: String,
    method: Option<String>,
    result: AuditResult,
    reason: Option<String>,
}

impl AuthenticationEvent {
    /// Create new authentication event.
    pub fn new(subject: String, result: AuditResult) -> Self {
        Self {
            subject,
            method: None,
            result,
            reason: None,
        }
    }

    /// Set authentication method.
    pub fn with_method(mut self, method: String) -> Self {
        self.method = Some(method);
        self
    }

    /// Set failure reason.
    pub fn with_reason(mut self, reason: String) -> Self {
        self.reason = Some(reason);
        self
    }

    /// Build audit log entry.
    pub fn build(self) -> AuditLogEntry {
        let mut entry = AuditLogEntry::new(AuditEventType::Authentication, self.result)
            .with_subject(self.subject);

        if let Some(method) = self.method {
            entry = entry.with_metadata("method".to_string(), method);
        }

        if let Some(reason) = self.reason {
            entry = entry.with_message(reason);
        }

        entry
    }
}

/// Data access event builder.
pub struct DataAccessEvent {
    subject: String,
    resource: String,
    action: String,
    result: AuditResult,
    rows_accessed: Option<u64>,
}

impl DataAccessEvent {
    /// Create new data access event.
    pub fn new(subject: String, resource: String, action: String, result: AuditResult) -> Self {
        Self {
            subject,
            resource,
            action,
            result,
            rows_accessed: None,
        }
    }

    /// Set rows accessed.
    pub fn with_rows_accessed(mut self, count: u64) -> Self {
        self.rows_accessed = Some(count);
        self
    }

    /// Build audit log entry.
    pub fn build(self) -> AuditLogEntry {
        let mut entry = AuditLogEntry::new(AuditEventType::DataAccess, self.result)
            .with_subject(self.subject)
            .with_resource(self.resource)
            .with_action(self.action);

        if let Some(count) = self.rows_accessed {
            entry = entry.with_metadata("rows_accessed".to_string(), count.to_string());
        }

        entry
    }
}

/// Compliance event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceEvent {
    /// Event type.
    pub event_type: String,
    /// Regulation (GDPR, HIPAA, etc.).
    pub regulation: String,
    /// Subject.
    pub subject: Option<String>,
    /// Data subject (person whose data is involved).
    pub data_subject: Option<String>,
    /// Result.
    pub result: AuditResult,
    /// Details.
    pub details: String,
}

impl ComplianceEvent {
    /// Create new compliance event.
    pub fn new(event_type: String, regulation: String, result: AuditResult) -> Self {
        Self {
            event_type,
            regulation,
            subject: None,
            data_subject: None,
            result,
            details: String::new(),
        }
    }

    /// Set subject.
    pub fn with_subject(mut self, subject: String) -> Self {
        self.subject = Some(subject);
        self
    }

    /// Set data subject.
    pub fn with_data_subject(mut self, data_subject: String) -> Self {
        self.data_subject = Some(data_subject);
        self
    }

    /// Set details.
    pub fn with_details(mut self, details: String) -> Self {
        self.details = details;
        self
    }

    /// Build audit log entry.
    pub fn build(self) -> AuditLogEntry {
        let mut entry = AuditLogEntry::new(AuditEventType::Compliance, self.result)
            .with_metadata("regulation".to_string(), self.regulation)
            .with_metadata("event_type".to_string(), self.event_type)
            .with_message(self.details);

        if let Some(subject) = self.subject {
            entry = entry.with_subject(subject);
        }

        if let Some(data_subject) = self.data_subject {
            entry = entry.with_metadata("data_subject".to_string(), data_subject);
        }

        entry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authentication_event() {
        let event = AuthenticationEvent::new("user-123".to_string(), AuditResult::Success)
            .with_method("password".to_string())
            .build();

        assert_eq!(event.event_type, AuditEventType::Authentication);
        assert_eq!(event.subject, Some("user-123".to_string()));
        assert_eq!(event.metadata.get("method"), Some(&"password".to_string()));
    }

    #[test]
    fn test_data_access_event() {
        let event = DataAccessEvent::new(
            "user-123".to_string(),
            "dataset-456".to_string(),
            "read".to_string(),
            AuditResult::Success,
        )
        .with_rows_accessed(100)
        .build();

        assert_eq!(event.event_type, AuditEventType::DataAccess);
        assert_eq!(
            event.metadata.get("rows_accessed"),
            Some(&"100".to_string())
        );
    }
}
