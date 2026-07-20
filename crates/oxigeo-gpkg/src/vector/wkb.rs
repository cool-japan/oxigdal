//! WKB (Well-Known Binary) constants, parser, and encoder for GeoPackage geometry.

use super::types::{CoordDim, GpkgGeometry, Point4D};
use crate::error::GpkgError;

// ─────────────────────────────────────────────────────────────────────────────
// WKB type constants — 2D (OGC simple)
// ─────────────────────────────────────────────────────────────────────────────

const WKB_POINT: u32 = 1;
const WKB_LINESTRING: u32 = 2;
const WKB_POLYGON: u32 = 3;
const WKB_MULTIPOINT: u32 = 4;
const WKB_MULTILINESTRING: u32 = 5;
const WKB_MULTIPOLYGON: u32 = 6;
const WKB_GEOMETRYCOLLECTION: u32 = 7;

// ─────────────────────────────────────────────────────────────────────────────
// WKB type constants — 3D / Z variants (ISO SQL-MM / OGC extended)
// ─────────────────────────────────────────────────────────────────────────────

const WKB_POINT_Z: u32 = 1001;
const WKB_LINESTRING_Z: u32 = 1002;
const WKB_POLYGON_Z: u32 = 1003;
const WKB_MULTIPOINT_Z: u32 = 1004;
const WKB_MULTILINESTRING_Z: u32 = 1005;
const WKB_MULTIPOLYGON_Z: u32 = 1006;
const WKB_GEOMETRYCOLLECTION_Z: u32 = 1007;

// ─────────────────────────────────────────────────────────────────────────────
// WKB type constants — XYM variants (ISO SQL/MM: base + 2000)
// ─────────────────────────────────────────────────────────────────────────────

/// WKB XYM Point type code (ISO SQL/MM).
///
/// These encode measure (M) coordinates in WKB streams. Writing uses these ISO
/// forms; reading also accepts the EWKB `0x4000_0000` high-bit convention.
pub const WKB_POINT_M: u32 = 2001;
/// WKB XYM LineString type code (ISO SQL/MM).
pub const WKB_LINESTRING_M: u32 = 2002;
/// WKB XYM Polygon type code (ISO SQL/MM).
pub const WKB_POLYGON_M: u32 = 2003;
/// WKB XYM MultiPoint type code (ISO SQL/MM).
pub const WKB_MULTIPOINT_M: u32 = 2004;
/// WKB XYM MultiLineString type code (ISO SQL/MM).
pub const WKB_MULTILINESTRING_M: u32 = 2005;
/// WKB XYM MultiPolygon type code (ISO SQL/MM).
pub const WKB_MULTIPOLYGON_M: u32 = 2006;
/// WKB XYM GeometryCollection type code (ISO SQL/MM).
pub const WKB_GEOMETRYCOLLECTION_M: u32 = 2007;

// ─────────────────────────────────────────────────────────────────────────────
// WKB type constants — XYZM variants (ISO SQL/MM: base + 3000)
// ─────────────────────────────────────────────────────────────────────────────

/// WKB XYZM Point type code (ISO SQL/MM).
///
/// These encode both elevation (Z) and measure (M) coordinates in WKB streams.
/// Writing uses these ISO forms; reading also accepts the EWKB `0xC000_0000`
/// combined high-bit convention.
pub const WKB_POINT_ZM: u32 = 3001;
/// WKB XYZM LineString type code (ISO SQL/MM).
pub const WKB_LINESTRING_ZM: u32 = 3002;
/// WKB XYZM Polygon type code (ISO SQL/MM).
pub const WKB_POLYGON_ZM: u32 = 3003;
/// WKB XYZM MultiPoint type code (ISO SQL/MM).
pub const WKB_MULTIPOINT_ZM: u32 = 3004;
/// WKB XYZM MultiLineString type code (ISO SQL/MM).
pub const WKB_MULTILINESTRING_ZM: u32 = 3005;
/// WKB XYZM MultiPolygon type code (ISO SQL/MM).
pub const WKB_MULTIPOLYGON_ZM: u32 = 3006;
/// WKB XYZM GeometryCollection type code (ISO SQL/MM).
pub const WKB_GEOMETRYCOLLECTION_ZM: u32 = 3007;

/// EWKB M high-bit flag (PostgreSQL/PostGIS extension).
///
/// When bit 30 is set and the base type is in `1..=7`, the geometry carries
/// M (measure) coordinates.  Reading accepts this convention in addition to
/// the ISO `2000 + base` form.
pub const WKB_EWKB_M_HIGHBIT: u32 = 0x4000_0000;

/// EWKB ZM combined high-bit flags (Z bit 31 | M bit 30 = `0xC000_0000`).
///
/// When both bits 31 and 30 are set and the base type is in `1..=7`, the
/// geometry carries both Z (elevation) and M (measure) coordinates.
pub const WKB_EWKB_ZM_HIGHBIT: u32 = 0xC000_0000;

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

        let _srs_id: i32 = if is_little_endian {
            i32::from_le_bytes([data[4], data[5], data[6], data[7]])
        } else {
            i32::from_be_bytes([data[4], data[5], data[6], data[7]])
        };

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
    pub fn to_gpb(geom: &GpkgGeometry, srs_id: i32) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(0x47);
        buf.push(0x50);
        buf.push(0);
        let is_empty = matches!(geom, GpkgGeometry::Empty);
        let empty_bit: u8 = if is_empty { 1 << 3 } else { 0 };
        let flags: u8 = empty_bit | (1 << 5);
        buf.push(flags);
        buf.extend_from_slice(&srs_id.to_le_bytes());
        if !is_empty {
            write_wkb(geom, &mut buf);
        }
        buf
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// dim_from_wkb_type
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve the coordinate dimensionality and OGC base type from a raw WKB type integer.
///
/// Handles three overlapping encoding conventions:
/// - **ISO SQL/MM integer offset**: type ≥ 1000 → Z; ≥ 2000 → M; ≥ 3000 → ZM.
/// - **ISO high-bit Z flag**: bit 31 set (`0x8000_0000`).
/// - **EWKB high bits**: bit 30 = M (`0x4000_0000`), bit 31 = Z (`0x8000_0000`).
///
/// Returns `(dim, base_type)` where `base_type` is in `1..=7`.
fn dim_from_wkb_type(raw_type: u32) -> (CoordDim, u32) {
    let has_z_bit = (raw_type & 0x8000_0000) != 0;
    let has_m_bit = (raw_type & 0x4000_0000) != 0;
    let stripped = raw_type & 0x0FFF_FFFF;

    let (iso_dim, base) = if stripped >= 3000 {
        (CoordDim::XYZM, stripped - 3000)
    } else if stripped >= 2000 {
        (CoordDim::XYM, stripped - 2000)
    } else if stripped >= 1000 {
        (CoordDim::XYZ, stripped - 1000)
    } else {
        (CoordDim::XY, stripped)
    };

    let dim = if base <= 7 && iso_dim == CoordDim::XY {
        match (has_z_bit, has_m_bit) {
            (true, true) => CoordDim::XYZM,
            (true, false) => CoordDim::XYZ,
            (false, true) => CoordDim::XYM,
            (false, false) => CoordDim::XY,
        }
    } else {
        iso_dim
    };

    (dim, base)
}

// ─────────────────────────────────────────────────────────────────────────────
// parse_wkb_inner
// ─────────────────────────────────────────────────────────────────────────────

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

    let raw_type = read_u32(data, pos, le)?;
    pos += 4;

    let (dim, base_type) = dim_from_wkb_type(raw_type);

    if base_type == 0 || base_type > 7 {
        return Err(GpkgError::UnknownWkbType(raw_type));
    }

    match (base_type, dim) {
        // ── 2D (XY) variants ─────────────────────────────────────────────
        (WKB_POINT, CoordDim::XY) => {
            let (x, y, new_pos) = read_point_coords(data, pos, le)?;
            Ok((GpkgGeometry::Point { x, y }, new_pos))
        }
        (WKB_LINESTRING, CoordDim::XY) => {
            let (coords, new_pos) = read_coord_sequence(data, pos, le)?;
            Ok((GpkgGeometry::LineString { coords }, new_pos))
        }
        (WKB_POLYGON, CoordDim::XY) => {
            let (rings, new_pos) = read_rings(data, pos, le)?;
            Ok((GpkgGeometry::Polygon { rings }, new_pos))
        }
        (WKB_MULTIPOINT, CoordDim::XY) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut points = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::Point { x, y } => points.push((x, y)),
                    GpkgGeometry::PointZ { x, y, .. } | GpkgGeometry::PointM { x, y, .. } => {
                        points.push((x, y))
                    }
                    GpkgGeometry::PointZM(p) => points.push((p.x, p.y)),
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
        (WKB_MULTILINESTRING, CoordDim::XY) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut lines = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::LineString { coords } => lines.push(coords),
                    GpkgGeometry::LineStringZ { coords } => {
                        lines.push(coords.into_iter().map(|(x, y, _)| (x, y)).collect())
                    }
                    GpkgGeometry::LineStringM { coords } => {
                        lines.push(coords.into_iter().map(|(x, y, _)| (x, y)).collect())
                    }
                    GpkgGeometry::LineStringZM { coords } => {
                        lines.push(coords.into_iter().map(|p| (p.x, p.y)).collect())
                    }
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
        (WKB_MULTIPOLYGON, CoordDim::XY) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut polygons = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::Polygon { rings } => polygons.push(rings),
                    GpkgGeometry::PolygonZ { rings } => polygons.push(
                        rings
                            .into_iter()
                            .map(|ring| ring.into_iter().map(|(x, y, _)| (x, y)).collect())
                            .collect(),
                    ),
                    GpkgGeometry::PolygonM { rings } => polygons.push(
                        rings
                            .into_iter()
                            .map(|ring| ring.into_iter().map(|(x, y, _)| (x, y)).collect())
                            .collect(),
                    ),
                    GpkgGeometry::PolygonZM { rings } => polygons.push(
                        rings
                            .into_iter()
                            .map(|ring| ring.into_iter().map(|p| (p.x, p.y)).collect())
                            .collect(),
                    ),
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
        (WKB_GEOMETRYCOLLECTION, CoordDim::XY) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut geoms = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                geoms.push(sub);
            }
            Ok((GpkgGeometry::GeometryCollection(geoms), pos2))
        }
        // ── 3D / Z variants ───────────────────────────────────────────────
        (WKB_POINT, CoordDim::XYZ) => {
            let (x, y, z, new_pos) = read_point_z_coords(data, pos, le)?;
            Ok((GpkgGeometry::PointZ { x, y, z }, new_pos))
        }
        (WKB_LINESTRING, CoordDim::XYZ) => {
            let (coords, new_pos) = read_coord_z_sequence(data, pos, le)?;
            Ok((GpkgGeometry::LineStringZ { coords }, new_pos))
        }
        (WKB_POLYGON, CoordDim::XYZ) => {
            let (rings, new_pos) = read_rings_z(data, pos, le)?;
            Ok((GpkgGeometry::PolygonZ { rings }, new_pos))
        }
        (WKB_MULTIPOINT, CoordDim::XYZ) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut points = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::PointZ { x, y, z } => points.push((x, y, z)),
                    GpkgGeometry::Point { x, y } => points.push((x, y, 0.0)),
                    other => {
                        return Err(GpkgError::WkbParseError(format!(
                            "Expected PointZ in MultiPointZ, got {}",
                            other.geometry_type()
                        )));
                    }
                }
            }
            Ok((GpkgGeometry::MultiPointZ { points }, pos2))
        }
        (WKB_MULTILINESTRING, CoordDim::XYZ) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut lines = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::LineStringZ { coords } => lines.push(coords),
                    GpkgGeometry::LineString { coords } => {
                        lines.push(coords.into_iter().map(|(x, y)| (x, y, 0.0)).collect())
                    }
                    other => {
                        return Err(GpkgError::WkbParseError(format!(
                            "Expected LineStringZ in MultiLineStringZ, got {}",
                            other.geometry_type()
                        )));
                    }
                }
            }
            Ok((GpkgGeometry::MultiLineStringZ { lines }, pos2))
        }
        (WKB_MULTIPOLYGON, CoordDim::XYZ) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut polygons = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::PolygonZ { rings } => polygons.push(rings),
                    GpkgGeometry::Polygon { rings } => polygons.push(
                        rings
                            .into_iter()
                            .map(|ring| ring.into_iter().map(|(x, y)| (x, y, 0.0)).collect())
                            .collect(),
                    ),
                    other => {
                        return Err(GpkgError::WkbParseError(format!(
                            "Expected PolygonZ in MultiPolygonZ, got {}",
                            other.geometry_type()
                        )));
                    }
                }
            }
            Ok((GpkgGeometry::MultiPolygonZ { polygons }, pos2))
        }
        (WKB_GEOMETRYCOLLECTION, CoordDim::XYZ) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut geoms = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                geoms.push(sub);
            }
            Ok((GpkgGeometry::GeometryCollectionZ(geoms), pos2))
        }
        // ── XYM variants ──────────────────────────────────────────────────
        (WKB_POINT, CoordDim::XYM) => {
            let (x, y, m, new_pos) = read_point_z_coords(data, pos, le)?;
            Ok((GpkgGeometry::PointM { x, y, m }, new_pos))
        }
        (WKB_LINESTRING, CoordDim::XYM) => {
            let (coords, new_pos) = read_coord_z_sequence(data, pos, le)?;
            Ok((GpkgGeometry::LineStringM { coords }, new_pos))
        }
        (WKB_POLYGON, CoordDim::XYM) => {
            let (rings, new_pos) = read_rings_z(data, pos, le)?;
            Ok((GpkgGeometry::PolygonM { rings }, new_pos))
        }
        (WKB_MULTIPOINT, CoordDim::XYM) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut points = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::PointM { x, y, m } => points.push((x, y, m)),
                    GpkgGeometry::Point { x, y } => points.push((x, y, 0.0)),
                    other => {
                        return Err(GpkgError::WkbParseError(format!(
                            "Expected PointM in MultiPointM, got {}",
                            other.geometry_type()
                        )));
                    }
                }
            }
            Ok((GpkgGeometry::MultiPointM { points }, pos2))
        }
        (WKB_MULTILINESTRING, CoordDim::XYM) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut lines = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::LineStringM { coords } => lines.push(coords),
                    GpkgGeometry::LineString { coords } => {
                        lines.push(coords.into_iter().map(|(x, y)| (x, y, 0.0)).collect())
                    }
                    other => {
                        return Err(GpkgError::WkbParseError(format!(
                            "Expected LineStringM in MultiLineStringM, got {}",
                            other.geometry_type()
                        )));
                    }
                }
            }
            Ok((GpkgGeometry::MultiLineStringM { lines }, pos2))
        }
        (WKB_MULTIPOLYGON, CoordDim::XYM) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut polygons = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::PolygonM { rings } => polygons.push(rings),
                    GpkgGeometry::Polygon { rings } => polygons.push(
                        rings
                            .into_iter()
                            .map(|ring| ring.into_iter().map(|(x, y)| (x, y, 0.0)).collect())
                            .collect(),
                    ),
                    other => {
                        return Err(GpkgError::WkbParseError(format!(
                            "Expected PolygonM in MultiPolygonM, got {}",
                            other.geometry_type()
                        )));
                    }
                }
            }
            Ok((GpkgGeometry::MultiPolygonM { polygons }, pos2))
        }
        (WKB_GEOMETRYCOLLECTION, CoordDim::XYM) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut geoms = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                geoms.push(sub);
            }
            Ok((GpkgGeometry::GeometryCollectionM(geoms), pos2))
        }
        // ── XYZM variants ─────────────────────────────────────────────────
        (WKB_POINT, CoordDim::XYZM) => {
            let (x, y, z, new_pos) = read_point_z_coords(data, pos, le)?;
            let m = read_f64(data, new_pos, le)?;
            Ok((
                GpkgGeometry::PointZM(Point4D {
                    x,
                    y,
                    z: Some(z),
                    m: Some(m),
                }),
                new_pos + 8,
            ))
        }
        (WKB_LINESTRING, CoordDim::XYZM) => {
            let (coords, new_pos) = read_coord_zm_sequence(data, pos, le)?;
            Ok((GpkgGeometry::LineStringZM { coords }, new_pos))
        }
        (WKB_POLYGON, CoordDim::XYZM) => {
            let (rings, new_pos) = read_rings_zm(data, pos, le)?;
            Ok((GpkgGeometry::PolygonZM { rings }, new_pos))
        }
        (WKB_MULTIPOINT, CoordDim::XYZM) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut points = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::PointZM(p) => points.push(p),
                    GpkgGeometry::Point { x, y } => points.push(Point4D {
                        x,
                        y,
                        z: None,
                        m: None,
                    }),
                    other => {
                        return Err(GpkgError::WkbParseError(format!(
                            "Expected PointZM in MultiPointZM, got {}",
                            other.geometry_type()
                        )));
                    }
                }
            }
            Ok((GpkgGeometry::MultiPointZM { points }, pos2))
        }
        (WKB_MULTILINESTRING, CoordDim::XYZM) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut lines = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::LineStringZM { coords } => lines.push(coords),
                    GpkgGeometry::LineString { coords } => lines.push(
                        coords
                            .into_iter()
                            .map(|(x, y)| Point4D {
                                x,
                                y,
                                z: None,
                                m: None,
                            })
                            .collect(),
                    ),
                    other => {
                        return Err(GpkgError::WkbParseError(format!(
                            "Expected LineStringZM in MultiLineStringZM, got {}",
                            other.geometry_type()
                        )));
                    }
                }
            }
            Ok((GpkgGeometry::MultiLineStringZM { lines }, pos2))
        }
        (WKB_MULTIPOLYGON, CoordDim::XYZM) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut polygons = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                match sub {
                    GpkgGeometry::PolygonZM { rings } => polygons.push(rings),
                    GpkgGeometry::Polygon { rings } => polygons.push(
                        rings
                            .into_iter()
                            .map(|ring| {
                                ring.into_iter()
                                    .map(|(x, y)| Point4D {
                                        x,
                                        y,
                                        z: None,
                                        m: None,
                                    })
                                    .collect()
                            })
                            .collect(),
                    ),
                    other => {
                        return Err(GpkgError::WkbParseError(format!(
                            "Expected PolygonZM in MultiPolygonZM, got {}",
                            other.geometry_type()
                        )));
                    }
                }
            }
            Ok((GpkgGeometry::MultiPolygonZM { polygons }, pos2))
        }
        (WKB_GEOMETRYCOLLECTION, CoordDim::XYZM) => {
            let (n, mut pos2) = read_u32_pos(data, pos, le)?;
            let mut geoms = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let (sub, new_pos) = parse_wkb_inner(data, pos2)?;
                pos2 = new_pos;
                geoms.push(sub);
            }
            Ok((GpkgGeometry::GeometryCollectionZM(geoms), pos2))
        }
        _ => Err(GpkgError::UnknownWkbType(raw_type)),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Low-level binary readers
// ─────────────────────────────────────────────────────────────────────────────

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

fn read_point_z_coords(
    data: &[u8],
    pos: usize,
    le: bool,
) -> Result<(f64, f64, f64, usize), GpkgError> {
    let x = read_f64(data, pos, le)?;
    let y = read_f64(data, pos + 8, le)?;
    let z = read_f64(data, pos + 16, le)?;
    Ok((x, y, z, pos + 24))
}

type CoordZSequenceResult = Result<(Vec<(f64, f64, f64)>, usize), GpkgError>;

fn read_coord_z_sequence(data: &[u8], pos: usize, le: bool) -> CoordZSequenceResult {
    let (n, mut cur) = read_u32_pos(data, pos, le)?;
    let mut coords = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let (x, y, z, new_cur) = read_point_z_coords(data, cur, le)?;
        cur = new_cur;
        coords.push((x, y, z));
    }
    Ok((coords, cur))
}

type RingsZResult = Result<(Vec<Vec<(f64, f64, f64)>>, usize), GpkgError>;

fn read_rings_z(data: &[u8], pos: usize, le: bool) -> RingsZResult {
    let (n_rings, mut cur) = read_u32_pos(data, pos, le)?;
    let mut rings = Vec::with_capacity(n_rings as usize);
    for _ in 0..n_rings {
        let (coords, new_cur) = read_coord_z_sequence(data, cur, le)?;
        cur = new_cur;
        rings.push(coords);
    }
    Ok((rings, cur))
}

type CoordZmSequenceResult = Result<(Vec<Point4D>, usize), GpkgError>;

fn read_coord_zm_sequence(data: &[u8], pos: usize, le: bool) -> CoordZmSequenceResult {
    let (n, mut cur) = read_u32_pos(data, pos, le)?;
    let mut coords = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let (x, y, z, after_xyz) = read_point_z_coords(data, cur, le)?;
        let m = read_f64(data, after_xyz, le)?;
        cur = after_xyz + 8;
        coords.push(Point4D {
            x,
            y,
            z: Some(z),
            m: Some(m),
        });
    }
    Ok((coords, cur))
}

type RingsZmResult = Result<(Vec<Vec<Point4D>>, usize), GpkgError>;

fn read_rings_zm(data: &[u8], pos: usize, le: bool) -> RingsZmResult {
    let (n_rings, mut cur) = read_u32_pos(data, pos, le)?;
    let mut rings = Vec::with_capacity(n_rings as usize);
    for _ in 0..n_rings {
        let (coords, new_cur) = read_coord_zm_sequence(data, cur, le)?;
        cur = new_cur;
        rings.push(coords);
    }
    Ok((rings, cur))
}

// ─────────────────────────────────────────────────────────────────────────────
// write_wkb
// ─────────────────────────────────────────────────────────────────────────────

/// Write a little-endian WKB representation of `geom` into `buf`.
pub(super) fn write_wkb(geom: &GpkgGeometry, buf: &mut Vec<u8>) {
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
        GpkgGeometry::PointZ { x, y, z } => {
            buf.extend_from_slice(&WKB_POINT_Z.to_le_bytes());
            buf.extend_from_slice(&x.to_le_bytes());
            buf.extend_from_slice(&y.to_le_bytes());
            buf.extend_from_slice(&z.to_le_bytes());
        }
        GpkgGeometry::LineStringZ { coords } => {
            buf.extend_from_slice(&WKB_LINESTRING_Z.to_le_bytes());
            buf.extend_from_slice(&(coords.len() as u32).to_le_bytes());
            for (x, y, z) in coords {
                buf.extend_from_slice(&x.to_le_bytes());
                buf.extend_from_slice(&y.to_le_bytes());
                buf.extend_from_slice(&z.to_le_bytes());
            }
        }
        GpkgGeometry::PolygonZ { rings } => {
            buf.extend_from_slice(&WKB_POLYGON_Z.to_le_bytes());
            buf.extend_from_slice(&(rings.len() as u32).to_le_bytes());
            for ring in rings {
                buf.extend_from_slice(&(ring.len() as u32).to_le_bytes());
                for (x, y, z) in ring {
                    buf.extend_from_slice(&x.to_le_bytes());
                    buf.extend_from_slice(&y.to_le_bytes());
                    buf.extend_from_slice(&z.to_le_bytes());
                }
            }
        }
        GpkgGeometry::MultiPointZ { points } => {
            buf.extend_from_slice(&WKB_MULTIPOINT_Z.to_le_bytes());
            buf.extend_from_slice(&(points.len() as u32).to_le_bytes());
            for (x, y, z) in points {
                write_wkb(
                    &GpkgGeometry::PointZ {
                        x: *x,
                        y: *y,
                        z: *z,
                    },
                    buf,
                );
            }
        }
        GpkgGeometry::MultiLineStringZ { lines } => {
            buf.extend_from_slice(&WKB_MULTILINESTRING_Z.to_le_bytes());
            buf.extend_from_slice(&(lines.len() as u32).to_le_bytes());
            for line in lines {
                write_wkb(
                    &GpkgGeometry::LineStringZ {
                        coords: line.clone(),
                    },
                    buf,
                );
            }
        }
        GpkgGeometry::MultiPolygonZ { polygons } => {
            buf.extend_from_slice(&WKB_MULTIPOLYGON_Z.to_le_bytes());
            buf.extend_from_slice(&(polygons.len() as u32).to_le_bytes());
            for poly in polygons {
                write_wkb(
                    &GpkgGeometry::PolygonZ {
                        rings: poly.clone(),
                    },
                    buf,
                );
            }
        }
        GpkgGeometry::GeometryCollectionZ(geoms) => {
            buf.extend_from_slice(&WKB_GEOMETRYCOLLECTION_Z.to_le_bytes());
            buf.extend_from_slice(&(geoms.len() as u32).to_le_bytes());
            for g in geoms {
                write_wkb(g, buf);
            }
        }
        GpkgGeometry::PointM { x, y, m } => {
            buf.extend_from_slice(&WKB_POINT_M.to_le_bytes());
            buf.extend_from_slice(&x.to_le_bytes());
            buf.extend_from_slice(&y.to_le_bytes());
            buf.extend_from_slice(&m.to_le_bytes());
        }
        GpkgGeometry::LineStringM { coords } => {
            buf.extend_from_slice(&WKB_LINESTRING_M.to_le_bytes());
            buf.extend_from_slice(&(coords.len() as u32).to_le_bytes());
            for (x, y, m) in coords {
                buf.extend_from_slice(&x.to_le_bytes());
                buf.extend_from_slice(&y.to_le_bytes());
                buf.extend_from_slice(&m.to_le_bytes());
            }
        }
        GpkgGeometry::PolygonM { rings } => {
            buf.extend_from_slice(&WKB_POLYGON_M.to_le_bytes());
            buf.extend_from_slice(&(rings.len() as u32).to_le_bytes());
            for ring in rings {
                buf.extend_from_slice(&(ring.len() as u32).to_le_bytes());
                for (x, y, m) in ring {
                    buf.extend_from_slice(&x.to_le_bytes());
                    buf.extend_from_slice(&y.to_le_bytes());
                    buf.extend_from_slice(&m.to_le_bytes());
                }
            }
        }
        GpkgGeometry::MultiPointM { points } => {
            buf.extend_from_slice(&WKB_MULTIPOINT_M.to_le_bytes());
            buf.extend_from_slice(&(points.len() as u32).to_le_bytes());
            for (x, y, m) in points {
                write_wkb(
                    &GpkgGeometry::PointM {
                        x: *x,
                        y: *y,
                        m: *m,
                    },
                    buf,
                );
            }
        }
        GpkgGeometry::MultiLineStringM { lines } => {
            buf.extend_from_slice(&WKB_MULTILINESTRING_M.to_le_bytes());
            buf.extend_from_slice(&(lines.len() as u32).to_le_bytes());
            for line in lines {
                write_wkb(
                    &GpkgGeometry::LineStringM {
                        coords: line.clone(),
                    },
                    buf,
                );
            }
        }
        GpkgGeometry::MultiPolygonM { polygons } => {
            buf.extend_from_slice(&WKB_MULTIPOLYGON_M.to_le_bytes());
            buf.extend_from_slice(&(polygons.len() as u32).to_le_bytes());
            for poly in polygons {
                write_wkb(
                    &GpkgGeometry::PolygonM {
                        rings: poly.clone(),
                    },
                    buf,
                );
            }
        }
        GpkgGeometry::GeometryCollectionM(geoms) => {
            buf.extend_from_slice(&WKB_GEOMETRYCOLLECTION_M.to_le_bytes());
            buf.extend_from_slice(&(geoms.len() as u32).to_le_bytes());
            for g in geoms {
                write_wkb(g, buf);
            }
        }
        GpkgGeometry::PointZM(p) => {
            buf.extend_from_slice(&WKB_POINT_ZM.to_le_bytes());
            buf.extend_from_slice(&p.x.to_le_bytes());
            buf.extend_from_slice(&p.y.to_le_bytes());
            buf.extend_from_slice(&p.z.unwrap_or(0.0).to_le_bytes());
            buf.extend_from_slice(&p.m.unwrap_or(0.0).to_le_bytes());
        }
        GpkgGeometry::LineStringZM { coords } => {
            buf.extend_from_slice(&WKB_LINESTRING_ZM.to_le_bytes());
            buf.extend_from_slice(&(coords.len() as u32).to_le_bytes());
            for p in coords {
                buf.extend_from_slice(&p.x.to_le_bytes());
                buf.extend_from_slice(&p.y.to_le_bytes());
                buf.extend_from_slice(&p.z.unwrap_or(0.0).to_le_bytes());
                buf.extend_from_slice(&p.m.unwrap_or(0.0).to_le_bytes());
            }
        }
        GpkgGeometry::PolygonZM { rings } => {
            buf.extend_from_slice(&WKB_POLYGON_ZM.to_le_bytes());
            buf.extend_from_slice(&(rings.len() as u32).to_le_bytes());
            for ring in rings {
                buf.extend_from_slice(&(ring.len() as u32).to_le_bytes());
                for p in ring {
                    buf.extend_from_slice(&p.x.to_le_bytes());
                    buf.extend_from_slice(&p.y.to_le_bytes());
                    buf.extend_from_slice(&p.z.unwrap_or(0.0).to_le_bytes());
                    buf.extend_from_slice(&p.m.unwrap_or(0.0).to_le_bytes());
                }
            }
        }
        GpkgGeometry::MultiPointZM { points } => {
            buf.extend_from_slice(&WKB_MULTIPOINT_ZM.to_le_bytes());
            buf.extend_from_slice(&(points.len() as u32).to_le_bytes());
            for p in points {
                write_wkb(&GpkgGeometry::PointZM(p.clone()), buf);
            }
        }
        GpkgGeometry::MultiLineStringZM { lines } => {
            buf.extend_from_slice(&WKB_MULTILINESTRING_ZM.to_le_bytes());
            buf.extend_from_slice(&(lines.len() as u32).to_le_bytes());
            for line in lines {
                write_wkb(
                    &GpkgGeometry::LineStringZM {
                        coords: line.clone(),
                    },
                    buf,
                );
            }
        }
        GpkgGeometry::MultiPolygonZM { polygons } => {
            buf.extend_from_slice(&WKB_MULTIPOLYGON_ZM.to_le_bytes());
            buf.extend_from_slice(&(polygons.len() as u32).to_le_bytes());
            for poly in polygons {
                write_wkb(
                    &GpkgGeometry::PolygonZM {
                        rings: poly.clone(),
                    },
                    buf,
                );
            }
        }
        GpkgGeometry::GeometryCollectionZM(geoms) => {
            buf.extend_from_slice(&WKB_GEOMETRYCOLLECTION_ZM.to_le_bytes());
            buf.extend_from_slice(&(geoms.len() as u32).to_le_bytes());
            for g in geoms {
                write_wkb(g, buf);
            }
        }
        GpkgGeometry::Empty => {
            buf.extend_from_slice(&WKB_GEOMETRYCOLLECTION.to_le_bytes());
            buf.extend_from_slice(&0u32.to_le_bytes());
        }
    }
}
