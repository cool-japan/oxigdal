//! Extended integration tests for oxigdal-copc.
//!
//! Brings total test count to 50+.

use oxigdal_copc::{CopcError, CopcInfo, LasHeader, LasVersion, Vlr, VlrKey};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a minimal LAS header byte slice (227 bytes) for the given version.
fn make_las_header(major: u8, minor: u8) -> Vec<u8> {
    let mut data = vec![0u8; 227];
    data[0..4].copy_from_slice(b"LASF");
    data[24] = major;
    data[25] = minor;
    data[94..96].copy_from_slice(&227u16.to_le_bytes());
    data[96..100].copy_from_slice(&227u32.to_le_bytes());
    data[100..104].copy_from_slice(&0u32.to_le_bytes());
    data[104] = 6;
    data[105..107].copy_from_slice(&30u16.to_le_bytes());
    data[107..111].copy_from_slice(&1000u32.to_le_bytes());
    let scale = 0.001f64.to_le_bytes();
    data[131..139].copy_from_slice(&scale);
    data[139..147].copy_from_slice(&scale);
    data[147..155].copy_from_slice(&scale);
    data[179..187].copy_from_slice(&10.0f64.to_le_bytes());
    data[187..195].copy_from_slice(&1.0f64.to_le_bytes());
    data[195..203].copy_from_slice(&20.0f64.to_le_bytes());
    data[203..211].copy_from_slice(&2.0f64.to_le_bytes());
    data[211..219].copy_from_slice(&5.0f64.to_le_bytes());
    data[219..227].copy_from_slice(&0.5f64.to_le_bytes());
    data
}

/// Build a LAS 1.4 header with 375+ bytes to include the 64-bit point count
/// field at offset 247.
fn make_las14_header_full(point_count_64: u64) -> Vec<u8> {
    let mut data = vec![0u8; 375];
    data[0..4].copy_from_slice(b"LASF");
    data[24] = 1;
    data[25] = 4;
    data[94..96].copy_from_slice(&375u16.to_le_bytes());
    data[96..100].copy_from_slice(&375u32.to_le_bytes());
    let scale = 0.01f64.to_le_bytes();
    data[131..139].copy_from_slice(&scale);
    data[139..147].copy_from_slice(&scale);
    data[147..155].copy_from_slice(&scale);
    // 64-bit point count at offset 247
    data[247..255].copy_from_slice(&point_count_64.to_le_bytes());
    data
}

/// Build a 160-byte CopcInfo VLR body.
fn make_copc_info(cx: f64, cy: f64, cz: f64, halfsize: f64) -> Vec<u8> {
    let mut data = vec![0u8; 160];
    data[0..8].copy_from_slice(&cx.to_le_bytes());
    data[8..16].copy_from_slice(&cy.to_le_bytes());
    data[16..24].copy_from_slice(&cz.to_le_bytes());
    data[24..32].copy_from_slice(&halfsize.to_le_bytes());
    data[32..40].copy_from_slice(&1.0f64.to_le_bytes());
    data[40..48].copy_from_slice(&500u64.to_le_bytes());
    data[48..56].copy_from_slice(&200u64.to_le_bytes());
    data[56..64].copy_from_slice(&0.0f64.to_le_bytes());
    data[64..72].copy_from_slice(&100.0f64.to_le_bytes());
    data
}

/// Build a minimal VLR (54-byte header + payload).
fn make_vlr(user_id: &str, record_id: u16, payload: &[u8]) -> Vec<u8> {
    let mut data = vec![0u8; 54 + payload.len()];
    let uid_bytes = user_id.as_bytes();
    let len = uid_bytes.len().min(16);
    data[2..2 + len].copy_from_slice(&uid_bytes[..len]);
    data[18..20].copy_from_slice(&record_id.to_le_bytes());
    data[20..22].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    data[54..54 + payload.len()].copy_from_slice(payload);
    data
}

// ── LAS header — version variants ─────────────────────────────────────────────

#[test]
fn test_las_version_10() {
    let data = make_las_header(1, 0);
    let hdr = LasHeader::parse(&data).expect("valid LAS 1.0");
    assert_eq!(hdr.version, LasVersion::V10);
}

#[test]
fn test_las_version_11() {
    let data = make_las_header(1, 1);
    let hdr = LasHeader::parse(&data).expect("valid LAS 1.1");
    assert_eq!(hdr.version, LasVersion::V11);
}

#[test]
fn test_las_version_13() {
    let data = make_las_header(1, 3);
    let hdr = LasHeader::parse(&data).expect("valid LAS 1.3");
    assert_eq!(hdr.version, LasVersion::V13);
}

#[test]
fn test_las_unsupported_version_returns_error() {
    let mut data = make_las_header(2, 0);
    data[24] = 2; // major = 2 — unsupported
    let err = LasHeader::parse(&data).expect_err("should fail for version 2.0");
    assert!(matches!(err, CopcError::UnsupportedVersion(2, 0)));
}

#[test]
fn test_las_unsupported_minor_returns_error() {
    let data = make_las_header(1, 7);
    let err = LasHeader::parse(&data).expect_err("unsupported minor");
    assert!(matches!(err, CopcError::UnsupportedVersion(1, 7)));
}

// ── LAS header — field extraction ─────────────────────────────────────────────

#[test]
fn test_las_header_size_field() {
    let data = make_las_header(1, 2);
    let hdr = LasHeader::parse(&data).expect("valid");
    assert_eq!(hdr.header_size, 227);
}

#[test]
fn test_las_offset_to_point_data() {
    let data = make_las_header(1, 2);
    let hdr = LasHeader::parse(&data).expect("valid");
    assert_eq!(hdr.offset_to_point_data, 227);
}

#[test]
fn test_las_point_data_format_id() {
    let data = make_las_header(1, 2);
    let hdr = LasHeader::parse(&data).expect("valid");
    assert_eq!(hdr.point_data_format_id, 6);
}

#[test]
fn test_las_point_data_record_length() {
    let data = make_las_header(1, 2);
    let hdr = LasHeader::parse(&data).expect("valid");
    assert_eq!(hdr.point_data_record_length, 30);
}

#[test]
fn test_las_number_of_vlrs_zero() {
    let data = make_las_header(1, 2);
    let hdr = LasHeader::parse(&data).expect("valid");
    assert_eq!(hdr.number_of_vlrs, 0);
}

#[test]
fn test_las14_64bit_point_count() {
    let expected_count: u64 = 123_456_789_012;
    let data = make_las14_header_full(expected_count);
    let hdr = LasHeader::parse(&data).expect("valid LAS 1.4 full");
    assert_eq!(hdr.version, LasVersion::V14);
    assert_eq!(hdr.number_of_point_records, expected_count);
}

#[test]
fn test_las_legacy_32bit_point_count_for_v12() {
    let data = make_las_header(1, 2);
    let hdr = LasHeader::parse(&data).expect("valid");
    // legacy count was set to 1000 in make_las_header
    assert_eq!(hdr.number_of_point_records, 1000);
}

#[test]
fn test_las_offsets_are_zero() {
    let data = make_las_header(1, 2);
    let hdr = LasHeader::parse(&data).expect("valid");
    assert!((hdr.offset_x).abs() < f64::EPSILON);
    assert!((hdr.offset_y).abs() < f64::EPSILON);
    assert!((hdr.offset_z).abs() < f64::EPSILON);
}

#[test]
fn test_las_scale_factors_custom() {
    let mut data = make_las_header(1, 2);
    let sx = 0.0001f64.to_le_bytes();
    let sy = 0.0002f64.to_le_bytes();
    let sz = 0.0003f64.to_le_bytes();
    data[131..139].copy_from_slice(&sx);
    data[139..147].copy_from_slice(&sy);
    data[147..155].copy_from_slice(&sz);
    let hdr = LasHeader::parse(&data).expect("valid");
    assert!((hdr.scale_x - 0.0001).abs() < 1e-10);
    assert!((hdr.scale_y - 0.0002).abs() < 1e-10);
    assert!((hdr.scale_z - 0.0003).abs() < 1e-10);
}

#[test]
fn test_las_bounds_min_less_than_max() {
    let data = make_las_header(1, 2);
    let hdr = LasHeader::parse(&data).expect("valid");
    let (min, max) = hdr.bounds();
    assert!(min[0] < max[0], "min_x < max_x");
    assert!(min[1] < max[1], "min_y < max_y");
    assert!(min[2] < max[2], "min_z < max_z");
}

#[test]
fn test_las_data_exactly_227_bytes_ok() {
    let data = make_las_header(1, 2);
    assert_eq!(data.len(), 227);
    assert!(LasHeader::parse(&data).is_ok());
}

#[test]
fn test_las_data_226_bytes_error() {
    let data = vec![0u8; 226];
    assert!(LasHeader::parse(&data).is_err());
}

#[test]
fn test_las_data_larger_than_min_ok() {
    let mut data = make_las_header(1, 2);
    data.extend_from_slice(&[0u8; 100]); // pad extra bytes
    assert!(LasHeader::parse(&data).is_ok());
}

// ── LAS system_id and generating_software ─────────────────────────────────────

#[test]
fn test_las_system_id_zeroed() {
    let data = make_las_header(1, 2);
    let hdr = LasHeader::parse(&data).expect("valid");
    // our helper zeroes these fields
    assert_eq!(hdr.system_id, [0u8; 32]);
}

#[test]
fn test_las_generating_software_set() {
    let mut data = make_las_header(1, 2);
    // generating_software starts at offset 58
    let sw = b"TestWriter\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
    data[58..90].copy_from_slice(&sw[..32]);
    let hdr = LasHeader::parse(&data).expect("valid");
    assert_eq!(&hdr.generating_software[..10], b"TestWriter");
}

// ── CopcInfo — extended ────────────────────────────────────────────────────────

#[test]
fn test_copc_info_spacing_field() {
    let data = make_copc_info(0.0, 0.0, 0.0, 1.0);
    let info = CopcInfo::parse(&data).expect("valid");
    assert!((info.spacing - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_copc_info_hierarchy_fields() {
    let data = make_copc_info(0.0, 0.0, 0.0, 1.0);
    let info = CopcInfo::parse(&data).expect("valid");
    assert_eq!(info.root_hier_offset, 500);
    assert_eq!(info.root_hier_size, 200);
}

#[test]
fn test_copc_info_gpstime_fields() {
    let data = make_copc_info(0.0, 0.0, 0.0, 1.0);
    let info = CopcInfo::parse(&data).expect("valid");
    assert!((info.gpstime_minimum).abs() < f64::EPSILON);
    assert!((info.gpstime_maximum - 100.0).abs() < f64::EPSILON);
}

#[test]
fn test_copc_info_exactly_160_bytes_ok() {
    let data = vec![0u8; 160];
    assert!(CopcInfo::parse(&data).is_ok());
}

#[test]
fn test_copc_info_159_bytes_error() {
    let data = vec![0u8; 159];
    assert!(CopcInfo::parse(&data).is_err());
}

#[test]
fn test_copc_info_bounds_asymmetric() {
    let data = make_copc_info(10.0, 20.0, 30.0, 5.0);
    let info = CopcInfo::parse(&data).expect("valid");
    let (min, max) = info.bounds();
    assert!((min[0] - 5.0).abs() < f64::EPSILON, "min_x = cx - h = 5");
    assert!((min[1] - 15.0).abs() < f64::EPSILON, "min_y = cy - h = 15");
    assert!((min[2] - 25.0).abs() < f64::EPSILON, "min_z = cz - h = 25");
    assert!((max[0] - 15.0).abs() < f64::EPSILON, "max_x = cx + h = 15");
    assert!((max[1] - 25.0).abs() < f64::EPSILON, "max_y = cy + h = 25");
    assert!((max[2] - 35.0).abs() < f64::EPSILON, "max_z = cz + h = 35");
}

#[test]
fn test_copc_info_bounds_symmetry_around_center() {
    let cx = 100.0f64;
    let cy = 200.0f64;
    let cz = 300.0f64;
    let h = 50.0f64;
    let data = make_copc_info(cx, cy, cz, h);
    let info = CopcInfo::parse(&data).expect("valid");
    let (min, max) = info.bounds();
    // (min + max) / 2 should equal center
    assert!((((min[0] + max[0]) / 2.0) - cx).abs() < f64::EPSILON);
    assert!((((min[1] + max[1]) / 2.0) - cy).abs() < f64::EPSILON);
    assert!((((min[2] + max[2]) / 2.0) - cz).abs() < f64::EPSILON);
}

// ── VLR — extended ────────────────────────────────────────────────────────────

#[test]
fn test_vlr_empty_payload() {
    let data = make_vlr("test", 42, b"");
    let (vlr, end) = Vlr::parse(&data, 0).expect("valid vlr with empty payload");
    assert_eq!(vlr.key.user_id, "test");
    assert_eq!(vlr.key.record_id, 42);
    assert!(vlr.data.is_empty());
    assert_eq!(end, 54);
}

#[test]
fn test_vlr_description_stripped_nulls() {
    let mut data = make_vlr("copc", 1, b"data");
    // description is at bytes 22..54 — write a string with trailing nulls
    let desc = b"MyDescription\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
    data[22..54].copy_from_slice(&desc[..32]);
    let (vlr, _) = Vlr::parse(&data, 0).expect("valid");
    assert_eq!(vlr.description, "MyDescription");
}

#[test]
fn test_vlr_offset_nonzero() {
    // Build two VLRs back to back and parse the second one using offset
    let payload1 = b"first";
    let payload2 = b"second";
    let vlr1_bytes = make_vlr("user1", 1, payload1);
    let vlr2_bytes = make_vlr("user2", 2, payload2);
    let mut combined = vlr1_bytes.clone();
    combined.extend_from_slice(&vlr2_bytes);

    let offset = vlr1_bytes.len();
    let (vlr2, _) = Vlr::parse(&combined, offset).expect("valid vlr2");
    assert_eq!(vlr2.key.user_id, "user2");
    assert_eq!(vlr2.key.record_id, 2);
    assert_eq!(vlr2.data, payload2);
}

#[test]
fn test_vlr_too_short_for_header() {
    // Only 53 bytes — one byte short of the minimum VLR header
    let data = vec![0u8; 53];
    assert!(Vlr::parse(&data, 0).is_err());
}

#[test]
fn test_vlr_user_id_exact_16_bytes() {
    // user_id exactly 16 bytes (no null terminator within the 16-byte field)
    let uid = "1234567890123456"; // 16 ASCII chars
    let data = make_vlr(uid, 99, b"p");
    let (vlr, _) = Vlr::parse(&data, 0).expect("valid");
    assert_eq!(vlr.key.user_id, uid);
}

#[test]
fn test_vlr_key_hash_equality() {
    use std::collections::HashMap;
    let mut map: HashMap<VlrKey, &str> = HashMap::new();
    let k = VlrKey {
        user_id: "copc".to_string(),
        record_id: 1,
    };
    map.insert(k.clone(), "copc info");
    assert_eq!(map.get(&k), Some(&"copc info"));
}

#[test]
fn test_vlr_parse_large_payload() {
    let payload = vec![0xABu8; 256];
    let data = make_vlr("biglaszip", 100, &payload);
    let (vlr, end) = Vlr::parse(&data, 0).expect("valid vlr large payload");
    assert_eq!(vlr.data.len(), 256);
    assert_eq!(end, 54 + 256);
}

// ── LasVersion equality ────────────────────────────────────────────────────────

#[test]
fn test_las_version_equality() {
    assert_eq!(LasVersion::V10, LasVersion::V10);
    assert_ne!(LasVersion::V10, LasVersion::V14);
    assert_ne!(LasVersion::V12, LasVersion::V13);
}

#[test]
fn test_las_version_debug_format() {
    let v = LasVersion::V14;
    let s = format!("{v:?}");
    assert!(s.contains("V14"));
}

// ── CopcError variants ────────────────────────────────────────────────────────

#[test]
fn test_copc_error_invalid_format_display() {
    let err = CopcError::InvalidFormat("bad magic".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("bad magic"));
}

#[test]
fn test_copc_error_unsupported_version_display() {
    let err = CopcError::UnsupportedVersion(3, 0);
    let msg = format!("{err}");
    assert!(msg.contains("3.0") || (msg.contains('3') && msg.contains('0')));
}
