//! Compression codecs for Zarr arrays
//!
//! This module provides compression and decompression support for Zarr chunks,
//! including Blosc, Zstd, Gzip, and LZ4 codecs.

#[cfg(feature = "blosc")]
pub mod blosc;

pub mod blosc2_codec;

#[cfg(feature = "gzip")]
pub mod gzip;

#[cfg(feature = "zstd")]
pub mod zstd_codec;

#[cfg(feature = "lz4")]
pub mod lz4_codec;

pub mod zfp_codec;

pub mod registry;

use crate::error::{CodecError, Result, ZarrError};
use serde::{Deserialize, Serialize};

/// Trait for compression/decompression codecs
pub trait Codec: Send + Sync {
    /// Returns the codec identifier
    fn id(&self) -> &str;

    /// Compresses data
    ///
    /// # Errors
    /// Returns error if compression fails
    fn encode(&self, data: &[u8]) -> Result<Vec<u8>>;

    /// Decompresses data
    ///
    /// # Errors
    /// Returns error if decompression fails
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>>;

    /// Returns the maximum compressed size for given input size
    fn max_encoded_size(&self, input_size: usize) -> usize {
        // Conservative estimate: input size + 10% overhead
        input_size + (input_size / 10) + 1024
    }

    /// Clones the codec
    fn clone_box(&self) -> Box<dyn Codec>;
}

/// Compressor configuration for Zarr v2
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "id")]
pub enum CompressorConfig {
    /// No compression
    #[serde(rename = "null")]
    Null,

    /// Blosc compressor
    #[cfg(feature = "blosc")]
    #[serde(rename = "blosc")]
    Blosc {
        /// Compression algorithm (e.g., "lz4", "zstd")
        cname: String,
        /// Compression level (0-9)
        clevel: u8,
        /// Shuffle mode (0=no shuffle, 1=byte shuffle, 2=bit shuffle)
        shuffle: u8,
        /// Block size
        #[serde(skip_serializing_if = "Option::is_none")]
        blocksize: Option<usize>,
    },

    /// Zstd compressor
    #[cfg(feature = "zstd")]
    #[serde(rename = "zstd")]
    Zstd {
        /// Compression level (1-22)
        level: i32,
    },

    /// Gzip compressor
    #[cfg(feature = "gzip")]
    #[serde(rename = "gzip")]
    Gzip {
        /// Compression level (0-9)
        level: u32,
    },

    /// LZ4 compressor
    #[cfg(feature = "lz4")]
    #[serde(rename = "lz4")]
    Lz4 {
        /// Acceleration factor (>= 1)
        #[serde(skip_serializing_if = "Option::is_none")]
        acceleration: Option<i32>,
    },

    /// Generic compressor (for unsupported types)
    #[serde(other)]
    Unknown,
}

impl CompressorConfig {
    /// Returns the compressor ID
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Null => "null",
            #[cfg(feature = "blosc")]
            Self::Blosc { .. } => "blosc",
            #[cfg(feature = "zstd")]
            Self::Zstd { .. } => "zstd",
            #[cfg(feature = "gzip")]
            Self::Gzip { .. } => "gzip",
            #[cfg(feature = "lz4")]
            Self::Lz4 { .. } => "lz4",
            Self::Unknown => "unknown",
        }
    }

    /// Creates a codec from this configuration
    ///
    /// # Errors
    /// Returns error if the codec is not available
    pub fn build(&self) -> Result<Box<dyn Codec>> {
        match self {
            Self::Null => Ok(Box::new(NullCodec)),
            #[cfg(feature = "blosc")]
            Self::Blosc {
                cname,
                clevel,
                shuffle,
                blocksize,
            } => Ok(Box::new(blosc::BloscCodec::new(
                cname.clone(),
                *clevel,
                *shuffle,
                *blocksize,
            )?)),
            #[cfg(feature = "zstd")]
            Self::Zstd { level } => Ok(Box::new(zstd_codec::ZstdCodec::new(*level)?)),
            #[cfg(feature = "gzip")]
            Self::Gzip { level } => Ok(Box::new(gzip::GzipCodec::new(*level)?)),
            #[cfg(feature = "lz4")]
            Self::Lz4 { acceleration } => Ok(Box::new(lz4_codec::Lz4Codec::new(*acceleration)?)),
            _ => Err(ZarrError::Codec(CodecError::CodecNotAvailable {
                codec: self.id().to_string(),
            })),
        }
    }
}

/// Null codec (no compression)
#[derive(Debug, Clone)]
pub struct NullCodec;

impl Codec for NullCodec {
    fn id(&self) -> &str {
        "null"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        Ok(data.to_vec())
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        Ok(data.to_vec())
    }

    fn max_encoded_size(&self, input_size: usize) -> usize {
        input_size
    }

    fn clone_box(&self) -> Box<dyn Codec> {
        Box::new(self.clone())
    }
}

/// Codec configuration for Zarr v3
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodecV3 {
    /// Codec name
    pub name: String,
    /// Codec configuration
    #[serde(flatten)]
    pub configuration: serde_json::Map<String, serde_json::Value>,
}

impl CodecV3 {
    /// Creates a new codec configuration
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            configuration: serde_json::Map::new(),
        }
    }

    /// Adds a configuration parameter
    #[must_use]
    pub fn with_param(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.configuration.insert(key.into(), value);
        self
    }

    /// Builds a codec from this configuration
    ///
    /// # Errors
    /// Returns error if the codec is not available or configuration is invalid
    pub fn build(&self) -> Result<Box<dyn Codec>> {
        match self.name.as_str() {
            "null" | "none" => Ok(Box::new(NullCodec)),
            #[cfg(feature = "gzip")]
            "gzip" => {
                let level = self
                    .configuration
                    .get("level")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(6) as u32;
                Ok(Box::new(gzip::GzipCodec::new(level)?))
            }
            #[cfg(feature = "zstd")]
            "zstd" => {
                let level = self
                    .configuration
                    .get("level")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(3) as i32;
                Ok(Box::new(zstd_codec::ZstdCodec::new(level)?))
            }
            #[cfg(feature = "lz4")]
            "lz4" => {
                let acceleration = self
                    .configuration
                    .get("acceleration")
                    .and_then(|v| v.as_i64())
                    .map(|v| v as i32);
                Ok(Box::new(lz4_codec::Lz4Codec::new(acceleration)?))
            }
            _ => Err(ZarrError::Codec(CodecError::UnknownCodec {
                codec: self.name.clone(),
            })),
        }
    }
}

/// Codec chain - multiple codecs applied in sequence
pub struct CodecChain {
    codecs: Vec<Box<dyn Codec>>,
}

impl CodecChain {
    /// Creates a new codec chain
    #[must_use]
    pub fn new(codecs: Vec<Box<dyn Codec>>) -> Self {
        Self { codecs }
    }

    /// Creates an empty codec chain
    #[must_use]
    pub fn empty() -> Self {
        Self { codecs: Vec::new() }
    }

    /// Adds a codec to the chain
    pub fn add(&mut self, codec: Box<dyn Codec>) {
        self.codecs.push(codec);
    }

    /// Returns the number of codecs in the chain
    #[must_use]
    pub fn len(&self) -> usize {
        self.codecs.len()
    }

    /// Returns true if the chain is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.codecs.is_empty()
    }

    /// Encodes data through the codec chain
    ///
    /// # Errors
    /// Returns error if any codec fails
    pub fn encode(&self, mut data: Vec<u8>) -> Result<Vec<u8>> {
        for codec in &self.codecs {
            data = codec.encode(&data)?;
        }
        Ok(data)
    }

    /// Decodes data through the codec chain (in reverse order)
    ///
    /// # Errors
    /// Returns error if any codec fails
    pub fn decode(&self, mut data: Vec<u8>) -> Result<Vec<u8>> {
        for codec in self.codecs.iter().rev() {
            data = codec.decode(&data)?;
        }
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null_codec() {
        let codec = NullCodec;
        let data = b"Hello, Zarr!";

        let encoded = codec.encode(data).expect("encode");
        assert_eq!(encoded, data);

        let decoded = codec.decode(&encoded).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_compressor_config_null() {
        let config = CompressorConfig::Null;
        assert_eq!(config.id(), "null");

        let codec = config.build().expect("build codec");
        assert_eq!(codec.id(), "null");
    }

    #[test]
    fn test_codec_v3() {
        let config = CodecV3::new("null");
        assert_eq!(config.name, "null");

        let codec = config.build().expect("build codec");
        assert_eq!(codec.id(), "null");
    }

    #[test]
    fn test_codec_chain_empty() {
        let chain = CodecChain::empty();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);

        let data = b"test data".to_vec();
        let encoded = chain.encode(data.clone()).expect("encode");
        assert_eq!(encoded, data);

        let decoded = chain.decode(encoded).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_codec_chain_single() {
        let mut chain = CodecChain::empty();
        chain.add(Box::new(NullCodec));

        assert_eq!(chain.len(), 1);

        let data = b"test data".to_vec();
        let encoded = chain.encode(data.clone()).expect("encode");
        let decoded = chain.decode(encoded).expect("decode");
        assert_eq!(decoded, data);
    }
}
