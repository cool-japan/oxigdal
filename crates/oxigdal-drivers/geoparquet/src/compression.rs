//! Compression support for GeoParquet files

use crate::error::{GeoParquetError, Result};
use parquet::basic::Compression as ParquetCompression;

/// Compression type for GeoParquet files
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionType {
    /// No compression
    Uncompressed,
    /// Snappy compression
    #[default]
    Snappy,
    /// Gzip compression
    Gzip,
    /// Brotli compression
    Brotli,
    /// LZ4 compression
    Lz4,
    /// Zstd compression
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
