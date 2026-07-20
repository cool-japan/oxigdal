//! Security scanning.

pub mod malware;
pub mod secrets;
pub mod vulnerability;

use serde::{Deserialize, Serialize};

/// Scan result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    /// Scan type.
    pub scan_type: ScanType,
    /// Findings.
    pub findings: Vec<Finding>,
    /// Scan timestamp.
    pub scanned_at: chrono::DateTime<chrono::Utc>,
}

/// Scan type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanType {
    /// Vulnerability scan.
    Vulnerability,
    /// Secret detection.
    Secrets,
    /// Malware scan.
    Malware,
}

/// Security finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// Finding ID.
    pub id: String,
    /// Severity.
    pub severity: Severity,
    /// Description.
    pub description: String,
    /// Location.
    pub location: Option<String>,
}

/// Finding severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    /// Informational.
    Info,
    /// Low severity.
    Low,
    /// Medium severity.
    Medium,
    /// High severity.
    High,
    /// Critical severity.
    Critical,
}
