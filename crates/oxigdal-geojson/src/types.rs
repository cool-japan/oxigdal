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
    /// 3-D multi-point
    MultiPointZ(Vec<[f64; 3]>),
    /// 2-D multi-line-string
    MultiLineString(Vec<Vec<[f64; 2]>>),
    /// 3-D multi-line-string
    MultiLineStringZ(Vec<Vec<[f64; 3]>>),
    /// 2-D multi-polygon
    MultiPolygon(Vec<Vec<Vec<[f64; 2]>>>),
    /// 3-D multi-polygon
    MultiPolygonZ(Vec<Vec<Vec<[f64; 3]>>>),
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
            Self::MultiPoint(_) | Self::MultiPointZ(_) => "MultiPoint",
            Self::MultiLineString(_) | Self::MultiLineStringZ(_) => "MultiLineString",
            Self::MultiPolygon(_) | Self::MultiPolygonZ(_) => "MultiPolygon",
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
            Self::MultiPointZ(pts) => pts.len(),
            Self::MultiLineString(lines) => lines.iter().map(|l| l.len()).sum(),
            Self::MultiLineStringZ(lines) => lines.iter().map(|l| l.len()).sum(),
            Self::MultiPolygon(polys) => polys.iter().flat_map(|p| p.iter()).map(|r| r.len()).sum(),
            Self::MultiPolygonZ(polys) => {
                polys.iter().flat_map(|p| p.iter()).map(|r| r.len()).sum()
            }
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
            Self::MultiPointZ(pts) => bbox_3d_as_2d(pts),
            Self::MultiLineString(lines) => {
                let all: Vec<[f64; 2]> = lines.iter().flatten().copied().collect();
                bbox_2d(&all)
            }
            Self::MultiLineStringZ(lines) => {
                let all: Vec<[f64; 3]> = lines.iter().flatten().copied().collect();
                bbox_3d_as_2d(&all)
            }
            Self::MultiPolygon(polys) => {
                let all: Vec<[f64; 2]> = polys
                    .iter()
                    .flat_map(|p| p.first().map(|r| r.as_slice()).unwrap_or(&[]))
                    .copied()
                    .collect();
                bbox_2d(&all)
            }
            Self::MultiPolygonZ(polys) => {
                let all: Vec<[f64; 3]> = polys
                    .iter()
                    .flat_map(|p| p.first().map(|r| r.as_slice()).unwrap_or(&[]))
                    .copied()
                    .collect();
                bbox_3d_as_2d(&all)
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

    /// Compute the 3-D (6-element) bounding box
    /// `[minx, miny, minz, maxx, maxy, maxz]` (RFC 7946 §5).
    ///
    /// Returns `Some` only when the geometry actually carries Z coordinates.
    /// For 2-D geometries returns `None` — use [`bbox()`](Self::bbox) instead.
    #[must_use]
    pub fn bbox_3d(&self) -> Option<[f64; 6]> {
        match self {
            Self::PointZ([x, y, z]) => Some([*x, *y, *z, *x, *y, *z]),
            Self::LineStringZ(pts) => bbox_3d_full(pts),
            Self::PolygonZ(rings) => {
                if rings.is_empty() {
                    return None;
                }
                bbox_3d_full(&rings[0])
            }
            Self::MultiPointZ(pts) => bbox_3d_full(pts),
            Self::MultiLineStringZ(lines) => {
                let all: Vec<[f64; 3]> = lines.iter().flatten().copied().collect();
                bbox_3d_full(&all)
            }
            Self::MultiPolygonZ(polys) => {
                let all: Vec<[f64; 3]> = polys
                    .iter()
                    .flat_map(|p| p.first().map(|r| r.as_slice()).unwrap_or(&[]))
                    .copied()
                    .collect();
                bbox_3d_full(&all)
            }
            Self::GeometryCollection(geoms) => {
                let bboxes: Vec<[f64; 6]> = geoms.iter().filter_map(|g| g.bbox_3d()).collect();
                union_bboxes_3d(&bboxes)
            }
            _ => None, // 2-D geometries have no Z
        }
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
            Self::MultiPointZ(pts) => {
                Self::MultiPoint(pts.iter().map(|[x, y, _]| [*x, *y]).collect())
            }
            Self::MultiLineStringZ(lines) => Self::MultiLineString(
                lines
                    .iter()
                    .map(|l| l.iter().map(|[x, y, _]| [*x, *y]).collect())
                    .collect(),
            ),
            Self::MultiPolygonZ(polys) => Self::MultiPolygon(
                polys
                    .iter()
                    .map(|p| {
                        p.iter()
                            .map(|r| r.iter().map(|[x, y, _]| [*x, *y]).collect())
                            .collect()
                    })
                    .collect(),
            ),
            Self::GeometryCollection(geoms) => {
                Self::GeometryCollection(geoms.iter().map(|g| g.to_2d()).collect())
            }
            other => other.clone(),
        }
    }

    // ── Area ─────────────────────────────────────────────────────────────────

    /// Signed area of a 2-D ring via the shoelace formula (half the cross-product sum).
    fn ring_signed_area(ring: &[[f64; 2]]) -> f64 {
        let n = ring.len();
        if n < 3 {
            return 0.0;
        }
        let mut sum = 0.0;
        for i in 0..n {
            let [x0, y0] = ring[i];
            let [x1, y1] = ring[(i + 1) % n];
            sum += x0 * y1 - x1 * y0;
        }
        sum * 0.5
    }

    /// Area of a polygon defined by rings (first ring = exterior, rest = holes).
    fn polygon_area(rings: &[Vec<[f64; 2]>]) -> f64 {
        if rings.is_empty() {
            return 0.0;
        }
        let exterior = Self::ring_signed_area(&rings[0]).abs();
        let hole_area: f64 = rings[1..]
            .iter()
            .map(|r| Self::ring_signed_area(r).abs())
            .sum();
        exterior - hole_area
    }

    /// Planar area in square coordinate units (CRS-dependent).
    ///
    /// Returns `0.0` for non-areal geometries (points, lines, null).
    /// For Z variants the Z coordinate is ignored; area is purely planimetric.
    #[must_use]
    pub fn area(&self) -> f64 {
        match self {
            Self::Polygon(rings) => Self::polygon_area(rings),
            Self::PolygonZ(rings) => {
                let rings_2d: Vec<Vec<[f64; 2]>> = rings
                    .iter()
                    .map(|r| r.iter().map(|[x, y, _]| [*x, *y]).collect())
                    .collect();
                Self::polygon_area(&rings_2d)
            }
            Self::MultiPolygon(polys) => polys.iter().map(|p| Self::polygon_area(p)).sum(),
            Self::MultiPolygonZ(polys) => polys
                .iter()
                .map(|p| {
                    let rings_2d: Vec<Vec<[f64; 2]>> = p
                        .iter()
                        .map(|r| r.iter().map(|[x, y, _]| [*x, *y]).collect())
                        .collect();
                    Self::polygon_area(&rings_2d)
                })
                .sum(),
            Self::GeometryCollection(geoms) => geoms.iter().map(|g| g.area()).sum(),
            _ => 0.0,
        }
    }

    // ── Length ───────────────────────────────────────────────────────────────

    /// Euclidean length of a 2-D polyline.
    fn line_length_2d(coords: &[[f64; 2]]) -> f64 {
        if coords.len() < 2 {
            return 0.0;
        }
        coords
            .windows(2)
            .map(|w| {
                let dx = w[1][0] - w[0][0];
                let dy = w[1][1] - w[0][1];
                dx.hypot(dy)
            })
            .sum()
    }

    /// Euclidean length of a 3-D polyline (full 3-D distance).
    fn line_length_3d(coords: &[[f64; 3]]) -> f64 {
        if coords.len() < 2 {
            return 0.0;
        }
        coords
            .windows(2)
            .map(|w| {
                let dx = w[1][0] - w[0][0];
                let dy = w[1][1] - w[0][1];
                let dz = w[1][2] - w[0][2];
                (dx * dx + dy * dy + dz * dz).sqrt()
            })
            .sum()
    }

    /// Planar length in coordinate units.
    ///
    /// Returns the perimeter for polygons (sum of all ring lengths),
    /// `0.0` for points and null geometry.
    #[must_use]
    pub fn length(&self) -> f64 {
        match self {
            Self::LineString(pts) => Self::line_length_2d(pts),
            Self::LineStringZ(pts) => Self::line_length_3d(pts),
            Self::MultiLineString(lines) => lines.iter().map(|l| Self::line_length_2d(l)).sum(),
            Self::MultiLineStringZ(lines) => lines.iter().map(|l| Self::line_length_3d(l)).sum(),
            Self::Polygon(rings) => rings.iter().map(|r| Self::line_length_2d(r)).sum(),
            Self::PolygonZ(rings) => rings.iter().map(|r| Self::line_length_3d(r)).sum(),
            Self::MultiPolygon(polys) => polys
                .iter()
                .flat_map(|p| p.iter())
                .map(|r| Self::line_length_2d(r))
                .sum(),
            Self::MultiPolygonZ(polys) => polys
                .iter()
                .flat_map(|p| p.iter())
                .map(|r| Self::line_length_3d(r))
                .sum(),
            Self::GeometryCollection(geoms) => geoms.iter().map(|g| g.length()).sum(),
            _ => 0.0,
        }
    }

    // ── Centroid ─────────────────────────────────────────────────────────────

    /// Centroid of a 2-D polygon ring via the Bourke signed-area formula.
    fn ring_centroid_weighted(ring: &[[f64; 2]]) -> (f64, f64, f64) {
        let n = ring.len();
        if n == 0 {
            return (0.0, 0.0, 0.0);
        }
        if n == 1 {
            return (ring[0][0], ring[0][1], 0.0);
        }
        let mut cx = 0.0_f64;
        let mut cy = 0.0_f64;
        let mut area = 0.0_f64;
        for i in 0..n {
            let [x0, y0] = ring[i];
            let [x1, y1] = ring[(i + 1) % n];
            let cross = x0 * y1 - x1 * y0;
            cx += (x0 + x1) * cross;
            cy += (y0 + y1) * cross;
            area += cross;
        }
        area *= 0.5;
        if area.abs() < f64::EPSILON {
            // Degenerate ring — return arithmetic mean.
            let mx: f64 = ring.iter().map(|p| p[0]).sum::<f64>() / n as f64;
            let my: f64 = ring.iter().map(|p| p[1]).sum::<f64>() / n as f64;
            return (mx, my, 0.0);
        }
        (cx / (6.0 * area), cy / (6.0 * area), area.abs())
    }

    /// Centroid of a polygon (exterior minus holes, area-weighted).
    fn polygon_centroid_2d(rings: &[Vec<[f64; 2]>]) -> Option<[f64; 2]> {
        if rings.is_empty() {
            return None;
        }
        let (cx, cy, _) = Self::ring_centroid_weighted(&rings[0]);
        Some([cx, cy])
    }

    /// Length-weighted midpoint centroid for a 2-D line string.
    fn linestring_centroid_2d(pts: &[[f64; 2]]) -> Option<[f64; 2]> {
        if pts.is_empty() {
            return None;
        }
        if pts.len() == 1 {
            return Some(pts[0]);
        }
        let total_len = Self::line_length_2d(pts);
        if total_len < f64::EPSILON {
            let mx: f64 = pts.iter().map(|p| p[0]).sum::<f64>() / pts.len() as f64;
            let my: f64 = pts.iter().map(|p| p[1]).sum::<f64>() / pts.len() as f64;
            return Some([mx, my]);
        }
        let half = total_len * 0.5;
        let mut acc = 0.0;
        for w in pts.windows(2) {
            let dx = w[1][0] - w[0][0];
            let dy = w[1][1] - w[0][1];
            let seg_len = dx.hypot(dy);
            if acc + seg_len >= half {
                let t = (half - acc) / seg_len;
                return Some([w[0][0] + t * dx, w[0][1] + t * dy]);
            }
            acc += seg_len;
        }
        pts.last().copied()
    }

    /// Centroid of the geometry as `[x, y]`.
    ///
    /// - **Point / MultiPoint**: arithmetic mean of positions.
    /// - **LineString / MultiLineString**: length-weighted midpoint.
    /// - **Polygon / MultiPolygon**: area-weighted Bourke centroid of exterior ring(s).
    /// - **GeometryCollection**: area-weighted mean if any areal members exist, else
    ///   length-weighted, else point mean.
    /// - Returns `None` for `Null` and empty geometries.
    #[must_use]
    pub fn centroid(&self) -> Option<[f64; 2]> {
        match self {
            Self::Point([x, y]) => Some([*x, *y]),
            Self::PointZ([x, y, _]) => Some([*x, *y]),
            Self::MultiPoint(pts) => {
                if pts.is_empty() {
                    return None;
                }
                let n = pts.len() as f64;
                Some([
                    pts.iter().map(|p| p[0]).sum::<f64>() / n,
                    pts.iter().map(|p| p[1]).sum::<f64>() / n,
                ])
            }
            Self::MultiPointZ(pts) => {
                if pts.is_empty() {
                    return None;
                }
                let n = pts.len() as f64;
                Some([
                    pts.iter().map(|p| p[0]).sum::<f64>() / n,
                    pts.iter().map(|p| p[1]).sum::<f64>() / n,
                ])
            }
            Self::LineString(pts) => Self::linestring_centroid_2d(pts),
            Self::LineStringZ(pts) => {
                let pts2d: Vec<[f64; 2]> = pts.iter().map(|[x, y, _]| [*x, *y]).collect();
                Self::linestring_centroid_2d(&pts2d)
            }
            Self::MultiLineString(lines) => {
                let mut sum_x = 0.0_f64;
                let mut sum_y = 0.0_f64;
                let mut total_w = 0.0_f64;
                for l in lines {
                    let w = Self::line_length_2d(l);
                    if let Some([cx, cy]) = Self::linestring_centroid_2d(l) {
                        sum_x += cx * w;
                        sum_y += cy * w;
                        total_w += w;
                    }
                }
                if total_w < f64::EPSILON {
                    return None;
                }
                Some([sum_x / total_w, sum_y / total_w])
            }
            Self::MultiLineStringZ(lines) => {
                let mut sum_x = 0.0_f64;
                let mut sum_y = 0.0_f64;
                let mut total_w = 0.0_f64;
                for l in lines {
                    let pts2d: Vec<[f64; 2]> = l.iter().map(|[x, y, _]| [*x, *y]).collect();
                    let w = Self::line_length_2d(&pts2d);
                    if let Some([cx, cy]) = Self::linestring_centroid_2d(&pts2d) {
                        sum_x += cx * w;
                        sum_y += cy * w;
                        total_w += w;
                    }
                }
                if total_w < f64::EPSILON {
                    return None;
                }
                Some([sum_x / total_w, sum_y / total_w])
            }
            Self::Polygon(rings) => Self::polygon_centroid_2d(rings),
            Self::PolygonZ(rings) => {
                let rings_2d: Vec<Vec<[f64; 2]>> = rings
                    .iter()
                    .map(|r| r.iter().map(|[x, y, _]| [*x, *y]).collect())
                    .collect();
                Self::polygon_centroid_2d(&rings_2d)
            }
            Self::MultiPolygon(polys) => {
                let mut sum_x = 0.0_f64;
                let mut sum_y = 0.0_f64;
                let mut total_a = 0.0_f64;
                for p in polys {
                    let a = Self::polygon_area(p);
                    if let Some([cx, cy]) = Self::polygon_centroid_2d(p) {
                        sum_x += cx * a;
                        sum_y += cy * a;
                        total_a += a;
                    }
                }
                if total_a < f64::EPSILON {
                    return None;
                }
                Some([sum_x / total_a, sum_y / total_a])
            }
            Self::MultiPolygonZ(polys) => {
                let mut sum_x = 0.0_f64;
                let mut sum_y = 0.0_f64;
                let mut total_a = 0.0_f64;
                for p in polys {
                    let rings_2d: Vec<Vec<[f64; 2]>> = p
                        .iter()
                        .map(|r| r.iter().map(|[x, y, _]| [*x, *y]).collect())
                        .collect();
                    let a = Self::polygon_area(&rings_2d);
                    if let Some([cx, cy]) = Self::polygon_centroid_2d(&rings_2d) {
                        sum_x += cx * a;
                        sum_y += cy * a;
                        total_a += a;
                    }
                }
                if total_a < f64::EPSILON {
                    return None;
                }
                Some([sum_x / total_a, sum_y / total_a])
            }
            Self::GeometryCollection(geoms) => {
                // Prefer area-weighted if any areal member exists.
                let total_area: f64 = geoms.iter().map(|g| g.area()).sum();
                if total_area > f64::EPSILON {
                    let mut sx = 0.0_f64;
                    let mut sy = 0.0_f64;
                    for g in geoms {
                        let a = g.area();
                        if let Some([cx, cy]) = g.centroid() {
                            sx += cx * a;
                            sy += cy * a;
                        }
                    }
                    return Some([sx / total_area, sy / total_area]);
                }
                // Fall back to length-weighted.
                let total_len: f64 = geoms.iter().map(|g| g.length()).sum();
                if total_len > f64::EPSILON {
                    let mut sx = 0.0_f64;
                    let mut sy = 0.0_f64;
                    for g in geoms {
                        let l = g.length();
                        if let Some([cx, cy]) = g.centroid() {
                            sx += cx * l;
                            sy += cy * l;
                        }
                    }
                    return Some([sx / total_len, sy / total_len]);
                }
                // Point mean.
                let valid: Vec<[f64; 2]> = geoms.iter().filter_map(|g| g.centroid()).collect();
                if valid.is_empty() {
                    return None;
                }
                let n = valid.len() as f64;
                Some([
                    valid.iter().map(|p| p[0]).sum::<f64>() / n,
                    valid.iter().map(|p| p[1]).sum::<f64>() / n,
                ])
            }
            Self::Null => None,
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

    /// Returns the 3-D (6-element) bounding box of the feature's geometry.
    ///
    /// Returns `None` if the geometry is absent or 2-D only.
    #[must_use]
    pub fn bbox_3d(&self) -> Option<[f64; 6]> {
        self.geometry.as_ref()?.bbox_3d()
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

/// Compute full 3-D bounding box `[minx, miny, minz, maxx, maxy, maxz]`.
fn bbox_3d_full(pts: &[[f64; 3]]) -> Option<[f64; 6]> {
    if pts.is_empty() {
        return None;
    }
    let mut minx = pts[0][0];
    let mut miny = pts[0][1];
    let mut minz = pts[0][2];
    let mut maxx = pts[0][0];
    let mut maxy = pts[0][1];
    let mut maxz = pts[0][2];
    for [x, y, z] in pts.iter().skip(1) {
        if *x < minx {
            minx = *x;
        }
        if *y < miny {
            miny = *y;
        }
        if *z < minz {
            minz = *z;
        }
        if *x > maxx {
            maxx = *x;
        }
        if *y > maxy {
            maxy = *y;
        }
        if *z > maxz {
            maxz = *z;
        }
    }
    Some([minx, miny, minz, maxx, maxy, maxz])
}

/// Union of multiple 3-D bounding boxes.
pub fn union_bboxes_3d(bboxes: &[[f64; 6]]) -> Option<[f64; 6]> {
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
        if bb[2] < result[2] {
            result[2] = bb[2];
        }
        if bb[3] > result[3] {
            result[3] = bb[3];
        }
        if bb[4] > result[4] {
            result[4] = bb[4];
        }
        if bb[5] > result[5] {
            result[5] = bb[5];
        }
    }
    Some(result)
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

    // ── 6D bbox (3D) tests ──────────────────────────────────────────────────

    #[test]
    fn test_bbox_3d_point_z() {
        let g = GeoJsonGeometry::PointZ([10.0, 20.0, 100.0]);
        assert_eq!(g.bbox_3d(), Some([10.0, 20.0, 100.0, 10.0, 20.0, 100.0]));
    }

    #[test]
    fn test_bbox_3d_linestring_z() {
        let g = GeoJsonGeometry::LineStringZ(vec![
            [0.0, 0.0, 10.0],
            [10.0, 5.0, 50.0],
            [5.0, 8.0, 20.0],
        ]);
        assert_eq!(g.bbox_3d(), Some([0.0, 0.0, 10.0, 10.0, 8.0, 50.0]));
    }

    #[test]
    fn test_bbox_3d_polygon_z() {
        let g = GeoJsonGeometry::PolygonZ(vec![vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 100.0],
            [10.0, 10.0, 200.0],
            [0.0, 10.0, 50.0],
            [0.0, 0.0, 0.0],
        ]]);
        let bb = g.bbox_3d().expect("should have 3d bbox");
        assert_eq!(bb, [0.0, 0.0, 0.0, 10.0, 10.0, 200.0]);
    }

    #[test]
    fn test_bbox_3d_returns_none_for_2d() {
        let g = GeoJsonGeometry::Point([1.0, 2.0]);
        assert_eq!(g.bbox_3d(), None);
        let g2 = GeoJsonGeometry::LineString(vec![[0.0, 0.0], [1.0, 1.0]]);
        assert_eq!(g2.bbox_3d(), None);
    }

    #[test]
    fn test_bbox_3d_multi_point_z() {
        let g =
            GeoJsonGeometry::MultiPointZ(vec![[1.0, 2.0, 10.0], [3.0, 4.0, 20.0], [0.5, 1.5, 5.0]]);
        assert_eq!(g.bbox_3d(), Some([0.5, 1.5, 5.0, 3.0, 4.0, 20.0]));
    }

    #[test]
    fn test_bbox_3d_feature() {
        let f = GeoJsonFeature {
            id: None,
            geometry: Some(GeoJsonGeometry::PointZ([5.0, 10.0, 50.0])),
            properties: None,
        };
        assert_eq!(f.bbox_3d(), Some([5.0, 10.0, 50.0, 5.0, 10.0, 50.0]));
    }

    #[test]
    fn test_bbox_3d_geometry_collection() {
        let g = GeoJsonGeometry::GeometryCollection(vec![
            GeoJsonGeometry::PointZ([1.0, 2.0, 10.0]),
            GeoJsonGeometry::PointZ([5.0, 6.0, 50.0]),
        ]);
        assert_eq!(g.bbox_3d(), Some([1.0, 2.0, 10.0, 5.0, 6.0, 50.0]));
    }

    #[test]
    fn test_union_bboxes_3d() {
        let bboxes = vec![
            [0.0, 0.0, 0.0, 10.0, 10.0, 100.0],
            [5.0, -5.0, 50.0, 15.0, 5.0, 200.0],
        ];
        assert_eq!(
            union_bboxes_3d(&bboxes),
            Some([0.0, -5.0, 0.0, 15.0, 10.0, 200.0])
        );
    }
}
