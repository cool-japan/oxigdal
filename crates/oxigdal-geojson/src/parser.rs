//! Streaming GeoJSON parser — bytes-based API.

use crate::error::GeoJsonError;
use crate::types::{FeatureId, GeoJsonCrs, GeoJsonFeature, GeoJsonGeometry};

// ─── Public parser struct ────────────────────────────────────────────────────

/// Bytes-based GeoJSON parser.
///
/// Parses an entire document from a `&[u8]` slice.  "Streaming" here means
/// that features are yielded lazily via [`StreamingFeatureReader`] after the
/// JSON tree has been loaded — suitable for bounded-memory iteration over
/// large collections that nonetheless fit in the I/O buffer.
#[derive(Debug, Clone)]
pub struct GeoJsonParser {
    /// Reject non-conforming GeoJSON when `true`.
    pub strict_mode: bool,
    /// Maximum JSON nesting depth (default `64`).
    pub max_depth: usize,
}

impl Default for GeoJsonParser {
    fn default() -> Self {
        Self::new()
    }
}

impl GeoJsonParser {
    /// Create a lenient parser with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            strict_mode: false,
            max_depth: 64,
        }
    }

    /// Enable strict-mode validation.
    #[must_use]
    pub fn strict(mut self) -> Self {
        self.strict_mode = true;
        self
    }

    /// Parse a complete GeoJSON document from bytes.
    ///
    /// # Errors
    ///
    /// Returns [`GeoJsonError`] on invalid JSON or unrecognised structure.
    pub fn parse(&self, input: &[u8]) -> Result<GeoJsonDocument, GeoJsonError> {
        let value: serde_json::Value = serde_json::from_slice(input)?;
        self.parse_value(value, 0)
    }

    /// Parse only the header (type, bbox, crs, name) without processing all
    /// features — useful for quick metadata inspection.
    ///
    /// # Errors
    ///
    /// Returns [`GeoJsonError`] on invalid JSON or missing `type` field.
    pub fn parse_header(&self, input: &[u8]) -> Result<FeatureCollectionHeader, GeoJsonError> {
        let value: serde_json::Value = serde_json::from_slice(input)?;

        let type_ = value
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GeoJsonError::MissingField("type".into()))?
            .to_string();

        let bbox = parse_bbox(value.get("bbox"));
        let crs = parse_crs(value.get("crs"));
        let name = value
            .get("name")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned);

        // Count features without parsing them
        let feature_count = value
            .get("features")
            .and_then(|f| f.as_array())
            .map(|arr| arr.len());

        Ok(FeatureCollectionHeader {
            type_,
            feature_count,
            bbox,
            crs,
            name,
        })
    }

    /// Parse a [`GeoJsonGeometry`] from a [`serde_json::Value`].
    ///
    /// # Errors
    ///
    /// Returns [`GeoJsonError`] on unrecognised geometry type or bad coords.
    pub fn parse_geometry(
        &self,
        value: &serde_json::Value,
    ) -> Result<GeoJsonGeometry, GeoJsonError> {
        self.parse_geometry_value(value, 0)
    }

    /// Parse a [`GeoJsonFeature`] from a [`serde_json::Value`].
    ///
    /// # Errors
    ///
    /// Returns [`GeoJsonError`] on invalid feature structure.
    pub fn parse_feature(&self, value: &serde_json::Value) -> Result<GeoJsonFeature, GeoJsonError> {
        let type_ = value
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GeoJsonError::MissingField("type".into()))?;

        if type_ != "Feature" {
            return Err(GeoJsonError::InvalidType {
                expected: "Feature".into(),
                got: type_.into(),
            });
        }

        let id = parse_feature_id(value.get("id"));

        let geometry = match value.get("geometry") {
            None => None,
            Some(serde_json::Value::Null) => Some(GeoJsonGeometry::Null),
            Some(geom_val) => Some(self.parse_geometry_value(geom_val, 0)?),
        };

        let properties = value.get("properties").cloned();

        Ok(GeoJsonFeature {
            id,
            geometry,
            properties,
        })
    }

    // ── Private helpers ───────────────────────────────────────────────────

    fn parse_value(
        &self,
        value: serde_json::Value,
        depth: usize,
    ) -> Result<GeoJsonDocument, GeoJsonError> {
        if depth > self.max_depth {
            return Err(GeoJsonError::MaxDepthExceeded);
        }

        let type_ = value
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GeoJsonError::MissingField("type".into()))?;

        match type_ {
            "FeatureCollection" => {
                let fc = self.parse_feature_collection_value(&value)?;
                Ok(GeoJsonDocument::FeatureCollection(fc))
            }
            "Feature" => {
                let feat = self.parse_feature(&value)?;
                Ok(GeoJsonDocument::Feature(feat))
            }
            _ => {
                // Try as geometry
                let geom = self.parse_geometry_value(&value, depth + 1)?;
                Ok(GeoJsonDocument::Geometry(geom))
            }
        }
    }

    fn parse_feature_collection_value(
        &self,
        value: &serde_json::Value,
    ) -> Result<FeatureCollection, GeoJsonError> {
        let features_arr = value
            .get("features")
            .and_then(|f| f.as_array())
            .ok_or_else(|| GeoJsonError::MissingField("features".into()))?;

        let mut features = Vec::with_capacity(features_arr.len());
        for feat_val in features_arr {
            features.push(self.parse_feature(feat_val)?);
        }

        let bbox = parse_bbox(value.get("bbox"));
        let crs = parse_crs(value.get("crs"));
        let name = value
            .get("name")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned);

        Ok(FeatureCollection {
            features,
            bbox,
            crs,
            name,
        })
    }

    fn parse_geometry_value(
        &self,
        value: &serde_json::Value,
        depth: usize,
    ) -> Result<GeoJsonGeometry, GeoJsonError> {
        if depth > self.max_depth {
            return Err(GeoJsonError::MaxDepthExceeded);
        }

        if value.is_null() {
            return Ok(GeoJsonGeometry::Null);
        }

        let type_ = value
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GeoJsonError::MissingField("type".into()))?;

        match type_ {
            "Point" => {
                let coords = value
                    .get("coordinates")
                    .ok_or_else(|| GeoJsonError::MissingField("coordinates".into()))?;
                parse_point_coords(coords)
            }
            "LineString" => {
                let coords = value
                    .get("coordinates")
                    .ok_or_else(|| GeoJsonError::MissingField("coordinates".into()))?;
                parse_linestring_coords(coords)
            }
            "Polygon" => {
                let coords = value
                    .get("coordinates")
                    .ok_or_else(|| GeoJsonError::MissingField("coordinates".into()))?;
                parse_polygon_coords(coords)
            }
            "MultiPoint" => {
                let coords = value
                    .get("coordinates")
                    .ok_or_else(|| GeoJsonError::MissingField("coordinates".into()))?;
                let arr = coords
                    .as_array()
                    .ok_or_else(|| GeoJsonError::InvalidCoordinates("expected array".into()))?;
                if arr.is_empty() {
                    return Err(GeoJsonError::EmptyCoordinates);
                }
                let pts: Result<Vec<[f64; 2]>, GeoJsonError> =
                    arr.iter().map(parse_coord_2d).collect();
                Ok(GeoJsonGeometry::MultiPoint(pts?))
            }
            "MultiLineString" => {
                let coords = value
                    .get("coordinates")
                    .ok_or_else(|| GeoJsonError::MissingField("coordinates".into()))?;
                let arr = coords
                    .as_array()
                    .ok_or_else(|| GeoJsonError::InvalidCoordinates("expected array".into()))?;
                let lines: Result<Vec<Vec<[f64; 2]>>, GeoJsonError> = arr
                    .iter()
                    .map(|line| {
                        let pts_arr = line.as_array().ok_or_else(|| {
                            GeoJsonError::InvalidCoordinates("expected array".into())
                        })?;
                        pts_arr.iter().map(parse_coord_2d).collect()
                    })
                    .collect();
                Ok(GeoJsonGeometry::MultiLineString(lines?))
            }
            "MultiPolygon" => {
                let coords = value
                    .get("coordinates")
                    .ok_or_else(|| GeoJsonError::MissingField("coordinates".into()))?;
                let arr = coords
                    .as_array()
                    .ok_or_else(|| GeoJsonError::InvalidCoordinates("expected array".into()))?;
                let polys: Result<Vec<Vec<Vec<[f64; 2]>>>, GeoJsonError> = arr
                    .iter()
                    .map(|poly| {
                        let rings = poly.as_array().ok_or_else(|| {
                            GeoJsonError::InvalidCoordinates("expected array".into())
                        })?;
                        rings
                            .iter()
                            .map(|ring| {
                                let ring_arr = ring.as_array().ok_or_else(|| {
                                    GeoJsonError::InvalidCoordinates("expected array".into())
                                })?;
                                ring_arr.iter().map(parse_coord_2d).collect()
                            })
                            .collect()
                    })
                    .collect();
                Ok(GeoJsonGeometry::MultiPolygon(polys?))
            }
            "GeometryCollection" => {
                let geometries = value
                    .get("geometries")
                    .and_then(|g| g.as_array())
                    .ok_or_else(|| GeoJsonError::MissingField("geometries".into()))?;
                let geoms: Result<Vec<GeoJsonGeometry>, GeoJsonError> = geometries
                    .iter()
                    .map(|g| self.parse_geometry_value(g, depth + 1))
                    .collect();
                Ok(GeoJsonGeometry::GeometryCollection(geoms?))
            }
            other => Err(GeoJsonError::InvalidType {
                expected: "GeoJSON geometry type".into(),
                got: other.into(),
            }),
        }
    }
}

// ─── GeoJsonDocument ────────────────────────────────────────────────────────

/// Top-level GeoJSON document discriminant.
#[derive(Debug, Clone, PartialEq)]
pub enum GeoJsonDocument {
    /// A GeoJSON `FeatureCollection`.
    FeatureCollection(FeatureCollection),
    /// A GeoJSON `Feature`.
    Feature(GeoJsonFeature),
    /// A bare GeoJSON geometry.
    Geometry(GeoJsonGeometry),
}

impl GeoJsonDocument {
    /// Returns the document type string.
    #[must_use]
    pub fn document_type(&self) -> &'static str {
        match self {
            Self::FeatureCollection(_) => "FeatureCollection",
            Self::Feature(_) => "Feature",
            Self::Geometry(_) => "Geometry",
        }
    }

    /// Borrow the inner [`FeatureCollection`], if applicable.
    #[must_use]
    pub fn as_feature_collection(&self) -> Option<&FeatureCollection> {
        match self {
            Self::FeatureCollection(fc) => Some(fc),
            _ => None,
        }
    }
}

// ─── FeatureCollection ──────────────────────────────────────────────────────

/// A parsed GeoJSON `FeatureCollection`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FeatureCollection {
    /// The features in this collection.
    pub features: Vec<GeoJsonFeature>,
    /// Optional bounding box from the JSON.
    pub bbox: Option<[f64; 4]>,
    /// Optional CRS (legacy, pre-RFC 7946).
    pub crs: Option<GeoJsonCrs>,
    /// Optional `name` property (common convention).
    pub name: Option<String>,
}

impl FeatureCollection {
    /// Number of features.
    #[must_use]
    pub fn len(&self) -> usize {
        self.features.len()
    }

    /// Returns `true` when the collection has no features.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// Compute the union bounding box of all feature geometries.
    #[must_use]
    pub fn compute_bbox(&self) -> Option<[f64; 4]> {
        let bboxes: Vec<[f64; 4]> = self.features.iter().filter_map(|f| f.bbox()).collect();
        crate::types::union_bboxes(&bboxes)
    }

    /// Return the unique geometry type strings present in this collection.
    #[must_use]
    pub fn geometry_types(&self) -> Vec<&'static str> {
        let mut seen = Vec::<&'static str>::new();
        for feat in &self.features {
            if let Some(geom) = &feat.geometry {
                let t = geom.geometry_type();
                if !seen.contains(&t) {
                    seen.push(t);
                }
            }
        }
        seen
    }
}

// ─── FeatureCollectionHeader ─────────────────────────────────────────────────

/// Lightweight metadata parsed without loading all features.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureCollectionHeader {
    /// Value of the `"type"` field.
    pub type_: String,
    /// Number of items in the `"features"` array (counted without parsing).
    pub feature_count: Option<usize>,
    /// Optional bounding box.
    pub bbox: Option<[f64; 4]>,
    /// Optional CRS (legacy).
    pub crs: Option<GeoJsonCrs>,
    /// Optional `"name"` property.
    pub name: Option<String>,
}

// ─── StreamingFeatureReader ──────────────────────────────────────────────────

/// Lazy iterator over features in a [`FeatureCollection`].
///
/// The collection is already in memory; this is a zero-copy iterator wrapper
/// that yields one feature at a time, enabling `for feat in reader { … }`
/// patterns without materialising a second `Vec`.
pub struct StreamingFeatureReader<'a> {
    features: std::slice::Iter<'a, GeoJsonFeature>,
}

impl<'a> StreamingFeatureReader<'a> {
    /// Wrap a reference to a collection's feature slice.
    pub fn from_collection(collection: &'a FeatureCollection) -> Self {
        Self {
            features: collection.features.iter(),
        }
    }
}

impl<'a> Iterator for StreamingFeatureReader<'a> {
    type Item = &'a GeoJsonFeature;

    fn next(&mut self) -> Option<Self::Item> {
        self.features.next()
    }
}

// ─── Private coordinate helpers ─────────────────────────────────────────────

fn parse_coord_2d(v: &serde_json::Value) -> Result<[f64; 2], GeoJsonError> {
    let arr = v
        .as_array()
        .ok_or_else(|| GeoJsonError::InvalidCoordinates("coordinate must be array".into()))?;
    if arr.len() < 2 {
        return Err(GeoJsonError::InvalidCoordinates(
            "coordinate needs at least 2 elements".into(),
        ));
    }
    let x = arr[0]
        .as_f64()
        .ok_or_else(|| GeoJsonError::InvalidCoordinates("x is not a number".into()))?;
    let y = arr[1]
        .as_f64()
        .ok_or_else(|| GeoJsonError::InvalidCoordinates("y is not a number".into()))?;
    Ok([x, y])
}

fn parse_coord_3d(v: &serde_json::Value) -> Result<[f64; 3], GeoJsonError> {
    let arr = v
        .as_array()
        .ok_or_else(|| GeoJsonError::InvalidCoordinates("coordinate must be array".into()))?;
    if arr.len() < 3 {
        return Err(GeoJsonError::InvalidCoordinates(
            "Z coordinate needs at least 3 elements".into(),
        ));
    }
    let x = arr[0]
        .as_f64()
        .ok_or_else(|| GeoJsonError::InvalidCoordinates("x is not a number".into()))?;
    let y = arr[1]
        .as_f64()
        .ok_or_else(|| GeoJsonError::InvalidCoordinates("y is not a number".into()))?;
    let z = arr[2]
        .as_f64()
        .ok_or_else(|| GeoJsonError::InvalidCoordinates("z is not a number".into()))?;
    Ok([x, y, z])
}

fn parse_point_coords(value: &serde_json::Value) -> Result<GeoJsonGeometry, GeoJsonError> {
    let arr = value
        .as_array()
        .ok_or_else(|| GeoJsonError::InvalidCoordinates("expected array".into()))?;
    if arr.is_empty() {
        return Err(GeoJsonError::EmptyCoordinates);
    }
    if arr.len() >= 3 {
        let c = parse_coord_3d(value)?;
        Ok(GeoJsonGeometry::PointZ(c))
    } else {
        let c = parse_coord_2d(value)?;
        Ok(GeoJsonGeometry::Point(c))
    }
}

fn parse_linestring_coords(value: &serde_json::Value) -> Result<GeoJsonGeometry, GeoJsonError> {
    let arr = value
        .as_array()
        .ok_or_else(|| GeoJsonError::InvalidCoordinates("expected array".into()))?;
    if arr.is_empty() {
        return Err(GeoJsonError::EmptyCoordinates);
    }
    // Detect Z by peeking at first coordinate
    let has_z = arr
        .first()
        .and_then(|c| c.as_array())
        .map(|c| c.len() >= 3)
        .unwrap_or(false);

    if has_z {
        let pts: Result<Vec<[f64; 3]>, GeoJsonError> = arr.iter().map(parse_coord_3d).collect();
        Ok(GeoJsonGeometry::LineStringZ(pts?))
    } else {
        let pts: Result<Vec<[f64; 2]>, GeoJsonError> = arr.iter().map(parse_coord_2d).collect();
        Ok(GeoJsonGeometry::LineString(pts?))
    }
}

fn parse_polygon_coords(value: &serde_json::Value) -> Result<GeoJsonGeometry, GeoJsonError> {
    let rings_arr = value
        .as_array()
        .ok_or_else(|| GeoJsonError::InvalidCoordinates("expected array".into()))?;
    if rings_arr.is_empty() {
        return Err(GeoJsonError::EmptyCoordinates);
    }
    // Detect Z by peeking at first position of first ring
    let has_z = rings_arr
        .first()
        .and_then(|r| r.as_array())
        .and_then(|r| r.first())
        .and_then(|c| c.as_array())
        .map(|c| c.len() >= 3)
        .unwrap_or(false);

    if has_z {
        let rings: Result<Vec<Vec<[f64; 3]>>, GeoJsonError> = rings_arr
            .iter()
            .map(|ring| {
                let ring_arr = ring
                    .as_array()
                    .ok_or_else(|| GeoJsonError::InvalidCoordinates("expected array".into()))?;
                ring_arr.iter().map(parse_coord_3d).collect()
            })
            .collect();
        Ok(GeoJsonGeometry::PolygonZ(rings?))
    } else {
        let rings: Result<Vec<Vec<[f64; 2]>>, GeoJsonError> = rings_arr
            .iter()
            .map(|ring| {
                let ring_arr = ring
                    .as_array()
                    .ok_or_else(|| GeoJsonError::InvalidCoordinates("expected array".into()))?;
                ring_arr.iter().map(parse_coord_2d).collect()
            })
            .collect();
        Ok(GeoJsonGeometry::Polygon(rings?))
    }
}

fn parse_bbox(value: Option<&serde_json::Value>) -> Option<[f64; 4]> {
    let arr = value?.as_array()?;
    if arr.len() < 4 {
        return None;
    }
    let minx = arr[0].as_f64()?;
    let miny = arr[1].as_f64()?;
    let maxx = arr[2].as_f64()?;
    let maxy = arr[3].as_f64()?;
    Some([minx, miny, maxx, maxy])
}

fn parse_crs(value: Option<&serde_json::Value>) -> Option<GeoJsonCrs> {
    let v = value?;
    serde_json::from_value(v.clone()).ok()
}

fn parse_feature_id(value: Option<&serde_json::Value>) -> Option<FeatureId> {
    let v = value?;
    if v.is_null() {
        return None;
    }
    if let Some(s) = v.as_str() {
        return Some(FeatureId::String(s.to_owned()));
    }
    if let Some(n) = v.as_f64() {
        return Some(FeatureId::Number(n));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> GeoJsonParser {
        GeoJsonParser::new()
    }

    #[test]
    fn test_parse_point_doc() {
        let json = br#"{"type":"Point","coordinates":[1.0,2.0]}"#;
        let doc = parser().parse(json).expect("valid");
        assert_eq!(doc.document_type(), "Geometry");
    }

    #[test]
    fn test_parse_feature() {
        let json = br#"{"type":"Feature","geometry":{"type":"Point","coordinates":[0.0,0.0]},"properties":null}"#;
        let doc = parser().parse(json).expect("valid");
        assert_eq!(doc.document_type(), "Feature");
    }

    #[test]
    fn test_parse_feature_collection() {
        let json = br#"{"type":"FeatureCollection","features":[]}"#;
        let doc = parser().parse(json).expect("valid");
        assert_eq!(doc.document_type(), "FeatureCollection");
    }

    #[test]
    fn test_parse_header() {
        let json = br#"{"type":"FeatureCollection","bbox":[0,0,10,10],"features":[{},{}]}"#;
        let hdr = parser().parse_header(json).expect("valid");
        assert_eq!(hdr.type_, "FeatureCollection");
        assert_eq!(hdr.feature_count, Some(2));
        assert_eq!(hdr.bbox, Some([0.0, 0.0, 10.0, 10.0]));
    }
}
