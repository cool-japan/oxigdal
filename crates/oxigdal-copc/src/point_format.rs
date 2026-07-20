//! LAS point record binary deserialization.
//!
//! Supports point data record formats 0-3 (LAS 1.0-1.3 legacy) and 6-10
//! (LAS 1.4 extended).  Each format has a fixed-size base record plus optional
//! trailing fields (GPS time, RGB colour, NIR, waveform packet).
//!
//! Reference: ASPRS LAS Specification 1.4 R15, Tables 6-22.

use crate::error::CopcError;
use crate::point::{Point3D, WaveformPacket};

/// Shared base fields extracted from a point record by `parse_legacy_base` /
/// `parse_extended_base`.
///
/// Tuple order: `(return_number, number_of_returns, classification,
/// scan_angle_rank, user_data, point_source_id, gps_time, rgb_offset)`.
///
/// `scan_angle_rank` is carried as `f32` degrees for both legacy (formats
/// 0-5, whole-degree resolution) and extended (formats 6-10, 0.006°
/// resolution, full ±180° range) base records -- see
/// [`crate::point::Point3D::scan_angle_rank`].
type BaseFields = (u8, u8, u8, f32, u8, u16, Option<f64>, Option<usize>);

/// Minimum record sizes per point data format ID.
///
/// | Format | Base | GPS time | RGB | NIR | Waveform | Total |
/// |--------|------|----------|-----|-----|----------|-------|
/// | 0      | 20   |          |     |     |          | 20    |
/// | 1      | 20   | 8        |     |     |          | 28    |
/// | 2      | 20   |          | 6   |     |          | 26    |
/// | 3      | 20   | 8        | 6   |     |          | 34    |
/// | 6      | 30   | (incl)   |     |     |          | 30    |
/// | 7      | 30   | (incl)   | 6   |     |          | 36    |
/// | 8      | 30   | (incl)   | 6   | 2   |          | 38    |
/// | 9      | 30   | (incl)   |     |     | 29       | 59    |
/// | 10     | 30   | (incl)   | 6   | 2   | 29       | 67    |
pub fn min_record_size(format_id: u8) -> Result<usize, CopcError> {
    match format_id {
        0 => Ok(20),
        1 => Ok(28),
        2 => Ok(26),
        3 => Ok(34),
        6 => Ok(30),
        7 => Ok(36),
        8 => Ok(38),
        9 => Ok(59),
        10 => Ok(67),
        other => Err(CopcError::InvalidFormat(format!(
            "Unsupported point data format ID: {other}"
        ))),
    }
}

/// Returns `true` when the format uses the extended 30-byte base (formats 6-10).
#[inline]
fn is_extended_format(format_id: u8) -> bool {
    format_id >= 6
}

/// Deserialize a single point record from `record` using the given format,
/// scale factors and offsets.
///
/// # Parameters
/// - `record` -- raw bytes of a single point record (length >= `min_record_size(format_id)`)
/// - `format_id` -- LAS point data record format (0, 1, 2, 3, 6, 7, 8)
/// - `scale` -- `[scale_x, scale_y, scale_z]` from the LAS header
/// - `offset` -- `[offset_x, offset_y, offset_z]` from the LAS header
///
/// # Errors
/// Returns [`CopcError::InvalidFormat`] when `record` is too short or the
/// format ID is unsupported.
pub fn deserialize_point(
    record: &[u8],
    format_id: u8,
    scale: [f64; 3],
    offset: [f64; 3],
) -> Result<Point3D, CopcError> {
    let min_size = min_record_size(format_id)?;
    if record.len() < min_size {
        return Err(CopcError::InvalidFormat(format!(
            "Point record too short for format {format_id}: {} bytes (need >= {min_size})",
            record.len()
        )));
    }

    // X/Y/Z raw i32 LE at bytes 0-11 (same for all formats).
    let raw_x = i32::from_le_bytes([record[0], record[1], record[2], record[3]]);
    let raw_y = i32::from_le_bytes([record[4], record[5], record[6], record[7]]);
    let raw_z = i32::from_le_bytes([record[8], record[9], record[10], record[11]]);

    let x = raw_x as f64 * scale[0] + offset[0];
    let y = raw_y as f64 * scale[1] + offset[1];
    let z = raw_z as f64 * scale[2] + offset[2];

    // Intensity at bytes 12-13 (u16 LE) -- same for all formats.
    let intensity = u16::from_le_bytes([record[12], record[13]]);

    let (
        return_number,
        number_of_returns,
        classification,
        scan_angle_rank,
        user_data,
        point_source_id,
        gps_time,
        rgb_offset,
    ) = if is_extended_format(format_id) {
        parse_extended_base(record, format_id)?
    } else {
        parse_legacy_base(record, format_id)?
    };

    // Parse optional RGB fields based on format and the computed rgb_offset.
    let (red, green, blue) = if let Some(rgb_off) = rgb_offset {
        if record.len() >= rgb_off + 6 {
            let r = u16::from_le_bytes([record[rgb_off], record[rgb_off + 1]]);
            let g = u16::from_le_bytes([record[rgb_off + 2], record[rgb_off + 3]]);
            let b = u16::from_le_bytes([record[rgb_off + 4], record[rgb_off + 5]]);
            (Some(r), Some(g), Some(b))
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };

    // Parse waveform packet for formats 9 and 10.
    // Format 9: waveform at byte 30 (immediately after the 30-byte extended base, no RGB).
    // Format 10: waveform at byte 38 (after 30-byte base + 6 RGB + 2 NIR).
    let waveform = match format_id {
        9 => Some(parse_waveform_packet(record, 30)?),
        10 => Some(parse_waveform_packet(record, 38)?),
        _ => None,
    };

    Ok(Point3D {
        x,
        y,
        z,
        intensity,
        return_number,
        number_of_returns,
        classification,
        scan_angle_rank,
        user_data,
        point_source_id,
        gps_time,
        red,
        green,
        blue,
        waveform,
    })
}

/// Parse fields from legacy base record (formats 0-5, 20-byte base).
///
/// Returns `(return_number, number_of_returns, classification, scan_angle_rank,
///           user_data, point_source_id, gps_time, rgb_offset)`.
fn parse_legacy_base(record: &[u8], format_id: u8) -> Result<BaseFields, CopcError> {
    // Byte 14: bits 0-2 = return_number, bits 3-5 = number_of_returns
    let packed = record[14];
    let return_number = packed & 0x07;
    let number_of_returns = (packed >> 3) & 0x07;

    // Byte 15: classification
    let classification = record[15];

    // Byte 16: scan angle rank (i8, whole degrees), widened to f32 so both
    // base-record parsers share one `Point3D::scan_angle_rank` representation.
    let scan_angle_rank = f32::from(record[16] as i8);

    // Byte 17: user data
    let user_data = record[17];

    // Bytes 18-19: point source ID (u16 LE)
    let point_source_id = u16::from_le_bytes([record[18], record[19]]);

    // GPS time and RGB depend on format
    let (gps_time, rgb_offset) = match format_id {
        0 => (None, None),
        1 => {
            // GPS time at byte 20
            let gps = read_f64_le(record, 20)?;
            (Some(gps), None)
        }
        2 => {
            // RGB at byte 20
            (None, Some(20))
        }
        3 => {
            // GPS time at byte 20, RGB at byte 28
            let gps = read_f64_le(record, 20)?;
            (Some(gps), Some(28))
        }
        _ => {
            return Err(CopcError::InvalidFormat(format!(
                "Unsupported legacy format: {format_id}"
            )));
        }
    };

    Ok((
        return_number,
        number_of_returns,
        classification,
        scan_angle_rank,
        user_data,
        point_source_id,
        gps_time,
        rgb_offset,
    ))
}

/// Parse fields from extended base record (formats 6-10, 30-byte base).
///
/// Returns `(return_number, number_of_returns, classification, scan_angle_rank,
///           user_data, point_source_id, gps_time, rgb_offset)`.
fn parse_extended_base(record: &[u8], format_id: u8) -> Result<BaseFields, CopcError> {
    // Byte 14: bits 0-3 = return_number, bits 4-7 = number_of_returns
    let packed = record[14];
    let return_number = packed & 0x0F;
    let number_of_returns = (packed >> 4) & 0x0F;

    // Byte 16: classification
    let classification = record[16];

    // Byte 17: user data
    let user_data = record[17];

    // Bytes 18-19: scan angle (i16 LE), multiply by 0.006 to get degrees.
    // Full precision (0.006-degree resolution, +/-180-degree range per ASPRS
    // LAS 1.4 R15) is preserved -- no rounding or clamping to the legacy i8
    // whole-degree representation.
    let raw_angle = i16::from_le_bytes([record[18], record[19]]);
    let angle_degrees = f64::from(raw_angle) * 0.006;
    let scan_angle_rank = angle_degrees as f32;

    // Bytes 20-21: point source ID (u16 LE)
    let point_source_id = u16::from_le_bytes([record[20], record[21]]);

    // Bytes 22-29: GPS time (f64 LE) -- always present in formats 6+
    let gps_time = read_f64_le(record, 22)?;

    // RGB offset depends on format
    let rgb_offset = match format_id {
        6 | 9 => None,          // No RGB
        7 | 8 | 10 => Some(30), // RGB at byte 30 (NIR at 36 for format 8/10, Point3D has no NIR field)
        _ => {
            return Err(CopcError::InvalidFormat(format!(
                "Unsupported extended format: {format_id}"
            )));
        }
    };

    Ok((
        return_number,
        number_of_returns,
        classification,
        scan_angle_rank,
        user_data,
        point_source_id,
        Some(gps_time),
        rgb_offset,
    ))
}

/// Deserialize multiple point records from a contiguous byte buffer.
///
/// # Parameters
/// - `data` -- raw bytes containing `count` point records packed end-to-end
/// - `count` -- number of point records to read
/// - `record_length` -- byte size of each record (may be larger than `min_record_size`)
/// - `format_id` -- LAS point data record format
/// - `scale` -- `[scale_x, scale_y, scale_z]`
/// - `offset` -- `[offset_x, offset_y, offset_z]`
///
/// # Errors
/// Returns [`CopcError::InvalidFormat`] when the data is too short or a record
/// is malformed.
pub fn deserialize_points(
    data: &[u8],
    count: usize,
    record_length: usize,
    format_id: u8,
    scale: [f64; 3],
    offset: [f64; 3],
) -> Result<Vec<Point3D>, CopcError> {
    let needed = count.checked_mul(record_length).ok_or_else(|| {
        CopcError::InvalidFormat("Point count * record length overflows usize".into())
    })?;
    if data.len() < needed {
        return Err(CopcError::InvalidFormat(format!(
            "Point data too short: {} bytes (need {needed} for {count} records of {record_length} bytes)",
            data.len()
        )));
    }

    let mut points = Vec::with_capacity(count);
    for i in 0..count {
        let start = i * record_length;
        let end = start + record_length;
        let record = &data[start..end];
        let point = deserialize_point(record, format_id, scale, offset)?;
        points.push(point);
    }
    Ok(points)
}

/// Parse a 29-byte waveform packet starting at `offset` within `record`.
///
/// The waveform packet layout (per ASPRS LAS 1.4 R15 Tables 17-18):
/// - Byte 0      : descriptor_index (u8)
/// - Bytes 1-8   : byte_offset_to_waveform_data (u64 LE)
/// - Bytes 9-12  : waveform_packet_size (u32 LE)
/// - Bytes 13-16 : return_point_waveform_location (f32 LE)
/// - Bytes 17-20 : X(t) parametric displacement (f32 LE)
/// - Bytes 21-24 : Y(t) parametric displacement (f32 LE)
/// - Bytes 25-28 : Z(t) parametric displacement (f32 LE)
fn parse_waveform_packet(record: &[u8], offset: usize) -> Result<WaveformPacket, CopcError> {
    if record.len() < offset + 29 {
        return Err(CopcError::InvalidFormat(format!(
            "Record too short for waveform packet at offset {offset}: need {}, got {}",
            offset + 29,
            record.len()
        )));
    }
    let w = &record[offset..offset + 29];
    Ok(WaveformPacket {
        descriptor_index: w[0],
        byte_offset: u64::from_le_bytes([w[1], w[2], w[3], w[4], w[5], w[6], w[7], w[8]]),
        packet_size: u32::from_le_bytes([w[9], w[10], w[11], w[12]]),
        return_point_loc: f32::from_le_bytes([w[13], w[14], w[15], w[16]]),
        x_t: f32::from_le_bytes([w[17], w[18], w[19], w[20]]),
        y_t: f32::from_le_bytes([w[21], w[22], w[23], w[24]]),
        z_t: f32::from_le_bytes([w[25], w[26], w[27], w[28]]),
    })
}

/// Read an f64 from `data` at byte offset `off` in little-endian order.
#[inline]
fn read_f64_le(data: &[u8], off: usize) -> Result<f64, CopcError> {
    if data.len() < off + 8 {
        return Err(CopcError::InvalidFormat(format!(
            "Cannot read f64 at offset {off}: data too short ({} bytes)",
            data.len()
        )));
    }
    Ok(f64::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
        data[off + 4],
        data[off + 5],
        data[off + 6],
        data[off + 7],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a format-0 point record (20 bytes).
    fn make_format0_record(raw_x: i32, raw_y: i32, raw_z: i32, intensity: u16) -> Vec<u8> {
        let mut rec = vec![0u8; 20];
        rec[0..4].copy_from_slice(&raw_x.to_le_bytes());
        rec[4..8].copy_from_slice(&raw_y.to_le_bytes());
        rec[8..12].copy_from_slice(&raw_z.to_le_bytes());
        rec[12..14].copy_from_slice(&intensity.to_le_bytes());
        // Byte 14: return_number=2 (bits 0-2), number_of_returns=3 (bits 3-5)
        rec[14] = 2 | (3 << 3);
        // Byte 15: classification = 2 (Ground)
        rec[15] = 2;
        // Byte 16: scan_angle_rank = -5
        rec[16] = (-5i8) as u8;
        // Byte 17: user_data = 42
        rec[17] = 42;
        // Bytes 18-19: point_source_id = 7
        rec[18..20].copy_from_slice(&7u16.to_le_bytes());
        rec
    }

    /// Build a format-1 point record (28 bytes = 20 base + 8 GPS).
    fn make_format1_record(raw_x: i32, raw_y: i32, raw_z: i32, gps_time: f64) -> Vec<u8> {
        let mut rec = make_format0_record(raw_x, raw_y, raw_z, 100);
        rec.resize(28, 0);
        rec[20..28].copy_from_slice(&gps_time.to_le_bytes());
        rec
    }

    /// Build a format-2 point record (26 bytes = 20 base + 6 RGB).
    fn make_format2_record(raw_x: i32, raw_y: i32, raw_z: i32, r: u16, g: u16, b: u16) -> Vec<u8> {
        let mut rec = make_format0_record(raw_x, raw_y, raw_z, 200);
        rec.resize(26, 0);
        rec[20..22].copy_from_slice(&r.to_le_bytes());
        rec[22..24].copy_from_slice(&g.to_le_bytes());
        rec[24..26].copy_from_slice(&b.to_le_bytes());
        rec
    }

    /// Build a format-3 point record (34 bytes = 20 base + 8 GPS + 6 RGB).
    fn make_format3_record(
        raw_x: i32,
        raw_y: i32,
        raw_z: i32,
        gps_time: f64,
        r: u16,
        g: u16,
        b: u16,
    ) -> Vec<u8> {
        let mut rec = make_format0_record(raw_x, raw_y, raw_z, 300);
        rec.resize(34, 0);
        rec[20..28].copy_from_slice(&gps_time.to_le_bytes());
        rec[28..30].copy_from_slice(&r.to_le_bytes());
        rec[30..32].copy_from_slice(&g.to_le_bytes());
        rec[32..34].copy_from_slice(&b.to_le_bytes());
        rec
    }

    /// Build a format-6 point record (30 bytes extended base).
    fn make_format6_record(raw_x: i32, raw_y: i32, raw_z: i32, gps_time: f64) -> Vec<u8> {
        let mut rec = vec![0u8; 30];
        rec[0..4].copy_from_slice(&raw_x.to_le_bytes());
        rec[4..8].copy_from_slice(&raw_y.to_le_bytes());
        rec[8..12].copy_from_slice(&raw_z.to_le_bytes());
        rec[12..14].copy_from_slice(&500u16.to_le_bytes()); // intensity
        // Byte 14: return_number=3 (bits 0-3), number_of_returns=5 (bits 4-7)
        rec[14] = 3 | (5 << 4);
        // Byte 15: classification flags
        rec[15] = 0;
        // Byte 16: classification = 6 (Building)
        rec[16] = 6;
        // Byte 17: user_data = 99
        rec[17] = 99;
        // Bytes 18-19: scan angle (i16), e.g. 5000 * 0.006 = 30.0 degrees
        rec[18..20].copy_from_slice(&5000i16.to_le_bytes());
        // Bytes 20-21: point_source_id = 12
        rec[20..22].copy_from_slice(&12u16.to_le_bytes());
        // Bytes 22-29: GPS time
        rec[22..30].copy_from_slice(&gps_time.to_le_bytes());
        rec
    }

    /// Build a format-7 point record (36 bytes = 30 base + 6 RGB).
    fn make_format7_record(
        raw_x: i32,
        raw_y: i32,
        raw_z: i32,
        gps_time: f64,
        r: u16,
        g: u16,
        b: u16,
    ) -> Vec<u8> {
        let mut rec = make_format6_record(raw_x, raw_y, raw_z, gps_time);
        rec.resize(36, 0);
        rec[30..32].copy_from_slice(&r.to_le_bytes());
        rec[32..34].copy_from_slice(&g.to_le_bytes());
        rec[34..36].copy_from_slice(&b.to_le_bytes());
        rec
    }

    /// Build a format-8 point record (38 bytes = 30 base + 6 RGB + 2 NIR).
    fn make_format8_record(
        raw_x: i32,
        raw_y: i32,
        raw_z: i32,
        gps_time: f64,
        r: u16,
        g: u16,
        b: u16,
    ) -> Vec<u8> {
        let mut rec = make_format7_record(raw_x, raw_y, raw_z, gps_time, r, g, b);
        rec.resize(38, 0);
        rec[36..38].copy_from_slice(&1000u16.to_le_bytes()); // NIR
        rec
    }

    // ---- Format 0 ----

    #[test]
    fn test_format0_coordinates() {
        let rec = make_format0_record(1000, 2000, 500, 150);
        let scale = [0.001, 0.001, 0.001];
        let offset = [0.0, 0.0, 0.0];
        let pt = deserialize_point(&rec, 0, scale, offset).expect("format 0 parse");
        assert!((pt.x - 1.0).abs() < 1e-9, "x = 1000 * 0.001 = 1.0");
        assert!((pt.y - 2.0).abs() < 1e-9, "y = 2000 * 0.001 = 2.0");
        assert!((pt.z - 0.5).abs() < 1e-9, "z = 500 * 0.001 = 0.5");
    }

    #[test]
    fn test_format0_with_offset() {
        let rec = make_format0_record(1000, 2000, 500, 150);
        let scale = [0.001, 0.001, 0.001];
        let offset = [100.0, 200.0, 50.0];
        let pt = deserialize_point(&rec, 0, scale, offset).expect("format 0 with offset");
        assert!((pt.x - 101.0).abs() < 1e-9);
        assert!((pt.y - 202.0).abs() < 1e-9);
        assert!((pt.z - 50.5).abs() < 1e-9);
    }

    #[test]
    fn test_format0_intensity() {
        let rec = make_format0_record(0, 0, 0, 150);
        let pt = deserialize_point(&rec, 0, [1.0; 3], [0.0; 3]).expect("format 0");
        assert_eq!(pt.intensity, 150);
    }

    #[test]
    fn test_format0_return_number() {
        let rec = make_format0_record(0, 0, 0, 0);
        let pt = deserialize_point(&rec, 0, [1.0; 3], [0.0; 3]).expect("format 0");
        assert_eq!(pt.return_number, 2);
        assert_eq!(pt.number_of_returns, 3);
    }

    #[test]
    fn test_format0_classification() {
        let rec = make_format0_record(0, 0, 0, 0);
        let pt = deserialize_point(&rec, 0, [1.0; 3], [0.0; 3]).expect("format 0");
        assert_eq!(pt.classification, 2);
    }

    #[test]
    fn test_format0_scan_angle() {
        let rec = make_format0_record(0, 0, 0, 0);
        let pt = deserialize_point(&rec, 0, [1.0; 3], [0.0; 3]).expect("format 0");
        assert_eq!(pt.scan_angle_rank, -5.0);
    }

    #[test]
    fn test_format0_user_data() {
        let rec = make_format0_record(0, 0, 0, 0);
        let pt = deserialize_point(&rec, 0, [1.0; 3], [0.0; 3]).expect("format 0");
        assert_eq!(pt.user_data, 42);
    }

    #[test]
    fn test_format0_point_source_id() {
        let rec = make_format0_record(0, 0, 0, 0);
        let pt = deserialize_point(&rec, 0, [1.0; 3], [0.0; 3]).expect("format 0");
        assert_eq!(pt.point_source_id, 7);
    }

    #[test]
    fn test_format0_no_gps_no_color() {
        let rec = make_format0_record(0, 0, 0, 0);
        let pt = deserialize_point(&rec, 0, [1.0; 3], [0.0; 3]).expect("format 0");
        assert!(pt.gps_time.is_none());
        assert!(pt.red.is_none());
        assert!(pt.green.is_none());
        assert!(pt.blue.is_none());
    }

    #[test]
    fn test_format0_too_short() {
        let rec = vec![0u8; 19]; // need 20
        assert!(deserialize_point(&rec, 0, [1.0; 3], [0.0; 3]).is_err());
    }

    // ---- Format 1 ----

    #[test]
    fn test_format1_gps_time() {
        let rec = make_format1_record(0, 0, 0, 12345.678);
        let pt = deserialize_point(&rec, 1, [1.0; 3], [0.0; 3]).expect("format 1");
        let gps = pt.gps_time.expect("format 1 should have GPS time");
        assert!((gps - 12345.678).abs() < 1e-6);
    }

    #[test]
    fn test_format1_no_color() {
        let rec = make_format1_record(0, 0, 0, 0.0);
        let pt = deserialize_point(&rec, 1, [1.0; 3], [0.0; 3]).expect("format 1");
        assert!(pt.red.is_none());
    }

    // ---- Format 2 ----

    #[test]
    fn test_format2_rgb() {
        let rec = make_format2_record(0, 0, 0, 255, 128, 64);
        let pt = deserialize_point(&rec, 2, [1.0; 3], [0.0; 3]).expect("format 2");
        assert_eq!(pt.red, Some(255));
        assert_eq!(pt.green, Some(128));
        assert_eq!(pt.blue, Some(64));
    }

    #[test]
    fn test_format2_no_gps() {
        let rec = make_format2_record(0, 0, 0, 0, 0, 0);
        let pt = deserialize_point(&rec, 2, [1.0; 3], [0.0; 3]).expect("format 2");
        assert!(pt.gps_time.is_none());
    }

    // ---- Format 3 ----

    #[test]
    fn test_format3_gps_and_rgb() {
        let rec = make_format3_record(1000, 2000, 3000, 99.9, 1000, 2000, 3000);
        let pt = deserialize_point(&rec, 3, [0.01, 0.01, 0.01], [0.0; 3]).expect("format 3");
        assert!((pt.x - 10.0).abs() < 1e-9);
        let gps = pt.gps_time.expect("format 3 should have GPS");
        assert!((gps - 99.9).abs() < 1e-6);
        assert_eq!(pt.red, Some(1000));
        assert_eq!(pt.green, Some(2000));
        assert_eq!(pt.blue, Some(3000));
    }

    // ---- Format 6 ----

    #[test]
    fn test_format6_extended_base() {
        let rec = make_format6_record(10000, 20000, 5000, 42.5);
        let pt = deserialize_point(&rec, 6, [0.001, 0.001, 0.001], [0.0; 3]).expect("format 6");
        assert!((pt.x - 10.0).abs() < 1e-9);
        assert!((pt.y - 20.0).abs() < 1e-9);
        assert!((pt.z - 5.0).abs() < 1e-9);
        assert_eq!(pt.intensity, 500);
        assert_eq!(pt.return_number, 3);
        assert_eq!(pt.number_of_returns, 5);
        assert_eq!(pt.classification, 6);
        assert_eq!(pt.user_data, 99);
        assert_eq!(pt.point_source_id, 12);
        let gps = pt.gps_time.expect("format 6 always has GPS");
        assert!((gps - 42.5).abs() < 1e-9);
    }

    #[test]
    fn test_format6_scan_angle_conversion() {
        let rec = make_format6_record(0, 0, 0, 0.0);
        // scan angle = 5000 * 0.006 = 30.0 degrees
        let pt = deserialize_point(&rec, 6, [1.0; 3], [0.0; 3]).expect("format 6");
        assert!((pt.scan_angle_rank - 30.0).abs() < 1e-4);
    }

    #[test]
    fn test_format6_scan_angle_negative() {
        let mut rec = make_format6_record(0, 0, 0, 0.0);
        // Set scan angle to -5000 -> -30.0 degrees
        rec[18..20].copy_from_slice(&(-5000i16).to_le_bytes());
        let pt = deserialize_point(&rec, 6, [1.0; 3], [0.0; 3]).expect("format 6 neg angle");
        assert!((pt.scan_angle_rank - (-30.0)).abs() < 1e-4);
    }

    #[test]
    fn test_format6_scan_angle_sub_degree_precision() {
        // 0.006-degree resolution: raw 1 -> 0.006 degrees, which would have
        // rounded to 0 under the old i8-truncating conversion. Verify the
        // full-precision f32 representation preserves it.
        let mut rec = make_format6_record(0, 0, 0, 0.0);
        rec[18..20].copy_from_slice(&1i16.to_le_bytes());
        let pt = deserialize_point(&rec, 6, [1.0; 3], [0.0; 3]).expect("format 6 sub-degree");
        assert!((pt.scan_angle_rank - 0.006).abs() < 1e-4);
        assert_ne!(
            pt.scan_angle_rank, 0.0,
            "sub-degree angle must not truncate to zero"
        );
    }

    #[test]
    fn test_format6_scan_angle_full_range_not_clamped() {
        // Extended-format scan angle spans +/-180 degrees at 0.006-degree
        // resolution (ASPRS LAS 1.4 R15). i16::MAX * 0.006 = 196.602 degrees;
        // formerly this was rounded and clamped to the legacy i8 range
        // (-128..127), silently corrupting any angle beyond +/-127 degrees.
        // The widened f32 field must retain the true value instead.
        let mut rec = make_format6_record(0, 0, 0, 0.0);
        rec[18..20].copy_from_slice(&i16::MAX.to_le_bytes());
        let pt = deserialize_point(&rec, 6, [1.0; 3], [0.0; 3]).expect("format 6 full range");
        assert!((pt.scan_angle_rank - 196.602).abs() < 1e-2);
        assert!(
            pt.scan_angle_rank > 127.0,
            "scan angle beyond +/-127 degrees must not be clamped, got {}",
            pt.scan_angle_rank
        );
    }

    #[test]
    fn test_format6_no_color() {
        let rec = make_format6_record(0, 0, 0, 0.0);
        let pt = deserialize_point(&rec, 6, [1.0; 3], [0.0; 3]).expect("format 6");
        assert!(pt.red.is_none());
    }

    // ---- Format 7 ----

    #[test]
    fn test_format7_gps_and_rgb() {
        let rec = make_format7_record(1000, 2000, 3000, 77.7, 500, 600, 700);
        let pt = deserialize_point(&rec, 7, [0.001; 3], [0.0; 3]).expect("format 7");
        let gps = pt.gps_time.expect("format 7 always has GPS");
        assert!((gps - 77.7).abs() < 1e-9);
        assert_eq!(pt.red, Some(500));
        assert_eq!(pt.green, Some(600));
        assert_eq!(pt.blue, Some(700));
    }

    // ---- Format 8 ----

    #[test]
    fn test_format8_gps_rgb_nir_ignored() {
        let rec = make_format8_record(0, 0, 0, 55.5, 11, 22, 33);
        let pt = deserialize_point(&rec, 8, [1.0; 3], [0.0; 3]).expect("format 8");
        let gps = pt.gps_time.expect("format 8 always has GPS");
        assert!((gps - 55.5).abs() < 1e-9);
        assert_eq!(pt.red, Some(11));
        assert_eq!(pt.green, Some(22));
        assert_eq!(pt.blue, Some(33));
        // NIR is silently ignored (no field in Point3D)
    }

    // ---- min_record_size ----

    #[test]
    fn test_min_record_size_all_formats() {
        assert_eq!(min_record_size(0).expect("f0"), 20);
        assert_eq!(min_record_size(1).expect("f1"), 28);
        assert_eq!(min_record_size(2).expect("f2"), 26);
        assert_eq!(min_record_size(3).expect("f3"), 34);
        assert_eq!(min_record_size(6).expect("f6"), 30);
        assert_eq!(min_record_size(7).expect("f7"), 36);
        assert_eq!(min_record_size(8).expect("f8"), 38);
        assert_eq!(min_record_size(9).expect("f9"), 59);
        assert_eq!(min_record_size(10).expect("f10"), 67);
    }

    #[test]
    fn test_min_record_size_unsupported() {
        assert!(min_record_size(4).is_err());
        assert!(min_record_size(5).is_err());
        assert!(min_record_size(11).is_err());
        assert!(min_record_size(255).is_err());
    }

    // ---- deserialize_points (batch) ----

    #[test]
    fn test_deserialize_points_batch() {
        let rec1 = make_format0_record(1000, 2000, 3000, 100);
        let rec2 = make_format0_record(4000, 5000, 6000, 200);
        let mut data = rec1;
        data.extend_from_slice(&rec2);
        let pts = deserialize_points(&data, 2, 20, 0, [0.001; 3], [0.0; 3]).expect("batch");
        assert_eq!(pts.len(), 2);
        assert!((pts[0].x - 1.0).abs() < 1e-9);
        assert!((pts[1].x - 4.0).abs() < 1e-9);
    }

    #[test]
    fn test_deserialize_points_batch_too_short() {
        let data = vec![0u8; 30]; // only 30 bytes, not enough for 2 records of 20
        assert!(deserialize_points(&data, 2, 20, 0, [1.0; 3], [0.0; 3]).is_err());
    }

    #[test]
    fn test_deserialize_points_zero_count() {
        let pts = deserialize_points(&[], 0, 20, 0, [1.0; 3], [0.0; 3]).expect("zero");
        assert!(pts.is_empty());
    }

    #[test]
    fn test_format0_negative_coordinates() {
        let rec = make_format0_record(-5000, -10000, -500, 0);
        let pt = deserialize_point(&rec, 0, [0.001; 3], [0.0; 3]).expect("negative coords");
        assert!((pt.x - (-5.0)).abs() < 1e-9);
        assert!((pt.y - (-10.0)).abs() < 1e-9);
        assert!((pt.z - (-0.5)).abs() < 1e-9);
    }

    #[test]
    fn test_unsupported_format_id() {
        let rec = vec![0u8; 50];
        assert!(deserialize_point(&rec, 4, [1.0; 3], [0.0; 3]).is_err());
        assert!(deserialize_point(&rec, 5, [1.0; 3], [0.0; 3]).is_err());
        assert!(deserialize_point(&rec, 11, [1.0; 3], [0.0; 3]).is_err());
    }

    #[test]
    fn test_format6_extended_return_numbers_up_to_15() {
        let mut rec = make_format6_record(0, 0, 0, 0.0);
        // Return number 15, number of returns 15
        rec[14] = 15 | (15 << 4);
        let pt = deserialize_point(&rec, 6, [1.0; 3], [0.0; 3]).expect("format 6 max returns");
        assert_eq!(pt.return_number, 15);
        assert_eq!(pt.number_of_returns, 15);
    }

    #[test]
    fn test_deserialize_points_with_record_length_padding() {
        // Record length 24 but format 0 only needs 20 -- 4 bytes padding
        let mut data = make_format0_record(1000, 0, 0, 0);
        data.extend_from_slice(&[0u8; 4]); // padding
        let mut data2 = make_format0_record(2000, 0, 0, 0);
        data2.extend_from_slice(&[0u8; 4]); // padding
        data.extend_from_slice(&data2);
        let pts =
            deserialize_points(&data, 2, 24, 0, [0.001; 3], [0.0; 3]).expect("padded records");
        assert_eq!(pts.len(), 2);
        assert!((pts[0].x - 1.0).abs() < 1e-9);
        assert!((pts[1].x - 2.0).abs() < 1e-9);
    }
}
