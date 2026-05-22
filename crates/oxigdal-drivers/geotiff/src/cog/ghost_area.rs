//! Ghost-area parser for Cloud Optimized GeoTIFF files.
//!
//! The COG ghost area is a NUL-terminated ASCII KV block written between the
//! TIFF header (8 bytes for classic TIFF, 16 bytes for BigTIFF) and the first
//! IFD. GDAL ≥ 3.1 emits this block when running its `cogger` driver to flag
//! files that follow strict COG layout conventions (16-byte tile alignment,
//! IFD-before-data ordering, etc.).
//!
//! The format is `KEY=VALUE\n` lines, with the entire block terminated by an
//! ASCII NUL byte. This parser is deliberately lenient: unknown keys are
//! preserved in [`GhostArea::raw_kv`] rather than being rejected, so the
//! parser stays useful as the GDAL spec evolves.
//!
//! # Reference
//!
//! - GDAL COG driver: <https://gdal.org/drivers/raster/cog.html>
//! - "GHOST_AREA" keys are documented in the GDAL `cogger` source at
//!   `frmts/gtiff/gtiffdataset_write.cpp` (look for `osLayout`,
//!   `BLOCK_SIZE_X/Y`, `BLOCK_ORDER`, `BLOCK_LEADER_SIZE_AS_UINT4`,
//!   `BLOCK_TRAILER_SIZE_AS_UINT4`, `MASK_INTERLEAVED_WITH_IMAGERY`,
//!   `KNOWN_INCOMPATIBLE_EDITION`).

use oxigdal_core::error::Result;
use oxigdal_core::io::{ByteRange, DataSource};

use crate::tiff::TiffFile;

/// Parsed COG "ghost area" metadata block.
///
/// All recognised keys are surfaced as typed fields. Anything else lands in
/// [`Self::raw_kv`] so the caller can inspect it without needing the parser to
/// understand every dialect of `cogger` output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GhostArea {
    /// `LAYOUT` key (e.g. `"IFDS_BEFORE_DATA"`).
    pub layout: Option<String>,
    /// `BLOCK_SIZE_X` × `BLOCK_SIZE_Y`.
    pub block_size: Option<(u32, u32)>,
    /// `BLOCK_ORDER` key (e.g. `"ROW_MAJOR"`).
    pub block_order: Option<String>,
    /// `BLOCK_LEADER_SIZE_AS_UINT4` (`YES`/`NO`).
    pub block_leader_size_as_uint4: bool,
    /// `BLOCK_TRAILER_SIZE_AS_UINT4` (`YES`/`NO`).
    pub block_trailer_size_as_uint4: bool,
    /// `MASK_INTERLEAVED_WITH_IMAGERY` (`YES`/`NO`).
    pub mask_interleaved_with_imagery: bool,
    /// `KNOWN_INCOMPATIBLE_EDITION` (`YES`/`NO`/absent).
    pub known_incompatible_edition: Option<bool>,
    /// All raw key/value pairs (including ones surfaced as typed fields above).
    pub raw_kv: Vec<(String, String)>,
}

/// Parses the ghost area between the TIFF header and the first IFD.
///
/// Returns `Ok(None)` if there is no gap between the header and the first IFD
/// (i.e. `first_ifd_offset <= header_size()`), if the gap contains no
/// recognisable ASCII key/value pairs, or if the gap is filled with zero bytes.
///
/// # Errors
///
/// Returns an error if the underlying [`DataSource`] fails to read the gap
/// region. Malformed (non-ASCII or oversized) ghost areas yield `Ok(None)` —
/// the parser never panics and never returns a hard error for spec-shaped
/// files.
pub fn parse_ghost_area<S: DataSource>(tiff: &TiffFile, src: &S) -> Result<Option<GhostArea>> {
    let header_size = tiff.header.variant.header_size() as u64;
    let first_ifd_offset = tiff.header.first_ifd_offset;

    // No gap → no ghost area.
    if first_ifd_offset <= header_size {
        return Ok(None);
    }

    let gap_size = first_ifd_offset - header_size;
    if gap_size == 0 {
        return Ok(None);
    }

    // Cap the gap size at 64 KiB. Real ghost areas are tens to a few hundred
    // bytes; anything larger is suspect (or a TIFF with a very strange offset
    // layout) and we should not blindly buffer it.
    const MAX_GHOST_AREA_BYTES: u64 = 64 * 1024;
    if gap_size > MAX_GHOST_AREA_BYTES {
        return Ok(None);
    }

    let bytes = match src.read_range(ByteRange::from_offset_length(header_size, gap_size)) {
        Ok(b) => b,
        Err(_) => {
            // Don't fail callers for unreadable gaps; treat as absent.
            return Ok(None);
        }
    };

    Ok(parse_ghost_bytes(&bytes))
}

/// Parses ghost-area bytes (already extracted from the gap).
///
/// Public for testing convenience.
fn parse_ghost_bytes(bytes: &[u8]) -> Option<GhostArea> {
    // GDAL writes the ghost area as ASCII `KEY=VALUE\n` lines, terminated by a
    // NUL byte. Trim at the first NUL, then split on newlines.
    let nul_pos = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let trimmed = &bytes[..nul_pos];

    if trimmed.is_empty() {
        return None;
    }

    // Reject if not 7-bit ASCII printable + newlines — keeps us from treating
    // arbitrary binary garbage as ghost-area text.
    let ascii_ok = trimmed
        .iter()
        .all(|&b| b == b'\n' || b == b'\r' || b == b'\t' || (0x20..=0x7e).contains(&b));
    if !ascii_ok {
        return None;
    }

    let text = match core::str::from_utf8(trimmed) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let mut raw_kv: Vec<(String, String)> = Vec::new();
    for raw_line in text.split('\n') {
        let line = raw_line.trim_matches(|c: char| c == '\r' || c.is_whitespace());
        if line.is_empty() {
            continue;
        }
        let Some(eq_idx) = line.find('=') else {
            continue;
        };
        let key = line[..eq_idx].trim().to_string();
        let value = line[eq_idx + 1..].trim().to_string();
        if key.is_empty() {
            continue;
        }
        raw_kv.push((key, value));
    }

    if raw_kv.is_empty() {
        return None;
    }

    let mut ghost = GhostArea {
        raw_kv: raw_kv.clone(),
        ..GhostArea::default()
    };

    let mut block_size_x: Option<u32> = None;
    let mut block_size_y: Option<u32> = None;
    for (k, v) in &raw_kv {
        match k.as_str() {
            "LAYOUT" => ghost.layout = Some(v.clone()),
            "BLOCK_SIZE_X" => block_size_x = v.parse().ok(),
            "BLOCK_SIZE_Y" => block_size_y = v.parse().ok(),
            "BLOCK_ORDER" => ghost.block_order = Some(v.clone()),
            "BLOCK_LEADER_SIZE_AS_UINT4" => {
                ghost.block_leader_size_as_uint4 = parse_yes_no(v).unwrap_or(false);
            }
            "BLOCK_TRAILER_SIZE_AS_UINT4" => {
                ghost.block_trailer_size_as_uint4 = parse_yes_no(v).unwrap_or(false);
            }
            "MASK_INTERLEAVED_WITH_IMAGERY" => {
                ghost.mask_interleaved_with_imagery = parse_yes_no(v).unwrap_or(false);
            }
            "KNOWN_INCOMPATIBLE_EDITION" => {
                ghost.known_incompatible_edition = parse_yes_no(v);
            }
            _ => {}
        }
    }

    if let (Some(x), Some(y)) = (block_size_x, block_size_y) {
        ghost.block_size = Some((x, y));
    }

    Some(ghost)
}

/// Parses GDAL's `YES` / `NO` / boolean-ish strings.
fn parse_yes_no(raw: &str) -> Option<bool> {
    let lc = raw.trim().to_ascii_uppercase();
    match lc.as_str() {
        "YES" | "TRUE" | "1" | "ON" => Some(true),
        "NO" | "FALSE" | "0" | "OFF" => Some(false),
        _ => None,
    }
}

/// Convenience: rebuilds the canonical ASCII representation of a ghost area
/// (for tests/round-trip use).
#[must_use]
pub fn render_ghost_area(area: &GhostArea) -> Vec<u8> {
    let mut out = String::new();
    for (k, v) in &area.raw_kv {
        out.push_str(k);
        out.push('=');
        out.push_str(v);
        out.push('\n');
    }
    let mut bytes = out.into_bytes();
    bytes.push(0);
    bytes
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::tiff::{ByteOrderType, TiffHeader};
    use oxigdal_core::error::Result;

    /// In-memory data source for tests.
    struct MemSource(Vec<u8>);

    impl DataSource for MemSource {
        fn size(&self) -> Result<u64> {
            Ok(self.0.len() as u64)
        }

        fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
            let start = range.start as usize;
            let end = range.end as usize;
            if end > self.0.len() {
                return Err(oxigdal_core::error::OxiGdalError::OutOfBounds {
                    message: format!("read past end: {} > {}", end, self.0.len()),
                });
            }
            Ok(self.0[start..end].to_vec())
        }
    }

    fn build_tiff_with_ghost(ghost_bytes: &[u8]) -> (TiffFile, MemSource) {
        // Construct a fake TIFF with header → ghost area → minimal IFD.
        let header_size = 8u64;
        let first_ifd_offset = header_size + ghost_bytes.len() as u64;
        let header = TiffHeader::classic(ByteOrderType::LittleEndian, first_ifd_offset as u32);

        let mut bytes = header.to_bytes();
        bytes.extend_from_slice(ghost_bytes);

        // Minimal IFD: 0 entries, no next pointer
        bytes.extend_from_slice(&[0x00, 0x00]); // 0 entries (u16 LE)
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // next IFD offset = 0

        // We can't go through TiffFile::parse for an empty IFD (it returns an
        // error if there are no usable IFDs after parsing). Instead, build a
        // synthetic TiffFile by hand for the ghost-area parser tests — only
        // the header is read.
        let tiff = TiffFile {
            header,
            ifds: Vec::new(),
        };
        (tiff, MemSource(bytes))
    }

    #[test]
    fn test_ghost_area_absent_returns_none() {
        // Header says first IFD at offset 8 (== header_size for classic) → no gap.
        let header = TiffHeader::classic(ByteOrderType::LittleEndian, 8);
        let bytes = header.to_bytes();
        let tiff = TiffFile {
            header,
            ifds: Vec::new(),
        };
        let src = MemSource(bytes);

        let result = parse_ghost_area(&tiff, &src).expect("should not error");
        assert!(result.is_none(), "no gap → no ghost area");
    }

    #[test]
    fn test_ghost_area_parses_known_keys() {
        let mut payload = Vec::new();
        payload.extend_from_slice(b"GDAL_STRUCTURAL_METADATA_SIZE=000128 bytes\n");
        payload.extend_from_slice(b"LAYOUT=IFDS_BEFORE_DATA\n");
        payload.extend_from_slice(b"BLOCK_SIZE_X=512\n");
        payload.extend_from_slice(b"BLOCK_SIZE_Y=512\n");
        payload.extend_from_slice(b"BLOCK_ORDER=ROW_MAJOR\n");
        payload.extend_from_slice(b"BLOCK_LEADER_SIZE_AS_UINT4=YES\n");
        payload.extend_from_slice(b"BLOCK_TRAILER_SIZE_AS_UINT4=YES\n");
        payload.extend_from_slice(b"MASK_INTERLEAVED_WITH_IMAGERY=NO\n");
        payload.push(0u8); // NUL terminator

        let (tiff, src) = build_tiff_with_ghost(&payload);
        let area = parse_ghost_area(&tiff, &src)
            .expect("read ok")
            .expect("ghost area present");

        assert_eq!(area.layout.as_deref(), Some("IFDS_BEFORE_DATA"));
        assert_eq!(area.block_size, Some((512, 512)));
        assert_eq!(area.block_order.as_deref(), Some("ROW_MAJOR"));
        assert!(area.block_leader_size_as_uint4);
        assert!(area.block_trailer_size_as_uint4);
        assert!(!area.mask_interleaved_with_imagery);
        assert_eq!(area.known_incompatible_edition, None);

        // GDAL_STRUCTURAL_METADATA_SIZE is unknown to typed fields but
        // preserved in raw_kv.
        assert!(
            area.raw_kv
                .iter()
                .any(|(k, _)| k == "GDAL_STRUCTURAL_METADATA_SIZE"),
            "unknown key preserved in raw_kv"
        );
    }

    #[test]
    fn test_ghost_area_preserves_unknown_keys_in_raw_kv() {
        let mut payload = Vec::new();
        payload.extend_from_slice(b"FUTURE_DIALECT_KEY=some-value-42\n");
        payload.extend_from_slice(b"ANOTHER_NEW_KEY=enabled\n");
        payload.extend_from_slice(b"LAYOUT=IFDS_BEFORE_DATA\n");
        payload.push(0u8);

        let (tiff, src) = build_tiff_with_ghost(&payload);
        let area = parse_ghost_area(&tiff, &src)
            .expect("read ok")
            .expect("ghost area present");

        // Typed key still recognised.
        assert_eq!(area.layout.as_deref(), Some("IFDS_BEFORE_DATA"));

        // Unknown keys preserved verbatim.
        let lookup = |key: &str| -> Option<String> {
            area.raw_kv
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(
            lookup("FUTURE_DIALECT_KEY").as_deref(),
            Some("some-value-42")
        );
        assert_eq!(lookup("ANOTHER_NEW_KEY").as_deref(), Some("enabled"));
        assert_eq!(area.raw_kv.len(), 3);
    }

    #[test]
    fn test_render_ghost_area_roundtrip() {
        let area = GhostArea {
            layout: Some("IFDS_BEFORE_DATA".to_string()),
            block_size: Some((256, 256)),
            block_order: None,
            block_leader_size_as_uint4: false,
            block_trailer_size_as_uint4: false,
            mask_interleaved_with_imagery: false,
            known_incompatible_edition: None,
            raw_kv: vec![
                ("LAYOUT".to_string(), "IFDS_BEFORE_DATA".to_string()),
                ("BLOCK_SIZE_X".to_string(), "256".to_string()),
                ("BLOCK_SIZE_Y".to_string(), "256".to_string()),
            ],
        };
        let rendered = render_ghost_area(&area);

        let parsed = parse_ghost_bytes(&rendered).expect("round-trips");
        assert_eq!(parsed.layout, area.layout);
        assert_eq!(parsed.block_size, area.block_size);
        assert_eq!(parsed.raw_kv.len(), 3);
    }

    #[test]
    fn test_ghost_area_non_ascii_returns_none() {
        // Random binary garbage in the gap → not a valid ghost area.
        let payload: Vec<u8> = vec![0xFF, 0xAA, 0x00, 0x12];

        let header_size = 8u64;
        let first_ifd_offset = header_size + payload.len() as u64;
        let header = TiffHeader::classic(ByteOrderType::LittleEndian, first_ifd_offset as u32);

        let mut bytes = header.to_bytes();
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(&[0u8; 6]); // padding so range read succeeds

        let tiff = TiffFile {
            header,
            ifds: Vec::new(),
        };
        let src = MemSource(bytes);

        let result = parse_ghost_area(&tiff, &src).expect("read ok");
        assert!(result.is_none(), "non-ASCII payload should not parse");
    }
}
