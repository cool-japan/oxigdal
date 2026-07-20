//! Enterprise security features for OxiGDAL.
//!
//! This crate provides comprehensive security features including:
//! - End-to-end encryption (at rest and in transit)
//! - Access control (RBAC and ABAC)
//! - Data lineage tracking
//! - Audit logging
//! - Multi-tenancy support
//! - Data anonymization
//! - Compliance reporting (GDPR, HIPAA, FedRAMP)
//! - Security scanning
//!
//! # Cargo features
//!
//! - `enterprise` (default): the full server-side surface listed above
//!   (async audit/lineage/multitenancy, encryption at rest, scanning, ...).
//! - `tls` (default): TLS-in-transit support via the pure-Rust OxiTLS stack;
//!   implies `enterprise`.
//! - `attestation` (default): lightweight, wasm-friendly crypto dependencies
//!   (blake3 hash chain / Merkle root / Ed25519 session seal) for
//!   tamper-evident session ledgers. The attestation module itself lands in a
//!   follow-up work package; today the feature only enables its dependencies.
//!
//! With `--no-default-features` the crate exposes only [`SecurityConfig`] and
//! the [`error`] module.

#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]
#![warn(missing_docs)]

#[cfg(feature = "enterprise")]
pub mod access_control;
#[cfg(feature = "enterprise")]
pub mod anonymization;
#[cfg(feature = "attestation")]
pub mod attestation;
#[cfg(feature = "enterprise")]
pub mod audit;
#[cfg(feature = "enterprise")]
pub mod compliance;
#[cfg(feature = "enterprise")]
pub mod encryption;
pub mod error;
#[cfg(feature = "enterprise")]
pub mod lineage;
#[cfg(feature = "enterprise")]
pub mod multitenancy;
#[cfg(feature = "enterprise")]
pub mod scanning;

pub use error::{Result, SecurityError};

/// Security configuration.
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// Enable encryption.
    pub encryption_enabled: bool,
    /// Enable access control.
    pub access_control_enabled: bool,
    /// Enable audit logging.
    pub audit_logging_enabled: bool,
    /// Enable lineage tracking.
    pub lineage_tracking_enabled: bool,
    /// Enable multi-tenancy.
    pub multitenancy_enabled: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            encryption_enabled: true,
            access_control_enabled: true,
            audit_logging_enabled: true,
            lineage_tracking_enabled: true,
            multitenancy_enabled: false,
        }
    }
}

impl SecurityConfig {
    /// Create a new security configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable encryption.
    pub fn with_encryption(mut self, enabled: bool) -> Self {
        self.encryption_enabled = enabled;
        self
    }

    /// Enable access control.
    pub fn with_access_control(mut self, enabled: bool) -> Self {
        self.access_control_enabled = enabled;
        self
    }

    /// Enable audit logging.
    pub fn with_audit_logging(mut self, enabled: bool) -> Self {
        self.audit_logging_enabled = enabled;
        self
    }

    /// Enable lineage tracking.
    pub fn with_lineage_tracking(mut self, enabled: bool) -> Self {
        self.lineage_tracking_enabled = enabled;
        self
    }

    /// Enable multi-tenancy.
    pub fn with_multitenancy(mut self, enabled: bool) -> Self {
        self.multitenancy_enabled = enabled;
        self
    }

    /// Create a secure configuration with all features enabled.
    pub fn secure() -> Self {
        Self {
            encryption_enabled: true,
            access_control_enabled: true,
            audit_logging_enabled: true,
            lineage_tracking_enabled: true,
            multitenancy_enabled: true,
        }
    }

    /// Create a minimal configuration.
    pub fn minimal() -> Self {
        Self {
            encryption_enabled: true,
            access_control_enabled: false,
            audit_logging_enabled: false,
            lineage_tracking_enabled: false,
            multitenancy_enabled: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_config() {
        let config = SecurityConfig::new()
            .with_encryption(true)
            .with_access_control(true);

        assert!(config.encryption_enabled);
        assert!(config.access_control_enabled);
    }

    #[test]
    fn test_secure_config() {
        let config = SecurityConfig::secure();
        assert!(config.encryption_enabled);
        assert!(config.access_control_enabled);
        assert!(config.audit_logging_enabled);
        assert!(config.lineage_tracking_enabled);
        assert!(config.multitenancy_enabled);
    }

    #[test]
    fn test_minimal_config() {
        let config = SecurityConfig::minimal();
        assert!(config.encryption_enabled);
        assert!(!config.access_control_enabled);
        assert!(!config.audit_logging_enabled);
    }
}
