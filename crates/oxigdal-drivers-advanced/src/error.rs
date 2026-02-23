//! Error types for advanced format drivers.

use std::io;

/// Result type for advanced format operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur when working with advanced geospatial formats.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// JPEG2000 format error
    #[error("JPEG2000 error: {0}")]
    Jpeg2000(String),

    /// GeoPackage format error
    #[error("GeoPackage error: {0}")]
    GeoPackage(String),

    /// KML format error
    #[error("KML error: {0}")]
    Kml(String),

    /// KMZ format error
    #[error("KMZ error: {0}")]
    Kmz(String),

    /// GML format error
    #[error("GML error: {0}")]
    Gml(String),

    /// XML parsing error
    #[error("XML parsing error: {0}")]
    XmlParse(#[from] quick_xml::Error),

    /// SQLite database error
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// ZIP archive error
    #[error("ZIP error: {0}")]
    Zip(#[from] oxiarc_core::error::OxiArcError),

    /// Invalid format error
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    /// Unsupported feature error
    #[error("Unsupported feature: {0}")]
    UnsupportedFeature(String),

    /// Validation error
    #[error("Validation error: {0}")]
    Validation(String),

    /// Encoding error
    #[error("Encoding error: {0}")]
    Encoding(String),

    /// Decoding error
    #[error("Decoding error: {0}")]
    Decoding(String),

    /// Compression error
    #[error("Compression error: {0}")]
    Compression(String),

    /// Decompression error
    #[error("Decompression error: {0}")]
    Decompression(String),

    /// Missing required field
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Invalid coordinate reference system
    #[error("Invalid CRS: {0}")]
    InvalidCrs(String),

    /// Geometry error
    #[error("Geometry error: {0}")]
    Geometry(String),

    /// Metadata error
    #[error("Metadata error: {0}")]
    Metadata(String),

    /// UTF-8 conversion error
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    /// String conversion error
    #[error("String conversion error: {0}")]
    Utf8Str(#[from] std::str::Utf8Error),

    /// Base64 decode error
    #[error("Base64 decode error: {0}")]
    Base64Decode(#[from] base64::DecodeError),

    /// Parse integer error
    #[error("Parse integer error: {0}")]
    ParseInt(#[from] std::num::ParseIntError),

    /// Parse float error
    #[error("Parse float error: {0}")]
    ParseFloat(#[from] std::num::ParseFloatError),

    /// JSON error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Custom error with context
    #[error("{context}: {source}")]
    WithContext {
        /// Error context
        context: String,
        /// Source error
        source: Box<Error>,
    },
}

impl Error {
    /// Add context to an error.
    pub fn with_context<S: Into<String>>(self, context: S) -> Self {
        Self::WithContext {
            context: context.into(),
            source: Box::new(self),
        }
    }

    /// Create a JPEG2000 error.
    pub fn jpeg2000<S: Into<String>>(msg: S) -> Self {
        Self::Jpeg2000(msg.into())
    }

    /// Create a GeoPackage error.
    pub fn geopackage<S: Into<String>>(msg: S) -> Self {
        Self::GeoPackage(msg.into())
    }

    /// Create a KML error.
    pub fn kml<S: Into<String>>(msg: S) -> Self {
        Self::Kml(msg.into())
    }

    /// Create a KMZ error.
    pub fn kmz<S: Into<String>>(msg: S) -> Self {
        Self::Kmz(msg.into())
    }

    /// Create a GML error.
    pub fn gml<S: Into<String>>(msg: S) -> Self {
        Self::Gml(msg.into())
    }

    /// Create an invalid format error.
    pub fn invalid_format<S: Into<String>>(msg: S) -> Self {
        Self::InvalidFormat(msg.into())
    }

    /// Create an unsupported feature error.
    pub fn unsupported<S: Into<String>>(msg: S) -> Self {
        Self::UnsupportedFeature(msg.into())
    }

    /// Create a validation error.
    pub fn validation<S: Into<String>>(msg: S) -> Self {
        Self::Validation(msg.into())
    }

    /// Create an encoding error.
    pub fn encoding<S: Into<String>>(msg: S) -> Self {
        Self::Encoding(msg.into())
    }

    /// Create a decoding error.
    pub fn decoding<S: Into<String>>(msg: S) -> Self {
        Self::Decoding(msg.into())
    }

    /// Create a missing field error.
    pub fn missing_field<S: Into<String>>(field: S) -> Self {
        Self::MissingField(field.into())
    }

    /// Create a geometry error.
    pub fn geometry<S: Into<String>>(msg: S) -> Self {
        Self::Geometry(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = Error::jpeg2000("test error");
        assert!(matches!(err, Error::Jpeg2000(_)));

        let err = Error::geopackage("gpkg error");
        assert!(matches!(err, Error::GeoPackage(_)));

        let err = Error::kml("kml error");
        assert!(matches!(err, Error::Kml(_)));
    }

    #[test]
    fn test_error_with_context() {
        let err = Error::jpeg2000("base error").with_context("reading file");
        assert!(matches!(err, Error::WithContext { .. }));
    }

    #[test]
    fn test_error_display() {
        let err = Error::validation("invalid data");
        let display = format!("{}", err);
        assert!(display.contains("Validation error"));
    }
}
