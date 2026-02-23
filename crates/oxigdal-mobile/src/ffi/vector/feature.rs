//! Feature data structures and field value types for FFI.
//!
//! Provides feature representations and field value conversions.

use super::super::types::OxiGdalBbox;
use super::geometry::FfiGeometry;
use std::collections::HashMap;

/// Field value types for features.
#[derive(Debug, Clone)]
pub enum FieldValue {
    /// Null value
    Null,
    /// String value
    String(String),
    /// Integer value
    Integer(i64),
    /// Double value
    Double(f64),
    /// Boolean value
    Bool(bool),
}

impl FieldValue {
    /// Converts the field value to a string representation.
    #[must_use]
    pub fn to_string_value(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::String(s) => s.clone(),
            Self::Integer(i) => i.to_string(),
            Self::Double(d) => d.to_string(),
            Self::Bool(b) => b.to_string(),
        }
    }

    /// Returns the value as an integer if applicable.
    #[must_use]
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(i) => Some(*i),
            Self::Double(d) => Some(*d as i64),
            Self::Bool(b) => Some(if *b { 1 } else { 0 }),
            Self::String(s) => s.parse().ok(),
            Self::Null => None,
        }
    }

    /// Returns the value as a double if applicable.
    #[must_use]
    pub fn as_double(&self) -> Option<f64> {
        match self {
            Self::Double(d) => Some(*d),
            Self::Integer(i) => Some(*i as f64),
            Self::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Self::String(s) => s.parse().ok(),
            Self::Null => None,
        }
    }
}

/// Internal feature data.
#[derive(Debug, Clone)]
pub struct FfiFeature {
    /// Feature ID
    pub fid: i64,
    /// Geometry
    pub geometry: Option<FfiGeometry>,
    /// Field values
    pub fields: HashMap<String, FieldValue>,
}

impl FfiFeature {
    /// Creates a new feature with the given FID.
    #[must_use]
    pub fn new(fid: i64) -> Self {
        Self {
            fid,
            geometry: None,
            fields: HashMap::new(),
        }
    }

    /// Returns the bounding box of this feature's geometry.
    #[must_use]
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        self.geometry.as_ref().and_then(FfiGeometry::bounds)
    }

    /// Checks if the feature intersects with the given bounding box.
    #[must_use]
    pub fn intersects_bbox(&self, bbox: &OxiGdalBbox) -> bool {
        if let Some((min_x, min_y, max_x, max_y)) = self.bounds() {
            // Check if bounding boxes overlap
            !(max_x < bbox.min_x || min_x > bbox.max_x || max_y < bbox.min_y || min_y > bbox.max_y)
        } else {
            // Features without geometry don't intersect
            false
        }
    }
}

/// Internal feature handle.
pub struct FeatureHandle {
    /// Feature data
    feature: FfiFeature,
}

impl FeatureHandle {
    /// Creates a new feature handle from FFI feature data.
    #[must_use]
    pub fn new(feature: FfiFeature) -> Self {
        Self { feature }
    }

    /// Returns a reference to the inner feature.
    #[must_use]
    pub fn inner(&self) -> &FfiFeature {
        &self.feature
    }
}
