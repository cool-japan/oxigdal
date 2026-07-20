//! GeoPackage Binary (GPB) encoder and feature-row encoder.
//!
//! Provides the minimal byte-layout needed to write point geometries into a
//! GeoPackage feature table.  The encoding follows:
//! - OGC GeoPackage Encoding Standard v1.3.1 §2.1.3 (GeoPackageBinary format)
//! - OGC Simple Features WKB specification (ISO 19125-1)

// ─────────────────────────────────────────────────────────────────────────────
// GeoPackage Binary header (GPB)
// ─────────────────────────────────────────────────────────────────────────────
//
// Byte layout:
//   0-1  magic = 0x47 0x50  ('G', 'P')
//   2    version = 0x00
//   3    flags:
//          bits 0-2  envelope indicator (0 = no envelope)
//          bit  3    empty-geometry flag (0 = not empty)
//          bit  4    reserved (0)
//          bit  5    byte-order (1 = little-endian WKB)
//          bits 6-7  reserved (0)
//   4-7  srs_id (little-endian i32)
//   [8+] WKB body
//
// Minimum flags byte for a non-empty 2-D point with no envelope, LE WKB:
//   0b0010_0001 = 0x21   (little-endian, no envelope, not empty)

/// GPB flags byte: little-endian WKB, no envelope, not empty.
const GPB_FLAGS_LE_NO_ENV: u8 = 1 << 5; // bit 5 = LE

// ─────────────────────────────────────────────────────────────────────────────
// WKB constants
// ─────────────────────────────────────────────────────────────────────────────

/// WKB byte-order for little-endian.
const WKB_LE: u8 = 0x01;

/// WKB geometry type for Point (2D), in little-endian u32.
const WKB_POINT_LE: [u8; 4] = 1u32.to_le_bytes();

// ─────────────────────────────────────────────────────────────────────────────
// Public encoder functions
// ─────────────────────────────────────────────────────────────────────────────

/// Encode a 2-D point as a GeoPackage Binary (GPB) blob.
///
/// The returned bytes are suitable for storage in a `BLOB` geometry column.
///
/// Layout of the returned bytes:
/// ```text
/// [0..2]   magic bytes (0x47, 0x50)
/// [2]      version (0x00)
/// [3]      flags (0x20 = LE, no envelope, not empty)
/// [4..8]   srs_id (little-endian i32)
/// [8]      WKB byte order (0x01 = LE)
/// [9..13]  WKB type = 1 (Point), little-endian u32
/// [13..21] x coordinate, little-endian f64
/// [21..29] y coordinate, little-endian f64
/// ```
///
/// Total: 29 bytes.
pub fn encode_gpkg_point(x: f64, y: f64, srs_id: i32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(29);

    // GPB header
    buf.push(0x47); // 'G'
    buf.push(0x50); // 'P'
    buf.push(0x00); // version
    buf.push(GPB_FLAGS_LE_NO_ENV); // flags
    buf.extend_from_slice(&srs_id.to_le_bytes()); // srs_id (4 bytes, LE)

    // WKB Point (little-endian)
    buf.push(WKB_LE); // byte order
    buf.extend_from_slice(&WKB_POINT_LE); // geometry type = 1
    buf.extend_from_slice(&x.to_le_bytes()); // x
    buf.extend_from_slice(&y.to_le_bytes()); // y

    buf
}

// ─────────────────────────────────────────────────────────────────────────────
// Feature row encoder
// ─────────────────────────────────────────────────────────────────────────────

/// Compute the bounding box `(min_x, min_y, max_x, max_y)` for a set of 2-D
/// points.  Returns `None` when the point set is empty.
pub fn compute_bbox(points: &[(i64, f64, f64)]) -> Option<(f64, f64, f64, f64)> {
    if points.is_empty() {
        return None;
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (_fid, x, y) in points {
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
