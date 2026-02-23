//! WFS (Web Feature Service) 2.0/3.0 implementation
//!
//! Provides OGC-compliant Web Feature Service supporting:
//! - GetCapabilities: Service metadata
//! - DescribeFeatureType: Schema information
//! - GetFeature: Feature retrieval with filtering
//! - Transaction: Feature insert, update, delete operations
//!
//! # Standards
//!
//! - OGC WFS 2.0.0 (ISO 19142:2010)
//! - OGC WFS 3.0 (OGC API - Features Part 1: Core)
//!
//! # Example
//!
//! ```no_run
//! use oxigdal_services::wfs::{ServiceInfo, WfsState};
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let info = ServiceInfo {
//!     title: "My WFS Service".to_string(),
//!     abstract_text: None,
//!     provider: "Provider".to_string(),
//!     service_url: "http://localhost:8080/wfs".to_string(),
//!     versions: vec!["2.0.0".to_string()],
//! };
//! let service = WfsState::new(info);
//! // Add feature types and handle requests
//! # Ok(())
//! # }
//! ```

pub mod capabilities;
pub mod database;
pub mod features;
pub mod transactions;

use crate::error::{ServiceError, ServiceResult};
use axum::{
    extract::{Query, State},
    response::Response,
};
use serde::Deserialize;
use std::sync::Arc;

/// WFS service state
#[derive(Clone)]
pub struct WfsState {
    /// Service metadata
    pub service_info: Arc<ServiceInfo>,
    /// Feature type registry
    pub feature_types: Arc<dashmap::DashMap<String, FeatureTypeInfo>>,
    /// Transaction support enabled
    pub transactions_enabled: bool,
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

/// Feature type information
#[derive(Debug, Clone)]
pub struct FeatureTypeInfo {
    /// Type name
    pub name: String,
    /// Title
    pub title: String,
    /// Abstract
    pub abstract_text: Option<String>,
    /// Default CRS
    pub default_crs: String,
    /// Other supported CRS
    pub other_crs: Vec<String>,
    /// Bounding box (minx, miny, maxx, maxy)
    pub bbox: Option<(f64, f64, f64, f64)>,
    /// Feature source
    pub source: FeatureSource,
}

/// Feature data source
#[derive(Debug, Clone)]
pub enum FeatureSource {
    /// File-based source (GeoJSON, Shapefile, etc.)
    File(std::path::PathBuf),
    /// Database source (PostGIS, etc.) with connection string (legacy)
    Database(String),
    /// Database source with full configuration
    DatabaseSource(database::DatabaseSource),
    /// In-memory features
    Memory(Vec<geojson::Feature>),
}

// Re-export database types
pub use database::{
    BboxFilter, CacheStats, CountCacheConfig, CountResult, CqlFilter, DatabaseFeatureCounter,
    DatabaseSource, DatabaseType,
};

/// WFS request parameters
#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct WfsRequest {
    /// Service name (must be "WFS")
    pub service: Option<String>,
    /// WFS version
    pub version: Option<String>,
    /// Request operation
    pub request: String,
    /// Additional parameters
    #[serde(flatten)]
    pub params: serde_json::Value,
}

impl WfsState {
    /// Create new WFS service state
    pub fn new(service_info: ServiceInfo) -> Self {
        Self {
            service_info: Arc::new(service_info),
            feature_types: Arc::new(dashmap::DashMap::new()),
            transactions_enabled: false,
        }
    }

    /// Add a feature type
    pub fn add_feature_type(&self, info: FeatureTypeInfo) -> ServiceResult<()> {
        self.feature_types.insert(info.name.clone(), info);
        Ok(())
    }

    /// Get feature type by name
    pub fn get_feature_type(&self, name: &str) -> Option<FeatureTypeInfo> {
        self.feature_types
            .get(name)
            .map(|entry| entry.value().clone())
    }

    /// Enable transaction support
    pub fn enable_transactions(&mut self) {
        self.transactions_enabled = true;
    }
}

/// Main WFS request handler
pub async fn handle_wfs_request(
    State(state): State<WfsState>,
    Query(params): Query<WfsRequest>,
) -> Result<Response, ServiceError> {
    // Validate service parameter
    if let Some(ref service) = params.service {
        if service.to_uppercase() != "WFS" {
            return Err(ServiceError::InvalidParameter(
                "SERVICE".to_string(),
                format!("Expected 'WFS', got '{}'", service),
            ));
        }
    }

    // Route to appropriate handler based on request type
    match params.request.to_uppercase().as_str() {
        "GETCAPABILITIES" => {
            let version = params.version.as_deref().unwrap_or("2.0.0");
            capabilities::handle_get_capabilities(&state, version).await
        }
        "DESCRIBEFEATURETYPE" => {
            let version = params.version.as_deref().unwrap_or("2.0.0");
            features::handle_describe_feature_type(&state, version, &params.params).await
        }
        "GETFEATURE" => {
            let version = params.version.as_deref().unwrap_or("2.0.0");
            features::handle_get_feature(&state, version, &params.params).await
        }
        "TRANSACTION" => {
            if !state.transactions_enabled {
                return Err(ServiceError::UnsupportedOperation(
                    "Transactions not enabled".to_string(),
                ));
            }
            let version = params.version.as_deref().unwrap_or("2.0.0");
            transactions::handle_transaction(&state, version, &params.params).await
        }
        _ => Err(ServiceError::UnsupportedOperation(params.request.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wfs_state_creation() {
        let info = ServiceInfo {
            title: "Test WFS".to_string(),
            abstract_text: Some("Test service".to_string()),
            provider: "COOLJAPAN OU".to_string(),
            service_url: "http://localhost/wfs".to_string(),
            versions: vec!["2.0.0".to_string()],
        };

        let state = WfsState::new(info);
        assert_eq!(state.service_info.title, "Test WFS");
        assert!(!state.transactions_enabled);
    }

    #[test]
    fn test_add_feature_type() {
        let info = ServiceInfo {
            title: "Test WFS".to_string(),
            abstract_text: None,
            provider: "COOLJAPAN OU".to_string(),
            service_url: "http://localhost/wfs".to_string(),
            versions: vec!["2.0.0".to_string()],
        };

        let state = WfsState::new(info);

        let feature_type = FeatureTypeInfo {
            name: "test_layer".to_string(),
            title: "Test Layer".to_string(),
            abstract_text: None,
            default_crs: "EPSG:4326".to_string(),
            other_crs: vec![],
            bbox: Some((-180.0, -90.0, 180.0, 90.0)),
            source: FeatureSource::Memory(vec![]),
        };

        assert!(
            state.add_feature_type(feature_type).is_ok(),
            "Failed to add feature type"
        );

        let retrieved = state.get_feature_type("test_layer");
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved.as_ref().map(|ft| &ft.name),
            Some(&"test_layer".to_string())
        );
    }
}
