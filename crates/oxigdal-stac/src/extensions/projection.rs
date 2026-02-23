//! Projection Extension for STAC.
//!
//! This extension provides information about the coordinate reference system
//! and projection of geospatial assets.

use crate::error::{Result, StacError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Schema URI for the Projection extension.
pub const SCHEMA_URI: &str = "https://stac-extensions.github.io/projection/v1.1.0/schema.json";

/// Projection extension data for a STAC Item or Asset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectionExtension {
    /// EPSG code of the coordinate reference system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epsg: Option<i32>,

    /// WKT2 string representing the coordinate reference system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wkt2: Option<String>,

    /// PROJJSON object representing the coordinate reference system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projjson: Option<Value>,

    /// Centroid of the geometry in the projection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub centroid: Option<Centroid>,

    /// Bounding box in the projection [minx, miny, maxx, maxy].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<Vec<f64>>,

    /// Shape of the asset in pixels [height, width].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<Vec<u32>>,

    /// Pixel size in the projection units (x, y).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<Vec<f64>>,
}

/// Centroid in the projection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Centroid {
    /// Latitude or northing.
    pub lat: f64,
    /// Longitude or easting.
    pub lon: f64,
}

impl ProjectionExtension {
    /// Creates a new Projection extension.
    ///
    /// # Returns
    ///
    /// A new Projection extension instance
    pub fn new() -> Self {
        Self {
            epsg: None,
            wkt2: None,
            projjson: None,
            centroid: None,
            bbox: None,
            shape: None,
            transform: None,
        }
    }

    /// Sets the EPSG code.
    ///
    /// # Arguments
    ///
    /// * `epsg` - EPSG code
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_epsg(mut self, epsg: i32) -> Self {
        self.epsg = Some(epsg);
        self
    }

    /// Sets the WKT2 string.
    ///
    /// # Arguments
    ///
    /// * `wkt2` - WKT2 string
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_wkt2(mut self, wkt2: impl Into<String>) -> Self {
        self.wkt2 = Some(wkt2.into());
        self
    }

    /// Sets the PROJJSON object.
    ///
    /// # Arguments
    ///
    /// * `projjson` - PROJJSON value
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_projjson(mut self, projjson: Value) -> Self {
        self.projjson = Some(projjson);
        self
    }

    /// Sets the centroid.
    ///
    /// # Arguments
    ///
    /// * `lat` - Latitude or northing
    /// * `lon` - Longitude or easting
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_centroid(mut self, lat: f64, lon: f64) -> Self {
        self.centroid = Some(Centroid { lat, lon });
        self
    }

    /// Sets the bounding box.
    ///
    /// # Arguments
    ///
    /// * `bbox` - Bounding box [minx, miny, maxx, maxy]
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_bbox(mut self, bbox: Vec<f64>) -> Self {
        self.bbox = Some(bbox);
        self
    }

    /// Sets the shape.
    ///
    /// # Arguments
    ///
    /// * `height` - Height in pixels
    /// * `width` - Width in pixels
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_shape(mut self, height: u32, width: u32) -> Self {
        self.shape = Some(vec![height, width]);
        self
    }

    /// Sets the affine transform.
    ///
    /// # Arguments
    ///
    /// * `transform` - Affine transform coefficients
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_transform(mut self, transform: Vec<f64>) -> Self {
        self.transform = Some(transform);
        self
    }

    /// Validates the Projection extension data.
    ///
    /// # Returns
    ///
    /// `Ok(())` if valid, otherwise an error
    pub fn validate(&self) -> Result<()> {
        // At least one CRS representation should be provided
        if self.epsg.is_none() && self.wkt2.is_none() && self.projjson.is_none() {
            // This is not strictly required, but recommended
        }

        // Validate bbox
        if let Some(bbox) = &self.bbox {
            if bbox.len() != 4 {
                return Err(StacError::InvalidExtension {
                    extension: "proj:bbox".to_string(),
                    reason: format!("bbox must have 4 elements, found {}", bbox.len()),
                });
            }
        }

        // Validate shape
        if let Some(shape) = &self.shape {
            if shape.len() != 2 {
                return Err(StacError::InvalidExtension {
                    extension: "proj:shape".to_string(),
                    reason: format!("shape must have 2 elements, found {}", shape.len()),
                });
            }
        }

        // Validate transform (affine transform should have 6 or 9 elements)
        if let Some(transform) = &self.transform {
            if transform.len() != 6 && transform.len() != 9 {
                return Err(StacError::InvalidExtension {
                    extension: "proj:transform".to_string(),
                    reason: format!(
                        "transform must have 6 or 9 elements, found {}",
                        transform.len()
                    ),
                });
            }
        }

        Ok(())
    }

    /// Converts the Projection extension to a JSON value.
    ///
    /// # Returns
    ///
    /// JSON value representation
    pub fn to_value(&self) -> Result<Value> {
        serde_json::to_value(self).map_err(|e| StacError::Serialization(e.to_string()))
    }

    /// Creates a Projection extension from a JSON value.
    ///
    /// # Arguments
    ///
    /// * `value` - JSON value
    ///
    /// # Returns
    ///
    /// Projection extension instance
    pub fn from_value(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|e| StacError::Deserialization(e.to_string()))
    }
}

impl Default for ProjectionExtension {
    fn default() -> Self {
        Self::new()
    }
}

/// Common EPSG codes.
pub mod epsg_codes {
    /// WGS 84 (GPS).
    pub const WGS84: i32 = 4326;

    /// Web Mercator.
    pub const WEB_MERCATOR: i32 = 3857;

    /// UTM Zone 33N.
    pub const UTM_33N: i32 = 32633;

    /// UTM Zone 10N.
    pub const UTM_10N: i32 = 32610;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_projection_extension_new() {
        let proj = ProjectionExtension::new();
        assert!(proj.epsg.is_none());
        assert!(proj.wkt2.is_none());
    }

    #[test]
    fn test_projection_extension_with_epsg() {
        let proj = ProjectionExtension::new().with_epsg(epsg_codes::WGS84);
        assert_eq!(proj.epsg, Some(epsg_codes::WGS84));
    }

    #[test]
    fn test_projection_extension_with_shape() {
        let proj = ProjectionExtension::new().with_shape(1024, 2048);
        assert_eq!(proj.shape, Some(vec![1024, 2048]));
    }

    #[test]
    fn test_projection_extension_with_centroid() {
        let proj = ProjectionExtension::new().with_centroid(37.7749, -122.4194);
        assert_eq!(
            proj.centroid,
            Some(Centroid {
                lat: 37.7749,
                lon: -122.4194
            })
        );
    }

    #[test]
    fn test_projection_extension_validate() {
        let valid = ProjectionExtension::new()
            .with_epsg(4326)
            .with_bbox(vec![-180.0, -90.0, 180.0, 90.0]);
        assert!(valid.validate().is_ok());

        let invalid_bbox = ProjectionExtension::new().with_bbox(vec![-180.0, -90.0]);
        assert!(invalid_bbox.validate().is_err());
    }

    #[test]
    fn test_projection_extension_with_transform() {
        let transform = vec![10.0, 0.0, 0.0, 0.0, -10.0, 0.0];
        let proj = ProjectionExtension::new().with_transform(transform.clone());
        assert_eq!(proj.transform, Some(transform));
        assert!(proj.validate().is_ok());
    }

    #[test]
    fn test_projection_extension_serialization() {
        let proj = ProjectionExtension::new()
            .with_epsg(4326)
            .with_shape(1024, 2048);

        let json = serde_json::to_string(&proj);
        assert!(json.is_ok());

        let deserialized: ProjectionExtension =
            serde_json::from_str(&json.expect("JSON serialization failed"))
                .expect("Deserialization failed");
        assert_eq!(proj, deserialized);
    }
}
