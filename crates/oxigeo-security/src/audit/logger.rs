//! Structured audit logger.

use crate::audit::{AuditEventType, AuditLogEntry, AuditResult};
use crate::error::Result;
use tokio::sync::mpsc;

/// Audit logger.
pub struct AuditLogger {
    sender: mpsc::UnboundedSender<AuditLogEntry>,
}

impl AuditLogger {
    /// Create a new audit logger.
    pub fn new(sender: mpsc::UnboundedSender<AuditLogEntry>) -> Self {
        Self { sender }
    }

    /// Log an audit event.
    pub fn log(&self, entry: AuditLogEntry) -> Result<()> {
        self.sender
            .send(entry)
            .map_err(|_| crate::error::SecurityError::audit_log("Failed to send audit log"))?;
        Ok(())
    }

    /// Log authentication event.
    pub fn log_authentication(
        &self,
        subject: String,
        result: AuditResult,
        source_ip: Option<String>,
    ) -> Result<()> {
        let mut entry =
            AuditLogEntry::new(AuditEventType::Authentication, result).with_subject(subject);

        if let Some(ip) = source_ip {
            entry = entry.with_source_ip(ip);
        }

        self.log(entry)
    }

    /// Log authorization event.
    pub fn log_authorization(
        &self,
        subject: String,
        resource: String,
        action: String,
        result: AuditResult,
    ) -> Result<()> {
        let entry = AuditLogEntry::new(AuditEventType::Authorization, result)
            .with_subject(subject)
            .with_resource(resource)
            .with_action(action);

        self.log(entry)
    }

    /// Log data access.
    pub fn log_data_access(
        &self,
        subject: String,
        resource: String,
        result: AuditResult,
    ) -> Result<()> {
        let entry = AuditLogEntry::new(AuditEventType::DataAccess, result)
            .with_subject(subject)
            .with_resource(resource)
            .with_action("read".to_string());

        self.log(entry)
    }

    /// Log data modification.
    pub fn log_data_modification(
        &self,
        subject: String,
        resource: String,
        action: String,
        result: AuditResult,
    ) -> Result<()> {
        let entry = AuditLogEntry::new(AuditEventType::DataModification, result)
            .with_subject(subject)
            .with_resource(resource)
            .with_action(action);

        self.log(entry)
    }

    /// Log configuration change.
    pub fn log_config_change(
        &self,
        subject: String,
        config_key: String,
        old_value: Option<String>,
        new_value: String,
    ) -> Result<()> {
        let mut entry = AuditLogEntry::new(AuditEventType::ConfigChange, AuditResult::Success)
            .with_subject(subject)
            .with_metadata("config_key".to_string(), config_key)
            .with_metadata("new_value".to_string(), new_value);

        if let Some(old) = old_value {
            entry = entry.with_metadata("old_value".to_string(), old);
        }

        self.log(entry)
    }

    /// Log encryption operation.
    pub fn log_encryption(
        &self,
        subject: String,
        resource: String,
        algorithm: String,
    ) -> Result<()> {
        let entry = AuditLogEntry::new(AuditEventType::Encryption, AuditResult::Success)
            .with_subject(subject)
            .with_resource(resource)
            .with_metadata("algorithm".to_string(), algorithm);

        self.log(entry)
    }

    /// Log decryption operation.
    pub fn log_decryption(
        &self,
        subject: String,
        resource: String,
        result: AuditResult,
    ) -> Result<()> {
        let entry = AuditLogEntry::new(AuditEventType::Decryption, result)
            .with_subject(subject)
            .with_resource(resource);

        self.log(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_logger() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let logger = AuditLogger::new(tx);

        logger
            .log_authentication("user-123".to_string(), AuditResult::Success, None)
            .expect("Failed to log");

        let entry = rx.try_recv().expect("No log entry received");
        assert_eq!(entry.event_type, AuditEventType::Authentication);
        assert_eq!(entry.result, AuditResult::Success);
    }
}
