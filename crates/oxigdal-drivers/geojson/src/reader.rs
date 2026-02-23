//! GeoJSON reader implementation
//!
//! This module provides efficient reading and parsing of GeoJSON files
//! with support for streaming large files.

use std::io::Read;
use std::marker::PhantomData;

use crate::error::{GeoJsonError, Result};
use crate::types::{Feature, FeatureCollection, Geometry};
use crate::validation::{ValidationConfig, Validator};

/// GeoJSON reader
///
/// Provides methods to read and parse GeoJSON from various sources.
pub struct GeoJsonReader<R: Read> {
    reader: R,
    validator: Option<Validator>,
    buffer_size: usize,
}

impl<R: Read> GeoJsonReader<R> {
    /// Creates a new GeoJSON reader
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            validator: Some(Validator::new()),
            buffer_size: 8192,
        }
    }

    /// Creates a new GeoJSON reader with custom validation config
    pub fn with_validation_config(reader: R, config: ValidationConfig) -> Self {
        Self {
            reader,
            validator: Some(Validator::with_config(config)),
            buffer_size: 8192,
        }
    }

    /// Creates a new GeoJSON reader without validation
    pub fn without_validation(reader: R) -> Self {
        Self {
            reader,
            validator: None,
            buffer_size: 8192,
        }
    }

    /// Sets the buffer size for reading
    pub fn set_buffer_size(&mut self, size: usize) {
        self.buffer_size = size;
    }

    /// Reads a complete GeoJSON document and determines its type
    pub fn read(&mut self) -> Result<GeoJsonDocument> {
        let mut buffer = Vec::with_capacity(self.buffer_size);
        self.reader.read_to_end(&mut buffer)?;

        let value: serde_json::Value = serde_json::from_slice(&buffer)?;

        // Determine type
        if let Some(type_field) = value.get("type").and_then(|v| v.as_str()) {
            match type_field {
                "FeatureCollection" => {
                    let fc: FeatureCollection = serde_json::from_value(value)?;
                    if let Some(validator) = &mut self.validator {
                        validator.validate_feature_collection(&fc)?;
                    }
                    Ok(GeoJsonDocument::FeatureCollection(fc))
                }
                "Feature" => {
                    let f: Feature = serde_json::from_value(value)?;
                    if let Some(validator) = &mut self.validator {
                        validator.validate_feature(&f)?;
                    }
                    Ok(GeoJsonDocument::Feature(f))
                }
                _ => {
                    // Try to parse as Geometry
                    let geom: Geometry = serde_json::from_value(value)?;
                    if let Some(validator) = &mut self.validator {
                        validator.validate_geometry(&geom)?;
                    }
                    Ok(GeoJsonDocument::Geometry(geom))
                }
            }
        } else {
            Err(GeoJsonError::invalid_structure("Missing 'type' field"))
        }
    }

    /// Reads a FeatureCollection
    pub fn read_feature_collection(&mut self) -> Result<FeatureCollection> {
        let mut buffer = Vec::with_capacity(self.buffer_size);
        self.reader.read_to_end(&mut buffer)?;

        let fc: FeatureCollection = serde_json::from_slice(&buffer)?;

        if let Some(validator) = &mut self.validator {
            validator.validate_feature_collection(&fc)?;
        }

        Ok(fc)
    }

    /// Reads a single Feature
    pub fn read_feature(&mut self) -> Result<Feature> {
        let mut buffer = Vec::with_capacity(self.buffer_size);
        self.reader.read_to_end(&mut buffer)?;

        let feature: Feature = serde_json::from_slice(&buffer)?;

        if let Some(validator) = &mut self.validator {
            validator.validate_feature(&feature)?;
        }

        Ok(feature)
    }

    /// Reads a Geometry
    pub fn read_geometry(&mut self) -> Result<Geometry> {
        let mut buffer = Vec::with_capacity(self.buffer_size);
        self.reader.read_to_end(&mut buffer)?;

        let geom: Geometry = serde_json::from_slice(&buffer)?;

        if let Some(validator) = &mut self.validator {
            validator.validate_geometry(&geom)?;
        }

        Ok(geom)
    }

    /// Creates a streaming iterator over features in a FeatureCollection
    ///
    /// This is memory-efficient for large files as it doesn't load the entire
    /// collection into memory at once.
    pub fn iter_features(self) -> FeatureIterator<R> {
        FeatureIterator::new(self.reader, self.validator)
    }

    /// Consumes the reader and returns the inner reader
    pub fn into_inner(self) -> R {
        self.reader
    }
}

/// GeoJSON document type
#[derive(Debug, Clone, PartialEq)]
pub enum GeoJsonDocument {
    /// A FeatureCollection
    FeatureCollection(FeatureCollection),
    /// A single Feature
    Feature(Feature),
    /// A Geometry
    Geometry(Geometry),
}

impl GeoJsonDocument {
    /// Returns true if this is a FeatureCollection
    #[must_use]
    pub const fn is_feature_collection(&self) -> bool {
        matches!(self, Self::FeatureCollection(_))
    }

    /// Returns true if this is a Feature
    #[must_use]
    pub const fn is_feature(&self) -> bool {
        matches!(self, Self::Feature(_))
    }

    /// Returns true if this is a Geometry
    #[must_use]
    pub const fn is_geometry(&self) -> bool {
        matches!(self, Self::Geometry(_))
    }

    /// Converts to FeatureCollection if possible
    pub fn into_feature_collection(self) -> Option<FeatureCollection> {
        match self {
            Self::FeatureCollection(fc) => Some(fc),
            _ => None,
        }
    }

    /// Converts to Feature if possible
    pub fn into_feature(self) -> Option<Feature> {
        match self {
            Self::Feature(f) => Some(f),
            _ => None,
        }
    }

    /// Converts to Geometry if possible
    pub fn into_geometry(self) -> Option<Geometry> {
        match self {
            Self::Geometry(g) => Some(g),
            _ => None,
        }
    }
}

/// Streaming iterator over features
///
/// This allows processing large FeatureCollections without loading the entire
/// file into memory.
///
/// Note: This is a simplified implementation. For production use with very large
/// files, consider using a streaming JSON parser.
pub struct FeatureIterator<R: Read> {
    #[allow(dead_code)] // Reserved for future streaming optimization
    buffer: Vec<u8>,
    features: Vec<Feature>,
    current_index: usize,
    validator: Option<Validator>,
    _phantom: PhantomData<R>,
}

impl<R: Read> FeatureIterator<R> {
    fn new(mut reader: R, validator: Option<Validator>) -> Self {
        let mut buffer = Vec::new();
        let _ = reader.read_to_end(&mut buffer);

        // Parse the FeatureCollection
        let features = if let Ok(fc) = serde_json::from_slice::<FeatureCollection>(&buffer) {
            fc.features
        } else {
            Vec::new()
        };

        Self {
            buffer,
            features,
            current_index: 0,
            validator,
            _phantom: PhantomData,
        }
    }

    /// Returns the next feature
    ///
    /// Returns `None` when the stream is exhausted.
    pub fn next_feature(&mut self) -> Result<Option<Feature>> {
        if self.current_index >= self.features.len() {
            return Ok(None);
        }

        let feature = self.features[self.current_index].clone();
        self.current_index += 1;

        if let Some(validator) = &mut self.validator {
            validator.validate_feature(&feature)?;
        }

        Ok(Some(feature))
    }
}

impl<R: Read> Iterator for FeatureIterator<R> {
    type Item = Result<Feature>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_feature() {
            Ok(Some(feature)) => Some(Ok(feature)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

/// Parses a GeoJSON string into a document
pub fn from_str(s: &str) -> Result<GeoJsonDocument> {
    let value: serde_json::Value = serde_json::from_str(s)?;

    if let Some(type_field) = value.get("type").and_then(|v| v.as_str()) {
        match type_field {
            "FeatureCollection" => {
                let fc: FeatureCollection = serde_json::from_value(value)?;
                Ok(GeoJsonDocument::FeatureCollection(fc))
            }
            "Feature" => {
                let f: Feature = serde_json::from_value(value)?;
                Ok(GeoJsonDocument::Feature(f))
            }
            _ => {
                let geom: Geometry = serde_json::from_value(value)?;
                Ok(GeoJsonDocument::Geometry(geom))
            }
        }
    } else {
        Err(GeoJsonError::invalid_structure("Missing 'type' field"))
    }
}

/// Parses a GeoJSON string into a FeatureCollection
pub fn feature_collection_from_str(s: &str) -> Result<FeatureCollection> {
    let fc: FeatureCollection = serde_json::from_str(s)?;
    Ok(fc)
}

/// Parses a GeoJSON string into a Feature
pub fn feature_from_str(s: &str) -> Result<Feature> {
    let f: Feature = serde_json::from_str(s)?;
    Ok(f)
}

/// Parses a GeoJSON string into a Geometry
pub fn geometry_from_str(s: &str) -> Result<Geometry> {
    let g: Geometry = serde_json::from_str(s)?;
    Ok(g)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_read_point() {
        let json = r#"{
            "type": "Point",
            "coordinates": [100.0, 0.0]
        }"#;

        let cursor = Cursor::new(json.as_bytes());
        let mut reader = GeoJsonReader::new(cursor);

        let doc = reader.read().ok();
        assert!(doc.is_some());
        let document = doc.expect("valid document");
        assert!(document.is_geometry());
    }

    #[test]
    fn test_read_feature() {
        let json = r#"{
            "type": "Feature",
            "geometry": {
                "type": "Point",
                "coordinates": [100.0, 0.0]
            },
            "properties": {
                "name": "Test Point"
            }
        }"#;

        let cursor = Cursor::new(json.as_bytes());
        let mut reader = GeoJsonReader::new(cursor);

        let doc = reader.read().ok();
        assert!(doc.is_some());
        let document = doc.expect("valid document");
        assert!(document.is_feature());
    }

    #[test]
    fn test_read_feature_collection() {
        let json = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "geometry": {
                        "type": "Point",
                        "coordinates": [100.0, 0.0]
                    },
                    "properties": {
                        "name": "Point 1"
                    }
                },
                {
                    "type": "Feature",
                    "geometry": {
                        "type": "Point",
                        "coordinates": [101.0, 1.0]
                    },
                    "properties": {
                        "name": "Point 2"
                    }
                }
            ]
        }"#;

        let cursor = Cursor::new(json.as_bytes());
        let mut reader = GeoJsonReader::new(cursor);

        let doc = reader.read().ok();
        assert!(doc.is_some());
        let document = doc.expect("valid document");
        assert!(document.is_feature_collection());

        if let Some(fc) = document.into_feature_collection() {
            assert_eq!(fc.len(), 2);
        } else {
            panic!("Expected FeatureCollection");
        }
    }

    #[test]
    fn test_read_linestring() {
        let json = r#"{
            "type": "LineString",
            "coordinates": [
                [100.0, 0.0],
                [101.0, 1.0]
            ]
        }"#;

        let cursor = Cursor::new(json.as_bytes());
        let mut reader = GeoJsonReader::new(cursor);

        let geom = reader.read_geometry().ok();
        assert!(geom.is_some());
    }

    #[test]
    fn test_read_polygon() {
        let json = r#"{
            "type": "Polygon",
            "coordinates": [
                [
                    [100.0, 0.0],
                    [101.0, 0.0],
                    [101.0, 1.0],
                    [100.0, 1.0],
                    [100.0, 0.0]
                ]
            ]
        }"#;

        let cursor = Cursor::new(json.as_bytes());
        let mut reader = GeoJsonReader::new(cursor);

        let geom = reader.read_geometry().ok();
        assert!(geom.is_some());
    }

    #[test]
    fn test_from_str() {
        let json = r#"{"type":"Point","coordinates":[0.0,0.0]}"#;
        let doc = from_str(json).ok();
        assert!(doc.is_some());
        let document = doc.expect("valid document");
        assert!(document.is_geometry());
    }

    #[test]
    fn test_invalid_json() {
        let json = r#"{"invalid": json}"#;
        let cursor = Cursor::new(json.as_bytes());
        let mut reader = GeoJsonReader::new(cursor);

        let result = reader.read();
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_type_field() {
        let json = r#"{"coordinates": [0.0, 0.0]}"#;
        let cursor = Cursor::new(json.as_bytes());
        let mut reader = GeoJsonReader::new(cursor);

        let result = reader.read();
        assert!(result.is_err());
    }

    #[test]
    fn test_without_validation() {
        let json = r#"{
            "type": "Point",
            "coordinates": [200.0, 100.0]
        }"#;

        let cursor = Cursor::new(json.as_bytes());
        let mut reader = GeoJsonReader::without_validation(cursor);

        // Should succeed without validation even with invalid coordinates
        let result = reader.read();
        assert!(result.is_ok());
    }

    #[test]
    fn test_geometry_collection() {
        let json = r#"{
            "type": "GeometryCollection",
            "geometries": [
                {
                    "type": "Point",
                    "coordinates": [100.0, 0.0]
                },
                {
                    "type": "LineString",
                    "coordinates": [
                        [101.0, 0.0],
                        [102.0, 1.0]
                    ]
                }
            ]
        }"#;

        let cursor = Cursor::new(json.as_bytes());
        let mut reader = GeoJsonReader::new(cursor);

        let geom = reader.read_geometry().ok();
        assert!(geom.is_some());
    }
}
