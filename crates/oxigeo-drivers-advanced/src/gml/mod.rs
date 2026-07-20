//! GML (Geography Markup Language) format driver.
//!
//! This module provides support for GML 3.2:
//! - Feature collections
//! - Geometry encoding/decoding
//! - CRS support
//! - OGC GML specification compliance

mod features;
mod geometry;
mod parser;
mod writer;

pub use features::{GmlFeature, GmlFeatureCollection, Property};
pub use geometry::{GmlGeometry, GmlLineString, GmlPoint, GmlPolygon};
pub use parser::GmlParser;
pub use writer::GmlWriter;

use crate::error::Result;
use std::io::{Read, Write};

/// GML version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GmlVersion {
    /// GML 2.1.2
    V2_1_2,
    /// GML 3.1.1
    V3_1_1,
    /// GML 3.2.1
    V3_2_1,
}

impl GmlVersion {
    /// Get version string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::V2_1_2 => "2.1.2",
            Self::V3_1_1 => "3.1.1",
            Self::V3_2_1 => "3.2.1",
        }
    }

    /// Parse GML version from string.
    pub fn parse_version(s: &str) -> Option<Self> {
        match s {
            "2.1.2" => Some(Self::V2_1_2),
            "3.1.1" => Some(Self::V3_1_1),
            "3.2.1" | "3.2" => Some(Self::V3_2_1),
            _ => None,
        }
    }
}

/// Read GML from reader.
pub fn read_gml<R: Read + std::io::BufRead>(reader: R) -> Result<GmlFeatureCollection> {
    let mut parser = GmlParser::new(reader)?;
    parser.parse()
}

/// Write GML to writer.
pub fn write_gml<W: Write>(writer: W, collection: &GmlFeatureCollection) -> Result<()> {
    let mut gml_writer = GmlWriter::new(writer);
    gml_writer.write(collection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gml_version() {
        assert_eq!(GmlVersion::V3_2_1.as_str(), "3.2.1");
        assert_eq!(GmlVersion::parse_version("3.2.1"), Some(GmlVersion::V3_2_1));
        assert_eq!(GmlVersion::parse_version("3.2"), Some(GmlVersion::V3_2_1));
        assert_eq!(GmlVersion::parse_version("4.0"), None);
    }
}
