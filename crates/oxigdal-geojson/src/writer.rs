//! GeoJSON writer — serialises geometry, features and collections to strings.

use crate::parser::{FeatureCollection, GeoJsonDocument};
use crate::types::{FeatureId, GeoJsonFeature, GeoJsonGeometry};

// ─── GeoJsonWriter ───────────────────────────────────────────────────────────

/// Configurable GeoJSON serialiser.
#[derive(Debug, Clone)]
pub struct GeoJsonWriter {
    /// `None` = compact, `Some(n)` = pretty-print with *n* spaces per indent.
    pub indent: Option<u32>,
    /// Number of decimal places for coordinates (default `6`).
    pub coordinate_precision: usize,
    /// Include a `"bbox"` field when `true`.
    pub bbox: bool,
}

impl Default for GeoJsonWriter {
    fn default() -> Self {
        Self {
            indent: None,
            coordinate_precision: 6,
            bbox: false,
        }
    }
}

impl GeoJsonWriter {
    /// Compact (single-line) output.
    #[must_use]
    pub fn compact() -> Self {
        Self::default()
    }

    /// Pretty-printed output with `indent` spaces per level.
    #[must_use]
    pub fn pretty(indent: u32) -> Self {
        Self {
            indent: Some(indent),
            ..Self::default()
        }
    }

    /// Override coordinate decimal precision.
    #[must_use]
    pub fn with_precision(mut self, precision: usize) -> Self {
        self.coordinate_precision = precision;
        self
    }

    /// Include computed `"bbox"` fields.
    #[must_use]
    pub fn with_bbox(mut self) -> Self {
        self.bbox = true;
        self
    }

    // ── Public write methods ─────────────────────────────────────────────

    /// Serialise a [`FeatureCollection`].
    #[must_use]
    pub fn write_feature_collection(&self, fc: &FeatureCollection) -> String {
        let bbox_opt = if self.bbox { fc.compute_bbox() } else { None };
        self.write_features_iter(fc.features.iter(), bbox_opt)
    }

    /// Serialise a single [`GeoJsonFeature`].
    #[must_use]
    pub fn write_feature(&self, feature: &GeoJsonFeature) -> String {
        self.serialize_feature(feature)
    }

    /// Serialise a [`GeoJsonGeometry`].
    #[must_use]
    pub fn write_geometry(&self, geom: &GeoJsonGeometry) -> String {
        let compact = self.serialize_geometry(geom);
        if let Some(spaces) = self.indent {
            pretty_print(&compact, spaces)
        } else {
            compact
        }
    }

    /// Write a `FeatureCollection` from an iterator of feature references.
    ///
    /// Builds the output string iteratively — suited to large collections.
    #[must_use]
    pub fn write_features_iter<'a, I>(&self, features: I, bbox: Option<[f64; 4]>) -> String
    where
        I: Iterator<Item = &'a GeoJsonFeature>,
    {
        let mut out = String::from(r#"{"type":"FeatureCollection""#);

        if let Some(bb) = bbox {
            out.push_str(&format!(
                r#","bbox":[{},{},{},{}]"#,
                self.format_coord(bb[0]),
                self.format_coord(bb[1]),
                self.format_coord(bb[2]),
                self.format_coord(bb[3])
            ));
        }

        out.push_str(r#","features":["#);
        let mut first = true;
        for feat in features {
            if !first {
                out.push(',');
            }
            first = false;
            out.push_str(&self.serialize_feature(feat));
        }
        out.push_str("]}");

        if let Some(spaces) = self.indent {
            pretty_print(&out, spaces)
        } else {
            out
        }
    }

    /// Serialise a [`GeoJsonDocument`].
    #[must_use]
    pub fn write_document(&self, doc: &GeoJsonDocument) -> String {
        match doc {
            GeoJsonDocument::FeatureCollection(fc) => self.write_feature_collection(fc),
            GeoJsonDocument::Feature(f) => self.write_feature(f),
            GeoJsonDocument::Geometry(g) => self.write_geometry(g),
        }
    }

    // ── Private helpers ───────────────────────────────────────────────────

    fn serialize_feature(&self, feature: &GeoJsonFeature) -> String {
        let mut out = String::from(r#"{"type":"Feature""#);

        // id
        if let Some(id) = &feature.id {
            match id {
                FeatureId::String(s) => {
                    out.push_str(&format!(r#","id":"{}""#, escape_json_string(s)));
                }
                FeatureId::Number(n) => {
                    out.push_str(&format!(r#","id":{}"#, self.format_coord(*n)));
                }
            }
        }

        // geometry
        out.push_str(r#","geometry":"#);
        match &feature.geometry {
            None | Some(GeoJsonGeometry::Null) => out.push_str("null"),
            Some(geom) => out.push_str(&self.serialize_geometry(geom)),
        }

        // properties
        out.push_str(r#","properties":"#);
        match &feature.properties {
            None => out.push_str("null"),
            Some(props) => {
                // Use serde_json for property serialisation
                out.push_str(&serde_json::to_string(props).unwrap_or_else(|_| "null".into()));
            }
        }

        out.push('}');

        if let Some(spaces) = self.indent {
            pretty_print(&out, spaces)
        } else {
            out
        }
    }

    fn serialize_geometry(&self, geom: &GeoJsonGeometry) -> String {
        match geom {
            GeoJsonGeometry::Null => "null".into(),
            GeoJsonGeometry::Point([x, y]) => {
                format!(
                    r#"{{"type":"Point","coordinates":[{},{}]}}"#,
                    self.format_coord(*x),
                    self.format_coord(*y)
                )
            }
            GeoJsonGeometry::PointZ([x, y, z]) => {
                format!(
                    r#"{{"type":"Point","coordinates":[{},{},{}]}}"#,
                    self.format_coord(*x),
                    self.format_coord(*y),
                    self.format_coord(*z)
                )
            }
            GeoJsonGeometry::LineString(pts) => {
                format!(
                    r#"{{"type":"LineString","coordinates":{}}}"#,
                    self.serialize_ring_2d(pts)
                )
            }
            GeoJsonGeometry::LineStringZ(pts) => {
                format!(
                    r#"{{"type":"LineString","coordinates":{}}}"#,
                    self.serialize_ring_3d(pts)
                )
            }
            GeoJsonGeometry::Polygon(rings) => {
                let rings_s: Vec<String> =
                    rings.iter().map(|r| self.serialize_ring_2d(r)).collect();
                format!(
                    r#"{{"type":"Polygon","coordinates":[{}]}}"#,
                    rings_s.join(",")
                )
            }
            GeoJsonGeometry::PolygonZ(rings) => {
                let rings_s: Vec<String> =
                    rings.iter().map(|r| self.serialize_ring_3d(r)).collect();
                format!(
                    r#"{{"type":"Polygon","coordinates":[{}]}}"#,
                    rings_s.join(",")
                )
            }
            GeoJsonGeometry::MultiPoint(pts) => {
                format!(
                    r#"{{"type":"MultiPoint","coordinates":{}}}"#,
                    self.serialize_ring_2d(pts)
                )
            }
            GeoJsonGeometry::MultiLineString(lines) => {
                let lines_s: Vec<String> =
                    lines.iter().map(|l| self.serialize_ring_2d(l)).collect();
                format!(
                    r#"{{"type":"MultiLineString","coordinates":[{}]}}"#,
                    lines_s.join(",")
                )
            }
            GeoJsonGeometry::MultiPolygon(polys) => {
                let polys_s: Vec<String> = polys
                    .iter()
                    .map(|poly| {
                        let rings_s: Vec<String> =
                            poly.iter().map(|r| self.serialize_ring_2d(r)).collect();
                        format!("[{}]", rings_s.join(","))
                    })
                    .collect();
                format!(
                    r#"{{"type":"MultiPolygon","coordinates":[{}]}}"#,
                    polys_s.join(",")
                )
            }
            GeoJsonGeometry::GeometryCollection(geoms) => {
                let geoms_s: Vec<String> =
                    geoms.iter().map(|g| self.serialize_geometry(g)).collect();
                format!(
                    r#"{{"type":"GeometryCollection","geometries":[{}]}}"#,
                    geoms_s.join(",")
                )
            }
        }
    }

    fn serialize_ring_2d(&self, pts: &[[f64; 2]]) -> String {
        let coords: Vec<String> = pts
            .iter()
            .map(|[x, y]| format!("[{},{}]", self.format_coord(*x), self.format_coord(*y)))
            .collect();
        format!("[{}]", coords.join(","))
    }

    fn serialize_ring_3d(&self, pts: &[[f64; 3]]) -> String {
        let coords: Vec<String> = pts
            .iter()
            .map(|[x, y, z]| {
                format!(
                    "[{},{},{}]",
                    self.format_coord(*x),
                    self.format_coord(*y),
                    self.format_coord(*z)
                )
            })
            .collect();
        format!("[{}]", coords.join(","))
    }

    /// Format a single floating-point coordinate with configured precision.
    fn format_coord(&self, v: f64) -> String {
        format!("{:.prec$}", v, prec = self.coordinate_precision)
    }
}

// ─── Validation ──────────────────────────────────────────────────────────────

/// GeoJSON conformance validator.
pub struct GeoJsonValidator;

impl GeoJsonValidator {
    /// Validate a geometry and collect issues.
    #[must_use]
    pub fn validate_geometry(geom: &GeoJsonGeometry) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        validate_geometry_inner(geom, None, &mut issues);
        issues
    }

    /// Validate a feature and collect issues.
    #[must_use]
    pub fn validate_feature(feature: &GeoJsonFeature) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if let Some(geom) = &feature.geometry {
            validate_geometry_inner(geom, Some("geometry"), &mut issues);
        }
        issues
    }

    /// Validate a full feature collection.
    #[must_use]
    pub fn validate_feature_collection(fc: &FeatureCollection) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        for (i, feat) in fc.features.iter().enumerate() {
            if let Some(geom) = &feat.geometry {
                let path = format!("features[{}].geometry", i);
                validate_geometry_inner(geom, Some(&path), &mut issues);
            }
        }
        issues
    }
}

/// A single validation finding.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationIssue {
    /// Severity of the issue.
    pub severity: IssueSeverity,
    /// Human-readable message.
    pub message: String,
    /// JSON path to the offending element, if known.
    pub path: Option<String>,
}

/// Classification of validation issues.
#[derive(Debug, Clone, PartialEq)]
pub enum IssueSeverity {
    /// Issue that makes the geometry non-conformant.
    Error,
    /// Issue worth noting but not strictly invalid.
    Warning,
}

// ─── Validation helpers ───────────────────────────────────────────────────────

fn validate_geometry_inner(
    geom: &GeoJsonGeometry,
    path: Option<&str>,
    issues: &mut Vec<ValidationIssue>,
) {
    let path_owned = path.map(ToOwned::to_owned);

    match geom {
        GeoJsonGeometry::Null => {}

        GeoJsonGeometry::Point([x, y]) => {
            check_finite_2(*x, *y, path, issues);
            check_lon_lat(*x, *y, path, issues);
        }
        GeoJsonGeometry::PointZ([x, y, z]) => {
            check_finite_3(*x, *y, *z, path, issues);
            check_lon_lat(*x, *y, path, issues);
        }

        GeoJsonGeometry::LineString(pts) => {
            if pts.len() < 2 {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    message: "LineString must have at least 2 positions".into(),
                    path: path_owned,
                });
            }
            for [x, y] in pts {
                check_finite_2(*x, *y, path, issues);
                check_lon_lat(*x, *y, path, issues);
            }
        }
        GeoJsonGeometry::LineStringZ(pts) => {
            if pts.len() < 2 {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    message: "LineString must have at least 2 positions".into(),
                    path: path_owned,
                });
            }
            for [x, y, z] in pts {
                check_finite_3(*x, *y, *z, path, issues);
                check_lon_lat(*x, *y, path, issues);
            }
        }

        GeoJsonGeometry::Polygon(rings) => {
            validate_polygon_rings_2d(rings, path, path_owned, issues);
        }
        GeoJsonGeometry::PolygonZ(rings) => {
            validate_polygon_rings_3d(rings, path, path_owned, issues);
        }

        GeoJsonGeometry::MultiPoint(pts) => {
            for [x, y] in pts {
                check_finite_2(*x, *y, path, issues);
                check_lon_lat(*x, *y, path, issues);
            }
        }

        GeoJsonGeometry::MultiLineString(lines) => {
            for line in lines {
                if line.len() < 2 {
                    issues.push(ValidationIssue {
                        severity: IssueSeverity::Error,
                        message: "LineString in MultiLineString must have ≥2 positions".into(),
                        path: path_owned.clone(),
                    });
                }
                for [x, y] in line {
                    check_finite_2(*x, *y, path, issues);
                    check_lon_lat(*x, *y, path, issues);
                }
            }
        }

        GeoJsonGeometry::MultiPolygon(polys) => {
            for rings in polys {
                validate_polygon_rings_2d(rings, path, path_owned.clone(), issues);
            }
        }

        GeoJsonGeometry::GeometryCollection(geoms) => {
            for (i, g) in geoms.iter().enumerate() {
                let sub_path = format!("{}[{}]", path.unwrap_or("geometry"), i);
                validate_geometry_inner(g, Some(&sub_path), issues);
            }
        }
    }
}

fn validate_polygon_rings_2d(
    rings: &[Vec<[f64; 2]>],
    path: Option<&str>,
    path_owned: Option<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    for (ri, ring) in rings.iter().enumerate() {
        if ring.len() < 4 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                message: format!("Polygon ring[{}] must have ≥4 positions", ri),
                path: path_owned.clone(),
            });
        }
        // Check first == last (closed ring)
        if ring.len() >= 2 {
            let first = ring[0];
            let last = ring[ring.len() - 1];
            if (first[0] - last[0]).abs() > f64::EPSILON
                || (first[1] - last[1]).abs() > f64::EPSILON
            {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    message: format!("Polygon ring[{}] must be closed (first == last)", ri),
                    path: path_owned.clone(),
                });
            }
        }
        for [x, y] in ring {
            check_finite_2(*x, *y, path, issues);
            check_lon_lat(*x, *y, path, issues);
        }
    }
}

fn validate_polygon_rings_3d(
    rings: &[Vec<[f64; 3]>],
    path: Option<&str>,
    path_owned: Option<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    for (ri, ring) in rings.iter().enumerate() {
        if ring.len() < 4 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                message: format!("Polygon ring[{}] must have ≥4 positions", ri),
                path: path_owned.clone(),
            });
        }
        for [x, y, z] in ring {
            check_finite_3(*x, *y, *z, path, issues);
            check_lon_lat(*x, *y, path, issues);
        }
    }
}

fn check_finite_2(x: f64, y: f64, path: Option<&str>, issues: &mut Vec<ValidationIssue>) {
    if !x.is_finite() || !y.is_finite() {
        issues.push(ValidationIssue {
            severity: IssueSeverity::Error,
            message: format!("Non-finite coordinate ({}, {})", x, y),
            path: path.map(ToOwned::to_owned),
        });
    }
}

fn check_finite_3(x: f64, y: f64, z: f64, path: Option<&str>, issues: &mut Vec<ValidationIssue>) {
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        issues.push(ValidationIssue {
            severity: IssueSeverity::Error,
            message: format!("Non-finite coordinate ({}, {}, {})", x, y, z),
            path: path.map(ToOwned::to_owned),
        });
    }
}

fn check_lon_lat(lon: f64, lat: f64, path: Option<&str>, issues: &mut Vec<ValidationIssue>) {
    if !(-180.0..=180.0).contains(&lon) {
        issues.push(ValidationIssue {
            severity: IssueSeverity::Warning,
            message: format!("Longitude {} outside [-180, 180]", lon),
            path: path.map(ToOwned::to_owned),
        });
    }
    if !(-90.0..=90.0).contains(&lat) {
        issues.push(ValidationIssue {
            severity: IssueSeverity::Warning,
            message: format!("Latitude {} outside [-90, 90]", lat),
            path: path.map(ToOwned::to_owned),
        });
    }
}

// ─── Pretty-print helper ─────────────────────────────────────────────────────

/// Very simple JSON pretty-printer that inserts indentation.
fn pretty_print(compact: &str, spaces: u32) -> String {
    let indent = " ".repeat(spaces as usize);
    let mut out = String::with_capacity(compact.len() * 2);
    let mut depth: usize = 0;
    let mut in_string = false;
    let mut escaped = false;
    let chars: Vec<char> = compact.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        if escaped {
            out.push(c);
            escaped = false;
            i += 1;
            continue;
        }

        if in_string {
            if c == '\\' {
                escaped = true;
                out.push(c);
            } else {
                if c == '"' {
                    in_string = false;
                }
                out.push(c);
            }
            i += 1;
            continue;
        }

        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '{' | '[' => {
                out.push(c);
                // Peek: if immediately closed, don't add newline
                let next = chars.get(i + 1).copied();
                if next != Some('}') && next != Some(']') {
                    depth += 1;
                    out.push('\n');
                    for _ in 0..depth {
                        out.push_str(&indent);
                    }
                }
            }
            '}' | ']' => {
                // Peek back: if last non-space was open bracket, no newline
                let prev = out.chars().last();
                if prev != Some('{') && prev != Some('[') {
                    depth = depth.saturating_sub(1);
                    out.push('\n');
                    for _ in 0..depth {
                        out.push_str(&indent);
                    }
                }
                out.push(c);
            }
            ',' => {
                out.push(c);
                out.push('\n');
                for _ in 0..depth {
                    out.push_str(&indent);
                }
            }
            ':' => {
                out.push(c);
                out.push(' ');
            }
            ' ' | '\n' | '\r' | '\t' => {
                // skip existing whitespace outside strings
            }
            _ => {
                out.push(c);
            }
        }
        i += 1;
    }
    out
}

/// Escape a string for JSON embedding.
fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_point() {
        let g = GeoJsonGeometry::Point([1.0, 2.0]);
        let w = GeoJsonWriter::compact().with_precision(1);
        let s = w.write_geometry(&g);
        assert!(s.contains("\"Point\""));
        assert!(s.contains("[1.0,2.0]"));
    }

    #[test]
    fn test_pretty_output_has_newlines() {
        let g = GeoJsonGeometry::Point([0.0, 0.0]);
        let w = GeoJsonWriter::pretty(2);
        let s = w.write_geometry(&g);
        assert!(s.contains('\n'));
    }
}
