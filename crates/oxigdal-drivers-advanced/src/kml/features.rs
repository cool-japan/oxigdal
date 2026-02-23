//! KML feature structures.

use serde::{Deserialize, Serialize};

/// KML Placemark feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Placemark {
    /// Placemark name
    pub name: Option<String>,
    /// Description
    pub description: Option<String>,
    /// Geometry
    pub geometry: Option<Geometry>,
    /// Style URL reference
    pub style_url: Option<String>,
    /// Extended data
    pub extended_data: Vec<(String, String)>,
}

impl Placemark {
    /// Create new placemark.
    pub fn new() -> Self {
        Self {
            name: None,
            description: None,
            geometry: None,
            style_url: None,
            extended_data: Vec::new(),
        }
    }

    /// Set name.
    pub fn with_name<S: Into<String>>(mut self, name: S) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set description.
    pub fn with_description<S: Into<String>>(mut self, description: S) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set geometry.
    pub fn with_geometry(mut self, geometry: Geometry) -> Self {
        self.geometry = Some(geometry);
        self
    }

    /// Add extended data field.
    pub fn add_data<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) {
        self.extended_data.push((key.into(), value.into()));
    }
}

impl Default for Placemark {
    fn default() -> Self {
        Self::new()
    }
}

/// KML geometry types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Geometry {
    /// Point geometry
    Point(Coordinates),
    /// LineString geometry
    LineString(Vec<Coordinates>),
    /// Polygon geometry
    Polygon {
        /// Outer boundary
        outer: Vec<Coordinates>,
        /// Inner boundaries (holes)
        inner: Vec<Vec<Coordinates>>,
    },
    /// MultiGeometry
    MultiGeometry(Vec<Geometry>),
}

/// Coordinate tuple (lon, lat, alt).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Coordinates {
    /// Longitude
    pub lon: f64,
    /// Latitude
    pub lat: f64,
    /// Altitude (optional)
    pub alt: Option<f64>,
}

impl Coordinates {
    /// Create new coordinates.
    pub fn new(lon: f64, lat: f64) -> Self {
        Self {
            lon,
            lat,
            alt: None,
        }
    }

    /// Create with altitude.
    pub fn with_altitude(lon: f64, lat: f64, alt: f64) -> Self {
        Self {
            lon,
            lat,
            alt: Some(alt),
        }
    }

    /// Format as KML coordinate string.
    pub fn to_kml_string(&self) -> String {
        if let Some(alt) = self.alt {
            format!("{},{},{}", self.lon, self.lat, alt)
        } else {
            format!("{},{},0", self.lon, self.lat)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placemark_creation() {
        let pm = Placemark::new()
            .with_name("Test")
            .with_description("Test placemark");

        assert_eq!(pm.name, Some("Test".to_string()));
        assert_eq!(pm.description, Some("Test placemark".to_string()));
    }

    #[test]
    fn test_coordinates() {
        let coords = Coordinates::new(-122.08, 37.42);
        assert_eq!(coords.lon, -122.08);
        assert_eq!(coords.lat, 37.42);
        assert!(coords.alt.is_none());

        let coords_with_alt = Coordinates::with_altitude(-122.08, 37.42, 100.0);
        assert_eq!(coords_with_alt.alt, Some(100.0));
    }

    #[test]
    fn test_coordinates_to_kml_string() {
        let coords = Coordinates::new(-122.08, 37.42);
        let kml_str = coords.to_kml_string();
        assert!(kml_str.contains("-122.08"));
        assert!(kml_str.contains("37.42"));

        let coords_with_alt = Coordinates::with_altitude(-122.08, 37.42, 100.0);
        let kml_str = coords_with_alt.to_kml_string();
        assert!(kml_str.contains("100"));
    }

    #[test]
    fn test_placemark_extended_data() {
        let mut pm = Placemark::new();
        pm.add_data("key1", "value1");
        pm.add_data("key2", "value2");
        assert_eq!(pm.extended_data.len(), 2);
    }
}
