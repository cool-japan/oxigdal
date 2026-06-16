//! GeoPackage geometry blob envelope extraction.
//!
//! Implements OGC GeoPackage §2.1.3 binary geometry header parsing and
//! ISO 13249-3 WKB coordinate scanning for bounding box extraction.

use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read};

use crate::error::{Error, Result};

/// 2-D bounding box: `(min_x, min_y, max_x, max_y)`.
pub(crate) type Bbox2d = (f64, f64, f64, f64);

// ─── GPKG binary header constants ────────────────────────────────────────────

const GPKG_MAGIC_0: u8 = b'G';
const GPKG_MAGIC_1: u8 = b'P';

/// Bit 0 of the flags byte: 1 = little-endian SRS-id / header floats.
const FLAG_BYTE_ORDER: u8 = 0x01;
/// Bits 1-3 of the flags byte encode the envelope contents indicator.
const FLAG_ENVELOPE_MASK: u8 = 0x0E;
/// Bit 4: geometry is empty.
const FLAG_IS_EMPTY: u8 = 0x10;

/// Extract envelope-contents-indicator (0-4) from the flags byte.
#[inline]
fn envelope_flag(flags: u8) -> u8 {
    (flags & FLAG_ENVELOPE_MASK) >> 1
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Parse a bounding box from a raw GeoPackage geometry blob.
///
/// Fast path: uses the pre-computed envelope stored in the GPKG binary header
/// (§2.1.3).  Falls back to a full WKB coordinate scan when no envelope is
/// present (`envelope_flag = 0`).
pub(crate) fn read_envelope_from_blob(blob: &[u8]) -> Result<Bbox2d> {
    if blob.len() < 8 {
        return Err(Error::geometry(
            "GeoPackage geometry blob is too short (< 8 bytes)",
        ));
    }

    // Verify magic bytes
    if blob[0] != GPKG_MAGIC_0 || blob[1] != GPKG_MAGIC_1 {
        return Err(Error::geometry(format!(
            "Invalid GeoPackage geometry magic: 0x{:02X}{:02X}",
            blob[0], blob[1]
        )));
    }

    // blob[2] = version (we accept any version)
    let flags = blob[3];
    let _le_header = (flags & FLAG_BYTE_ORDER) != 0; // byte-order for srs_id field
    let env_flag = envelope_flag(flags);
    let is_empty = (flags & FLAG_IS_EMPTY) != 0;

    if is_empty {
        return Err(Error::geometry("GeoPackage geometry is flagged as empty"));
    }

    // Skip: magic(2) + version(1) + flags(1) + srs_id(4) = 8 bytes
    let after_header: usize = 8;

    // Envelope float64 values are *always* little-endian per GeoPackage spec §2.1.3.
    match env_flag {
        0 => {
            // No envelope — scan WKB body directly
            let wkb = &blob[after_header..];
            scan_wkb_coordinates(wkb)
        }
        1 => {
            // [minx, maxx, miny, maxy] — 32 bytes
            read_xy_envelope_le(blob, after_header, 4)
        }
        2 | 3 => {
            // [minx, maxx, miny, maxy, minz, maxz] or [minx, maxx, miny, maxy, minm, maxm]
            // — 48 bytes; we only need the first four f64 values
            read_xy_envelope_le(blob, after_header, 6)
        }
        4 => {
            // [minx, maxx, miny, maxy, minz, maxz, minm, maxm] — 64 bytes
            read_xy_envelope_le(blob, after_header, 8)
        }
        _ => Err(Error::geometry(format!(
            "Unknown GeoPackage envelope flag: {}",
            env_flag
        ))),
    }
}

/// Read `[minx, maxx, miny, maxy, ...]` from a little-endian blob starting at
/// `offset`.  `n_f64` is the total number of f64 values in the envelope block
/// (we only consume the first four).
fn read_xy_envelope_le(blob: &[u8], offset: usize, n_f64: usize) -> Result<Bbox2d> {
    let required = offset + n_f64 * 8;
    if blob.len() < required {
        return Err(Error::geometry(format!(
            "GeoPackage envelope truncated: need {} bytes, got {}",
            required,
            blob.len()
        )));
    }

    let mut cur = Cursor::new(&blob[offset..]);

    let min_x = cur.read_f64::<LittleEndian>()?;
    let max_x = cur.read_f64::<LittleEndian>()?;
    let min_y = cur.read_f64::<LittleEndian>()?;
    let max_y = cur.read_f64::<LittleEndian>()?;

    Ok((min_x, min_y, max_x, max_y))
}

// ─── WKB coordinate scanner ───────────────────────────────────────────────────

/// Scan all XY coordinates in a raw WKB buffer and return the bounding box.
fn scan_wkb_coordinates(wkb: &[u8]) -> Result<Bbox2d> {
    let mut scanner = WkbScanner::new(wkb);
    scanner.scan_geometry()?;

    if !scanner.initialized {
        return Err(Error::geometry("WKB geometry contained no coordinates"));
    }

    Ok((scanner.min_x, scanner.min_y, scanner.max_x, scanner.max_y))
}

// ─── WkbScanner ──────────────────────────────────────────────────────────────

struct WkbScanner<'a> {
    cursor: Cursor<&'a [u8]>,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    initialized: bool,
}

impl<'a> WkbScanner<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            cursor: Cursor::new(data),
            min_x: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            min_y: f64::INFINITY,
            max_y: f64::NEG_INFINITY,
            initialized: false,
        }
    }

    #[inline]
    fn update(&mut self, x: f64, y: f64) {
        if x < self.min_x {
            self.min_x = x;
        }
        if x > self.max_x {
            self.max_x = x;
        }
        if y < self.min_y {
            self.min_y = y;
        }
        if y > self.max_y {
            self.max_y = y;
        }
        self.initialized = true;
    }

    /// Read a single `f64` value using the given WKB byte-order byte.
    #[inline]
    fn read_f64(&mut self, byte_order: u8) -> Result<f64> {
        if byte_order == 0 {
            Ok(self.cursor.read_f64::<BigEndian>()?)
        } else {
            Ok(self.cursor.read_f64::<LittleEndian>()?)
        }
    }

    /// Read a `u32` value using the given WKB byte-order byte.
    #[inline]
    fn read_u32(&mut self, byte_order: u8) -> Result<u32> {
        if byte_order == 0 {
            Ok(self.cursor.read_u32::<BigEndian>()?)
        } else {
            Ok(self.cursor.read_u32::<LittleEndian>()?)
        }
    }

    /// Skip `n` bytes from the cursor (for consuming Z/M values we don't need).
    #[inline]
    fn skip_f64(&mut self, byte_order: u8) -> Result<()> {
        let _ = self.read_f64(byte_order)?;
        Ok(())
    }

    /// Scan one WKB geometry starting at the current cursor position.
    fn scan_geometry(&mut self) -> Result<()> {
        let byte_order = self.cursor.read_u8()?;
        if byte_order > 1 {
            return Err(Error::geometry(format!(
                "Invalid WKB byte-order byte: {}",
                byte_order
            )));
        }

        let raw_type = self.read_u32(byte_order)?;

        // Decode has_z / has_m / base_type from the raw type code.
        //
        // Supported encodings:
        //  - ISO WKB base:        1-7
        //  - ISO WKB + Z:         1001-1007
        //  - ISO WKB + M:         2001-2007
        //  - ISO WKB + ZM:        3001-3007
        //  - EWKB (PostGIS):      high bits 0x80000000 (Z) / 0x40000000 (M)
        //  - ISO SQL/MM (bit 31): handled via EWKB path

        let ewkb_has_z = (raw_type & 0x8000_0000) != 0;
        let ewkb_has_m = (raw_type & 0x4000_0000) != 0;

        // Strip EWKB high bits to get the numeric type
        let numeric_type = raw_type & 0x1FFF_FFFF;

        let (has_z, has_m, base_type) = if ewkb_has_z || ewkb_has_m {
            (ewkb_has_z, ewkb_has_m, numeric_type)
        } else {
            match numeric_type {
                1..=7 => (false, false, numeric_type),
                1001..=1007 => (true, false, numeric_type - 1000),
                2001..=2007 => (false, true, numeric_type - 2000),
                3001..=3007 => (true, true, numeric_type - 3000),
                _ => {
                    return Err(Error::geometry(format!(
                        "Unsupported WKB type code: {}",
                        raw_type
                    )));
                }
            }
        };

        match base_type {
            1 => self.scan_point(byte_order, has_z, has_m)?,
            2 => self.scan_linestring(byte_order, has_z, has_m)?,
            3 => self.scan_polygon(byte_order, has_z, has_m)?,
            4..=7 => self.scan_collection(byte_order)?,
            _ => {
                return Err(Error::geometry(format!(
                    "Unknown WKB base type: {}",
                    base_type
                )));
            }
        }

        Ok(())
    }

    /// Read and record a single point's XY (optionally consuming Z and/or M).
    fn scan_point(&mut self, byte_order: u8, has_z: bool, has_m: bool) -> Result<()> {
        let x = self.read_f64(byte_order)?;
        let y = self.read_f64(byte_order)?;
        if has_z {
            self.skip_f64(byte_order)?;
        }
        if has_m {
            self.skip_f64(byte_order)?;
        }
        self.update(x, y);
        Ok(())
    }

    /// Scan a LineString: u32 count followed by that many points.
    fn scan_linestring(&mut self, byte_order: u8, has_z: bool, has_m: bool) -> Result<()> {
        let count = self.read_u32(byte_order)?;
        for _ in 0..count {
            self.scan_point(byte_order, has_z, has_m)?;
        }
        Ok(())
    }

    /// Scan a Polygon: u32 num_rings; for each ring u32 num_points + that many points.
    fn scan_polygon(&mut self, byte_order: u8, has_z: bool, has_m: bool) -> Result<()> {
        let num_rings = self.read_u32(byte_order)?;
        for _ in 0..num_rings {
            self.scan_linestring(byte_order, has_z, has_m)?;
        }
        Ok(())
    }

    /// Scan a Multi* or GeometryCollection: u32 count then recursive sub-geometries.
    /// Each sub-geometry has its own WKB byte-order byte + type header.
    fn scan_collection(&mut self, _byte_order: u8) -> Result<()> {
        // The collection count itself uses the outer byte_order, but each
        // sub-geometry is self-describing (its own byte-order byte).
        let count = {
            // Re-read the byte_order from the outer context is not available here
            // because each collection stores its own count with the *collection's*
            // byte order.  We already consumed the byte-order byte for the outer
            // geometry, so we need to read the count using that same byte order.
            // However, scan_geometry is called fresh (byte_order already consumed).
            // We pass _byte_order through for the count, which is correct per WKB.
            self.read_u32(_byte_order)?
        };
        for _ in 0..count {
            self.scan_geometry()?;
        }
        Ok(())
    }
}

// The `Read` trait is used by byteorder extension methods (`read_u8`).
// The import at the top of the file is required to bring that trait into scope.
const _USE_READ_TRAIT: fn() = || {
    let _ = std::io::empty().read(&mut []);
};

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ─────────────────────────────────────────────────────────────

    /// Build a minimal GPKG blob with XY envelope (flag=1) for a point.
    fn make_gpkg_blob_with_envelope(x: f64, y: f64) -> Vec<u8> {
        // Header: magic(2) + version(1) + flags(1) + srs_id(4)
        // flags: bit0=1 (LE), bits1-3=001 (envelope flag=1) → 0x03
        let mut blob = vec![b'G', b'P', 0x00u8, 0x03u8];
        blob.extend_from_slice(&4326u32.to_le_bytes()); // srs_id LE
        // envelope: minx, maxx, miny, maxy (f64 LE) — for a point min==max
        blob.extend_from_slice(&x.to_le_bytes());
        blob.extend_from_slice(&x.to_le_bytes());
        blob.extend_from_slice(&y.to_le_bytes());
        blob.extend_from_slice(&y.to_le_bytes());
        // WKB Point body: LE byte-order + type 1 (Point) + x + y
        blob.extend_from_slice(&[1u8]);
        blob.extend_from_slice(&1u32.to_le_bytes());
        blob.extend_from_slice(&x.to_le_bytes());
        blob.extend_from_slice(&y.to_le_bytes());
        blob
    }

    /// Build a GPKG blob with no envelope (flag=0), WKB body only.
    fn make_gpkg_blob_no_envelope(x: f64, y: f64) -> Vec<u8> {
        // flags: bit0=1 (LE), bits1-3=000 (no envelope) → 0x01
        let mut blob = vec![b'G', b'P', 0x00u8, 0x01u8];
        blob.extend_from_slice(&4326u32.to_le_bytes()); // srs_id
        // WKB Point directly after header
        blob.extend_from_slice(&[1u8]);
        blob.extend_from_slice(&1u32.to_le_bytes());
        blob.extend_from_slice(&x.to_le_bytes());
        blob.extend_from_slice(&y.to_le_bytes());
        blob
    }

    // ── tests ────────────────────────────────────────────────────────────────

    #[test]
    fn test_envelope_flag_1_xy() -> crate::error::Result<()> {
        let blob = make_gpkg_blob_with_envelope(10.0, 20.0);
        let (min_x, min_y, max_x, max_y) = read_envelope_from_blob(&blob)?;
        assert_eq!(min_x, 10.0);
        assert_eq!(min_y, 20.0);
        assert_eq!(max_x, 10.0);
        assert_eq!(max_y, 20.0);
        Ok(())
    }

    #[test]
    fn test_envelope_flag_0_wkb_scan() -> crate::error::Result<()> {
        let blob = make_gpkg_blob_no_envelope(30.0, 40.0);
        let (min_x, min_y, max_x, max_y) = read_envelope_from_blob(&blob)?;
        assert_eq!(min_x, 30.0);
        assert_eq!(min_y, 40.0);
        assert_eq!(max_x, 30.0);
        assert_eq!(max_y, 40.0);
        Ok(())
    }

    #[test]
    fn test_bad_magic_returns_error() {
        let blob = vec![0x00u8, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00];
        assert!(read_envelope_from_blob(&blob).is_err());
    }

    #[test]
    fn test_empty_flag_returns_error() {
        let mut blob = make_gpkg_blob_with_envelope(1.0, 2.0);
        // Set the is_empty bit (bit 4) in flags byte (index 3)
        blob[3] |= FLAG_IS_EMPTY;
        assert!(read_envelope_from_blob(&blob).is_err());
    }

    #[test]
    fn test_too_short_blob_returns_error() {
        let blob = vec![b'G', b'P', 0x00u8, 0x03u8];
        assert!(read_envelope_from_blob(&blob).is_err());
    }

    #[test]
    fn test_wkb_linestring_scan() -> crate::error::Result<()> {
        // Build a standalone WKB LineString (no GPKG header) with three points
        // and verify that scan_wkb_coordinates returns the correct bbox.
        let mut wkb = vec![1u8]; // LE byte order
        wkb.extend_from_slice(&2u32.to_le_bytes()); // type = LineString
        wkb.extend_from_slice(&3u32.to_le_bytes()); // 3 points
        for (x, y) in [(0.0f64, 0.0f64), (10.0, 5.0), (5.0, -3.0)] {
            wkb.extend_from_slice(&x.to_le_bytes());
            wkb.extend_from_slice(&y.to_le_bytes());
        }
        let (min_x, min_y, max_x, max_y) = scan_wkb_coordinates(&wkb)?;
        assert_eq!(min_x, 0.0);
        assert_eq!(min_y, -3.0);
        assert_eq!(max_x, 10.0);
        assert_eq!(max_y, 5.0);
        Ok(())
    }

    #[test]
    fn test_envelope_flag_2_xyz() -> crate::error::Result<()> {
        // Build a GPKG blob with envelope flag=2 (XY + Z)
        // flags: bit0=1 (LE), bits1-3=010 (envelope flag=2) → 0b00000101 = 0x05
        let mut blob = vec![b'G', b'P', 0x00u8, 0x05u8];
        blob.extend_from_slice(&4326u32.to_le_bytes());
        // envelope: minx=1, maxx=2, miny=3, maxy=4, minz=5, maxz=6
        for v in [1.0f64, 2.0, 3.0, 4.0, 5.0, 6.0] {
            blob.extend_from_slice(&v.to_le_bytes());
        }
        // Append a dummy WKB point (not consumed since envelope is present)
        blob.extend_from_slice(&[1u8]);
        blob.extend_from_slice(&1u32.to_le_bytes());
        blob.extend_from_slice(&1.0f64.to_le_bytes());
        blob.extend_from_slice(&3.0f64.to_le_bytes());

        let (min_x, min_y, max_x, max_y) = read_envelope_from_blob(&blob)?;
        assert_eq!(min_x, 1.0);
        assert_eq!(min_y, 3.0);
        assert_eq!(max_x, 2.0);
        assert_eq!(max_y, 4.0);
        Ok(())
    }
}
