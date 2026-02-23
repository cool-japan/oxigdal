//! Google Cloud Platform integrations.
//!
//! This module provides deep integrations with GCP services including
//! BigQuery GIS, Dataflow, Vertex AI, Cloud Monitoring, and cost optimization.

// TEMPORARY: BigQuery disabled due to arrow version incompatibility
// #[cfg(feature = "bigquery")]
// pub mod bigquery;
pub mod cost;
pub mod dataflow;
pub mod monitoring;
pub mod vertex_ai;
pub mod workload_identity;

use crate::error::Result;

/// GCP configuration for enhanced services.
#[derive(Debug, Clone)]
pub struct GcpConfig {
    /// Project ID
    pub project_id: String,
    /// Location/Region
    pub location: Option<String>,
}

impl GcpConfig {
    /// Creates a new GCP configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the GCP configuration cannot be created.
    pub fn new(project_id: String, location: Option<String>) -> Result<Self> {
        Ok(Self {
            project_id,
            location,
        })
    }

    /// Gets the project ID.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Gets the location.
    pub fn location(&self) -> Option<&str> {
        self.location.as_deref()
    }
}

/// GCP client manager for all services.
#[derive(Debug)]
pub struct GcpClient {
    config: GcpConfig,
    // TEMPORARY: BigQuery disabled due to arrow version incompatibility
    // #[cfg(feature = "bigquery")]
    // bigquery: bigquery::BigQueryClient,
    dataflow: dataflow::DataflowClient,
    vertex_ai: vertex_ai::VertexAiClient,
    monitoring: monitoring::MonitoringClient,
    workload_identity: workload_identity::WorkloadIdentityClient,
    cost: cost::CostClient,
}

impl GcpClient {
    /// Creates a new GCP client manager.
    ///
    /// # Errors
    ///
    /// Returns an error if the GCP configuration cannot be loaded.
    pub async fn new(project_id: String, location: Option<String>) -> Result<Self> {
        let config = GcpConfig::new(project_id, location)?;

        Ok(Self {
            // TEMPORARY: BigQuery disabled due to arrow version incompatibility
            // #[cfg(feature = "bigquery")]
            // bigquery: bigquery::BigQueryClient::new(&config).await?,
            dataflow: dataflow::DataflowClient::new(&config)?,
            vertex_ai: vertex_ai::VertexAiClient::new(&config)?,
            monitoring: monitoring::MonitoringClient::new(&config).await?,
            workload_identity: workload_identity::WorkloadIdentityClient::new(&config)?,
            cost: cost::CostClient::new(&config)?,
            config,
        })
    }

    // TEMPORARY: BigQuery disabled due to arrow version incompatibility
    // /// Gets a reference to the BigQuery client.
    // #[cfg(feature = "bigquery")]
    // pub fn bigquery(&self) -> &bigquery::BigQueryClient {
    //     &self.bigquery
    // }

    /// Gets a reference to the Dataflow client.
    pub fn dataflow(&self) -> &dataflow::DataflowClient {
        &self.dataflow
    }

    /// Gets a reference to the Vertex AI client.
    pub fn vertex_ai(&self) -> &vertex_ai::VertexAiClient {
        &self.vertex_ai
    }

    /// Gets a reference to the Monitoring client.
    pub fn monitoring(&self) -> &monitoring::MonitoringClient {
        &self.monitoring
    }

    /// Gets a reference to the Workload Identity client.
    pub fn workload_identity(&self) -> &workload_identity::WorkloadIdentityClient {
        &self.workload_identity
    }

    /// Gets a reference to the Cost client.
    pub fn cost(&self) -> &cost::CostClient {
        &self.cost
    }

    /// Gets the project ID.
    pub fn project_id(&self) -> &str {
        self.config.project_id()
    }

    /// Gets the location.
    pub fn location(&self) -> Option<&str> {
        self.config.location()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gcp_config_creation() {
        let config = GcpConfig::new(
            "my-project-123".to_string(),
            Some("us-central1".to_string()),
        );
        assert!(config.is_ok());
        if let Ok(config) = config {
            assert_eq!(config.project_id(), "my-project-123");
            assert_eq!(config.location(), Some("us-central1"));
        }
    }
}
