//! Tile content format detection from magic bytes.
//!
//! PMTiles archives declare a `TileType` in the header, but in practice the
//! declared type may be `Unknown` or may not match the actual tile payload.
//! This module sniffs the raw (possibly still compressed) tile bytes and
//! returns the most specific format it can determine.
//!
//! Detection order (most specific first):
//! 1. PNG – 8-byte magic
//! 2. JPEG – 3-byte SOI prefix
//! 3. WebP – RIFF…WEBP 12-byte fingerprint
//! 4. AVIF / HEIF – ISO BMFF `ftyp` box at offset 4
//! 5. GZip – 2-byte ID
//! 6. Zstandard – 4-byte magic
//! 7. MVT (protobuf) – heuristic: valid protobuf tag byte and minimum length
//! 8. Unknown – fallback

use crate::header::TileType;

// ── DetectedTileFormat ────────────────────────────────────────────────────────

/// Tile content format detected from magic bytes / heuristics.
///
/// This is intentionally separate from [`TileType`] because it can also
/// represent compressed wrappers (Gzip, Zstd) that are not tile types on
/// their own, and because the detection is content-driven rather than
/// archive-header-driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetectedTileFormat {
    /// Protocol Buffers – Mapbox Vector Tile.
    ///
    /// Protobuf has no universal magic; detection uses a heuristic based on
    /// the first byte's protobuf tag field (wire type + field number).
    Mvt,
    /// PNG: `\x89PNG\r\n\x1a\n` (8-byte magic).
    Png,
    /// JPEG: `\xff\xd8\xff` (3-byte SOI prefix).
    Jpeg,
    /// WebP: `RIFF....WEBP` (bytes 0–3 + bytes 8–11).
    Webp,
    /// AVIF / HEIF / ISO BMFF: `ftyp` box at byte offset 4.
    Avif,
    /// GZip-compressed data: `\x1f\x8b` (2-byte magic).
    Gzip,
    /// Zstandard-compressed data: `\x28\xb5\x2f\xfd` (4-byte magic).
    Zstd,
    /// Content did not match any known signature.
    Unknown,
}

impl DetectedTileFormat {
    /// Convert to the corresponding [`TileType`] where there is an unambiguous
    /// mapping.  Compression wrappers (`Gzip`, `Zstd`) and `Unknown` return
    /// `None` because they do not map to a single tile type.
    pub fn as_tile_type(self) -> Option<TileType> {
        match self {
            Self::Mvt => Some(TileType::Mvt),
            Self::Png => Some(TileType::Png),
            Self::Jpeg => Some(TileType::Jpeg),
            Self::Webp => Some(TileType::Webp),
            Self::Avif => Some(TileType::Avif),
            Self::Gzip | Self::Zstd | Self::Unknown => None,
        }
    }

    /// MIME type suitable for HTTP `Content-Type` headers.
    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Avif => "image/avif",
            Self::Mvt => "application/vnd.mapbox-vector-tile",
            Self::Gzip => "application/gzip",
            Self::Zstd => "application/zstd",
            Self::Unknown => "application/octet-stream",
        }
    }

    /// Return `true` if this is a raster image format.
    pub fn is_raster(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Webp | Self::Avif)
    }

    /// Return `true` if this is a vector tile format.
    pub fn is_vector(self) -> bool {
        matches!(self, Self::Mvt)
    }

    /// Return `true` if this is a compression wrapper rather than a tile format.
    pub fn is_compressed(self) -> bool {
        matches!(self, Self::Gzip | Self::Zstd)
    }
}

// ── Magic byte constants ──────────────────────────────────────────────────────

/// PNG 8-byte signature: `\x89PNG\r\n\x1a\n`.
const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/// JPEG 3-byte SOI + APP marker prefix: `\xff\xd8\xff`.
const JPEG_MAGIC: [u8; 3] = [0xff, 0xd8, 0xff];

/// GZip 2-byte identifier: `\x1f\x8b`.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Zstandard 4-byte magic: `\x28\xb5\x2f\xfd`.
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

// ── Public detection function ─────────────────────────────────────────────────

/// Detect the tile format from raw (possibly still compressed) tile data.
///
/// Sniffs the leading bytes for well-known magic sequences and falls back to
/// a protobuf heuristic for MVT tiles.  An empty slice always returns
/// [`DetectedTileFormat::Unknown`].
///
/// # Detection order
/// 1. PNG (`\x89PNG\r\n\x1a\n`, 8 bytes)
/// 2. JPEG (`\xff\xd8\xff`, 3 bytes)
/// 3. WebP (`RIFF` at 0–3, `WEBP` at 8–11, 12 bytes total)
/// 4. AVIF/HEIF/ISO-BMFF (`ftyp` at offset 4, 8+ bytes)
/// 5. GZip (`\x1f\x8b`, 2 bytes)
/// 6. Zstd (`\x28\xb5\x2f\xfd`, 4 bytes)
/// 7. MVT heuristic (valid protobuf tag byte, at least 2 bytes)
/// 8. Unknown
pub fn detect_tile_format(data: &[u8]) -> DetectedTileFormat {
    if data.is_empty() {
        return DetectedTileFormat::Unknown;
    }

    // 1. PNG
    if data.len() >= PNG_MAGIC.len() && data[..PNG_MAGIC.len()] == PNG_MAGIC {
        return DetectedTileFormat::Png;
    }

    // 2. JPEG
    if data.len() >= JPEG_MAGIC.len() && data[..JPEG_MAGIC.len()] == JPEG_MAGIC {
        return DetectedTileFormat::Jpeg;
    }

    // 3. WebP: RIFF at [0..4] AND WEBP at [8..12]
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return DetectedTileFormat::Webp;
    }

    // 4. AVIF / HEIF / ISO Base Media File Format: `ftyp` box at offset 4.
    //    The first 4 bytes are the box size (u32 big-endian, value varies),
    //    bytes 4–7 are the box type, which must be exactly `ftyp`.
    if data.len() >= 8 && &data[4..8] == b"ftyp" {
        return DetectedTileFormat::Avif;
    }

    // 5. GZip
    if data.len() >= GZIP_MAGIC.len() && data[..GZIP_MAGIC.len()] == GZIP_MAGIC {
        return DetectedTileFormat::Gzip;
    }

    // 6. Zstandard
    if data.len() >= ZSTD_MAGIC.len() && data[..ZSTD_MAGIC.len()] == ZSTD_MAGIC {
        return DetectedTileFormat::Zstd;
    }

    // 7. MVT heuristic
    if is_likely_mvt(data) {
        return DetectedTileFormat::Mvt;
    }

    DetectedTileFormat::Unknown
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Heuristic: does the data look like a Mapbox Vector Tile (protobuf-encoded)?
///
/// A valid MVT starts with a protobuf tag byte.  The byte encodes:
/// - `wire_type = byte & 0x07`  →  must be in {0, 1, 2, 5} (valid protobuf wire
///   types); wire types 3 and 4 are deprecated/reserved.
/// - `field_number = byte >> 3`  →  must be ≥ 1 (field numbers start at 1).
///
/// The most common first tag in an MVT is `0x1a` (field=3 "layers", wire=2
/// length-delimited), but other tags are possible.
///
/// We additionally require at least 2 bytes so there is content after the tag,
/// matching the minimum meaningful protobuf message size.
fn is_likely_mvt(data: &[u8]) -> bool {
    if data.len() < 2 {
        return false;
    }
    let first = data[0];
    let wire_type = first & 0x07;
    let field_number = first >> 3;
    // Valid protobuf wire types are 0 (varint), 1 (64-bit), 2 (length-delimited),
    // 5 (32-bit).  Wire types 3 and 4 are group start/end (deprecated).
    matches!(wire_type, 0 | 1 | 2 | 5) && field_number >= 1
}

// ── Dominant-format counting helper ──────────────────────────────────────────

/// Accumulator for counting occurrences of each [`DetectedTileFormat`] variant.
///
/// Used by [`dominant_format_from_samples`] to find the mode of a sample set
/// without heap-allocating a `HashMap`.
#[derive(Debug, Default)]
pub(crate) struct FormatCounts {
    pub png: usize,
    pub jpeg: usize,
    pub webp: usize,
    pub avif: usize,
    pub mvt: usize,
    pub gzip: usize,
    pub zstd: usize,
    pub unknown: usize,
}

impl FormatCounts {
    /// Increment the counter for `fmt`.
    pub fn add(&mut self, fmt: DetectedTileFormat) {
        match fmt {
            DetectedTileFormat::Png => self.png += 1,
            DetectedTileFormat::Jpeg => self.jpeg += 1,
            DetectedTileFormat::Webp => self.webp += 1,
            DetectedTileFormat::Avif => self.avif += 1,
            DetectedTileFormat::Mvt => self.mvt += 1,
            DetectedTileFormat::Gzip => self.gzip += 1,
            DetectedTileFormat::Zstd => self.zstd += 1,
            DetectedTileFormat::Unknown => self.unknown += 1,
        }
    }

    /// Return the [`DetectedTileFormat`] variant with the highest count.
    ///
    /// Ties are broken by the order listed in `DetectedTileFormat` (Mvt first).
    /// Returns `None` when all counts are zero (no samples were added).
    pub fn dominant(&self) -> Option<DetectedTileFormat> {
        let pairs: [(usize, DetectedTileFormat); 8] = [
            (self.png, DetectedTileFormat::Png),
            (self.jpeg, DetectedTileFormat::Jpeg),
            (self.webp, DetectedTileFormat::Webp),
            (self.avif, DetectedTileFormat::Avif),
            (self.mvt, DetectedTileFormat::Mvt),
            (self.gzip, DetectedTileFormat::Gzip),
            (self.zstd, DetectedTileFormat::Zstd),
            (self.unknown, DetectedTileFormat::Unknown),
        ];
        let total: usize = pairs.iter().map(|(c, _)| c).sum();
        if total == 0 {
            return None;
        }
        pairs
            .iter()
            .max_by_key(|(count, _)| *count)
            .map(|(_, fmt)| *fmt)
    }
}

// ── Unit tests (module-level) ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_png_magic_bytes() {
        let data = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00];
        assert_eq!(detect_tile_format(&data), DetectedTileFormat::Png);
    }

    #[test]
    fn detect_jpeg_magic_bytes() {
        let data = [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10];
        assert_eq!(detect_tile_format(&data), DetectedTileFormat::Jpeg);
    }

    #[test]
    fn detect_webp_magic_bytes() {
        let mut data = [0u8; 12];
        data[0..4].copy_from_slice(b"RIFF");
        data[4..8].copy_from_slice(&1000u32.to_le_bytes());
        data[8..12].copy_from_slice(b"WEBP");
        assert_eq!(detect_tile_format(&data), DetectedTileFormat::Webp);
    }

    #[test]
    fn detect_avif_ftyp_box() {
        let mut data = [0u8; 12];
        data[0..4].copy_from_slice(&24u32.to_be_bytes()); // box size
        data[4..8].copy_from_slice(b"ftyp");
        data[8..12].copy_from_slice(b"avif");
        assert_eq!(detect_tile_format(&data), DetectedTileFormat::Avif);
    }

    #[test]
    fn detect_gzip_magic_bytes() {
        let data = [0x1f, 0x8b, 0x08, 0x00];
        assert_eq!(detect_tile_format(&data), DetectedTileFormat::Gzip);
    }

    #[test]
    fn detect_zstd_magic_bytes() {
        let data = [0x28, 0xb5, 0x2f, 0xfd, 0x04, 0x00];
        assert_eq!(detect_tile_format(&data), DetectedTileFormat::Zstd);
    }

    #[test]
    fn detect_likely_mvt_valid_protobuf_tag() {
        // 0x1a = field 3 (layers in MVT spec), wire type 2 (length-delimited)
        let data = [0x1a, 0x05, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(detect_tile_format(&data), DetectedTileFormat::Mvt);
    }

    #[test]
    fn detect_empty_returns_unknown() {
        assert_eq!(detect_tile_format(&[]), DetectedTileFormat::Unknown);
    }

    #[test]
    fn format_counts_dominant_single_winner() {
        let mut counts = FormatCounts::default();
        counts.add(DetectedTileFormat::Png);
        counts.add(DetectedTileFormat::Png);
        counts.add(DetectedTileFormat::Jpeg);
        assert_eq!(counts.dominant(), Some(DetectedTileFormat::Png));
    }

    #[test]
    fn format_counts_dominant_empty_returns_none() {
        let counts = FormatCounts::default();
        assert_eq!(counts.dominant(), None);
    }
}
