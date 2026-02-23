//! Integration tests for compliance.

use oxigdal_security::compliance::{gdpr::GdprCompliance, reports::ComplianceReport};

#[test]
fn test_gdpr_compliance() {
    let checker = GdprCompliance::new()
        .with_encryption(true)
        .with_audit_logging(true)
        .with_consent_management(true)
        .with_data_retention_policy(true);

    let result = checker.check();
    assert!(result.compliant);
    assert_eq!(result.issues.len(), 0);
}

#[test]
fn test_compliance_report() {
    let gdpr = GdprCompliance::new()
        .with_encryption(true)
        .with_audit_logging(true)
        .with_consent_management(true)
        .with_data_retention_policy(true);

    let report = ComplianceReport::new(vec![gdpr.check()]);
    assert!(report.overall_compliant);

    let text = report.to_text();
    assert!(text.contains("COMPLIANT"));
}
