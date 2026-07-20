//! Extended Variable Length Record (EVLR) chain parsing for LAS 1.4 files.
//!
//! Extended VLRs are located *after* the point data, addressed by two public
//! header fields: `start_of_first_evlr` (a `u64` byte offset from the start of
//! the file) and `number_of_evlrs` (a `u32` count). Unlike classic VLRs they
//! carry a 64-bit `record_length_after_header`, allowing payloads larger than
//! 65 535 bytes.
//!
//! Reference: ASPRS LAS Specification 1.4 R15 (November 2019), section
//! "Extended Variable Length Records (EVLRs)".
//!
//! # EVLR wire layout (60-byte header + payload)
//! | Bytes | Field |
//! |-------|-------|
//! | 2  | Reserved (ignored) |
//! | 16 | User ID (ASCII, null-padded) |
//! | 2  | Record ID (little-endian) |
//! | 8  | Record length after header (little-endian `u64`) |
//! | 32 | Description (ASCII, null-padded) |
//! | N  | Data |

use crate::error::CopcError;
use crate::las_header::LasVersion;

/// Size in bytes of an EVLR header (excluding the variable-length payload).
///
/// Layout: 2 (reserved) + 16 (user id) + 2 (record id) + 8 (record length) +
/// 32 (description) = 60 bytes.
pub const EVLR_HEADER_SIZE: usize = 60;

/// A parsed Extended Variable Length Record (EVLR).
///
/// Mirrors the wire layout one-to-one. Use [`ExtendedVlr::user_id_str`] and
/// [`ExtendedVlr::description_str`] to obtain trimmed UTF-8 views of the
/// null-padded ASCII fields.
#[derive(Debug, Clone)]
pub struct ExtendedVlr {
    /// Reserved field (LAS 1.4 sets this to 0; preserved verbatim).
    pub reserved: u16,
    /// User identifier (16 bytes, null-padded ASCII).
    pub user_id: [u8; 16],
    /// Record identifier (namespaced by `user_id`).
    pub record_id: u16,
    /// Number of payload bytes following the 60-byte header.
    pub record_length: u64,
    /// Human-readable description (32 bytes, null-padded ASCII).
    pub description: [u8; 32],
    /// Raw EVLR payload bytes.
    pub data: Vec<u8>,
}

impl ExtendedVlr {
    /// Return the user ID as a lossy UTF-8 string with trailing NULs trimmed.
    pub fn user_id_str(&self) -> String {
        trim_null_utf8(&self.user_id)
    }

    /// Return the description as a lossy UTF-8 string with trailing NULs trimmed.
    pub fn description_str(&self) -> String {
        trim_null_utf8(&self.description)
    }
}

/// Decode a null-padded ASCII byte field to a trimmed lossy-UTF-8 `String`.
fn trim_null_utf8(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .to_string()
}

/// Returns `true` when the given LAS version supports Extended VLRs.
///
/// Only LAS 1.4 defines the `start_of_first_evlr` / `number_of_evlrs` header
/// fields, so callers should skip EVLR parsing for earlier versions.
pub fn version_supports_evlr(version: &LasVersion) -> bool {
    matches!(version, LasVersion::V14)
}

/// Read a little-endian `u16` at `offset`, bounds-checking against `data`.
fn read_u16_le(data: &[u8], offset: usize) -> Result<u16, CopcError> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| CopcError::InvalidFormat("EVLR u16 read offset overflow".into()))?;
    if end > data.len() {
        return Err(CopcError::InvalidFormat(format!(
            "EVLR truncated: need {end} bytes but only {} available",
            data.len()
        )));
    }
    Ok(u16::from_le_bytes([data[offset], data[offset + 1]]))
}

/// Read a little-endian `u64` at `offset`, bounds-checking against `data`.
fn read_u64_le(data: &[u8], offset: usize) -> Result<u64, CopcError> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| CopcError::InvalidFormat("EVLR u64 read offset overflow".into()))?;
    if end > data.len() {
        return Err(CopcError::InvalidFormat(format!(
            "EVLR truncated: need {end} bytes but only {} available",
            data.len()
        )));
    }
    Ok(u64::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ]))
}

/// Parse a single EVLR starting at `offset` within `data`.
///
/// Returns the parsed record and the byte offset immediately following its
/// payload.
///
/// # Errors
/// Returns [`CopcError::InvalidFormat`] when the header or the declared payload
/// extends past the end of `data`, or when arithmetic would overflow.
pub fn parse_evlr(data: &[u8], offset: usize) -> Result<(ExtendedVlr, usize), CopcError> {
    let header_end = offset
        .checked_add(EVLR_HEADER_SIZE)
        .ok_or_else(|| CopcError::InvalidFormat("EVLR header offset overflow".into()))?;
    if header_end > data.len() {
        return Err(CopcError::InvalidFormat(format!(
            "EVLR header truncated at offset {offset} (need {header_end} bytes, have {})",
            data.len()
        )));
    }

    let reserved = read_u16_le(data, offset)?;

    let mut user_id = [0u8; 16];
    user_id.copy_from_slice(&data[offset + 2..offset + 18]);

    let record_id = read_u16_le(data, offset + 18)?;
    let record_length = read_u64_le(data, offset + 20)?;

    let mut description = [0u8; 32];
    description.copy_from_slice(&data[offset + 28..offset + 60]);

    let data_start = header_end;
    let len_usize = usize::try_from(record_length).map_err(|_| {
        CopcError::InvalidFormat(format!(
            "EVLR record_length {record_length} exceeds platform usize"
        ))
    })?;
    let data_end = data_start
        .checked_add(len_usize)
        .ok_or_else(|| CopcError::InvalidFormat("EVLR payload offset overflow".into()))?;
    if data_end > data.len() {
        return Err(CopcError::InvalidFormat(format!(
            "EVLR payload truncated: need {data_end} bytes but only {} available",
            data.len()
        )));
    }

    Ok((
        ExtendedVlr {
            reserved,
            user_id,
            record_id,
            record_length,
            description,
            data: data[data_start..data_end].to_vec(),
        },
        data_end,
    ))
}

/// Walk the EVLR chain and return every Extended VLR.
///
/// Starting at `start_of_first_evlr`, sequentially parses `number_of_evlrs`
/// records, advancing past each header and payload. A count of zero yields an
/// empty vector without touching `bytes`.
///
/// # Errors
/// Returns [`CopcError::InvalidFormat`] when `start_of_first_evlr` is out of
/// range, when any record is truncated, or when offsets overflow. The function
/// never panics on malformed input.
pub fn parse_evlr_chain(
    bytes: &[u8],
    start_of_first_evlr: u64,
    number_of_evlrs: u32,
) -> Result<Vec<ExtendedVlr>, CopcError> {
    if number_of_evlrs == 0 {
        return Ok(Vec::new());
    }

    let mut offset = usize::try_from(start_of_first_evlr).map_err(|_| {
        CopcError::InvalidFormat(format!(
            "EVLR start offset {start_of_first_evlr} exceeds platform usize"
        ))
    })?;

    if offset > bytes.len() {
        return Err(CopcError::InvalidFormat(format!(
            "EVLR start offset {offset} exceeds file length {}",
            bytes.len()
        )));
    }

    let mut evlrs = Vec::with_capacity(number_of_evlrs as usize);
    for i in 0..number_of_evlrs {
        let (evlr, next_offset) = parse_evlr(bytes, offset).map_err(|e| {
            CopcError::InvalidFormat(format!("Failed to parse EVLR #{i} at offset {offset}: {e}"))
        })?;
        evlrs.push(evlr);
        offset = next_offset;
    }

    Ok(evlrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a single EVLR (60-byte header + payload) and append it to `buf`.
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

    #[test]
    fn test_evlr_header_size_constant() {
        assert_eq!(EVLR_HEADER_SIZE, 60);
    }

    #[test]
    fn test_parse_single_evlr() {
        let mut buf = Vec::new();
        append_evlr(&mut buf, "test_user", 42, b"hello");
        let (evlr, next) = parse_evlr(&buf, 0).expect("parse");
        assert_eq!(evlr.record_id, 42);
        assert_eq!(evlr.record_length, 5);
        assert_eq!(evlr.data, b"hello");
        assert_eq!(next, 60 + 5);
        assert_eq!(evlr.user_id_str(), "test_user");
    }

    #[test]
    fn test_chain_zero_records() {
        let evlrs = parse_evlr_chain(&[], 0, 0).expect("zero");
        assert!(evlrs.is_empty());
    }

    #[test]
    fn test_chain_two_records() {
        let mut buf = vec![0u8; 8]; // some padding before the EVLR region
        let start = buf.len() as u64;
        append_evlr(&mut buf, "first", 1, b"aaaa");
        append_evlr(&mut buf, "second", 2, b"bbbbbb");
        let evlrs = parse_evlr_chain(&buf, start, 2).expect("two");
        assert_eq!(evlrs.len(), 2);
        assert_eq!(evlrs[0].record_id, 1);
        assert_eq!(evlrs[0].data, b"aaaa");
        assert_eq!(evlrs[1].record_id, 2);
        assert_eq!(evlrs[1].data, b"bbbbbb");
    }

    #[test]
    fn test_chain_truncated_errors_no_panic() {
        let mut buf = Vec::new();
        append_evlr(&mut buf, "x", 1, b"payload");
        buf.truncate(30); // chop in the middle of the header
        assert!(parse_evlr_chain(&buf, 0, 1).is_err());
    }

    #[test]
    fn test_start_offset_out_of_range_errors() {
        let buf = vec![0u8; 10];
        assert!(parse_evlr_chain(&buf, 1000, 1).is_err());
    }

    #[test]
    fn test_version_guard() {
        assert!(version_supports_evlr(&LasVersion::V14));
        assert!(!version_supports_evlr(&LasVersion::V13));
        assert!(!version_supports_evlr(&LasVersion::V10));
    }
}
