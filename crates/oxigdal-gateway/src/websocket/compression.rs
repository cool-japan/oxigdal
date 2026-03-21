//! WebSocket message compression.

use crate::error::{GatewayError, Result};

/// Compression method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionMethod {
    /// No compression
    None,
    /// Deflate compression
    Deflate,
    /// Brotli compression
    Brotli,
}

/// Message compressor.
pub struct MessageCompressor {
    method: CompressionMethod,
}

impl MessageCompressor {
    /// Creates a new message compressor.
    pub fn new(method: CompressionMethod) -> Self {
        Self { method }
    }

    /// Compresses data.
    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        match self.method {
            CompressionMethod::None => Ok(data.to_vec()),
            CompressionMethod::Deflate => self.compress_deflate(data),
            CompressionMethod::Brotli => self.compress_brotli(data),
        }
    }

    /// Decompresses data.
    pub fn decompress(&self, data: &[u8]) -> Result<Vec<u8>> {
        match self.method {
            CompressionMethod::None => Ok(data.to_vec()),
            CompressionMethod::Deflate => self.decompress_deflate(data),
            CompressionMethod::Brotli => self.decompress_brotli(data),
        }
    }

    /// Compresses using deflate.
    fn compress_deflate(&self, data: &[u8]) -> Result<Vec<u8>> {
        oxiarc_deflate::deflate(data, 6)
            .map_err(|e| GatewayError::InternalError(format!("Compression error: {}", e)))
    }

    /// Decompresses using deflate.
    fn decompress_deflate(&self, data: &[u8]) -> Result<Vec<u8>> {
        oxiarc_deflate::inflate(data)
            .map_err(|e| GatewayError::InternalError(format!("Decompression error: {}", e)))
    }

    /// Compresses using brotli.
    fn compress_brotli(&self, data: &[u8]) -> Result<Vec<u8>> {
        oxiarc_brotli::compress(data, 11)
            .map_err(|e| GatewayError::InternalError(format!("Compression error: {}", e)))
    }

    /// Decompresses using brotli.
    fn decompress_brotli(&self, data: &[u8]) -> Result<Vec<u8>> {
        oxiarc_brotli::decompress(data)
            .map_err(|e| GatewayError::InternalError(format!("Decompression error: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_compression() {
        let compressor = MessageCompressor::new(CompressionMethod::None);
        let data = b"Hello, World!";

        let compressed = compressor.compress(data);
        assert!(compressed.is_ok());
        let compressed = compressed.ok().unwrap_or_default();
        assert_eq!(compressed, data);

        let decompressed = compressor.decompress(&compressed);
        assert!(decompressed.is_ok());
        let decompressed = decompressed.ok().unwrap_or_default();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_deflate_compression() {
        let compressor = MessageCompressor::new(CompressionMethod::Deflate);
        // Use larger data with repetition to ensure compression is effective
        let data = b"Hello, World! This is a test message for compression. Hello, World! This is a test message for compression. Hello, World! This is a test message for compression. Hello, World! This is a test message for compression. Hello, World! This is a test message for compression. Hello, World! This is a test message for compression.";

        let compressed = compressor.compress(data);
        assert!(compressed.is_ok());
        let compressed = compressed.ok().unwrap_or_default();
        assert!(compressed.len() < data.len());

        let decompressed = compressor.decompress(&compressed);
        assert!(decompressed.is_ok());
        let decompressed = decompressed.ok().unwrap_or_default();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_brotli_compression() {
        let compressor = MessageCompressor::new(CompressionMethod::Brotli);
        let data = b"Hello, World! This is a test message for brotli compression.";

        let compressed = compressor.compress(data);
        assert!(compressed.is_ok());

        let compressed = compressed.ok().unwrap_or_default();
        let decompressed = compressor.decompress(&compressed);
        assert!(decompressed.is_ok());
        let decompressed = decompressed.ok().unwrap_or_default();
        assert_eq!(decompressed, data);
    }
}
