//! GeoParquet metadata structures
//!
//! This module implements the GeoParquet 1.0 metadata specification,
//! which defines how geospatial metadata is stored in Parquet file metadata.
//!
//! The GeoParquet metadata is stored in the file-level key-value metadata
//! under the "geo" key as a JSON object.

use crate::error::{GeoParquetError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// GeoParquet format version
pub const GEOPARQUET_VERSION: &str = "1.0.0";

/// Metadata key in Parquet file metadata
pub const GEOPARQUET_METADATA_KEY: &str = "geo";

/// GeoParquet metadata structure (root object)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeoParquetMetadata {
    /// GeoParquet specification version
    pub version: String,

    /// Primary geometry column name
    pub primary_column: String,

    /// Metadata for each geometry column
    pub columns: HashMap<String, GeometryColumnMetadata>,
}

impl GeoParquetMetadata {
    /// Creates new GeoParquet metadata
    pub fn new(primary_column: impl Into<String>) -> Self {
        Self {
            version: GEOPARQUET_VERSION.to_string(),
            primary_column: primary_column.into(),
            columns: HashMap::new(),
        }
    }

    /// Adds a geometry column
    pub fn add_column(
        &mut self,
        name: impl Into<String>,
        metadata: GeometryColumnMetadata,
    ) -> &mut Self {
        self.columns.insert(name.into(), metadata);
        self
    }

    /// Gets metadata for a geometry column
    pub fn get_column(&self, name: &str) -> Option<&GeometryColumnMetadata> {
        self.columns.get(name)
    }

    /// Gets metadata for the primary geometry column
    pub fn primary_column_metadata(&self) -> Result<&GeometryColumnMetadata> {
        self.columns
            .get(&self.primary_column)
            .ok_or_else(|| GeoParquetError::missing_field(&self.primary_column))
    }

    /// Validates the metadata
    pub fn validate(&self) -> Result<()> {
        // Check version
        if self.version != GEOPARQUET_VERSION {
            return Err(GeoParquetError::invalid_metadata(format!(
                "Unsupported GeoParquet version: {} (expected {})",
                self.version, GEOPARQUET_VERSION
            )));
        }

        // Check primary column exists
        if !self.columns.contains_key(&self.primary_column) {
            return Err(GeoParquetError::missing_field(&self.primary_column));
        }

        // Validate each column
        for (name, column) in &self.columns {
            column.validate().map_err(|e| {
                GeoParquetError::invalid_metadata(format!("Invalid column '{}': {}", name, e))
            })?;
        }

        Ok(())
    }

    /// Serializes to JSON string
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }

    /// Deserializes from JSON string
    pub fn from_json(json: &str) -> Result<Self> {
        let metadata: Self = serde_json::from_str(json)?;
        metadata.validate()?;
        Ok(metadata)
    }
}

/// Metadata for a single geometry column
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeometryColumnMetadata {
    /// Encoding format (currently only "WKB" is supported)
    pub encoding: EncodingType,

    /// Geometry types present in this column
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub geometry_types: Vec<String>,

    /// Coordinate Reference System
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crs: Option<Crs>,

    /// Column-level bounding box
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<Vec<f64>>,

    /// Edges interpretation (planar or spherical)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edges: Option<EdgesInterpretation>,

    /// Orientation (counter-clockwise for polygons)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<Orientation>,

    /// Epoch for coordinate reference system
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epoch: Option<f64>,
}

impl GeometryColumnMetadata {
    /// Creates new geometry column metadata with WKB encoding
    pub fn new_wkb() -> Self {
        Self {
            encoding: EncodingType::Wkb,
            geometry_types: Vec::new(),
            crs: None,
            bbox: None,
            edges: None,
            orientation: None,
            epoch: None,
        }
    }

    /// Sets the CRS
    pub fn with_crs(mut self, crs: Crs) -> Self {
        self.crs = Some(crs);
        self
    }

    /// Sets the bounding box
    pub fn with_bbox(mut self, bbox: Vec<f64>) -> Self {
        self.bbox = Some(bbox);
        self
    }

    /// Sets geometry types
    pub fn with_geometry_types(mut self, types: Vec<String>) -> Self {
        self.geometry_types = types;
        self
    }

    /// Sets edges interpretation
    pub fn with_edges(mut self, edges: EdgesInterpretation) -> Self {
        self.edges = Some(edges);
        self
    }

    /// Sets polygon orientation
    pub fn with_orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = Some(orientation);
        self
    }

    /// Validates the column metadata
    pub fn validate(&self) -> Result<()> {
        // Validate encoding
        if self.encoding != EncodingType::Wkb {
            return Err(GeoParquetError::unsupported(format!(
                "Encoding: {:?}",
                self.encoding
            )));
        }

        // Validate bbox if present
        if let Some(ref bbox) = self.bbox {
            if bbox.len() != 4 && bbox.len() != 6 {
                return Err(GeoParquetError::invalid_bbox(format!(
                    "Bounding box must have 4 or 6 elements, got {}",
                    bbox.len()
                )));
            }

            // Check min/max ordering
            if bbox.len() == 4 {
                if bbox[0] > bbox[2] || bbox[1] > bbox[3] {
                    return Err(GeoParquetError::invalid_bbox(
                        "Min values must be <= max values",
                    ));
                }
            } else if bbox.len() == 6
                && (bbox[0] > bbox[3] || bbox[1] > bbox[4] || bbox[2] > bbox[5])
            {
                return Err(GeoParquetError::invalid_bbox(
                    "Min values must be <= max values",
                ));
            }
        }

        // Validate CRS if present
        if let Some(ref crs) = self.crs {
            crs.validate()?;
        }

        Ok(())
    }
}

/// Geometry encoding type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncodingType {
    /// Well-Known Binary encoding
    #[serde(rename = "WKB")]
    Wkb,
}

/// Coordinate Reference System
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Crs {
    /// PROJJSON CRS definition
    ProjJson(serde_json::Value),
    /// WKT2 CRS string
    Wkt2(String),
}

impl Crs {
    /// Creates a CRS from WKT2 string
    pub fn from_wkt2(wkt: impl Into<String>) -> Self {
        Self::Wkt2(wkt.into())
    }

    /// Creates a CRS from EPSG code
    pub fn from_epsg(code: u32) -> Self {
        Self::ProjJson(serde_json::json!({
            "type": "GeographicCRS",
            "id": {
                "authority": "EPSG",
                "code": code
            }
        }))
    }

    /// Returns WGS 84 (EPSG:4326)
    pub fn wgs84() -> Self {
        Self::from_epsg(4326)
    }

    /// Validates the CRS
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Wkt2(wkt) => {
                if wkt.is_empty() {
                    return Err(GeoParquetError::invalid_crs("Empty WKT2 string"));
                }
                Ok(())
            }
            Self::ProjJson(json) => {
                if !json.is_object() {
                    return Err(GeoParquetError::invalid_crs("PROJJSON must be an object"));
                }
                Ok(())
            }
        }
    }
}

/// Edges interpretation for geodetic coordinate systems
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgesInterpretation {
    /// Planar edges (straight lines in projected coordinates)
    Planar,
    /// Spherical edges (great circle arcs)
    Spherical,
}

/// Polygon ring orientation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Orientation {
    /// Counter-clockwise orientation (exterior rings)
    CounterClockwise,
}

/// Statistics for a geometry column
#[derive(Debug, Clone, Default)]
pub struct GeometryStatistics {
    /// Total number of geometries
    pub count: u64,
    /// Number of null geometries
    pub null_count: u64,
    /// Bounding box covering all geometries
    pub bbox: Option<Vec<f64>>,
    /// Geometry types encountered
    pub geometry_types: Vec<String>,
}

impl GeometryStatistics {
    /// Creates new empty statistics
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates statistics with a new geometry
    pub fn update(&mut self, geometry_type: Option<&str>, bbox: Option<&[f64]>) {
        self.count += 1;

        if let Some(geom_type) = geometry_type {
            if !self.geometry_types.contains(&geom_type.to_string()) {
                self.geometry_types.push(geom_type.to_string());
            }

            if let Some(new_bbox) = bbox {
                if let Some(ref mut existing_bbox) = self.bbox {
                    // Merge bounding boxes
                    Self::merge_bbox(existing_bbox, new_bbox);
                } else {
                    self.bbox = Some(new_bbox.to_vec());
                }
            }
        } else {
            self.null_count += 1;
        }
    }

    /// Merges two bounding boxes (expands existing to include new)
    fn merge_bbox(existing: &mut [f64], new: &[f64]) {
        if existing.len() == new.len() {
            if existing.len() == 4 {
                // 2D bbox: [minx, miny, maxx, maxy]
                existing[0] = existing[0].min(new[0]); // minx
                existing[1] = existing[1].min(new[1]); // miny
                existing[2] = existing[2].max(new[2]); // maxx
                existing[3] = existing[3].max(new[3]); // maxy
            } else if existing.len() == 6 {
                // 3D bbox: [minx, miny, minz, maxx, maxy, maxz]
                existing[0] = existing[0].min(new[0]); // minx
                existing[1] = existing[1].min(new[1]); // miny
                existing[2] = existing[2].min(new[2]); // minz
                existing[3] = existing[3].max(new[3]); // maxx
                existing[4] = existing[4].max(new[4]); // maxy
                existing[5] = existing[5].max(new[5]); // maxz
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geoparquet_metadata_creation() {
        let mut metadata = GeoParquetMetadata::new("geometry");
        let column = GeometryColumnMetadata::new_wkb()
            .with_crs(Crs::wgs84())
            .with_bbox(vec![-180.0, -90.0, 180.0, 90.0]);
        metadata.add_column("geometry", column);

        assert_eq!(metadata.version, GEOPARQUET_VERSION);
        assert_eq!(metadata.primary_column, "geometry");
        assert!(metadata.validate().is_ok());
    }

    #[test]
    fn test_geometry_column_metadata() {
        let metadata = GeometryColumnMetadata::new_wkb()
            .with_crs(Crs::wgs84())
            .with_bbox(vec![-180.0, -90.0, 180.0, 90.0])
            .with_geometry_types(vec!["Point".to_string(), "Polygon".to_string()]);

        assert_eq!(metadata.encoding, EncodingType::Wkb);
        assert!(metadata.crs.is_some());
        assert_eq!(metadata.geometry_types.len(), 2);
        assert!(metadata.validate().is_ok());
    }

    #[test]
    fn test_invalid_bbox() {
        let metadata = GeometryColumnMetadata::new_wkb().with_bbox(vec![1.0, 2.0, 3.0]); // Only 3 elements

        assert!(metadata.validate().is_err());
    }

    #[test]
    fn test_bbox_ordering() {
        let metadata = GeometryColumnMetadata::new_wkb().with_bbox(vec![10.0, 20.0, 5.0, 15.0]); // min > max

        assert!(metadata.validate().is_err());
    }

    #[test]
    fn test_crs_creation() {
        let wgs84 = Crs::wgs84();
        assert!(wgs84.validate().is_ok());

        let wkt = Crs::from_wkt2("GEOGCS[\"WGS 84\"]");
        assert!(wkt.validate().is_ok());

        let empty_wkt = Crs::from_wkt2("");
        assert!(empty_wkt.validate().is_err());
    }

    #[test]
    fn test_metadata_serialization() {
        let mut metadata = GeoParquetMetadata::new("geometry");
        let column = GeometryColumnMetadata::new_wkb()
            .with_crs(Crs::wgs84())
            .with_bbox(vec![-180.0, -90.0, 180.0, 90.0]);
        metadata.add_column("geometry", column);

        let json = metadata.to_json();
        assert!(json.is_ok());

        let deserialized = GeoParquetMetadata::from_json(&json.expect("json should serialize"));
        assert!(deserialized.is_ok());
        assert_eq!(deserialized.expect("should deserialize"), metadata);
    }

    #[test]
    fn test_geometry_statistics() {
        let mut stats = GeometryStatistics::new();

        stats.update(Some("Point"), Some(&[1.0, 2.0, 3.0, 4.0]));
        assert_eq!(stats.count, 1);
        assert_eq!(stats.null_count, 0);
        assert!(stats.bbox.is_some());

        stats.update(Some("Polygon"), Some(&[0.0, 0.0, 5.0, 5.0]));
        assert_eq!(stats.count, 2);
        assert_eq!(stats.geometry_types.len(), 2);

        // Check bbox was expanded
        let bbox = stats.bbox.as_ref().expect("bbox should exist");
        assert_eq!(bbox, &vec![0.0, 0.0, 5.0, 5.0]);

        stats.update(None, None);
        assert_eq!(stats.null_count, 1);
    }
}
