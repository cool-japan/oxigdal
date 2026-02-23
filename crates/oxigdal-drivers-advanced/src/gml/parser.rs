//! GML parser.

use super::{GmlFeature, GmlFeatureCollection, GmlGeometry};
use crate::error::{Error, Result};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::io::BufRead;

/// GML parser.
pub struct GmlParser<R> {
    reader: Reader<R>,
}

impl<R: BufRead> GmlParser<R> {
    /// Create new GML parser.
    pub fn new(reader: R) -> Result<Self> {
        let mut xml_reader = Reader::from_reader(reader);
        xml_reader.config_mut().trim_text(true);

        Ok(Self { reader: xml_reader })
    }

    /// Parse GML document.
    pub fn parse(&mut self) -> Result<GmlFeatureCollection> {
        let mut collection = GmlFeatureCollection::new();
        let mut buf = Vec::new();
        let mut in_collection = false;

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let name = e.name();
                    match name.as_ref() {
                        b"FeatureCollection" => in_collection = true,
                        b"featureMember" | b"featureMembers" if in_collection => {
                            if let Ok(feature) = self.parse_feature_member() {
                                collection.add_feature(feature);
                            }
                        }
                        b"boundedBy" if in_collection => {
                            // Parse envelope/bounds if needed
                        }
                        _ => {}
                    }
                }
                Ok(Event::End(e)) => {
                    if e.name().as_ref() == b"FeatureCollection" {
                        in_collection = false;
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(Error::gml(format!("XML parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }

        Ok(collection)
    }

    /// Parse feature member.
    fn parse_feature_member(&mut self) -> Result<GmlFeature> {
        let mut feature = GmlFeature::new();
        let mut buf = Vec::new();

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                    // Check for common geometry elements
                    if name.contains("Point") || name.contains("point") {
                        feature.geometry = self.parse_point().ok();
                    } else if name.contains("LineString") || name.contains("linestring") {
                        feature.geometry = self.parse_linestring().ok();
                    } else if name.contains("Polygon") || name.contains("polygon") {
                        feature.geometry = self.parse_polygon().ok();
                    } else {
                        // Treat as property
                        if let Ok(value) = self.read_text() {
                            feature.add_property(name, value);
                        }
                    }
                }
                Ok(Event::End(e)) => {
                    let name = e.name();
                    if name.as_ref() == b"featureMember" || name.as_ref() == b"featureMembers" {
                        break;
                    }
                }
                Ok(Event::Eof) => return Err(Error::gml("Unexpected EOF in feature")),
                Err(e) => return Err(Error::gml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }

        Ok(feature)
    }

    /// Parse GML Point.
    fn parse_point(&mut self) -> Result<GmlGeometry> {
        let coords = self.parse_pos_or_coordinates()?;
        if coords.is_empty() {
            return Err(Error::gml("Empty Point coordinates"));
        }
        Ok(GmlGeometry::Point {
            coordinates: coords[0].clone(),
        })
    }

    /// Parse GML LineString.
    fn parse_linestring(&mut self) -> Result<GmlGeometry> {
        let coords = self.parse_pos_list_or_coordinates()?;
        Ok(GmlGeometry::LineString {
            coordinates: coords,
        })
    }

    /// Parse GML Polygon.
    fn parse_polygon(&mut self) -> Result<GmlGeometry> {
        let mut exterior = Vec::new();
        let mut interior = Vec::new();
        let mut buf = Vec::new();

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let name = e.name();
                    if name.as_ref() == b"exterior" || name.as_ref() == b"outerBoundaryIs" {
                        exterior = self.parse_linear_ring()?;
                    } else if name.as_ref() == b"interior" || name.as_ref() == b"innerBoundaryIs" {
                        interior.push(self.parse_linear_ring()?);
                    }
                }
                Ok(Event::End(e)) => {
                    let name_bytes = e.name();
                    let name = String::from_utf8_lossy(name_bytes.as_ref());
                    if name.contains("Polygon") || name.contains("polygon") {
                        break;
                    }
                }
                Ok(Event::Eof) => return Err(Error::gml("Unexpected EOF in Polygon")),
                _ => {}
            }
            buf.clear();
        }

        Ok(GmlGeometry::Polygon { exterior, interior })
    }

    /// Parse linear ring.
    fn parse_linear_ring(&mut self) -> Result<Vec<Vec<f64>>> {
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let name = e.name();
                    if name.as_ref() == b"posList" || name.as_ref() == b"coordinates" {
                        return self.parse_pos_list_or_coordinates();
                    }
                }
                Ok(Event::End(_)) => {}
                Ok(Event::Eof) => return Err(Error::gml("Unexpected EOF in LinearRing")),
                _ => {}
            }
            buf.clear();
        }
    }

    /// Parse posList or coordinates.
    fn parse_pos_list_or_coordinates(&mut self) -> Result<Vec<Vec<f64>>> {
        let text = self.read_text()?;
        parse_coordinate_text(&text)
    }

    /// Parse single pos or coordinate.
    fn parse_pos_or_coordinates(&mut self) -> Result<Vec<Vec<f64>>> {
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    if e.name().as_ref() == b"pos" || e.name().as_ref() == b"coordinates" {
                        let text = self.read_text()?;
                        return parse_coordinate_text(&text);
                    }
                }
                Ok(Event::End(_)) => {}
                Ok(Event::Eof) => return Err(Error::gml("Unexpected EOF")),
                _ => {}
            }
            buf.clear();
        }
    }

    /// Read text content.
    fn read_text(&mut self) -> Result<String> {
        let mut buf = Vec::new();
        let mut text = String::new();

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Text(e)) => {
                    text.push_str(&e.decode().map_err(|e| Error::gml(format!("{}", e)))?);
                }
                Ok(Event::End(_)) => break,
                Ok(Event::Eof) => return Err(Error::gml("Unexpected EOF")),
                Err(e) => return Err(Error::gml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }

        Ok(text)
    }
}

/// Parse coordinate text (space/comma separated numbers).
fn parse_coordinate_text(text: &str) -> Result<Vec<Vec<f64>>> {
    let mut coords = Vec::new();
    let numbers: Vec<f64> = text
        .split_whitespace()
        .flat_map(|s| s.split(','))
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();

    // Group into coordinate tuples (2D or 3D)
    let dim = if numbers.len() % 3 == 0 { 3 } else { 2 };
    for chunk in numbers.chunks(dim) {
        coords.push(chunk.to_vec());
    }

    Ok(coords)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_coordinate_text() {
        let text = "1.0 2.0 3.0 4.0";
        let coords = parse_coordinate_text(text).ok();
        assert!(coords.is_some());
        if let Some(c) = coords {
            assert_eq!(c.len(), 2);
            assert_eq!(c[0], vec![1.0, 2.0]);
            assert_eq!(c[1], vec![3.0, 4.0]);
        }
    }

    #[test]
    fn test_parse_coordinate_text_3d() {
        let text = "1.0 2.0 3.0 4.0 5.0 6.0";
        let coords = parse_coordinate_text(text).ok();
        assert!(coords.is_some());
        if let Some(c) = coords {
            assert_eq!(c.len(), 2);
            assert_eq!(c[0], vec![1.0, 2.0, 3.0]);
        }
    }
}
