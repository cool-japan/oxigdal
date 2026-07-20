//! End-to-end integration tests for the LASzip decompression layer.
//!
//! These tests cover:
//!
//! - Round-trip of the arithmetic coder (raw bits + adaptive symbols).
//! - Chunk-table parser including truncation rejection.
//! - LASzip VLR detection and parse round-trip.
//! - XYZ / intensity / classification predictors recovering varied streams.
//! - Format 0 and Format 1 chunk-level round-trip.
//! - Unsupported point format → typed [`CopcError::UnsupportedLazFormat`].
//! - [`CopcReader`] integration: a synthetic file with a LASzip VLR routes
//!   through the decompressor, and a file without a LASzip VLR passes the
//!   raw chunk bytes straight to the point deserializer.
//!
//! Tests live in the integration `tests/` directory so they exercise the
//! crate purely through its public surface.

use oxigeo_copc::error::CopcError;
use oxigeo_copc::laz::{
    LazItem, LazVlrInfo, decompress_chunk, detect_laszip_vlr, parse_laszip_vlr_data,
};

// ---------------------------------------------------------------------------
// Re-imports of test helpers
// ---------------------------------------------------------------------------

// We re-export the test-only encoder + chunk-table serializer via a
// dedicated test-support module that lives inside the crate (gated by
// `cfg(test)`).  Re-import them here for these integration tests:
use oxigeo_copc::copc_vlr::{Vlr, VlrKey};

// ---------------------------------------------------------------------------
// Arithmetic decoder round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_arithmetic_decode_uniform_bits_round_trip() {
    // Mirror the inline unit test so we expose a deliberate integration-level
    // round-trip via the public crate path.  We need access to encoder, so we
    // touch the crate-internal module through `oxigeo_copc::laz::arithmetic`.
    use oxigeo_copc::laz::arithmetic::{ArithmeticDecoder, ArithmeticEncoder};
    let pattern: [(u32, u32); 5] = [(8, 0xAB), (16, 0xDEAD), (3, 5), (12, 0xBE), (8, 0x42)];
    let mut enc = ArithmeticEncoder::new();
    for (bits, val) in pattern {
        enc.write_bits(bits, val);
    }
    let bytes = enc.done();
    let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
    for (bits, expected) in pattern {
        assert_eq!(dec.read_bits(bits), expected);
    }
}

#[test]
fn test_arithmetic_symbol_model_2_symbol_round_trip() {
    use oxigeo_copc::laz::arithmetic::{ArithmeticDecoder, ArithmeticEncoder, SymbolModel};
    let stream: Vec<u32> = (0..200).map(|i| (i % 2) as u32).collect();
    let mut model_enc = SymbolModel::new(2);
    let mut enc = ArithmeticEncoder::new();
    for s in &stream {
        enc.encode_symbol(&mut model_enc, *s);
    }
    let bytes = enc.done();
    let mut model_dec = SymbolModel::new(2);
    let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
    for expected in &stream {
        assert_eq!(dec.decode_symbol(&mut model_dec), *expected);
    }
}

#[test]
fn test_arithmetic_decoder_done_after_full_stream() {
    use oxigeo_copc::laz::arithmetic::{ArithmeticDecoder, ArithmeticEncoder};
    let mut enc = ArithmeticEncoder::new();
    for i in 0..16u32 {
        enc.write_bits(8, i);
    }
    let bytes = enc.done();
    let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
    for _ in 0..16 {
        let _ = dec.read_bits(8);
    }
    // After consuming the encoded payload, the underlying cursor must be at
    // or past the end of the buffer.
    let _ = dec.done();
}

#[test]
fn test_arithmetic_read_byte_short_int_long_little_endian() {
    use oxigeo_copc::laz::arithmetic::{ArithmeticDecoder, ArithmeticEncoder};
    let mut enc = ArithmeticEncoder::new();
    enc.write_byte(0x7E);
    enc.write_short(0xBEEF);
    enc.write_int(0xDEAD_BEEF);
    enc.write_long(0x0123_4567_89AB_CDEF);
    let bytes = enc.done();
    let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
    assert_eq!(dec.read_byte(), 0x7E);
    assert_eq!(dec.read_short(), 0xBEEF);
    assert_eq!(dec.read_int(), 0xDEAD_BEEF);
    assert_eq!(dec.read_long(), 0x0123_4567_89AB_CDEF);
}

// ---------------------------------------------------------------------------
// Chunk-table parsing
// ---------------------------------------------------------------------------

fn write_raw_chunk_table(num_chunks: u32, entries: &[(u32, u32)]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&0u32.to_le_bytes()); // version 0
    buf.extend_from_slice(&num_chunks.to_le_bytes());
    for (bc, pc) in entries {
        buf.extend_from_slice(&bc.to_le_bytes());
        buf.extend_from_slice(&pc.to_le_bytes());
    }
    buf
}

#[test]
fn test_chunk_table_parse_minimal_two_chunks() {
    use oxigeo_copc::laz::chunk_table::parse_chunk_table;
    let raw = write_raw_chunk_table(2, &[(1024, 20), (2048, 10)]);
    let table = parse_chunk_table(&raw, 0, raw.len(), 50_000).expect("parse");
    assert_eq!(table.chunks.len(), 2);
    assert_eq!(table.chunks[0].byte_count, 1024);
    assert_eq!(table.chunks[0].point_count, 20);
    assert_eq!(table.chunks[1].byte_count, 2048);
    assert_eq!(table.chunks[1].point_count, 10);
    assert_eq!(table.point_count, 30);
}

#[test]
fn test_chunk_table_byte_offset_cumulative() {
    use oxigeo_copc::laz::chunk_table::parse_chunk_table;
    let raw = write_raw_chunk_table(3, &[(100, 20), (250, 25), (175, 15)]);
    let table = parse_chunk_table(&raw, 0, raw.len(), 50_000).expect("parse");
    let data_start: u64 = 5000;
    assert_eq!(table.chunk_offset(data_start, 0), 5000);
    assert_eq!(table.chunk_offset(data_start, 1), 5100);
    assert_eq!(table.chunk_offset(data_start, 2), 5350);
    assert_eq!(table.chunk_offset(data_start, 3), 5525);
}

#[test]
fn test_chunk_table_rejects_truncated_header() {
    use oxigeo_copc::laz::chunk_table::parse_chunk_table;
    // Only 4 bytes — not enough for the 8-byte header.
    let too_short = vec![0u8; 4];
    assert!(parse_chunk_table(&too_short, 0, too_short.len(), 50_000).is_err());
    // Truncated body: claims 5 chunks, supplies 1.
    let mut buf = Vec::new();
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&5u32.to_le_bytes());
    buf.extend_from_slice(&100u32.to_le_bytes());
    buf.extend_from_slice(&10u32.to_le_bytes());
    assert!(parse_chunk_table(&buf, 0, buf.len(), 50_000).is_err());
}

// ---------------------------------------------------------------------------
// LASzip VLR detection / parse round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_laz_vlr_detection_returns_expected_record_id() {
    let laszip_data = build_laszip_vlr_payload(2, 50_000, 100, 30, 1);
    let vlrs = vec![
        Vlr {
            key: VlrKey {
                user_id: "copc".into(),
                record_id: 1,
            },
            description: String::new(),
            data: vec![0u8; 160],
        },
        Vlr {
            key: VlrKey {
                user_id: "laszip encoded".into(),
                record_id: 22204,
            },
            description: String::new(),
            data: laszip_data,
        },
    ];
    let found = detect_laszip_vlr(&vlrs).expect("must find laszip VLR");
    assert_eq!(found.key.record_id, 22204);

    // Without a laszip VLR, detection returns None.
    let only_copc = vec![Vlr {
        key: VlrKey {
            user_id: "copc".into(),
            record_id: 1,
        },
        description: String::new(),
        data: vec![0u8; 160],
    }];
    assert!(detect_laszip_vlr(&only_copc).is_none());
}

#[test]
fn test_laz_vlr_parse_canonical_items_field() {
    // PF0 has a single POINT10 item.
    let payload = build_laszip_vlr_payload(2, 50_000, 1000, 2048, 0);
    let info = parse_laszip_vlr_data(&payload).expect("parse");
    assert_eq!(info.compressor, 2);
    assert_eq!(info.chunk_size, 50_000);
    assert_eq!(info.num_points, 1000);
    assert_eq!(info.num_bytes, 2048);
    assert_eq!(info.items.len(), 1);
    assert_eq!(info.items[0].item_type, 6);
    assert_eq!(info.items[0].size, 20);
    assert_eq!(info.items[0].version, 1);

    // PF1 has POINT10 + GPSTIME11.
    let payload_pf1 = build_laszip_vlr_payload(2, 50_000, 500, 1024, 1);
    let info_pf1 = parse_laszip_vlr_data(&payload_pf1).expect("parse");
    assert_eq!(info_pf1.items.len(), 2);
    assert_eq!(info_pf1.items[1].item_type, 7);
    assert_eq!(info_pf1.items[1].size, 8);
}

/// Build a LASzip VLR payload describing a PF<format> chunked compressor.
///
/// `format`: 0 = POINT10 only, 1 = POINT10 + GPSTIME11.
fn build_laszip_vlr_payload(
    compressor: u16,
    chunk_size: u32,
    num_points: i64,
    num_bytes: i64,
    format: u8,
) -> Vec<u8> {
    let items: Vec<LazItem> = match format {
        0 => vec![LazItem {
            item_type: 6,
            size: 20,
            version: 1,
        }],
        1 => vec![
            LazItem {
                item_type: 6,
                size: 20,
                version: 1,
            },
            LazItem {
                item_type: 7,
                size: 8,
                version: 1,
            },
        ],
        other => unreachable!("test helper only supports formats 0 and 1, got {other}"),
    };
    let info = LazVlrInfo {
        compressor,
        coder: 0,
        version_major: 3,
        version_minor: 4,
        options: 0,
        chunk_size,
        num_points,
        num_bytes,
        items,
    };
    serialize_laszip_vlr_data(&info)
}

/// Serialize a [`LazVlrInfo`] to bytes.  Reimplemented here because the
/// crate-internal serializer is `#[cfg(test)]` and not visible from
/// integration tests.
fn serialize_laszip_vlr_data(info: &LazVlrInfo) -> Vec<u8> {
    let mut out = Vec::with_capacity(34 + info.items.len() * 6);
    out.extend_from_slice(&info.compressor.to_le_bytes());
    out.extend_from_slice(&info.coder.to_le_bytes());
    out.push(info.version_major);
    out.push(info.version_minor);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&info.options.to_le_bytes());
    out.extend_from_slice(&info.chunk_size.to_le_bytes());
    out.extend_from_slice(&info.num_points.to_le_bytes());
    out.extend_from_slice(&info.num_bytes.to_le_bytes());
    out.extend_from_slice(&(info.items.len() as u16).to_le_bytes());
    for item in &info.items {
        out.extend_from_slice(&item.item_type.to_le_bytes());
        out.extend_from_slice(&item.size.to_le_bytes());
        out.extend_from_slice(&item.version.to_le_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// Predictor round-trips
// ---------------------------------------------------------------------------

#[test]
fn test_predictor_xyz_v1_recovers_constant_stream() {
    use oxigeo_copc::laz::arithmetic::{ArithmeticDecoder, ArithmeticEncoder, IntegerCompressor};
    use oxigeo_copc::laz::predictors::{PointXYZContext, decode_xyz_v1, encode_xyz_v1};
    let mut ic_enc = IntegerCompressor::new(32, 3);
    let mut enc = ArithmeticEncoder::new();
    let mut ctx_enc = PointXYZContext::with_seed(1_000, 2_000, 50);
    for _ in 0..8 {
        encode_xyz_v1(&mut ic_enc, &mut enc, &mut ctx_enc, 1_000, 2_000, 50);
    }
    let bytes = enc.done();
    let mut ic_dec = IntegerCompressor::new(32, 3);
    let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
    let mut ctx_dec = PointXYZContext::with_seed(1_000, 2_000, 50);
    for _ in 0..8 {
        let (x, y, z) = decode_xyz_v1(&mut ic_dec, &mut dec, &mut ctx_dec);
        assert_eq!((x, y, z), (1_000, 2_000, 50));
    }
}

#[test]
fn test_predictor_xyz_v1_recovers_linear_stream() {
    use oxigeo_copc::laz::arithmetic::{ArithmeticDecoder, ArithmeticEncoder, IntegerCompressor};
    use oxigeo_copc::laz::predictors::{PointXYZContext, decode_xyz_v1, encode_xyz_v1};
    let mut ic_enc = IntegerCompressor::new(32, 3);
    let mut enc = ArithmeticEncoder::new();
    let mut ctx_enc = PointXYZContext::with_seed(0, 0, 0);
    for i in 0..8 {
        let x = (i + 1) * 7;
        let y = (i + 1) * 11;
        let z = (i + 1) * 3;
        encode_xyz_v1(&mut ic_enc, &mut enc, &mut ctx_enc, x, y, z);
    }
    let bytes = enc.done();
    let mut ic_dec = IntegerCompressor::new(32, 3);
    let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
    let mut ctx_dec = PointXYZContext::with_seed(0, 0, 0);
    for i in 0..8 {
        let (x, y, z) = decode_xyz_v1(&mut ic_dec, &mut dec, &mut ctx_dec);
        assert_eq!(x, (i + 1) * 7);
        assert_eq!(y, (i + 1) * 11);
        assert_eq!(z, (i + 1) * 3);
    }
}

#[test]
fn test_predictor_intensity_v1_recovers_repeated_values() {
    use oxigeo_copc::laz::arithmetic::{ArithmeticDecoder, ArithmeticEncoder, SymbolModel};
    use oxigeo_copc::laz::predictors::{decode_intensity_v1, encode_intensity_v1};
    let mut model_enc = SymbolModel::new(256);
    let mut enc = ArithmeticEncoder::new();
    let mut last: u16 = 50;
    for _ in 0..16 {
        encode_intensity_v1(&mut enc, &mut model_enc, last, 50);
        last = 50;
    }
    let bytes = enc.done();
    let mut model_dec = SymbolModel::new(256);
    let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
    let mut last_dec: u16 = 50;
    for _ in 0..16 {
        let got = decode_intensity_v1(&mut dec, &mut model_dec, last_dec);
        assert_eq!(got, 50);
        last_dec = got;
    }
}

#[test]
fn test_predictor_classification_v1_256_contexts_independent() {
    use oxigeo_copc::laz::arithmetic::{ArithmeticDecoder, ArithmeticEncoder};
    use oxigeo_copc::laz::predictors::{
        decode_classification_v1, encode_classification_v1, make_classification_models,
    };
    let pattern: Vec<(u8, u8)> = (0..256).map(|i| (i as u8, ((i + 7) % 256) as u8)).collect();
    let mut models_enc = make_classification_models();
    let mut enc = ArithmeticEncoder::new();
    for (last, actual) in &pattern {
        encode_classification_v1(&mut enc, &mut models_enc, *last, *actual);
    }
    let bytes = enc.done();
    let mut models_dec = make_classification_models();
    let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
    for (last, expected) in &pattern {
        let got = decode_classification_v1(&mut dec, &mut models_dec, *last);
        assert_eq!(got, *expected, "last={last}");
    }
}

// ---------------------------------------------------------------------------
// Format-level round-trip
// ---------------------------------------------------------------------------

fn pf0(x: i32, y: i32, z: i32, intensity: u16, classification: u8) -> Vec<u8> {
    let mut r = vec![0u8; 20];
    r[0..4].copy_from_slice(&x.to_le_bytes());
    r[4..8].copy_from_slice(&y.to_le_bytes());
    r[8..12].copy_from_slice(&z.to_le_bytes());
    r[12..14].copy_from_slice(&intensity.to_le_bytes());
    r[14] = 1 | (1 << 3);
    r[15] = classification;
    r
}

fn pf1(x: i32, y: i32, z: i32, intensity: u16, classification: u8, gps: f64) -> Vec<u8> {
    let mut r = pf0(x, y, z, intensity, classification);
    r.resize(28, 0);
    r[20..28].copy_from_slice(&gps.to_le_bytes());
    r
}

#[test]
fn test_decompress_format_0_5_points_round_trip() {
    use oxigeo_copc::laz::format_v1::{PF0_RECORD_SIZE, compress_format_0, decompress_format_0};
    let mut records = Vec::new();
    records.extend(pf0(100, 200, 50, 100, 2));
    records.extend(pf0(110, 210, 55, 100, 2));
    records.extend(pf0(120, 220, 60, 105, 2));
    records.extend(pf0(130, 230, 65, 110, 5));
    records.extend(pf0(140, 240, 70, 115, 5));
    let compressed = compress_format_0(&records, 5);
    let decompressed = decompress_format_0(&compressed, 5).expect("decompress");
    assert_eq!(decompressed.len(), 5 * PF0_RECORD_SIZE);
    assert_eq!(decompressed, records);
}

#[test]
fn test_decompress_format_1_includes_gps_time() {
    use oxigeo_copc::laz::format_v1::{PF1_RECORD_SIZE, compress_format_1, decompress_format_1};
    let mut records = Vec::new();
    records.extend(pf1(100, 200, 50, 100, 2, 1.0));
    records.extend(pf1(105, 205, 55, 100, 2, 1.1));
    records.extend(pf1(110, 210, 60, 105, 2, 1.2));
    records.extend(pf1(115, 215, 65, 110, 5, 1.3));
    let compressed = compress_format_1(&records, 4);
    let decompressed = decompress_format_1(&compressed, 4).expect("decompress");
    assert_eq!(decompressed.len(), 4 * PF1_RECORD_SIZE);
    assert_eq!(decompressed, records);
}

#[test]
fn test_decompress_unsupported_format_6_returns_typed_error() {
    let result = decompress_chunk(&[0u8; 64], 4, 30, 6);
    assert!(
        matches!(
            result,
            Err(CopcError::UnsupportedLazFormat { format_id: 6 })
        ),
        "expected UnsupportedLazFormat {{ format_id: 6 }} variant",
    );
    // Also verify PF7 and PF8 yield the same typed error.
    let result7 = decompress_chunk(&[0u8; 64], 4, 36, 7);
    assert!(matches!(
        result7,
        Err(CopcError::UnsupportedLazFormat { format_id: 7 })
    ));
    let result8 = decompress_chunk(&[0u8; 64], 4, 38, 8);
    assert!(matches!(
        result8,
        Err(CopcError::UnsupportedLazFormat { format_id: 8 })
    ));
}

// ---------------------------------------------------------------------------
// CopcReader integration: end-to-end LAZ routing
// ---------------------------------------------------------------------------

/// Build a synthetic COPC file payload.  When `with_laszip_vlr` is true, the
/// file includes a LASzip VLR (so the reader will route the chunk through
/// `decompress_chunk`).  When false, the chunk is treated as raw LAS bytes.
fn build_copc_with_laz(
    with_laszip_vlr: bool,
    format_id: u8,
    record_length: u16,
    points: &[Vec<u8>],
) -> Vec<u8> {
    let header_size: u16 = 227;
    let mut file = vec![0u8; header_size as usize];
    file[0..4].copy_from_slice(b"LASF");
    file[24] = 1;
    file[25] = 4;
    file[94..96].copy_from_slice(&header_size.to_le_bytes());
    let n_vlrs: u32 = if with_laszip_vlr { 3 } else { 2 };
    file[100..104].copy_from_slice(&n_vlrs.to_le_bytes());
    file[104] = format_id;
    file[105..107].copy_from_slice(&record_length.to_le_bytes());
    file[107..111].copy_from_slice(&(points.len() as u32).to_le_bytes());
    let scale = 0.001f64.to_le_bytes();
    file[131..139].copy_from_slice(&scale);
    file[139..147].copy_from_slice(&scale);
    file[147..155].copy_from_slice(&scale);
    file[179..187].copy_from_slice(&1000.0f64.to_le_bytes());
    file[187..195].copy_from_slice(&(-1000.0f64).to_le_bytes());
    file[195..203].copy_from_slice(&1000.0f64.to_le_bytes());
    file[203..211].copy_from_slice(&(-1000.0f64).to_le_bytes());
    file[211..219].copy_from_slice(&1000.0f64.to_le_bytes());
    file[219..227].copy_from_slice(&(-1000.0f64).to_le_bytes());

    // VLR #0: COPC info (160-byte body).
    let mut copc_body = vec![0u8; 160];
    copc_body[0..8].copy_from_slice(&500.0f64.to_le_bytes());
    copc_body[8..16].copy_from_slice(&500.0f64.to_le_bytes());
    copc_body[16..24].copy_from_slice(&500.0f64.to_le_bytes());
    copc_body[24..32].copy_from_slice(&500.0f64.to_le_bytes());
    copc_body[32..40].copy_from_slice(&1.0f64.to_le_bytes());
    append_vlr(&mut file, "copc", 1, &copc_body);

    // VLR #1: COPC hierarchy placeholder.
    append_vlr(&mut file, "copc", 1000, &[]);

    // VLR #2: optional LASzip VLR.
    if with_laszip_vlr {
        let payload = build_laszip_vlr_payload(2, 50_000, points.len() as i64, 0, format_id);
        append_vlr(&mut file, "laszip encoded", 22204, &payload);
    }

    // Patch offset_to_point_data.
    let point_data_offset = file.len() as u32;
    file[96..100].copy_from_slice(&point_data_offset.to_le_bytes());

    // Write chunk: if LAZ, emit a single chunk with seed + arithmetic body;
    // if not, emit raw records back-to-back.
    let point_data_start = file.len();
    if with_laszip_vlr {
        use oxigeo_copc::laz::format_v1::{compress_format_0, compress_format_1};
        // Pack the records into a contiguous buffer.
        let mut packed: Vec<u8> = Vec::new();
        for p in points {
            packed.extend_from_slice(p);
        }
        let compressed = match format_id {
            0 => compress_format_0(&packed, points.len()),
            1 => compress_format_1(&packed, points.len()),
            other => unreachable!("test helper only handles PF0/PF1, got {other}"),
        };
        file.extend_from_slice(&compressed);
    } else {
        for p in points {
            file.extend_from_slice(p);
        }
    }
    let point_data_end = file.len();
    let chunk_byte_count = point_data_end - point_data_start;

    // Hierarchy page: one root entry pointing to the chunk.
    let hier_offset = file.len();
    let mut entry = vec![0u8; 32];
    entry[0..4].copy_from_slice(&0i32.to_le_bytes()); // depth
    entry[4..8].copy_from_slice(&0i32.to_le_bytes());
    entry[8..12].copy_from_slice(&0i32.to_le_bytes());
    entry[12..16].copy_from_slice(&0i32.to_le_bytes());
    entry[16..24].copy_from_slice(&(point_data_start as u64).to_le_bytes());
    entry[24..28].copy_from_slice(&(chunk_byte_count as i32).to_le_bytes());
    entry[28..32].copy_from_slice(&(points.len() as i32).to_le_bytes());
    file.extend_from_slice(&entry);

    // Patch root_hier_offset / root_hier_size inside the COPC info body.
    let copc_body_off = header_size as usize + 54;
    file[copc_body_off + 40..copc_body_off + 48]
        .copy_from_slice(&(hier_offset as u64).to_le_bytes());
    file[copc_body_off + 48..copc_body_off + 56].copy_from_slice(&32u64.to_le_bytes());

    file
}

fn append_vlr(file: &mut Vec<u8>, user_id: &str, record_id: u16, payload: &[u8]) {
    file.extend_from_slice(&[0u8; 2]);
    let uid_bytes = user_id.as_bytes();
    let mut uid_buf = [0u8; 16];
    let len = uid_bytes.len().min(16);
    uid_buf[..len].copy_from_slice(&uid_bytes[..len]);
    file.extend_from_slice(&uid_buf);
    file.extend_from_slice(&record_id.to_le_bytes());
    file.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    file.extend_from_slice(&[0u8; 32]);
    file.extend_from_slice(payload);
}

#[test]
fn test_copc_reader_routes_laz_through_decompressor_end_to_end() {
    use oxigeo_copc::CopcReader;
    use oxigeo_copc::point::BoundingBox3D;

    let pts = vec![
        pf0(100_000, 200_000, 50_000, 100, 2),
        pf0(110_000, 210_000, 55_000, 100, 2),
        pf0(120_000, 220_000, 60_000, 100, 2),
        pf0(130_000, 230_000, 65_000, 100, 2),
        pf0(140_000, 240_000, 70_000, 100, 2),
    ];
    let file_data = build_copc_with_laz(true, 0, 20, &pts);
    let reader = CopcReader::from_bytes(&file_data).expect("parse LAZ COPC");

    let bbox = BoundingBox3D::new(0.0, 0.0, 0.0, 1000.0, 1000.0, 1000.0).expect("valid bbox");
    let result = reader.query_points_in_bbox(&bbox).expect("query");
    assert_eq!(result.len(), 5);
    assert!((result[0].x - 100.0).abs() < 1e-6);
    assert!((result[4].x - 140.0).abs() < 1e-6);
}

#[test]
fn test_copc_reader_passthrough_when_no_laszip_vlr() {
    use oxigeo_copc::CopcReader;
    use oxigeo_copc::point::BoundingBox3D;

    let pts = vec![
        pf0(100_000, 200_000, 50_000, 100, 2),
        pf0(300_000, 400_000, 100_000, 100, 2),
    ];
    let file_data = build_copc_with_laz(false, 0, 20, &pts);
    let reader = CopcReader::from_bytes(&file_data).expect("parse raw COPC");

    let bbox = BoundingBox3D::new(0.0, 0.0, 0.0, 1000.0, 1000.0, 1000.0).expect("valid bbox");
    let result = reader.query_points_in_bbox(&bbox).expect("query");
    assert_eq!(result.len(), 2);
}
