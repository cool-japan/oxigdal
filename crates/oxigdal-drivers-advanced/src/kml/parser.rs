//! KML XML parser.

use super::features::{Coordinates, Geometry as KmlGeometry};
use super::{KmlDocument, NetworkLink, Placemark, RefreshMode, Style};
use crate::error::{Error, Result};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::io::BufRead;

/// KML parser.
pub struct KmlParser<R> {
    reader: Reader<R>,
}

impl<R: BufRead> KmlParser<R> {
    /// Create new KML parser.
    pub fn new(reader: R) -> Result<Self> {
        let mut xml_reader = Reader::from_reader(reader);
        xml_reader.config_mut().trim_text(true);

        Ok(Self { reader: xml_reader })
    }

    /// Parse KML document.
    pub fn parse(&mut self) -> Result<KmlDocument> {
        let mut doc = KmlDocument::new();
        let mut buf = Vec::new();
        let mut in_document = false;

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let name = e.name();
                    match name.as_ref() {
                        b"kml" => {}
                        b"Document" => in_document = true,
                        b"Placemark" if in_document => {
                            if let Ok(placemark) = self.parse_placemark() {
                                doc.add_placemark(placemark);
                            }
                        }
                        b"Style" if in_document => {
                            if let Ok(style) = self.parse_style() {
                                doc.add_style(style);
                            }
                        }
                        b"NetworkLink" if in_document => {
                            if let Ok(link) = self.parse_network_link() {
                                doc.add_network_link(link);
                            }
                        }
                        b"name" if in_document => {
                            if let Ok(name) = self.read_text() {
                                doc.name = Some(name);
                            }
                        }
                        b"description" if in_document => {
                            if let Ok(desc) = self.read_text() {
                                doc.description = Some(desc);
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::End(e)) => {
                    if e.name().as_ref() == b"Document" {
                        in_document = false;
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(Error::kml(format!("XML parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }

        Ok(doc)
    }

    /// Parse placemark element.
    fn parse_placemark(&mut self) -> Result<Placemark> {
        let mut placemark = Placemark::new();
        let mut buf = Vec::new();

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"name" => placemark.name = self.read_text().ok(),
                    b"description" => placemark.description = self.read_text().ok(),
                    b"Point" => placemark.geometry = Some(self.parse_point()?),
                    b"LineString" => placemark.geometry = Some(self.parse_linestring()?),
                    b"Polygon" => placemark.geometry = Some(self.parse_polygon()?),
                    _ => {}
                },
                Ok(Event::End(e)) => {
                    if e.name().as_ref() == b"Placemark" {
                        break;
                    }
                }
                Ok(Event::Eof) => return Err(Error::kml("Unexpected EOF in Placemark")),
                Err(e) => return Err(Error::kml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }

        Ok(placemark)
    }

    /// Parse Point geometry.
    fn parse_point(&mut self) -> Result<KmlGeometry> {
        let coordinates = self.parse_coordinates()?;
        if coordinates.is_empty() {
            return Err(Error::kml("Empty Point coordinates"));
        }
        Ok(KmlGeometry::Point(coordinates[0]))
    }

    /// Parse LineString geometry.
    fn parse_linestring(&mut self) -> Result<KmlGeometry> {
        let coordinates = self.parse_coordinates()?;
        Ok(KmlGeometry::LineString(coordinates))
    }

    /// Parse Polygon geometry.
    fn parse_polygon(&mut self) -> Result<KmlGeometry> {
        let mut outer_ring = Vec::new();
        let mut inner_rings = Vec::new();
        let mut buf = Vec::new();

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"outerBoundaryIs" | b"exterior" => {
                        outer_ring = self.parse_linear_ring()?;
                    }
                    b"innerBoundaryIs" | b"interior" => {
                        inner_rings.push(self.parse_linear_ring()?);
                    }
                    _ => {}
                },
                Ok(Event::End(e)) => {
                    if e.name().as_ref() == b"Polygon" {
                        break;
                    }
                }
                Ok(Event::Eof) => return Err(Error::kml("Unexpected EOF in Polygon")),
                Err(e) => return Err(Error::kml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }

        Ok(KmlGeometry::Polygon {
            outer: outer_ring,
            inner: inner_rings,
        })
    }

    /// Parse linear ring.
    fn parse_linear_ring(&mut self) -> Result<Vec<Coordinates>> {
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) if e.name().as_ref() == b"coordinates" => {
                    return self.parse_coordinates();
                }
                Ok(Event::End(_)) => {}
                Ok(Event::Eof) => return Err(Error::kml("Unexpected EOF in LinearRing")),
                Err(e) => return Err(Error::kml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }
    }

    /// Parse coordinates element.
    fn parse_coordinates(&mut self) -> Result<Vec<Coordinates>> {
        let text = self.read_text()?;
        parse_coordinate_string(&text)
    }

    /// Parse Style element.
    fn parse_style(&mut self) -> Result<Style> {
        let style = Style::default();
        // Simplified style parsing
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::End(e)) if e.name().as_ref() == b"Style" => break,
                Ok(Event::Eof) => return Err(Error::kml("Unexpected EOF in Style")),
                _ => {}
            }
            buf.clear();
        }
        Ok(style)
    }

    /// Parse NetworkLink element.
    fn parse_network_link(&mut self) -> Result<NetworkLink> {
        let mut name = None;
        let mut href = String::new();
        let mut buf = Vec::new();

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"name" => name = self.read_text().ok(),
                    b"href" => href = self.read_text()?,
                    _ => {}
                },
                Ok(Event::End(e)) if e.name().as_ref() == b"NetworkLink" => break,
                Ok(Event::Eof) => return Err(Error::kml("Unexpected EOF in NetworkLink")),
                _ => {}
            }
            buf.clear();
        }

        Ok(NetworkLink {
            name,
            visibility: true,
            refresh_mode: RefreshMode::OnChange,
            href,
        })
    }

    /// Read text content.
    fn read_text(&mut self) -> Result<String> {
        let mut buf = Vec::new();
        let mut text = String::new();

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Text(e)) => {
                    text.push_str(&e.decode().map_err(|e| Error::kml(format!("{}", e)))?);
                }
                Ok(Event::End(_)) => break,
                Ok(Event::Eof) => return Err(Error::kml("Unexpected EOF")),
                Err(e) => return Err(Error::kml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }

        Ok(text)
    }
}

/// Parse coordinate string into Coordinates.
fn parse_coordinate_string(s: &str) -> Result<Vec<Coordinates>> {
    let mut coords = Vec::new();

    for point_str in s.split_whitespace() {
        let parts: Vec<&str> = point_str.split(',').collect();
        if parts.len() < 2 {
            continue;
        }

        let lon: f64 = parts[0]
            .parse()
            .map_err(|_| Error::kml("Invalid longitude"))?;
        let lat: f64 = parts[1]
            .parse()
            .map_err(|_| Error::kml("Invalid latitude"))?;
        let alt: Option<f64> = if parts.len() >= 3 {
            parts[2].parse().ok()
        } else {
            None
        };

        coords.push(Coordinates { lon, lat, alt });
    }

    Ok(coords)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_coordinate_string() {
        let s = "-122.0822035425683,37.42228990140251,0";
        let coords = parse_coordinate_string(s).ok();
        assert!(coords.is_some());
        if let Some(c) = coords {
            assert_eq!(c.len(), 1);
            assert!((c[0].lon + 122.08).abs() < 0.01);
            assert!((c[0].lat - 37.42).abs() < 0.01);
        }
    }

    #[test]
    fn test_parse_multiple_coordinates() {
        let s = "-122.08,37.42,0 -122.09,37.43,0";
        let coords = parse_coordinate_string(s).ok();
        assert!(coords.is_some());
        if let Some(c) = coords {
            assert_eq!(c.len(), 2);
        }
    }
}
