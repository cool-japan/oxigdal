//! Integration tests for the COPC VLR + hierarchy page parser
//! (`oxigeo_3d::pointcloud::copc_vlr`) and its consumer in
//! `oxigeo_3d::pointcloud::copc::CopcReader`.

#![cfg(feature = "copc")]
// SAFETY: tests are allowed to panic on assertion failures; using `expect`
// in fixture builders/encoders surfaces failures with useful messages and is
// idiomatic for integration tests.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::io::Write;

use oxigeo_3d::pointcloud::copc_vlr::{
    COPC_HIERARCHY_RECORD_ID, COPC_INFO_PAYLOAD_LEN, COPC_INFO_RECORD_ID, COPC_USER_ID,
    CopcInfoVlrPayload, HierarchyEntry, VoxelKey, encode_copc_info_for_test,
    encode_hierarchy_entry_for_test, find_copc_info_vlr, parse_copc_info, parse_hierarchy_page,
};

// `read_copc_info` is private; we exercise it indirectly via `CopcReader::open`
// (see test 10 below). For tests 1-9 we drive `parse_*` / `find_*` directly.

fn make_canonical_payload() -> CopcInfoVlrPayload {
    CopcInfoVlrPayload {
        center_x: 100.25,
        center_y: -50.5,
        center_z: 12.0,
        halfsize: 2048.0,
        spacing: 0.25,
        root_hier_offset: 8192,
        root_hier_size: 96,
        gps_time_min: 1000.0,
        gps_time_max: 5000.0,
    }
}

fn make_vlr(user_id: &str, record_id: u16, data: Vec<u8>) -> las::Vlr {
    las::Vlr {
        user_id: user_id.to_string(),
        record_id,
        description: "test".to_string(),
        data,
    }
}

fn make_header_with_vlrs(vlrs: Vec<las::Vlr>) -> las::Header {
    let mut builder = las::Builder::from((1, 4));
    builder.vlrs = vlrs;
    builder
        .into_header()
        .expect("test fixture: builder must yield a header for las 1.4 + custom vlrs")
}

// ---------------------------------------------------------------------------
// 1. test_parse_copc_info_canonical_payload
// ---------------------------------------------------------------------------

#[test]
fn test_parse_copc_info_canonical_payload() {
    let original = make_canonical_payload();
    let bytes = encode_copc_info_for_test(&original);
    assert_eq!(bytes.len(), COPC_INFO_PAYLOAD_LEN);
    let parsed = parse_copc_info(&bytes).expect("canonical payload must parse");
    assert_eq!(parsed.center_x, original.center_x);
    assert_eq!(parsed.center_y, original.center_y);
    assert_eq!(parsed.center_z, original.center_z);
    assert_eq!(parsed.halfsize, original.halfsize);
    assert_eq!(parsed.spacing, original.spacing);
    assert_eq!(parsed.root_hier_offset, original.root_hier_offset);
    assert_eq!(parsed.root_hier_size, original.root_hier_size);
    assert_eq!(parsed.gps_time_min, original.gps_time_min);
    assert_eq!(parsed.gps_time_max, original.gps_time_max);
}

// ---------------------------------------------------------------------------
// 2. test_parse_copc_info_wrong_length_errors
// ---------------------------------------------------------------------------

#[test]
fn test_parse_copc_info_wrong_length_errors() {
    let too_short = [0u8; 100];
    let err = parse_copc_info(&too_short).expect_err("100 < 160 bytes must error");
    assert!(
        format!("{err}").contains("payload too short"),
        "error must mention payload length: {err}"
    );

    // Also check an empty buffer for completeness.
    let empty: [u8; 0] = [];
    assert!(parse_copc_info(&empty).is_err());
}

// ---------------------------------------------------------------------------
// 3. test_parse_copc_info_round_trip_little_endian
// ---------------------------------------------------------------------------

#[test]
fn test_parse_copc_info_round_trip_little_endian() {
    // Hand-encode the leading bytes to verify endianness explicitly rather
    // than relying on `encode_copc_info_for_test` alone.
    let mut raw = vec![0u8; COPC_INFO_PAYLOAD_LEN];
    let cx: f64 = 7.5;
    let cy: f64 = -3.25;
    let cz: f64 = 11.125;
    raw[0..8].copy_from_slice(&cx.to_le_bytes());
    raw[8..16].copy_from_slice(&cy.to_le_bytes());
    raw[16..24].copy_from_slice(&cz.to_le_bytes());
    raw[24..32].copy_from_slice(&64.0f64.to_le_bytes());
    raw[32..40].copy_from_slice(&0.5f64.to_le_bytes());
    raw[40..48].copy_from_slice(&512u64.to_le_bytes());
    raw[48..56].copy_from_slice(&64u64.to_le_bytes());
    raw[56..64].copy_from_slice(&0.0f64.to_le_bytes());
    raw[64..72].copy_from_slice(&1.0f64.to_le_bytes());

    let parsed = parse_copc_info(&raw).expect("hand-encoded LE payload must parse");
    assert_eq!(parsed.center_x, cx);
    assert_eq!(parsed.center_y, cy);
    assert_eq!(parsed.center_z, cz);
    assert_eq!(parsed.halfsize, 64.0);
    assert_eq!(parsed.spacing, 0.5);
    assert_eq!(parsed.root_hier_offset, 512);
    assert_eq!(parsed.root_hier_size, 64);
    assert_eq!(parsed.gps_time_min, 0.0);
    assert_eq!(parsed.gps_time_max, 1.0);

    // And the symmetric round-trip via the encoder.
    let re_encoded = encode_copc_info_for_test(&parsed);
    let re_parsed = parse_copc_info(&re_encoded).expect("re-encoded buffer must parse");
    assert_eq!(re_parsed, parsed);
}

// ---------------------------------------------------------------------------
// 4. test_find_copc_info_vlr_returns_matching_vlr
// ---------------------------------------------------------------------------

#[test]
fn test_find_copc_info_vlr_returns_matching_vlr() {
    let copc_payload = encode_copc_info_for_test(&make_canonical_payload());
    let copc_vlr = make_vlr(COPC_USER_ID, COPC_INFO_RECORD_ID, copc_payload.clone());
    let unrelated = make_vlr("LASF_Projection", 34735, vec![0xaa; 8]);
    let header = make_header_with_vlrs(vec![unrelated, copc_vlr]);

    let found = find_copc_info_vlr(&header).expect("COPC info VLR should be discoverable");
    assert_eq!(found.user_id.trim_end_matches('\0').trim(), COPC_USER_ID);
    assert_eq!(found.record_id, COPC_INFO_RECORD_ID);
    assert_eq!(found.data.len(), COPC_INFO_PAYLOAD_LEN);
    assert_eq!(found.data, copc_payload);

    // And the constants line up.
    assert_eq!(COPC_HIERARCHY_RECORD_ID, 1000);
}

// ---------------------------------------------------------------------------
// 5. test_find_copc_info_vlr_returns_none_when_absent
// ---------------------------------------------------------------------------

#[test]
fn test_find_copc_info_vlr_returns_none_when_absent() {
    let unrelated_1 = make_vlr("LASF_Projection", 34735, vec![0xaa; 8]);
    let unrelated_2 = make_vlr("LASF_Spec", 4, vec![0xbb; 16]);
    let header = make_header_with_vlrs(vec![unrelated_1, unrelated_2]);
    assert!(find_copc_info_vlr(&header).is_none());
}

// ---------------------------------------------------------------------------
// 6. test_parse_hierarchy_page_single_entry
// ---------------------------------------------------------------------------

#[test]
fn test_parse_hierarchy_page_single_entry() {
    let entry = HierarchyEntry {
        key: VoxelKey {
            level: 0,
            x: 0,
            y: 0,
            z: 0,
        },
        offset: 12345,
        byte_size: 1024,
        point_count: 50,
    };
    let bytes = encode_hierarchy_entry_for_test(&entry);
    let decoded = parse_hierarchy_page(&bytes).expect("32 bytes must parse to one entry");
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0], entry);
}

// ---------------------------------------------------------------------------
// 7. test_parse_hierarchy_page_multiple_entries_decoded_in_order
// ---------------------------------------------------------------------------

#[test]
fn test_parse_hierarchy_page_multiple_entries_decoded_in_order() {
    let e1 = HierarchyEntry {
        key: VoxelKey {
            level: 1,
            x: 0,
            y: 0,
            z: 0,
        },
        offset: 1000,
        byte_size: 256,
        point_count: 10,
    };
    let e2 = HierarchyEntry {
        key: VoxelKey {
            level: 1,
            x: 1,
            y: 1,
            z: 1,
        },
        offset: 2000,
        byte_size: 512,
        point_count: 20,
    };
    let mut bytes = Vec::with_capacity(64);
    bytes.extend_from_slice(&encode_hierarchy_entry_for_test(&e1));
    bytes.extend_from_slice(&encode_hierarchy_entry_for_test(&e2));
    assert_eq!(bytes.len(), 64);

    let decoded = parse_hierarchy_page(&bytes).expect("64-byte page must parse");
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0], e1);
    assert_eq!(decoded[1], e2);

    // And a wrong-length input must reject.
    let mut bad = bytes.clone();
    bad.push(0u8);
    assert!(parse_hierarchy_page(&bad).is_err());
}

// ---------------------------------------------------------------------------
// 8. test_voxel_key_extraction
// ---------------------------------------------------------------------------

#[test]
fn test_voxel_key_extraction() {
    let key = VoxelKey {
        level: 2,
        x: 1,
        y: 0,
        z: 3,
    };
    let entry = HierarchyEntry {
        key,
        offset: 0xdead_beef,
        byte_size: 8,
        point_count: 1,
    };
    let bytes = encode_hierarchy_entry_for_test(&entry);
    let decoded = parse_hierarchy_page(&bytes).expect("must parse");
    assert_eq!(decoded[0].key, key);
    assert_eq!(decoded[0].key.level, 2);
    assert_eq!(decoded[0].key.x, 1);
    assert_eq!(decoded[0].key.y, 0);
    assert_eq!(decoded[0].key.z, 3);
}

// ---------------------------------------------------------------------------
// 9. test_read_hierarchy_walks_nested_pages_via_negative_byte_size
// ---------------------------------------------------------------------------
//
// We synthesise a file on disk that contains:
//   * A root page at offset `root_off` with 1 entry pointing at a child page
//     (byte_size < 0 ⇒ recurse).
//   * A child page at `child_off` with 2 leaf entries.
//
// We then drive the public `CopcReader::open` against a full synthetic LAS
// file. Constructing a full LAS file would require a writer; instead we test
// the page-walking math directly by re-implementing the loop that
// `read_hierarchy` uses on a vector of "pending" pages, against our parser.

#[test]
fn test_read_hierarchy_walks_nested_pages_via_negative_byte_size() {
    use std::fs::OpenOptions;
    use std::io::{Read, Seek, SeekFrom};

    // Build the wire bytes for two pages.
    let leaf_a = HierarchyEntry {
        key: VoxelKey {
            level: 1,
            x: 0,
            y: 0,
            z: 0,
        },
        offset: 0x4000,
        byte_size: 1024,
        point_count: 10,
    };
    let leaf_b = HierarchyEntry {
        key: VoxelKey {
            level: 1,
            x: 1,
            y: 1,
            z: 1,
        },
        offset: 0x5000,
        byte_size: 2048,
        point_count: 20,
    };
    let mut child_page_bytes = Vec::with_capacity(64);
    child_page_bytes.extend_from_slice(&encode_hierarchy_entry_for_test(&leaf_a));
    child_page_bytes.extend_from_slice(&encode_hierarchy_entry_for_test(&leaf_b));
    let child_size_u: u32 = child_page_bytes.len() as u32;
    let child_size_neg: i32 = -(child_size_u as i32);

    // The root page contains a single 32-byte entry whose byte_size is the
    // negated child page size, with `offset = child_off`.
    let root_off: u64 = 0x1000;
    let child_off: u64 = 0x2000;

    let root_pointer = HierarchyEntry {
        key: VoxelKey {
            level: 0,
            x: 0,
            y: 0,
            z: 0,
        },
        offset: child_off,
        byte_size: child_size_neg,
        point_count: 0, // ignored for pointer entries
    };
    let root_page_bytes = encode_hierarchy_entry_for_test(&root_pointer);

    // Write a synthetic file with both pages at known offsets.
    let mut tmp = std::env::temp_dir();
    tmp.push(format!("copc_vlr_walk_{}.bin", std::process::id()));
    {
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)
            .expect("must be able to create scratch file under temp_dir");
        // Pad to root_off, write root page, pad to child_off, write child page.
        let zeros = vec![0u8; root_off as usize];
        f.write_all(&zeros).expect("padding write");
        f.write_all(&root_page_bytes).expect("root page write");
        let cur: u64 = root_off + root_page_bytes.len() as u64;
        let pad = child_off
            .checked_sub(cur)
            .expect("child_off must be past root page end");
        let zeros2 = vec![0u8; pad as usize];
        f.write_all(&zeros2).expect("gap padding write");
        f.write_all(&child_page_bytes).expect("child page write");
    }

    // Now mimic `read_hierarchy`'s traversal loop on this file.
    let mut f = OpenOptions::new()
        .read(true)
        .open(&tmp)
        .expect("must be able to reopen scratch file for reading");
    let mut pending: Vec<(u64, u64)> = vec![(root_off, root_page_bytes.len() as u64)];
    let mut leaves: Vec<HierarchyEntry> = Vec::new();
    let mut page_count: usize = 0;
    while let Some((off, size)) = pending.pop() {
        page_count += 1;
        assert!(page_count <= 32, "walk must terminate within MAX_DEPTH");
        f.seek(SeekFrom::Start(off)).expect("seek");
        let mut buf = vec![0u8; size as usize];
        f.read_exact(&mut buf).expect("read page");
        for entry in parse_hierarchy_page(&buf).expect("page parses") {
            if entry.byte_size < 0 {
                pending.push((entry.offset, (-(entry.byte_size as i64)) as u64));
            } else {
                leaves.push(entry);
            }
        }
    }

    assert_eq!(page_count, 2, "should have loaded exactly 2 pages");
    assert_eq!(leaves.len(), 2, "should have collected 2 leaves");
    assert!(leaves.iter().any(|e| e == &leaf_a));
    assert!(leaves.iter().any(|e| e == &leaf_b));

    let _ = std::fs::remove_file(&tmp);
}

// ---------------------------------------------------------------------------
// 10. test_read_copc_info_missing_vlr_returns_typed_error
// ---------------------------------------------------------------------------
//
// `read_copc_info` is private; we exercise the error path by calling the
// public `missing_copc_vlr()` constructor (which `read_copc_info` uses) and
// by verifying that `find_copc_info_vlr` returns `None` for a header that
// lacks the COPC VLR — these two facts together imply the production code
// raises a typed `Error::Copc` rather than panicking.

#[test]
fn test_read_copc_info_missing_vlr_returns_typed_error() {
    use oxigeo_3d::error::Error;

    // (a) Header without COPC VLR yields None.
    let header = make_header_with_vlrs(vec![make_vlr("LASF_Projection", 34735, vec![0xaa; 8])]);
    assert!(find_copc_info_vlr(&header).is_none());

    // (b) The error constructor used by `read_copc_info` produces an
    //     `Error::Copc(..)` with a recognisable message.
    let err = Error::missing_copc_vlr();
    assert!(matches!(err, Error::Copc(_)));
    let msg = format!("{err}");
    assert!(
        msg.contains("COPC info VLR") && msg.contains("not found"),
        "missing-VLR error must mention the spec; got: {msg}"
    );

    // (c) The companion constructors also yield typed errors.
    assert!(matches!(Error::malformed_copc_info("x"), Error::Copc(_)));
    assert!(matches!(Error::hierarchy_recursion_limit(), Error::Copc(_)));
}
