//! KML (Keyhole Markup Language) format driver.
//!
//! This module provides support for reading and writing KML 2.2 files:
//! - Placemark features
//! - LineString and Polygon geometries
//! - Styles and icons
//! - NetworkLinks
//! - Google Earth compatibility

mod features;
mod parser;
mod styles;
mod writer;

pub use features::{Coordinates, Geometry as KmlGeometry, Placemark};
pub use parser::KmlParser;
pub use styles::{IconStyle, LineStyle, PolyStyle, Style, StyleMap};
pub use writer::KmlWriter;

use crate::error::{Error, Result};
use std::io::{Read, Write};
use std::str::FromStr;

/// KML document representation.
#[derive(Debug, Clone)]
pub struct KmlDocument {
    /// Document name
    pub name: Option<String>,
    /// Document description
    pub description: Option<String>,
    /// Placemarks
    pub placemarks: Vec<Placemark>,
    /// Styles
    pub styles: Vec<Style>,
    /// Style maps
    pub style_maps: Vec<StyleMap>,
    /// Network links
    pub network_links: Vec<NetworkLink>,
}

impl KmlDocument {
    /// Create new KML document.
    pub fn new() -> Self {
        Self {
            name: None,
            description: None,
            placemarks: Vec::new(),
            styles: Vec::new(),
            style_maps: Vec::new(),
            network_links: Vec::new(),
        }
    }

    /// Add placemark.
    pub fn add_placemark(&mut self, placemark: Placemark) {
        self.placemarks.push(placemark);
    }

    /// Add style.
    pub fn add_style(&mut self, style: Style) {
        self.styles.push(style);
    }

    /// Add style map.
    pub fn add_style_map(&mut self, style_map: StyleMap) {
        self.style_maps.push(style_map);
    }

    /// Add network link.
    pub fn add_network_link(&mut self, link: NetworkLink) {
        self.network_links.push(link);
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

    /// Get placemark count.
    pub fn placemark_count(&self) -> usize {
        self.placemarks.len()
    }

    /// Find style by ID.
    pub fn find_style(&self, id: &str) -> Option<&Style> {
        self.styles.iter().find(|s| s.id.as_deref() == Some(id))
    }
}

impl Default for KmlDocument {
    fn default() -> Self {
        Self::new()
    }
}

/// Network Link for external KML references.
#[derive(Debug, Clone)]
pub struct NetworkLink {
    /// Link name
    pub name: Option<String>,
    /// Visibility
    pub visibility: bool,
    /// Refresh mode
    pub refresh_mode: RefreshMode,
    /// URL/HREF
    pub href: String,
}

/// Refresh mode for network links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshMode {
    /// On change
    OnChange,
    /// On interval
    OnInterval,
    /// On expire
    OnExpire,
}

impl RefreshMode {
    /// Get as string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::OnChange => "onChange",
            Self::OnInterval => "onInterval",
            Self::OnExpire => "onExpire",
        }
    }
}

impl FromStr for RefreshMode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "onchange" => Ok(Self::OnChange),
            "oninterval" => Ok(Self::OnInterval),
            "onexpire" => Ok(Self::OnExpire),
            _ => Err(Error::kml(format!("Unknown refresh mode: {}", s))),
        }
    }
}

/// Read KML from reader.
pub fn read_kml<R: Read + std::io::BufRead>(reader: R) -> Result<KmlDocument> {
    let mut parser = KmlParser::new(reader)?;
    parser.parse()
}

/// Write KML to writer.
pub fn write_kml<W: Write>(writer: W, document: &KmlDocument) -> Result<()> {
    let mut kml_writer = KmlWriter::new(writer);
    kml_writer.write(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kml_document_creation() {
        let doc = KmlDocument::new();
        assert!(doc.name.is_none());
        assert_eq!(doc.placemark_count(), 0);
    }

    #[test]
    fn test_kml_document_builder() {
        let doc = KmlDocument::new()
            .with_name("Test Document")
            .with_description("Test Description");

        assert_eq!(doc.name, Some("Test Document".to_string()));
        assert_eq!(doc.description, Some("Test Description".to_string()));
    }

    #[test]
    fn test_refresh_mode() {
        let rm = RefreshMode::from_str("onChange");
        assert!(rm.is_ok());
        if let Ok(mode) = rm {
            assert_eq!(mode, RefreshMode::OnChange);
        }
        let rm = RefreshMode::from_str("oninterval");
        assert!(rm.is_ok());
        if let Ok(mode) = rm {
            assert_eq!(mode, RefreshMode::OnInterval);
        }
        assert_eq!(RefreshMode::OnChange.as_str(), "onChange");
    }
}
