//! Parallel compression and decompression
//!
//! This module provides parallel compression/decompression using rayon.

use crate::{
    codecs::{Lz4Codec, ZstdCodec},
    error::{CompressionError, Result},
    metadata::CompressionMetadata,
};
use rayon::prelude::*;
use std::time::Instant;

/// Parallel compression configuration
#[derive(Debug, Clone)]
pub struct ParallelConfig {
    /// Block size for parallel processing
    pub block_size: usize,

    /// Number of threads (0 = auto)
    pub num_threads: usize,
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            block_size: 1024 * 1024, // 1 MB
            num_threads: 0,          // Auto-detect
        }
    }
}

impl ParallelConfig {
    /// Create config with block size
    pub fn with_block_size(block_size: usize) -> Self {
        Self {
            block_size,
            ..Default::default()
        }
    }

    /// Set number of threads
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.num_threads = threads;
        self
    }
}

/// Parallel compressor
pub struct ParallelCompressor {
    config: ParallelConfig,
}

impl ParallelCompressor {
    /// Create new parallel compressor
    pub fn new() -> Self {
        Self {
            config: ParallelConfig::default(),
        }
    }

    /// Create with configuration
    pub fn with_config(config: ParallelConfig) -> Self {
        Self { config }
    }

    /// Compress data in parallel using LZ4
    pub fn compress_lz4(&self, input: &[u8]) -> Result<(Vec<u8>, CompressionMetadata)> {
        let start = Instant::now();

        if input.is_empty() {
            let metadata = CompressionMetadata::new("lz4-parallel".to_string(), 0, 0);
            return Ok((Vec::new(), metadata));
        }

        let codec = Lz4Codec::new();

        // Split into blocks
        let blocks: Vec<&[u8]> = input.chunks(self.config.block_size).collect();

        // Compress blocks in parallel
        let compressed_blocks: Result<Vec<Vec<u8>>> = blocks
            .par_iter()
            .map(|block| codec.compress(block))
            .collect();

        let compressed_blocks = compressed_blocks?;

        // Assemble output: header + block sizes + compressed blocks
        let mut output = Vec::new();

        // Write header: num_blocks (u32) + original_size (u64) + block_size (u32)
        output.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
        output.extend_from_slice(&(input.len() as u64).to_le_bytes());
        output.extend_from_slice(&(self.config.block_size as u32).to_le_bytes());

        // Write block sizes
        for block in &compressed_blocks {
            output.extend_from_slice(&(block.len() as u32).to_le_bytes());
        }

        // Write compressed blocks
        for block in compressed_blocks {
            output.extend_from_slice(&block);
        }

        let duration = start.elapsed();

        let metadata =
            CompressionMetadata::new("lz4-parallel".to_string(), input.len(), output.len())
                .with_duration(duration);

        Ok((output, metadata))
    }

    /// Decompress LZ4 parallel data
    pub fn decompress_lz4(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        if input.len() < 16 {
            return Err(CompressionError::InvalidBufferSize(
                "Input too small for parallel format".to_string(),
            ));
        }

        // Read header
        let num_blocks = u32::from_le_bytes([input[0], input[1], input[2], input[3]]) as usize;
        let original_size = u64::from_le_bytes([
            input[4], input[5], input[6], input[7], input[8], input[9], input[10], input[11],
        ]) as usize;
        let block_size = u32::from_le_bytes([input[12], input[13], input[14], input[15]]) as usize;

        let mut pos = 16;

        // Read block sizes
        let mut block_sizes = Vec::with_capacity(num_blocks);
        for _ in 0..num_blocks {
            if pos + 4 > input.len() {
                return Err(CompressionError::InvalidBufferSize(
                    "Truncated block size".to_string(),
                ));
            }

            let size =
                u32::from_le_bytes([input[pos], input[pos + 1], input[pos + 2], input[pos + 3]])
                    as usize;
            block_sizes.push(size);
            pos += 4;
        }

        // Extract compressed blocks
        let mut compressed_blocks = Vec::with_capacity(num_blocks);
        for &size in &block_sizes {
            if pos + size > input.len() {
                return Err(CompressionError::InvalidBufferSize(
                    "Truncated compressed block".to_string(),
                ));
            }

            compressed_blocks.push(&input[pos..pos + size]);
            pos += size;
        }

        // Decompress blocks in parallel
        let codec = Lz4Codec::new();

        let decompressed_blocks: Result<Vec<Vec<u8>>> = compressed_blocks
            .par_iter()
            .enumerate()
            .map(|(i, block)| {
                let expected_size = if i == num_blocks - 1 {
                    // Last block may be smaller
                    original_size - (i * block_size)
                } else {
                    block_size
                };
                codec.decompress(block, Some(expected_size))
            })
            .collect();

        let decompressed_blocks = decompressed_blocks?;

        // Assemble output
        let mut output = Vec::with_capacity(original_size);
        for block in decompressed_blocks {
            output.extend_from_slice(&block);
        }

        Ok(output)
    }

    /// Compress data in parallel using Zstd
    pub fn compress_zstd(&self, input: &[u8]) -> Result<(Vec<u8>, CompressionMetadata)> {
        let start = Instant::now();

        if input.is_empty() {
            let metadata = CompressionMetadata::new("zstd-parallel".to_string(), 0, 0);
            return Ok((Vec::new(), metadata));
        }

        let codec = ZstdCodec::new();

        let blocks: Vec<&[u8]> = input.chunks(self.config.block_size).collect();

        let compressed_blocks: Result<Vec<Vec<u8>>> = blocks
            .par_iter()
            .map(|block| codec.compress(block))
            .collect();

        let compressed_blocks = compressed_blocks?;

        let mut output = Vec::new();

        output.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
        output.extend_from_slice(&(input.len() as u64).to_le_bytes());
        output.extend_from_slice(&(self.config.block_size as u32).to_le_bytes());

        for block in &compressed_blocks {
            output.extend_from_slice(&(block.len() as u32).to_le_bytes());
        }

        for block in compressed_blocks {
            output.extend_from_slice(&block);
        }

        let duration = start.elapsed();

        let metadata =
            CompressionMetadata::new("zstd-parallel".to_string(), input.len(), output.len())
                .with_duration(duration);

        Ok((output, metadata))
    }

    /// Decompress Zstd parallel data
    pub fn decompress_zstd(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        if input.len() < 16 {
            return Err(CompressionError::InvalidBufferSize(
                "Input too small for parallel format".to_string(),
            ));
        }

        let num_blocks = u32::from_le_bytes([input[0], input[1], input[2], input[3]]) as usize;
        let original_size = u64::from_le_bytes([
            input[4], input[5], input[6], input[7], input[8], input[9], input[10], input[11],
        ]) as usize;
        let block_size = u32::from_le_bytes([input[12], input[13], input[14], input[15]]) as usize;

        let mut pos = 16;

        let mut block_sizes = Vec::with_capacity(num_blocks);
        for _ in 0..num_blocks {
            if pos + 4 > input.len() {
                return Err(CompressionError::InvalidBufferSize(
                    "Truncated block size".to_string(),
                ));
            }

            let size =
                u32::from_le_bytes([input[pos], input[pos + 1], input[pos + 2], input[pos + 3]])
                    as usize;
            block_sizes.push(size);
            pos += 4;
        }

        let mut compressed_blocks = Vec::with_capacity(num_blocks);
        for &size in &block_sizes {
            if pos + size > input.len() {
                return Err(CompressionError::InvalidBufferSize(
                    "Truncated compressed block".to_string(),
                ));
            }

            compressed_blocks.push(&input[pos..pos + size]);
            pos += size;
        }

        let codec = ZstdCodec::new();

        let decompressed_blocks: Result<Vec<Vec<u8>>> = compressed_blocks
            .par_iter()
            .map(|block| codec.decompress(block, Some(block_size * 2)))
            .collect();

        let decompressed_blocks = decompressed_blocks?;

        let mut output = Vec::with_capacity(original_size);
        for block in decompressed_blocks {
            output.extend_from_slice(&block);
        }

        Ok(output)
    }
}

impl Default for ParallelCompressor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallel_lz4() {
        let compressor = ParallelCompressor::new();
        let data = vec![42u8; 10_000_000]; // 10 MB

        let (compressed, metadata) = compressor.compress_lz4(&data).expect("Compression failed");

        assert!(compressed.len() < data.len());
        assert!(metadata.compression_ratio > 1.0);

        let decompressed = compressor
            .decompress_lz4(&compressed)
            .expect("Decompression failed");

        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_parallel_zstd() {
        let compressor = ParallelCompressor::new();
        let data = vec![42u8; 10_000_000]; // 10 MB

        let (compressed, metadata) = compressor.compress_zstd(&data).expect("Compression failed");

        assert!(compressed.len() < data.len());
        assert!(metadata.compression_ratio > 1.0);

        let decompressed = compressor
            .decompress_zstd(&compressed)
            .expect("Decompression failed");

        assert_eq!(decompressed, data);
    }
}
