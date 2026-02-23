//! Deep cloud platform integrations for AWS, Azure, and GCP.
//!
//! This crate provides enhanced cloud platform integrations beyond basic storage,
//! including analytics, ML services, cost optimization, and monitoring.

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod error;

pub mod aws;
pub mod azure;
pub mod gcp;

pub use error::{CloudEnhancedError, Result};

/// Cloud provider type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloudProvider {
    /// Amazon Web Services
    Aws,
    /// Microsoft Azure
    Azure,
    /// Google Cloud Platform
    Gcp,
}

impl CloudProvider {
    /// Returns the name of the cloud provider.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Aws => "AWS",
            Self::Azure => "Azure",
            Self::Gcp => "GCP",
        }
    }
}

impl std::fmt::Display for CloudProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Cloud resource type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceType {
    /// Storage (S3, Blob, GCS)
    Storage,
    /// Analytics/Query service (Athena, Synapse, BigQuery)
    Analytics,
    /// Data catalog (Glue, Purview, Data Catalog)
    DataCatalog,
    /// ML service (SageMaker, Azure ML, Vertex AI)
    MachineLearning,
    /// Monitoring (CloudWatch, Monitor, Cloud Monitoring)
    Monitoring,
    /// Serverless compute (Lambda, Functions, Cloud Functions)
    ServerlessCompute,
}

impl ResourceType {
    /// Returns the name of the resource type.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Storage => "Storage",
            Self::Analytics => "Analytics",
            Self::DataCatalog => "Data Catalog",
            Self::MachineLearning => "Machine Learning",
            Self::Monitoring => "Monitoring",
            Self::ServerlessCompute => "Serverless Compute",
        }
    }
}

impl std::fmt::Display for ResourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cloud_provider_name() {
        assert_eq!(CloudProvider::Aws.name(), "AWS");
        assert_eq!(CloudProvider::Azure.name(), "Azure");
        assert_eq!(CloudProvider::Gcp.name(), "GCP");
    }

    #[test]
    fn test_resource_type_name() {
        assert_eq!(ResourceType::Storage.name(), "Storage");
        assert_eq!(ResourceType::Analytics.name(), "Analytics");
        assert_eq!(ResourceType::DataCatalog.name(), "Data Catalog");
    }
}
