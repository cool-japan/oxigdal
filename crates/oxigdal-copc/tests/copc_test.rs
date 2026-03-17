//! Integration tests for oxigdal-copc.

use oxigdal_copc::{CopcInfo, LasHeader, LasVersion, Vlr, VlrKey};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a minimal LAS header byte slice (227 bytes) for the given version.
fn make_las_header(major: u8, minor: u8) -> Vec<u8> {
    let mut data = vec![0u8; 227];
    // Magic "LASF"
    data[0..4].copy_from_slice(b"LASF");
    // Version
    data[24] = major;
    data[25] = minor;
    // header_size (LE u16) — use 227
    data[94..96].copy_from_slice(&227u16.to_le_bytes());
    // offset_to_point_data (LE u32)
    data[96..100].copy_from_slice(&227u32.to_le_bytes());
    // number_of_vlrs
    data[100..104].copy_from_slice(&0u32.to_le_bytes());
    // point_data_format_id
    data[104] = 6;
    // point_data_record_length (LE u16)
    data[105..107].copy_from_slice(&30u16.to_le_bytes());
    // legacy point count (LE u32)
    data[107..111].copy_from_slice(&1000u32.to_le_bytes());

    // scale factors at offsets 131, 139, 147 (all f64 LE = 0.001)
    let scale = 0.001f64.to_le_bytes();
    data[131..139].copy_from_slice(&scale);
    data[139..147].copy_from_slice(&scale);
    data[147..155].copy_from_slice(&scale);

    // offset at 155, 163, 171 — all zero

    // bounds: max_x=10.0 at 179, min_x=1.0 at 187, max_y=20.0 at 195, min_y=2.0 at 203,
    //         max_z=5.0 at 211, min_z=0.5 at 219
    data[179..187].copy_from_slice(&10.0f64.to_le_bytes());
    data[187..195].copy_from_slice(&1.0f64.to_le_bytes());
    data[195..203].copy_from_slice(&20.0f64.to_le_bytes());
    data[203..211].copy_from_slice(&2.0f64.to_le_bytes());
    data[211..219].copy_from_slice(&5.0f64.to_le_bytes());
    data[219..227].copy_from_slice(&0.5f64.to_le_bytes());
    data
}

/// Build a 160-byte CopcInfo VLR body with the given centre and halfsize.
fn make_copc_info(cx: f64, cy: f64, cz: f64, halfsize: f64) -> Vec<u8> {
    let mut data = vec![0u8; 160];
    data[0..8].copy_from_slice(&cx.to_le_bytes());
    data[8..16].copy_from_slice(&cy.to_le_bytes());
    data[16..24].copy_from_slice(&cz.to_le_bytes());
    data[24..32].copy_from_slice(&halfsize.to_le_bytes());
    // spacing = 1.0
    data[32..40].copy_from_slice(&1.0f64.to_le_bytes());
    // root_hier_offset = 500, root_hier_size = 200
    data[40..48].copy_from_slice(&500u64.to_le_bytes());
    data[48..56].copy_from_slice(&200u64.to_le_bytes());
    // gpstime min/max = 0.0 / 100.0
    data[56..64].copy_from_slice(&0.0f64.to_le_bytes());
    data[64..72].copy_from_slice(&100.0f64.to_le_bytes());
    data
}

/// Build a minimal VLR (54-byte header + payload).
fn make_vlr(user_id: &str, record_id: u16, payload: &[u8]) -> Vec<u8> {
    let mut data = vec![0u8; 54 + payload.len()];
    // reserved (2 bytes) — leave as zero
    // user_id (16 bytes, null-padded)
    let uid_bytes = user_id.as_bytes();
    let len = uid_bytes.len().min(16);
    data[2..2 + len].copy_from_slice(&uid_bytes[..len]);
    // record_id (LE u16)
    data[18..20].copy_from_slice(&record_id.to_le_bytes());
    // record_length_after_header (LE u16)
    data[20..22].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    // description (32 bytes) — leave as zero
    // payload
    data[54..54 + payload.len()].copy_from_slice(payload);
    data
}

// ── Test 1: LAS magic validation — correct ────────────────────────────────────

#[test]
fn test_las_magic_valid() {
    let data = make_las_header(1, 4);
    assert!(LasHeader::parse(&data).is_ok());
}

// ── Test 2: LAS magic validation — wrong ─────────────────────────────────────

#[test]
fn test_las_magic_invalid() {
    let mut data = make_las_header(1, 4);
    data[0] = b'X';
    assert!(LasHeader::parse(&data).is_err());
}

// ── Test 3: too short → error ─────────────────────────────────────────────────

#[test]
fn test_las_too_short() {
    let data = vec![0u8; 100];
    assert!(LasHeader::parse(&data).is_err());
}

// ── Test 4: LAS 1.4 version parsed correctly ──────────────────────────────────

#[test]
fn test_las_version_14() {
    let data = make_las_header(1, 4);
    let hdr = LasHeader::parse(&data).expect("valid LAS 1.4 header");
    assert_eq!(hdr.version, LasVersion::V14);
}

// ── Test 5: LAS 1.2 version parsed correctly ──────────────────────────────────

#[test]
fn test_las_version_12() {
    let data = make_las_header(1, 2);
    let hdr = LasHeader::parse(&data).expect("valid LAS 1.2 header");
    assert_eq!(hdr.version, LasVersion::V12);
    // Legacy point count at offset 107 should be 1000
    assert_eq!(hdr.number_of_point_records, 1000);
}

// ── Test 6: LasVersion::from_bytes all 5 variants ────────────────────────────

#[test]
fn test_las_version_from_bytes_all() {
    assert_eq!(LasVersion::from_bytes(1, 0), Some(LasVersion::V10));
    assert_eq!(LasVersion::from_bytes(1, 1), Some(LasVersion::V11));
    assert_eq!(LasVersion::from_bytes(1, 2), Some(LasVersion::V12));
    assert_eq!(LasVersion::from_bytes(1, 3), Some(LasVersion::V13));
    assert_eq!(LasVersion::from_bytes(1, 4), Some(LasVersion::V14));
}

// ── Test 7: LasVersion::from_bytes unknown → None ────────────────────────────

#[test]
fn test_las_version_from_bytes_unknown() {
    assert_eq!(LasVersion::from_bytes(2, 0), None);
    assert_eq!(LasVersion::from_bytes(1, 9), None);
}

// ── Test 8: bounds() ──────────────────────────────────────────────────────────

#[test]
fn test_las_bounds() {
    let data = make_las_header(1, 2);
    let hdr = LasHeader::parse(&data).expect("valid");
    let (min, max) = hdr.bounds();
    assert!((min[0] - 1.0).abs() < f64::EPSILON, "min_x");
    assert!((min[1] - 2.0).abs() < f64::EPSILON, "min_y");
    assert!((min[2] - 0.5).abs() < f64::EPSILON, "min_z");
    assert!((max[0] - 10.0).abs() < f64::EPSILON, "max_x");
    assert!((max[1] - 20.0).abs() < f64::EPSILON, "max_y");
    assert!((max[2] - 5.0).abs() < f64::EPSILON, "max_z");
}

// ── Test 9: scale factors parsed ──────────────────────────────────────────────

#[test]
fn test_las_scale_factors() {
    let data = make_las_header(1, 2);
    let hdr = LasHeader::parse(&data).expect("valid");
    assert!((hdr.scale_x - 0.001).abs() < 1e-10);
    assert!((hdr.scale_y - 0.001).abs() < 1e-10);
    assert!((hdr.scale_z - 0.001).abs() < 1e-10);
}

// ── Test 10: CopcInfo parse 160-byte body → Ok ────────────────────────────────

#[test]
fn test_copc_info_parse_ok() {
    let data = make_copc_info(100.0, 200.0, 50.0, 25.0);
    let info = CopcInfo::parse(&data).expect("valid copc info");
    assert!((info.center_x - 100.0).abs() < f64::EPSILON);
    assert!((info.center_y - 200.0).abs() < f64::EPSILON);
    assert!((info.center_z - 50.0).abs() < f64::EPSILON);
    assert!((info.halfsize - 25.0).abs() < f64::EPSILON);
}

// ── Test 11: CopcInfo parse too short → error ─────────────────────────────────

#[test]
fn test_copc_info_too_short() {
    let data = vec![0u8; 100];
    assert!(CopcInfo::parse(&data).is_err());
}

// ── Test 12: CopcInfo bounds() correct ───────────────────────────────────────

#[test]
fn test_copc_info_bounds() {
    // centre=(0,0,0), halfsize=10 → min=(-10,-10,-10), max=(10,10,10)
    let data = make_copc_info(0.0, 0.0, 0.0, 10.0);
    let info = CopcInfo::parse(&data).expect("valid");
    let (min, max) = info.bounds();
    for i in 0..3 {
        assert!((min[i] - (-10.0)).abs() < f64::EPSILON, "min[{i}]");
        assert!((max[i] - 10.0).abs() < f64::EPSILON, "max[{i}]");
    }
}

// ── Test 13: VlrKey equality ──────────────────────────────────────────────────

#[test]
fn test_vlr_key_equality() {
    let k1 = VlrKey {
        user_id: "copc".to_string(),
        record_id: 1,
    };
    let k2 = VlrKey {
        user_id: "copc".to_string(),
        record_id: 1,
    };
    let k3 = VlrKey {
        user_id: "laszip".to_string(),
        record_id: 22204,
    };
    assert_eq!(k1, k2);
    assert_ne!(k1, k3);
}

// ── Test 14: Vlr::parse basic ─────────────────────────────────────────────────

#[test]
fn test_vlr_parse_basic() {
    let payload = b"hello world";
    let data = make_vlr("copc", 1, payload);
    let (vlr, end) = Vlr::parse(&data, 0).expect("valid vlr");
    assert_eq!(vlr.key.user_id, "copc");
    assert_eq!(vlr.key.record_id, 1);
    assert_eq!(vlr.data, payload);
    assert_eq!(end, data.len());
}

// ── Test 15: Vlr::parse truncated → error ─────────────────────────────────────

#[test]
fn test_vlr_parse_truncated() {
    // Build a VLR header that claims a large payload but doesn't supply it.
    let mut data = vec![0u8; 54];
    data[18..20].copy_from_slice(&1u16.to_le_bytes()); // record_id = 1
    data[20..22].copy_from_slice(&1000u16.to_le_bytes()); // claims 1000-byte payload
    // But our data slice is only 54 bytes → truncated
    assert!(Vlr::parse(&data, 0).is_err());
}
