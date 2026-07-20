//! Audit logging framework.

pub mod events;
pub mod logger;
pub mod query;
pub mod storage;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    /// Entry ID.
    pub id: String,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Event type.
    pub event_type: AuditEventType,
    /// Subject (user/service).
    pub subject: Option<String>,
    /// Resource.
    pub resource: Option<String>,
    /// Action.
    pub action: Option<String>,
    /// Result (success/failure).
    pub result: AuditResult,
    /// Source IP.
    pub source_ip: Option<String>,
    /// Tenant ID.
    pub tenant_id: Option<String>,
    /// Additional metadata.
    pub metadata: HashMap<String, String>,
    /// Message.
    pub message: Option<String>,
}

impl AuditLogEntry {
    /// Create a new audit log entry.
    pub fn new(event_type: AuditEventType, result: AuditResult) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            event_type,
            subject: None,
            resource: None,
            action: None,
            result,
            source_ip: None,
            tenant_id: None,
            metadata: HashMap::new(),
            message: None,
        }
    }

    /// Set subject.
    pub fn with_subject(mut self, subject: String) -> Self {
        self.subject = Some(subject);
        self
    }

    /// Set resource.
    pub fn with_resource(mut self, resource: String) -> Self {
        self.resource = Some(resource);
        self
    }

    /// Set action.
    pub fn with_action(mut self, action: String) -> Self {
        self.action = Some(action);
        self
    }

    /// Set source IP.
    pub fn with_source_ip(mut self, ip: String) -> Self {
        self.source_ip = Some(ip);
        self
    }

    /// Set tenant ID.
    pub fn with_tenant_id(mut self, tenant_id: String) -> Self {
        self.tenant_id = Some(tenant_id);
        self
    }

    /// Add metadata.
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Set message.
    pub fn with_message(mut self, message: String) -> Self {
        self.message = Some(message);
        self
    }
}

/// Audit event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuditEventType {
    /// Authentication.
    Authentication,
    /// Authorization.
    Authorization,
    /// Data access.
    DataAccess,
    /// Data modification.
    DataModification,
    /// Configuration change.
    ConfigChange,
    /// User management.
    UserManagement,
    /// Role management.
    RoleManagement,
    /// Key management.
    KeyManagement,
    /// Encryption operation.
    Encryption,
    /// Decryption operation.
    Decryption,
    /// Compliance operation.
    Compliance,
    /// Security scan.
    SecurityScan,
    /// System event.
    System,
}

/// Audit result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditResult {
    /// Operation succeeded.
    Success,
    /// Operation failed.
    Failure,
    /// Access denied.
    Denied,
}

/// Audit log severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AuditSeverity {
    /// Informational.
    Info,
    /// Warning.
    Warning,
    /// Error.
    Error,
    /// Critical.
    Critical,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_log_entry_creation() {
        let entry = AuditLogEntry::new(AuditEventType::Authentication, AuditResult::Success)
            .with_subject("user-123".to_string())
            .with_action("login".to_string())
            .with_source_ip("192.168.1.1".to_string());

        assert_eq!(entry.event_type, AuditEventType::Authentication);
        assert_eq!(entry.result, AuditResult::Success);
        assert_eq!(entry.subject, Some("user-123".to_string()));
    }

    #[test]
    fn test_audit_entry_serialization() {
        let entry = AuditLogEntry::new(AuditEventType::DataAccess, AuditResult::Success);
        let json = serde_json::to_string(&entry).expect("Serialization failed");
        let deserialized: AuditLogEntry =
            serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(deserialized.event_type, entry.event_type);
        assert_eq!(deserialized.result, entry.result);
    }
}
