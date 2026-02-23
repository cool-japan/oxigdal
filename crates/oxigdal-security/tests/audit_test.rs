//! Integration tests for audit logging.

use oxigdal_security::audit::{
    AuditEventType, AuditLogEntry, AuditResult,
    logger::AuditLogger,
    storage::{AuditStorage, InMemoryAuditStorage, StorageQuery},
};
use tokio::sync::mpsc;

#[tokio::test]
async fn test_audit_logging() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let logger = AuditLogger::new(tx);
    let storage = InMemoryAuditStorage::new();

    logger
        .log_authentication("user-123".to_string(), AuditResult::Success, None)
        .expect("Failed to log");

    if let Some(entry) = rx.recv().await {
        storage.store(entry).expect("Failed to store");
    }

    assert_eq!(storage.len(), 1);

    let query = StorageQuery::new().with_subject("user-123".to_string());
    let results = storage.query(&query).expect("Query failed");
    assert_eq!(results.len(), 1);
}

#[test]
fn test_audit_storage() {
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
