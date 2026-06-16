//! Tests for `build_codec_from_metadata` dispatch in `sharding.rs`.
//!
//! The tests exercise the codec dispatch through the public surface:
//!   - `oxigdal_zarr::sharding::parse_sharding_config`  (builds CodecChain from CodecMetadata)
//!   - `oxigdal_zarr::codecs::{Codec, NullCodec, CodecChain}`
//!   - `oxigdal_zarr::metadata::v3::*`  (CodecMetadata variants and ShardingConfig)
//!
//! Because `build_codec_from_metadata` itself is private, every assertion is driven
//! through `parse_sharding_config`, which calls it for every entry in `config.codecs`
//! and `config.index_codecs`.
#![allow(clippy::expect_used)]

use oxigdal_zarr::codecs::{Codec, CodecChain, NullCodec};
#[cfg(not(feature = "blosc"))]
use oxigdal_zarr::metadata::v3::BloscConfig;
use oxigdal_zarr::metadata::v3::{
    BytesConfig, CodecMetadata, EndianConfig, GzipConfig, ShardingConfig, TransposeConfig,
    ZstdConfig,
};
use oxigdal_zarr::sharding::parse_sharding_config;

// ── convenience helpers ──────────────────────────────────────────────────────

/// Minimal ShardingConfig that uses only null-equivalent codecs so `parse_sharding_config`
/// can always build the chains regardless of which optional features are enabled.
fn minimal_sharding_config(chunk_codecs: Vec<CodecMetadata>) -> ShardingConfig {
    ShardingConfig {
        chunk_shape: vec![4, 4],
        codecs: chunk_codecs,
        index_codecs: vec![CodecMetadata::Bytes {
            configuration: Some(BytesConfig {
                endian: Some("little".to_string()),
            }),
        }],
        index_location: Some("end".to_string()),
    }
}

/// Round-trip a byte buffer through a CodecChain (encode then decode).
fn roundtrip(chain: &CodecChain, data: &[u8]) -> Vec<u8> {
    let encoded = chain.encode(data.to_vec()).expect("encode");
    chain.decode(encoded).expect("decode")
}

// ── 1. NullCodec identity ────────────────────────────────────────────────────

/// NullCodec must return data unchanged on both encode and decode.
#[test]
fn test_null_codec_roundtrip() {
    let codec = NullCodec;
    let data = b"hello zarr sharding";
    let encoded = codec.encode(data).expect("encode");
    assert_eq!(
        encoded.as_slice(),
        data,
        "NullCodec.encode must be identity"
    );
    let decoded = codec.decode(&encoded).expect("decode");
    assert_eq!(
        decoded.as_slice(),
        data,
        "NullCodec.decode must be identity"
    );
}

// ── 2. Unknown / Generic codec → error ──────────────────────────────────────

/// A `CodecMetadata::Generic` variant (produced by serde when encountering an unknown tag)
/// must cause `parse_sharding_config` to return an error — proving the "always NullCodec"
/// behaviour has been removed.
#[test]
fn test_unknown_codec_returns_unsupported_error() {
    let config = minimal_sharding_config(vec![CodecMetadata::Generic]);
    let result = parse_sharding_config(&config);
    assert!(
        result.is_err(),
        "Generic/unknown codec must produce an error, got Ok"
    );
    match result {
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("unknown")
                    || err_msg.contains("Unknown")
                    || err_msg.contains("Codec"),
                "error message should mention codec or unknown; got: {err_msg}"
            );
        }
        Ok(_) => {
            // already asserted is_err() above; this branch is unreachable
        }
    }
}

// ── 3. Gzip round-trip (feature-gated) ──────────────────────────────────────

/// When the `gzip` feature is compiled in, a GzipCodec must compress and then
/// decompress a payload correctly.
#[test]
#[cfg(feature = "gzip")]
fn test_gzip_codec_roundtrip() {
    let config = minimal_sharding_config(vec![CodecMetadata::Gzip {
        configuration: Some(GzipConfig { level: Some(6) }),
    }]);
    let (chunk_chain, _) = parse_sharding_config(&config).expect("parse_sharding_config");
    let data: Vec<u8> = (0u8..128).collect();
    assert_eq!(
        roundtrip(&chunk_chain, &data),
        data,
        "gzip round-trip must be identity"
    );
}

/// Without the `gzip` feature, attempting to build a GzipCodec should fail.
#[test]
#[cfg(not(feature = "gzip"))]
fn test_gzip_unavailable_without_feature() {
    let config = minimal_sharding_config(vec![CodecMetadata::Gzip {
        configuration: Some(GzipConfig { level: Some(6) }),
    }]);
    assert!(
        parse_sharding_config(&config).is_err(),
        "gzip must not be available when feature is off"
    );
}

// ── 4. Zstd round-trip (feature-gated) ──────────────────────────────────────

#[test]
#[cfg(feature = "zstd")]
fn test_zstd_codec_roundtrip() {
    let config = minimal_sharding_config(vec![CodecMetadata::Zstd {
        configuration: Some(ZstdConfig {
            level: Some(3),
            checksum: None,
        }),
    }]);
    let (chunk_chain, _) = parse_sharding_config(&config).expect("parse_sharding_config");
    let data: Vec<u8> = (0u8..200).collect();
    assert_eq!(
        roundtrip(&chunk_chain, &data),
        data,
        "zstd round-trip must be identity"
    );
}

#[test]
#[cfg(not(feature = "zstd"))]
fn test_zstd_unavailable_without_feature() {
    let config = minimal_sharding_config(vec![CodecMetadata::Zstd {
        configuration: Some(ZstdConfig {
            level: Some(3),
            checksum: None,
        }),
    }]);
    assert!(
        parse_sharding_config(&config).is_err(),
        "zstd must not be available when feature is off"
    );
}

// ── 5. Codec chain via ShardingConfig (null-compatible) ──────────────────────

/// A ShardingConfig with Bytes + Gzip (when gzip is enabled) must parse without error,
/// and the resulting chunk chain must have codec(s) in it.
#[test]
fn test_codec_chain_via_sharding_config() {
    let chunk_codecs = vec![CodecMetadata::Bytes {
        configuration: Some(BytesConfig {
            endian: Some("little".to_string()),
        }),
    }];
    let config = minimal_sharding_config(chunk_codecs);
    let result = parse_sharding_config(&config);
    assert!(
        result.is_ok(),
        "parse_sharding_config with Bytes codec must succeed; err={:?}",
        result.err()
    );
    let (chunk_chain, index_chain) = result.expect("parse returns Ok");
    // chains must survive an encode-decode cycle
    let data = b"test data payload".to_vec();
    let out = chunk_chain.encode(data.clone()).expect("chunk encode");
    let back = chunk_chain.decode(out).expect("chunk decode");
    assert_eq!(back, data);
    let iout = index_chain.encode(data.clone()).expect("index encode");
    let iback = index_chain.decode(iout).expect("index decode");
    assert_eq!(iback, data);
}

// ── 6. Malformed / extreme configs do not panic ──────────────────────────────

/// A GzipConfig with `level = None` must use the default and not panic.
#[test]
#[cfg(feature = "gzip")]
fn test_gzip_no_level_defaults_gracefully() {
    let config = minimal_sharding_config(vec![CodecMetadata::Gzip {
        configuration: Some(GzipConfig { level: None }),
    }]);
    let result = parse_sharding_config(&config);
    assert!(
        result.is_ok(),
        "gzip with no level should default; err={:?}",
        result.err()
    );
}

/// A ZstdConfig with `level = None` must use the default and not panic.
#[test]
#[cfg(feature = "zstd")]
fn test_zstd_no_level_defaults_gracefully() {
    let config = minimal_sharding_config(vec![CodecMetadata::Zstd {
        configuration: Some(ZstdConfig {
            level: None,
            checksum: None,
        }),
    }]);
    let result = parse_sharding_config(&config);
    assert!(
        result.is_ok(),
        "zstd with no level should default; err={:?}",
        result.err()
    );
}

/// A Gzip codec with `configuration: None` must still build using defaults.
#[test]
#[cfg(feature = "gzip")]
fn test_gzip_none_configuration_uses_defaults() {
    let config = minimal_sharding_config(vec![CodecMetadata::Gzip {
        configuration: None,
    }]);
    let result = parse_sharding_config(&config);
    assert!(
        result.is_ok(),
        "gzip with None config should default; err={:?}",
        result.err()
    );
    let (chain, _) = result.expect("gzip none config parse");
    let data: Vec<u8> = (0u8..64).collect();
    assert_eq!(roundtrip(&chain, &data), data);
}

// ── 7. Bytes codec (endian variant) resolves successfully ────────────────────

#[test]
fn test_bytes_codec_little_endian_resolves() {
    let config = minimal_sharding_config(vec![CodecMetadata::Bytes {
        configuration: Some(BytesConfig {
            endian: Some("little".to_string()),
        }),
    }]);
    let result = parse_sharding_config(&config);
    assert!(
        result.is_ok(),
        "Bytes (little) should succeed; err={:?}",
        result.err()
    );
}

#[test]
fn test_bytes_codec_big_endian_resolves() {
    let config = minimal_sharding_config(vec![CodecMetadata::Bytes {
        configuration: Some(BytesConfig {
            endian: Some("big".to_string()),
        }),
    }]);
    let result = parse_sharding_config(&config);
    assert!(result.is_ok(), "Bytes (big) should succeed");
}

#[test]
fn test_bytes_codec_no_config_resolves() {
    let config = minimal_sharding_config(vec![CodecMetadata::Bytes {
        configuration: None,
    }]);
    let result = parse_sharding_config(&config);
    assert!(result.is_ok(), "Bytes (None config) should succeed");
}

// ── 8. Crc32c falls back to NullCodec with a warning ─────────────────────────

/// `crc32c` is a checksum codec not yet fully implemented; the dispatch must return
/// a passthrough (NullCodec-equivalent) and must NOT return an error.
#[test]
fn test_crc32c_falls_back_to_null_with_warning() {
    let config = minimal_sharding_config(vec![CodecMetadata::Crc32c {
        configuration: None,
    }]);
    let result = parse_sharding_config(&config);
    assert!(
        result.is_ok(),
        "crc32c should fall back to NullCodec (passthrough); err={:?}",
        result.err()
    );
    let (chain, _) = result.expect("crc32c parse");
    let data = b"checksum test payload".to_vec();
    assert_eq!(chain.encode(data.clone()).expect("encode"), data);
}

// ── 9. Error message contains the codec name ─────────────────────────────────

/// When `Generic` is encountered, the error message must reference the codec problem.
#[test]
fn test_unsupported_codec_error_has_codec_context() {
    let config = minimal_sharding_config(vec![CodecMetadata::Generic]);
    let result = parse_sharding_config(&config);
    assert!(result.is_err(), "Generic must produce an error, got Ok");
    match result {
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            // The error must mention something about codec / unknown in a meaningful way
            assert!(
                msg.contains("codec") || msg.contains("unknown"),
                "error must mention codec or unknown; got: {msg}"
            );
        }
        Ok(_) => {
            // already asserted is_err() above; unreachable
        }
    }
}

// ── 10. Previous "always NullCodec" behaviour is gone ────────────────────────

/// The previous stub always returned `Ok(NullCodec)` for every input, including
/// the `Generic` variant.  After the fix, `Generic` must fail.  This test is the
/// direct regression guard for the old behaviour.
#[test]
fn test_build_codec_metadata_wires_to_real_codec_not_null() {
    // Generic represents an unknown/unsupported codec; the old code returned
    // NullCodec for it, which silently ignored encoding mismatches.
    let config = minimal_sharding_config(vec![CodecMetadata::Generic]);
    let result = parse_sharding_config(&config);
    assert!(
        result.is_err(),
        "The old stub returned Ok(NullCodec) for Generic; the new dispatch must return Err"
    );
}

// ── 11. Transpose codec treats as passthrough ───────────────────────────────

#[test]
fn test_transpose_codec_is_passthrough() {
    let config = minimal_sharding_config(vec![CodecMetadata::Transpose {
        configuration: TransposeConfig { order: vec![1, 0] },
    }]);
    let result = parse_sharding_config(&config);
    assert!(
        result.is_ok(),
        "Transpose should resolve to NullCodec passthrough; err={:?}",
        result.err()
    );
    let (chain, _) = result.expect("transpose parse");
    let data = b"transpose test".to_vec();
    assert_eq!(chain.encode(data.clone()).expect("encode"), data);
}

// ── 12. Endian codec treats as passthrough ──────────────────────────────────

#[test]
fn test_endian_codec_is_passthrough() {
    let config = minimal_sharding_config(vec![CodecMetadata::Endian {
        configuration: EndianConfig {
            endian: "little".to_string(),
        },
    }]);
    let result = parse_sharding_config(&config);
    assert!(
        result.is_ok(),
        "Endian should resolve to NullCodec passthrough; err={:?}",
        result.err()
    );
    let (chain, _) = result.expect("endian parse");
    let data = b"endian test".to_vec();
    assert_eq!(chain.encode(data.clone()).expect("encode"), data);
}

// ── 13. Mixed chain: Bytes + compression (feature-gated) ────────────────────

/// A two-codec chain [Bytes, Gzip] must compress/decompress correctly when gzip
/// is available.
#[test]
#[cfg(feature = "gzip")]
fn test_mixed_bytes_gzip_chain_roundtrip() {
    let config = minimal_sharding_config(vec![
        CodecMetadata::Bytes {
            configuration: Some(BytesConfig {
                endian: Some("little".to_string()),
            }),
        },
        CodecMetadata::Gzip {
            configuration: Some(GzipConfig { level: Some(4) }),
        },
    ]);
    let (chain, _) = parse_sharding_config(&config).expect("parse");
    let data: Vec<u8> = (0u8..=255).cycle().take(512).collect();
    assert_eq!(roundtrip(&chain, &data), data);
}

// ── 14. Blosc unavailability without feature ─────────────────────────────────

/// When blosc feature is disabled, requesting a Blosc codec must fail.
#[test]
#[cfg(not(feature = "blosc"))]
fn test_blosc_unavailable_without_feature() {
    let config = minimal_sharding_config(vec![CodecMetadata::Blosc {
        configuration: BloscConfig {
            cname: "lz4".to_string(),
            clevel: 5,
            shuffle: 1,
            typesize: None,
            blocksize: None,
        },
    }]);
    assert!(
        parse_sharding_config(&config).is_err(),
        "blosc must fail when feature is disabled"
    );
}

// ── 15. Empty codec chain succeeds ──────────────────────────────────────────

/// An empty codec list is valid and the resulting chain is a no-op.
#[test]
fn test_empty_codec_list_is_noop() {
    let config = ShardingConfig {
        chunk_shape: vec![2, 2],
        codecs: vec![],
        index_codecs: vec![],
        index_location: None,
    };
    let result = parse_sharding_config(&config);
    assert!(result.is_ok(), "empty codec list must succeed");
    let (chunk_chain, _) = result.expect("empty list parse");
    let data = b"noop test".to_vec();
    assert_eq!(chunk_chain.encode(data.clone()).expect("encode"), data);
}
