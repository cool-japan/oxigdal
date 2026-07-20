//! Shared Zarr v3 `codecs` metadata dispatcher.
//!
//! The v3 reader (`reader::v3`), v3 writer (`writer::v3`), and the sharding
//! extension (`sharding`) all need to turn a [`CodecMetadata`] entry from a
//! `zarr.json` file into a concrete [`Codec`] implementation. This module is
//! the single source of truth for that mapping so the three call sites
//! cannot drift out of sync, and so that a codec this crate does not
//! actually implement (Blosc without the `blosc` feature, LZ4/zfp -- which
//! have no dedicated [`CodecMetadata`] variant and therefore decode as
//! [`CodecMetadata::Generic`], etc.) is rejected with a typed error instead
//! of silently falling back to an identity ("no-op") codec.
//!
//! A silent no-op codec is a data-integrity hazard: an array whose metadata
//! declares e.g. `blosc` compression but whose bytes are handled by an
//! identity codec would have its still-compressed bytes returned to callers
//! as if they were the decoded array -- corruption, not a decode failure.

use crate::codecs::crc32c::Crc32cCodec;
use crate::codecs::{Codec, NullCodec};
use crate::error::{CodecError, Result, ZarrError};
use crate::metadata::v3::CodecMetadata;

/// Builds a codec implementation from Zarr v3 codec metadata.
///
/// # Errors
/// Returns [`CodecError::CodecNotAvailable`] when the codec is recognised by
/// this crate but its cargo feature is not compiled in, and
/// [`CodecError::UnknownCodec`] for any codec metadata this crate does not
/// implement at all -- including every [`CodecMetadata::Generic`] entry
/// (unrecognised codec names, e.g. `lz4` or `zfp`, which have no dedicated
/// enum variant and so parse as `Generic`).
pub(crate) fn build_codec_from_metadata(metadata: &CodecMetadata) -> Result<Box<dyn Codec>> {
    match metadata {
        CodecMetadata::Gzip { configuration } => {
            #[cfg(feature = "gzip")]
            {
                use crate::codecs::gzip::GzipCodec;
                let level = configuration.as_ref().and_then(|c| c.level).unwrap_or(6);
                Ok(Box::new(GzipCodec::new(level)?))
            }
            #[cfg(not(feature = "gzip"))]
            {
                let _ = configuration;
                Err(ZarrError::Codec(CodecError::CodecNotAvailable {
                    codec: "gzip".to_string(),
                }))
            }
        }
        CodecMetadata::Zstd { configuration } => {
            #[cfg(feature = "zstd")]
            {
                use crate::codecs::zstd_codec::ZstdCodec;
                let level = configuration.as_ref().and_then(|c| c.level).unwrap_or(3);
                Ok(Box::new(ZstdCodec::new(level)?))
            }
            #[cfg(not(feature = "zstd"))]
            {
                let _ = configuration;
                Err(ZarrError::Codec(CodecError::CodecNotAvailable {
                    codec: "zstd".to_string(),
                }))
            }
        }
        CodecMetadata::Blosc { configuration } => {
            #[cfg(feature = "blosc")]
            {
                use crate::codecs::blosc::BloscCodec;
                Ok(Box::new(BloscCodec::new(
                    configuration.cname.clone(),
                    configuration.clevel,
                    configuration.shuffle,
                    configuration.blocksize,
                )?))
            }
            #[cfg(not(feature = "blosc"))]
            {
                let _ = configuration;
                Err(ZarrError::Codec(CodecError::CodecNotAvailable {
                    codec: "blosc".to_string(),
                }))
            }
        }
        // Transpose is an array-to-array codec that reorders elements based
        // on the array's shape/dtype, which is not available to a plain
        // bytes-to-bytes `Codec`. This crate's codec pipeline only ever sees
        // raw chunk bytes at this layer, so (matching the pre-existing
        // sharding.rs behaviour this dispatcher replaces) it is passed
        // through unchanged. Tracked as a follow-up: full array-aware
        // transpose support needs to move up into the reader/writer, which
        // do have shape/dtype context.
        CodecMetadata::Transpose { .. } => Ok(Box::new(NullCodec)),
        // Bytes and Endian codecs describe byte-order transformations that
        // are already applied when chunk data is serialised/deserialised
        // elsewhere in this crate; treated as identity here.
        CodecMetadata::Bytes { .. } | CodecMetadata::Endian { .. } => Ok(Box::new(NullCodec)),
        // crc32c (ZEP-0002 checksum codec, CRC-32C/Castagnoli): compute and
        // verify a real checksum instead of passing corrupted data through.
        CodecMetadata::Crc32c { .. } => Ok(Box::new(Crc32cCodec::new())),
        // sharding_indexed is handled by the dedicated ShardReader/
        // ShardWriter logic one layer up (see `crate::sharding`); the byte
        // pipeline treats it as identity here.
        CodecMetadata::ShardingIndexed { .. } => Ok(Box::new(NullCodec)),
        // Any codec name this crate does not recognise, including codecs
        // such as "lz4" or "zfp" that have no dedicated `CodecMetadata`
        // variant and therefore deserialize into `Generic`.
        CodecMetadata::Generic => Err(ZarrError::Codec(CodecError::UnknownCodec {
            codec: "unknown (Generic)".to_string(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_unknown_codec_errors() {
        let result = build_codec_from_metadata(&CodecMetadata::Generic);
        assert!(matches!(
            result,
            Err(ZarrError::Codec(CodecError::UnknownCodec { .. }))
        ));
    }

    #[test]
    fn test_dispatch_crc32c_returns_real_codec() {
        let codec = build_codec_from_metadata(&CodecMetadata::Crc32c {
            configuration: None,
        })
        .expect("crc32c codec should build");
        assert_eq!(codec.id(), "crc32c");

        // A real codec must grow the payload (append checksum) rather than
        // behave as an identity/no-op codec.
        let encoded = codec.encode(b"payload").expect("encode");
        assert_eq!(encoded.len(), b"payload".len() + 4);
    }

    #[cfg(not(feature = "blosc"))]
    #[test]
    fn test_dispatch_blosc_without_feature_errors() {
        use crate::metadata::v3::BloscConfig;

        let result = build_codec_from_metadata(&CodecMetadata::Blosc {
            configuration: BloscConfig {
                cname: "zstd".to_string(),
                clevel: 5,
                shuffle: 1,
                typesize: None,
                blocksize: None,
            },
        });
        assert!(matches!(
            result,
            Err(ZarrError::Codec(CodecError::CodecNotAvailable { .. }))
        ));
    }
}
