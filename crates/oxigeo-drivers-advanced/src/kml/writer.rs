//! KML writer.

use super::{
    KmlDocument, NetworkLink, Placemark, Style, StyleMap, features::Geometry as KmlGeometry,
};
use crate::error::Result;
use std::io::Write;

/// KML writer.
pub struct KmlWriter<W> {
    writer: W,
}

impl<W: Write> KmlWriter<W> {
    /// Create new KML writer.
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    /// Write KML document.
    pub fn write(&mut self, doc: &KmlDocument) -> Result<()> {
        self.write_header()?;
        self.write_document(doc)?;
        self.write_footer()?;
        Ok(())
    }

    /// Write XML header.
    fn write_header(&mut self) -> Result<()> {
        writeln!(self.writer, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>")?;
        writeln!(
            self.writer,
            "<kml xmlns=\"http://www.opengis.net/kml/2.2\">"
        )?;
        writeln!(self.writer, "  <Document>")?;
        Ok(())
    }

    /// Write document content.
    fn write_document(&mut self, doc: &KmlDocument) -> Result<()> {
        if let Some(name) = &doc.name {
            writeln!(self.writer, "    <name>{}</name>", escape_xml(name))?;
        }
        if let Some(desc) = &doc.description {
            writeln!(
                self.writer,
                "    <description>{}</description>",
                escape_xml(desc)
            )?;
        }

        // Write style definitions before they are referenced by placemarks
        // or style maps.
        for style in &doc.styles {
            self.write_style(style)?;
        }
        for style_map in &doc.style_maps {
            self.write_style_map(style_map)?;
        }
        for link in &doc.network_links {
            self.write_network_link(link)?;
        }

        // Write placemarks
        for placemark in &doc.placemarks {
            self.write_placemark(placemark)?;
        }

        Ok(())
    }

    /// Write a `<Style>` element, including any Icon/Line/Poly/Label sub-styles.
    fn write_style(&mut self, style: &Style) -> Result<()> {
        if let Some(id) = &style.id {
            writeln!(self.writer, "    <Style id=\"{}\">", escape_xml(id))?;
        } else {
            writeln!(self.writer, "    <Style>")?;
        }

        if let Some(icon) = &style.icon_style {
            writeln!(self.writer, "      <IconStyle>")?;
            if let Some(color) = &icon.color {
                writeln!(self.writer, "        <color>{}</color>", escape_xml(color))?;
            }
            writeln!(self.writer, "        <scale>{}</scale>", icon.scale)?;
            if let Some(href) = &icon.href {
                writeln!(self.writer, "        <Icon>")?;
                writeln!(self.writer, "          <href>{}</href>", escape_xml(href))?;
                writeln!(self.writer, "        </Icon>")?;
            }
            writeln!(self.writer, "      </IconStyle>")?;
        }

        if let Some(line) = &style.line_style {
            writeln!(self.writer, "      <LineStyle>")?;
            if let Some(color) = &line.color {
                writeln!(self.writer, "        <color>{}</color>", escape_xml(color))?;
            }
            writeln!(self.writer, "        <width>{}</width>", line.width)?;
            writeln!(self.writer, "      </LineStyle>")?;
        }

        if let Some(poly) = &style.poly_style {
            writeln!(self.writer, "      <PolyStyle>")?;
            if let Some(color) = &poly.color {
                writeln!(self.writer, "        <color>{}</color>", escape_xml(color))?;
            }
            writeln!(self.writer, "        <fill>{}</fill>", i32::from(poly.fill))?;
            writeln!(
                self.writer,
                "        <outline>{}</outline>",
                i32::from(poly.outline)
            )?;
            writeln!(self.writer, "      </PolyStyle>")?;
        }

        if let Some(label) = &style.label_style {
            writeln!(self.writer, "      <LabelStyle>")?;
            if let Some(color) = &label.color {
                writeln!(self.writer, "        <color>{}</color>", escape_xml(color))?;
            }
            writeln!(self.writer, "        <scale>{}</scale>", label.scale)?;
            writeln!(self.writer, "      </LabelStyle>")?;
        }

        writeln!(self.writer, "    </Style>")?;
        Ok(())
    }

    /// Write a `<StyleMap>` element (normal/highlight style URL pair).
    fn write_style_map(&mut self, style_map: &StyleMap) -> Result<()> {
        if let Some(id) = &style_map.id {
            writeln!(self.writer, "    <StyleMap id=\"{}\">", escape_xml(id))?;
        } else {
            writeln!(self.writer, "    <StyleMap>")?;
        }

        writeln!(self.writer, "      <Pair>")?;
        writeln!(self.writer, "        <key>normal</key>")?;
        writeln!(
            self.writer,
            "        <styleUrl>{}</styleUrl>",
            escape_xml(&style_map.normal)
        )?;
        writeln!(self.writer, "      </Pair>")?;

        writeln!(self.writer, "      <Pair>")?;
        writeln!(self.writer, "        <key>highlight</key>")?;
        writeln!(
            self.writer,
            "        <styleUrl>{}</styleUrl>",
            escape_xml(&style_map.highlight)
        )?;
        writeln!(self.writer, "      </Pair>")?;

        writeln!(self.writer, "    </StyleMap>")?;
        Ok(())
    }

    /// Write a `<NetworkLink>` element.
    fn write_network_link(&mut self, link: &NetworkLink) -> Result<()> {
        writeln!(self.writer, "    <NetworkLink>")?;
        if let Some(name) = &link.name {
            writeln!(self.writer, "      <name>{}</name>", escape_xml(name))?;
        }
        writeln!(
            self.writer,
            "      <visibility>{}</visibility>",
            i32::from(link.visibility)
        )?;
        writeln!(self.writer, "      <Link>")?;
        writeln!(
            self.writer,
            "        <href>{}</href>",
            escape_xml(&link.href)
        )?;
        writeln!(
            self.writer,
            "        <refreshMode>{}</refreshMode>",
            link.refresh_mode.as_str()
        )?;
        writeln!(self.writer, "      </Link>")?;
        writeln!(self.writer, "    </NetworkLink>")?;
        Ok(())
    }

    /// Write a placemark's `<ExtendedData>` block, if it has any data fields.
    fn write_extended_data(&mut self, data: &[(String, String)]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        writeln!(self.writer, "      <ExtendedData>")?;
        for (key, value) in data {
            writeln!(self.writer, "        <Data name=\"{}\">", escape_xml(key))?;
            writeln!(
                self.writer,
                "          <value>{}</value>",
                escape_xml(value)
            )?;
            writeln!(self.writer, "        </Data>")?;
        }
        writeln!(self.writer, "      </ExtendedData>")?;
        Ok(())
    }

    /// Write placemark.
    fn write_placemark(&mut self, placemark: &Placemark) -> Result<()> {
        writeln!(self.writer, "    <Placemark>")?;

        if let Some(name) = &placemark.name {
            writeln!(self.writer, "      <name>{}</name>", escape_xml(name))?;
        }
        if let Some(desc) = &placemark.description {
            writeln!(
                self.writer,
                "      <description>{}</description>",
                escape_xml(desc)
            )?;
        }
        if let Some(style_url) = &placemark.style_url {
            writeln!(
                self.writer,
                "      <styleUrl>{}</styleUrl>",
                escape_xml(style_url)
            )?;
        }

        if let Some(geom) = &placemark.geometry {
            self.write_geometry(geom)?;
        }

        self.write_extended_data(&placemark.extended_data)?;

        writeln!(self.writer, "    </Placemark>")?;
        Ok(())
    }

    /// Write geometry.
    fn write_geometry(&mut self, geom: &KmlGeometry) -> Result<()> {
        match geom {
            KmlGeometry::Point(coord) => {
                writeln!(self.writer, "      <Point>")?;
                writeln!(
                    self.writer,
                    "        <coordinates>{}</coordinates>",
                    coord.to_kml_string()
                )?;
                writeln!(self.writer, "      </Point>")?;
            }
            KmlGeometry::LineString(coords) => {
                writeln!(self.writer, "      <LineString>")?;
                write!(self.writer, "        <coordinates>")?;
                for coord in coords {
                    write!(self.writer, "{} ", coord.to_kml_string())?;
                }
                writeln!(self.writer, "</coordinates>")?;
                writeln!(self.writer, "      </LineString>")?;
            }
            KmlGeometry::Polygon { outer, inner } => {
                writeln!(self.writer, "      <Polygon>")?;
                writeln!(self.writer, "        <outerBoundaryIs>")?;
                writeln!(self.writer, "          <LinearRing>")?;
                write!(self.writer, "            <coordinates>")?;
                for coord in outer {
                    write!(self.writer, "{} ", coord.to_kml_string())?;
                }
                writeln!(self.writer, "</coordinates>")?;
                writeln!(self.writer, "          </LinearRing>")?;
                writeln!(self.writer, "        </outerBoundaryIs>")?;
                for hole in inner {
                    writeln!(self.writer, "        <innerBoundaryIs>")?;
                    writeln!(self.writer, "          <LinearRing>")?;
                    write!(self.writer, "            <coordinates>")?;
                    for coord in hole {
                        write!(self.writer, "{} ", coord.to_kml_string())?;
                    }
                    writeln!(self.writer, "</coordinates>")?;
                    writeln!(self.writer, "          </LinearRing>")?;
                    writeln!(self.writer, "        </innerBoundaryIs>")?;
                }
                writeln!(self.writer, "      </Polygon>")?;
            }
            KmlGeometry::MultiGeometry(geoms) => {
                writeln!(self.writer, "      <MultiGeometry>")?;
                for g in geoms {
                    self.write_geometry(g)?;
                }
                writeln!(self.writer, "      </MultiGeometry>")?;
            }
        }
        Ok(())
    }

    /// Write footer.
    fn write_footer(&mut self) -> Result<()> {
        writeln!(self.writer, "  </Document>")?;
        writeln!(self.writer, "</kml>")?;
        Ok(())
    }
}

/// Escape XML special characters.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kml::features::Coordinates;
    use crate::kml::{IconStyle, LabelStyle, LineStyle, PolyStyle, RefreshMode};

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("test & < >"), "test &amp; &lt; &gt;");
        assert_eq!(escape_xml("quote \" and '"), "quote &quot; and &apos;");
    }

    #[test]
    fn test_write_empty_document() -> Result<()> {
        let mut buf = Vec::new();
        let doc = KmlDocument::new();
        let mut writer = KmlWriter::new(&mut buf);
        writer.write(&doc)?;

        let output =
            String::from_utf8(buf).map_err(|e| crate::error::Error::encoding(e.to_string()))?;
        assert!(output.contains("<kml"));
        assert!(output.contains("<Document>"));
        Ok(())
    }

    #[test]
    fn test_write_document_with_placemark() -> Result<()> {
        let mut buf = Vec::new();
        let mut doc = KmlDocument::new().with_name("Test");

        let placemark = Placemark::new()
            .with_name("Test Point")
            .with_geometry(KmlGeometry::Point(Coordinates::new(-122.08, 37.42)));

        doc.add_placemark(placemark);

        let mut writer = KmlWriter::new(&mut buf);
        writer.write(&doc)?;

        let output =
            String::from_utf8(buf).map_err(|e| crate::error::Error::encoding(e.to_string()))?;
        assert!(output.contains("Test Point"));
        assert!(output.contains("-122.08"));
        Ok(())
    }

    #[test]
    fn test_write_polygon_with_holes() -> Result<()> {
        let mut buf = Vec::new();
        let mut doc = KmlDocument::new();

        let outer = vec![
            Coordinates::new(0.0, 0.0),
            Coordinates::new(10.0, 0.0),
            Coordinates::new(10.0, 10.0),
            Coordinates::new(0.0, 10.0),
            Coordinates::new(0.0, 0.0),
        ];
        let hole = vec![
            Coordinates::new(2.0, 2.0),
            Coordinates::new(4.0, 2.0),
            Coordinates::new(4.0, 4.0),
            Coordinates::new(2.0, 4.0),
            Coordinates::new(2.0, 2.0),
        ];

        let placemark = Placemark::new().with_geometry(KmlGeometry::Polygon {
            outer,
            inner: vec![hole],
        });
        doc.add_placemark(placemark);

        let mut writer = KmlWriter::new(&mut buf);
        writer.write(&doc)?;

        let output =
            String::from_utf8(buf).map_err(|e| crate::error::Error::encoding(e.to_string()))?;
        assert!(output.contains("<outerBoundaryIs>"));
        assert!(output.contains("<innerBoundaryIs>"));
        assert!(output.contains("2,2,0"));
        Ok(())
    }

    #[test]
    fn test_write_polygon_without_holes_omits_inner_boundary() -> Result<()> {
        let mut buf = Vec::new();
        let mut doc = KmlDocument::new();

        let outer = vec![
            Coordinates::new(0.0, 0.0),
            Coordinates::new(10.0, 0.0),
            Coordinates::new(10.0, 10.0),
        ];
        let placemark = Placemark::new().with_geometry(KmlGeometry::Polygon {
            outer,
            inner: vec![],
        });
        doc.add_placemark(placemark);

        let mut writer = KmlWriter::new(&mut buf);
        writer.write(&doc)?;

        let output =
            String::from_utf8(buf).map_err(|e| crate::error::Error::encoding(e.to_string()))?;
        assert!(!output.contains("<innerBoundaryIs>"));
        Ok(())
    }

    #[test]
    fn test_write_style_full() -> Result<()> {
        let mut buf = Vec::new();
        let mut doc = KmlDocument::new();

        let style = Style::new()
            .with_id("style1")
            .with_icon_style(
                IconStyle::new()
                    .with_color("ff0000ff")
                    .with_scale(1.5)
                    .with_href("http://example.com/icon.png"),
            )
            .with_line_style(LineStyle::new().with_color("ff00ff00").with_width(2.0))
            .with_poly_style(
                PolyStyle::new()
                    .with_color("7fff0000")
                    .with_fill(true)
                    .with_outline(false),
            )
            .with_label_style(LabelStyle::new().with_color("ffffffff").with_scale(1.2));

        doc.add_style(style);

        let mut writer = KmlWriter::new(&mut buf);
        writer.write(&doc)?;

        let output =
            String::from_utf8(buf).map_err(|e| crate::error::Error::encoding(e.to_string()))?;
        assert!(output.contains("<Style id=\"style1\">"));
        assert!(output.contains("<IconStyle>"));
        assert!(output.contains("ff0000ff"));
        assert!(output.contains("<Icon>"));
        assert!(output.contains("http://example.com/icon.png"));
        assert!(output.contains("<LineStyle>"));
        assert!(output.contains("<width>2</width>"));
        assert!(output.contains("<PolyStyle>"));
        assert!(output.contains("<fill>1</fill>"));
        assert!(output.contains("<outline>0</outline>"));
        assert!(output.contains("<LabelStyle>"));
        Ok(())
    }

    #[test]
    fn test_write_style_map() -> Result<()> {
        let mut buf = Vec::new();
        let mut doc = KmlDocument::new();

        doc.add_style_map(StyleMap::new("#normalStyle", "#highlightStyle").with_id("map1"));

        let mut writer = KmlWriter::new(&mut buf);
        writer.write(&doc)?;

        let output =
            String::from_utf8(buf).map_err(|e| crate::error::Error::encoding(e.to_string()))?;
        assert!(output.contains("<StyleMap id=\"map1\">"));
        assert!(output.contains("#normalStyle"));
        assert!(output.contains("#highlightStyle"));
        assert!(output.contains("<key>normal</key>"));
        assert!(output.contains("<key>highlight</key>"));
        Ok(())
    }

    #[test]
    fn test_write_network_link() -> Result<()> {
        let mut buf = Vec::new();
        let mut doc = KmlDocument::new();

        doc.add_network_link(NetworkLink {
            name: Some("External".to_string()),
            visibility: false,
            refresh_mode: RefreshMode::OnInterval,
            href: "http://example.com/data.kml".to_string(),
        });

        let mut writer = KmlWriter::new(&mut buf);
        writer.write(&doc)?;

        let output =
            String::from_utf8(buf).map_err(|e| crate::error::Error::encoding(e.to_string()))?;
        assert!(output.contains("<NetworkLink>"));
        assert!(output.contains("External"));
        assert!(output.contains("<visibility>0</visibility>"));
        assert!(output.contains("http://example.com/data.kml"));
        assert!(output.contains("<refreshMode>onInterval</refreshMode>"));
        Ok(())
    }

    #[test]
    fn test_write_placemark_extended_data() -> Result<()> {
        let mut buf = Vec::new();
        let mut doc = KmlDocument::new();

        let mut placemark = Placemark::new().with_name("With Data");
        placemark.add_data("population", "1000");
        placemark.add_data("area_km2", "42.5");
        doc.add_placemark(placemark);

        let mut writer = KmlWriter::new(&mut buf);
        writer.write(&doc)?;

        let output =
            String::from_utf8(buf).map_err(|e| crate::error::Error::encoding(e.to_string()))?;
        assert!(output.contains("<ExtendedData>"));
        assert!(output.contains("<Data name=\"population\">"));
        assert!(output.contains("<value>1000</value>"));
        assert!(output.contains("<Data name=\"area_km2\">"));
        assert!(output.contains("<value>42.5</value>"));
        Ok(())
    }

    #[test]
    fn test_write_placemark_without_extended_data_omits_element() -> Result<()> {
        let mut buf = Vec::new();
        let mut doc = KmlDocument::new();
        doc.add_placemark(Placemark::new().with_name("No Data"));

        let mut writer = KmlWriter::new(&mut buf);
        writer.write(&doc)?;

        let output =
            String::from_utf8(buf).map_err(|e| crate::error::Error::encoding(e.to_string()))?;
        assert!(!output.contains("<ExtendedData>"));
        Ok(())
    }
}
