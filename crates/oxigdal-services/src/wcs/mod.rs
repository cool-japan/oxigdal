//! WCS (Web Coverage Service) 2.0 implementation
//!
//! Provides OGC-compliant Web Coverage Service supporting:
//! - GetCapabilities: Service metadata
//! - DescribeCoverage: Coverage schema and structure
//! - GetCoverage: Raster data retrieval with subsetting and format conversion
//!
//! # Standards
//!
//! - OGC WCS 2.0.1 Core
//! - OGC WCS 2.0 GeoTIFF Coverage Encoding
//! - OGC WCS 2.0 Range Subsetting Extension
//!
//! # Example
//!
//! ```no_run
//! use oxigdal_services::wcs::{ServiceInfo, WcsState};
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let info = ServiceInfo {
//!     title: "My WCS Service".to_string(),
//!     abstract_text: None,
//!     provider: "Provider".to_string(),
//!     service_url: "http://localhost:8080/wcs".to_string(),
//!     versions: vec!["2.0.1".to_string()],
//! };
//! let service = WcsState::new(info);
//! // Add coverages and handle requests
//! # Ok(())
//! # }
//! ```

pub mod capabilities;
pub mod coverage;

use crate::error::{ServiceError, ServiceResult};
use axum::{
    extract::{Query, State},
    response::Response,
};
use serde::Deserialize;
use std::sync::Arc;

/// WCS service state
#[derive(Clone)]
pub struct WcsState {
    /// Service metadata
    pub service_info: Arc<ServiceInfo>,
    /// Coverage registry
    pub coverages: Arc<dashmap::DashMap<String, CoverageInfo>>,
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

/// Coverage information
#[derive(Debug, Clone)]
pub struct CoverageInfo {
    /// Coverage identifier
    pub coverage_id: String,
    /// Title
    pub title: String,
    /// Abstract
    pub abstract_text: Option<String>,
    /// Native CRS
    pub native_crs: String,
    /// Bounding box in native CRS (minx, miny, maxx, maxy)
    pub bbox: (f64, f64, f64, f64),
    /// Grid dimensions (width, height)
    pub grid_size: (usize, usize),
    /// Grid origin (x, y)
    pub grid_origin: (f64, f64),
    /// Grid resolution (x, y)
    pub grid_resolution: (f64, f64),
    /// Number of bands
    pub band_count: usize,
    /// Band names
    pub band_names: Vec<String>,
    /// Data type
    pub data_type: String,
    /// Coverage source
    pub source: CoverageSource,
    /// Supported formats
    pub formats: Vec<String>,
}

/// Coverage data source
#[derive(Debug, Clone)]
pub enum CoverageSource {
    /// File-based source (GeoTIFF, NetCDF, etc.)
    File(std::path::PathBuf),
    /// Remote URL
    Url(String),
    /// In-memory coverage
    Memory,
}

/// WCS request parameters
#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct WcsRequest {
    /// Service name (must be "WCS")
    pub service: Option<String>,
    /// WCS version
    pub version: Option<String>,
    /// Request operation
    pub request: String,
    /// Additional parameters
    #[serde(flatten)]
    pub params: serde_json::Value,
}

impl WcsState {
    /// Create new WCS service state
    pub fn new(service_info: ServiceInfo) -> Self {
        Self {
            service_info: Arc::new(service_info),
            coverages: Arc::new(dashmap::DashMap::new()),
        }
    }

    /// Add a coverage
    pub fn add_coverage(&self, info: CoverageInfo) -> ServiceResult<()> {
        self.coverages.insert(info.coverage_id.clone(), info);
        Ok(())
    }

    /// Get coverage by ID
    pub fn get_coverage(&self, coverage_id: &str) -> Option<CoverageInfo> {
        self.coverages
            .get(coverage_id)
            .map(|entry| entry.value().clone())
    }
}

/// Main WCS request handler
pub async fn handle_wcs_request(
    State(state): State<WcsState>,
    Query(params): Query<WcsRequest>,
) -> Result<Response, ServiceError> {
    // Validate service parameter
    if let Some(ref service) = params.service {
        if service.to_uppercase() != "WCS" {
            return Err(ServiceError::InvalidParameter(
                "SERVICE".to_string(),
                format!("Expected 'WCS', got '{}'", service),
            ));
        }
    }

    // Route to appropriate handler based on request type
    match params.request.to_uppercase().as_str() {
        "GETCAPABILITIES" => {
            let version = params.version.as_deref().unwrap_or("2.0.1");
            capabilities::handle_get_capabilities(&state, version).await
        }
        "DESCRIBECOVERAGE" => {
            let version = params.version.as_deref().unwrap_or("2.0.1");
            coverage::handle_describe_coverage(&state, version, &params.params).await
        }
        "GETCOVERAGE" => {
            let version = params.version.as_deref().unwrap_or("2.0.1");
            coverage::handle_get_coverage(&state, version, &params.params).await
        }
        _ => Err(ServiceError::UnsupportedOperation(params.request.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wcs_state_creation() {
        let info = ServiceInfo {
            title: "Test WCS".to_string(),
            abstract_text: Some("Test service".to_string()),
            provider: "COOLJAPAN OU".to_string(),
            service_url: "http://localhost/wcs".to_string(),
            versions: vec!["2.0.1".to_string()],
        };

        let state = WcsState::new(info);
        assert_eq!(state.service_info.title, "Test WCS");
    }

    #[test]
    fn test_add_coverage() {
        let info = ServiceInfo {
            title: "Test WCS".to_string(),
            abstract_text: None,
            provider: "COOLJAPAN OU".to_string(),
            service_url: "http://localhost/wcs".to_string(),
            versions: vec!["2.0.1".to_string()],
        };

        let state = WcsState::new(info);

        let coverage = CoverageInfo {
            coverage_id: "test_coverage".to_string(),
            title: "Test Coverage".to_string(),
            abstract_text: None,
            native_crs: "EPSG:4326".to_string(),
            bbox: (-180.0, -90.0, 180.0, 90.0),
            grid_size: (1024, 512),
            grid_origin: (-180.0, 90.0),
            grid_resolution: (0.35, -0.35),
            band_count: 3,
            band_names: vec!["Red".to_string(), "Green".to_string(), "Blue".to_string()],
            data_type: "Byte".to_string(),
            source: CoverageSource::Memory,
            formats: vec!["image/tiff".to_string(), "image/png".to_string()],
        };

        assert!(
            state.add_coverage(coverage).is_ok(),
            "Failed to add coverage"
        );

        let retrieved = state.get_coverage("test_coverage");
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved.as_ref().map(|c| &c.coverage_id),
            Some(&"test_coverage".to_string())
        );
    }
}
