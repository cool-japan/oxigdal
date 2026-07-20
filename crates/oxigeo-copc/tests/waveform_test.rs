//! Tests for LAS point data record formats 9 and 10 (full-waveform).
//!
//! Covers `WaveformPacket` parsing, field fidelity, error handling, and
//! batch deserialization for both format IDs.

use oxigeo_copc::{
    WaveformPacket,
    point_format::{deserialize_point, deserialize_points, min_record_size},
};

// ---------------------------------------------------------------------------
// Low-level byte-building helpers
// ---------------------------------------------------------------------------

/// Build a 30-byte format-6 / extended-base record.
///
/// The encoding mirrors `make_format6_record` in `point_format.rs` tests:
/// - intensity = 500
/// - return_number = 3, number_of_returns = 5
/// - classification = 6 (Building)
/// - user_data = 99
/// - scan_angle raw i16 = 5000 (0.006 * 5000 = 30 °)
/// - point_source_id = 12
/// - gps_time supplied by caller
fn make_format6_bytes(raw_x: i32, raw_y: i32, raw_z: i32, gps_time: f64) -> Vec<u8> {
    let mut rec = vec![0u8; 30];
    rec[0..4].copy_from_slice(&raw_x.to_le_bytes());
    rec[4..8].copy_from_slice(&raw_y.to_le_bytes());
    rec[8..12].copy_from_slice(&raw_z.to_le_bytes());
    rec[12..14].copy_from_slice(&500u16.to_le_bytes()); // intensity
    rec[14] = 3 | (5 << 4); // return_number=3, number_of_returns=5
    rec[15] = 0; // classification flags
    rec[16] = 6; // classification = Building
    rec[17] = 99; // user_data
    rec[18..20].copy_from_slice(&5000i16.to_le_bytes()); // scan angle
    rec[20..22].copy_from_slice(&12u16.to_le_bytes()); // point_source_id
    rec[22..30].copy_from_slice(&gps_time.to_le_bytes());
    rec
}

/// Build a 38-byte format-8 record (30-byte base + 6 RGB + 2 NIR).
fn make_format8_bytes(
    raw_x: i32,
    raw_y: i32,
    raw_z: i32,
    gps_time: f64,
    r: u16,
    g: u16,
    b: u16,
) -> Vec<u8> {
    let mut rec = make_format6_bytes(raw_x, raw_y, raw_z, gps_time);
    rec.resize(38, 0);
    rec[30..32].copy_from_slice(&r.to_le_bytes());
    rec[32..34].copy_from_slice(&g.to_le_bytes());
    rec[34..36].copy_from_slice(&b.to_le_bytes());
    rec[36..38].copy_from_slice(&1000u16.to_le_bytes()); // NIR
    rec
}

/// Build a 29-byte serialized waveform packet from its constituent fields.
fn make_waveform_bytes(
    descriptor: u8,
    byte_offset: u64,
    packet_size: u32,
    return_point_loc: f32,
    x_t: f32,
    y_t: f32,
    z_t: f32,
) -> [u8; 29] {
    let mut buf = [0u8; 29];
    buf[0] = descriptor;
    buf[1..9].copy_from_slice(&byte_offset.to_le_bytes());
    buf[9..13].copy_from_slice(&packet_size.to_le_bytes());
    buf[13..17].copy_from_slice(&return_point_loc.to_le_bytes());
    buf[17..21].copy_from_slice(&x_t.to_le_bytes());
    buf[21..25].copy_from_slice(&y_t.to_le_bytes());
    buf[25..29].copy_from_slice(&z_t.to_le_bytes());
    buf
}

/// Build a 59-byte format-9 record: 30-byte PF6 base + 29-byte waveform.
fn make_format9_bytes(raw_x: i32, raw_y: i32, raw_z: i32, gps_time: f64) -> Vec<u8> {
    let mut v = make_format6_bytes(raw_x, raw_y, raw_z, gps_time);
    v.extend_from_slice(&[0u8; 29]);
    v
}

/// Build a format-9 record with explicit waveform fields.
#[allow(clippy::too_many_arguments)]
fn make_format9_with_waveform(
    raw_x: i32,
    raw_y: i32,
    raw_z: i32,
    gps_time: f64,
    descriptor: u8,
    byte_offset: u64,
    packet_size: u32,
    return_point_loc: f32,
    x_t: f32,
    y_t: f32,
    z_t: f32,
) -> Vec<u8> {
    let mut v = make_format6_bytes(raw_x, raw_y, raw_z, gps_time);
    let wf = make_waveform_bytes(
        descriptor,
        byte_offset,
        packet_size,
        return_point_loc,
        x_t,
        y_t,
        z_t,
    );
    v.extend_from_slice(&wf);
    v
}

/// Build a 67-byte format-10 record: 38-byte PF8 base + 29-byte waveform.
#[allow(clippy::too_many_arguments)]
fn make_format10_with_waveform(
    raw_x: i32,
    raw_y: i32,
    raw_z: i32,
    gps_time: f64,
    r: u16,
    g: u16,
    b: u16,
    descriptor: u8,
    byte_offset: u64,
    packet_size: u32,
    return_point_loc: f32,
    x_t: f32,
    y_t: f32,
    z_t: f32,
) -> Vec<u8> {
    let mut v = make_format8_bytes(raw_x, raw_y, raw_z, gps_time, r, g, b);
    let wf = make_waveform_bytes(
        descriptor,
        byte_offset,
        packet_size,
        return_point_loc,
        x_t,
        y_t,
        z_t,
    );
    v.extend_from_slice(&wf);
    v
}

// ---------------------------------------------------------------------------
// min_record_size tests
// ---------------------------------------------------------------------------

#[test]
fn test_format9_min_record_size_is_59() {
    assert_eq!(
        min_record_size(9).expect("format 9 should be supported"),
        59
    );
}

#[test]
fn test_format10_min_record_size_is_67() {
    assert_eq!(
        min_record_size(10).expect("format 10 should be supported"),
        67
    );
}

// ---------------------------------------------------------------------------
// Format 9 parsing tests
// ---------------------------------------------------------------------------

#[test]
fn test_format9_waveform_descriptor_and_offset_decoded() {
    let rec = make_format9_with_waveform(
        1000,
        2000,
        500,
        42.5,
        7,                         // descriptor_index
        0x0102_0304_0506_0708_u64, // byte_offset
        4096,                      // packet_size
        1.5_f32,                   // return_point_loc
        0.1_f32,                   // x_t
        0.2_f32,                   // y_t
        0.3_f32,                   // z_t
    );
    let pt = deserialize_point(&rec, 9, [0.001; 3], [0.0; 3])
        .expect("format 9 should parse successfully");
    let wf = pt.waveform.expect("format 9 must carry a waveform packet");
    assert_eq!(wf.descriptor_index, 7);
    assert_eq!(wf.byte_offset, 0x0102_0304_0506_0708_u64);
    assert_eq!(wf.packet_size, 4096);
    assert!(
        (wf.return_point_loc - 1.5).abs() < 1e-6,
        "return_point_loc mismatch"
    );
    assert!((wf.x_t - 0.1).abs() < 1e-6, "x_t mismatch");
    assert!((wf.y_t - 0.2).abs() < 1e-6, "y_t mismatch");
    assert!((wf.z_t - 0.3).abs() < 1e-6, "z_t mismatch");
}

#[test]
fn test_format9_has_no_rgb() {
    let rec = make_format9_bytes(100, 200, 50, 1.0);
    let pt = deserialize_point(&rec, 9, [0.001; 3], [0.0; 3]).expect("format 9 parse");
    assert!(pt.red.is_none(), "format 9 must not carry red channel");
    assert!(pt.green.is_none(), "format 9 must not carry green channel");
    assert!(pt.blue.is_none(), "format 9 must not carry blue channel");
}

#[test]
fn test_format9_gps_time_decoded() {
    let gps = 987654.321_f64;
    let rec = make_format9_bytes(0, 0, 0, gps);
    let pt = deserialize_point(&rec, 9, [1.0; 3], [0.0; 3]).expect("format 9 parse");
    let decoded = pt.gps_time.expect("format 9 always carries GPS time");
    assert!((decoded - gps).abs() < 1e-9, "GPS time round-trip failed");
}

#[test]
fn test_format9_base_fields_match_format6() {
    // The first 30 bytes of a format-9 record are identical to a format-6 record.
    // Parsing both should produce the same x/y/z, intensity, classification, etc.
    let raw_x = 10_000_i32;
    let raw_y = 20_000_i32;
    let raw_z = 5_000_i32;
    let gps = 100.0_f64;
    let scale = [0.001; 3];
    let origin = [0.0; 3];

    let rec6 = make_format6_bytes(raw_x, raw_y, raw_z, gps);
    let rec9 = make_format9_bytes(raw_x, raw_y, raw_z, gps);

    let pt6 = deserialize_point(&rec6, 6, scale, origin).expect("format 6 parse");
    let pt9 = deserialize_point(&rec9, 9, scale, origin).expect("format 9 parse");

    assert!((pt6.x - pt9.x).abs() < 1e-12);
    assert!((pt6.y - pt9.y).abs() < 1e-12);
    assert!((pt6.z - pt9.z).abs() < 1e-12);
    assert_eq!(pt6.intensity, pt9.intensity);
    assert_eq!(pt6.return_number, pt9.return_number);
    assert_eq!(pt6.number_of_returns, pt9.number_of_returns);
    assert_eq!(pt6.classification, pt9.classification);
    assert_eq!(pt6.user_data, pt9.user_data);
    assert_eq!(pt6.point_source_id, pt9.point_source_id);
    let g6 = pt6.gps_time.expect("pf6 gps");
    let g9 = pt9.gps_time.expect("pf9 gps");
    assert!((g6 - g9).abs() < 1e-12);
    // PF6 has no waveform; PF9 does
    assert!(pt6.waveform.is_none());
    assert!(pt9.waveform.is_some());
}

#[test]
fn test_format9_waveform_all_zero_bytes() {
    // A waveform packet of all-zero bytes must be parseable without error.
    let rec = make_format9_bytes(0, 0, 0, 0.0);
    let pt =
        deserialize_point(&rec, 9, [1.0; 3], [0.0; 3]).expect("all-zero PF9 record should parse");
    let wf = pt.waveform.expect("waveform must be present");
    assert_eq!(wf.descriptor_index, 0);
    assert_eq!(wf.byte_offset, 0);
    assert_eq!(wf.packet_size, 0);
    assert_eq!(wf.return_point_loc, 0.0);
    assert_eq!(wf.x_t, 0.0);
    assert_eq!(wf.y_t, 0.0);
    assert_eq!(wf.z_t, 0.0);
}

// ---------------------------------------------------------------------------
// Format 10 parsing tests
// ---------------------------------------------------------------------------

#[test]
fn test_format10_rgb_plus_waveform_decoded() {
    let rec = make_format10_with_waveform(
        5000, 6000, 7000, 55.5, /* r */ 100, /* g */ 200, /* b */ 300,
        /* descriptor */ 3, /* byte_offset */ 65536, /* packet_size */ 256,
        /* return_point_loc */ 2.5, /* x_t */ -0.5, /* y_t */ 0.5,
        /* z_t */ 1.0,
    );
    let pt = deserialize_point(&rec, 10, [0.001; 3], [0.0; 3]).expect("format 10 parse");

    // RGB must be present
    assert_eq!(pt.red, Some(100));
    assert_eq!(pt.green, Some(200));
    assert_eq!(pt.blue, Some(300));

    // Waveform must be present and correctly decoded
    let wf = pt.waveform.expect("format 10 must carry waveform");
    assert_eq!(wf.descriptor_index, 3);
    assert_eq!(wf.byte_offset, 65536);
    assert_eq!(wf.packet_size, 256);
    assert!((wf.return_point_loc - 2.5).abs() < 1e-6);
    assert!((wf.x_t - (-0.5)).abs() < 1e-6);
    assert!((wf.y_t - 0.5).abs() < 1e-6);
    assert!((wf.z_t - 1.0).abs() < 1e-6);
}

#[test]
fn test_format10_waveform_xyz_temporal_params() {
    // Verify that the three parametric-direction fields survive a round-trip.
    let x_t = -1.234_f32;
    let y_t = 5.678_f32;
    let z_t = -9.012_f32;
    let rec = make_format10_with_waveform(0, 0, 0, 0.0, 0, 0, 0, 1, 0, 0, 0.0, x_t, y_t, z_t);
    let pt = deserialize_point(&rec, 10, [1.0; 3], [0.0; 3]).expect("format 10 parse");
    let wf = pt.waveform.expect("waveform present");
    assert!((wf.x_t - x_t).abs() < 1e-7, "x_t = {}", wf.x_t);
    assert!((wf.y_t - y_t).abs() < 1e-7, "y_t = {}", wf.y_t);
    assert!((wf.z_t - z_t).abs() < 1e-7, "z_t = {}", wf.z_t);
}

#[test]
fn test_format10_base_fields_match_format8() {
    let raw_x = 3_000_i32;
    let raw_y = 4_000_i32;
    let raw_z = 1_000_i32;
    let gps = 77.7_f64;
    let (r, g, b) = (111_u16, 222_u16, 333_u16);
    let scale = [0.001; 3];
    let origin = [0.0; 3];

    let rec8 = make_format8_bytes(raw_x, raw_y, raw_z, gps, r, g, b);
    let rec10 = make_format10_with_waveform(
        raw_x, raw_y, raw_z, gps, r, g, b, 0, 0, 0, 0.0, 0.0, 0.0, 0.0,
    );

    let pt8 = deserialize_point(&rec8, 8, scale, origin).expect("format 8 parse");
    let pt10 = deserialize_point(&rec10, 10, scale, origin).expect("format 10 parse");

    assert!((pt8.x - pt10.x).abs() < 1e-12);
    assert!((pt8.y - pt10.y).abs() < 1e-12);
    assert!((pt8.z - pt10.z).abs() < 1e-12);
    assert_eq!(pt8.intensity, pt10.intensity);
    assert_eq!(pt8.classification, pt10.classification);
    assert_eq!(pt8.red, pt10.red);
    assert_eq!(pt8.green, pt10.green);
    assert_eq!(pt8.blue, pt10.blue);
    let g8 = pt8.gps_time.expect("pf8 gps");
    let g10 = pt10.gps_time.expect("pf10 gps");
    assert!((g8 - g10).abs() < 1e-12);
    // PF8 has no waveform; PF10 does
    assert!(pt8.waveform.is_none());
    assert!(pt10.waveform.is_some());
}

// ---------------------------------------------------------------------------
// Error handling tests
// ---------------------------------------------------------------------------

#[test]
fn test_format9_truncated_record_returns_error() {
    // A 40-byte buffer for format 9 is too short (need >= 59).
    let rec = vec![0u8; 40];
    let result = deserialize_point(&rec, 9, [1.0; 3], [0.0; 3]);
    assert!(result.is_err(), "Truncated PF9 record must return an error");
}

#[test]
fn test_format10_truncated_record_returns_error() {
    // A 50-byte buffer for format 10 is too short (need >= 67).
    let rec = vec![0u8; 50];
    let result = deserialize_point(&rec, 10, [1.0; 3], [0.0; 3]);
    assert!(
        result.is_err(),
        "Truncated PF10 record must return an error"
    );
}

// ---------------------------------------------------------------------------
// Batch deserialization tests
// ---------------------------------------------------------------------------

#[test]
fn test_deserialize_points_batch_format9() {
    // Build 3 format-9 records (3 * 59 = 177 bytes).
    let rec0 = make_format9_with_waveform(1000, 2000, 500, 10.0, 1, 100, 64, 0.5, 0.1, 0.2, 0.3);
    let rec1 =
        make_format9_with_waveform(3000, 4000, 1000, 20.0, 2, 200, 128, 1.0, -0.1, -0.2, -0.3);
    let rec2 = make_format9_with_waveform(5000, 6000, 1500, 30.0, 3, 300, 256, 1.5, 0.5, 0.5, 0.5);
    let mut data = rec0;
    data.extend_from_slice(&rec1);
    data.extend_from_slice(&rec2);

    let pts = deserialize_points(&data, 3, 59, 9, [0.001; 3], [0.0; 3])
        .expect("batch format-9 deserialize");
    assert_eq!(pts.len(), 3);

    // All three points must have waveform data
    assert!(pts[0].waveform.is_some(), "pt[0] waveform absent");
    assert!(pts[1].waveform.is_some(), "pt[1] waveform absent");
    assert!(pts[2].waveform.is_some(), "pt[2] waveform absent");

    // Spot-check descriptor_index values
    assert_eq!(
        pts[0]
            .waveform
            .as_ref()
            .expect("pt[0] waveform")
            .descriptor_index,
        1
    );
    assert_eq!(
        pts[1]
            .waveform
            .as_ref()
            .expect("pt[1] waveform")
            .descriptor_index,
        2
    );
    assert_eq!(
        pts[2]
            .waveform
            .as_ref()
            .expect("pt[2] waveform")
            .descriptor_index,
        3
    );

    // Coordinates
    assert!((pts[0].x - 1.0).abs() < 1e-9);
    assert!((pts[1].x - 3.0).abs() < 1e-9);
    assert!((pts[2].x - 5.0).abs() < 1e-9);
}

#[test]
fn test_waveform_packet_struct_is_publicly_accessible() {
    // Confirm WaveformPacket can be constructed and compared from outside the crate.
    let wf = WaveformPacket {
        descriptor_index: 5,
        byte_offset: 1024,
        packet_size: 512,
        return_point_loc: 3.15,
        x_t: 1.0,
        y_t: -1.0,
        z_t: 0.0,
    };
    assert_eq!(wf.descriptor_index, 5);
    assert_eq!(wf.byte_offset, 1024);
    assert_eq!(wf.packet_size, 512);
    assert!((wf.return_point_loc - 3.15).abs() < 1e-5);
}
