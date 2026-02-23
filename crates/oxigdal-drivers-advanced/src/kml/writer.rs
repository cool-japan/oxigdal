//! KML writer.

use super::{KmlDocument, Placemark, features::Geometry as KmlGeometry};
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

        // Write placemarks
        for placemark in &doc.placemarks {
            self.write_placemark(placemark)?;
        }

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
            KmlGeometry::Polygon { outer, inner: _ } => {
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
}
