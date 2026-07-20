//! GML parser.

use super::{GmlFeature, GmlFeatureCollection, GmlGeometry};
use crate::error::{Error, Result};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::io::BufRead;

/// Default coordinate dimensionality per the GML spec when `srsDimension`
/// is absent from every enclosing element.
const DEFAULT_SRS_DIMENSION: usize = 2;

/// GML parser.
pub struct GmlParser<R> {
    reader: Reader<R>,
    /// `srsDimension` inherited from the nearest enclosing element that
    /// declared it (geometry element or `posList`/`pos` itself). Reset per
    /// geometry so a dimension set on one feature never leaks into the next.
    current_srs_dimension: Option<usize>,
}

impl<R: BufRead> GmlParser<R> {
    /// Create new GML parser.
    pub fn new(reader: R) -> Result<Self> {
        let mut xml_reader = Reader::from_reader(reader);
        xml_reader.config_mut().trim_text(true);

        Ok(Self {
            reader: xml_reader,
            current_srs_dimension: None,
        })
    }

    /// Extract the `srsDimension` attribute from a start-tag event, if present.
    fn extract_srs_dimension(e: &BytesStart) -> Option<usize> {
        e.attributes().flatten().find_map(|attr| {
            if attr.key.as_ref() == b"srsDimension" {
                std::str::from_utf8(&attr.value)
                    .ok()
                    .and_then(|s| s.parse::<usize>().ok())
            } else {
                None
            }
        })
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
                Ok(Event::End(e)) if e.name().as_ref() == b"FeatureCollection" => {
                    in_collection = false;
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
                        self.current_srs_dimension = Self::extract_srs_dimension(&e);
                        feature.geometry = self.parse_point().ok();
                    } else if name.contains("LineString") || name.contains("linestring") {
                        self.current_srs_dimension = Self::extract_srs_dimension(&e);
                        feature.geometry = self.parse_linestring().ok();
                    } else if name.contains("Polygon") || name.contains("polygon") {
                        self.current_srs_dimension = Self::extract_srs_dimension(&e);
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
                        // This Start event has already been consumed by the
                        // match above, so we read the tuples directly here
                        // rather than delegating to
                        // `parse_pos_list_or_coordinates` (which expects to
                        // scan forward to its own un-consumed Start tag).
                        if let Some(dim) = Self::extract_srs_dimension(&e) {
                            self.current_srs_dimension = Some(dim);
                        }
                        let text = self.read_text()?;
                        let dim = self.current_srs_dimension.unwrap_or(DEFAULT_SRS_DIMENSION);
                        return parse_coordinate_text(&text, dim);
                    }
                }
                Ok(Event::End(_)) => {}
                Ok(Event::Eof) => return Err(Error::gml("Unexpected EOF in LinearRing")),
                _ => {}
            }
            buf.clear();
        }
    }

    /// Parse a `posList`/`coordinates` element that has NOT yet been
    /// consumed by the caller: scans forward for its Start tag (honoring any
    /// `srsDimension` attribute found there), then reads its tuples.
    fn parse_pos_list_or_coordinates(&mut self) -> Result<Vec<Vec<f64>>> {
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e))
                    if e.name().as_ref() == b"posList" || e.name().as_ref() == b"coordinates" =>
                {
                    if let Some(dim) = Self::extract_srs_dimension(&e) {
                        self.current_srs_dimension = Some(dim);
                    }
                    let text = self.read_text()?;
                    let dim = self.current_srs_dimension.unwrap_or(DEFAULT_SRS_DIMENSION);
                    return parse_coordinate_text(&text, dim);
                }
                Ok(Event::End(_)) => {}
                Ok(Event::Eof) => return Err(Error::gml("Unexpected EOF")),
                Err(e) => return Err(Error::gml(format!("Parse error: {}", e))),
                _ => {}
            }
            buf.clear();
        }
    }

    /// Parse single pos or coordinate.
    fn parse_pos_or_coordinates(&mut self) -> Result<Vec<Vec<f64>>> {
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e))
                    if e.name().as_ref() == b"pos" || e.name().as_ref() == b"coordinates" =>
                {
                    if let Some(dim) = Self::extract_srs_dimension(&e) {
                        self.current_srs_dimension = Some(dim);
                    }
                    let text = self.read_text()?;
                    let dim = self.current_srs_dimension.unwrap_or(DEFAULT_SRS_DIMENSION);
                    return parse_coordinate_text(&text, dim);
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

/// Parse coordinate text (space/comma separated numbers) into tuples of the
/// given `dim`ensionality. The dimension is provided explicitly by the
/// caller (from the `srsDimension` attribute, defaulting to 2 per the GML
/// spec) rather than guessed from the flat number count, which is ambiguous
/// whenever a 2-D vertex count happens to be a multiple of 3.
fn parse_coordinate_text(text: &str, dim: usize) -> Result<Vec<Vec<f64>>> {
    // Guard against a malformed/zero srsDimension causing chunks(0) to panic.
    let dim = dim.max(1);

    let mut coords = Vec::new();
    let numbers: Vec<f64> = text
        .split_whitespace()
        .flat_map(|s| s.split(','))
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();

    for chunk in numbers.chunks(dim) {
        coords.push(chunk.to_vec());
    }

    Ok(coords)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn test_parse_coordinate_text() {
        let text = "1.0 2.0 3.0 4.0";
        let coords = parse_coordinate_text(text, 2).ok();
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
        let coords = parse_coordinate_text(text, 3).ok();
        assert!(coords.is_some());
        if let Some(c) = coords {
            assert_eq!(c.len(), 2);
            assert_eq!(c[0], vec![1.0, 2.0, 3.0]);
        }
    }

    #[test]
    fn test_parse_coordinate_text_zero_dim_does_not_panic() {
        // A malformed/zero srsDimension must not panic via chunks(0); it is
        // clamped to 1.
        let text = "1.0 2.0 3.0";
        let coords = parse_coordinate_text(text, 0).ok();
        assert!(coords.is_some());
        if let Some(c) = coords {
            assert_eq!(c.len(), 3);
            assert_eq!(c[0], vec![1.0]);
        }
    }

    // NOTE: `GmlParser::parse`'s top-level dispatch matches the exact,
    // unprefixed byte strings `b"FeatureCollection"` / `b"featureMember"`
    // (see the `match name.as_ref()` in `parse`), so these fixtures
    // deliberately omit the `gml:` namespace prefix on those two wrapper
    // elements to exercise the parser's actual current behavior. (The
    // writer emitting `<gml:FeatureCollection>` while the parser expects
    // unprefixed `<FeatureCollection>` is a pre-existing, separate mismatch
    // outside the scope of the srsDimension fix under test here.) Geometry
    // elements are also placed as direct children of `featureMember`
    // (rather than wrapped in an intermediate `<Feature>` property
    // container) since `parse_feature_member`'s generic property-capture
    // path is not nesting-aware.

    /// Regression test for the divisibility-by-3 heuristic bug: a 2-D
    /// LineString with exactly 3 vertices (6 flat numbers) must NOT be
    /// misclassified as 3-D. Without an explicit `srsDimension`, GML default
    /// is 2, so `posList` text with 6 numbers must yield three 2-D tuples.
    #[test]
    fn test_gml_2d_linestring_six_numbers_not_misread_as_3d() -> Result<()> {
        let xml = r#"<?xml version="1.0"?>
<FeatureCollection>
  <featureMember>
    <LineString>
      <posList>1.0 2.0 3.0 4.0 5.0 6.0</posList>
    </LineString>
  </featureMember>
</FeatureCollection>"#;

        let reader = BufReader::new(xml.as_bytes());
        let mut parser = GmlParser::new(reader)?;
        let collection = parser.parse()?;

        assert_eq!(collection.features.len(), 1);
        let geom = collection.features[0]
            .geometry
            .as_ref()
            .ok_or_else(|| Error::gml("expected geometry"))?;
        match geom {
            GmlGeometry::LineString { coordinates } => {
                assert_eq!(
                    coordinates.len(),
                    3,
                    "expected 3 two-D vertices, not 2 3-D ones"
                );
                assert_eq!(coordinates[0], vec![1.0, 2.0]);
                assert_eq!(coordinates[1], vec![3.0, 4.0]);
                assert_eq!(coordinates[2], vec![5.0, 6.0]);
            }
            other => {
                return Err(Error::gml(format!(
                    "unexpected geometry variant: {other:?}"
                )));
            }
        }
        Ok(())
    }

    /// An explicit `srsDimension="3"` on the geometry element must route
    /// vertices into 3-D tuples even when the flat count would also parse
    /// unambiguously as 2-D.
    #[test]
    fn test_gml_explicit_srs_dimension_3() -> Result<()> {
        let xml = r#"<?xml version="1.0"?>
<FeatureCollection>
  <featureMember>
    <LineString srsDimension="3">
      <posList>1.0 2.0 3.0 4.0 5.0 6.0</posList>
    </LineString>
  </featureMember>
</FeatureCollection>"#;

        let reader = BufReader::new(xml.as_bytes());
        let mut parser = GmlParser::new(reader)?;
        let collection = parser.parse()?;

        assert_eq!(collection.features.len(), 1);
        let geom = collection.features[0]
            .geometry
            .as_ref()
            .ok_or_else(|| Error::gml("expected geometry"))?;
        match geom {
            GmlGeometry::LineString { coordinates } => {
                assert_eq!(coordinates.len(), 2);
                assert_eq!(coordinates[0], vec![1.0, 2.0, 3.0]);
                assert_eq!(coordinates[1], vec![4.0, 5.0, 6.0]);
            }
            other => {
                return Err(Error::gml(format!(
                    "unexpected geometry variant: {other:?}"
                )));
            }
        }
        Ok(())
    }

    /// `srsDimension` declared directly on `posList` must also be honored.
    #[test]
    fn test_gml_srs_dimension_on_pos_list() -> Result<()> {
        let xml = r#"<?xml version="1.0"?>
<FeatureCollection>
  <featureMember>
    <LineString>
      <posList srsDimension="3">1.0 2.0 3.0 4.0 5.0 6.0</posList>
    </LineString>
  </featureMember>
</FeatureCollection>"#;

        let reader = BufReader::new(xml.as_bytes());
        let mut parser = GmlParser::new(reader)?;
        let collection = parser.parse()?;

        assert_eq!(collection.features.len(), 1);
        let geom = collection.features[0]
            .geometry
            .as_ref()
            .ok_or_else(|| Error::gml("expected geometry"))?;
        match geom {
            GmlGeometry::LineString { coordinates } => {
                assert_eq!(coordinates.len(), 2);
            }
            other => {
                return Err(Error::gml(format!(
                    "unexpected geometry variant: {other:?}"
                )));
            }
        }
        Ok(())
    }
}
