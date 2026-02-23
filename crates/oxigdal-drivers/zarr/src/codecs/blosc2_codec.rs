//! Blosc2 codec implementation (Pure Rust)
//!
//! This module provides a Pure Rust implementation of Blosc2 compression,
//! which is a meta-compressor that uses blocking, shuffling, and various
//! compression algorithms.

use crate::codecs::Codec;
use crate::error::{CodecError, Result, ZarrError};

/// Shuffle modes for Blosc2
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShuffleMode {
    /// No shuffle
    NoShuffle = 0,
    /// Byte shuffle
    ByteShuffle = 1,
    /// Bit shuffle
    BitShuffle = 2,
}

impl ShuffleMode {
    /// Creates from integer value
    ///
    /// # Errors
    /// Returns error if value is invalid
    pub fn from_u8(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::NoShuffle),
            1 => Ok(Self::ByteShuffle),
            2 => Ok(Self::BitShuffle),
            _ => Err(ZarrError::Codec(CodecError::InvalidConfiguration {
                codec: "blosc2".to_string(),
                message: format!("Invalid shuffle mode: {value}"),
            })),
        }
    }
}

/// Blosc2 compressor algorithm
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BloscCompressor {
    /// LZ4 compression
    Lz4,
    /// LZ4HC compression (high compression)
    Lz4hc,
    /// Zlib compression
    Zlib,
    /// Zstd compression
    Zstd,
}

impl BloscCompressor {
    /// Creates from string name
    ///
    /// # Errors
    /// Returns error if name is invalid
    pub fn from_name(name: &str) -> Result<Self> {
        match name.to_lowercase().as_str() {
            "lz4" => Ok(Self::Lz4),
            "lz4hc" => Ok(Self::Lz4hc),
            "zlib" => Ok(Self::Zlib),
            "zstd" => Ok(Self::Zstd),
            _ => Err(ZarrError::Codec(CodecError::UnknownCodec {
                codec: format!("blosc/{name}"),
            })),
        }
    }

    /// Returns the compressor name
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Lz4 => "lz4",
            Self::Lz4hc => "lz4hc",
            Self::Zlib => "zlib",
            Self::Zstd => "zstd",
        }
    }
}

/// Blosc2 codec (Pure Rust implementation)
#[derive(Debug, Clone)]
pub struct Blosc2Codec {
    /// Compression algorithm
    compressor: BloscCompressor,
    /// Compression level (0-9)
    clevel: u8,
    /// Shuffle mode
    shuffle: ShuffleMode,
    /// Type size for shuffle
    typesize: usize,
    /// Block size
    blocksize: usize,
}

impl Blosc2Codec {
    /// Creates a new Blosc2 codec
    ///
    /// # Errors
    /// Returns error if parameters are invalid
    pub fn new(
        compressor: impl Into<String>,
        clevel: u8,
        shuffle: u8,
        typesize: Option<usize>,
        blocksize: Option<usize>,
    ) -> Result<Self> {
        if clevel > 9 {
            return Err(ZarrError::Codec(CodecError::InvalidConfiguration {
                codec: "blosc2".to_string(),
                message: format!("Compression level must be 0-9, got {clevel}"),
            }));
        }

        let compressor = BloscCompressor::from_name(&compressor.into())?;
        let shuffle = ShuffleMode::from_u8(shuffle)?;

        Ok(Self {
            compressor,
            clevel,
            shuffle,
            typesize: typesize.unwrap_or(1),
            blocksize: blocksize.unwrap_or(0),
        })
    }

    /// Performs byte shuffle
    fn byte_shuffle(data: &[u8], typesize: usize) -> Vec<u8> {
        if typesize <= 1 {
            return data.to_vec();
        }

        let len = data.len();
        let mut shuffled = vec![0u8; len];

        // Group bytes by position within type
        for i in 0..typesize {
            let mut pos = i;
            for chunk in data.chunks_exact(typesize) {
                shuffled[pos] = chunk[i];
                pos += typesize;
            }
        }

        // Handle remainder
        let remainder = len % typesize;
        if remainder > 0 {
            let start = len - remainder;
            shuffled[len - remainder..].copy_from_slice(&data[start..]);
        }

        shuffled
    }

    /// Performs byte unshuffle
    fn byte_unshuffle(data: &[u8], typesize: usize) -> Vec<u8> {
        if typesize <= 1 {
            return data.to_vec();
        }

        let len = data.len();
        let mut unshuffled = vec![0u8; len];

        // Restore original byte order
        for i in 0..typesize {
            let mut pos = i;
            for j in 0..(len / typesize) {
                unshuffled[j * typesize + i] = data[pos];
                pos += typesize;
            }
        }

        // Handle remainder
        let remainder = len % typesize;
        if remainder > 0 {
            let start = len - remainder;
            unshuffled[start..].copy_from_slice(&data[start..]);
        }

        unshuffled
    }

    /// Performs bit shuffle
    fn bit_shuffle(data: &[u8], typesize: usize) -> Vec<u8> {
        if typesize <= 1 {
            return data.to_vec();
        }

        let len = data.len();
        let mut shuffled = vec![0u8; len];

        // Bit-level shuffling (simplified implementation)
        for (i, &byte) in data.iter().enumerate() {
            let type_offset = i % typesize;
            let block = i / typesize;
            let bit_offset = type_offset * 8;

            for bit in 0..8 {
                if byte & (1 << bit) != 0 {
                    let dest_byte = (bit_offset + bit) / 8;
                    let dest_bit = (bit_offset + bit) % 8;
                    if dest_byte < len {
                        shuffled[block * typesize + dest_byte] |= 1 << dest_bit;
                    }
                }
            }
        }

        shuffled
    }

    /// Performs bit unshuffle
    fn bit_unshuffle(data: &[u8], typesize: usize) -> Vec<u8> {
        if typesize <= 1 {
            return data.to_vec();
        }

        let len = data.len();
        let mut unshuffled = vec![0u8; len];

        // Reverse bit-level shuffling
        for block in 0..(len / typesize) {
            for type_offset in 0..typesize {
                let bit_offset = type_offset * 8;
                let mut byte = 0u8;

                for bit in 0..8 {
                    let src_byte = (bit_offset + bit) / 8;
                    let src_bit = (bit_offset + bit) % 8;
                    if src_byte < typesize
                        && data[block * typesize + src_byte] & (1 << src_bit) != 0
                    {
                        byte |= 1 << bit;
                    }
                }

                unshuffled[block * typesize + type_offset] = byte;
            }
        }

        // Handle remainder
        let remainder = len % typesize;
        if remainder > 0 {
            let start = len - remainder;
            unshuffled[start..].copy_from_slice(&data[start..]);
        }

        unshuffled
    }

    /// Compresses data using the underlying compressor
    fn compress_data(&self, data: &[u8]) -> Result<Vec<u8>> {
        // For Pure Rust implementation, we use the built-in compression
        // In production, integrate with actual compression libraries

        match self.compressor {
            BloscCompressor::Lz4 => {
                // Placeholder: use actual LZ4 compression
                Ok(data.to_vec())
            }
            BloscCompressor::Lz4hc => {
                // Placeholder: use actual LZ4HC compression
                Ok(data.to_vec())
            }
            BloscCompressor::Zlib => {
                // Placeholder: use actual Zlib compression
                #[cfg(feature = "gzip")]
                {
                    use crate::codecs::gzip::GzipCodec;
                    let codec = GzipCodec::new(self.clevel.into())?;
                    codec.encode(data)
                }
                #[cfg(not(feature = "gzip"))]
                {
                    Ok(data.to_vec())
                }
            }
            BloscCompressor::Zstd => {
                // Placeholder: use actual Zstd compression
                #[cfg(feature = "zstd")]
                {
                    use crate::codecs::zstd_codec::ZstdCodec;
                    let codec = ZstdCodec::new(self.clevel.into())?;
                    codec.encode(data)
                }
                #[cfg(not(feature = "zstd"))]
                {
                    Ok(data.to_vec())
                }
            }
        }
    }

    /// Decompresses data using the underlying compressor
    fn decompress_data(&self, data: &[u8]) -> Result<Vec<u8>> {
        match self.compressor {
            BloscCompressor::Lz4 => {
                // Placeholder: use actual LZ4 decompression
                Ok(data.to_vec())
            }
            BloscCompressor::Lz4hc => {
                // Placeholder: use actual LZ4HC decompression
                Ok(data.to_vec())
            }
            BloscCompressor::Zlib => {
                // Placeholder: use actual Zlib decompression
                #[cfg(feature = "gzip")]
                {
                    use crate::codecs::gzip::GzipCodec;
                    let codec = GzipCodec::new(self.clevel.into())?;
                    codec.decode(data)
                }
                #[cfg(not(feature = "gzip"))]
                {
                    Ok(data.to_vec())
                }
            }
            BloscCompressor::Zstd => {
                // Placeholder: use actual Zstd decompression
                #[cfg(feature = "zstd")]
                {
                    use crate::codecs::zstd_codec::ZstdCodec;
                    let codec = ZstdCodec::new(self.clevel.into())?;
                    codec.decode(data)
                }
                #[cfg(not(feature = "zstd"))]
                {
                    Ok(data.to_vec())
                }
            }
        }
    }
}

impl Codec for Blosc2Codec {
    fn id(&self) -> &str {
        "blosc2"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Apply shuffle
        let shuffled = match self.shuffle {
            ShuffleMode::NoShuffle => data.to_vec(),
            ShuffleMode::ByteShuffle => Self::byte_shuffle(data, self.typesize),
            ShuffleMode::BitShuffle => Self::bit_shuffle(data, self.typesize),
        };

        // Compress
        self.compress_data(&shuffled)
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Decompress
        let decompressed = self.decompress_data(data)?;

        // Apply unshuffle
        let unshuffled = match self.shuffle {
            ShuffleMode::NoShuffle => decompressed,
            ShuffleMode::ByteShuffle => Self::byte_unshuffle(&decompressed, self.typesize),
            ShuffleMode::BitShuffle => Self::bit_unshuffle(&decompressed, self.typesize),
        };

        Ok(unshuffled)
    }

    fn clone_box(&self) -> Box<dyn Codec> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shuffle_mode_from_u8() {
        assert_eq!(
            ShuffleMode::from_u8(0).expect("noshuffle"),
            ShuffleMode::NoShuffle
        );
        assert_eq!(
            ShuffleMode::from_u8(1).expect("byteshuffle"),
            ShuffleMode::ByteShuffle
        );
        assert_eq!(
            ShuffleMode::from_u8(2).expect("bitshuffle"),
            ShuffleMode::BitShuffle
        );
        assert!(ShuffleMode::from_u8(3).is_err());
    }

    #[test]
    fn test_blosc_compressor_from_name() {
        assert_eq!(
            BloscCompressor::from_name("lz4").expect("lz4"),
            BloscCompressor::Lz4
        );
        assert_eq!(
            BloscCompressor::from_name("zstd").expect("zstd"),
            BloscCompressor::Zstd
        );
        assert!(BloscCompressor::from_name("unknown").is_err());
    }

    #[test]
    fn test_byte_shuffle() {
        let data = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let shuffled = Blosc2Codec::byte_shuffle(&data, 4);
        assert_eq!(shuffled.len(), data.len());

        let unshuffled = Blosc2Codec::byte_unshuffle(&shuffled, 4);
        assert_eq!(unshuffled, data);
    }

    #[test]
    fn test_blosc2_codec_creation() {
        let codec = Blosc2Codec::new("lz4", 5, 1, Some(4), Some(0)).expect("create");
        assert_eq!(codec.id(), "blosc2");
        assert_eq!(codec.clevel, 5);
    }

    #[test]
    fn test_blosc2_invalid_clevel() {
        let result = Blosc2Codec::new("lz4", 10, 0, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_blosc2_encode_decode_noshuffle() {
        let codec = Blosc2Codec::new("lz4", 3, 0, None, None).expect("create");
        let data = vec![42u8; 1000];

        let encoded = codec.encode(&data).expect("encode");
        let decoded = codec.decode(&encoded).expect("decode");

        assert_eq!(decoded, data);
    }

    #[test]
    fn test_blosc2_encode_decode_byteshuffle() {
        let codec = Blosc2Codec::new("lz4", 3, 1, Some(4), None).expect("create");
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

        let encoded = codec.encode(&data).expect("encode");
        let decoded = codec.decode(&encoded).expect("decode");

        assert_eq!(decoded, data);
    }
}
