//! GML feature structures.

use super::GmlGeometry;
use serde::{Deserialize, Serialize};

/// GML feature collection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GmlFeatureCollection {
    /// Features
    pub features: Vec<GmlFeature>,
    /// Bounds
    pub bounds: Option<Bounds>,
    /// CRS identifier
    pub crs: Option<String>,
}

impl GmlFeatureCollection {
    /// Create new feature collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add feature.
    pub fn add_feature(&mut self, feature: GmlFeature) {
        self.features.push(feature);
    }

    /// Set CRS.
    pub fn with_crs<S: Into<String>>(mut self, crs: S) -> Self {
        self.crs = Some(crs.into());
        self
    }

    /// Set bounds.
    pub fn with_bounds(mut self, bounds: Bounds) -> Self {
        self.bounds = Some(bounds);
        self
    }

    /// Get feature count.
    pub fn len(&self) -> usize {
        self.features.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }
}

/// GML feature.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GmlFeature {
    /// Feature ID (gml:id)
    pub id: Option<String>,
    /// Geometry
    pub geometry: Option<GmlGeometry>,
    /// Properties
    pub properties: Vec<Property>,
}

impl GmlFeature {
    /// Create new feature.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set ID.
    pub fn with_id<S: Into<String>>(mut self, id: S) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set geometry.
    pub fn with_geometry(mut self, geometry: GmlGeometry) -> Self {
        self.geometry = Some(geometry);
        self
    }

    /// Add property.
    pub fn add_property<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) {
        self.properties.push(Property {
            name: key.into(),
            value: value.into(),
        });
    }

    /// Get property by name.
    pub fn get_property(&self, name: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.value.as_str())
    }
}

/// Feature property.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Property {
    /// Property name
    pub name: String,
    /// Property value
    pub value: String,
}

/// Bounding box.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Bounds {
    /// Minimum X
    pub min_x: f64,
    /// Minimum Y
    pub min_y: f64,
    /// Maximum X
    pub max_x: f64,
    /// Maximum Y
    pub max_y: f64,
}

impl Bounds {
    /// Create new bounds.
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_collection() {
        let mut collection = GmlFeatureCollection::new();
        assert!(collection.is_empty());

        collection.add_feature(GmlFeature::new());
        assert_eq!(collection.len(), 1);
    }

    #[test]
    fn test_feature_properties() {
        let mut feature = GmlFeature::new();
        feature.add_property("name", "Test");
        feature.add_property("value", "123");

        assert_eq!(feature.get_property("name"), Some("Test"));
        assert_eq!(feature.get_property("value"), Some("123"));
        assert_eq!(feature.get_property("missing"), None);
    }

    #[test]
    fn test_bounds() {
        let bounds = Bounds::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(bounds.min_x, 0.0);
        assert_eq!(bounds.max_x, 10.0);
    }
}
