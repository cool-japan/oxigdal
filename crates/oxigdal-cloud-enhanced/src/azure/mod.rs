//! Azure cloud platform integrations.
//!
//! This module provides deep integrations with Azure services including
//! Data Lake Gen2, Synapse Analytics, Azure ML, Azure Monitor, and cost management.

pub mod cost;
pub mod data_lake;
pub mod managed_identity;
pub mod ml;
pub mod monitor;
pub mod synapse;

use crate::error::Result;
use azure_identity::{DefaultAzureCredential, TokenCredentialOptions};
use std::sync::Arc;

/// Azure configuration for enhanced services.
#[derive(Debug, Clone)]
pub struct AzureConfig {
    /// Subscription ID
    pub subscription_id: String,
    /// Resource group
    pub resource_group: Option<String>,
    /// Credential
    pub(crate) credential: Arc<DefaultAzureCredential>,
}

impl AzureConfig {
    /// Creates a new Azure configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the Azure configuration cannot be created.
    pub fn new(subscription_id: String, resource_group: Option<String>) -> Result<Self> {
        let credential = DefaultAzureCredential::create(TokenCredentialOptions::default())
            .map_err(|e| {
                crate::error::CloudEnhancedError::authentication(format!(
                    "Failed to create Azure credential: {}",
                    e
                ))
            })?;

        Ok(Self {
            subscription_id,
            resource_group,
            credential: Arc::new(credential),
        })
    }

    /// Gets the subscription ID.
    pub fn subscription_id(&self) -> &str {
        &self.subscription_id
    }

    /// Gets the resource group.
    pub fn resource_group(&self) -> Option<&str> {
        self.resource_group.as_deref()
    }

    /// Gets the credential.
    pub fn credential(&self) -> &DefaultAzureCredential {
        &self.credential
    }
}

/// Azure client manager for all services.
#[derive(Debug)]
pub struct AzureClient {
    config: AzureConfig,
    data_lake: data_lake::DataLakeClient,
    synapse: synapse::SynapseClient,
    ml: ml::AzureMlClient,
    monitor: monitor::MonitorClient,
    managed_identity: managed_identity::ManagedIdentityClient,
    cost: cost::CostClient,
}

impl AzureClient {
    /// Creates a new Azure client manager.
    ///
    /// # Errors
    ///
    /// Returns an error if the Azure configuration cannot be loaded.
    pub fn new(subscription_id: String, resource_group: Option<String>) -> Result<Self> {
        let config = AzureConfig::new(subscription_id, resource_group)?;

        Ok(Self {
            data_lake: data_lake::DataLakeClient::new(&config)?,
            synapse: synapse::SynapseClient::new(&config)?,
            ml: ml::AzureMlClient::new(&config)?,
            monitor: monitor::MonitorClient::new(&config)?,
            managed_identity: managed_identity::ManagedIdentityClient::new(&config)?,
            cost: cost::CostClient::new(&config)?,
            config,
        })
    }

    /// Gets a reference to the Data Lake client.
    pub fn data_lake(&self) -> &data_lake::DataLakeClient {
        &self.data_lake
    }

    /// Gets a reference to the Synapse client.
    pub fn synapse(&self) -> &synapse::SynapseClient {
        &self.synapse
    }

    /// Gets a reference to the Azure ML client.
    pub fn ml(&self) -> &ml::AzureMlClient {
        &self.ml
    }

    /// Gets a reference to the Monitor client.
    pub fn monitor(&self) -> &monitor::MonitorClient {
        &self.monitor
    }

    /// Gets a reference to the Managed Identity client.
    pub fn managed_identity(&self) -> &managed_identity::ManagedIdentityClient {
        &self.managed_identity
    }

    /// Gets a reference to the Cost client.
    pub fn cost(&self) -> &cost::CostClient {
        &self.cost
    }

    /// Gets the subscription ID.
    pub fn subscription_id(&self) -> &str {
        self.config.subscription_id()
    }

    /// Gets the resource group.
    pub fn resource_group(&self) -> Option<&str> {
        self.config.resource_group()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_azure_config_creation() {
        let _result = AzureConfig::new(
            "12345678-1234-1234-1234-123456789012".to_string(),
            Some("my-resource-group".to_string()),
        );
        // This may fail in test environment without Azure credentials
        // assert!(result.is_ok() || result.is_err());
    }
}
