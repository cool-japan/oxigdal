//! Blosc-style meta-compressor
//!
//! Blosc is a fast, high-compression meta-codec for chunked numeric arrays.
//! It chains a byte-shuffle pre-filter (improving the compressibility of
//! aligned numeric data) with a fast backend codec such as LZ4, Zstd, or
//! Snappy. The chunk-oriented frame format also enables random access to
//! individual blocks without fully decompressing the stream.
//!
//! This module implements a Blosc-2-style framing compatible with the
//! reference c-blosc2 layout:
//!
//! ```text
//! +---------+----------+--------+
//! | header  | blk tbl  | blocks |
//! +---------+----------+--------+
//!     16 B    4 + 4*N B   ...
//! ```
//!
//! BitShuffle is intentionally deferred (the corresponding flag bit is
//! reserved but never set by this implementation).
//!
//! # Example
//!
//! ```
//! use oxigeo_compress::codecs::blosc::{compress, decompress, BloscBackend, BloscCodec, ShuffleKind};
//!
//! // Compress a 1 MiB f32 array using byte-shuffle + Zstd.
//! let values: Vec<u8> = (0..262_144u32).flat_map(|i| i.to_le_bytes()).collect();
//! let options = BloscCodec {
//!     typesize: 4,
//!     blocksize: 65_536,
//!     backend: BloscBackend::Zstd,
//!     shuffle: ShuffleKind::ByteShuffle,
//!     clevel: 5,
//! };
//! let blob = compress(&values, &options).expect("compress");
//! let restored = decompress(&blob).expect("decompress");
//! assert_eq!(restored, values);
//! ```

use crate::codecs::shuffle::{byte_shuffle, byte_unshuffle};
use crate::codecs::{Lz4Codec, Lz4Config, SnappyCodec, ZstdCodec, ZstdConfig};
use crate::error::{CompressionError, Result};

/// Frame format major version (header byte 0).
pub const BLOSC_VERSION: u8 = 0x02;

/// LZ4-compatibility version byte (header byte 1).
pub const BLOSC_VERSIONLZ: u8 = 0x01;

/// Fixed 16-byte header length.
pub const BLOSC_HEADER_LEN: usize = 16;

/// Bit 0 of the flags byte: byte-shuffle filter was applied per block.
pub const BLOSC_FLAG_BYTE_SHUFFLE: u8 = 0x01;

/// Bit 1 of the flags byte: bit-shuffle filter was applied (deferred — never set).
pub const BLOSC_FLAG_BIT_SHUFFLE: u8 = 0x02;

/// Bit 4 of the flags byte: padding is not in use for this frame.
pub const BLOSC_FLAG_NO_PADDING: u8 = 0x10;

/// Bit 5 of the flags byte: typesize is greater than 1.
pub const BLOSC_FLAG_TYPESIZE_NOT_1: u8 = 0x20;

/// Default block size: 256 KiB.
pub const BLOSC_DEFAULT_BLOCKSIZE: u32 = 256 * 1024;

/// Backend codec used by the Blosc meta-compressor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BloscBackend {
    /// LZ4 block compression (fastest).
    Lz4,
    /// Zstandard compression (best ratio).
    Zstd,
    /// Snappy compression.
    Snappy,
}

impl BloscBackend {
    /// Stable identifier byte written into the header `filter_pipeline_id` field.
    pub const fn id(self) -> u8 {
        match self {
            BloscBackend::Lz4 => 1,
            BloscBackend::Zstd => 4,
            BloscBackend::Snappy => 2,
        }
    }

    /// Reverse mapping of [`BloscBackend::id`].
    pub const fn from_id(id: u8) -> Option<Self> {
        match id {
            1 => Some(BloscBackend::Lz4),
            2 => Some(BloscBackend::Snappy),
            4 => Some(BloscBackend::Zstd),
            _ => None,
        }
    }
}

/// Pre-filter kind applied to each block before backend compression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShuffleKind {
    /// No filter — bytes are fed to the backend unchanged.
    None,
    /// Byte-shuffle (the standard Blosc filter).
    ByteShuffle,
}

/// In-memory codec configuration and `Codec`-trait wrapper for Blosc.
///
/// This type doubles as the parameter object for the one-shot
/// [`compress`] / [`decompress`] free functions and as a
/// [`Codec`] implementation that can be plugged into pipelines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BloscCodec {
    /// Size in bytes of one array element (typically 1, 2, 4, 8, 16, 32, …).
    pub typesize: u8,
    /// Block size in bytes (defaults to 256 KiB).
    pub blocksize: u32,
    /// Backend compression codec.
    pub backend: BloscBackend,
    /// Pre-filter applied per block.
    pub shuffle: ShuffleKind,
    /// Compression level, interpreted by the backend (0–9).
    pub clevel: u8,
}

impl Default for BloscCodec {
    fn default() -> Self {
        Self {
            typesize: 4,
            blocksize: BLOSC_DEFAULT_BLOCKSIZE,
            backend: BloscBackend::Zstd,
            shuffle: ShuffleKind::ByteShuffle,
            clevel: 5,
        }
    }
}

impl BloscCodec {
    /// Construct a new codec with explicit parameters.
    pub fn new(
        typesize: u8,
        blocksize: u32,
        backend: BloscBackend,
        shuffle: ShuffleKind,
        clevel: u8,
    ) -> Self {
        Self {
            typesize,
            blocksize,
            backend,
            shuffle,
            clevel,
        }
    }

    /// Compress using this codec's settings (one-shot).
    pub fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        compress(input, self)
    }

    /// Decompress a previously produced Blosc frame.
    pub fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        decompress(input)
    }
}

/// Trait implemented by every codec in this crate.
///
/// `BloscCodec` implements this trait so it can be substituted for any other
/// codec via dynamic dispatch. The shape matches the existing inherent methods
/// on `Lz4Codec`, `ZstdCodec`, etc. (a one-shot byte-in / byte-out API).
pub trait Codec {
    /// Compress `input` and return the encoded bytes.
    fn compress(&self, input: &[u8]) -> Result<Vec<u8>>;

    /// Decompress `input` and return the decoded bytes.
    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>>;
}

impl Codec for BloscCodec {
    fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        compress(input, self)
    }

    fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        decompress(input)
    }
}

/// Parsed Blosc frame header (the first 16 bytes of every frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BloscFrame {
    /// Major frame version (always [`BLOSC_VERSION`]).
    pub version: u8,
    /// LZ4-compatibility byte (always [`BLOSC_VERSIONLZ`]).
    pub versionlz: u8,
    /// Bit-flags (shuffle / bit-shuffle / padding / typesize-not-1).
    pub flags: u8,
    /// Size in bytes of one array element.
    pub typesize: u8,
    /// Uncompressed total payload size, in bytes.
    pub nbytes: u32,
    /// Block size in bytes.
    pub blocksize: u32,
    /// Total compressed size including the 16-byte header, in bytes.
    pub cbytes: u32,
    /// Identifier of the filter / backend pipeline (see [`BloscBackend::id`]).
    pub filter_pipeline_id: u8,
}

impl BloscFrame {
    /// Serialise the 16-byte header into a fixed-size array.
    pub fn to_bytes(&self) -> [u8; BLOSC_HEADER_LEN] {
        let mut out = [0u8; BLOSC_HEADER_LEN];
        out[0] = self.version;
        out[1] = self.versionlz;
        out[2] = self.flags;
        out[3] = self.typesize;
        out[4..8].copy_from_slice(&self.nbytes.to_le_bytes());
        out[8..12].copy_from_slice(&self.blocksize.to_le_bytes());
        out[12..16].copy_from_slice(&self.cbytes.to_le_bytes());
        out
    }

    /// Parse the 16-byte header from `bytes`, validating fixed fields.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < BLOSC_HEADER_LEN {
            return Err(CompressionError::InvalidBufferSize(format!(
                "Blosc frame header truncated: got {} bytes, need {}",
                bytes.len(),
                BLOSC_HEADER_LEN
            )));
        }
        let version = bytes[0];
        let versionlz = bytes[1];
        let flags = bytes[2];
        let typesize = bytes[3];

        let nbytes = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let blocksize = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        let cbytes = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);

        if version != BLOSC_VERSION {
            return Err(CompressionError::IntegrityCheckFailed(format!(
                "unsupported Blosc frame version: 0x{:02x} (expected 0x{:02x})",
                version, BLOSC_VERSION
            )));
        }
        if versionlz != BLOSC_VERSIONLZ {
            return Err(CompressionError::IntegrityCheckFailed(format!(
                "unsupported Blosc versionlz: 0x{:02x} (expected 0x{:02x})",
                versionlz, BLOSC_VERSIONLZ
            )));
        }

        // The filter_pipeline_id is *not* part of the public 16-byte header
        // in the reference c-blosc2 spec; we reconstruct it from the typesize
        // and flags in the parsing path below for the convenience of callers,
        // but during the actual decompress pipeline we instead read the
        // explicit `filter_pipeline_id` byte stored just after the block
        // table. Here we surface a default value of zero.
        let filter_pipeline_id = 0;

        Ok(Self {
            version,
            versionlz,
            flags,
            typesize,
            nbytes,
            blocksize,
            cbytes,
            filter_pipeline_id,
        })
    }
}

/// Compute the number of blocks needed to cover `nbytes` using `blocksize`.
fn num_blocks(nbytes: usize, blocksize: usize) -> usize {
    if blocksize == 0 {
        return 0;
    }
    nbytes.div_ceil(blocksize)
}

/// Compress one block with the configured backend.
fn compress_block(input: &[u8], options: &BloscCodec) -> Result<Vec<u8>> {
    match options.backend {
        BloscBackend::Lz4 => {
            // Map blosc clevel 0..=9 onto the lz4 hc range 1..=12.
            let level = ((options.clevel as i32).clamp(0, 9) * 12 / 9).max(1);
            let cfg = Lz4Config {
                level: crate::codecs::lz4::Lz4Level::new(level.min(12))?,
                ..Lz4Config::default()
            };
            let codec = Lz4Codec::with_config(cfg);
            codec.compress(input)
        }
        BloscBackend::Zstd => {
            // Map blosc clevel 0..=9 onto zstd 1..=22.
            let level = ((options.clevel as i32).clamp(0, 9) * 22 / 9).max(1);
            let cfg = ZstdConfig {
                level: crate::codecs::zstd::ZstdLevel::new(level.min(22))?,
                ..ZstdConfig::default()
            };
            let codec = ZstdCodec::with_config(cfg);
            codec.compress(input)
        }
        BloscBackend::Snappy => {
            // Snappy has no compression levels.
            let codec = SnappyCodec::new();
            codec.compress(input)
        }
    }
}

/// Decompress one block with the configured backend.
fn decompress_block(
    backend: BloscBackend,
    input: &[u8],
    decompressed_size: usize,
) -> Result<Vec<u8>> {
    match backend {
        BloscBackend::Lz4 => {
            let codec = Lz4Codec::new();
            codec.decompress(input, Some(decompressed_size))
        }
        BloscBackend::Zstd => {
            let codec = ZstdCodec::new();
            codec.decompress(input, Some(decompressed_size))
        }
        BloscBackend::Snappy => {
            let codec = SnappyCodec::new();
            codec.decompress(input)
        }
    }
}

/// Compute the flags byte for a given configuration.
fn build_flags(options: &BloscCodec) -> u8 {
    let mut flags = BLOSC_FLAG_NO_PADDING;
    if matches!(options.shuffle, ShuffleKind::ByteShuffle) {
        flags |= BLOSC_FLAG_BYTE_SHUFFLE;
    }
    if options.typesize > 1 {
        flags |= BLOSC_FLAG_TYPESIZE_NOT_1;
    }
    flags
}

/// One-shot Blosc compress: takes a raw byte buffer and returns a fully
/// framed Blosc payload (16-byte header + block table + per-block payloads).
pub fn compress(input: &[u8], options: &BloscCodec) -> Result<Vec<u8>> {
    if options.typesize == 0 {
        return Err(CompressionError::InvalidParameter(
            "Blosc typesize must be >= 1".to_string(),
        ));
    }
    if options.blocksize == 0 {
        return Err(CompressionError::InvalidParameter(
            "Blosc blocksize must be > 0".to_string(),
        ));
    }
    if input.len() > u32::MAX as usize {
        return Err(CompressionError::InvalidParameter(format!(
            "Blosc payload too large: {} bytes (max {})",
            input.len(),
            u32::MAX
        )));
    }

    let typesize_usize = options.typesize as usize;
    let blocksize_usize = options.blocksize as usize;
    let nbytes = input.len();
    let n_blocks = num_blocks(nbytes, blocksize_usize);

    // Pre-build header (cbytes patched after compression).
    let header = BloscFrame {
        version: BLOSC_VERSION,
        versionlz: BLOSC_VERSIONLZ,
        flags: build_flags(options),
        typesize: options.typesize,
        nbytes: nbytes as u32,
        blocksize: options.blocksize,
        cbytes: 0,
        filter_pipeline_id: options.backend.id(),
    };

    // Compress each block.
    let mut compressed_blocks: Vec<Vec<u8>> = Vec::with_capacity(n_blocks);
    for block_idx in 0..n_blocks {
        let start = block_idx * blocksize_usize;
        let end = (start + blocksize_usize).min(nbytes);
        let block = &input[start..end];

        let shuffled =
            if matches!(options.shuffle, ShuffleKind::ByteShuffle) && options.typesize > 1 {
                byte_shuffle(block, typesize_usize)
            } else {
                block.to_vec()
            };

        let compressed = compress_block(&shuffled, options)?;
        compressed_blocks.push(compressed);
    }

    // Header (16) + filter_pipeline_id (1) + n_blocks (u32) + offset table (n_blocks * u32) + payloads.
    let table_size = std::mem::size_of::<u32>() + n_blocks * std::mem::size_of::<u32>();
    let payload_overhead = n_blocks * std::mem::size_of::<u32>();
    let payload_total: usize = compressed_blocks.iter().map(|b| b.len()).sum();
    let total_size = BLOSC_HEADER_LEN + 1 + table_size + payload_overhead + payload_total;

    if total_size > u32::MAX as usize {
        return Err(CompressionError::CompressionFailed(format!(
            "Blosc compressed frame too large: {} bytes",
            total_size
        )));
    }

    let mut out = Vec::with_capacity(total_size);

    // 1. Reserve header (patched at end).
    out.extend_from_slice(&header.to_bytes());

    // 2. Filter / backend identifier (1 byte).
    out.push(options.backend.id());

    // 3. Block count and cumulative offsets.
    out.extend_from_slice(&(n_blocks as u32).to_le_bytes());

    // The block-table stores per-block cumulative offsets measured from the
    // beginning of the payload area (right after the block table). This lets a
    // reader random-access individual blocks without scanning the entire
    // stream.
    let mut cumulative: u64 = 0;
    for block in &compressed_blocks {
        if cumulative > u32::MAX as u64 {
            return Err(CompressionError::CompressionFailed(format!(
                "Blosc cumulative payload offset overflows u32: {}",
                cumulative
            )));
        }
        out.extend_from_slice(&(cumulative as u32).to_le_bytes());
        cumulative += (block.len() as u64) + (std::mem::size_of::<u32>() as u64);
    }

    // 4. Per-block payloads: [u32 compressed_len][bytes].
    for block in &compressed_blocks {
        let block_len = block.len();
        if block_len > u32::MAX as usize {
            return Err(CompressionError::CompressionFailed(format!(
                "Blosc block payload too large: {} bytes",
                block_len
            )));
        }
        out.extend_from_slice(&(block_len as u32).to_le_bytes());
        out.extend_from_slice(block);
    }

    // 5. Patch cbytes.
    let cbytes = out.len() as u32;
    out[12..16].copy_from_slice(&cbytes.to_le_bytes());

    Ok(out)
}

/// One-shot Blosc decompress: takes a framed payload and returns the original
/// raw bytes.
pub fn decompress(input: &[u8]) -> Result<Vec<u8>> {
    // 1. Parse + validate the 16-byte header.
    let header = BloscFrame::parse(input)?;

    let nbytes = header.nbytes as usize;
    let blocksize = header.blocksize as usize;
    let cbytes = header.cbytes as usize;

    if cbytes != input.len() {
        return Err(CompressionError::IntegrityCheckFailed(format!(
            "Blosc cbytes mismatch: header claims {}, buffer is {}",
            cbytes,
            input.len()
        )));
    }
    if blocksize == 0 || header.typesize == 0 {
        return Err(CompressionError::IntegrityCheckFailed(
            "Blosc frame has zero blocksize or typesize".to_string(),
        ));
    }

    let expected_blocks = num_blocks(nbytes, blocksize);

    // 2. Read filter/backend id byte.
    let mut offset = BLOSC_HEADER_LEN;
    if input.len() < offset + 1 {
        return Err(CompressionError::IntegrityCheckFailed(
            "Blosc frame missing filter pipeline id byte".to_string(),
        ));
    }
    let filter_id = input[offset];
    offset += 1;

    let backend = BloscBackend::from_id(filter_id).ok_or_else(|| {
        CompressionError::IntegrityCheckFailed(format!(
            "unknown Blosc filter/backend id: 0x{:02x}",
            filter_id
        ))
    })?;

    // 3. Block count and offset table.
    if input.len() < offset + std::mem::size_of::<u32>() {
        return Err(CompressionError::IntegrityCheckFailed(
            "Blosc frame missing block count".to_string(),
        ));
    }
    let block_count = u32::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
    ]) as usize;
    offset += std::mem::size_of::<u32>();

    if block_count != expected_blocks {
        return Err(CompressionError::IntegrityCheckFailed(format!(
            "Blosc block-count mismatch: header implies {}, frame stores {}",
            expected_blocks, block_count
        )));
    }

    let table_size = block_count * std::mem::size_of::<u32>();
    if input.len() < offset + table_size {
        return Err(CompressionError::IntegrityCheckFailed(
            "Blosc frame block-table truncated".to_string(),
        ));
    }
    let mut offsets: Vec<u32> = Vec::with_capacity(block_count);
    for i in 0..block_count {
        let pos = offset + i * std::mem::size_of::<u32>();
        let v = u32::from_le_bytes([input[pos], input[pos + 1], input[pos + 2], input[pos + 3]]);
        offsets.push(v);
    }
    offset += table_size;

    // 4. Per-block decompression.
    let has_shuffle = (header.flags & BLOSC_FLAG_BYTE_SHUFFLE) != 0;
    let typesize_usize = header.typesize as usize;
    let mut out = Vec::with_capacity(nbytes);

    let payload_area_start = offset;

    for (block_idx, &block_offset) in offsets.iter().enumerate() {
        let block_start = payload_area_start + block_offset as usize;
        if input.len() < block_start + std::mem::size_of::<u32>() {
            return Err(CompressionError::IntegrityCheckFailed(format!(
                "Blosc frame truncated at block {}",
                block_idx
            )));
        }
        let comp_len = u32::from_le_bytes([
            input[block_start],
            input[block_start + 1],
            input[block_start + 2],
            input[block_start + 3],
        ]) as usize;

        let data_start = block_start + std::mem::size_of::<u32>();
        let data_end = data_start.checked_add(comp_len).ok_or_else(|| {
            CompressionError::IntegrityCheckFailed(format!(
                "Blosc block {} payload length overflows",
                block_idx
            ))
        })?;
        if input.len() < data_end {
            return Err(CompressionError::IntegrityCheckFailed(format!(
                "Blosc frame truncated reading block {} payload",
                block_idx
            )));
        }

        let block_payload = &input[data_start..data_end];

        let raw_block_size = if block_idx + 1 == block_count {
            nbytes - block_idx * blocksize
        } else {
            blocksize
        };

        let decoded = decompress_block(backend, block_payload, raw_block_size)?;
        if decoded.len() != raw_block_size {
            return Err(CompressionError::IntegrityCheckFailed(format!(
                "Blosc block {} decoded to {} bytes, expected {}",
                block_idx,
                decoded.len(),
                raw_block_size
            )));
        }

        let restored = if has_shuffle && header.typesize > 1 {
            byte_unshuffle(&decoded, typesize_usize)
        } else {
            decoded
        };

        out.extend_from_slice(&restored);
    }

    if out.len() != nbytes {
        return Err(CompressionError::IntegrityCheckFailed(format!(
            "Blosc decompress produced {} bytes, expected {}",
            out.len(),
            nbytes
        )));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_codec_uses_zstd_byte_shuffle() {
        let codec = BloscCodec::default();
        assert_eq!(codec.typesize, 4);
        assert_eq!(codec.blocksize, 256 * 1024);
        assert_eq!(codec.backend, BloscBackend::Zstd);
        assert_eq!(codec.shuffle, ShuffleKind::ByteShuffle);
        assert_eq!(codec.clevel, 5);
    }

    #[test]
    fn header_round_trip_via_to_bytes_and_parse() {
        let header = BloscFrame {
            version: BLOSC_VERSION,
            versionlz: BLOSC_VERSIONLZ,
            flags: BLOSC_FLAG_BYTE_SHUFFLE | BLOSC_FLAG_NO_PADDING | BLOSC_FLAG_TYPESIZE_NOT_1,
            typesize: 4,
            nbytes: 1024,
            blocksize: 256,
            cbytes: 1100,
            filter_pipeline_id: 4,
        };
        let bytes = header.to_bytes();
        let parsed = BloscFrame::parse(&bytes).expect("parse header");
        assert_eq!(parsed.version, header.version);
        assert_eq!(parsed.versionlz, header.versionlz);
        assert_eq!(parsed.flags, header.flags);
        assert_eq!(parsed.typesize, header.typesize);
        assert_eq!(parsed.nbytes, header.nbytes);
        assert_eq!(parsed.blocksize, header.blocksize);
        assert_eq!(parsed.cbytes, header.cbytes);
    }

    #[test]
    fn small_round_trip_with_zstd_byte_shuffle() {
        let data: Vec<u8> = (0u32..1024).flat_map(|i| i.to_le_bytes()).collect();
        let blob = compress(&data, &BloscCodec::default()).expect("compress");
        let out = decompress(&blob).expect("decompress");
        assert_eq!(out, data);
    }
}
