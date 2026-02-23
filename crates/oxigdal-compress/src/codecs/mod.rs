//! Compression codecs
//!
//! This module provides various compression codecs optimized for different
//! types of geospatial data.

pub mod brotli;
pub mod deflate;
pub mod delta;
pub mod dictionary;
pub mod lz4;
pub mod rle;
pub mod snappy;
pub mod zstd;

pub use self::{
    brotli::{BrotliCodec, BrotliConfig, BrotliQuality},
    deflate::{DeflateCodec, DeflateConfig, DeflateFormat, DeflateLevel},
    delta::{DeltaCodec, DeltaConfig, DeltaDataType},
    dictionary::{DictionaryCodec, DictionaryConfig},
    lz4::{Lz4Codec, Lz4Config, Lz4Level},
    rle::{RleCodec, RleConfig},
    snappy::{SnappyCodec, SnappyConfig},
    zstd::{ZstdCodec, ZstdConfig, ZstdLevel},
};

/// Compression codec type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CodecType {
    /// LZ4 compression
    Lz4,
    /// Zstandard compression
    Zstd,
    /// Brotli compression
    Brotli,
    /// Snappy compression
    Snappy,
    /// DEFLATE compression
    Deflate,
    /// Delta encoding
    Delta,
    /// Run-length encoding
    Rle,
    /// Dictionary encoding
    Dictionary,
}

impl CodecType {
    /// Get codec name
    pub fn name(&self) -> &'static str {
        match self {
            CodecType::Lz4 => "lz4",
            CodecType::Zstd => "zstd",
            CodecType::Brotli => "brotli",
            CodecType::Snappy => "snappy",
            CodecType::Deflate => "deflate",
            CodecType::Delta => "delta",
            CodecType::Rle => "rle",
            CodecType::Dictionary => "dictionary",
        }
    }

    /// Parse codec type from string
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "lz4" => Some(CodecType::Lz4),
            "zstd" | "zstandard" => Some(CodecType::Zstd),
            "brotli" | "br" => Some(CodecType::Brotli),
            "snappy" | "snap" => Some(CodecType::Snappy),
            "deflate" | "gzip" | "zlib" => Some(CodecType::Deflate),
            "delta" => Some(CodecType::Delta),
            "rle" => Some(CodecType::Rle),
            "dictionary" | "dict" => Some(CodecType::Dictionary),
            _ => None,
        }
    }

    /// Check if codec is lossless
    pub fn is_lossless(&self) -> bool {
        match self {
            CodecType::Lz4
            | CodecType::Zstd
            | CodecType::Brotli
            | CodecType::Snappy
            | CodecType::Deflate
            | CodecType::Delta
            | CodecType::Rle
            | CodecType::Dictionary => true,
        }
    }

    /// Get typical compression speed (1-10, higher = faster)
    pub fn speed_score(&self) -> u8 {
        match self {
            CodecType::Snappy => 10,
            CodecType::Lz4 => 9,
            CodecType::Delta => 8,
            CodecType::Rle => 8,
            CodecType::Dictionary => 7,
            CodecType::Deflate => 5,
            CodecType::Zstd => 6,
            CodecType::Brotli => 3,
        }
    }

    /// Get typical compression ratio score (1-10, higher = better compression)
    pub fn ratio_score(&self) -> u8 {
        match self {
            CodecType::Brotli => 9,
            CodecType::Zstd => 8,
            CodecType::Deflate => 7,
            CodecType::Lz4 => 5,
            CodecType::Dictionary => 7,
            CodecType::Delta => 6,
            CodecType::Rle => 8,
            CodecType::Snappy => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codec_type_from_name() {
        assert_eq!(CodecType::from_name("lz4"), Some(CodecType::Lz4));
        assert_eq!(CodecType::from_name("zstd"), Some(CodecType::Zstd));
        assert_eq!(CodecType::from_name("zstandard"), Some(CodecType::Zstd));
        assert_eq!(CodecType::from_name("brotli"), Some(CodecType::Brotli));
        assert_eq!(CodecType::from_name("snappy"), Some(CodecType::Snappy));
        assert_eq!(CodecType::from_name("deflate"), Some(CodecType::Deflate));
        assert_eq!(CodecType::from_name("invalid"), None);
    }

    #[test]
    fn test_codec_type_is_lossless() {
        assert!(CodecType::Lz4.is_lossless());
        assert!(CodecType::Zstd.is_lossless());
        assert!(CodecType::Delta.is_lossless());
    }

    #[test]
    fn test_codec_scores() {
        assert!(CodecType::Snappy.speed_score() > CodecType::Brotli.speed_score());
        assert!(CodecType::Brotli.ratio_score() > CodecType::Snappy.ratio_score());
    }
}
