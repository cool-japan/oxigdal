//! GeoPackage vector feature table types.
//!
//! Provides pure-Rust data structures for GeoPackage geometry (GeoPackageBinary /
//! WKB), field definitions, feature rows, and feature tables.  No SQLite
//! library is required — this module is a binary parser.

use std::collections::HashMap;

use crate::error::GpkgError;

// ─────────────────────────────────────────────────────────────────────────────
// FieldType
// ─────────────────────────────────────────────────────────────────────────────

/// Column type categories used in GeoPackage / SQLite schemas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldType {
    /// Signed integer (SQLite INTEGER affinity).
    Integer,
    /// IEEE-754 double (SQLite REAL affinity).
    Real,
    /// UTF-8 text (SQLite TEXT affinity).
    Text,
    /// Raw binary (SQLite BLOB affinity).
    Blob,
    /// Boolean stored as INTEGER 0/1.
    Boolean,
    /// Calendar date stored as TEXT `"YYYY-MM-DD"`.
    Date,
    /// Date+time stored as TEXT `"YYYY-MM-DDTHH:MM:SS.sssZ"`.
    DateTime,
    /// SQL NULL / unknown type.
    Null,
}

impl FieldType {
    /// Derive a [`FieldType`] from a SQLite type-name string (case-insensitive).
    ///
    /// Unrecognised strings map to [`FieldType::Text`] following SQLite type
    /// affinity rules.
    pub fn from_sql_type(type_str: &str) -> Self {
        match type_str.to_ascii_uppercase().trim() {
            "INTEGER" | "INT" | "TINYINT" | "SMALLINT" | "MEDIUMINT" | "BIGINT"
            | "UNSIGNED BIG INT" | "INT2" | "INT8" => Self::Integer,
            "REAL" | "DOUBLE" | "DOUBLE PRECISION" | "FLOAT" | "NUMERIC" | "DECIMAL" => Self::Real,
            "BLOB" => Self::Blob,
            "BOOLEAN" | "BOOL" => Self::Boolean,
            "DATE" => Self::Date,
            "DATETIME" | "TIMESTAMP" => Self::DateTime,
            "NULL" => Self::Null,
            _ => Self::Text,
        }
    }

    /// Return the canonical SQL type name string for this field type.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Integer => "INTEGER",
            Self::Real => "REAL",
            Self::Text => "TEXT",
            Self::Blob => "BLOB",
            Self::Boolean => "BOOLEAN",
            Self::Date => "DATE",
            Self::DateTime => "DATETIME",
            Self::Null => "NULL",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FieldValue
// ─────────────────────────────────────────────────────────────────────────────

/// A runtime value read from a GeoPackage feature-table column.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    /// Signed 64-bit integer.
    Integer(i64),
    /// IEEE-754 double-precision float.
    Real(f64),
    /// UTF-8 text.
    Text(String),
    /// Raw binary data.
    Blob(Vec<u8>),
    /// Boolean value.
    Boolean(bool),
    /// SQL NULL.
    Null,
}

impl FieldValue {
    /// Return the contained integer, or `None` for other variants.
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(v) => Some(*v),
            _ => None,
        }
    }

    /// Return the contained float, or `None` for other variants.
    pub fn as_real(&self) -> Option<f64> {
        match self {
            Self::Real(v) => Some(*v),
            _ => None,
        }
    }

    /// Return a reference to the contained text, or `None` for other variants.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Return the contained boolean, or `None` for other variants.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Return `true` if this is the SQL NULL variant.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Return the [`FieldType`] that corresponds to this value's variant.
    pub fn field_type(&self) -> FieldType {
        match self {
            Self::Integer(_) => FieldType::Integer,
            Self::Real(_) => FieldType::Real,
            Self::Text(_) => FieldType::Text,
            Self::Blob(_) => FieldType::Blob,
            Self::Boolean(_) => FieldType::Boolean,
            Self::Null => FieldType::Null,
        }
    }

    /// Serialise this value as a JSON fragment (no trailing newline).
    fn to_json(&self) -> String {
        match self {
            Self::Integer(v) => v.to_string(),
            Self::Real(v) => {
                if v.is_finite() {
                    format!("{v}")
                } else {
                    "null".into()
                }
            }
            Self::Text(s) => json_string_escape(s),
            Self::Blob(b) => {
                // Encode as a hex string prefixed with "0x"
                let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
                json_string_escape(&format!("0x{hex}"))
            }
            Self::Boolean(b) => if *b { "true" } else { "false" }.into(),
            Self::Null => "null".into(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FieldDefinition
// ─────────────────────────────────────────────────────────────────────────────

/// Schema description of a single column in a feature table.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDefinition {
    /// Column name.
    pub name: String,
    /// Declared column type.
    pub field_type: FieldType,
    /// `true` when a NOT NULL constraint is present.
    pub not_null: bool,
    /// `true` when this column is (part of) the primary key.
    pub primary_key: bool,
    /// Optional DEFAULT expression as a raw SQL string.
    pub default_value: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// GpkgGeometry
// ─────────────────────────────────────────────────────────────────────────────

/// A decoded GeoPackage geometry value.
///
/// Coordinates are always (x, y) pairs — typically (longitude, latitude) for
/// geographic SRSs or (easting, northing) for projected ones.
#[derive(Debug, Clone, PartialEq)]
pub enum GpkgGeometry {
    /// A single point.
    Point {
        /// X coordinate (longitude / easting).
        x: f64,
        /// Y coordinate (latitude / northing).
        y: f64,
    },
    /// An ordered sequence of points forming a line.
    LineString {
        /// Coordinate pairs along the line.
        coords: Vec<(f64, f64)>,
    },
    /// A polygon defined by one exterior ring and zero or more interior rings.
    Polygon {
        /// Rings: index 0 is the exterior ring; subsequent entries are holes.
        rings: Vec<Vec<(f64, f64)>>,
    },
    /// A collection of points.
    MultiPoint {
        /// Individual point coordinates.
        points: Vec<(f64, f64)>,
    },
    /// A collection of line strings.
    MultiLineString {
        /// Individual line strings, each as a coordinate sequence.
        lines: Vec<Vec<(f64, f64)>>,
    },
    /// A collection of polygons.
    MultiPolygon {
        /// Individual polygons, each as a list of rings.
        polygons: Vec<Vec<Vec<(f64, f64)>>>,
    },
    /// A heterogeneous collection of geometries.
    GeometryCollection(Vec<GpkgGeometry>),
    /// An explicitly empty geometry (GeoPackage envelope-indicator = 0, empty flag set).
    Empty,
}

impl GpkgGeometry {
    /// Return the OGC geometry-type name (uppercase).
    pub fn geometry_type(&self) -> &'static str {
        match self {
            Self::Point { .. } => "Point",
            Self::LineString { .. } => "LineString",
            Self::Polygon { .. } => "Polygon",
            Self::MultiPoint { .. } => "MultiPoint",
            Self::MultiLineString { .. } => "MultiLineString",
            Self::MultiPolygon { .. } => "MultiPolygon",
            Self::GeometryCollection(_) => "GeometryCollection",
            Self::Empty => "Empty",
        }
    }

    /// Return the total number of coordinate points in this geometry.
    pub fn point_count(&self) -> usize {
        match self {
            Self::Point { .. } => 1,
            Self::LineString { coords } => coords.len(),
            Self::Polygon { rings } => rings.iter().map(|r| r.len()).sum(),
            Self::MultiPoint { points } => points.len(),
            Self::MultiLineString { lines } => lines.iter().map(|l| l.len()).sum(),
            Self::MultiPolygon { polygons } => polygons
                .iter()
                .flat_map(|poly| poly.iter())
                .map(|ring| ring.len())
                .sum(),
            Self::GeometryCollection(geoms) => geoms.iter().map(|g| g.point_count()).sum(),
            Self::Empty => 0,
        }
    }

    /// Return the axis-aligned bounding box `(min_x, min_y, max_x, max_y)`, or
    /// `None` for empty / zero-point geometries.
    pub fn bbox(&self) -> Option<(f64, f64, f64, f64)> {
        let coords: Vec<(f64, f64)> = self.collect_coords();
        if coords.is_empty() {
            return None;
        }
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for (x, y) in &coords {
            if *x < min_x {
                min_x = *x;
            }
            if *y < min_y {
                min_y = *y;
            }
            if *x > max_x {
                max_x = *x;
            }
            if *y > max_y {
                max_y = *y;
            }
        }
        if min_x.is_finite() {
            Some((min_x, min_y, max_x, max_y))
        } else {
            None
        }
    }

    /// Collect all coordinate pairs depth-first.
    fn collect_coords(&self) -> Vec<(f64, f64)> {
        match self {
            Self::Point { x, y } => vec![(*x, *y)],
            Self::LineString { coords } => coords.clone(),
            Self::Polygon { rings } => rings.iter().flatten().copied().collect(),
            Self::MultiPoint { points } => points.clone(),
            Self::MultiLineString { lines } => lines.iter().flatten().copied().collect(),
            Self::MultiPolygon { polygons } => polygons
                .iter()
                .flat_map(|poly| poly.iter().flatten())
                .copied()
                .collect(),
            Self::GeometryCollection(geoms) => {
                geoms.iter().flat_map(|g| g.collect_coords()).collect()
            }
            Self::Empty => vec![],
        }
    }

    /// Serialise this geometry as a GeoJSON geometry object string.
    pub(crate) fn to_geojson_geometry(&self) -> String {
        match self {
            Self::Point { x, y } => {
                format!(r#"{{"type":"Point","coordinates":[{x},{y}]}}"#)
            }
            Self::LineString { coords } => {
                let pts = coords_to_json_array(coords);
                format!(r#"{{"type":"LineString","coordinates":{pts}}}"#)
            }
            Self::Polygon { rings } => {
                let rings_json = rings
                    .iter()
                    .map(|r| coords_to_json_array(r))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"Polygon","coordinates":[{rings_json}]}}"#)
            }
            Self::MultiPoint { points } => {
                let pts = coords_to_json_array(points);
                format!(r#"{{"type":"MultiPoint","coordinates":{pts}}}"#)
            }
            Self::MultiLineString { lines } => {
                let lines_json = lines
                    .iter()
                    .map(|l| coords_to_json_array(l))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"MultiLineString","coordinates":[{lines_json}]}}"#)
            }
            Self::MultiPolygon { polygons } => {
                let polys_json = polygons
                    .iter()
                    .map(|poly| {
                        let rings_json = poly
                            .iter()
                            .map(|r| coords_to_json_array(r))
                            .collect::<Vec<_>>()
                            .join(",");
                        format!("[{rings_json}]")
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"MultiPolygon","coordinates":[{polys_json}]}}"#)
            }
            Self::GeometryCollection(geoms) => {
                let geom_json = geoms
                    .iter()
                    .map(|g| g.to_geojson_geometry())
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"GeometryCollection","geometries":[{geom_json}]}}"#)
            }
            Self::Empty => "null".into(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GpkgBinaryParser
// ─────────────────────────────────────────────────────────────────────────────

/// Parser and encoder for the GeoPackageBinary (GPB) and WKB geometry formats.
///
/// The GeoPackageBinary layout is:
///
/// ```text
/// magic[2]     = 0x47 0x50  ("GP")
/// version[1]
/// flags[1]     bits 0-2: envelope indicator
///              bit  3:   empty-geometry flag
///              bit  5:   byte order (0=BE, 1=LE)
/// srs_id[4]    i32, same byte order as flags bit 5
/// envelope     0/32/48/64 bytes depending on flags bits 0-2
/// WKB          remainder of the blob
/// ```
pub struct GpkgBinaryParser;

impl GpkgBinaryParser {
    /// Parse a GeoPackageBinary blob into a [`GpkgGeometry`].
    ///
    /// # Errors
    /// - [`GpkgError::InvalidGeometryMagic`] — first two bytes are not `GP`
    /// - [`GpkgError::InsufficientData`] — blob too short
    /// - [`GpkgError::WkbParseError`] / [`GpkgError::UnknownWkbType`] — WKB invalid
    pub fn parse(data: &[u8]) -> Result<GpkgGeometry, GpkgError> {
        if data.len() < 8 {
            return Err(GpkgError::InsufficientData {
                needed: 8,
                available: data.len(),
            });
        }
        if data[0] != 0x47 || data[1] != 0x50 {
            return Err(GpkgError::InvalidGeometryMagic);
        }

        let flags = data[3];
        let is_little_endian = (flags >> 5) & 1 == 1;
        let envelope_indicator = flags & 0b0000_0111;
        let empty_flag = (flags >> 3) & 1 == 1;

        // srs_id at bytes 4..8 (not used for geometry parsing, but we consume it)
        let _srs_id: i32 = if is_little_endian {
            i32::from_le_bytes([data[4], data[5], data[6], data[7]])
        } else {
            i32::from_be_bytes([data[4], data[5], data[6], data[7]])
        };

        // Envelope size in bytes: 0, 32, 48, 48, or 64
        let envelope_bytes: usize = match envelope_indicator {
            0 => 0,
            1 => 32,
            2 | 3 => 48,
            4 => 64,
            _ => {
                return Err(GpkgError::WkbParseError(format!(
                    "Unknown envelope indicator {envelope_indicator}"
                )));
            }
        };

        let header_size = 8 + envelope_bytes;
        if data.len() < header_size {
            return Err(GpkgError::InsufficientData {
                needed: header_size,
                available: data.len(),
            });
        }

        if empty_flag {
            return Ok(GpkgGeometry::Empty);
        }

        let wkb = &data[header_size..];
        if wkb.is_empty() {
            return Ok(GpkgGeometry::Empty);
        }
        Self::parse_wkb(wkb)
    }

    /// Parse a WKB (Well-Known Binary) blob into a [`GpkgGeometry`].
    ///
    /// Both big-endian (`byte_order = 0`) and little-endian (`byte_order = 1`)
    /// WKB are supported.
    pub fn parse_wkb(data: &[u8]) -> Result<GpkgGeometry, GpkgError> {
        let (geom, _consumed) = parse_wkb_inner(data, 0)?;
        Ok(geom)
    }

    /// Encode a [`GpkgGeometry`] as little-endian WKB.
    pub fn to_wkb(geom: &GpkgGeometry) -> Vec<u8> {
        let mut buf = Vec::new();
        write_wkb(geom, &mut buf);
        buf
    }

    /// Encode a [`GpkgGeometry`] as a GeoPackageBinary blob (no envelope, LE byte order).
    ///
    /// The resulting bytes begin with the magic `GP` (`0x47 0x50`).
    pub fn to_gpb(geom: &GpkgGeometry, srs_id: i32) -> Vec<u8> {
        let mut buf = Vec::new();
        // magic
        buf.push(0x47); // 'G'
        buf.push(0x50); // 'P'
        // version
        buf.push(0);
        // flags: no envelope (bits 0-2 = 0), not empty (bit 3 = 0), LE (bit 5 = 1)
        let is_empty = matches!(geom, GpkgGeometry::Empty);
        let empty_bit: u8 = if is_empty { 1 << 3 } else { 0 };
        let flags: u8 = empty_bit | (1 << 5); // LE, no envelope
        buf.push(flags);
        // srs_id (LE i32)
        buf.extend_from_slice(&srs_id.to_le_bytes());
        // WKB body
        if !is_empty {
            write_wkb(geom, &mut buf);
        }
        buf
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WKB internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// WKB type constants.
const WKB_POINT: u32 = 1;
const WKB_LINESTRING: u32 = 2;
const WKB_POLYGON: u32 = 3;
const WKB_MULTIPOINT: u32 = 4;
const WKB_MULTILINESTRING: u32 = 5;
const WKB_MULTIPOLYGON: u32 = 6;
const WKB_GEOMETRYCOLLECTION: u32 = 7;

/// Parse one WKB geometry starting at `data[offset]`.
/// Returns `(geometry, new_offset)`.
fn parse_wkb_inner(data: &[u8], offset: usize) -> Result<(GpkgGeometry, usize), GpkgError> {
    if data.len() < offset + 5 {
        return Err(GpkgError::InsufficientData {
            needed: offset + 5,
            available: data.len(),
        });
    }
    let byte_order = data[offset];
    let le = byte_order == 1;
    let mut pos = offset + 1;

    let wkb_type = read_u32(data, pos, le)?;
    pos += 4;

    match wkb_type {
        WKB_POINT => {
            let (x, y, new_pos) = read_point_coords(data, pos, le)?;
            Ok((GpkgGeometry::Point { x, y }, new_pos))
        }
        WKB_LINESTRING => {
            let (coords, new_pos) = read_coord_sequence(data, pos, le)?;
            Ok((GpkgGeometry::LineString { coords }, new_pos))
        }
        WKB_POLYGON => {
            let (rings, new_pos) = read_rings(data, pos, le)?;
            Ok((GpkgGeometry::Polygon { rings }, new_pos))
        }
        WKB_MULTIPOINT => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut points = Vec::with_capacity(n as usize);
            for _ in 0..n {
                // Each sub-geometry is a complete WKB Point
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::Point { x, y } => points.push((x, y)),
                    other => {
                        return Err(GpkgError::WkbParseError(format!(
                            "Expected Point in MultiPoint, got {}",
                            other.geometry_type()
                        )));
                    }
                }
            }
            Ok((GpkgGeometry::MultiPoint { points }, pos2))
        }
        WKB_MULTILINESTRING => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut lines = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::LineString { coords } => lines.push(coords),
                    other => {
                        return Err(GpkgError::WkbParseError(format!(
                            "Expected LineString in MultiLineString, got {}",
                            other.geometry_type()
                        )));
                    }
                }
            }
            Ok((GpkgGeometry::MultiLineString { lines }, pos2))
        }
        WKB_MULTIPOLYGON => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut polygons = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::Polygon { rings } => polygons.push(rings),
                    other => {
                        return Err(GpkgError::WkbParseError(format!(
                            "Expected Polygon in MultiPolygon, got {}",
                            other.geometry_type()
                        )));
                    }
                }
            }
            Ok((GpkgGeometry::MultiPolygon { polygons }, pos2))
        }
        WKB_GEOMETRYCOLLECTION => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut geoms = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                geoms.push(sub);
            }
            Ok((GpkgGeometry::GeometryCollection(geoms), pos2))
        }
        other => Err(GpkgError::UnknownWkbType(other)),
    }
}

/// Read a u32 from `data[pos]` with the given byte order, returning (value, pos+4).
fn read_u32_pos(data: &[u8], pos: usize, le: bool) -> Result<(u32, usize), GpkgError> {
    Ok((read_u32(data, pos, le)?, pos + 4))
}

fn read_u32(data: &[u8], pos: usize, le: bool) -> Result<u32, GpkgError> {
    if data.len() < pos + 4 {
        return Err(GpkgError::InsufficientData {
            needed: pos + 4,
            available: data.len(),
        });
    }
    let bytes = [data[pos], data[pos + 1], data[pos + 2], data[pos + 3]];
    Ok(if le {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    })
}

fn read_f64(data: &[u8], pos: usize, le: bool) -> Result<f64, GpkgError> {
    if data.len() < pos + 8 {
        return Err(GpkgError::InsufficientData {
            needed: pos + 8,
            available: data.len(),
        });
    }
    let bytes = [
        data[pos],
        data[pos + 1],
        data[pos + 2],
        data[pos + 3],
        data[pos + 4],
        data[pos + 5],
        data[pos + 6],
        data[pos + 7],
    ];
    Ok(if le {
        f64::from_le_bytes(bytes)
    } else {
        f64::from_be_bytes(bytes)
    })
}

fn read_point_coords(data: &[u8], pos: usize, le: bool) -> Result<(f64, f64, usize), GpkgError> {
    let x = read_f64(data, pos, le)?;
    let y = read_f64(data, pos + 8, le)?;
    Ok((x, y, pos + 16))
}

fn read_coord_sequence(
    data: &[u8],
    pos: usize,
    le: bool,
) -> Result<(Vec<(f64, f64)>, usize), GpkgError> {
    let (n, mut cur) = read_u32_pos(data, pos, le)?;
    let mut coords = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let (x, y, new_cur) = read_point_coords(data, cur, le)?;
        cur = new_cur;
        coords.push((x, y));
    }
    Ok((coords, cur))
}

type RingsResult = Result<(Vec<Vec<(f64, f64)>>, usize), GpkgError>;

fn read_rings(data: &[u8], pos: usize, le: bool) -> RingsResult {
    let (n_rings, mut cur) = read_u32_pos(data, pos, le)?;
    let mut rings = Vec::with_capacity(n_rings as usize);
    for _ in 0..n_rings {
        let (coords, new_cur) = read_coord_sequence(data, cur, le)?;
        cur = new_cur;
        rings.push(coords);
    }
    Ok((rings, cur))
}

/// Write a little-endian WKB representation of `geom` into `buf`.
fn write_wkb(geom: &GpkgGeometry, buf: &mut Vec<u8>) {
    buf.push(1); // LE
    match geom {
        GpkgGeometry::Point { x, y } => {
            buf.extend_from_slice(&WKB_POINT.to_le_bytes());
            buf.extend_from_slice(&x.to_le_bytes());
            buf.extend_from_slice(&y.to_le_bytes());
        }
        GpkgGeometry::LineString { coords } => {
            buf.extend_from_slice(&WKB_LINESTRING.to_le_bytes());
            buf.extend_from_slice(&(coords.len() as u32).to_le_bytes());
            for (x, y) in coords {
                buf.extend_from_slice(&x.to_le_bytes());
                buf.extend_from_slice(&y.to_le_bytes());
            }
        }
        GpkgGeometry::Polygon { rings } => {
            buf.extend_from_slice(&WKB_POLYGON.to_le_bytes());
            buf.extend_from_slice(&(rings.len() as u32).to_le_bytes());
            for ring in rings {
                buf.extend_from_slice(&(ring.len() as u32).to_le_bytes());
                for (x, y) in ring {
                    buf.extend_from_slice(&x.to_le_bytes());
                    buf.extend_from_slice(&y.to_le_bytes());
                }
            }
        }
        GpkgGeometry::MultiPoint { points } => {
            buf.extend_from_slice(&WKB_MULTIPOINT.to_le_bytes());
            buf.extend_from_slice(&(points.len() as u32).to_le_bytes());
            for (x, y) in points {
                write_wkb(&GpkgGeometry::Point { x: *x, y: *y }, buf);
            }
        }
        GpkgGeometry::MultiLineString { lines } => {
            buf.extend_from_slice(&WKB_MULTILINESTRING.to_le_bytes());
            buf.extend_from_slice(&(lines.len() as u32).to_le_bytes());
            for line in lines {
                write_wkb(
                    &GpkgGeometry::LineString {
                        coords: line.clone(),
                    },
                    buf,
                );
            }
        }
        GpkgGeometry::MultiPolygon { polygons } => {
            buf.extend_from_slice(&WKB_MULTIPOLYGON.to_le_bytes());
            buf.extend_from_slice(&(polygons.len() as u32).to_le_bytes());
            for poly in polygons {
                write_wkb(
                    &GpkgGeometry::Polygon {
                        rings: poly.clone(),
                    },
                    buf,
                );
            }
        }
        GpkgGeometry::GeometryCollection(geoms) => {
            buf.extend_from_slice(&WKB_GEOMETRYCOLLECTION.to_le_bytes());
            buf.extend_from_slice(&(geoms.len() as u32).to_le_bytes());
            for g in geoms {
                write_wkb(g, buf);
            }
        }
        GpkgGeometry::Empty => {
            // Encode as an empty GeometryCollection
            buf.extend_from_slice(&WKB_GEOMETRYCOLLECTION.to_le_bytes());
            buf.extend_from_slice(&0u32.to_le_bytes());
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FeatureRow
// ─────────────────────────────────────────────────────────────────────────────

/// A single feature (row) read from a GeoPackage feature table.
#[derive(Debug, Clone)]
pub struct FeatureRow {
    /// Feature identifier (primary key value).
    pub fid: i64,
    /// Decoded geometry, or `None` when the geometry column is NULL.
    pub geometry: Option<GpkgGeometry>,
    /// Non-geometry attribute values, keyed by column name.
    pub fields: HashMap<String, FieldValue>,
}

impl FeatureRow {
    /// Look up a field by name.
    pub fn get_field(&self, name: &str) -> Option<&FieldValue> {
        self.fields.get(name)
    }

    /// Convenience: return the integer value of a field, or `None`.
    pub fn get_integer(&self, name: &str) -> Option<i64> {
        self.fields.get(name)?.as_integer()
    }

    /// Convenience: return the real value of a field, or `None`.
    pub fn get_real(&self, name: &str) -> Option<f64> {
        self.fields.get(name)?.as_real()
    }

    /// Convenience: return the text value of a field, or `None`.
    pub fn get_text(&self, name: &str) -> Option<&str> {
        self.fields.get(name)?.as_text()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FeatureTable
// ─────────────────────────────────────────────────────────────────────────────

/// An in-memory representation of a GeoPackage feature table.
///
/// Holds the table schema and all feature rows that have been loaded.
#[derive(Debug, Clone)]
pub struct FeatureTable {
    /// Name of the feature table (matches `gpkg_contents.table_name`).
    pub name: String,
    /// Name of the geometry column.
    pub geometry_column: String,
    /// Spatial reference system ID, or `None` when unknown.
    pub srs_id: Option<i32>,
    /// Column definitions (excludes the geometry column and FID).
    pub schema: Vec<FieldDefinition>,
    /// Loaded feature rows.
    pub features: Vec<FeatureRow>,
}

impl FeatureTable {
    /// Create a new, empty feature table with the given name and geometry column.
    pub fn new(name: impl Into<String>, geometry_column: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            geometry_column: geometry_column.into(),
            srs_id: None,
            schema: Vec::new(),
            features: Vec::new(),
        }
    }

    /// Return the number of loaded feature rows.
    pub fn feature_count(&self) -> usize {
        self.features.len()
    }

    /// Append a feature row to the table.
    pub fn add_feature(&mut self, row: FeatureRow) {
        self.features.push(row);
    }

    /// Find a feature by its FID, or return `None`.
    pub fn get_feature(&self, fid: i64) -> Option<&FeatureRow> {
        self.features.iter().find(|r| r.fid == fid)
    }

    /// Return the union bounding box of all feature geometries, or `None` when
    /// there are no geometries.
    pub fn bbox(&self) -> Option<(f64, f64, f64, f64)> {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut found = false;

        for row in &self.features {
            if let Some(geom) = &row.geometry {
                if let Some((gx0, gy0, gx1, gy1)) = geom.bbox() {
                    found = true;
                    if gx0 < min_x {
                        min_x = gx0;
                    }
                    if gy0 < min_y {
                        min_y = gy0;
                    }
                    if gx1 > max_x {
                        max_x = gx1;
                    }
                    if gy1 > max_y {
                        max_y = gy1;
                    }
                }
            }
        }

        if found {
            Some((min_x, min_y, max_x, max_y))
        } else {
            None
        }
    }

    /// Return all features whose geometry bounding box intersects the query bbox.
    ///
    /// Features with `None` geometry are excluded.  The check is a simple AABB
    /// intersection test (not precise polygon intersection).
    pub fn features_in_bbox(
        &self,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    ) -> Vec<&FeatureRow> {
        self.features
            .iter()
            .filter(|row| {
                if let Some(geom) = &row.geometry {
                    if let Some((gx0, gy0, gx1, gy1)) = geom.bbox() {
                        // AABB intersects when not separated on either axis
                        return gx0 <= max_x && gx1 >= min_x && gy0 <= max_y && gy1 >= min_y;
                    }
                }
                false
            })
            .collect()
    }

    /// Collect all distinct (non-Null) values for a named field across all features.
    ///
    /// The returned vec is deduplicated by equality.
    pub fn distinct_values(&self, field_name: &str) -> Vec<FieldValue> {
        let mut seen: Vec<FieldValue> = Vec::new();
        for row in &self.features {
            if let Some(val) = row.fields.get(field_name) {
                if !val.is_null() && !seen.contains(val) {
                    seen.push(val.clone());
                }
            }
        }
        seen
    }

    /// Serialise the feature table as a GeoJSON FeatureCollection string.
    ///
    /// Geometry `None` is encoded as `"geometry":null`.
    pub fn to_geojson(&self) -> String {
        let features_json: String = self
            .features
            .iter()
            .map(|row| {
                let geom_json = match &row.geometry {
                    Some(g) => g.to_geojson_geometry(),
                    None => "null".into(),
                };
                let props_json = build_properties_json(&row.fields);
                format!(r#"{{"type":"Feature","geometry":{geom_json},"properties":{props_json}}}"#)
            })
            .collect::<Vec<_>>()
            .join(",");

        format!(r#"{{"type":"FeatureCollection","features":[{features_json}]}}"#)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SrsInfo
// ─────────────────────────────────────────────────────────────────────────────

/// Spatial reference system metadata (from `gpkg_spatial_ref_sys`).
#[derive(Debug, Clone, PartialEq)]
pub struct SrsInfo {
    /// Human-readable name for this SRS.
    pub srs_name: String,
    /// Numeric SRS identifier (primary key in `gpkg_spatial_ref_sys`).
    pub srs_id: i32,
    /// Defining organisation (e.g. `"EPSG"`).
    pub organization: String,
    /// Organisation-assigned CRS code.
    pub org_coord_sys_id: i32,
    /// WKT or PROJ definition of the SRS.
    pub definition: String,
    /// Optional free-text description.
    pub description: Option<String>,
}

impl SrsInfo {
    /// Return the standard WGS 84 geographic SRS (EPSG:4326).
    pub fn wgs84() -> Self {
        Self {
            srs_name: "WGS 84".into(),
            srs_id: 4326,
            organization: "EPSG".into(),
            org_coord_sys_id: 4326,
            definition: concat!(
                "GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",",
                "SPHEROID[\"WGS 84\",6378137,298.257223563]],",
                "PRIMEM[\"Greenwich\",0],",
                "UNIT[\"degree\",0.0174532925199433]]"
            )
            .into(),
            description: Some("World Geodetic System 1984".into()),
        }
    }

    /// Return the Web Mercator (Pseudo-Mercator) projected SRS (EPSG:3857).
    pub fn web_mercator() -> Self {
        Self {
            srs_name: "WGS 84 / Pseudo-Mercator".into(),
            srs_id: 3857,
            organization: "EPSG".into(),
            org_coord_sys_id: 3857,
            definition: concat!(
                "PROJCS[\"WGS 84 / Pseudo-Mercator\",",
                "GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",",
                "SPHEROID[\"WGS 84\",6378137,298.257223563]],",
                "PRIMEM[\"Greenwich\",0],",
                "UNIT[\"degree\",0.0174532925199433]],",
                "PROJECTION[\"Mercator_1SP\"],",
                "PARAMETER[\"central_meridian\",0],",
                "PARAMETER[\"scale_factor\",1],",
                "PARAMETER[\"false_easting\",0],",
                "PARAMETER[\"false_northing\",0],",
                "UNIT[\"metre\",1]]"
            )
            .into(),
            description: Some("Web Mercator projection used by many web mapping services".into()),
        }
    }

    /// Return `true` when this SRS uses geographic (lat/lon) coordinates.
    ///
    /// Heuristic: considers `srs_id` values in the range 4000–4999 as geographic.
    pub fn is_geographic(&self) -> bool {
        (4000..5000).contains(&self.srs_id)
    }

    /// Return the EPSG code when the defining organisation is `"EPSG"`.
    pub fn epsg_code(&self) -> Option<i32> {
        if self.organization.eq_ignore_ascii_case("EPSG") {
            Some(self.org_coord_sys_id)
        } else {
            None
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON helper utilities
// ─────────────────────────────────────────────────────────────────────────────

/// Escape a string for use as a JSON string value (including the surrounding quotes).
fn json_string_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render a coordinate sequence as a JSON array of `[x,y]` arrays.
fn coords_to_json_array(coords: &[(f64, f64)]) -> String {
    let inner: String = coords
        .iter()
        .map(|(x, y)| format!("[{x},{y}]"))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{inner}]")
}

/// Render a `HashMap<String, FieldValue>` as a JSON object.
fn build_properties_json(fields: &HashMap<String, FieldValue>) -> String {
    if fields.is_empty() {
        return "{}".into();
    }
    // Sort keys for deterministic output
    let mut pairs: Vec<(&String, &FieldValue)> = fields.iter().collect();
    pairs.sort_by_key(|(k, _)| k.as_str());
    let members: String = pairs
        .iter()
        .map(|(k, v)| format!("{}:{}", json_string_escape(k), v.to_json()))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{members}}}")
}
