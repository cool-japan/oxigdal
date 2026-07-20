//! CRC-32C (Castagnoli) checksum codec for the Zarr v3 `crc32c` codec.
//!
//! Per ZEP-0002 `crc32c` is a bytes-to-bytes checksum codec: on encode, a
//! trailing 4-byte little-endian CRC-32C checksum is appended to the
//! payload; on decode, the trailing 4 bytes are split off, the checksum is
//! recomputed over the remaining payload, and a
//! [`CodecError::ChecksumMismatch`]
//! error is returned if they disagree. Corrupted or tampered chunk/shard
//! data is therefore rejected instead of being silently passed through.
//!
//! This implements the Castagnoli polynomial (`0x1EDC6F41`, reflected
//! `0x82F6_3B78`) as used by iSCSI/ext4/SCTP -- **not** the CRC-32 (IEEE)
//! polynomial used by [`crate::transformers::Crc32Transformer`], which is a
//! different algorithm despite the similar name.
//!
//! Pure Rust, table-driven, no external `crc`/`crc32c`/C-FFI dependency
//! (COOLJAPAN Pure Rust Policy).

use crate::codecs::Codec;
use crate::error::{CodecError, Result, ZarrError};

/// Reflected CRC-32C (Castagnoli) polynomial.
const POLY: u32 = 0x82F6_3B78;

/// Builds the 256-entry CRC-32C lookup table at compile time.
const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ POLY;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

static TABLE: [u32; 256] = build_table();

/// Computes the CRC-32C (Castagnoli) checksum of `data`.
///
/// Reference check value: `checksum(b"123456789") == 0xE306_9283`.
#[must_use]
pub fn checksum(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        let idx = ((crc ^ u32::from(byte)) & 0xFF) as usize;
        crc = (crc >> 8) ^ TABLE[idx];
    }
    !crc
}

/// The Zarr v3 `crc32c` checksum codec.
///
/// Unlike compression codecs, this codec grows the payload by exactly 4
/// bytes on encode and shrinks it by 4 bytes on decode (after verifying the
/// checksum); it never changes the payload content itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct Crc32cCodec;

impl Crc32cCodec {
    /// Creates a new CRC-32C codec.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Codec for Crc32cCodec {
    fn id(&self) -> &str {
        "crc32c"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        let crc = checksum(data);
        let mut out = Vec::with_capacity(data.len() + 4);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc.to_le_bytes());
        Ok(out)
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 4 {
            return Err(ZarrError::Codec(CodecError::DecompressionFailed {
                message: format!(
                    "crc32c codec: payload too small to contain a checksum \
                     ({} bytes, need at least 4)",
                    data.len()
                ),
            }));
        }

        let split = data.len() - 4;
        let (payload, checksum_bytes) = data.split_at(split);
        let expected = u32::from_le_bytes([
            checksum_bytes[0],
            checksum_bytes[1],
            checksum_bytes[2],
            checksum_bytes[3],
        ]);
        let actual = checksum(payload);

        if actual != expected {
            return Err(ZarrError::Codec(CodecError::ChecksumMismatch {
                expected,
                actual,
            }));
        }

        Ok(payload.to_vec())
    }

    fn max_encoded_size(&self, input_size: usize) -> usize {
        input_size + 4
    }

    fn clone_box(&self) -> Box<dyn Codec> {
        Box::new(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32c_check_value() {
        // Standard CRC-32C (Castagnoli) check value for the ASCII string
        // "123456789", as specified by the Rocksoft CRC catalogue.
        assert_eq!(checksum(b"123456789"), 0xE306_9283);
    }

    #[test]
    fn test_crc32c_codec_roundtrip() {
        let codec = Crc32cCodec::new();
        let data = b"Hello, Zarr v3 crc32c!";

        let encoded = codec.encode(data).expect("encode");
        assert_eq!(encoded.len(), data.len() + 4);

        let decoded = codec.decode(&encoded).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_crc32c_codec_detects_corruption() {
        let codec = Crc32cCodec::new();
        let data = b"data that must not be silently corrupted";

        let mut encoded = codec.encode(data).expect("encode");
        // Flip a bit in the payload (not the trailing checksum bytes).
        encoded[0] ^= 0xFF;

        let result = codec.decode(&encoded);
        assert!(
            matches!(
                result,
                Err(ZarrError::Codec(CodecError::ChecksumMismatch { .. }))
            ),
            "corrupted crc32c payload must be rejected, got {result:?}"
        );
    }

    #[test]
    fn test_crc32c_codec_rejects_truncated_payload() {
        let codec = Crc32cCodec::new();
        assert!(codec.decode(&[1, 2, 3]).is_err());
        assert!(codec.decode(&[]).is_err());
    }

    #[test]
    fn test_crc32c_codec_empty_payload_roundtrip() {
        let codec = Crc32cCodec::new();
        let encoded = codec.encode(&[]).expect("encode empty");
        assert_eq!(encoded.len(), 4);
        let decoded = codec.decode(&encoded).expect("decode empty");
        assert!(decoded.is_empty());
    }
}
