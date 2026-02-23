//! WPS (Web Processing Service) 2.0 implementation
//!
//! Provides OGC-compliant Web Processing Service supporting:
//! - GetCapabilities: Service and process metadata
//! - DescribeProcess: Process input/output descriptions
//! - Execute: Process execution (synchronous and asynchronous)
//!
//! # Standards
//!
//! - OGC WPS 2.0.0
//!
//! # Example
//!
//! ```no_run
//! use oxigdal_services::wps::{ServiceInfo, WpsState};
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let info = ServiceInfo {
//!     title: "My WPS Service".to_string(),
//!     abstract_text: None,
//!     provider: "Provider".to_string(),
//!     service_url: "http://localhost:8080/wps".to_string(),
//!     versions: vec!["2.0.0".to_string()],
//! };
//! let service = WpsState::new(info);
//! // Add processes and handle requests
//! # Ok(())
//! # }
//! ```

pub mod builtin;
pub mod capabilities;
pub mod processes;

use crate::error::{ServiceError, ServiceResult};
use async_trait::async_trait;
use axum::{
    extract::{Query, State},
    response::Response,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// WPS service state
#[derive(Clone)]
pub struct WpsState {
    /// Service metadata
    pub service_info: Arc<ServiceInfo>,
    /// Process registry
    pub processes: Arc<dashmap::DashMap<String, Arc<dyn Process>>>,
}

/// Service metadata
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    /// Service title
    pub title: String,
    /// Service abstract/description
    pub abstract_text: Option<String>,
    /// Service provider
    pub provider: String,
    /// Service URL
    pub service_url: String,
    /// Supported versions
    pub versions: Vec<String>,
}

/// Process trait
#[async_trait]
pub trait Process: Send + Sync {
    /// Get process identifier
    fn identifier(&self) -> &str;

    /// Get process title
    fn title(&self) -> &str;

    /// Get process abstract
    fn abstract_text(&self) -> Option<&str> {
        None
    }

    /// Get input descriptions
    fn inputs(&self) -> Vec<InputDescription>;

    /// Get output descriptions
    fn outputs(&self) -> Vec<OutputDescription>;

    /// Execute process
    async fn execute(&self, inputs: ProcessInputs) -> ServiceResult<ProcessOutputs>;
}

/// Input description
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputDescription {
    /// Input identifier
    pub identifier: String,
    /// Input title
    pub title: String,
    /// Input abstract
    pub abstract_text: Option<String>,
    /// Data type
    pub data_type: DataType,
    /// Minimum occurrences
    pub min_occurs: usize,
    /// Maximum occurrences (None = unbounded)
    pub max_occurs: Option<usize>,
}

/// Output description
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputDescription {
    /// Output identifier
    pub identifier: String,
    /// Output title
    pub title: String,
    /// Output abstract
    pub abstract_text: Option<String>,
    /// Data type
    pub data_type: DataType,
}

/// Data type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataType {
    /// Literal data (string, number, etc.)
    Literal(LiteralDataType),
    /// Complex data (GeoJSON, GeoTIFF, etc.)
    Complex(ComplexDataType),
    /// Bounding box
    BoundingBox,
}

/// Literal data type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiteralDataType {
    /// Data type (integer, double, string, etc.)
    pub data_type: String,
    /// Allowed values
    pub allowed_values: Option<Vec<String>>,
}

/// Complex data type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexDataType {
    /// MIME type
    pub mime_type: String,
    /// Encoding
    pub encoding: Option<String>,
    /// Schema
    pub schema: Option<String>,
}

/// Process inputs
#[derive(Debug, Clone, Default)]
pub struct ProcessInputs {
    /// Input values
    pub inputs: dashmap::DashMap<String, Vec<InputValue>>,
}

/// Input value
#[derive(Debug, Clone)]
pub enum InputValue {
    /// Literal value
    Literal(String),
    /// Complex data
    Complex(Vec<u8>),
    /// Reference (URL)
    Reference(String),
    /// Bounding box
    BoundingBox {
        /// Lower corner (x, y)
        lower: (f64, f64),
        /// Upper corner (x, y)
        upper: (f64, f64),
        /// CRS
        crs: Option<String>,
    },
}

/// Process outputs
#[derive(Debug, Clone, Default)]
pub struct ProcessOutputs {
    /// Output values
    pub outputs: dashmap::DashMap<String, OutputValue>,
}

/// Output value
#[derive(Debug, Clone)]
pub enum OutputValue {
    /// Literal value
    Literal(String),
    /// Complex data
    Complex(Vec<u8>),
    /// Reference (URL to result)
    Reference(String),
}

/// WPS request parameters
#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct WpsRequest {
    /// Service name (must be "WPS")
    pub service: Option<String>,
    /// WPS version
    pub version: Option<String>,
    /// Request operation
    pub request: String,
    /// Additional parameters
    #[serde(flatten)]
    pub params: serde_json::Value,
}

impl WpsState {
    /// Create new WPS service state
    pub fn new(service_info: ServiceInfo) -> Self {
        let state = Self {
            service_info: Arc::new(service_info),
            processes: Arc::new(dashmap::DashMap::new()),
        };

        // Register built-in processes
        builtin::register_builtin_processes(&state);

        state
    }

    /// Add a process
    pub fn add_process(&self, process: Arc<dyn Process>) -> ServiceResult<()> {
        self.processes
            .insert(process.identifier().to_string(), process);
        Ok(())
    }

    /// Get process by identifier
    pub fn get_process(&self, identifier: &str) -> Option<Arc<dyn Process>> {
        self.processes
            .get(identifier)
            .map(|entry| Arc::clone(entry.value()))
    }
}

/// Main WPS request handler
pub async fn handle_wps_request(
    State(state): State<WpsState>,
    Query(params): Query<WpsRequest>,
) -> Result<Response, ServiceError> {
    // Validate service parameter
    if let Some(ref service) = params.service {
        if service.to_uppercase() != "WPS" {
            return Err(ServiceError::InvalidParameter(
                "SERVICE".to_string(),
                format!("Expected 'WPS', got '{}'", service),
            ));
        }
    }

    // Route to appropriate handler based on request type
    match params.request.to_uppercase().as_str() {
        "GETCAPABILITIES" => {
            let version = params.version.as_deref().unwrap_or("2.0.0");
            capabilities::handle_get_capabilities(&state, version).await
        }
        "DESCRIBEPROCESS" => {
            let version = params.version.as_deref().unwrap_or("2.0.0");
            processes::handle_describe_process(&state, version, &params.params).await
        }
        "EXECUTE" => {
            let version = params.version.as_deref().unwrap_or("2.0.0");
            processes::handle_execute(&state, version, &params.params).await
        }
        _ => Err(ServiceError::UnsupportedOperation(params.request.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wps_state_creation() {
        let info = ServiceInfo {
            title: "Test WPS".to_string(),
            abstract_text: Some("Test service".to_string()),
            provider: "COOLJAPAN OU".to_string(),
            service_url: "http://localhost/wps".to_string(),
            versions: vec!["2.0.0".to_string()],
        };

        let state = WpsState::new(info);
        assert_eq!(state.service_info.title, "Test WPS");
        // Built-in processes should be registered
        assert!(!state.processes.is_empty());
    }
}
