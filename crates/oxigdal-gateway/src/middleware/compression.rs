//! Response compression middleware.

use super::{Middleware, Request, Response};
use crate::error::Result;

/// Compression algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionAlgorithm {
    /// Gzip compression
    Gzip,
    /// Brotli compression
    Brotli,
    /// No compression
    None,
}

/// Compression configuration.
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// Minimum size to compress (bytes)
    pub min_size: usize,
    /// Preferred algorithm
    pub algorithm: CompressionAlgorithm,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            min_size: 1024, // 1KB
            algorithm: CompressionAlgorithm::Gzip,
        }
    }
}

/// Compression middleware.
pub struct CompressionMiddleware {
    config: CompressionConfig,
}

impl CompressionMiddleware {
    /// Creates a new compression middleware.
    pub fn new(config: CompressionConfig) -> Self {
        Self { config }
    }

    /// Compresses data using gzip.
    fn compress_gzip(&self, data: &[u8]) -> Result<Vec<u8>> {
        oxiarc_archive::gzip::compress(data, 6)
            .map_err(|e| crate::error::GatewayError::InternalError(e.to_string()))
    }

    /// Compresses data using brotli.
    fn compress_brotli(&self, data: &[u8]) -> Result<Vec<u8>> {
        oxiarc_brotli::compress(data, 11)
            .map_err(|e| crate::error::GatewayError::InternalError(e.to_string()))
    }
}

#[async_trait::async_trait]
impl Middleware for CompressionMiddleware {
    async fn before_request(&self, _request: &mut Request) -> Result<()> {
        Ok(())
    }

    async fn after_response(&self, response: &mut Response) -> Result<()> {
        if response.body.len() < self.config.min_size {
            return Ok(());
        }

        let compressed = match self.config.algorithm {
            CompressionAlgorithm::Gzip => {
                response
                    .headers
                    .insert("Content-Encoding".to_string(), "gzip".to_string());
                self.compress_gzip(&response.body)?
            }
            CompressionAlgorithm::Brotli => {
                response
                    .headers
                    .insert("Content-Encoding".to_string(), "br".to_string());
                self.compress_brotli(&response.body)?
            }
            CompressionAlgorithm::None => return Ok(()),
        };

        response.body = compressed;
        response.headers.insert(
            "Content-Length".to_string(),
            response.body.len().to_string(),
        );

        Ok(())
    }
}
