//! Error types for OGC web services
//!
//! Provides comprehensive error handling for WFS, WCS, WPS, and CSW services
//! with OGC-compliant exception responses.

use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use thiserror::Error;

/// Service errors for OGC web services
#[derive(Debug, Error)]
pub enum ServiceError {
    /// Invalid request parameter
    #[error("Invalid parameter '{0}': {1}")]
    InvalidParameter(String, String),

    /// Missing required parameter
    #[error("Missing required parameter: {0}")]
    MissingParameter(String),

    /// Feature/layer not found
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// Invalid CRS reference
    #[error("Invalid CRS: {0}")]
    InvalidCrs(String),

    /// Invalid bounding box
    #[error("Invalid bounding box: {0}")]
    InvalidBbox(String),

    /// Unsupported format
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    /// Unsupported operation
    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),

    /// Invalid XML request
    #[error("Invalid XML: {0}")]
    InvalidXml(String),

    /// Invalid GeoJSON
    #[error("Invalid GeoJSON: {0}")]
    InvalidGeoJson(String),

    /// Transaction error
    #[error("Transaction error: {0}")]
    Transaction(String),

    /// Process execution error
    #[error("Process execution error: {0}")]
    ProcessExecution(String),

    /// Coverage access error
    #[error("Coverage error: {0}")]
    Coverage(String),

    /// Catalog search error
    #[error("Catalog error: {0}")]
    Catalog(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// OxiGDAL error
    #[error("OxiGDAL error: {0}")]
    OxiGdal(#[from] oxigdal_core::OxiGdalError),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// XML writing error
    #[error("XML error: {0}")]
    Xml(String),

    /// Internal server error
    #[error("Internal server error: {0}")]
    Internal(String),
}

impl From<quick_xml::Error> for ServiceError {
    fn from(err: quick_xml::Error) -> Self {
        ServiceError::Xml(err.to_string())
    }
}

impl From<serde_json::Error> for ServiceError {
    fn from(err: serde_json::Error) -> Self {
        ServiceError::Serialization(err.to_string())
    }
}

impl From<geojson::Error> for ServiceError {
    fn from(err: geojson::Error) -> Self {
        ServiceError::InvalidGeoJson(err.to_string())
    }
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let (status, exception_code, exception_text) = match &self {
            ServiceError::InvalidParameter(_, _) | ServiceError::MissingParameter(_) => (
                StatusCode::BAD_REQUEST,
                "InvalidParameterValue",
                self.to_string(),
            ),
            ServiceError::NotFound(_) => (StatusCode::NOT_FOUND, "NotFound", self.to_string()),
            ServiceError::UnsupportedOperation(_) | ServiceError::UnsupportedFormat(_) => (
                StatusCode::BAD_REQUEST,
                "OperationNotSupported",
                self.to_string(),
            ),
            ServiceError::InvalidXml(_) | ServiceError::InvalidGeoJson(_) => {
                (StatusCode::BAD_REQUEST, "InvalidRequest", self.to_string())
            }
            ServiceError::InvalidCrs(_) => {
                (StatusCode::BAD_REQUEST, "InvalidCRS", self.to_string())
            }
            ServiceError::InvalidBbox(_) => (
                StatusCode::BAD_REQUEST,
                "InvalidBoundingBox",
                self.to_string(),
            ),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "NoApplicableCode",
                self.to_string(),
            ),
        };

        // Return OGC ServiceExceptionReport format
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<ExceptionReport version="2.0.0"
    xmlns="http://www.opengis.net/ows/2.0"
    xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
    xsi:schemaLocation="http://www.opengis.net/ows/2.0 http://schemas.opengis.net/ows/2.0/owsExceptionReport.xsd">
  <Exception exceptionCode="{}">
    <ExceptionText>{}</ExceptionText>
  </Exception>
</ExceptionReport>"#,
            exception_code, exception_text
        );

        (status, [(header::CONTENT_TYPE, "application/xml")], xml).into_response()
    }
}

/// Result type for service operations
pub type ServiceResult<T> = Result<T, ServiceError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ServiceError::MissingParameter("VERSION".to_string());
        assert_eq!(err.to_string(), "Missing required parameter: VERSION");

        let err = ServiceError::InvalidParameter("BBOX".to_string(), "malformed".to_string());
        assert_eq!(err.to_string(), "Invalid parameter 'BBOX': malformed");
    }

    #[test]
    fn test_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let svc_err: ServiceError = io_err.into();
        assert!(matches!(svc_err, ServiceError::Io(_)));
    }
}
