//! Compliance framework.

pub mod fedramp;
pub mod gdpr;
pub mod hipaa;
pub mod reports;

use serde::{Deserialize, Serialize};

/// Compliance standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComplianceStandard {
    /// GDPR (EU).
    GDPR,
    /// HIPAA (US Healthcare).
    HIPAA,
    /// FedRAMP (US Federal).
    FedRAMP,
    /// SOC 2.
    SOC2,
    /// ISO 27001.
    ISO27001,
}

/// Compliance check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceCheckResult {
    /// Standard being checked.
    pub standard: ComplianceStandard,
    /// Compliant or not.
    pub compliant: bool,
    /// Issues found.
    pub issues: Vec<String>,
    /// Recommendations.
    pub recommendations: Vec<String>,
}
