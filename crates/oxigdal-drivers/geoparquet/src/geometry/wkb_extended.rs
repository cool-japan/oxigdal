//! Extended WKB encoding for all geometry types including nested structures.
//!
//! This module supplements `wkb.rs` with:
//! - Functional helper functions for each geometry type (no struct required).
//! - Full Z/M coordinate support.
//! - `MultiPolygon` with holes encoded in a single call.
//! - `GeometryCollection` wrapping raw WKB blobs.
//! - Bbox computation and column-level geometry statistics.
//!
//! All encoding is little-endian (NDR byte order = 0x01) which is the
//! conventional default for GeoParquet.

use byteorder::{LittleEndian, WriteBytesExt};
use std::io::Write;

use crate::error::{GeoParquetError, Result};

// ── WKB type codes ────────────────────────────────────────────────────────────

/// OGC WKB geometry type codes (ISO SQL/MM variant with Z/M encoding in the
/// 1000-range).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum WkbType {
    /// 2D Point
    Point = 1,
    /// 2D LineString
    LineString = 2,
    /// 2D Polygon
    Polygon = 3,
    /// 2D MultiPoint
    MultiPoint = 4,
    /// 2D MultiLineString
    MultiLineString = 5,
    /// 2D MultiPolygon
    MultiPolygon = 6,
    /// 2D GeometryCollection
    GeometryCollection = 7,
    // Z variants (ISO 19125)
    /// 3D Point (Z)
    PointZ = 1001,
    /// 3D LineString (Z)
    LineStringZ = 1002,
    /// 3D Polygon (Z)
    PolygonZ = 1003,
    /// 3D MultiPolygon (Z)
    MultiPolygonZ = 1006,
    // M variants
    /// Point with measure
    PointM = 2001,
    // ZM variants
    /// Point with Z and M
    PointZm = 3001,
}

impl WkbType {
    /// Returns the u32 code for this geometry type.
    #[must_use]
    pub const fn code(self) -> u32 {
        self as u32
    }
}

// ── Byte-order constant ───────────────────────────────────────────────────────

/// WKB NDR (little-endian) byte-order marker.
const WKB_LITTLE_ENDIAN: u8 = 0x01;

// ── Type aliases for complex multi-polygon inputs ─────────────────────────────

/// A 2D polygon represented as `(exterior_ring, holes)`.
pub type Polygon2d = (Vec<(f64, f64)>, Vec<Vec<(f64, f64)>>);

/// A 3D polygon represented as `(exterior_ring_z, holes_z)`.
pub type Polygon3d = (Vec<(f64, f64, f64)>, Vec<Vec<(f64, f64, f64)>>);

// ── Ring / coordinate helpers ─────────────────────────────────────────────────

/// Encodes a 2D polygon ring (list of `(x, y)` pairs) as WKB ring bytes.
///
/// A WKB ring is: `u32 num_points` followed by `num_points × (f64 x, f64 y)`.
///
/// # Errors
/// Returns an error if the buffer write fails (should not occur with `Vec`).
pub fn encode_ring_2d(ring: &[(f64, f64)]) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(4 + ring.len() * 16);
    buf.write_u32::<LittleEndian>(ring.len() as u32)
        .map_err(GeoParquetError::Io)?;
    for &(x, y) in ring {
        buf.write_f64::<LittleEndian>(x)
            .map_err(GeoParquetError::Io)?;
        buf.write_f64::<LittleEndian>(y)
            .map_err(GeoParquetError::Io)?;
    }
    Ok(buf)
}

/// Encodes a 3D polygon ring (list of `(x, y, z)` triples) as WKB ring bytes.
///
/// # Errors
/// Returns an error if the buffer write fails.
pub fn encode_ring_3d(ring: &[(f64, f64, f64)]) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(4 + ring.len() * 24);
    buf.write_u32::<LittleEndian>(ring.len() as u32)
        .map_err(GeoParquetError::Io)?;
    for &(x, y, z) in ring {
        buf.write_f64::<LittleEndian>(x)
            .map_err(GeoParquetError::Io)?;
        buf.write_f64::<LittleEndian>(y)
            .map_err(GeoParquetError::Io)?;
        buf.write_f64::<LittleEndian>(z)
            .map_err(GeoParquetError::Io)?;
    }
    Ok(buf)
}

// ── Point encoders ────────────────────────────────────────────────────────────

/// Encodes a 2D point `(x, y)` to WKB.
///
/// # Errors
/// Returns an error if the buffer write fails.
pub fn encode_point_2d(x: f64, y: f64) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(21);
    buf.write_u8(WKB_LITTLE_ENDIAN)
        .map_err(GeoParquetError::Io)?;
    buf.write_u32::<LittleEndian>(WkbType::Point.code())
        .map_err(GeoParquetError::Io)?;
    buf.write_f64::<LittleEndian>(x)
        .map_err(GeoParquetError::Io)?;
    buf.write_f64::<LittleEndian>(y)
        .map_err(GeoParquetError::Io)?;
    Ok(buf)
}

/// Encodes a 3D point `(x, y, z)` to WKB using ISO type code 1001.
///
/// # Errors
/// Returns an error if the buffer write fails.
pub fn encode_point_z(x: f64, y: f64, z: f64) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(29);
    buf.write_u8(WKB_LITTLE_ENDIAN)
        .map_err(GeoParquetError::Io)?;
    buf.write_u32::<LittleEndian>(WkbType::PointZ.code())
        .map_err(GeoParquetError::Io)?;
    buf.write_f64::<LittleEndian>(x)
        .map_err(GeoParquetError::Io)?;
    buf.write_f64::<LittleEndian>(y)
        .map_err(GeoParquetError::Io)?;
    buf.write_f64::<LittleEndian>(z)
        .map_err(GeoParquetError::Io)?;
    Ok(buf)
}

/// Encodes a 2D+M point `(x, y, m)` to WKB using ISO type code 2001.
///
/// # Errors
/// Returns an error if the buffer write fails.
pub fn encode_point_m(x: f64, y: f64, m: f64) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(29);
    buf.write_u8(WKB_LITTLE_ENDIAN)
        .map_err(GeoParquetError::Io)?;
    buf.write_u32::<LittleEndian>(WkbType::PointM.code())
        .map_err(GeoParquetError::Io)?;
    buf.write_f64::<LittleEndian>(x)
        .map_err(GeoParquetError::Io)?;
    buf.write_f64::<LittleEndian>(y)
        .map_err(GeoParquetError::Io)?;
    buf.write_f64::<LittleEndian>(m)
        .map_err(GeoParquetError::Io)?;
    Ok(buf)
}

/// Encodes a 4D point `(x, y, z, m)` to WKB using ISO type code 3001.
///
/// # Errors
/// Returns an error if the buffer write fails.
pub fn encode_point_zm(x: f64, y: f64, z: f64, m: f64) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(37);
    buf.write_u8(WKB_LITTLE_ENDIAN)
        .map_err(GeoParquetError::Io)?;
    buf.write_u32::<LittleEndian>(WkbType::PointZm.code())
        .map_err(GeoParquetError::Io)?;
    buf.write_f64::<LittleEndian>(x)
        .map_err(GeoParquetError::Io)?;
    buf.write_f64::<LittleEndian>(y)
        .map_err(GeoParquetError::Io)?;
    buf.write_f64::<LittleEndian>(z)
        .map_err(GeoParquetError::Io)?;
    buf.write_f64::<LittleEndian>(m)
        .map_err(GeoParquetError::Io)?;
    Ok(buf)
}

// ── Polygon encoders ──────────────────────────────────────────────────────────

/// Encodes a 2D Polygon with optional holes to WKB.
///
/// # Arguments
/// * `exterior` – outer ring as `(x, y)` pairs
/// * `holes` – zero or more interior rings
///
/// # Errors
/// Returns an error if a ring encoding fails.
pub fn encode_polygon(exterior: &[(f64, f64)], holes: &[Vec<(f64, f64)>]) -> Result<Vec<u8>> {
    let num_rings = 1u32 + holes.len() as u32;
    let mut ring_bufs: Vec<Vec<u8>> = Vec::with_capacity(num_rings as usize);
    ring_bufs.push(encode_ring_2d(exterior)?);
    for hole in holes {
        ring_bufs.push(encode_ring_2d(hole)?);
    }

    let ring_total: usize = ring_bufs.iter().map(|r| r.len()).sum();
    let mut buf = Vec::with_capacity(1 + 4 + 4 + ring_total);

    buf.write_u8(WKB_LITTLE_ENDIAN)
        .map_err(GeoParquetError::Io)?;
    buf.write_u32::<LittleEndian>(WkbType::Polygon.code())
        .map_err(GeoParquetError::Io)?;
    buf.write_u32::<LittleEndian>(num_rings)
        .map_err(GeoParquetError::Io)?;
    for ring in ring_bufs {
        buf.write_all(&ring).map_err(GeoParquetError::Io)?;
    }
    Ok(buf)
}

/// Encodes a 3D Polygon (PolygonZ) with optional holes to WKB.
///
/// # Errors
/// Returns an error if a ring encoding fails.
pub fn encode_polygon_z(
    exterior: &[(f64, f64, f64)],
    holes: &[Vec<(f64, f64, f64)>],
) -> Result<Vec<u8>> {
    let num_rings = 1u32 + holes.len() as u32;
    let mut ring_bufs: Vec<Vec<u8>> = Vec::with_capacity(num_rings as usize);
    ring_bufs.push(encode_ring_3d(exterior)?);
    for hole in holes {
        ring_bufs.push(encode_ring_3d(hole)?);
    }

    let ring_total: usize = ring_bufs.iter().map(|r| r.len()).sum();
    let mut buf = Vec::with_capacity(1 + 4 + 4 + ring_total);

    buf.write_u8(WKB_LITTLE_ENDIAN)
        .map_err(GeoParquetError::Io)?;
    buf.write_u32::<LittleEndian>(WkbType::PolygonZ.code())
        .map_err(GeoParquetError::Io)?;
    buf.write_u32::<LittleEndian>(num_rings)
        .map_err(GeoParquetError::Io)?;
    for ring in ring_bufs {
        buf.write_all(&ring).map_err(GeoParquetError::Io)?;
    }
    Ok(buf)
}

// ── MultiPolygon encoders ─────────────────────────────────────────────────────

/// Encodes a 2D MultiPolygon (each polygon may have holes) to WKB.
///
/// `polygons` is a slice of `(exterior_ring, holes)` tuples.
///
/// # Errors
/// Returns an error if any polygon encoding fails.
pub fn encode_multi_polygon(polygons: &[Polygon2d]) -> Result<Vec<u8>> {
    let mut poly_bufs: Vec<Vec<u8>> = Vec::with_capacity(polygons.len());
    for (exterior, holes) in polygons {
        poly_bufs.push(encode_polygon(exterior, holes)?);
    }

    let poly_total: usize = poly_bufs.iter().map(|p| p.len()).sum();
    let mut buf = Vec::with_capacity(1 + 4 + 4 + poly_total);

    buf.write_u8(WKB_LITTLE_ENDIAN)
        .map_err(GeoParquetError::Io)?;
    buf.write_u32::<LittleEndian>(WkbType::MultiPolygon.code())
        .map_err(GeoParquetError::Io)?;
    buf.write_u32::<LittleEndian>(poly_bufs.len() as u32)
        .map_err(GeoParquetError::Io)?;
    for poly in poly_bufs {
        buf.write_all(&poly).map_err(GeoParquetError::Io)?;
    }
    Ok(buf)
}

/// Encodes a 3D MultiPolygon (MultiPolygonZ) with holes to WKB.
///
/// # Errors
/// Returns an error if any polygon encoding fails.
pub fn encode_multi_polygon_z(polygons: &[Polygon3d]) -> Result<Vec<u8>> {
    let mut poly_bufs: Vec<Vec<u8>> = Vec::with_capacity(polygons.len());
    for (exterior, holes) in polygons {
        poly_bufs.push(encode_polygon_z(exterior, holes)?);
    }

    let poly_total: usize = poly_bufs.iter().map(|p| p.len()).sum();
    let mut buf = Vec::with_capacity(1 + 4 + 4 + poly_total);

    buf.write_u8(WKB_LITTLE_ENDIAN)
        .map_err(GeoParquetError::Io)?;
    buf.write_u32::<LittleEndian>(WkbType::MultiPolygonZ.code())
        .map_err(GeoParquetError::Io)?;
    buf.write_u32::<LittleEndian>(poly_bufs.len() as u32)
        .map_err(GeoParquetError::Io)?;
    for poly in poly_bufs {
        buf.write_all(&poly).map_err(GeoParquetError::Io)?;
    }
    Ok(buf)
}

// ── GeometryCollection encoder ────────────────────────────────────────────────

/// Encodes a `GeometryCollection` from a slice of pre-encoded WKB geometries.
///
/// Each element of `geometries` must already be a complete, valid WKB blob.
///
/// # Errors
/// Returns an error if the buffer write fails.
pub fn encode_geometry_collection(geometries: &[Vec<u8>]) -> Result<Vec<u8>> {
    let inner_total: usize = geometries.iter().map(|g| g.len()).sum();
    let mut buf = Vec::with_capacity(1 + 4 + 4 + inner_total);

    buf.write_u8(WKB_LITTLE_ENDIAN)
        .map_err(GeoParquetError::Io)?;
    buf.write_u32::<LittleEndian>(WkbType::GeometryCollection.code())
        .map_err(GeoParquetError::Io)?;
    buf.write_u32::<LittleEndian>(geometries.len() as u32)
        .map_err(GeoParquetError::Io)?;
    for geom in geometries {
        buf.write_all(geom).map_err(GeoParquetError::Io)?;
    }
    Ok(buf)
}

// ── Bounding box helpers ──────────────────────────────────────────────────────

/// Reads a little-endian `f64` from a byte slice at the given offset.
fn read_le_f64(data: &[u8], offset: usize) -> Option<f64> {
    let bytes: [u8; 8] = data.get(offset..offset + 8)?.try_into().ok()?;
    Some(f64::from_le_bytes(bytes))
}

/// Reads a little-endian `u32` from a byte slice at the given offset.
fn read_le_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = data.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

/// Computes the 2D bounding box `(min_x, min_y, max_x, max_y)` of a WKB-encoded
/// geometry.
///
/// Returns `None` for empty or unrecognised geometries.  Only the 2D components
/// (x, y) are considered.
#[must_use]
pub fn wkb_bbox(wkb: &[u8]) -> Option<(f64, f64, f64, f64)> {
    if wkb.len() < 5 {
        return None;
    }

    let is_le = wkb[0] == WKB_LITTLE_ENDIAN;
    if !is_le {
        // Big-endian not handled in this helper; callers can use the WkbReader
        // from wkb.rs for full portability.
        return None;
    }

    let type_code = read_le_u32(wkb, 1)?;
    // Normalise to base type (strip Z/M 1000/2000/3000 prefix).
    let base_type = type_code % 1000;
    let has_z = (type_code / 1000) == 1 || (type_code / 1000) == 3;
    let coord_stride = if has_z { 24usize } else { 16usize }; // bytes per 2D or 3D coord

    match base_type {
        1 => {
            // Point: 5 (header) + coord bytes
            let x = read_le_f64(wkb, 5)?;
            let y = read_le_f64(wkb, 13)?;
            Some((x, y, x, y))
        }
        2 | 3 => {
            // LineString / Polygon (we scan the first ring for Polygon)
            // For Polygon the ring count is at offset 5, then ring point count at 9
            let offset = if base_type == 3 {
                // skip num_rings u32
                9usize
            } else {
                5usize
            };
            let num_points = read_le_u32(wkb, offset)? as usize;
            let data_start = offset + 4;

            let mut min_x = f64::INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut max_y = f64::NEG_INFINITY;

            for i in 0..num_points {
                let base = data_start + i * coord_stride;
                let x = read_le_f64(wkb, base)?;
                let y = read_le_f64(wkb, base + 8)?;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }

            if min_x.is_finite() {
                Some((min_x, min_y, max_x, max_y))
            } else {
                None
            }
        }
        _ => {
            // For multi-geometries and collections we fall back to iterating
            // sub-geometry WKB blobs is complex without a full parser; return
            // None to signal "not computed".
            None
        }
    }
}

// ── Column-level geometry statistics ─────────────────────────────────────────

/// Column-level geometry statistics derived from a batch of WKB values.
///
/// These are written into the GeoParquet metadata `geometry_types` and `bbox`
/// fields of the geometry column.
#[derive(Debug, Clone, PartialEq)]
pub struct GeometryStats {
    /// Union bounding box of all geometries, if computable.
    pub bbox: Option<(f64, f64, f64, f64)>,
    /// Distinct base geometry type codes observed.
    pub geometry_types: Vec<u32>,
    /// `true` if any geometry has a Z coordinate.
    pub has_z: bool,
    /// `true` if any geometry has an M coordinate.
    pub has_m: bool,
    /// Total number of WKB values processed.
    pub count: usize,
}

impl GeometryStats {
    /// Creates an empty `GeometryStats`.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            bbox: None,
            geometry_types: Vec::new(),
            has_z: false,
            has_m: false,
            count: 0,
        }
    }
}

/// Computes `GeometryStats` for a collection of WKB-encoded geometries.
///
/// Empty or malformed WKB blobs are silently skipped; they still increment `count`.
#[must_use]
pub fn compute_geometry_stats(wkb_values: &[Vec<u8>]) -> GeometryStats {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut has_any_bbox = false;
    let mut type_set = std::collections::HashSet::new();
    let mut has_z = false;
    let mut has_m = false;

    for wkb in wkb_values {
        if wkb.len() < 5 {
            continue;
        }

        // Read type code.
        if wkb[0] == WKB_LITTLE_ENDIAN {
            if let Some(type_code) = read_le_u32(wkb, 1) {
                let base = type_code % 1000;
                let qualifier = type_code / 1000;
                type_set.insert(base);
                if qualifier == 1 || qualifier == 3 {
                    has_z = true;
                }
                if qualifier == 2 || qualifier == 3 {
                    has_m = true;
                }
            }
        }

        // Extend bounding box.
        if let Some((bx_min, by_min, bx_max, by_max)) = wkb_bbox(wkb) {
            min_x = min_x.min(bx_min);
            min_y = min_y.min(by_min);
            max_x = max_x.max(bx_max);
            max_y = max_y.max(by_max);
            has_any_bbox = true;
        }
    }

    let bbox = if has_any_bbox && min_x.is_finite() {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    };

    let mut geometry_types: Vec<u32> = type_set.into_iter().collect();
    geometry_types.sort_unstable();

    GeometryStats {
        bbox,
        geometry_types,
        has_z,
        has_m,
        count: wkb_values.len(),
    }
}

// ── WKB round-trip decoder (2D geometry types for validation) ─────────────────

/// Reads a 2D Point from WKB (little-endian only).
///
/// Returns `(x, y)` or an error.
///
/// # Errors
/// Returns an error if the buffer is too small or the type code is wrong.
pub fn decode_point_2d(wkb: &[u8]) -> Result<(f64, f64)> {
    if wkb.len() < 21 {
        return Err(GeoParquetError::invalid_wkb(
            "Point WKB too short (need 21 bytes)",
        ));
    }
    if wkb[0] != WKB_LITTLE_ENDIAN {
        return Err(GeoParquetError::invalid_wkb(
            "only little-endian WKB supported",
        ));
    }
    let type_code = read_le_u32(wkb, 1)
        .ok_or_else(|| GeoParquetError::invalid_wkb("cannot read WKB type code"))?;
    if type_code != WkbType::Point.code() {
        return Err(GeoParquetError::invalid_wkb(format!(
            "expected Point (1), got {type_code}"
        )));
    }
    let x = read_le_f64(wkb, 5).ok_or_else(|| GeoParquetError::invalid_wkb("cannot read x"))?;
    let y = read_le_f64(wkb, 13).ok_or_else(|| GeoParquetError::invalid_wkb("cannot read y"))?;
    Ok((x, y))
}

/// Reads a 3D Point (PointZ) from WKB (little-endian only).
///
/// Returns `(x, y, z)` or an error.
///
/// # Errors
/// Returns an error if the buffer is too small or the type code is wrong.
pub fn decode_point_z(wkb: &[u8]) -> Result<(f64, f64, f64)> {
    if wkb.len() < 29 {
        return Err(GeoParquetError::invalid_wkb(
            "PointZ WKB too short (need 29 bytes)",
        ));
    }
    if wkb[0] != WKB_LITTLE_ENDIAN {
        return Err(GeoParquetError::invalid_wkb(
            "only little-endian WKB supported",
        ));
    }
    let type_code = read_le_u32(wkb, 1)
        .ok_or_else(|| GeoParquetError::invalid_wkb("cannot read WKB type code"))?;
    if type_code != WkbType::PointZ.code() {
        return Err(GeoParquetError::invalid_wkb(format!(
            "expected PointZ (1001), got {type_code}"
        )));
    }
    let x = read_le_f64(wkb, 5).ok_or_else(|| GeoParquetError::invalid_wkb("cannot read x"))?;
    let y = read_le_f64(wkb, 13).ok_or_else(|| GeoParquetError::invalid_wkb("cannot read y"))?;
    let z = read_le_f64(wkb, 21).ok_or_else(|| GeoParquetError::invalid_wkb("cannot read z"))?;
    Ok((x, y, z))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── WkbType ───────────────────────────────────────────────────────────────

    #[test]
    fn test_wkb_type_codes() {
        assert_eq!(WkbType::Point.code(), 1);
        assert_eq!(WkbType::LineString.code(), 2);
        assert_eq!(WkbType::Polygon.code(), 3);
        assert_eq!(WkbType::MultiPolygon.code(), 6);
        assert_eq!(WkbType::GeometryCollection.code(), 7);
        assert_eq!(WkbType::PointZ.code(), 1001);
        assert_eq!(WkbType::PointM.code(), 2001);
        assert_eq!(WkbType::PointZm.code(), 3001);
    }

    // ── encode/decode Point roundtrips ───────────────────────────────────────

    #[test]
    fn test_point_2d_roundtrip() {
        let wkb = encode_point_2d(1.5, 2.5).expect("encode");
        assert_eq!(wkb.len(), 21);
        let (x, y) = decode_point_2d(&wkb).expect("decode");
        assert!((x - 1.5).abs() < f64::EPSILON);
        assert!((y - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_point_z_roundtrip() {
        let wkb = encode_point_z(10.0, 20.0, 30.0).expect("encode");
        assert_eq!(wkb.len(), 29);
        let (x, y, z) = decode_point_z(&wkb).expect("decode");
        assert!((x - 10.0).abs() < f64::EPSILON);
        assert!((y - 20.0).abs() < f64::EPSILON);
        assert!((z - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_point_m_has_correct_type() {
        let wkb = encode_point_m(1.0, 2.0, 3.0).expect("encode");
        let type_code = read_le_u32(&wkb, 1).expect("read type");
        assert_eq!(type_code, WkbType::PointM.code());
    }

    #[test]
    fn test_point_zm_has_correct_type() {
        let wkb = encode_point_zm(1.0, 2.0, 3.0, 4.0).expect("encode");
        let type_code = read_le_u32(&wkb, 1).expect("read type");
        assert_eq!(type_code, WkbType::PointZm.code());
        assert_eq!(wkb.len(), 37);
    }

    #[test]
    fn test_point_decode_wrong_type_errors() {
        let polygon_wkb = encode_polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)], &[])
            .expect("encode polygon");
        assert!(decode_point_2d(&polygon_wkb).is_err());
    }

    // ── encode_ring ───────────────────────────────────────────────────────────

    #[test]
    fn test_ring_2d_byte_count() {
        let ring = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)];
        let buf = encode_ring_2d(&ring).expect("encode ring");
        // 4 (count) + 4 * 16 = 68
        assert_eq!(buf.len(), 4 + 4 * 16);
    }

    #[test]
    fn test_ring_3d_byte_count() {
        let ring = vec![(0.0, 0.0, 1.0), (1.0, 0.0, 1.0), (0.0, 0.0, 1.0)];
        let buf = encode_ring_3d(&ring).expect("encode ring 3d");
        // 4 + 3 * 24 = 76
        assert_eq!(buf.len(), 4 + 3 * 24);
    }

    // ── encode_polygon ────────────────────────────────────────────────────────

    #[test]
    fn test_polygon_no_holes_type_code() {
        let ext = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 0.0)];
        let wkb = encode_polygon(&ext, &[]).expect("encode");
        // byte order + type code + ring count
        assert_eq!(wkb[0], WKB_LITTLE_ENDIAN);
        let type_code = read_le_u32(&wkb, 1).expect("type");
        assert_eq!(type_code, WkbType::Polygon.code());
        let num_rings = read_le_u32(&wkb, 5).expect("rings");
        assert_eq!(num_rings, 1);
    }

    #[test]
    fn test_polygon_with_hole_ring_count() {
        let ext = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 0.0)];
        let hole = vec![(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 1.0)];
        let wkb = encode_polygon(&ext, &[hole]).expect("encode");
        let num_rings = read_le_u32(&wkb, 5).expect("rings");
        assert_eq!(num_rings, 2);
    }

    #[test]
    fn test_polygon_z_type_code() {
        let ext = vec![(0.0, 0.0, 5.0), (1.0, 0.0, 5.0), (0.0, 0.0, 5.0)];
        let wkb = encode_polygon_z(&ext, &[]).expect("encode");
        let type_code = read_le_u32(&wkb, 1).expect("type");
        assert_eq!(type_code, WkbType::PolygonZ.code());
    }

    // ── encode_multi_polygon ──────────────────────────────────────────────────

    #[test]
    fn test_multi_polygon_no_holes() {
        let polys = vec![
            (vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)], vec![]),
            (vec![(2.0, 2.0), (3.0, 2.0), (3.0, 3.0), (2.0, 2.0)], vec![]),
        ];
        let wkb = encode_multi_polygon(&polys).expect("encode");
        let type_code = read_le_u32(&wkb, 1).expect("type");
        assert_eq!(type_code, WkbType::MultiPolygon.code());
        let num_polys = read_le_u32(&wkb, 5).expect("num polys");
        assert_eq!(num_polys, 2);
    }

    #[test]
    fn test_multi_polygon_with_holes() {
        let polys = vec![(
            vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 0.0)],
            vec![vec![(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 1.0)]],
        )];
        let wkb = encode_multi_polygon(&polys).expect("encode");
        // The embedded polygon's ring count should be 2 (exterior + 1 hole).
        // Polygon header starts at byte 9 (after MultiPolygon header 9).
        // polygon: byte_order (1) + type (4) + num_rings (4) = 9 bytes header
        // multipolygon: byte_order (1) + type (4) + num_polys (4) = 9 bytes
        let poly_num_rings = read_le_u32(&wkb, 9 + 5).expect("poly rings");
        assert_eq!(poly_num_rings, 2);
    }

    #[test]
    fn test_multi_polygon_z_type_code() {
        let polys = vec![(
            vec![(0.0, 0.0, 1.0), (1.0, 0.0, 1.0), (0.0, 0.0, 1.0)],
            vec![],
        )];
        let wkb = encode_multi_polygon_z(&polys).expect("encode");
        let type_code = read_le_u32(&wkb, 1).expect("type");
        assert_eq!(type_code, WkbType::MultiPolygonZ.code());
    }

    // ── encode_geometry_collection ────────────────────────────────────────────

    #[test]
    fn test_geometry_collection_empty() {
        let wkb = encode_geometry_collection(&[]).expect("encode");
        let type_code = read_le_u32(&wkb, 1).expect("type");
        assert_eq!(type_code, WkbType::GeometryCollection.code());
        let num_geoms = read_le_u32(&wkb, 5).expect("num geoms");
        assert_eq!(num_geoms, 0);
    }

    #[test]
    fn test_geometry_collection_with_points() {
        let p1 = encode_point_2d(1.0, 2.0).expect("pt1");
        let p2 = encode_point_2d(3.0, 4.0).expect("pt2");
        let wkb = encode_geometry_collection(&[p1, p2]).expect("encode gc");
        let type_code = read_le_u32(&wkb, 1).expect("type");
        assert_eq!(type_code, WkbType::GeometryCollection.code());
        let num_geoms = read_le_u32(&wkb, 5).expect("num");
        assert_eq!(num_geoms, 2);
    }

    #[test]
    fn test_geometry_collection_mixed() {
        let pt = encode_point_z(1.0, 2.0, 3.0).expect("point z");
        let poly = encode_polygon(&[(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 0.0)], &[])
            .expect("polygon");
        let gc = encode_geometry_collection(&[pt, poly]).expect("gc");
        let num_geoms = read_le_u32(&gc, 5).expect("num");
        assert_eq!(num_geoms, 2);
    }

    // ── wkb_bbox ──────────────────────────────────────────────────────────────

    #[test]
    fn test_bbox_point() {
        let wkb = encode_point_2d(3.0, 7.0).expect("encode");
        let bbox = wkb_bbox(&wkb).expect("bbox");
        assert!((bbox.0 - 3.0).abs() < f64::EPSILON);
        assert!((bbox.1 - 7.0).abs() < f64::EPSILON);
        assert!((bbox.2 - 3.0).abs() < f64::EPSILON);
        assert!((bbox.3 - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_bbox_polygon() {
        let ring = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 0.0)];
        let wkb = encode_polygon(&ring, &[]).expect("encode");
        let bbox = wkb_bbox(&wkb).expect("bbox");
        assert!((bbox.0 - 0.0).abs() < f64::EPSILON);
        assert!((bbox.1 - 0.0).abs() < f64::EPSILON);
        assert!((bbox.2 - 4.0).abs() < f64::EPSILON);
        assert!((bbox.3 - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_bbox_empty_returns_none() {
        let empty: Vec<u8> = vec![];
        assert!(wkb_bbox(&empty).is_none());
    }

    #[test]
    fn test_bbox_short_data_returns_none() {
        let short = vec![0x01u8, 0x00, 0x00];
        assert!(wkb_bbox(&short).is_none());
    }

    // ── compute_geometry_stats ────────────────────────────────────────────────

    #[test]
    fn test_stats_empty_input() {
        let stats = compute_geometry_stats(&[]);
        assert!(stats.bbox.is_none());
        assert!(stats.geometry_types.is_empty());
        assert_eq!(stats.count, 0);
        assert!(!stats.has_z);
        assert!(!stats.has_m);
    }

    #[test]
    fn test_stats_single_point() {
        let wkb = encode_point_2d(1.0, 2.0).expect("encode");
        let stats = compute_geometry_stats(&[wkb]);
        assert_eq!(stats.count, 1);
        assert!(stats.bbox.is_some());
        assert!(stats.geometry_types.contains(&1u32)); // Point
        assert!(!stats.has_z);
    }

    #[test]
    fn test_stats_z_point_sets_has_z() {
        let wkb = encode_point_z(1.0, 2.0, 3.0).expect("encode");
        let stats = compute_geometry_stats(&[wkb]);
        assert!(stats.has_z);
        assert!(!stats.has_m);
    }

    #[test]
    fn test_stats_m_point_sets_has_m() {
        let wkb = encode_point_m(1.0, 2.0, 5.0).expect("encode");
        let stats = compute_geometry_stats(&[wkb]);
        assert!(!stats.has_z);
        assert!(stats.has_m);
    }

    #[test]
    fn test_stats_zm_point_sets_both() {
        let wkb = encode_point_zm(1.0, 2.0, 3.0, 4.0).expect("encode");
        let stats = compute_geometry_stats(&[wkb]);
        assert!(stats.has_z);
        assert!(stats.has_m);
    }

    #[test]
    fn test_stats_multiple_geometries_bbox_union() {
        let p1 = encode_point_2d(0.0, 0.0).expect("p1");
        let p2 = encode_point_2d(10.0, 20.0).expect("p2");
        let stats = compute_geometry_stats(&[p1, p2]);
        let bbox = stats.bbox.expect("bbox");
        assert!((bbox.0 - 0.0).abs() < f64::EPSILON); // min_x
        assert!((bbox.1 - 0.0).abs() < f64::EPSILON); // min_y
        assert!((bbox.2 - 10.0).abs() < f64::EPSILON); // max_x
        assert!((bbox.3 - 20.0).abs() < f64::EPSILON); // max_y
    }

    #[test]
    fn test_stats_distinct_geometry_types() {
        let pt = encode_point_2d(0.0, 0.0).expect("pt");
        let poly =
            encode_polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)], &[]).expect("poly");
        let stats = compute_geometry_stats(&[pt, poly]);
        assert!(stats.geometry_types.contains(&1)); // Point
        assert!(stats.geometry_types.contains(&3)); // Polygon
    }
}
