//! SLO compliance reporting.

use super::{Slo, SloStatus};
use crate::error::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// SLO compliance report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReport {
    /// Report generation timestamp.
    pub generated_at: DateTime<Utc>,

    /// Start of the report period.
    pub period_start: DateTime<Utc>,
    /// End of the report period.
    pub period_end: DateTime<Utc>,

    /// SLO statuses.
    pub slo_statuses: Vec<SloStatus>,

    /// Overall compliance percentage.
    pub overall_compliance: f64,

    /// Number of SLOs met.
    pub slos_met: usize,

    /// Number of SLOs not met.
    pub slos_not_met: usize,
}

impl ComplianceReport {
    /// Create a new compliance report.
    pub fn new(
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
        slo_statuses: Vec<SloStatus>,
    ) -> Self {
        let slos_met = slo_statuses.iter().filter(|s| s.is_met).count();
        let slos_not_met = slo_statuses.len() - slos_met;
        let overall_compliance = if !slo_statuses.is_empty() {
            (slos_met as f64 / slo_statuses.len() as f64) * 100.0
        } else {
            0.0
        };

        Self {
            generated_at: Utc::now(),
            period_start,
            period_end,
            slo_statuses,
            overall_compliance,
            slos_met,
            slos_not_met,
        }
    }

    /// Export report as JSON.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(crate::error::ObservabilityError::Serialization)
    }
}

/// Report generator for SLOs.
pub struct ReportGenerator;

impl ReportGenerator {
    /// Generate a compliance report for the given SLOs.
    pub fn generate(
        _slos: &[Slo],
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
    ) -> Result<ComplianceReport> {
        // Placeholder - would query actual metrics
        let slo_statuses = Vec::new();
        Ok(ComplianceReport::new(
            period_start,
            period_end,
            slo_statuses,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slo::ErrorBudget;

    #[test]
    fn test_compliance_report() {
        let statuses = vec![
            SloStatus::new(
                "slo1".to_string(),
                99.95,
                99.9,
                ErrorBudget::from_target(99.9),
            ),
            SloStatus::new(
                "slo2".to_string(),
                99.85,
                99.9,
                ErrorBudget::from_target(99.9),
            ),
        ];

        let report = ComplianceReport::new(Utc::now(), Utc::now(), statuses);
        assert_eq!(report.slos_met, 1);
        assert_eq!(report.slos_not_met, 1);
        assert_eq!(report.overall_compliance, 50.0);
    }
}
