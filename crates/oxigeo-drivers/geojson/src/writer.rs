//! GeoJSON writer implementation
//!
//! This module provides efficient writing and serialization of GeoJSON objects
//! with support for pretty-printing, streaming, and newline-delimited GeoJSON
//! (GeoJSONL / GeoJSON-seq).

use std::io::Write;
use std::path::Path;

use serde_json::ser::{CompactFormatter, PrettyFormatter, Serializer};

use crate::error::Result;
use crate::types::{
    Feature, FeatureCollection, Geometry, GeometryCollection, LineString, MultiLineString,
    MultiPoint, MultiPolygon, Point, Polygon,
};

/// Writer configuration
#[derive(Debug, Clone)]
pub struct WriterConfig {
    /// Enable pretty-printing (indentation)
    pub pretty: bool,
    /// Indentation string (e.g., "  " or "\t")
    pub indent: String,
    /// Write bounding boxes
    pub write_bbox: bool,
    /// Compute bounding boxes if not present
    pub compute_bbox: bool,
    /// Write CRS (not recommended per RFC 7946)
    pub write_crs: bool,
    /// Significant digits for coordinates
    pub coordinate_precision: Option<usize>,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self {
            pretty: false,
            indent: "  ".to_string(),
            write_bbox: false,
            compute_bbox: false,
            write_crs: false,
            coordinate_precision: None,
        }
    }
}

impl WriterConfig {
    /// Creates a config for pretty-printed output
    pub fn pretty() -> Self {
        Self {
            pretty: true,
            ..Default::default()
        }
    }

    /// Creates a config for compact output
    pub fn compact() -> Self {
        Self {
            pretty: false,
            ..Default::default()
        }
    }

    /// Sets coordinate precision (significant digits)
    pub fn with_precision(mut self, precision: usize) -> Self {
        self.coordinate_precision = Some(precision);
        self
    }

    /// Enables bounding box writing
    pub fn with_bbox(mut self, compute: bool) -> Self {
        self.write_bbox = true;
        self.compute_bbox = compute;
        self
    }

    /// Enables CRS writing
    pub fn with_crs(mut self) -> Self {
        self.write_crs = true;
        self
    }
}

// ─── Coordinate precision helpers ────────────────────────────────────────────

/// Rounds every element of a position vector to `precision` decimal places.
fn round_position(pos: &[f64], precision: usize) -> Vec<f64> {
    let factor = 10_f64.powi(precision as i32);
    pos.iter().map(|&v| (v * factor).round() / factor).collect()
}

/// Returns a new `Geometry` with all coordinate values rounded to `precision`
/// decimal places. `bbox` fields are dropped (caller re-computes or removes
/// them as needed).
fn apply_precision_to_geometry(geom: &Geometry, precision: usize) -> Geometry {
    match geom {
        Geometry::Point(p) => Geometry::Point(Point {
            coordinates: round_position(&p.coordinates, precision),
            bbox: None,
        }),
        Geometry::LineString(ls) => Geometry::LineString(LineString {
            coordinates: ls
                .coordinates
                .iter()
                .map(|pos| round_position(pos, precision))
                .collect(),
            bbox: None,
        }),
        Geometry::Polygon(poly) => Geometry::Polygon(Polygon {
            coordinates: poly
                .coordinates
                .iter()
                .map(|ring| {
                    ring.iter()
                        .map(|pos| round_position(pos, precision))
                        .collect()
                })
                .collect(),
            bbox: None,
        }),
        Geometry::MultiPoint(mp) => Geometry::MultiPoint(MultiPoint {
            coordinates: mp
                .coordinates
                .iter()
                .map(|pos| round_position(pos, precision))
                .collect(),
            bbox: None,
        }),
        Geometry::MultiLineString(mls) => Geometry::MultiLineString(MultiLineString {
            coordinates: mls
                .coordinates
                .iter()
                .map(|line| {
                    line.iter()
                        .map(|pos| round_position(pos, precision))
                        .collect()
                })
                .collect(),
            bbox: None,
        }),
        Geometry::MultiPolygon(mpoly) => Geometry::MultiPolygon(MultiPolygon {
            coordinates: mpoly
                .coordinates
                .iter()
                .map(|rings| {
                    rings
                        .iter()
                        .map(|ring| {
                            ring.iter()
                                .map(|pos| round_position(pos, precision))
                                .collect()
                        })
                        .collect()
                })
                .collect(),
            bbox: None,
        }),
        Geometry::GeometryCollection(gc) => Geometry::GeometryCollection(GeometryCollection {
            geometries: gc
                .geometries
                .iter()
                .map(|g| apply_precision_to_geometry(g, precision))
                .collect(),
            bbox: None,
        }),
    }
}

/// Apply coordinate precision to a `Feature`'s geometry (if any).
fn apply_precision_to_feature(feature: Feature, precision: usize) -> Feature {
    let geometry = feature
        .geometry
        .map(|g| apply_precision_to_geometry(&g, precision));
    Feature {
        geometry,
        ..feature
    }
}

/// GeoJSON writer
///
/// Provides methods to write GeoJSON objects to various outputs.
pub struct GeoJsonWriter<W: Write> {
    writer: W,
    config: WriterConfig,
}

impl<W: Write> GeoJsonWriter<W> {
    /// Creates a new GeoJSON writer with default configuration
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            config: WriterConfig::default(),
        }
    }

    /// Creates a new GeoJSON writer with custom configuration
    pub fn with_config(writer: W, config: WriterConfig) -> Self {
        Self { writer, config }
    }

    /// Creates a new GeoJSON writer for pretty-printed output
    pub fn pretty(writer: W) -> Self {
        Self {
            writer,
            config: WriterConfig::pretty(),
        }
    }

    /// Creates a new GeoJSON writer for compact output
    pub fn compact(writer: W) -> Self {
        Self {
            writer,
            config: WriterConfig::compact(),
        }
    }

    /// Writes a FeatureCollection
    pub fn write_feature_collection(&mut self, fc: &FeatureCollection) -> Result<()> {
        let mut fc = fc.clone();

        // Apply coordinate precision before any other processing
        if let Some(precision) = self.config.coordinate_precision {
            for feature in &mut fc.features {
                if let Some(geom) = feature.geometry.take() {
                    feature.geometry = Some(apply_precision_to_geometry(&geom, precision));
                }
            }
        }

        // Compute bounding box if requested
        if self.config.compute_bbox && fc.bbox.is_none() {
            fc.compute_bbox();
        }

        // Remove bbox if not requested
        if !self.config.write_bbox {
            fc.bbox = None;
        }

        // Remove CRS if not requested
        if !self.config.write_crs {
            fc.crs = None;
        }

        self.write_json(&fc)?;
        Ok(())
    }

    /// Writes a Feature
    pub fn write_feature(&mut self, feature: &Feature) -> Result<()> {
        let mut feature = feature.clone();

        // Apply coordinate precision before any other processing
        if let Some(precision) = self.config.coordinate_precision {
            feature = apply_precision_to_feature(feature, precision);
        }

        // Compute bounding box if requested
        if self.config.compute_bbox && feature.bbox.is_none() {
            feature.compute_bbox();
        }

        // Remove bbox if not requested
        if !self.config.write_bbox {
            feature.bbox = None;
        }

        // Remove CRS if not requested
        if !self.config.write_crs {
            feature.crs = None;
        }

        self.write_json(&feature)?;
        Ok(())
    }

    /// Writes a Geometry
    pub fn write_geometry(&mut self, geometry: &Geometry) -> Result<()> {
        let mut geometry = geometry.clone();

        // Apply coordinate precision before any other processing
        if let Some(precision) = self.config.coordinate_precision {
            geometry = apply_precision_to_geometry(&geometry, precision);
        }

        // Compute bounding box if requested
        if self.config.compute_bbox {
            match &mut geometry {
                Geometry::Point(p) if p.bbox.is_none() => p.bbox = p.compute_bbox(),
                Geometry::LineString(ls) if ls.bbox.is_none() => ls.bbox = ls.compute_bbox(),
                Geometry::Polygon(p) if p.bbox.is_none() => p.bbox = p.compute_bbox(),
                Geometry::MultiPoint(mp) if mp.bbox.is_none() => mp.bbox = mp.compute_bbox(),
                Geometry::MultiLineString(mls) if mls.bbox.is_none() => {
                    mls.bbox = mls.compute_bbox()
                }
                Geometry::MultiPolygon(mp) if mp.bbox.is_none() => mp.bbox = mp.compute_bbox(),
                Geometry::GeometryCollection(gc) if gc.bbox.is_none() => {
                    gc.bbox = gc.compute_bbox()
                }
                _ => {}
            }
        }

        // Remove bbox if not requested
        if !self.config.write_bbox {
            match &mut geometry {
                Geometry::Point(p) => p.bbox = None,
                Geometry::LineString(ls) => ls.bbox = None,
                Geometry::Polygon(p) => p.bbox = None,
                Geometry::MultiPoint(mp) => mp.bbox = None,
                Geometry::MultiLineString(mls) => mls.bbox = None,
                Geometry::MultiPolygon(mp) => mp.bbox = None,
                Geometry::GeometryCollection(gc) => gc.bbox = None,
            }
        }

        self.write_json(&geometry)?;
        Ok(())
    }

    /// Writes multiple features (streaming)
    ///
    /// This writes features one at a time, which is more memory-efficient
    /// for large collections.
    pub fn write_features<I>(&mut self, features: I) -> Result<()>
    where
        I: IntoIterator<Item = Feature>,
    {
        self.write_str(r#"{"type":"FeatureCollection","features":["#)?;

        let mut first = true;
        for feature in features {
            if !first {
                self.write_str(",")?;
            }
            first = false;

            self.write_feature(&feature)?;
        }

        self.write_str("]}")?;
        Ok(())
    }

    /// Starts writing a FeatureCollection (for streaming)
    pub fn start_feature_collection(&mut self) -> Result<()> {
        self.write_str(r#"{"type":"FeatureCollection","features":["#)?;
        Ok(())
    }

    /// Writes a feature to an ongoing FeatureCollection
    pub fn write_feature_streaming(&mut self, feature: &Feature, first: bool) -> Result<()> {
        if !first {
            self.write_str(",")?;
        }
        self.write_feature(feature)?;
        Ok(())
    }

    /// Finishes writing a FeatureCollection
    pub fn finish_feature_collection(&mut self) -> Result<()> {
        self.write_str("]}")?;
        self.writer.flush()?;
        Ok(())
    }

    /// Flushes the writer
    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }

    /// Consumes the writer and returns the inner writer
    pub fn into_inner(self) -> W {
        self.writer
    }

    /// Writes a JSON-serializable value
    fn write_json<T: serde::Serialize>(&mut self, value: &T) -> Result<()> {
        if self.config.pretty {
            let formatter = PrettyFormatter::with_indent(self.config.indent.as_bytes());
            let mut serializer = Serializer::with_formatter(&mut self.writer, formatter);
            value.serialize(&mut serializer)?;
        } else {
            let formatter = CompactFormatter;
            let mut serializer = Serializer::with_formatter(&mut self.writer, formatter);
            value.serialize(&mut serializer)?;
        }
        Ok(())
    }

    /// Writes a string directly
    fn write_str(&mut self, s: &str) -> Result<()> {
        self.writer.write_all(s.as_bytes())?;
        Ok(())
    }
}

/// Converts a FeatureCollection to a GeoJSON string
pub fn to_string(fc: &FeatureCollection) -> Result<String> {
    let json = serde_json::to_string(fc)?;
    Ok(json)
}

/// Converts a FeatureCollection to a pretty-printed GeoJSON string
pub fn to_string_pretty(fc: &FeatureCollection) -> Result<String> {
    let json = serde_json::to_string_pretty(fc)?;
    Ok(json)
}

/// Converts a Feature to a GeoJSON string
pub fn feature_to_string(feature: &Feature) -> Result<String> {
    let json = serde_json::to_string(feature)?;
    Ok(json)
}

/// Converts a Feature to a pretty-printed GeoJSON string
pub fn feature_to_string_pretty(feature: &Feature) -> Result<String> {
    let json = serde_json::to_string_pretty(feature)?;
    Ok(json)
}

/// Converts a Geometry to a GeoJSON string
pub fn geometry_to_string(geometry: &Geometry) -> Result<String> {
    let json = serde_json::to_string(geometry)?;
    Ok(json)
}

/// Converts a Geometry to a pretty-printed GeoJSON string
pub fn geometry_to_string_pretty(geometry: &Geometry) -> Result<String> {
    let json = serde_json::to_string_pretty(geometry)?;
    Ok(json)
}

// ─── GeoJSONL / newline-delimited GeoJSON ─────────────────────────────────────

/// Write an iterator of features to a writer in GeoJSONL format.
///
/// Each feature is serialised as a compact JSON object followed by a newline
/// (`\n`).  No wrapping `FeatureCollection` is emitted.
///
/// # Errors
///
/// Returns the first serialisation or I/O error encountered.
pub fn write_geojsonl<W, I>(mut writer: W, features: I) -> Result<()>
where
    W: Write,
    I: IntoIterator<Item = Feature>,
{
    for feature in features {
        let line = serde_json::to_string(&feature)?;
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
    }
    Ok(())
}

/// Write features to a file path in GeoJSONL format (one feature per line).
///
/// The file is created (or truncated) at `path`.
///
/// # Errors
///
/// Returns an error when the file cannot be created or when serialisation fails.
pub fn write_geojsonl_to_file<P, I>(path: P, features: I) -> Result<()>
where
    P: AsRef<Path>,
    I: IntoIterator<Item = Feature>,
{
    let file = std::fs::File::create(path.as_ref())?;
    let writer = std::io::BufWriter::new(file);
    write_geojsonl(writer, features)
}

/// GeoJSON formatting utilities
pub mod format {
    use super::*;

    /// Formats coordinates with specified precision
    pub fn format_coordinate(coord: f64, precision: usize) -> String {
        format!("{:.prec$}", coord, prec = precision)
    }

    /// Formats a position with specified precision
    pub fn format_position(pos: &[f64], precision: usize) -> String {
        pos.iter()
            .map(|&c| format_coordinate(c, precision))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Minifies GeoJSON (removes whitespace)
    pub fn minify(geojson: &str) -> Result<String> {
        let value: serde_json::Value = serde_json::from_str(geojson)?;
        let minified = serde_json::to_string(&value)?;
        Ok(minified)
    }

    /// Prettifies GeoJSON (adds indentation)
    pub fn prettify(geojson: &str) -> Result<String> {
        let value: serde_json::Value = serde_json::from_str(geojson)?;
        let prettified = serde_json::to_string_pretty(&value)?;
        Ok(prettified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Point;

    #[test]
    fn test_write_point() {
        let point = Point::new_2d(100.0, 0.0).expect("valid point");
        let geometry = Geometry::Point(point);

        let mut buffer = Vec::new();
        let mut writer = GeoJsonWriter::new(&mut buffer);

        assert!(writer.write_geometry(&geometry).is_ok());
        assert!(writer.flush().is_ok());

        let json = String::from_utf8(buffer).ok();
        assert!(json.is_some());
        let json_str = json.expect("valid UTF-8");
        assert!(json_str.contains("Point"));
        assert!(json_str.contains("100"));
    }

    /// Non-ASCII attribute values (CJK, emoji, accented Latin, RTL) must survive
    /// a write → read round-trip byte-for-byte through the crate's own writer and
    /// reader, and the serialized form must be valid UTF-8 carrying the raw
    /// characters (serde_json does not \u-escape non-ASCII).
    #[test]
    fn test_non_ascii_property_round_trip() {
        use crate::reader::GeoJsonReader;
        use serde_json::Value;

        let mut props = serde_json::Map::new();
        props.insert("名前".to_string(), Value::String("東京都".to_string()));
        props.insert("emoji".to_string(), Value::String("🗾🌏".to_string()));
        props.insert(
            "accent".to_string(),
            Value::String("Zürich café Å".to_string()),
        );
        props.insert("rtl".to_string(), Value::String("مرحبا".to_string()));

        let point = Point::new_2d(139.69, 35.68).expect("valid point");
        let feature = Feature::new(Some(Geometry::Point(point)), Some(props.clone()));
        let fc = FeatureCollection::new(vec![feature]);

        let json = to_string(&fc).expect("serialize");
        // The raw multibyte characters must appear (not mangled or lost).
        assert!(
            json.contains("東京都"),
            "CJK value must be present verbatim"
        );
        assert!(json.contains("🗾🌏"), "emoji must be present verbatim");

        let mut reader = GeoJsonReader::new(std::io::Cursor::new(json.into_bytes()));
        let back = reader.read_feature_collection().expect("read back");
        assert_eq!(back.features.len(), 1);
        let got = back.features[0]
            .properties
            .as_ref()
            .expect("properties present");
        assert_eq!(got, &props, "non-ASCII properties must round-trip exactly");
    }

    /// The same non-ASCII round-trip through the streaming `GeoJsonWriter` buffer
    /// path (not just the `to_string` helper).
    #[test]
    fn test_non_ascii_property_round_trip_writer_buffer() {
        use crate::reader::GeoJsonReader;
        use serde_json::Value;

        let mut props = serde_json::Map::new();
        props.insert(
            "straße".to_string(),
            Value::String("Grüße 日本語".to_string()),
        );
        let point = Point::new_2d(13.4, 52.5).expect("valid point");
        let feature = Feature::new(Some(Geometry::Point(point)), Some(props.clone()));
        let fc = FeatureCollection::new(vec![feature]);

        let mut buffer = Vec::new();
        {
            let mut writer = GeoJsonWriter::new(&mut buffer);
            writer
                .write_feature_collection(&fc)
                .expect("write collection");
            writer.flush().expect("flush");
        }

        let mut reader = GeoJsonReader::new(std::io::Cursor::new(buffer));
        let back = reader.read_feature_collection().expect("read back");
        let got = back.features[0]
            .properties
            .as_ref()
            .expect("properties present");
        assert_eq!(
            got.get("straße").and_then(Value::as_str),
            Some("Grüße 日本語")
        );
        assert_eq!(got, &props);
    }

    /// Non-ASCII values must also survive the GeoJSONL (newline-delimited)
    /// streaming write/read path.
    #[test]
    fn test_non_ascii_geojsonl_round_trip() {
        use crate::reader::read_geojsonl;
        use serde_json::Value;

        let mut props = serde_json::Map::new();
        props.insert(
            "ville".to_string(),
            Value::String("Montréal 🇨🇦".to_string()),
        );
        let point = Point::new_2d(-73.57, 45.5).expect("valid point");
        let feature = Feature::new(Some(Geometry::Point(point)), Some(props.clone()));

        let mut buffer = Vec::new();
        write_geojsonl(&mut buffer, std::iter::once(feature)).expect("write geojsonl");

        let features = read_geojsonl(std::io::Cursor::new(buffer)).expect("read geojsonl");
        assert_eq!(features.len(), 1);
        let got = features[0].properties.as_ref().expect("properties present");
        assert_eq!(
            got.get("ville").and_then(Value::as_str),
            Some("Montréal 🇨🇦")
        );
    }

    #[test]
    fn test_write_feature() {
        let point = Point::new_2d(100.0, 0.0).expect("valid point");
        let geometry = Geometry::Point(point);
        let feature = Feature::new(Some(geometry), None);

        let mut buffer = Vec::new();
        let mut writer = GeoJsonWriter::new(&mut buffer);

        assert!(writer.write_feature(&feature).is_ok());
        assert!(writer.flush().is_ok());

        let json = String::from_utf8(buffer).ok();
        assert!(json.is_some());
        let json_str = json.expect("valid UTF-8");
        assert!(json_str.contains("Feature"));
    }

    #[test]
    fn test_write_feature_collection() {
        let point1 = Point::new_2d(100.0, 0.0).expect("valid point");
        let point2 = Point::new_2d(101.0, 1.0).expect("valid point");

        let feature1 = Feature::new(Some(Geometry::Point(point1)), None);
        let feature2 = Feature::new(Some(Geometry::Point(point2)), None);

        let fc = FeatureCollection::new(vec![feature1, feature2]);

        let mut buffer = Vec::new();
        let mut writer = GeoJsonWriter::new(&mut buffer);

        assert!(writer.write_feature_collection(&fc).is_ok());
        assert!(writer.flush().is_ok());

        let json = String::from_utf8(buffer).ok();
        assert!(json.is_some());
        let json_str = json.expect("valid UTF-8");
        assert!(json_str.contains("FeatureCollection"));
        assert!(json_str.contains("features"));
    }

    #[test]
    fn test_write_pretty() {
        let point = Point::new_2d(100.0, 0.0).expect("valid point");
        let geometry = Geometry::Point(point);

        let mut buffer = Vec::new();
        let mut writer = GeoJsonWriter::pretty(&mut buffer);

        assert!(writer.write_geometry(&geometry).is_ok());
        assert!(writer.flush().is_ok());

        let json = String::from_utf8(buffer).ok();
        assert!(json.is_some());
        let json_str = json.expect("valid UTF-8");
        // Pretty output should contain newlines
        assert!(json_str.contains('\n'));
    }

    #[test]
    fn test_write_compact() {
        let point = Point::new_2d(100.0, 0.0).expect("valid point");
        let geometry = Geometry::Point(point);

        let mut buffer = Vec::new();
        let mut writer = GeoJsonWriter::compact(&mut buffer);

        assert!(writer.write_geometry(&geometry).is_ok());
        assert!(writer.flush().is_ok());

        let json = String::from_utf8(buffer).ok();
        assert!(json.is_some());
        let json_str = json.expect("valid UTF-8");
        // Compact output should not contain newlines (except in strings)
        assert!(!json_str.trim().contains('\n'));
    }

    #[test]
    fn test_streaming_features() {
        let features = [
            Feature::new(
                Some(Geometry::Point(
                    Point::new_2d(0.0, 0.0).expect("valid point"),
                )),
                None,
            ),
            Feature::new(
                Some(Geometry::Point(
                    Point::new_2d(1.0, 1.0).expect("valid point"),
                )),
                None,
            ),
        ];

        let mut buffer = Vec::new();
        let mut writer = GeoJsonWriter::new(&mut buffer);

        assert!(writer.start_feature_collection().is_ok());
        for (i, feature) in features.iter().enumerate() {
            assert!(writer.write_feature_streaming(feature, i == 0).is_ok());
        }
        assert!(writer.finish_feature_collection().is_ok());

        let json = String::from_utf8(buffer).ok();
        assert!(json.is_some());
    }

    #[test]
    fn test_to_string() {
        let fc = FeatureCollection::empty();
        let json = to_string(&fc).ok();
        assert!(json.is_some());
    }

    #[test]
    fn test_to_string_pretty() {
        let fc = FeatureCollection::empty();
        let json = to_string_pretty(&fc).ok();
        assert!(json.is_some());
        let json_str = json.expect("valid json");
        assert!(json_str.contains('\n'));
    }

    #[test]
    fn test_format_minify() {
        let json = r#"{
            "type": "Point",
            "coordinates": [100.0, 0.0]
        }"#;

        let minified = format::minify(json).ok();
        assert!(minified.is_some());
        let min = minified.expect("valid minified");
        assert!(!min.contains('\n'));
    }

    #[test]
    fn test_format_prettify() {
        let json = r#"{"type":"Point","coordinates":[100.0,0.0]}"#;

        let prettified = format::prettify(json).ok();
        assert!(prettified.is_some());
        let pretty = prettified.expect("valid prettified");
        assert!(pretty.contains('\n'));
    }

    #[test]
    fn test_write_with_bbox() {
        let mut point = Point::new_2d(100.0, 0.0).expect("valid point");
        point.bbox = Some(vec![100.0, 0.0, 100.0, 0.0]);
        let geometry = Geometry::Point(point);

        let mut buffer = Vec::new();
        let config = WriterConfig::default().with_bbox(false);
        let mut writer = GeoJsonWriter::with_config(&mut buffer, config);

        assert!(writer.write_geometry(&geometry).is_ok());
        assert!(writer.flush().is_ok());

        let json = String::from_utf8(buffer).ok();
        assert!(json.is_some());
        let json_str = json.expect("valid UTF-8");
        assert!(json_str.contains("bbox"));
    }
}
