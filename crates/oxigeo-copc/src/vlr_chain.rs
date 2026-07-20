//! VLR chain parsing: iterate through all Variable Length Records in a LAS file.
//!
//! Builds on [`Vlr::parse`](crate::copc_vlr::Vlr::parse) to walk the full VLR
//! sequence after the public header and locate COPC-specific records.

use crate::copc_vlr::{CopcInfo, Vlr};
use crate::error::CopcError;
use crate::las_header::LasHeader;

/// Parse all VLRs from the raw file bytes using information in the LAS header.
///
/// Iterates `header.number_of_vlrs` times starting at `header.header_size` bytes
/// from the beginning of `data`.
///
/// # Errors
/// Returns [`CopcError::InvalidFormat`] when the data is truncated or a VLR is
/// malformed.
pub fn parse_vlrs(data: &[u8], header: &LasHeader) -> Result<Vec<Vlr>, CopcError> {
    let mut vlrs = Vec::with_capacity(header.number_of_vlrs as usize);
    let mut offset = header.header_size as usize;

    for i in 0..header.number_of_vlrs {
        let (vlr, next_offset) = Vlr::parse(data, offset).map_err(|e| {
            CopcError::InvalidFormat(format!("Failed to parse VLR #{i} at offset {offset}: {e}"))
        })?;
        vlrs.push(vlr);
        offset = next_offset;
    }

    Ok(vlrs)
}

/// Find the COPC info VLR (user_id = "copc", record_id = 1) and parse its body.
///
/// # Errors
/// Returns [`CopcError::InvalidFormat`] when no COPC info VLR is found or its
/// body cannot be parsed.
pub fn find_copc_info(vlrs: &[Vlr]) -> Result<CopcInfo, CopcError> {
    let vlr = vlrs
        .iter()
        .find(|v| v.key.user_id == "copc" && v.key.record_id == 1)
        .ok_or_else(|| {
            CopcError::InvalidFormat("No COPC info VLR (user_id=\"copc\", record_id=1)".into())
        })?;
    CopcInfo::parse(&vlr.data)
}

/// Find the COPC hierarchy VLR (user_id = "copc", record_id = 1000).
///
/// Returns a reference to the VLR payload bytes.
///
/// # Errors
/// Returns [`CopcError::InvalidFormat`] when no hierarchy VLR is present.
pub fn find_copc_hierarchy_vlr(vlrs: &[Vlr]) -> Result<&Vlr, CopcError> {
    vlrs.iter()
        .find(|v| v.key.user_id == "copc" && v.key.record_id == 1000)
        .ok_or_else(|| {
            CopcError::InvalidFormat(
                "No COPC hierarchy VLR (user_id=\"copc\", record_id=1000)".into(),
            )
        })
}

/// Find the COPC hierarchy record (user_id = "copc", record_id = 1000) among
/// Extended VLRs, used as a fallback when it is absent from the classic VLR
/// chain.
///
/// Large COPC hierarchies may be stored as an EVLR after the point data rather
/// than as a classic VLR. This helper scans the EVLR list for that record and
/// returns a reference to it.
///
/// # Errors
/// Returns [`CopcError::InvalidFormat`] when no matching EVLR is present.
pub fn find_copc_hierarchy_in_evlrs(
    evlrs: &[crate::extended_vlr::ExtendedVlr],
) -> Result<&crate::extended_vlr::ExtendedVlr, CopcError> {
    evlrs
        .iter()
        .find(|e| e.user_id_str() == "copc" && e.record_id == 1000)
        .ok_or_else(|| {
            CopcError::InvalidFormat(
                "No COPC hierarchy EVLR (user_id=\"copc\", record_id=1000)".into(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal LAS header with `n_vlrs` VLRs starting at offset 227.
    fn make_header_for_vlrs(n_vlrs: u32) -> Vec<u8> {
        let mut data = vec![0u8; 227];
        data[0..4].copy_from_slice(b"LASF");
        data[24] = 1;
        data[25] = 4;
        data[94..96].copy_from_slice(&227u16.to_le_bytes());
        data[96..100].copy_from_slice(&227u32.to_le_bytes());
        data[100..104].copy_from_slice(&n_vlrs.to_le_bytes());
        data[104] = 6; // format id
        data[105..107].copy_from_slice(&30u16.to_le_bytes());
        let scale = 0.001f64.to_le_bytes();
        data[131..139].copy_from_slice(&scale);
        data[139..147].copy_from_slice(&scale);
        data[147..155].copy_from_slice(&scale);
        data
    }

    /// Append a VLR to a byte buffer.
    fn append_vlr(buf: &mut Vec<u8>, user_id: &str, record_id: u16, payload: &[u8]) {
        // reserved (2 bytes)
        buf.extend_from_slice(&[0u8; 2]);
        // user_id (16 bytes, null-padded)
        let uid_bytes = user_id.as_bytes();
        let mut uid_buf = [0u8; 16];
        let len = uid_bytes.len().min(16);
        uid_buf[..len].copy_from_slice(&uid_bytes[..len]);
        buf.extend_from_slice(&uid_buf);
        // record_id
        buf.extend_from_slice(&record_id.to_le_bytes());
        // record_length_after_header
        buf.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        // description (32 bytes)
        buf.extend_from_slice(&[0u8; 32]);
        // payload
        buf.extend_from_slice(payload);
    }

    /// Build a 160-byte CopcInfo VLR body.
    fn make_copc_info_payload() -> Vec<u8> {
        let mut data = vec![0u8; 160];
        data[0..8].copy_from_slice(&50.0f64.to_le_bytes()); // center_x
        data[8..16].copy_from_slice(&50.0f64.to_le_bytes()); // center_y
        data[16..24].copy_from_slice(&25.0f64.to_le_bytes()); // center_z
        data[24..32].copy_from_slice(&50.0f64.to_le_bytes()); // halfsize
        data[32..40].copy_from_slice(&1.0f64.to_le_bytes()); // spacing
        data[40..48].copy_from_slice(&500u64.to_le_bytes()); // root_hier_offset
        data[48..56].copy_from_slice(&200u64.to_le_bytes()); // root_hier_size
        data
    }

    #[test]
    fn test_parse_vlrs_zero() {
        let data = make_header_for_vlrs(0);
        let header = LasHeader::parse(&data).expect("valid header");
        let vlrs = parse_vlrs(&data, &header).expect("zero VLRs should succeed");
        assert!(vlrs.is_empty());
    }

    #[test]
    fn test_parse_vlrs_two() {
        let mut data = make_header_for_vlrs(2);
        append_vlr(&mut data, "copc", 1, &make_copc_info_payload());
        append_vlr(&mut data, "copc", 1000, b"hierarchy_data");
        let header = LasHeader::parse(&data).expect("valid header");
        let vlrs = parse_vlrs(&data, &header).expect("should parse 2 VLRs");
        assert_eq!(vlrs.len(), 2);
        assert_eq!(vlrs[0].key.user_id, "copc");
        assert_eq!(vlrs[0].key.record_id, 1);
        assert_eq!(vlrs[1].key.user_id, "copc");
        assert_eq!(vlrs[1].key.record_id, 1000);
    }

    #[test]
    fn test_parse_vlrs_truncated_data_errors() {
        // Header claims 1 VLR but data ends at header
        let data = make_header_for_vlrs(1);
        let header = LasHeader::parse(&data).expect("valid header");
        assert!(parse_vlrs(&data, &header).is_err());
    }

    #[test]
    fn test_find_copc_info_present() {
        let mut data = make_header_for_vlrs(2);
        append_vlr(&mut data, "other", 99, b"something");
        append_vlr(&mut data, "copc", 1, &make_copc_info_payload());
        let header = LasHeader::parse(&data).expect("valid header");
        let vlrs = parse_vlrs(&data, &header).expect("parse vlrs");
        let info = find_copc_info(&vlrs).expect("should find COPC info");
        assert!((info.center_x - 50.0).abs() < f64::EPSILON);
        assert!((info.halfsize - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_find_copc_info_missing() {
        let vlrs = vec![Vlr {
            key: crate::copc_vlr::VlrKey {
                user_id: "laszip".into(),
                record_id: 22204,
            },
            description: String::new(),
            data: vec![0u8; 10],
        }];
        assert!(find_copc_info(&vlrs).is_err());
    }

    #[test]
    fn test_find_copc_hierarchy_vlr_present() {
        let mut data = make_header_for_vlrs(2);
        append_vlr(&mut data, "copc", 1, &make_copc_info_payload());
        append_vlr(&mut data, "copc", 1000, b"hier_bytes");
        let header = LasHeader::parse(&data).expect("valid header");
        let vlrs = parse_vlrs(&data, &header).expect("parse vlrs");
        let hier_vlr = find_copc_hierarchy_vlr(&vlrs).expect("should find hierarchy VLR");
        assert_eq!(hier_vlr.key.record_id, 1000);
        assert_eq!(hier_vlr.data, b"hier_bytes");
    }

    #[test]
    fn test_find_copc_hierarchy_vlr_missing() {
        let vlrs: Vec<Vlr> = Vec::new();
        assert!(find_copc_hierarchy_vlr(&vlrs).is_err());
    }

    #[test]
    fn test_parse_vlrs_three_mixed() {
        let mut data = make_header_for_vlrs(3);
        append_vlr(&mut data, "laszip", 22204, b"lz_data");
        append_vlr(&mut data, "copc", 1, &make_copc_info_payload());
        append_vlr(&mut data, "copc", 1000, b"hier");
        let header = LasHeader::parse(&data).expect("valid header");
        let vlrs = parse_vlrs(&data, &header).expect("parse vlrs");
        assert_eq!(vlrs.len(), 3);
        assert_eq!(vlrs[0].key.user_id, "laszip");
        assert_eq!(vlrs[1].key.user_id, "copc");
        assert_eq!(vlrs[2].key.user_id, "copc");
    }

    #[test]
    fn test_parse_vlrs_empty_payload() {
        let mut data = make_header_for_vlrs(1);
        append_vlr(&mut data, "test", 42, b"");
        let header = LasHeader::parse(&data).expect("valid header");
        let vlrs = parse_vlrs(&data, &header).expect("parse vlrs");
        assert_eq!(vlrs.len(), 1);
        assert!(vlrs[0].data.is_empty());
    }
}
