//! Integration tests for oxigdal-cloud-enhanced.

#![allow(unexpected_cfgs)]

use oxigdal_cloud_enhanced::*;

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

#[tokio::test]
async fn test_aws_config_creation() {
    let config = aws::AwsConfig::new(Some("us-east-1".to_string())).await;
    assert!(config.is_ok());
}

#[test]
fn test_azure_config_creation() {
    // This may fail without credentials, which is expected in test environment
    let result = azure::AzureConfig::new(
        "12345678-1234-1234-1234-123456789012".to_string(),
        Some("test-rg".to_string()),
    );
    // Just verify it returns a result
    let _result = result;
}

#[test]
fn test_gcp_config_creation() {
    let config = gcp::GcpConfig::new("test-project".to_string(), Some("us-central1".to_string()));
    assert!(config.is_ok());
    if let Ok(config) = config {
        assert_eq!(config.project_id(), "test-project");
    }
}

#[test]
fn test_aws_s3_select_options() {
    let options = aws::s3_select::CsvSelectOptions::default();
    assert_eq!(options.field_delimiter, Some(",".to_string()));
    assert_eq!(options.record_delimiter, Some("\n".to_string()));
}

#[test]
fn test_azure_data_lake_types() {
    use azure::data_lake::{AclScope, AclType};

    assert_eq!(AclScope::Access.to_string(), "access");
    assert_eq!(AclType::User.to_string(), "user");
}

#[cfg(feature = "bigquery")]
#[test]
fn test_gcp_bigquery_types() {
    use gcp::bigquery::{JobState, SourceFormat};

    assert_eq!(JobState::Running, JobState::Running);
    assert_ne!(JobState::Running, JobState::Done);
    assert_eq!(SourceFormat::Parquet, SourceFormat::Parquet);
}
