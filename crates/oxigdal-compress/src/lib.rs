//! OxiGDAL Compression Library
//!
//! Advanced compression codecs and auto-selection for geospatial data.
//!
//! # Features
//!
//! - **Standard Codecs**: LZ4, Zstandard, Brotli, Snappy, DEFLATE
//! - **Geospatial Codecs**: Delta encoding, RLE, Dictionary compression
//! - **Floating-Point**: ZFP and SZ-style compression with error bounds
//! - **Auto-Selection**: Intelligent codec selection based on data characteristics
//! - **Parallel Processing**: Multi-threaded compression/decompression
//! - **Benchmarking**: Built-in performance measurement
//!
//! # Examples
//!
//! ```rust
//! use oxigdal_compress::{
//!     codecs::{Lz4Codec, ZstdCodec},
//!     auto_select::{AutoSelector, CompressionGoal, DataType, DataCharacteristics},
//! };
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Basic compression with LZ4
//! let codec = Lz4Codec::new();
//! let data = b"Hello, world!".repeat(1000);
//! let compressed = codec.compress(&data)?;
//! let decompressed = codec.decompress(&compressed, Some(data.len()))?;
//! assert_eq!(decompressed, data);
//!
//! // Auto-selection
//! let selector = AutoSelector::new(CompressionGoal::Balanced);
//! let characteristics = DataCharacteristics {
//!     data_type: DataType::Categorical,
//!     size: 10000,
//!     entropy: 0.3,
//!     unique_count: Some(10),
//!     value_range: None,
//!     run_length_ratio: Some(50.0),
//! };
//! let recommendations = selector.recommend(&characteristics);
//! println!("Recommended codec: {:?}", recommendations[0].codec);
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]
#![warn(clippy::unwrap_used, clippy::panic)]

pub mod auto_select;
pub mod benchmark;
pub mod codecs;
pub mod error;
pub mod floating_point;
pub mod metadata;
pub mod parallel;

pub use error::{CompressionError, Result};

/// Prelude module for common imports
pub mod prelude {
    pub use crate::{
        auto_select::{AutoSelector, CompressionGoal, DataCharacteristics, DataType},
        codecs::{
            BrotliCodec, CodecType, DeflateCodec, DeltaCodec, DictionaryCodec, Lz4Codec, RleCodec,
            SnappyCodec, ZstdCodec,
        },
        error::{CompressionError, Result},
        metadata::CompressionMetadata,
        parallel::ParallelCompressor,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lz4_roundtrip() {
        use codecs::Lz4Codec;

        let codec = Lz4Codec::new();
        let data = b"Test data".repeat(100);

        let compressed = codec.compress(&data).expect("Compression failed");
        let decompressed = codec
            .decompress(&compressed, Some(data.len()))
            .expect("Decompression failed");

        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_zstd_roundtrip() {
        use codecs::ZstdCodec;

        let codec = ZstdCodec::new();
        let data = b"Test data".repeat(100);

        let compressed = codec.compress(&data).expect("Compression failed");
        let decompressed = codec
            .decompress(&compressed, Some(data.len() * 2))
            .expect("Decompression failed");

        assert_eq!(decompressed, data);
    }
}
