//! Codec chaining pipeline — compose lossless codecs into a single composite codec.
//!
//! A [`CodecPipeline`] walks its stages forward on compress and in reverse on
//! decompress. The compressed output begins with a self-describing frame header
//! so [`CodecPipeline::decompress_self_describing`] can reconstruct the stage
//! list without the caller knowing the original pipeline.
//!
//! # Frame header
//!
//! The frame header is fixed-stride and self-describing:
//!
//! ```text
//! [ 'O', 'X', 'P', 'L', version=1, num_stages, (stage_id, typesize) * num_stages ]
//! ```
//!
//! The header length is therefore `6 + 2 * num_stages` bytes. The `typesize`
//! byte is present for **every** stage to keep the header fixed-stride; it is
//! `0` for all stages except [`PipelineStage::Shuffle`].
//!
//! # Example
//!
//! ```rust
//! use oxigdal_compress::codecs::{CodecPipeline, PipelineStage};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let pipeline = CodecPipeline::new()
//!     .push(PipelineStage::Shuffle { typesize: 4 })
//!     .push(PipelineStage::Zstd);
//!
//! let data = b"repeated payload ".repeat(256);
//! let compressed = pipeline.compress(&data)?;
//! let restored = CodecPipeline::decompress_self_describing(&compressed)?;
//! assert_eq!(restored, data);
//! # Ok(())
//! # }
//! ```

use crate::codecs::{
    BrotliCodec, DeflateCodec, DeltaCodec, DictionaryCodec, Lz4Codec, RleCodec, SnappyCodec,
    ZstdCodec,
};
use crate::error::CompressionError;

/// One stage of a codec pipeline. Lossless codecs only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    /// Byte-shuffle pre-filter grouping bytes by position within each
    /// `typesize`-byte element. Improves downstream entropy-coder ratios on
    /// fixed-width numeric arrays.
    Shuffle {
        /// Element width in bytes (e.g. `4` for `f32`, `8` for `f64`).
        typesize: u8,
    },
    /// First-order byte-wise delta encoding.
    Delta,
    /// Run-length encoding.
    Rle,
    /// LZ4 block compression.
    Lz4,
    /// Zstandard compression.
    Zstd,
    /// Snappy compression.
    Snappy,
    /// Brotli compression.
    Brotli,
    /// DEFLATE compression.
    Deflate,
    /// Dictionary encoding.
    Dictionary,
}

impl PipelineStage {
    /// One-byte stage id for the frame header.
    fn id(self) -> u8 {
        match self {
            PipelineStage::Shuffle { .. } => 1,
            PipelineStage::Delta => 2,
            PipelineStage::Rle => 3,
            PipelineStage::Lz4 => 4,
            PipelineStage::Zstd => 5,
            PipelineStage::Snappy => 6,
            PipelineStage::Brotli => 7,
            PipelineStage::Deflate => 8,
            PipelineStage::Dictionary => 9,
        }
    }

    /// The per-stage typesize byte written to the frame header. Non-zero only
    /// for [`PipelineStage::Shuffle`].
    fn typesize_byte(self) -> u8 {
        match self {
            PipelineStage::Shuffle { typesize } => typesize,
            _ => 0,
        }
    }

    /// Reconstruct a stage from its id byte. Shuffle carries an extra typesize
    /// byte; for every other stage the typesize byte is ignored.
    fn from_id(id: u8, typesize: u8) -> Result<PipelineStage, CompressionError> {
        Ok(match id {
            1 => PipelineStage::Shuffle { typesize },
            2 => PipelineStage::Delta,
            3 => PipelineStage::Rle,
            4 => PipelineStage::Lz4,
            5 => PipelineStage::Zstd,
            6 => PipelineStage::Snappy,
            7 => PipelineStage::Brotli,
            8 => PipelineStage::Deflate,
            9 => PipelineStage::Dictionary,
            other => {
                return Err(CompressionError::InvalidMetadata(format!(
                    "unknown pipeline stage id {other} in frame header"
                )));
            }
        })
    }
}

/// Frame-header magic bytes — `OXPL` (OXigdal PipeLine).
const PIPELINE_MAGIC: [u8; 4] = [b'O', b'X', b'P', b'L'];

/// Frame-header format version.
const PIPELINE_VERSION: u8 = 1;

/// Size of the fixed prefix: magic(4) + version(1) + num_stages(1).
const HEADER_PREFIX_LEN: usize = 6;

/// Bytes per stage descriptor in the header: id(1) + typesize(1).
const STAGE_DESCRIPTOR_LEN: usize = 2;

/// A composite lossless codec built by chaining individual codec stages.
///
/// Stages are applied left-to-right (in `push` order) on compression and
/// right-to-left on decompression. The compressed payload is prefixed with a
/// self-describing frame header.
#[derive(Debug, Clone, Default)]
pub struct CodecPipeline {
    stages: Vec<PipelineStage>,
}

impl CodecPipeline {
    /// Create an empty pipeline.
    ///
    /// An empty pipeline emits a header-only (6-byte) frame on `compress` and
    /// returns the payload unchanged on `decompress`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a stage, returning the pipeline for chaining.
    pub fn push(mut self, stage: PipelineStage) -> Self {
        self.stages.push(stage);
        self
    }

    /// The ordered list of stages in this pipeline.
    pub fn stages(&self) -> &[PipelineStage] {
        &self.stages
    }

    /// Compress `input` through every stage in order, prefixed with a
    /// self-describing frame header.
    ///
    /// # Errors
    ///
    /// Returns a [`CompressionError`] if any stage's codec fails.
    pub fn compress(&self, input: &[u8]) -> Result<Vec<u8>, CompressionError> {
        let mut output = encode_header(&self.stages);

        let mut buffer = input.to_vec();
        for &stage in &self.stages {
            buffer = apply_stage_forward(stage, &buffer)?;
        }

        output.extend_from_slice(&buffer);
        Ok(output)
    }

    /// Decompress a frame produced by [`CodecPipeline::compress`].
    ///
    /// The stage list is read **from the frame header**, not from `self`; the
    /// `self` stage list is used only for API symmetry. Stages are walked in
    /// reverse.
    ///
    /// # Errors
    ///
    /// Returns a [`CompressionError`] if the header is malformed, truncated,
    /// has a bad magic/version, carries an unknown stage id, or if any stage's
    /// codec fails.
    pub fn decompress(&self, input: &[u8]) -> Result<Vec<u8>, CompressionError> {
        let (stages, payload) = decode_header(input)?;
        decompress_payload(&stages, payload)
    }

    /// Reconstruct the pipeline purely from the frame header, then decompress.
    ///
    /// This is the entry point for a caller that holds only the compressed
    /// bytes and does not know which stages produced them.
    ///
    /// # Errors
    ///
    /// Returns a [`CompressionError`] if the header is malformed, truncated,
    /// has a bad magic/version, carries an unknown stage id, or if any stage's
    /// codec fails.
    pub fn decompress_self_describing(input: &[u8]) -> Result<Vec<u8>, CompressionError> {
        let (stages, payload) = decode_header(input)?;
        decompress_payload(&stages, payload)
    }
}

/// Encode the frame header for `stages`.
fn encode_header(stages: &[PipelineStage]) -> Vec<u8> {
    let num_stages = stages.len();
    let mut header = Vec::with_capacity(HEADER_PREFIX_LEN + STAGE_DESCRIPTOR_LEN * num_stages);
    header.extend_from_slice(&PIPELINE_MAGIC);
    header.push(PIPELINE_VERSION);
    // `num_stages` is capped at 255 by the single-byte field; pipelines never
    // approach this in practice.
    header.push(num_stages as u8);
    for &stage in stages {
        header.push(stage.id());
        header.push(stage.typesize_byte());
    }
    header
}

/// Parse and validate a frame header, returning the decoded stage list and a
/// slice over the remaining compressed payload.
fn decode_header(input: &[u8]) -> Result<(Vec<PipelineStage>, &[u8]), CompressionError> {
    if input.len() < HEADER_PREFIX_LEN {
        return Err(CompressionError::InvalidMetadata(format!(
            "pipeline frame truncated: need at least {HEADER_PREFIX_LEN} header bytes, got {}",
            input.len()
        )));
    }

    if input[0..4] != PIPELINE_MAGIC {
        return Err(CompressionError::InvalidMetadata(
            "pipeline frame has bad magic (expected 'OXPL')".to_string(),
        ));
    }

    let version = input[4];
    if version != PIPELINE_VERSION {
        return Err(CompressionError::InvalidMetadata(format!(
            "unsupported pipeline frame version {version} (expected {PIPELINE_VERSION})"
        )));
    }

    let num_stages = input[5] as usize;
    let header_len = HEADER_PREFIX_LEN + STAGE_DESCRIPTOR_LEN * num_stages;
    if input.len() < header_len {
        return Err(CompressionError::InvalidMetadata(format!(
            "pipeline frame truncated: header declares {num_stages} stage(s) \
             needing {header_len} bytes, got {}",
            input.len()
        )));
    }

    let mut stages = Vec::with_capacity(num_stages);
    for stage_index in 0..num_stages {
        let offset = HEADER_PREFIX_LEN + STAGE_DESCRIPTOR_LEN * stage_index;
        let id = input[offset];
        let typesize = input[offset + 1];
        stages.push(PipelineStage::from_id(id, typesize)?);
    }

    Ok((stages, &input[header_len..]))
}

/// Walk `stages` in reverse over `payload`, applying each stage's inverse.
fn decompress_payload(
    stages: &[PipelineStage],
    payload: &[u8],
) -> Result<Vec<u8>, CompressionError> {
    let mut buffer = payload.to_vec();
    for &stage in stages.iter().rev() {
        buffer = apply_stage_inverse(stage, &buffer)?;
    }
    Ok(buffer)
}

/// Group `data` bytes by their position within each `typesize`-byte element.
///
/// For `typesize <= 1` the data is returned unchanged. When
/// `data.len() % typesize != 0`, the trailing partial element's bytes are
/// appended verbatim after the shuffled body so the transform stays reversible.
fn byte_shuffle(data: &[u8], typesize: usize) -> Vec<u8> {
    if typesize <= 1 || data.len() < typesize {
        return data.to_vec();
    }
    let element_count = data.len() / typesize;
    let shuffled_len = element_count * typesize;
    let mut output = Vec::with_capacity(data.len());
    for byte_position in 0..typesize {
        for element_index in 0..element_count {
            output.push(data[element_index * typesize + byte_position]);
        }
    }
    // Tail bytes (incomplete trailing element) pass through unchanged.
    output.extend_from_slice(&data[shuffled_len..]);
    output
}

/// Inverse of [`byte_shuffle`] — regroup planar bytes back into interleaved
/// `typesize`-byte elements. The trailing partial element is restored verbatim.
fn byte_unshuffle(data: &[u8], typesize: usize) -> Vec<u8> {
    if typesize <= 1 || data.len() < typesize {
        return data.to_vec();
    }
    let element_count = data.len() / typesize;
    let shuffled_len = element_count * typesize;
    let mut output = vec![0u8; data.len()];
    for byte_position in 0..typesize {
        for element_index in 0..element_count {
            output[element_index * typesize + byte_position] =
                data[byte_position * element_count + element_index];
        }
    }
    // Restore tail bytes (incomplete trailing element).
    output[shuffled_len..].copy_from_slice(&data[shuffled_len..]);
    output
}

/// Apply a single stage in the forward (compress) direction.
fn apply_stage_forward(stage: PipelineStage, data: &[u8]) -> Result<Vec<u8>, CompressionError> {
    match stage {
        PipelineStage::Shuffle { typesize } => Ok(byte_shuffle(data, typesize as usize)),
        PipelineStage::Delta => DeltaCodec::default().compress(data),
        PipelineStage::Rle => RleCodec::default().compress(data),
        PipelineStage::Lz4 => Lz4Codec::default().compress(data),
        PipelineStage::Zstd => ZstdCodec::default().compress(data),
        PipelineStage::Snappy => SnappyCodec::default().compress(data),
        PipelineStage::Brotli => BrotliCodec::default().compress(data),
        PipelineStage::Deflate => DeflateCodec::default().compress(data),
        PipelineStage::Dictionary => DictionaryCodec::default().compress(data),
    }
}

/// Apply a single stage in the inverse (decompress) direction.
fn apply_stage_inverse(stage: PipelineStage, data: &[u8]) -> Result<Vec<u8>, CompressionError> {
    match stage {
        PipelineStage::Shuffle { typesize } => Ok(byte_unshuffle(data, typesize as usize)),
        PipelineStage::Delta => DeltaCodec::default().decompress(data),
        PipelineStage::Rle => RleCodec::default().decompress(data),
        // Zstd frames embed their own content size, so `None` is a genuine
        // self-describing decode. Lz4 raw blocks carry no such field — see
        // `decompress_lz4_stage` for how that case is handled.
        PipelineStage::Lz4 => decompress_lz4_stage(data),
        PipelineStage::Zstd => ZstdCodec::default().decompress(data, None),
        PipelineStage::Snappy => SnappyCodec::default().decompress(data),
        PipelineStage::Brotli => BrotliCodec::default().decompress(data),
        PipelineStage::Deflate => DeflateCodec::default().decompress(data),
        PipelineStage::Dictionary => DictionaryCodec::default().decompress(data),
    }
}

/// Output-size multipliers tried in order when decompressing a
/// [`PipelineStage::Lz4`] payload with no known target size.
///
/// Raw LZ4 blocks (unlike Zstd frames) carry no embedded decompressed-length
/// field, and the pipeline's frame header does not record per-stage
/// intermediate sizes. `Lz4Codec::decompress`'s own `None`-hint fallback
/// guesses `input.len() * 4`, which undershoots whenever a block compresses
/// better than 4:1 — exactly the common case for a `Shuffle`-then-`Lz4`
/// pipeline over smooth numeric data (e.g. a near-linear-ramp `f32` array).
/// `oxiarc_lz4::decompress_block`'s `max_output` is a safe upper bound, not a
/// required exact size, so retrying with a growing bound converges on the
/// true size without any wire-format change.
const LZ4_SIZE_GUESS_MULTIPLIERS: &[usize] = &[4, 16, 64, 256, 1024, 4096, 16384];

/// Decompress a [`PipelineStage::Lz4`] payload, growing the output-size bound
/// on each retry until decoding succeeds. See [`LZ4_SIZE_GUESS_MULTIPLIERS`].
fn decompress_lz4_stage(data: &[u8]) -> Result<Vec<u8>, CompressionError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let codec = Lz4Codec::default();
    let mut last_err = None;
    for &multiplier in LZ4_SIZE_GUESS_MULTIPLIERS {
        let guess = data.len().saturating_mul(multiplier);
        match codec.decompress(data, Some(guess)) {
            Ok(output) => return Ok(output),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        CompressionError::InvalidMetadata(
            "lz4 pipeline stage decompression failed for an empty guess schedule".to_string(),
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_id_round_trips_through_from_id() {
        let cases = [
            PipelineStage::Shuffle { typesize: 4 },
            PipelineStage::Delta,
            PipelineStage::Rle,
            PipelineStage::Lz4,
            PipelineStage::Zstd,
            PipelineStage::Snappy,
            PipelineStage::Brotli,
            PipelineStage::Deflate,
            PipelineStage::Dictionary,
        ];
        for stage in cases {
            let restored = PipelineStage::from_id(stage.id(), stage.typesize_byte())
                .expect("known id must decode");
            assert_eq!(stage, restored);
        }
    }

    #[test]
    fn from_id_rejects_unknown_id() {
        assert!(PipelineStage::from_id(0, 0).is_err());
        assert!(PipelineStage::from_id(10, 0).is_err());
        assert!(PipelineStage::from_id(255, 0).is_err());
    }

    #[test]
    fn byte_shuffle_round_trips_with_tail() {
        // 13 bytes, typesize 4 -> 3 full elements + 1 tail byte.
        let data: Vec<u8> = (0..13u8).collect();
        let shuffled = byte_shuffle(&data, 4);
        assert_eq!(shuffled.len(), data.len());
        let restored = byte_unshuffle(&shuffled, 4);
        assert_eq!(restored, data);
    }

    #[test]
    fn byte_shuffle_identity_for_typesize_one() {
        let data: Vec<u8> = (0..32u8).collect();
        assert_eq!(byte_shuffle(&data, 1), data);
        assert_eq!(byte_unshuffle(&data, 1), data);
    }
}
