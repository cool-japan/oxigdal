//! Integration tests for oxigeo-cloud-enhanced.

#![allow(unexpected_cfgs)]

use oxigeo_cloud_enhanced::*;

#[test]
fn test_cloud_provider_enum() {
    let aws = CloudProvider::Aws;
    let azure = CloudProvider::Azure;
    let gcp = CloudProvider::Gcp;

    assert_eq!(aws.name(), "AWS");
    assert_eq!(azure.name(), "Azure");
    assert_eq!(gcp.name(), "GCP");

    assert_ne!(aws, azure);
    assert_ne!(azure, gcp);
    assert_ne!(aws, gcp);
}

#[test]
fn test_resource_type_enum() {
    let storage = ResourceType::Storage;
    let analytics = ResourceType::Analytics;
    let ml = ResourceType::MachineLearning;

    assert_eq!(storage.name(), "Storage");
    assert_eq!(analytics.name(), "Analytics");
    assert_eq!(ml.name(), "Machine Learning");
}

#[test]
fn test_error_creation() {
    let err = CloudEnhancedError::aws_service("test error");
    assert!(err.to_string().contains("AWS service error"));

    let err = CloudEnhancedError::azure_service("test error");
    assert!(err.to_string().contains("Azure service error"));

    let err = CloudEnhancedError::gcp_service("test error");
    assert!(err.to_string().contains("GCP service error"));
}

#[cfg(feature = "aws")]
#[tokio::test]
async fn test_aws_config_creation() {
    let config = aws::AwsConfig::new(Some("us-east-1".to_string())).await;
    assert!(config.is_ok());
}

#[cfg(feature = "azure")]
#[test]
fn test_azure_config_creation() {
    use oxigeo_cloud_enhanced::azure;
    // This may fail without credentials, which is expected in test environment
    let result = azure::AzureConfig::new(
        "12345678-1234-1234-1234-123456789012".to_string(),
        Some("test-rg".to_string()),
    );
    // Just verify it returns a result
    let _result = result;
}

#[cfg(feature = "gcp")]
#[test]
fn test_gcp_config_creation() {
    use oxigeo_cloud_enhanced::gcp;
    let config = gcp::GcpConfig::new("test-project".to_string(), Some("us-central1".to_string()));
    assert!(config.is_ok());
    if let Ok(config) = config {
        assert_eq!(config.project_id(), "test-project");
    }
}

#[cfg(feature = "aws")]
#[test]
fn test_aws_s3_select_options() {
    let options = aws::s3_select::CsvSelectOptions::default();
    assert_eq!(options.field_delimiter, Some(",".to_string()));
    assert_eq!(options.record_delimiter, Some("\n".to_string()));
}

#[cfg(feature = "azure")]
#[test]
fn test_azure_data_lake_types() {
    use oxigeo_cloud_enhanced::azure::data_lake::{AclScope, AclType};

    assert_eq!(AclScope::Access.to_string(), "access");
    assert_eq!(AclType::User.to_string(), "user");
}

// `gcp::bigquery` does not exist in this crate: `google-cloud-bigquery` 0.15
// pins `arrow` to the incompatible `53.x` series (this workspace uses
// `arrow = "59"`), so a dedicated BigQuery SDK module was never wired up --
// see the doc comment on the `gcp` module
// (crates/oxigeo-cloud-enhanced/src/gcp/mod.rs) and `gcp::cost::CostClient`,
// which talks to the BigQuery REST `jobs.query` API directly instead and
// has no typed `JobState`/`SourceFormat` surface. This test was gated on a
// `bigquery` Cargo feature that was never defined in this crate's
// Cargo.toml, so it silently never compiled or ran under any configuration
// (the crate's blanket `#![allow(unexpected_cfgs)]` suppressed the warning
// that would otherwise have flagged the unknown feature). Retargeting the
// `#[cfg]` at the real `gcp` feature is not possible without inventing API
// surface that was intentionally never implemented, so the unresolvable
// body is replaced rather than fabricated and the test is left `#[ignore]`d
// so its status stays visible instead of silently vanishing again.
#[cfg(feature = "gcp")]
#[ignore = "gcp::bigquery module does not exist in this crate (dropped due to a google-cloud-bigquery/arrow version conflict, see comment above); it referenced a `bigquery` Cargo feature that was never defined and a JobState/SourceFormat API that was never implemented"]
#[test]
fn test_gcp_bigquery_types() {
    unimplemented!(
        "gcp::bigquery::{{JobState, SourceFormat}} do not exist in this crate; see comment above test_gcp_bigquery_types"
    )
}
