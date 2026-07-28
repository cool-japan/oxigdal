//! HIPAA compliance.

use crate::compliance::{ComplianceCheckResult, ComplianceStandard};

/// HIPAA compliance checker.
pub struct HipaaCompliance {
    encryption_enabled: bool,
    access_controls_enabled: bool,
    audit_controls_enabled: bool,
    integrity_controls_enabled: bool,
    transmission_security_enabled: bool,
}

impl HipaaCompliance {
    /// Create new HIPAA compliance checker.
    ///
    /// All safeguards start disabled; enable the ones your deployment satisfies with the
    /// `with_*` builder methods before calling [`Self::check`].
    pub fn new() -> Self {
        Self {
            encryption_enabled: false,
            access_controls_enabled: false,
            audit_controls_enabled: false,
            integrity_controls_enabled: false,
            transmission_security_enabled: false,
        }
    }

    /// Declare that PHI is encrypted at rest (§164.312(a)(2)(iv)).
    pub fn with_encryption(mut self, enabled: bool) -> Self {
        self.encryption_enabled = enabled;
        self
    }

    /// Declare that access controls are configured (§164.312(a)(1)).
    pub fn with_access_controls(mut self, enabled: bool) -> Self {
        self.access_controls_enabled = enabled;
        self
    }

    /// Declare that audit controls are enabled (§164.312(b)).
    pub fn with_audit_controls(mut self, enabled: bool) -> Self {
        self.audit_controls_enabled = enabled;
        self
    }

    /// Declare that integrity controls protect PHI from improper alteration (§164.312(c)(1)).
    pub fn with_integrity_controls(mut self, enabled: bool) -> Self {
        self.integrity_controls_enabled = enabled;
        self
    }

    /// Declare that transmission security is in place (§164.312(e)(1)).
    pub fn with_transmission_security(mut self, enabled: bool) -> Self {
        self.transmission_security_enabled = enabled;
        self
    }

    /// Check compliance.
    pub fn check(&self) -> ComplianceCheckResult {
        let mut issues = Vec::new();
        let mut recommendations = Vec::new();

        if !self.encryption_enabled {
            issues.push("PHI encryption not enabled".to_string());
            recommendations.push("Enable encryption for all PHI".to_string());
        }

        if !self.access_controls_enabled {
            issues.push("Access controls not properly configured".to_string());
            recommendations.push("Implement role-based access controls".to_string());
        }

        if !self.audit_controls_enabled {
            issues.push("Audit controls not enabled".to_string());
            recommendations.push("Enable comprehensive audit logging".to_string());
        }

        if !self.integrity_controls_enabled {
            issues.push("Integrity controls not enabled".to_string());
            recommendations.push(
                "Implement mechanisms to protect PHI from improper alteration/destruction"
                    .to_string(),
            );
        }

        if !self.transmission_security_enabled {
            issues.push("Transmission security not enabled".to_string());
            recommendations.push(
                "Encrypt PHI in transit (e.g. TLS) to enforce transmission security".to_string(),
            );
        }

        ComplianceCheckResult {
            standard: ComplianceStandard::HIPAA,
            compliant: issues.is_empty(),
            issues,
            recommendations,
        }
    }
}

impl Default for HipaaCompliance {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hipaa_defaults_flag_all_five_safeguards() {
        let result = HipaaCompliance::new().check();
        assert!(!result.compliant);
        assert_eq!(result.issues.len(), 5);
    }

    #[test]
    fn test_hipaa_fully_configured_is_compliant() {
        let result = HipaaCompliance::new()
            .with_encryption(true)
            .with_access_controls(true)
            .with_audit_controls(true)
            .with_integrity_controls(true)
            .with_transmission_security(true)
            .check();
        assert!(result.compliant);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_hipaa_integrity_and_transmission_are_actually_checked() {
        // The three originally-checked safeguards satisfied, but the two that used to be
        // silently ignored must still be reported.
        let result = HipaaCompliance::new()
            .with_encryption(true)
            .with_access_controls(true)
            .with_audit_controls(true)
            .check();
        assert!(!result.compliant);
        assert_eq!(result.issues.len(), 2);
    }
}
