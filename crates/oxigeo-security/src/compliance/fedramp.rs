//! FedRAMP compliance.

use crate::compliance::{ComplianceCheckResult, ComplianceStandard};

/// FedRAMP compliance checker.
pub struct FedRampCompliance {
    encryption_enabled: bool,
    mfa_enabled: bool,
    incident_response_plan: bool,
    continuous_monitoring: bool,
}

impl FedRampCompliance {
    /// Create new FedRAMP compliance checker.
    ///
    /// All controls start disabled; enable the ones your deployment satisfies with the
    /// `with_*` builder methods before calling [`Self::check`].
    pub fn new() -> Self {
        Self {
            encryption_enabled: false,
            mfa_enabled: false,
            incident_response_plan: false,
            continuous_monitoring: false,
        }
    }

    /// Declare that FIPS 140-2 validated encryption is enabled.
    pub fn with_encryption(mut self, enabled: bool) -> Self {
        self.encryption_enabled = enabled;
        self
    }

    /// Declare that multi-factor authentication is enforced for all users.
    pub fn with_mfa(mut self, enabled: bool) -> Self {
        self.mfa_enabled = enabled;
        self
    }

    /// Declare that a documented incident-response plan is in place.
    pub fn with_incident_response_plan(mut self, enabled: bool) -> Self {
        self.incident_response_plan = enabled;
        self
    }

    /// Declare that continuous monitoring (ConMon) is operating.
    pub fn with_continuous_monitoring(mut self, enabled: bool) -> Self {
        self.continuous_monitoring = enabled;
        self
    }

    /// Check compliance.
    pub fn check(&self) -> ComplianceCheckResult {
        let mut issues = Vec::new();
        let mut recommendations = Vec::new();

        if !self.encryption_enabled {
            issues.push("FIPS 140-2 encryption not enabled".to_string());
            recommendations.push("Enable FIPS 140-2 validated encryption".to_string());
        }

        if !self.mfa_enabled {
            issues.push("Multi-factor authentication not enabled".to_string());
            recommendations.push("Implement MFA for all users".to_string());
        }

        if !self.incident_response_plan {
            issues.push("Incident response plan not established".to_string());
            recommendations
                .push("Document and exercise an incident response plan (IR family)".to_string());
        }

        if !self.continuous_monitoring {
            issues.push("Continuous monitoring not enabled".to_string());
            recommendations
                .push("Establish a continuous monitoring (ConMon) program (CA-7)".to_string());
        }

        ComplianceCheckResult {
            standard: ComplianceStandard::FedRAMP,
            compliant: issues.is_empty(),
            issues,
            recommendations,
        }
    }
}

impl Default for FedRampCompliance {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fedramp_defaults_non_compliant_with_all_controls_flagged() {
        let result = FedRampCompliance::new().check();
        assert!(!result.compliant);
        // All four controls must be reported, not just the two that used to be checked.
        assert_eq!(result.issues.len(), 4);
    }

    #[test]
    fn test_fedramp_fully_configured_is_compliant() {
        let result = FedRampCompliance::new()
            .with_encryption(true)
            .with_mfa(true)
            .with_incident_response_plan(true)
            .with_continuous_monitoring(true)
            .check();
        assert!(result.compliant);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_fedramp_partial_reports_remaining_gaps() {
        let result = FedRampCompliance::new()
            .with_encryption(true)
            .with_mfa(true)
            .check();
        assert!(!result.compliant);
        // Encryption + MFA satisfied, IR plan + ConMon still missing.
        assert_eq!(result.issues.len(), 2);
    }
}
