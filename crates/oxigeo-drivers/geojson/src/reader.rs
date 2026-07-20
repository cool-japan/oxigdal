//! GeoJSON reader implementation
//!
//! This module provides efficient reading and parsing of GeoJSON files
//! with support for streaming large files, GeoJSONL (newline-delimited GeoJSON),
//! and spatial filtering via bounding-box predicates.

use std::io::{BufRead, Read};
use std::marker::PhantomData;
use std::path::Path;

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

    /// Creates an iterator over features in a FeatureCollection
    ///
    /// Note: this reads and fully parses the entire underlying document up
    /// front (it is not incremental/streaming — see [`FeatureIterator`] for
    /// details); iteration itself, however, yields features one at a time
    /// without cloning the whole collection into the caller's hands at once.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying reader fails or the document isn't
    /// valid GeoJSON. Callers that need to distinguish "empty collection" from
    /// "read/parse failure" should use this fallible constructor rather than
    /// treating an empty iterator as success.
    pub fn iter_features(self) -> Result<FeatureIterator<R>> {
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

/// Iterator over features in a `FeatureCollection`
///
/// # Memory profile
///
/// This implementation reads the whole underlying reader into memory and
/// fully deserializes the `FeatureCollection` up front (in `FeatureIterator::new`);
/// it has the same peak-memory profile as [`GeoJsonReader::read_feature_collection`].
/// It is **not** an incremental/streaming JSON parser. The name and per-item
/// [`Iterator`] interface are kept because they let callers process features
/// one at a time (e.g. bail out early, or avoid holding a second collection),
/// but large files are still fully buffered and parsed before the first
/// feature is yielded.
///
/// For genuinely constant-memory processing of very large files, a real
/// streaming JSON parser (e.g. token-by-token, only materializing one
/// `Feature` at a time) would be required; that is tracked as future work.
pub struct FeatureIterator<R: Read> {
    #[allow(dead_code)] // Reserved for future streaming optimization
    buffer: Vec<u8>,
    features: Vec<Feature>,
    current_index: usize,
    validator: Option<Validator>,
    _phantom: PhantomData<R>,
}

impl<R: Read> FeatureIterator<R> {
    fn new(mut reader: R, validator: Option<Validator>) -> Result<Self> {
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer)?;

        // Parse the FeatureCollection, propagating I/O and JSON-parse errors
        // instead of silently substituting an empty feature list: a truncated
        // read or malformed document must be distinguishable from a
        // legitimately-empty, well-formed FeatureCollection.
        let fc: FeatureCollection = serde_json::from_slice(&buffer)?;

        Ok(Self {
            buffer,
            features: fc.features,
            current_index: 0,
            validator,
            _phantom: PhantomData,
        })
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

// ─── GeoJSONL / newline-delimited GeoJSON ─────────────────────────────────────

/// Read all features from a GeoJSON-seq / newline-delimited GeoJSON stream.
///
/// Each non-empty line must be a valid GeoJSON `Feature` object.
/// Blank lines and lines starting with `//` are skipped (comment lines).
///
/// # Errors
///
/// Returns the first I/O error or the first JSON-parse error encountered,
/// including the 1-based line number in the error message.
pub fn read_geojsonl<R: BufRead>(reader: R) -> Result<Vec<Feature>> {
    let mut features = Vec::new();
    for (idx, line_result) in reader.lines().enumerate() {
        let line = line_result.map_err(|e| {
            GeoJsonError::Io(std::io::Error::new(
                e.kind(),
                format!("I/O error at line {}: {e}", idx + 1),
            ))
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        let feature: Feature =
            serde_json::from_str(trimmed).map_err(|e| GeoJsonError::JsonParse {
                message: format!("line {}: {e}", idx + 1),
                line: Some(idx + 1),
                column: Some(e.column()),
            })?;
        features.push(feature);
    }
    Ok(features)
}

/// Open a GeoJSON file by path.
///
/// If the path has a `.geojsonl`, `.ndjson`, or `.jsonl` extension the file is
/// read as newline-delimited GeoJSON and wrapped in a synthetic
/// `FeatureCollection`.  Otherwise the whole file is parsed as standard GeoJSON
/// and returned as a [`GeoJsonDocument`].
///
/// # Errors
///
/// Returns an error when the file cannot be opened or the content is invalid.
pub fn open<P: AsRef<Path>>(path: P) -> Result<GeoJsonDocument> {
    let path = path.as_ref();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);

    let is_nd = matches!(
        ext.as_deref(),
        Some("geojsonl") | Some("ndjson") | Some("jsonl")
    );

    if is_nd {
        let features = open_geojsonl(path)?;
        Ok(GeoJsonDocument::FeatureCollection(FeatureCollection::new(
            features,
        )))
    } else {
        let file = std::fs::File::open(path)?;
        let buf_reader = std::io::BufReader::new(file);
        let mut reader = GeoJsonReader::new(buf_reader);
        reader.read()
    }
}

/// Open a GeoJSON-seq / newline-delimited GeoJSON file by path.
///
/// Each non-empty, non-comment line is parsed as a standalone GeoJSON `Feature`.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or any line contains invalid
/// GeoJSON.
pub fn open_geojsonl<P: AsRef<Path>>(path: P) -> Result<Vec<Feature>> {
    let file = std::fs::File::open(path.as_ref())?;
    let buf_reader = std::io::BufReader::new(file);
    read_geojsonl(buf_reader)
}

// ─── Spatial filtering ────────────────────────────────────────────────────────

/// Compute the tight axis-aligned bounding box of a geometry.
///
/// Returns `(min_x, min_y, max_x, max_y)` or `None` for empty geometries.
pub fn geometry_bbox(geom: &Geometry) -> Option<(f64, f64, f64, f64)> {
    let bbox_vec = geom.compute_bbox()?;
    if bbox_vec.len() >= 4 {
        Some((bbox_vec[0], bbox_vec[1], bbox_vec[2], bbox_vec[3]))
    } else {
        None
    }
}

/// Test whether a geometry's bounding box intersects the query rectangle.
///
/// Returns `false` when the geometry has no computable bounding box.
pub fn feature_bbox_intersects(
    geom: &Geometry,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
) -> bool {
    match geometry_bbox(geom) {
        None => false,
        Some((gmin_x, gmin_y, gmax_x, gmax_y)) => {
            // Axis-aligned rectangle overlap test: NOT (one is entirely to the
            // side / above / below the other).
            !(gmax_x < min_x || gmin_x > max_x || gmax_y < min_y || gmin_y > max_y)
        }
    }
}

/// Read a `FeatureCollection` from the reader and return only those features
/// whose geometry bounding box intersects the supplied query rectangle.
///
/// This is a full-scan filter (no spatial index).  Features without geometry
/// are excluded.
///
/// # Errors
///
/// Returns any error produced while reading the underlying GeoJSON document.
pub fn features_in_bbox<R: Read>(
    reader: &mut GeoJsonReader<R>,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
) -> Result<Vec<Feature>> {
    let fc = reader.read_feature_collection()?;
    let filtered = fc
        .features
        .into_iter()
        .filter(|f| {
            f.geometry
                .as_ref()
                .is_some_and(|g| feature_bbox_intersects(g, min_x, min_y, max_x, max_y))
        })
        .collect();
    Ok(filtered)
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
    fn test_iter_features_success() {
        let json = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "geometry": {"type": "Point", "coordinates": [100.0, 0.0]},
                    "properties": {"name": "Point 1"}
                },
                {
                    "type": "Feature",
                    "geometry": {"type": "Point", "coordinates": [101.0, 1.0]},
                    "properties": {"name": "Point 2"}
                }
            ]
        }"#;

        let cursor = Cursor::new(json.as_bytes());
        let reader = GeoJsonReader::new(cursor);
        let iter = reader.iter_features().expect("valid feature collection");
        let features: Result<Vec<Feature>> = iter.collect();
        let features = features.expect("all features valid");
        assert_eq!(features.len(), 2);
    }

    #[test]
    fn test_iter_features_malformed_json_returns_error_not_empty_iterator() {
        // Truncated/invalid JSON must surface as an error from `iter_features`,
        // not silently produce a zero-feature iterator indistinguishable from
        // a legitimately-empty FeatureCollection.
        let json = r#"{"type": "FeatureCollection", "features": [ { "type": "Fea"#;
        let cursor = Cursor::new(json.as_bytes());
        let reader = GeoJsonReader::new(cursor);
        let result = reader.iter_features();
        assert!(result.is_err());
    }

    #[test]
    fn test_iter_features_non_feature_collection_json_returns_error() {
        let json = r#"{"type": "Point", "coordinates": [1.0, 2.0]}"#;
        let cursor = Cursor::new(json.as_bytes());
        let reader = GeoJsonReader::new(cursor);
        let result = reader.iter_features();
        assert!(result.is_err());
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
