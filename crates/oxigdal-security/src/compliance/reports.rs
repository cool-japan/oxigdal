//! Compliance reporting.

use crate::compliance::ComplianceCheckResult;
use serde::{Deserialize, Serialize};

/// Compliance report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReport {
    /// Report timestamp.
    pub generated_at: chrono::DateTime<chrono::Utc>,
    /// Compliance results.
    pub results: Vec<ComplianceCheckResult>,
    /// Overall compliant.
    pub overall_compliant: bool,
}

impl ComplianceReport {
    /// Create new compliance report.
    pub fn new(results: Vec<ComplianceCheckResult>) -> Self {
        let overall_compliant = results.iter().all(|r| r.compliant);

        Self {
            generated_at: chrono::Utc::now(),
            results,
            overall_compliant,
        }
    }

    /// Generate report text.
    pub fn to_text(&self) -> String {
        let mut text = format!("Compliance Report\nGenerated: {}\n\n", self.generated_at);

        for result in &self.results {
            text.push_str(&format!(
                "{:?}: {}\n",
                result.standard,
                if result.compliant {
                    "COMPLIANT"
                } else {
                    "NON-COMPLIANT"
                }
            ));

            if !result.issues.is_empty() {
                text.push_str("Issues:\n");
                for issue in &result.issues {
                    text.push_str(&format!("  - {}\n", issue));
                }
            }
        }

        text
    }
}
