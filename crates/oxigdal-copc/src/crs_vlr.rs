//! Structural parsing of coordinate-reference-system (CRS) VLRs.
//!
//! LAS files describe their CRS using either GeoTIFF keys or an OGC WKT string,
//! carried in well-known VLR / EVLR records:
//!
//! | Record ID | Tag | Payload |
//! |-----------|-----|---------|
//! | 34735 | `GeoKeyDirectoryTag`   | array of 16-bit GeoTIFF key entries |
//! | 34736 | `GeoDoubleParamsTag`   | array of `f64` parameters |
//! | 34737 | `GeoAsciiParamsTag`    | NUL-separated ASCII strings |
//! | 2112  | OGC Coordinate System WKT | a single WKT string |
//!
//! This module parses the *structures* only. Semantic interpretation (turning a
//! GeoKeyDirectory or WKT into a projection definition) is intentionally out of
//! scope here so that the COPC crate stays free of any projection dependency.
//!
//! Reference: OGC GeoTIFF 1.1 / ASPRS LAS 1.4 R15.

use crate::copc_vlr::Vlr;
use crate::error::CopcError;
use crate::extended_vlr::ExtendedVlr;

/// Record ID of the GeoTIFF `GeoKeyDirectoryTag`.
pub const RECORD_ID_GEO_KEY_DIRECTORY: u16 = 34735;
/// Record ID of the GeoTIFF `GeoDoubleParamsTag`.
pub const RECORD_ID_GEO_DOUBLE_PARAMS: u16 = 34736;
/// Record ID of the GeoTIFF `GeoAsciiParamsTag`.
pub const RECORD_ID_GEO_ASCII_PARAMS: u16 = 34737;
/// Record ID of the OGC Coordinate System WKT record.
pub const RECORD_ID_OGC_WKT: u16 = 2112;

/// User ID under which GeoTIFF CRS VLRs are stored ("LASF_Projection").
pub const USER_ID_LASF_PROJECTION: &str = "LASF_Projection";

/// A single entry in a GeoTIFF GeoKeyDirectory.
///
/// Each entry is four little-endian `u16` values. When `tiff_tag_location` is
/// zero, `value_offset` *is* the value; otherwise it indexes into the
/// `GeoDoubleParamsTag` (34736) or `GeoAsciiParamsTag` (34737) arrays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoKeyEntry {
    /// GeoTIFF key identifier.
    pub key_id: u16,
    /// Location of the value: 0 = inline, else the source tag's record ID.
    pub tiff_tag_location: u16,
    /// Number of values (1 for inline keys).
    pub count: u16,
    /// Inline value, or an index into the referenced params array.
    pub value_offset: u16,
}

/// A parsed GeoTIFF GeoKeyDirectory (record ID 34735).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoKeyDirectory {
    /// Directory format version (always 1 per the GeoTIFF spec).
    pub key_directory_version: u16,
    /// Key set revision (major).
    pub key_revision: u16,
    /// Key set revision (minor).
    pub minor_revision: u16,
    /// The decoded key entries.
    pub keys: Vec<GeoKeyEntry>,
}

/// Aggregated structural CRS data extracted from a file's VLRs and EVLRs.
///
/// Every field is optional/empty when the corresponding record is absent, so an
/// LAS file with no CRS metadata yields a fully-empty [`CrsInfo`].
#[derive(Debug, Clone, Default)]
pub struct CrsInfo {
    /// Parsed GeoTIFF key directory (record 34735), if present.
    pub geotiff_keys: Option<GeoKeyDirectory>,
    /// GeoTIFF double parameters (record 34736).
    pub geo_doubles: Vec<f64>,
    /// GeoTIFF ASCII parameters (record 34737), raw NUL-separated bytes as text.
    pub geo_ascii: Option<String>,
    /// OGC WKT coordinate system string (record 2112).
    pub wkt: Option<String>,
}

impl CrsInfo {
    /// Returns `true` when no CRS records of any kind were found.
    pub fn is_empty(&self) -> bool {
        self.geotiff_keys.is_none()
            && self.geo_doubles.is_empty()
            && self.geo_ascii.is_none()
            && self.wkt.is_none()
    }
}

/// Read a little-endian `u16` at `offset`, bounds-checking against `data`.
fn read_u16_le(data: &[u8], offset: usize) -> Result<u16, CopcError> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| CopcError::InvalidFormat("GeoKey u16 read offset overflow".into()))?;
    if end > data.len() {
        return Err(CopcError::InvalidFormat(format!(
            "GeoKeyDirectory truncated: need {end} bytes but only {} available",
            data.len()
        )));
    }
    Ok(u16::from_le_bytes([data[offset], data[offset + 1]]))
}

/// Parse a GeoTIFF GeoKeyDirectory (record ID 34735) from its raw payload.
///
/// The payload begins with a 4-`u16` header
/// (`key_directory_version`, `key_revision`, `minor_revision`, `number_of_keys`)
/// followed by `number_of_keys` entries of 4 `u16` each.
///
/// # Errors
/// Returns [`CopcError::InvalidFormat`] when the payload is shorter than the
/// header, or when the declared key count would read past the end of `data`.
pub fn parse_geo_key_directory(data: &[u8]) -> Result<GeoKeyDirectory, CopcError> {
    // 4-u16 header = 8 bytes minimum.
    if data.len() < 8 {
        return Err(CopcError::InvalidFormat(format!(
            "GeoKeyDirectory too short: {} bytes (need >= 8)",
            data.len()
        )));
    }

    let key_directory_version = read_u16_le(data, 0)?;
    let key_revision = read_u16_le(data, 2)?;
    let minor_revision = read_u16_le(data, 4)?;
    let number_of_keys = read_u16_le(data, 6)? as usize;

    // Each key entry is 4 * u16 = 8 bytes; they follow the 8-byte header.
    let bytes_needed =
        8usize
            .checked_add(number_of_keys.checked_mul(8).ok_or_else(|| {
                CopcError::InvalidFormat("GeoKeyDirectory key count overflow".into())
            })?)
            .ok_or_else(|| CopcError::InvalidFormat("GeoKeyDirectory size overflow".into()))?;
    if data.len() < bytes_needed {
        return Err(CopcError::InvalidFormat(format!(
            "GeoKeyDirectory truncated: {number_of_keys} keys need {bytes_needed} bytes \
             but only {} available",
            data.len()
        )));
    }

    let mut keys = Vec::with_capacity(number_of_keys);
    for i in 0..number_of_keys {
        let base = 8 + i * 8;
        keys.push(GeoKeyEntry {
            key_id: read_u16_le(data, base)?,
            tiff_tag_location: read_u16_le(data, base + 2)?,
            count: read_u16_le(data, base + 4)?,
            value_offset: read_u16_le(data, base + 6)?,
        });
    }

    Ok(GeoKeyDirectory {
        key_directory_version,
        key_revision,
        minor_revision,
        keys,
    })
}

/// Decode a `GeoDoubleParamsTag` payload (record 34736) into a `Vec<f64>`.
///
/// Whole little-endian `f64` values are read; any trailing partial value is
/// ignored (the GeoTIFF spec guarantees a multiple of 8 bytes).
fn parse_geo_doubles(data: &[u8]) -> Vec<f64> {
    data.chunks_exact(8)
        .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
        .collect()
}

/// Decode an ASCII payload (records 34737 / 2112) to a trimmed lossy-UTF-8
/// string.
fn parse_ascii(data: &[u8]) -> String {
    String::from_utf8_lossy(data)
        .trim_end_matches('\0')
        .to_string()
}

/// Apply a single record (identified by `record_id` + `data`) to `info`.
///
/// Each arm only fills its slot when still empty, so the first occurrence of a
/// record id wins (classic VLRs are scanned before EVLRs). Malformed
/// GeoKeyDirectory payloads are skipped silently.
fn apply_record(info: &mut CrsInfo, record_id: u16, data: &[u8]) {
    match record_id {
        RECORD_ID_GEO_KEY_DIRECTORY if info.geotiff_keys.is_none() => {
            if let Ok(dir) = parse_geo_key_directory(data) {
                info.geotiff_keys = Some(dir);
            }
        }
        RECORD_ID_GEO_DOUBLE_PARAMS if info.geo_doubles.is_empty() => {
            info.geo_doubles = parse_geo_doubles(data);
        }
        RECORD_ID_GEO_ASCII_PARAMS if info.geo_ascii.is_none() => {
            info.geo_ascii = Some(parse_ascii(data));
        }
        RECORD_ID_OGC_WKT if info.wkt.is_none() => {
            info.wkt = Some(parse_ascii(data));
        }
        _ => {}
    }
}

/// Extract structural CRS information by scanning both classic VLRs and EVLRs.
///
/// Classic VLRs are scanned first; an EVLR only contributes a record when the
/// corresponding classic VLR was absent. Malformed GeoKeyDirectory payloads are
/// silently skipped (leaving `geotiff_keys` as `None`) rather than aborting the
/// whole scan, since CRS metadata is advisory.
pub fn extract_crs_info(vlrs: &[Vlr], evlrs: &[ExtendedVlr]) -> CrsInfo {
    let mut info = CrsInfo::default();

    for vlr in vlrs {
        apply_record(&mut info, vlr.key.record_id, &vlr.data);
    }
    for evlr in evlrs {
        apply_record(&mut info, evlr.record_id, &evlr.data);
    }

    info
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::copc_vlr::VlrKey;

    fn make_vlr(record_id: u16, data: Vec<u8>) -> Vlr {
        Vlr {
            key: VlrKey {
                user_id: USER_ID_LASF_PROJECTION.into(),
                record_id,
            },
            description: String::new(),
            data,
        }
    }

    fn build_geo_key_directory(keys: &[GeoKeyEntry]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u16.to_le_bytes()); // key_directory_version
        buf.extend_from_slice(&1u16.to_le_bytes()); // key_revision
        buf.extend_from_slice(&0u16.to_le_bytes()); // minor_revision
        buf.extend_from_slice(&(keys.len() as u16).to_le_bytes());
        for k in keys {
            buf.extend_from_slice(&k.key_id.to_le_bytes());
            buf.extend_from_slice(&k.tiff_tag_location.to_le_bytes());
            buf.extend_from_slice(&k.count.to_le_bytes());
            buf.extend_from_slice(&k.value_offset.to_le_bytes());
        }
        buf
    }

    #[test]
    fn test_parse_geo_key_directory_minimal() {
        let entry = GeoKeyEntry {
            key_id: 1024,
            tiff_tag_location: 0,
            count: 1,
            value_offset: 1,
        };
        let bytes = build_geo_key_directory(std::slice::from_ref(&entry));
        let dir = parse_geo_key_directory(&bytes).expect("parse");
        assert_eq!(dir.key_directory_version, 1);
        assert_eq!(dir.keys.len(), 1);
        assert_eq!(dir.keys[0], entry);
    }

    #[test]
    fn test_parse_geo_key_directory_too_short() {
        assert!(parse_geo_key_directory(&[0u8; 4]).is_err());
    }

    #[test]
    fn test_parse_geo_key_directory_truncated_keys() {
        // Header says 3 keys but only one is present.
        let mut bytes = build_geo_key_directory(&[GeoKeyEntry {
            key_id: 1,
            tiff_tag_location: 0,
            count: 1,
            value_offset: 0,
        }]);
        bytes[6..8].copy_from_slice(&3u16.to_le_bytes());
        assert!(parse_geo_key_directory(&bytes).is_err());
    }

    #[test]
    fn test_extract_wkt_from_classic_vlr() {
        let wkt = "PROJCS[\"test\"]";
        let info = extract_crs_info(&[make_vlr(RECORD_ID_OGC_WKT, wkt.as_bytes().to_vec())], &[]);
        assert_eq!(info.wkt.as_deref(), Some(wkt));
    }

    #[test]
    fn test_extract_geo_ascii() {
        let mut data = b"WGS 84".to_vec();
        data.push(0); // NUL terminator
        let info = extract_crs_info(&[make_vlr(RECORD_ID_GEO_ASCII_PARAMS, data)], &[]);
        assert_eq!(info.geo_ascii.as_deref(), Some("WGS 84"));
    }

    #[test]
    fn test_extract_geo_doubles() {
        let mut data = Vec::new();
        data.extend_from_slice(&6378137.0f64.to_le_bytes());
        data.extend_from_slice(&298.257_223_563f64.to_le_bytes());
        let info = extract_crs_info(&[make_vlr(RECORD_ID_GEO_DOUBLE_PARAMS, data)], &[]);
        assert_eq!(info.geo_doubles.len(), 2);
        assert!((info.geo_doubles[0] - 6378137.0).abs() < 1e-6);
    }

    #[test]
    fn test_extract_geotiff_keys() {
        let bytes = build_geo_key_directory(&[GeoKeyEntry {
            key_id: 3072,
            tiff_tag_location: 0,
            count: 1,
            value_offset: 32633,
        }]);
        let info = extract_crs_info(&[make_vlr(RECORD_ID_GEO_KEY_DIRECTORY, bytes)], &[]);
        let dir = info.geotiff_keys.expect("keys present");
        assert_eq!(dir.keys[0].value_offset, 32633);
    }

    #[test]
    fn test_extract_absent_all_none() {
        let info = extract_crs_info(&[], &[]);
        assert!(info.is_empty());
        assert!(info.geotiff_keys.is_none());
        assert!(info.geo_doubles.is_empty());
        assert!(info.geo_ascii.is_none());
        assert!(info.wkt.is_none());
    }

    #[test]
    fn test_extract_wkt_from_evlr_fallback() {
        let wkt = "GEOGCS[\"evlr\"]";
        let evlr = ExtendedVlr {
            reserved: 0,
            user_id: [0u8; 16],
            record_id: RECORD_ID_OGC_WKT,
            record_length: wkt.len() as u64,
            description: [0u8; 32],
            data: wkt.as_bytes().to_vec(),
        };
        let info = extract_crs_info(&[], &[evlr]);
        assert_eq!(info.wkt.as_deref(), Some(wkt));
    }
}
