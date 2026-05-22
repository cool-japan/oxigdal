//! LASzip Item Compressor v1 decoders for LAS Point Formats 0 and 1.
//!
//! Format 0 is the 20-byte legacy point.  Format 1 adds an 8-byte GPS time
//! suffix for a total of 28 bytes per record.
//!
//! Chunk wire layout (v1):
//!
//! 1. **Raw first point** — emitted by the encoder as raw bytes (size of one
//!    record).  This seeds the predictor contexts.
//! 2. **Arithmetic-coded body** — `point_count - 1` predicted point deltas
//!    sharing a single Range Coder stream.
//!
//! Reference: `LASzip/src/lasreaditemcompressed_v1.cpp`,
//! `laswriteitemcompressed_v1.cpp`.

use crate::error::CopcError;
#[cfg(any(test, feature = "laz-encoder"))]
use crate::laz::arithmetic::ArithmeticEncoder;
use crate::laz::arithmetic::{ArithmeticDecoder, IntegerCompressor, SymbolModel};
use crate::laz::predictors::{
    PointXYZContext, decode_classification_v1, decode_gps_time_v1, decode_intensity_v1,
    decode_point_source_id_v1, decode_return_flags_v1, decode_scan_angle_rank_v1,
    decode_user_data_v1, decode_xyz_v1, make_classification_models,
};

/// Record size of LAS point format 0 (20 bytes).
pub const PF0_RECORD_SIZE: usize = 20;
/// Record size of LAS point format 1 (28 bytes — PF0 + 8-byte GPS time).
pub const PF1_RECORD_SIZE: usize = 28;

/// Per-format scratch state carried across the chunk.
struct Format0State {
    xyz_ic: IntegerCompressor,
    xyz_ctx: PointXYZContext,
    intensity_model: SymbolModel,
    return_flags_model: SymbolModel,
    classification_models: Vec<SymbolModel>,
    scan_angle_model: SymbolModel,
    user_data_model: SymbolModel,
    point_source_id_model: SymbolModel,
    last_intensity: u16,
    last_classification: u8,
    last_scan_angle: i8,
    last_user_data: u8,
    last_point_source_id: u16,
}

impl Format0State {
    fn new(seed_record: &[u8]) -> Result<Self, CopcError> {
        if seed_record.len() < PF0_RECORD_SIZE {
            return Err(CopcError::LazDecoderError(format!(
                "PF0 seed record too short: {} bytes (need {})",
                seed_record.len(),
                PF0_RECORD_SIZE
            )));
        }
        let raw_x = i32::from_le_bytes([
            seed_record[0],
            seed_record[1],
            seed_record[2],
            seed_record[3],
        ]);
        let raw_y = i32::from_le_bytes([
            seed_record[4],
            seed_record[5],
            seed_record[6],
            seed_record[7],
        ]);
        let raw_z = i32::from_le_bytes([
            seed_record[8],
            seed_record[9],
            seed_record[10],
            seed_record[11],
        ]);
        let intensity = u16::from_le_bytes([seed_record[12], seed_record[13]]);
        // byte 14 = return flags
        let classification = seed_record[15];
        let scan_angle_rank = seed_record[16] as i8;
        let user_data = seed_record[17];
        let point_source_id = u16::from_le_bytes([seed_record[18], seed_record[19]]);

        Ok(Self {
            xyz_ic: IntegerCompressor::new(32, 3),
            xyz_ctx: PointXYZContext::with_seed(raw_x, raw_y, raw_z),
            intensity_model: SymbolModel::new(256),
            return_flags_model: SymbolModel::new(256),
            classification_models: make_classification_models(),
            scan_angle_model: SymbolModel::new(256),
            user_data_model: SymbolModel::new(256),
            point_source_id_model: SymbolModel::new(256),
            last_intensity: intensity,
            last_classification: classification,
            last_scan_angle: scan_angle_rank,
            last_user_data: user_data,
            last_point_source_id: point_source_id,
        })
    }

    /// Decode a single record into a 20-byte buffer.
    fn decode_one(&mut self, decoder: &mut ArithmeticDecoder<'_>, out: &mut [u8]) {
        let (x, y, z) = decode_xyz_v1(&mut self.xyz_ic, decoder, &mut self.xyz_ctx);
        out[0..4].copy_from_slice(&x.to_le_bytes());
        out[4..8].copy_from_slice(&y.to_le_bytes());
        out[8..12].copy_from_slice(&z.to_le_bytes());

        let intensity =
            decode_intensity_v1(decoder, &mut self.intensity_model, self.last_intensity);
        out[12..14].copy_from_slice(&intensity.to_le_bytes());
        self.last_intensity = intensity;

        let return_flags = decode_return_flags_v1(decoder, &mut self.return_flags_model);
        out[14] = return_flags;

        let classification = decode_classification_v1(
            decoder,
            &mut self.classification_models,
            self.last_classification,
        );
        out[15] = classification;
        self.last_classification = classification;

        let scan_angle =
            decode_scan_angle_rank_v1(decoder, &mut self.scan_angle_model, self.last_scan_angle);
        out[16] = scan_angle as u8;
        self.last_scan_angle = scan_angle;

        let user_data =
            decode_user_data_v1(decoder, &mut self.user_data_model, self.last_user_data);
        out[17] = user_data;
        self.last_user_data = user_data;

        let psid = decode_point_source_id_v1(
            decoder,
            &mut self.point_source_id_model,
            self.last_point_source_id,
        );
        out[18..20].copy_from_slice(&psid.to_le_bytes());
        self.last_point_source_id = psid;
    }
}

/// Encoder companion to [`Format0State`] (test-only).
#[cfg(any(test, feature = "laz-encoder"))]
struct Format0Encoder {
    xyz_ic: IntegerCompressor,
    xyz_ctx: PointXYZContext,
    intensity_model: SymbolModel,
    return_flags_model: SymbolModel,
    classification_models: Vec<SymbolModel>,
    scan_angle_model: SymbolModel,
    user_data_model: SymbolModel,
    point_source_id_model: SymbolModel,
    last_intensity: u16,
    last_classification: u8,
    last_scan_angle: i8,
    last_user_data: u8,
    last_point_source_id: u16,
}

#[cfg(any(test, feature = "laz-encoder"))]
impl Format0Encoder {
    fn new(seed_record: &[u8]) -> Self {
        let raw_x = i32::from_le_bytes([
            seed_record[0],
            seed_record[1],
            seed_record[2],
            seed_record[3],
        ]);
        let raw_y = i32::from_le_bytes([
            seed_record[4],
            seed_record[5],
            seed_record[6],
            seed_record[7],
        ]);
        let raw_z = i32::from_le_bytes([
            seed_record[8],
            seed_record[9],
            seed_record[10],
            seed_record[11],
        ]);
        let intensity = u16::from_le_bytes([seed_record[12], seed_record[13]]);
        let classification = seed_record[15];
        let scan_angle = seed_record[16] as i8;
        let user_data = seed_record[17];
        let psid = u16::from_le_bytes([seed_record[18], seed_record[19]]);
        Self {
            xyz_ic: IntegerCompressor::new(32, 3),
            xyz_ctx: PointXYZContext::with_seed(raw_x, raw_y, raw_z),
            intensity_model: SymbolModel::new(256),
            return_flags_model: SymbolModel::new(256),
            classification_models: make_classification_models(),
            scan_angle_model: SymbolModel::new(256),
            user_data_model: SymbolModel::new(256),
            point_source_id_model: SymbolModel::new(256),
            last_intensity: intensity,
            last_classification: classification,
            last_scan_angle: scan_angle,
            last_user_data: user_data,
            last_point_source_id: psid,
        }
    }

    fn encode_one(&mut self, encoder: &mut ArithmeticEncoder, record: &[u8]) {
        use crate::laz::predictors::{
            encode_classification_v1, encode_intensity_v1, encode_point_source_id_v1,
            encode_return_flags_v1, encode_scan_angle_rank_v1, encode_user_data_v1, encode_xyz_v1,
        };
        let x = i32::from_le_bytes([record[0], record[1], record[2], record[3]]);
        let y = i32::from_le_bytes([record[4], record[5], record[6], record[7]]);
        let z = i32::from_le_bytes([record[8], record[9], record[10], record[11]]);
        encode_xyz_v1(&mut self.xyz_ic, encoder, &mut self.xyz_ctx, x, y, z);

        let intensity = u16::from_le_bytes([record[12], record[13]]);
        encode_intensity_v1(
            encoder,
            &mut self.intensity_model,
            self.last_intensity,
            intensity,
        );
        self.last_intensity = intensity;

        let return_flags = record[14];
        encode_return_flags_v1(encoder, &mut self.return_flags_model, return_flags);

        let classification = record[15];
        encode_classification_v1(
            encoder,
            &mut self.classification_models,
            self.last_classification,
            classification,
        );
        self.last_classification = classification;

        let scan_angle = record[16] as i8;
        encode_scan_angle_rank_v1(
            encoder,
            &mut self.scan_angle_model,
            self.last_scan_angle,
            scan_angle,
        );
        self.last_scan_angle = scan_angle;

        let user_data = record[17];
        encode_user_data_v1(
            encoder,
            &mut self.user_data_model,
            self.last_user_data,
            user_data,
        );
        self.last_user_data = user_data;

        let psid = u16::from_le_bytes([record[18], record[19]]);
        encode_point_source_id_v1(
            encoder,
            &mut self.point_source_id_model,
            self.last_point_source_id,
            psid,
        );
        self.last_point_source_id = psid;
    }
}

/// Decompress a LASzip v1 chunk encoded in point format 0.
///
/// `compressed` layout:
/// - First 20 bytes: raw seed record.
/// - Remaining bytes: arithmetic-coded body for the remaining `point_count - 1`
///   records.
///
/// # Errors
/// Returns [`CopcError::LazDecoderError`] when the input is truncated, the
/// seed record is too small, or the arithmetic decoder cannot initialize.
pub fn decompress_format_0(compressed: &[u8], point_count: usize) -> Result<Vec<u8>, CopcError> {
    if point_count == 0 {
        return Ok(Vec::new());
    }
    if compressed.len() < PF0_RECORD_SIZE {
        return Err(CopcError::LazDecoderError(format!(
            "PF0 chunk truncated: {} bytes (need >= {} for seed record)",
            compressed.len(),
            PF0_RECORD_SIZE
        )));
    }

    let mut out = Vec::with_capacity(point_count * PF0_RECORD_SIZE);
    // Copy raw seed record.
    out.extend_from_slice(&compressed[..PF0_RECORD_SIZE]);

    if point_count == 1 {
        return Ok(out);
    }

    let mut state = Format0State::new(&compressed[..PF0_RECORD_SIZE])?;
    let mut decoder = ArithmeticDecoder::new(&compressed[PF0_RECORD_SIZE..])?;

    let mut buf = [0u8; PF0_RECORD_SIZE];
    for _ in 1..point_count {
        state.decode_one(&mut decoder, &mut buf);
        out.extend_from_slice(&buf);
    }
    Ok(out)
}

/// Encoder companion of [`decompress_format_0`] (test-only).
///
/// `records` must be tightly packed at `PF0_RECORD_SIZE` bytes per record and
/// contain at least one record.
#[cfg(any(test, feature = "laz-encoder"))]
pub fn compress_format_0(records: &[u8], point_count: usize) -> Vec<u8> {
    let mut out = Vec::new();
    if point_count == 0 {
        return out;
    }
    out.extend_from_slice(&records[..PF0_RECORD_SIZE]);
    if point_count == 1 {
        return out;
    }
    let mut state = Format0Encoder::new(&records[..PF0_RECORD_SIZE]);
    let mut encoder = ArithmeticEncoder::new();
    for i in 1..point_count {
        let start = i * PF0_RECORD_SIZE;
        let end = start + PF0_RECORD_SIZE;
        state.encode_one(&mut encoder, &records[start..end]);
    }
    let encoded = encoder.done();
    out.extend_from_slice(&encoded);
    out
}

/// Decompress a LASzip v1 chunk encoded in point format 1 (PF0 + GPS time).
///
/// `compressed` layout:
/// - First 28 bytes: raw seed record (PF0 fields + f64 LE GPS time).
/// - Remaining bytes: arithmetic-coded body.
///
/// # Errors
/// Returns [`CopcError::LazDecoderError`] on malformed input.
pub fn decompress_format_1(compressed: &[u8], point_count: usize) -> Result<Vec<u8>, CopcError> {
    if point_count == 0 {
        return Ok(Vec::new());
    }
    if compressed.len() < PF1_RECORD_SIZE {
        return Err(CopcError::LazDecoderError(format!(
            "PF1 chunk truncated: {} bytes (need >= {} for seed record)",
            compressed.len(),
            PF1_RECORD_SIZE
        )));
    }

    let mut out = Vec::with_capacity(point_count * PF1_RECORD_SIZE);
    out.extend_from_slice(&compressed[..PF1_RECORD_SIZE]);
    if point_count == 1 {
        return Ok(out);
    }

    let mut state = Format0State::new(&compressed[..PF0_RECORD_SIZE])?;
    let mut gps_ic = IntegerCompressor::new(32, 2);
    let mut last_gps_bits = f64::from_le_bytes([
        compressed[20],
        compressed[21],
        compressed[22],
        compressed[23],
        compressed[24],
        compressed[25],
        compressed[26],
        compressed[27],
    ])
    .to_bits() as i64;

    let mut decoder = ArithmeticDecoder::new(&compressed[PF1_RECORD_SIZE..])?;
    let mut record_buf = [0u8; PF1_RECORD_SIZE];
    for _ in 1..point_count {
        // Decode base 20 bytes.
        state.decode_one(&mut decoder, &mut record_buf[..PF0_RECORD_SIZE]);
        // Decode GPS time.
        let gps = decode_gps_time_v1(&mut gps_ic, &mut decoder, &mut last_gps_bits);
        record_buf[20..28].copy_from_slice(&gps.to_le_bytes());
        out.extend_from_slice(&record_buf);
    }
    Ok(out)
}

/// Encoder companion of [`decompress_format_1`] (test-only).
#[cfg(any(test, feature = "laz-encoder"))]
pub fn compress_format_1(records: &[u8], point_count: usize) -> Vec<u8> {
    use crate::laz::predictors::encode_gps_time_v1;
    let mut out = Vec::new();
    if point_count == 0 {
        return out;
    }
    out.extend_from_slice(&records[..PF1_RECORD_SIZE]);
    if point_count == 1 {
        return out;
    }
    let mut state = Format0Encoder::new(&records[..PF0_RECORD_SIZE]);
    let mut gps_ic = IntegerCompressor::new(32, 2);
    let mut last_gps_bits = f64::from_le_bytes([
        records[20],
        records[21],
        records[22],
        records[23],
        records[24],
        records[25],
        records[26],
        records[27],
    ])
    .to_bits() as i64;
    let mut encoder = ArithmeticEncoder::new();
    for i in 1..point_count {
        let start = i * PF1_RECORD_SIZE;
        let end_pf0 = start + PF0_RECORD_SIZE;
        state.encode_one(&mut encoder, &records[start..end_pf0]);
        let gps = f64::from_le_bytes([
            records[end_pf0],
            records[end_pf0 + 1],
            records[end_pf0 + 2],
            records[end_pf0 + 3],
            records[end_pf0 + 4],
            records[end_pf0 + 5],
            records[end_pf0 + 6],
            records[end_pf0 + 7],
        ]);
        encode_gps_time_v1(&mut gps_ic, &mut encoder, &mut last_gps_bits, gps);
    }
    let encoded = encoder.done();
    out.extend_from_slice(&encoded);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pf0_record(x: i32, y: i32, z: i32, intensity: u16, classification: u8) -> Vec<u8> {
        let mut r = vec![0u8; PF0_RECORD_SIZE];
        r[0..4].copy_from_slice(&x.to_le_bytes());
        r[4..8].copy_from_slice(&y.to_le_bytes());
        r[8..12].copy_from_slice(&z.to_le_bytes());
        r[12..14].copy_from_slice(&intensity.to_le_bytes());
        r[14] = 1 | (1 << 3);
        r[15] = classification;
        r
    }

    fn pf1_record(x: i32, y: i32, z: i32, intensity: u16, classification: u8, gps: f64) -> Vec<u8> {
        let mut r = pf0_record(x, y, z, intensity, classification);
        r.resize(PF1_RECORD_SIZE, 0);
        r[20..28].copy_from_slice(&gps.to_le_bytes());
        r
    }

    #[test]
    fn test_decompress_format_0_5_points_round_trip() {
        // Build 5 PF0 records.
        let mut records = Vec::new();
        records.extend_from_slice(&pf0_record(100, 200, 50, 100, 2));
        records.extend_from_slice(&pf0_record(110, 210, 55, 100, 2));
        records.extend_from_slice(&pf0_record(120, 220, 60, 105, 2));
        records.extend_from_slice(&pf0_record(130, 230, 65, 110, 5));
        records.extend_from_slice(&pf0_record(140, 240, 70, 115, 5));

        let compressed = compress_format_0(&records, 5);
        let decompressed = decompress_format_0(&compressed, 5).expect("decompress");
        assert_eq!(decompressed.len(), 5 * PF0_RECORD_SIZE);
        for i in 0..5 {
            let start = i * PF0_RECORD_SIZE;
            let end = start + PF0_RECORD_SIZE;
            assert_eq!(
                &decompressed[start..end],
                &records[start..end],
                "record {i} differs"
            );
        }
    }

    #[test]
    fn test_decompress_format_1_includes_gps_time() {
        let mut records = Vec::new();
        records.extend_from_slice(&pf1_record(100, 200, 50, 100, 2, 1.0));
        records.extend_from_slice(&pf1_record(105, 205, 55, 100, 2, 1.1));
        records.extend_from_slice(&pf1_record(110, 210, 60, 105, 2, 1.2));

        let compressed = compress_format_1(&records, 3);
        let decompressed = decompress_format_1(&compressed, 3).expect("decompress");
        assert_eq!(decompressed.len(), 3 * PF1_RECORD_SIZE);
        for i in 0..3 {
            let start = i * PF1_RECORD_SIZE;
            let end = start + PF1_RECORD_SIZE;
            assert_eq!(
                &decompressed[start..end],
                &records[start..end],
                "record {i} differs"
            );
        }
    }
}
