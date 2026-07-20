//! Compression support for GeoParquet files

use crate::error::{GeoParquetError, Result};
use parquet::basic::Compression as ParquetCompression;

/// Compression type for GeoParquet files
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionType {
    /// No compression
    #[default]
    Uncompressed,
    /// Snappy compression
    Snappy,
    /// Gzip compression
    Gzip,
    /// Brotli compression
    Brotli,
    /// LZ4 compression
    Lz4,
    /// Zstd compression.
    ///
    /// This variant exists so that the compression codec of an existing file
    /// can be *identified* from its Parquet metadata. Actual Zstd page
    /// (de)compression is **not** available in the pure-Rust build: the upstream
    /// `parquet` codec is backed by the C `libzstd` FFI (`zstd-sys`), which the
    /// COOLJAPAN Pure-Rust policy forbids, so the `parquet/zstd` feature is not
    /// exposed by this crate. Reading or writing Zstd-compressed pages therefore
    /// surfaces a typed `parquet` error rather than linking C code. See the
    /// `[features]` note in `Cargo.toml`.
    Zstd,
}

impl CompressionType {
    /// Converts to Parquet compression type
    pub fn to_parquet(self) -> ParquetCompression {
        match self {
            Self::Uncompressed => ParquetCompression::UNCOMPRESSED,
            Self::Snappy => ParquetCompression::SNAPPY,
            Self::Gzip => ParquetCompression::GZIP(Default::default()),
            Self::Brotli => ParquetCompression::BROTLI(Default::default()),
            Self::Lz4 => ParquetCompression::LZ4,
            Self::Zstd => ParquetCompression::ZSTD(Default::default()),
        }
    }

    /// Converts from Parquet compression type
    pub fn from_parquet(compression: &ParquetCompression) -> Result<Self> {
        match compression {
            ParquetCompression::UNCOMPRESSED => Ok(Self::Uncompressed),
            ParquetCompression::SNAPPY => Ok(Self::Snappy),
            ParquetCompression::GZIP(_) => Ok(Self::Gzip),
            ParquetCompression::BROTLI(_) => Ok(Self::Brotli),
            ParquetCompression::LZ4 => Ok(Self::Lz4),
            ParquetCompression::ZSTD(_) => Ok(Self::Zstd),
            other => Err(GeoParquetError::unsupported(format!(
                "Compression type: {other:?}"
            ))),
        }
    }

    /// Returns the name of this compression type
    pub fn name(self) -> &'static str {
        match self {
            Self::Uncompressed => "uncompressed",
            Self::Snappy => "snappy",
            Self::Gzip => "gzip",
            Self::Brotli => "brotli",
            Self::Lz4 => "lz4",
            Self::Zstd => "zstd",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_conversion() {
        let snappy = CompressionType::Snappy;
        let parquet_compression = snappy.to_parquet();
        assert_eq!(parquet_compression, ParquetCompression::SNAPPY);

        let back = CompressionType::from_parquet(&parquet_compression);
        assert!(back.is_ok());
        assert_eq!(back.expect("should convert"), CompressionType::Snappy);
    }

    #[test]
    fn test_compression_names() {
        assert_eq!(CompressionType::Snappy.name(), "snappy");
        assert_eq!(CompressionType::Gzip.name(), "gzip");
    }
}
