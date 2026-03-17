//! GeoJSON types: geometry, feature, feature collection, CRS.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

// ─── Geometry ───────────────────────────────────────────────────────────────

/// All GeoJSON geometry variants (RFC 7946 + optional Z coordinate).
#[derive(Debug, Clone, PartialEq)]
pub enum GeoJsonGeometry {
    /// 2-D point `[lon, lat]`
    Point([f64; 2]),
    /// 3-D point `[lon, lat, z]`
    PointZ([f64; 3]),
    /// 2-D line string
    LineString(Vec<[f64; 2]>),
    /// 3-D line string
    LineStringZ(Vec<[f64; 3]>),
    /// 2-D polygon (exterior ring + holes)
    Polygon(Vec<Vec<[f64; 2]>>),
    /// 3-D polygon
    PolygonZ(Vec<Vec<[f64; 3]>>),
    /// 2-D multi-point
    MultiPoint(Vec<[f64; 2]>),
    /// 2-D multi-line-string
    MultiLineString(Vec<Vec<[f64; 2]>>),
    /// 2-D multi-polygon
    MultiPolygon(Vec<Vec<Vec<[f64; 2]>>>),
    /// Heterogeneous geometry collection
    GeometryCollection(Vec<GeoJsonGeometry>),
    /// Null / absent geometry
    Null,
}

impl GeoJsonGeometry {
    /// Returns the RFC 7946 type string.
    #[must_use]
    pub fn geometry_type(&self) -> &'static str {
        match self {
            Self::Point(_) | Self::PointZ(_) => "Point",
            Self::LineString(_) | Self::LineStringZ(_) => "LineString",
            Self::Polygon(_) | Self::PolygonZ(_) => "Polygon",
            Self::MultiPoint(_) => "MultiPoint",
            Self::MultiLineString(_) => "MultiLineString",
            Self::MultiPolygon(_) => "MultiPolygon",
            Self::GeometryCollection(_) => "GeometryCollection",
            Self::Null => "null",
        }
    }

    /// Total number of coordinate positions in the geometry.
    #[must_use]
    pub fn point_count(&self) -> usize {
        match self {
            Self::Point(_) | Self::PointZ(_) => 1,
            Self::Null => 0,
            Self::LineString(pts) => pts.len(),
            Self::LineStringZ(pts) => pts.len(),
            Self::Polygon(rings) => rings.iter().map(|r| r.len()).sum(),
            Self::PolygonZ(rings) => rings.iter().map(|r| r.len()).sum(),
            Self::MultiPoint(pts) => pts.len(),
            Self::MultiLineString(lines) => lines.iter().map(|l| l.len()).sum(),
            Self::MultiPolygon(polys) => polys.iter().flat_map(|p| p.iter()).map(|r| r.len()).sum(),
            Self::GeometryCollection(geoms) => geoms.iter().map(|g| g.point_count()).sum(),
        }
    }

    /// Compute the 2-D bounding box `[minx, miny, maxx, maxy]`.
    /// Returns `None` for null or empty geometries.
    #[must_use]
    pub fn bbox(&self) -> Option<[f64; 4]> {
        match self {
            Self::Null => None,
            Self::Point([x, y]) => Some([*x, *y, *x, *y]),
            Self::PointZ([x, y, _]) => Some([*x, *y, *x, *y]),
            Self::LineString(pts) => bbox_2d(pts),
            Self::LineStringZ(pts) => bbox_3d_as_2d(pts),
            Self::Polygon(rings) => {
                if rings.is_empty() {
                    return None;
                }
                bbox_2d(&rings[0])
            }
            Self::PolygonZ(rings) => {
                if rings.is_empty() {
                    return None;
                }
                bbox_3d_as_2d(&rings[0])
            }
            Self::MultiPoint(pts) => bbox_2d(pts),
            Self::MultiLineString(lines) => {
                let all: Vec<[f64; 2]> = lines.iter().flatten().copied().collect();
                bbox_2d(&all)
            }
            Self::MultiPolygon(polys) => {
                let all: Vec<[f64; 2]> = polys
                    .iter()
                    .flat_map(|p| p.first().map(|r| r.as_slice()).unwrap_or(&[]))
                    .copied()
                    .collect();
                bbox_2d(&all)
            }
            Self::GeometryCollection(geoms) => {
                let bboxes: Vec<[f64; 4]> = geoms.iter().filter_map(|g| g.bbox()).collect();
                union_bboxes(&bboxes)
            }
        }
    }

    /// Returns `true` when this geometry carries no coordinates.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.point_count() == 0
    }

    /// Drop the Z coordinate, returning a 2-D geometry.
    #[must_use]
    pub fn to_2d(&self) -> Self {
        match self {
            Self::PointZ([x, y, _]) => Self::Point([*x, *y]),
            Self::LineStringZ(pts) => {
                Self::LineString(pts.iter().map(|[x, y, _]| [*x, *y]).collect())
            }
            Self::PolygonZ(rings) => Self::Polygon(
                rings
                    .iter()
                    .map(|r| r.iter().map(|[x, y, _]| [*x, *y]).collect())
                    .collect(),
            ),
            Self::GeometryCollection(geoms) => {
                Self::GeometryCollection(geoms.iter().map(|g| g.to_2d()).collect())
            }
            other => other.clone(),
        }
    }
}

// ─── Feature ────────────────────────────────────────────────────────────────

/// A GeoJSON Feature.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoJsonFeature {
    /// Optional feature identifier (string or number).
    pub id: Option<FeatureId>,
    /// Optional geometry.
    pub geometry: Option<GeoJsonGeometry>,
    /// Optional properties object.
    pub properties: Option<serde_json::Value>,
}

/// Feature identifier variants.
#[derive(Debug, Clone, PartialEq)]
pub enum FeatureId {
    /// String identifier.
    String(String),
    /// Numeric identifier.
    Number(f64),
}

impl GeoJsonFeature {
    /// Retrieve a typed property value by key.
    /// Returns `None` if the key is absent or deserialization fails.
    pub fn get_property<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let props = self.properties.as_ref()?;
        let value = props.get(key)?;
        serde_json::from_value(value.clone()).ok()
    }

    /// Returns `true` if this feature has a non-null geometry.
    #[must_use]
    pub fn has_geometry(&self) -> bool {
        matches!(&self.geometry, Some(g) if !matches!(g, GeoJsonGeometry::Null))
    }

    /// Returns the bounding box of the feature's geometry, if any.
    #[must_use]
    pub fn bbox(&self) -> Option<[f64; 4]> {
        self.geometry.as_ref()?.bbox()
    }
}

// ─── CRS ────────────────────────────────────────────────────────────────────

/// Legacy Coordinate Reference System (pre-RFC 7946).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoJsonCrs {
    /// CRS type: `"name"` or `"link"`.
    #[serde(rename = "type")]
    pub type_: String,
    /// CRS properties object.
    pub properties: serde_json::Value,
}

impl GeoJsonCrs {
    /// Create a named CRS.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            type_: "name".into(),
            properties: serde_json::json!({ "name": name.into() }),
        }
    }

    /// WGS 84 geographic CRS (EPSG:4326 / CRS84).
    #[must_use]
    pub fn epsg4326() -> Self {
        Self::named("urn:ogc:def:crs:OGC:1.3:CRS84")
    }

    /// Web Mercator (EPSG:3857).
    #[must_use]
    pub fn epsg3857() -> Self {
        Self::named("EPSG:3857")
    }

    /// Extract an EPSG code from the name property, if present.
    /// Recognises patterns `EPSG:NNNN` and `urn:ogc:def:crs:EPSG::NNNN`.
    #[must_use]
    pub fn epsg_code(&self) -> Option<i32> {
        let name = self.properties.get("name")?.as_str()?;

        // Direct "EPSG:NNNN" format
        if let Some(code_str) = name.strip_prefix("EPSG:") {
            return code_str.parse().ok();
        }

        // URN format: urn:ogc:def:crs:EPSG::NNNN or urn:ogc:def:crs:OGC:1.3:CRS84
        if name.contains("EPSG") {
            // Find the last colon-separated token that parses as an integer
            if let Some(last) = name.rsplit(':').find(|s| !s.is_empty()) {
                return last.parse().ok();
            }
        }

        None
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Compute 2-D bounding box from a slice of `[x, y]` coordinates.
fn bbox_2d(pts: &[[f64; 2]]) -> Option<[f64; 4]> {
    if pts.is_empty() {
        return None;
    }
    let mut minx = pts[0][0];
    let mut miny = pts[0][1];
    let mut maxx = pts[0][0];
    let mut maxy = pts[0][1];
    for [x, y] in pts.iter().skip(1) {
        if *x < minx {
            minx = *x;
        }
        if *y < miny {
            miny = *y;
        }
        if *x > maxx {
            maxx = *x;
        }
        if *y > maxy {
            maxy = *y;
        }
    }
    Some([minx, miny, maxx, maxy])
}

/// Compute 2-D bounding box from `[x, y, z]` coordinates (ignores Z).
fn bbox_3d_as_2d(pts: &[[f64; 3]]) -> Option<[f64; 4]> {
    if pts.is_empty() {
        return None;
    }
    let pts2: Vec<[f64; 2]> = pts.iter().map(|[x, y, _]| [*x, *y]).collect();
    bbox_2d(&pts2)
}

/// Union of multiple bounding boxes.
pub fn union_bboxes(bboxes: &[[f64; 4]]) -> Option<[f64; 4]> {
    if bboxes.is_empty() {
        return None;
    }
    let mut result = bboxes[0];
    for bb in bboxes.iter().skip(1) {
        if bb[0] < result[0] {
            result[0] = bb[0];
        }
        if bb[1] < result[1] {
            result[1] = bb[1];
        }
        if bb[2] > result[2] {
            result[2] = bb[2];
        }
        if bb[3] > result[3] {
            result[3] = bb[3];
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_geometry_type() {
        let g = GeoJsonGeometry::Point([1.0, 2.0]);
        assert_eq!(g.geometry_type(), "Point");
    }

    #[test]
    fn test_null_geometry_type() {
        let g = GeoJsonGeometry::Null;
        assert_eq!(g.geometry_type(), "null");
        assert!(g.is_empty());
    }

    #[test]
    fn test_point_bbox() {
        let g = GeoJsonGeometry::Point([10.0, 20.0]);
        assert_eq!(g.bbox(), Some([10.0, 20.0, 10.0, 20.0]));
    }

    #[test]
    fn test_linestring_bbox() {
        let g = GeoJsonGeometry::LineString(vec![[0.0, 0.0], [10.0, 5.0]]);
        assert_eq!(g.bbox(), Some([0.0, 0.0, 10.0, 5.0]));
    }

    #[test]
    fn test_to_2d_drops_z() {
        let g = GeoJsonGeometry::PointZ([1.0, 2.0, 3.0]);
        assert_eq!(g.to_2d(), GeoJsonGeometry::Point([1.0, 2.0]));
    }
}
