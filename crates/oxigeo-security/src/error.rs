//! Error types for the security crate.

use thiserror::Error;

/// Result type for security operations.
pub type Result<T> = std::result::Result<T, SecurityError>;

/// Errors that can occur in security operations.
#[derive(Error, Debug)]
pub enum SecurityError {
    /// Encryption operation failed.
    #[error("Encryption error: {0}")]
    Encryption(String),

    /// Decryption operation failed.
    #[error("Decryption error: {0}")]
    Decryption(String),

    /// Key management error.
    #[error("Key management error: {0}")]
    KeyManagement(String),

    /// Key derivation error.
    #[error("Key derivation error: {0}")]
    KeyDerivation(String),

    /// Authentication failed.
    #[error("Authentication failed: {0}")]
    Authentication(String),

    /// Authorization failed.
    #[error("Authorization failed: {0}")]
    Authorization(String),

    /// Access denied.
    #[error("Access denied: {0}")]
    AccessDenied(String),

    /// Permission denied.
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Policy evaluation failed.
    #[error("Policy evaluation error: {0}")]
    PolicyEvaluation(String),

    /// Role not found.
    #[error("Role not found: {0}")]
    RoleNotFound(String),

    /// User not found.
    #[error("User not found: {0}")]
    UserNotFound(String),

    /// Tenant not found.
    #[error("Tenant not found: {0}")]
    TenantNotFound(String),

    /// Tenant isolation violation.
    #[error("Tenant isolation violation: {0}")]
    TenantIsolationViolation(String),

    /// Quota exceeded.
    #[error("Quota exceeded: {0}")]
    QuotaExceeded(String),

    /// Audit logging error.
    #[error("Audit logging error: {0}")]
    AuditLog(String),

    /// Audit query error.
    #[error("Audit query error: {0}")]
    AuditQuery(String),

    /// Lineage tracking error.
    #[error("Lineage tracking error: {0}")]
    LineageTracking(String),

    /// Lineage query error.
    #[error("Lineage query error: {0}")]
    LineageQuery(String),

    /// Anonymization error.
    #[error("Anonymization error: {0}")]
    Anonymization(String),

    /// Compliance violation.
    #[error("Compliance violation: {0}")]
    ComplianceViolation(String),

    /// GDPR compliance error.
    #[error("GDPR compliance error: {0}")]
    GdprCompliance(String),

    /// HIPAA compliance error.
    #[error("HIPAA compliance error: {0}")]
    HipaaCompliance(String),

    /// FedRAMP compliance error.
    #[error("FedRAMP compliance error: {0}")]
    FedRampCompliance(String),

    /// Security scanning error.
    #[error("Security scanning error: {0}")]
    SecurityScan(String),

    /// Vulnerability detected.
    #[error("Vulnerability detected: {0}")]
    VulnerabilityDetected(String),

    /// Secret detected in data.
    #[error("Secret detected: {0}")]
    SecretDetected(String),

    /// Malware detected.
    #[error("Malware detected: {0}")]
    MalwareDetected(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Invalid input.
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Invalid key format.
    #[error("Invalid key format: {0}")]
    InvalidKeyFormat(String),

    /// Invalid ciphertext.
    #[error("Invalid ciphertext: {0}")]
    InvalidCiphertext(String),

    /// TLS error.
    #[error("TLS error: {0}")]
    Tls(String),

    /// Certificate error.
    #[error("Certificate error: {0}")]
    Certificate(String),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deserialization error.
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// Storage error.
    #[error("Storage error: {0}")]
    Storage(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl SecurityError {
    /// Create a new encryption error.
    pub fn encryption<S: Into<String>>(msg: S) -> Self {
        SecurityError::Encryption(msg.into())
    }

    /// Create a new decryption error.
    pub fn decryption<S: Into<String>>(msg: S) -> Self {
        SecurityError::Decryption(msg.into())
    }

    /// Create a new key management error.
    pub fn key_management<S: Into<String>>(msg: S) -> Self {
        SecurityError::KeyManagement(msg.into())
    }

    /// Create a new key derivation error.
    pub fn key_derivation<S: Into<String>>(msg: S) -> Self {
        SecurityError::KeyDerivation(msg.into())
    }

    /// Create a new authentication error.
    pub fn authentication<S: Into<String>>(msg: S) -> Self {
        SecurityError::Authentication(msg.into())
    }

    /// Create a new authorization error.
    pub fn authorization<S: Into<String>>(msg: S) -> Self {
        SecurityError::Authorization(msg.into())
    }

    /// Create a new access denied error.
    pub fn access_denied<S: Into<String>>(msg: S) -> Self {
        SecurityError::AccessDenied(msg.into())
    }

    /// Create a new permission denied error.
    pub fn permission_denied<S: Into<String>>(msg: S) -> Self {
        SecurityError::PermissionDenied(msg.into())
    }

    /// Create a new internal error.
    pub fn internal<S: Into<String>>(msg: S) -> Self {
        SecurityError::Internal(msg.into())
    }

    /// Create a new tenant not found error.
    pub fn tenant_not_found<S: Into<String>>(msg: S) -> Self {
        SecurityError::TenantNotFound(msg.into())
    }

    /// Create a new quota exceeded error.
    pub fn quota_exceeded<S: Into<String>>(msg: S) -> Self {
        SecurityError::QuotaExceeded(msg.into())
    }

    /// Create a new lineage tracking error.
    pub fn lineage_tracking<S: Into<String>>(msg: S) -> Self {
        SecurityError::LineageTracking(msg.into())
    }

    /// Create a new lineage query error.
    pub fn lineage_query<S: Into<String>>(msg: S) -> Self {
        SecurityError::LineageQuery(msg.into())
    }

    /// Create a new audit log error.
    pub fn audit_log<S: Into<String>>(msg: S) -> Self {
        SecurityError::AuditLog(msg.into())
    }

    /// Create a new audit query error.
    pub fn audit_query<S: Into<String>>(msg: S) -> Self {
        SecurityError::AuditQuery(msg.into())
    }

    /// Create a new policy evaluation error.
    pub fn policy_evaluation<S: Into<String>>(msg: S) -> Self {
        SecurityError::PolicyEvaluation(msg.into())
    }

    /// Create a new role not found error.
    pub fn role_not_found<S: Into<String>>(msg: S) -> Self {
        SecurityError::RoleNotFound(msg.into())
    }

    /// Create a new user not found error.
    pub fn user_not_found<S: Into<String>>(msg: S) -> Self {
        SecurityError::UserNotFound(msg.into())
    }

    /// Create a new anonymization error.
    pub fn anonymization<S: Into<String>>(msg: S) -> Self {
        SecurityError::Anonymization(msg.into())
    }

    /// Create a new compliance violation error.
    pub fn compliance_violation<S: Into<String>>(msg: S) -> Self {
        SecurityError::ComplianceViolation(msg.into())
    }

    /// Create a new invalid input error.
    pub fn invalid_input<S: Into<String>>(msg: S) -> Self {
        SecurityError::InvalidInput(msg.into())
    }

    /// Create a new serialization error.
    pub fn serialization<S: Into<String>>(msg: S) -> Self {
        SecurityError::Serialization(msg.into())
    }

    /// Create a new deserialization error.
    pub fn deserialization<S: Into<String>>(msg: S) -> Self {
        SecurityError::Deserialization(msg.into())
    }

    /// Create a new certificate error.
    pub fn certificate<S: Into<String>>(msg: S) -> Self {
        SecurityError::Certificate(msg.into())
    }

    /// Create a new TLS error.
    pub fn tls<S: Into<String>>(msg: S) -> Self {
        SecurityError::Tls(msg.into())
    }

    /// Create a new storage error.
    pub fn storage<S: Into<String>>(msg: S) -> Self {
        SecurityError::Storage(msg.into())
    }

    /// Create a new invalid configuration error.
    pub fn invalid_configuration<S: Into<String>>(msg: S) -> Self {
        SecurityError::InvalidConfiguration(msg.into())
    }

    /// Create a new invalid key format error.
    pub fn invalid_key_format<S: Into<String>>(msg: S) -> Self {
        SecurityError::InvalidKeyFormat(msg.into())
    }

    /// Create a new invalid ciphertext error.
    pub fn invalid_ciphertext<S: Into<String>>(msg: S) -> Self {
        SecurityError::InvalidCiphertext(msg.into())
    }

    /// Create a new tenant isolation violation error.
    pub fn tenant_isolation_violation<S: Into<String>>(msg: S) -> Self {
        SecurityError::TenantIsolationViolation(msg.into())
    }

    /// Create a new GDPR compliance error.
    pub fn gdpr_compliance<S: Into<String>>(msg: S) -> Self {
        SecurityError::GdprCompliance(msg.into())
    }

    /// Create a new HIPAA compliance error.
    pub fn hipaa_compliance<S: Into<String>>(msg: S) -> Self {
        SecurityError::HipaaCompliance(msg.into())
    }

    /// Create a new FedRAMP compliance error.
    pub fn fedramp_compliance<S: Into<String>>(msg: S) -> Self {
        SecurityError::FedRampCompliance(msg.into())
    }

    /// Create a new security scan error.
    pub fn security_scan<S: Into<String>>(msg: S) -> Self {
        SecurityError::SecurityScan(msg.into())
    }

    /// Create a new vulnerability detected error.
    pub fn vulnerability_detected<S: Into<String>>(msg: S) -> Self {
        SecurityError::VulnerabilityDetected(msg.into())
    }

    /// Create a new secret detected error.
    pub fn secret_detected<S: Into<String>>(msg: S) -> Self {
        SecurityError::SecretDetected(msg.into())
    }

    /// Create a new malware detected error.
    pub fn malware_detected<S: Into<String>>(msg: S) -> Self {
        SecurityError::MalwareDetected(msg.into())
    }
}
