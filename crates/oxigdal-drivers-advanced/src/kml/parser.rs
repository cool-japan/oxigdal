//! KML XML parser.

use super::features::{Coordinates, Geometry as KmlGeometry};
use super::styles::{IconStyle, LabelStyle, LineStyle, PolyStyle};
use super::{KmlDocument, NetworkLink, Placemark, RefreshMode, Style};
use crate::error::{Error, Result};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::io::BufRead;
use std::str::FromStr;

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
                            let id = extract_attr(&e, b"id");
                            if let Ok(style) = self.parse_style(id) {
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
                Ok(Event::End(e)) if e.name().as_ref() == b"Document" => {
                    in_document = false;
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
                Ok(Event::End(e)) if e.name().as_ref() == b"Placemark" => {
                    break;
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
                Ok(Event::End(e)) if e.name().as_ref() == b"Polygon" => {
                    break;
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

    /// Parse Style element, including its `IconStyle`/`LineStyle`/`PolyStyle`/
    /// `LabelStyle` children. `id` is the `id` attribute captured off the
    /// `<Style>` start tag by the caller.
    fn parse_style(&mut self, id: Option<String>) -> Result<Style> {
        let mut style = Style {
            id,
            ..Style::default()
        };
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"IconStyle" => style.icon_style = Some(self.parse_icon_style()?),
                    b"LineStyle" => style.line_style = Some(self.parse_line_style()?),
                    b"PolyStyle" => style.poly_style = Some(self.parse_poly_style()?),
                    b"LabelStyle" => style.label_style = Some(self.parse_label_style()?),
                    _ => {}
                },
                Ok(Event::End(e)) if e.name().as_ref() == b"Style" => break,
                Ok(Event::Eof) => return Err(Error::kml("Unexpected EOF in Style")),
                Err(e) => return Err(Error::kml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }
        Ok(style)
    }

    /// Parse `<IconStyle>` (color, scale, and `Icon/href`).
    fn parse_icon_style(&mut self) -> Result<IconStyle> {
        let mut icon = IconStyle::new();
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"color" => {
                        icon.color = self.read_text().ok();
                    }
                    b"scale" => {
                        let text = self.read_text()?;
                        icon.scale = text
                            .trim()
                            .parse::<f64>()
                            .map_err(|_| Error::kml("Invalid IconStyle scale"))?;
                    }
                    b"Icon" => {
                        icon.href = self.parse_icon_href()?;
                    }
                    _ => {}
                },
                Ok(Event::End(e)) if e.name().as_ref() == b"IconStyle" => break,
                Ok(Event::Eof) => return Err(Error::kml("Unexpected EOF in IconStyle")),
                Err(e) => return Err(Error::kml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }
        Ok(icon)
    }

    /// Parse `<Icon><href>...</href></Icon>` nested inside `IconStyle`.
    fn parse_icon_href(&mut self) -> Result<Option<String>> {
        let mut href = None;
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) if e.name().as_ref() == b"href" => {
                    href = self.read_text().ok();
                }
                Ok(Event::End(e)) if e.name().as_ref() == b"Icon" => break,
                Ok(Event::Eof) => return Err(Error::kml("Unexpected EOF in Icon")),
                Err(e) => return Err(Error::kml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }
        Ok(href)
    }

    /// Parse `<LineStyle>` (color, width).
    fn parse_line_style(&mut self) -> Result<LineStyle> {
        let mut line = LineStyle::new();
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"color" => {
                        line.color = self.read_text().ok();
                    }
                    b"width" => {
                        let text = self.read_text()?;
                        line.width = text
                            .trim()
                            .parse::<f64>()
                            .map_err(|_| Error::kml("Invalid LineStyle width"))?;
                    }
                    _ => {}
                },
                Ok(Event::End(e)) if e.name().as_ref() == b"LineStyle" => break,
                Ok(Event::Eof) => return Err(Error::kml("Unexpected EOF in LineStyle")),
                Err(e) => return Err(Error::kml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }
        Ok(line)
    }

    /// Parse `<PolyStyle>` (color, fill, outline).
    fn parse_poly_style(&mut self) -> Result<PolyStyle> {
        let mut poly = PolyStyle::new();
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"color" => {
                        poly.color = self.read_text().ok();
                    }
                    b"fill" => {
                        let text = self.read_text()?;
                        poly.fill = parse_kml_bool(&text);
                    }
                    b"outline" => {
                        let text = self.read_text()?;
                        poly.outline = parse_kml_bool(&text);
                    }
                    _ => {}
                },
                Ok(Event::End(e)) if e.name().as_ref() == b"PolyStyle" => break,
                Ok(Event::Eof) => return Err(Error::kml("Unexpected EOF in PolyStyle")),
                Err(e) => return Err(Error::kml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }
        Ok(poly)
    }

    /// Parse `<LabelStyle>` (color, scale).
    fn parse_label_style(&mut self) -> Result<LabelStyle> {
        let mut label = LabelStyle::default();
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"color" => {
                        label.color = self.read_text().ok();
                    }
                    b"scale" => {
                        let text = self.read_text()?;
                        label.scale = text
                            .trim()
                            .parse::<f64>()
                            .map_err(|_| Error::kml("Invalid LabelStyle scale"))?;
                    }
                    _ => {}
                },
                Ok(Event::End(e)) if e.name().as_ref() == b"LabelStyle" => break,
                Ok(Event::Eof) => return Err(Error::kml("Unexpected EOF in LabelStyle")),
                Err(e) => return Err(Error::kml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }
        Ok(label)
    }

    /// Parse NetworkLink element, including `<visibility>` and the
    /// `<Link>`/`<Url>` child's `refreshMode`/`href`.
    fn parse_network_link(&mut self) -> Result<NetworkLink> {
        let mut name = None;
        let mut href = String::new();
        let mut visibility = None;
        let mut refresh_mode = None;
        let mut buf = Vec::new();

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"name" => name = self.read_text().ok(),
                    b"visibility" => {
                        if let Ok(text) = self.read_text() {
                            visibility = Some(parse_kml_bool(&text));
                        }
                    }
                    b"href" => href = self.read_text()?,
                    b"Link" | b"Url" => {
                        let (link_href, link_refresh) = self.parse_link_element()?;
                        if let Some(h) = link_href {
                            href = h;
                        }
                        if let Some(r) = link_refresh {
                            refresh_mode = Some(r);
                        }
                    }
                    _ => {}
                },
                Ok(Event::End(e)) if e.name().as_ref() == b"NetworkLink" => break,
                Ok(Event::Eof) => return Err(Error::kml("Unexpected EOF in NetworkLink")),
                Err(e) => return Err(Error::kml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }

        Ok(NetworkLink {
            name,
            visibility: visibility.unwrap_or(true),
            refresh_mode: refresh_mode.unwrap_or(RefreshMode::OnChange),
            href,
        })
    }

    /// Parse a `<Link>` (or legacy `<Url>`) element's `href` and `refreshMode`.
    fn parse_link_element(&mut self) -> Result<(Option<String>, Option<RefreshMode>)> {
        let mut href = None;
        let mut refresh_mode = None;
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"href" => href = self.read_text().ok(),
                    b"refreshMode" => {
                        if let Ok(text) = self.read_text() {
                            // An unrecognized refresh mode falls back to the
                            // caller's default rather than failing the whole
                            // NetworkLink parse.
                            refresh_mode = RefreshMode::from_str(text.trim()).ok();
                        }
                    }
                    _ => {}
                },
                Ok(Event::End(e))
                    if e.name().as_ref() == b"Link" || e.name().as_ref() == b"Url" =>
                {
                    break;
                }
                Ok(Event::Eof) => return Err(Error::kml("Unexpected EOF in Link")),
                Err(e) => return Err(Error::kml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }
        Ok((href, refresh_mode))
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

/// Extract a named attribute's value from a start-tag event, if present.
fn extract_attr(e: &BytesStart, name: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|attr| {
        if attr.key.as_ref() == name {
            Some(String::from_utf8_lossy(&attr.value).into_owned())
        } else {
            None
        }
    })
}

/// Parse a KML boolean text value (`0`/`1`/`true`/`false`, case-insensitive).
fn parse_kml_bool(text: &str) -> bool {
    matches!(text.trim().to_ascii_lowercase().as_str(), "1" | "true")
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

    #[test]
    fn test_parse_kml_bool() {
        assert!(parse_kml_bool("1"));
        assert!(parse_kml_bool("true"));
        assert!(parse_kml_bool("True"));
        assert!(!parse_kml_bool("0"));
        assert!(!parse_kml_bool("false"));
        assert!(!parse_kml_bool(""));
    }

    /// Regression test: parsing a `<Style>` must populate id and every
    /// sub-style, not just return `Style::default()`.
    #[test]
    fn test_parse_style_full() -> Result<()> {
        let xml = r#"<?xml version="1.0"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <Style id="myStyle">
      <IconStyle>
        <color>ff0000ff</color>
        <scale>1.5</scale>
        <Icon>
          <href>http://example.com/icon.png</href>
        </Icon>
      </IconStyle>
      <LineStyle>
        <color>ff00ff00</color>
        <width>3.0</width>
      </LineStyle>
      <PolyStyle>
        <color>7fff0000</color>
        <fill>0</fill>
        <outline>1</outline>
      </PolyStyle>
      <LabelStyle>
        <color>ffffffff</color>
        <scale>0.8</scale>
      </LabelStyle>
    </Style>
  </Document>
</kml>"#;

        let mut parser = KmlParser::new(std::io::Cursor::new(xml.as_bytes().to_vec()))?;
        let doc = parser.parse()?;

        assert_eq!(doc.styles.len(), 1);
        let style = &doc.styles[0];
        assert_eq!(style.id, Some("myStyle".to_string()));

        let icon = style
            .icon_style
            .as_ref()
            .ok_or_else(|| Error::kml("missing icon_style"))?;
        assert_eq!(icon.color, Some("ff0000ff".to_string()));
        assert_eq!(icon.scale, 1.5);
        assert_eq!(icon.href, Some("http://example.com/icon.png".to_string()));

        let line = style
            .line_style
            .as_ref()
            .ok_or_else(|| Error::kml("missing line_style"))?;
        assert_eq!(line.color, Some("ff00ff00".to_string()));
        assert_eq!(line.width, 3.0);

        let poly = style
            .poly_style
            .as_ref()
            .ok_or_else(|| Error::kml("missing poly_style"))?;
        assert_eq!(poly.color, Some("7fff0000".to_string()));
        assert!(!poly.fill);
        assert!(poly.outline);

        let label = style
            .label_style
            .as_ref()
            .ok_or_else(|| Error::kml("missing label_style"))?;
        assert_eq!(label.color, Some("ffffffff".to_string()));
        assert_eq!(label.scale, 0.8);

        Ok(())
    }

    /// Regression test: `NetworkLink` visibility and refreshMode must come
    /// from the actual XML, not hardcoded defaults.
    #[test]
    fn test_parse_network_link_visibility_and_refresh_mode() -> Result<()> {
        let xml = r#"<?xml version="1.0"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <NetworkLink>
      <name>External Data</name>
      <visibility>0</visibility>
      <Link>
        <href>http://example.com/data.kml</href>
        <refreshMode>onInterval</refreshMode>
      </Link>
    </NetworkLink>
  </Document>
</kml>"#;

        let mut parser = KmlParser::new(std::io::Cursor::new(xml.as_bytes().to_vec()))?;
        let doc = parser.parse()?;

        assert_eq!(doc.network_links.len(), 1);
        let link = &doc.network_links[0];
        assert_eq!(link.name, Some("External Data".to_string()));
        assert!(!link.visibility);
        assert_eq!(link.refresh_mode, RefreshMode::OnInterval);
        assert_eq!(link.href, "http://example.com/data.kml");

        Ok(())
    }

    /// A NetworkLink without explicit visibility/refreshMode elements must
    /// fall back to the KML spec defaults (visible, onChange).
    #[test]
    fn test_parse_network_link_defaults_when_absent() -> Result<()> {
        let xml = r#"<?xml version="1.0"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <NetworkLink>
      <name>Defaults</name>
      <href>http://example.com/defaults.kml</href>
    </NetworkLink>
  </Document>
</kml>"#;

        let mut parser = KmlParser::new(std::io::Cursor::new(xml.as_bytes().to_vec()))?;
        let doc = parser.parse()?;

        let link = &doc.network_links[0];
        assert!(link.visibility);
        assert_eq!(link.refresh_mode, RefreshMode::OnChange);
        assert_eq!(link.href, "http://example.com/defaults.kml");

        Ok(())
    }
}
