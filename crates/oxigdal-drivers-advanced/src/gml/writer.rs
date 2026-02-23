//! GML writer.

use super::{GmlFeatureCollection, GmlGeometry};
use crate::error::Result;
use std::io::Write;

/// GML writer.
pub struct GmlWriter<W> {
    writer: W,
}

impl<W: Write> GmlWriter<W> {
    /// Create new GML writer.
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    /// Write GML feature collection.
    pub fn write(&mut self, collection: &GmlFeatureCollection) -> Result<()> {
        self.write_header(collection)?;
        self.write_features(collection)?;
        self.write_footer()?;
        Ok(())
    }

    /// Write GML header.
    fn write_header(&mut self, collection: &GmlFeatureCollection) -> Result<()> {
        writeln!(self.writer, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>")?;
        writeln!(
            self.writer,
            "<gml:FeatureCollection xmlns:gml=\"http://www.opengis.net/gml/3.2\">"
        )?;

        if let Some(crs) = &collection.crs {
            writeln!(self.writer, "  <gml:boundedBy>")?;
            writeln!(
                self.writer,
                "    <gml:Envelope srsName=\"{}\">",
                escape_xml(crs)
            )?;
            if let Some(bounds) = &collection.bounds {
                writeln!(
                    self.writer,
                    "      <gml:lowerCorner>{} {}</gml:lowerCorner>",
                    bounds.min_x, bounds.min_y
                )?;
                writeln!(
                    self.writer,
                    "      <gml:upperCorner>{} {}</gml:upperCorner>",
                    bounds.max_x, bounds.max_y
                )?;
            }
            writeln!(self.writer, "    </gml:Envelope>")?;
            writeln!(self.writer, "  </gml:boundedBy>")?;
        }

        Ok(())
    }

    /// Write features.
    fn write_features(&mut self, collection: &GmlFeatureCollection) -> Result<()> {
        for feature in &collection.features {
            writeln!(self.writer, "  <gml:featureMember>")?;
            writeln!(self.writer, "    <Feature>")?;

            if let Some(id) = &feature.id {
                writeln!(self.writer, "      <gml:id>{}</gml:id>", escape_xml(id))?;
            }

            // Write properties
            for prop in &feature.properties {
                writeln!(
                    self.writer,
                    "      <{}>{}</{}>",
                    escape_xml(&prop.name),
                    escape_xml(&prop.value),
                    escape_xml(&prop.name)
                )?;
            }

            // Write geometry
            if let Some(geom) = &feature.geometry {
                self.write_geometry(geom)?;
            }

            writeln!(self.writer, "    </Feature>")?;
            writeln!(self.writer, "  </gml:featureMember>")?;
        }
        Ok(())
    }

    /// Write geometry.
    fn write_geometry(&mut self, geom: &GmlGeometry) -> Result<()> {
        match geom {
            GmlGeometry::Point { coordinates } => {
                writeln!(self.writer, "      <gml:Point>")?;
                write!(self.writer, "        <gml:pos>")?;
                for coord in coordinates {
                    write!(self.writer, "{} ", coord)?;
                }
                writeln!(self.writer, "</gml:pos>")?;
                writeln!(self.writer, "      </gml:Point>")?;
            }
            GmlGeometry::LineString { coordinates } => {
                writeln!(self.writer, "      <gml:LineString>")?;
                write!(self.writer, "        <gml:posList>")?;
                for coord in coordinates {
                    for c in coord {
                        write!(self.writer, "{} ", c)?;
                    }
                }
                writeln!(self.writer, "</gml:posList>")?;
                writeln!(self.writer, "      </gml:LineString>")?;
            }
            GmlGeometry::Polygon { exterior, interior } => {
                writeln!(self.writer, "      <gml:Polygon>")?;
                writeln!(self.writer, "        <gml:exterior>")?;
                writeln!(self.writer, "          <gml:LinearRing>")?;
                write!(self.writer, "            <gml:posList>")?;
                for coord in exterior {
                    for c in coord {
                        write!(self.writer, "{} ", c)?;
                    }
                }
                writeln!(self.writer, "</gml:posList>")?;
                writeln!(self.writer, "          </gml:LinearRing>")?;
                writeln!(self.writer, "        </gml:exterior>")?;

                for ring in interior {
                    writeln!(self.writer, "        <gml:interior>")?;
                    writeln!(self.writer, "          <gml:LinearRing>")?;
                    write!(self.writer, "            <gml:posList>")?;
                    for coord in ring {
                        for c in coord {
                            write!(self.writer, "{} ", c)?;
                        }
                    }
                    writeln!(self.writer, "</gml:posList>")?;
                    writeln!(self.writer, "          </gml:LinearRing>")?;
                    writeln!(self.writer, "        </gml:interior>")?;
                }

                writeln!(self.writer, "      </gml:Polygon>")?;
            }
            GmlGeometry::MultiGeometry { geometries } => {
                writeln!(self.writer, "      <gml:MultiGeometry>")?;
                for g in geometries {
                    self.write_geometry(g)?;
                }
                writeln!(self.writer, "      </gml:MultiGeometry>")?;
            }
        }
        Ok(())
    }

    /// Write footer.
    fn write_footer(&mut self) -> Result<()> {
        writeln!(self.writer, "</gml:FeatureCollection>")?;
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
    use crate::gml::{GmlFeature, GmlFeatureCollection};

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("test & < >"), "test &amp; &lt; &gt;");
    }

    #[test]
    fn test_write_empty_collection() -> Result<()> {
        let mut buf = Vec::new();
        let collection = GmlFeatureCollection::new();
        let mut writer = GmlWriter::new(&mut buf);
        writer.write(&collection)?;

        let output =
            String::from_utf8(buf).map_err(|e| crate::error::Error::encoding(e.to_string()))?;
        assert!(output.contains("<gml:FeatureCollection"));
        Ok(())
    }

    #[test]
    fn test_write_collection_with_feature() -> Result<()> {
        let mut buf = Vec::new();
        let mut collection = GmlFeatureCollection::new();

        let mut feature = GmlFeature::new().with_id("f1");
        feature.add_property("name", "Test Feature");

        collection.add_feature(feature);

        let mut writer = GmlWriter::new(&mut buf);
        writer.write(&collection)?;

        let output =
            String::from_utf8(buf).map_err(|e| crate::error::Error::encoding(e.to_string()))?;
        assert!(output.contains("Test Feature"));
        assert!(output.contains("f1"));
        Ok(())
    }
}
