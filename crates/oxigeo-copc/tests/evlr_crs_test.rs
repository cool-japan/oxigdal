//! Integration tests for Extended VLR (EVLR) chain parsing and structural CRS
//! VLR extraction in `oxigeo-copc`.
//!
//! Byte fixtures are built in-memory; no files are written. Where a temp file
//! is exercised it uses [`std::env::temp_dir`].

use oxigeo_copc::crs_vlr::{
    CrsInfo, GeoKeyEntry, RECORD_ID_GEO_ASCII_PARAMS, RECORD_ID_GEO_DOUBLE_PARAMS,
    RECORD_ID_GEO_KEY_DIRECTORY, RECORD_ID_OGC_WKT, extract_crs_info, parse_geo_key_directory,
};
use oxigeo_copc::extended_vlr::{
    EVLR_HEADER_SIZE, ExtendedVlr, parse_evlr_chain, version_supports_evlr,
};
use oxigeo_copc::{LasVersion, Vlr, VlrKey};

// ---- byte-fixture helpers ----

/// Append a single EVLR (60-byte header + payload) to `buf`.
fn append_evlr(buf: &mut Vec<u8>, user_id: &str, record_id: u16, payload: &[u8]) {
    buf.extend_from_slice(&0u16.to_le_bytes()); // reserved
    let uid = user_id.as_bytes();
    let mut uid_buf = [0u8; 16];
    let len = uid.len().min(16);
    uid_buf[..len].copy_from_slice(&uid[..len]);
    buf.extend_from_slice(&uid_buf);
    buf.extend_from_slice(&record_id.to_le_bytes());
    buf.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    buf.extend_from_slice(&[0u8; 32]); // description
    buf.extend_from_slice(payload);
}

/// Append a single EVLR whose user_id / description carry trailing NUL padding.
fn append_evlr_named(
    buf: &mut Vec<u8>,
    user_id: &[u8; 16],
    description: &[u8; 32],
    record_id: u16,
    payload: &[u8],
) {
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(user_id);
    buf.extend_from_slice(&record_id.to_le_bytes());
    buf.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    buf.extend_from_slice(description);
    buf.extend_from_slice(payload);
}

/// Build a GeoKeyDirectory payload from a header revision and key entries.
fn build_geo_key_directory(
    key_dir_version: u16,
    key_revision: u16,
    minor_revision: u16,
    keys: &[GeoKeyEntry],
) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&key_dir_version.to_le_bytes());
    buf.extend_from_slice(&key_revision.to_le_bytes());
    buf.extend_from_slice(&minor_revision.to_le_bytes());
    buf.extend_from_slice(&(keys.len() as u16).to_le_bytes());
    for k in keys {
        buf.extend_from_slice(&k.key_id.to_le_bytes());
        buf.extend_from_slice(&k.tiff_tag_location.to_le_bytes());
        buf.extend_from_slice(&k.count.to_le_bytes());
        buf.extend_from_slice(&k.value_offset.to_le_bytes());
    }
    buf
}

fn make_vlr(record_id: u16, data: Vec<u8>) -> Vlr {
    Vlr {
        key: VlrKey {
            user_id: "LASF_Projection".into(),
            record_id,
        },
        description: String::new(),
        data,
    }
}

fn make_evlr(record_id: u16, data: Vec<u8>) -> ExtendedVlr {
    ExtendedVlr {
        reserved: 0,
        user_id: [0u8; 16],
        record_id,
        record_length: data.len() as u64,
        description: [0u8; 32],
        data,
    }
}

// ---- EVLR tests ----

#[test]
fn test_evlr_header_size_60() {
    assert_eq!(EVLR_HEADER_SIZE, 60);
}

#[test]
fn test_evlr_chain_walk_two_records() {
    let mut buf = vec![0u8; 16]; // padding standing in for header + points
    let start = buf.len() as u64;
    append_evlr(&mut buf, "copc", 1000, b"hierarchy-bytes");
    append_evlr(
        &mut buf,
        "LASF_Projection",
        RECORD_ID_OGC_WKT,
        b"PROJCS[..]",
    );
    let evlrs = parse_evlr_chain(&buf, start, 2).expect("walk two EVLRs");
    assert_eq!(evlrs.len(), 2);
    assert_eq!(evlrs[0].record_id, 1000);
    assert_eq!(evlrs[0].data, b"hierarchy-bytes");
    assert_eq!(evlrs[0].record_length, b"hierarchy-bytes".len() as u64);
    assert_eq!(evlrs[1].record_id, RECORD_ID_OGC_WKT);
    assert_eq!(evlrs[1].data, b"PROJCS[..]");
}

#[test]
fn test_evlr_zero_count_returns_empty() {
    let buf = vec![0u8; 100];
    let evlrs = parse_evlr_chain(&buf, 50, 0).expect("zero count");
    assert!(evlrs.is_empty());
    // Even with a wild start offset, a zero count must not error.
    assert!(
        parse_evlr_chain(&[], 9_999_999, 0)
            .expect("zero")
            .is_empty()
    );
}

#[test]
fn test_evlr_truncated_returns_error_no_panic() {
    let mut buf = Vec::new();
    append_evlr(&mut buf, "copc", 1000, b"some-payload-bytes");
    // Cut into the declared payload region so the record is truncated.
    buf.truncate(EVLR_HEADER_SIZE + 4);
    let result = parse_evlr_chain(&buf, 0, 1);
    assert!(
        result.is_err(),
        "truncated payload must return Err, not panic"
    );

    // Also: header itself truncated.
    let result2 = parse_evlr_chain(&buf[..30], 0, 1);
    assert!(result2.is_err());
}

#[test]
fn test_evlr_user_id_description_trimmed() {
    let mut user_id = [0u8; 16];
    user_id[..4].copy_from_slice(b"copc");
    let mut description = [0u8; 32];
    description[..11].copy_from_slice(b"COPC EVLR\0\0");
    let mut buf = Vec::new();
    append_evlr_named(&mut buf, &user_id, &description, 1000, b"data");
    let evlrs = parse_evlr_chain(&buf, 0, 1).expect("parse");
    assert_eq!(evlrs[0].user_id_str(), "copc");
    assert_eq!(evlrs[0].description_str(), "COPC EVLR");
    // The trimmed string carries no embedded trailing NUL bytes.
    assert!(!evlrs[0].user_id_str().contains('\0'));
}

#[test]
fn test_evlr_guarded_on_version_minor_lt_4() {
    // The version guard only admits LAS 1.4.
    assert!(version_supports_evlr(&LasVersion::V14));
    assert!(!version_supports_evlr(&LasVersion::V13));
    assert!(!version_supports_evlr(&LasVersion::V12));
    assert!(!version_supports_evlr(&LasVersion::V11));
    assert!(!version_supports_evlr(&LasVersion::V10));
}

// ---- GeoKeyDirectory tests ----

#[test]
fn test_geo_key_directory_parse_minimal() {
    let entry = GeoKeyEntry {
        key_id: 1024, // GTModelTypeGeoKey
        tiff_tag_location: 0,
        count: 1,
        value_offset: 1,
    };
    let bytes = build_geo_key_directory(1, 1, 0, std::slice::from_ref(&entry));
    let dir = parse_geo_key_directory(&bytes).expect("minimal parse");
    assert_eq!(dir.key_directory_version, 1);
    assert_eq!(dir.key_revision, 1);
    assert_eq!(dir.minor_revision, 0);
    assert_eq!(dir.keys.len(), 1);
    assert_eq!(dir.keys[0], entry);
}

#[test]
fn test_geo_key_directory_entry_count_matches_header() {
    let keys = vec![
        GeoKeyEntry {
            key_id: 1024,
            tiff_tag_location: 0,
            count: 1,
            value_offset: 1,
        },
        GeoKeyEntry {
            key_id: 3072, // ProjectedCSTypeGeoKey
            tiff_tag_location: 0,
            count: 1,
            value_offset: 32633, // UTM 33N
        },
        GeoKeyEntry {
            key_id: 2048, // GeographicTypeGeoKey
            tiff_tag_location: 0,
            count: 1,
            value_offset: 4326,
        },
    ];
    let bytes = build_geo_key_directory(1, 1, 0, &keys);
    let dir = parse_geo_key_directory(&bytes).expect("parse");
    assert_eq!(dir.keys.len(), 3);
    assert_eq!(dir.keys[1].value_offset, 32633);
    assert_eq!(dir.keys[2].key_id, 2048);
}

#[test]
fn test_geo_key_directory_truncated_errors() {
    // Header advertises 5 keys but the payload only carries 1.
    let mut bytes = build_geo_key_directory(
        1,
        1,
        0,
        &[GeoKeyEntry {
            key_id: 1,
            tiff_tag_location: 0,
            count: 1,
            value_offset: 0,
        }],
    );
    bytes[6..8].copy_from_slice(&5u16.to_le_bytes());
    assert!(parse_geo_key_directory(&bytes).is_err());

    // Payload shorter than the 8-byte header is also an error.
    assert!(parse_geo_key_directory(&[0u8; 6]).is_err());
}

// ---- CRS extraction tests ----

#[test]
fn test_crs_info_extracts_wkt_record_2112() {
    let wkt = "COMPD_CS[\"WGS 84 + EGM2008\",GEOGCS[\"WGS 84\"]]";
    let vlrs = vec![make_vlr(RECORD_ID_OGC_WKT, wkt.as_bytes().to_vec())];
    let info = extract_crs_info(&vlrs, &[]);
    assert_eq!(info.wkt.as_deref(), Some(wkt));
    assert!(info.geotiff_keys.is_none());

    // Same record stored as an EVLR is also picked up.
    let evlrs = vec![make_evlr(RECORD_ID_OGC_WKT, wkt.as_bytes().to_vec())];
    let info2 = extract_crs_info(&[], &evlrs);
    assert_eq!(info2.wkt.as_deref(), Some(wkt));
}

#[test]
fn test_crs_info_extracts_geo_ascii_34737() {
    let mut data = b"UTM Zone 33N|WGS 84".to_vec();
    data.push(0); // trailing NUL
    let vlrs = vec![make_vlr(RECORD_ID_GEO_ASCII_PARAMS, data)];
    let info = extract_crs_info(&vlrs, &[]);
    assert_eq!(info.geo_ascii.as_deref(), Some("UTM Zone 33N|WGS 84"));
}

#[test]
fn test_crs_info_geotiff_keys_34735() {
    let keys = vec![GeoKeyEntry {
        key_id: 3072,
        tiff_tag_location: 0,
        count: 1,
        value_offset: 32633,
    }];
    let dir_bytes = build_geo_key_directory(1, 1, 0, &keys);

    // GeoDoubleParamsTag with two values, attached as an EVLR fallback.
    let mut dbl = Vec::new();
    dbl.extend_from_slice(&6378137.0f64.to_le_bytes());
    dbl.extend_from_slice(&298.257_223_563f64.to_le_bytes());

    let vlrs = vec![make_vlr(RECORD_ID_GEO_KEY_DIRECTORY, dir_bytes)];
    let evlrs = vec![make_evlr(RECORD_ID_GEO_DOUBLE_PARAMS, dbl)];
    let info = extract_crs_info(&vlrs, &evlrs);

    let dir = info.geotiff_keys.expect("geotiff keys present");
    assert_eq!(dir.keys.len(), 1);
    assert_eq!(dir.keys[0].key_id, 3072);
    assert_eq!(dir.keys[0].value_offset, 32633);
    assert_eq!(info.geo_doubles.len(), 2);
    assert!((info.geo_doubles[0] - 6378137.0).abs() < 1e-6);
}

#[test]
fn test_crs_info_absent_returns_all_none() {
    let info: CrsInfo = extract_crs_info(&[], &[]);
    assert!(info.is_empty());
    assert!(info.geotiff_keys.is_none());
    assert!(info.geo_doubles.is_empty());
    assert!(info.geo_ascii.is_none());
    assert!(info.wkt.is_none());

    // Non-CRS VLRs/EVLRs must not contribute anything.
    let vlrs = vec![make_vlr(22204, vec![1, 2, 3])]; // laszip-like record id
    let evlrs = vec![make_evlr(999, vec![4, 5, 6])];
    let info2 = extract_crs_info(&vlrs, &evlrs);
    assert!(info2.is_empty());
}

#[test]
fn test_crs_classic_vlr_takes_precedence_over_evlr() {
    // When the same record id appears in both, the classic VLR wins.
    let classic = "CLASSIC_WKT";
    let evlr_wkt = "EVLR_WKT";
    let vlrs = vec![make_vlr(RECORD_ID_OGC_WKT, classic.as_bytes().to_vec())];
    let evlrs = vec![make_evlr(RECORD_ID_OGC_WKT, evlr_wkt.as_bytes().to_vec())];
    let info = extract_crs_info(&vlrs, &evlrs);
    assert_eq!(info.wkt.as_deref(), Some(classic));
}
