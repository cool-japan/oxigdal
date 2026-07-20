//! GDPR compliance.

use crate::compliance::{ComplianceCheckResult, ComplianceStandard};
use serde::{Deserialize, Serialize};

/// GDPR data subject rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataSubjectRight {
    /// Right to access.
    Access,
    /// Right to rectification.
    Rectification,
    /// Right to erasure ("right to be forgotten").
    Erasure,
    /// Right to restrict processing.
    Restriction,
    /// Right to data portability.
    Portability,
    /// Right to object.
    Object,
}

/// GDPR compliance checker.
pub struct GdprCompliance {
    encryption_enabled: bool,
    audit_logging_enabled: bool,
    consent_management_enabled: bool,
    data_retention_policy: bool,
}

impl GdprCompliance {
    /// Create new GDPR compliance checker.
    pub fn new() -> Self {
        Self {
            encryption_enabled: false,
            audit_logging_enabled: false,
            consent_management_enabled: false,
            data_retention_policy: false,
        }
    }

    /// Enable encryption.
    pub fn with_encryption(mut self, enabled: bool) -> Self {
        self.encryption_enabled = enabled;
        self
    }

    /// Enable audit logging.
    pub fn with_audit_logging(mut self, enabled: bool) -> Self {
        self.audit_logging_enabled = enabled;
        self
    }

    /// Enable consent management.
    pub fn with_consent_management(mut self, enabled: bool) -> Self {
        self.consent_management_enabled = enabled;
        self
    }

    /// Enable data retention policy.
    pub fn with_data_retention_policy(mut self, enabled: bool) -> Self {
        self.data_retention_policy = enabled;
        self
    }

    /// Check compliance.
    pub fn check(&self) -> ComplianceCheckResult {
        let mut issues = Vec::new();
        let mut recommendations = Vec::new();

        if !self.encryption_enabled {
            issues.push("Encryption not enabled for personal data".to_string());
            recommendations.push("Enable encryption at rest for all personal data".to_string());
        }

        if !self.audit_logging_enabled {
            issues.push("Audit logging not enabled".to_string());
            recommendations.push("Enable comprehensive audit logging".to_string());
        }

        if !self.consent_management_enabled {
            issues.push("Consent management not enabled".to_string());
            recommendations.push("Implement consent management system".to_string());
        }

        if !self.data_retention_policy {
            issues.push("Data retention policy not defined".to_string());
            recommendations.push("Define and implement data retention policy".to_string());
        }

        ComplianceCheckResult {
            standard: ComplianceStandard::GDPR,
            compliant: issues.is_empty(),
            issues,
            recommendations,
        }
    }
}

impl Default for GdprCompliance {
    fn default() -> Self {
        Self::new()
    }
}

/// Data subject access request (DSAR).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSubjectAccessRequest {
    /// Request ID.
    pub id: String,
    /// Data subject identifier.
    pub data_subject_id: String,
    /// Rights being exercised.
    pub rights: Vec<DataSubjectRight>,
    /// Request timestamp.
    pub requested_at: chrono::DateTime<chrono::Utc>,
    /// Status.
    pub status: DsarStatus,
}

/// DSAR status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DsarStatus {
    /// Pending.
    Pending,
    /// Processing.
    Processing,
    /// Completed.
    Completed,
    /// Rejected.
    Rejected,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gdpr_compliance_check() {
        let checker = GdprCompliance::new()
            .with_encryption(true)
            .with_audit_logging(true)
            .with_consent_management(false)
            .with_data_retention_policy(true);

        let result = checker.check();
        assert!(!result.compliant);
        assert_eq!(result.issues.len(), 1);
    }
}
