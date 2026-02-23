//! CSW (Catalog Service for the Web) 2.0.2 implementation
//!
//! Provides OGC-compliant Catalog Service supporting:
//! - GetCapabilities: Service metadata
//! - GetRecords: Metadata search
//! - GetRecordById: Metadata retrieval
//!
//! # Standards
//!
//! - OGC CSW 2.0.2

pub mod capabilities;
pub mod records;

use crate::error::{ServiceError, ServiceResult};
use axum::{
    extract::{Query, State},
    response::Response,
};
use serde::Deserialize;
use std::sync::Arc;

/// CSW service state
#[derive(Clone)]
pub struct CswState {
    /// Service metadata
    pub service_info: Arc<ServiceInfo>,
    /// Metadata records
    pub records: Arc<dashmap::DashMap<String, MetadataRecord>>,
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

/// Metadata record
#[derive(Debug, Clone)]
pub struct MetadataRecord {
    /// Record identifier
    pub identifier: String,
    /// Record title
    pub title: String,
    /// Record abstract/description
    pub abstract_text: Option<String>,
    /// Keywords
    pub keywords: Vec<String>,
    /// Bounding box (minx, miny, maxx, maxy)
    pub bbox: Option<(f64, f64, f64, f64)>,
}

/// CSW request parameters
#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct CswRequest {
    /// Service name (must be "CSW")
    pub service: Option<String>,
    /// CSW version
    pub version: Option<String>,
    /// Request operation
    pub request: String,
    /// Additional parameters
    #[serde(flatten)]
    pub params: serde_json::Value,
}

impl CswState {
    /// Create new CSW service state
    pub fn new(service_info: ServiceInfo) -> Self {
        Self {
            service_info: Arc::new(service_info),
            records: Arc::new(dashmap::DashMap::new()),
        }
    }

    /// Add a metadata record
    pub fn add_record(&self, record: MetadataRecord) -> ServiceResult<()> {
        self.records.insert(record.identifier.clone(), record);
        Ok(())
    }
}

/// Main CSW request handler
pub async fn handle_csw_request(
    State(state): State<CswState>,
    Query(params): Query<CswRequest>,
) -> Result<Response, ServiceError> {
    if let Some(ref service) = params.service {
        if service.to_uppercase() != "CSW" {
            return Err(ServiceError::InvalidParameter(
                "SERVICE".to_string(),
                format!("Expected 'CSW', got '{}'", service),
            ));
        }
    }

    match params.request.to_uppercase().as_str() {
        "GETCAPABILITIES" => {
            let version = params.version.as_deref().unwrap_or("2.0.2");
            capabilities::handle_get_capabilities(&state, version).await
        }
        "GETRECORDS" => {
            let version = params.version.as_deref().unwrap_or("2.0.2");
            records::handle_get_records(&state, version, &params.params).await
        }
        "GETRECORDBYID" => {
            let version = params.version.as_deref().unwrap_or("2.0.2");
            records::handle_get_record_by_id(&state, version, &params.params).await
        }
        _ => Err(ServiceError::UnsupportedOperation(params.request.clone())),
    }
}
