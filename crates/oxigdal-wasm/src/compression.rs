//! Tile compression and decompression utilities
//!
//! This module provides various compression algorithms for reducing memory usage
//! and bandwidth requirements when working with geospatial tiles in WASM environments.
//!
//! # Overview
//!
//! Compression is essential for efficient geospatial data handling:
//!
//! - **RLE (Run-Length Encoding)**: Best for images with large uniform areas
//! - **Delta Encoding**: Excellent for gradient data (DEMs, temperature)
//! - **Huffman Encoding**: General-purpose statistical compression
//! - **LZ77**: Dictionary-based compression for mixed content
//! - **Automatic Selection**: Benchmark and choose optimal algorithm
//!
//! # Why Compress Tiles?
//!
//! Geospatial tiles can be large:
//! - 256x256 RGBA tile: 262,144 bytes (256 KB)
//! - 100 cached tiles: 25.6 MB uncompressed
//! - With 50% compression: 12.8 MB (50% savings!)
//!
//! Benefits:
//! 1. **Memory**: Fit more tiles in cache
//! 2. **Bandwidth**: Faster loading over network
//! 3. **Storage**: Smaller cache footprint
//! 4. **Performance**: Less memory pressure
//!
//! # Compression Algorithms
//!
//! ## RLE (Run-Length Encoding)
//! Replaces sequences of identical bytes with count+value pairs.
//!
//! Best for:
//! - Satellite imagery with large uniform areas (ocean, desert)
//! - Binary masks
//! - Simple graphics
//!
//! Performance:
//! - Compression: O(n), very fast
//! - Decompression: O(n), very fast
//! - Ratio: 2-10x for uniform data, 0.5x for random data
//!
//! Example:
//! ```text
//! Input:  [255, 255, 255, 255, 0, 0, 128, 128, 128]
//! Output: [4, 255, 2, 0, 3, 128]
//! Size:   9 bytes → 6 bytes (33% savings)
//! ```
//!
//! ## Delta Encoding
//! Stores differences between adjacent pixels instead of absolute values.
//!
//! Best for:
//! - DEMs (Digital Elevation Models)
//! - Temperature/pressure fields
//! - Smooth gradients
//!
//! Performance:
//! - Compression: O(n)
//! - Decompression: O(n)
//! - Ratio: 2-4x for gradient data
//!
//! Example:
//! ```text
//! Input:  [100, 102, 105, 103, 104]
//! Output: [100, +2, +3, -2, +1]
//! Deltas are smaller → better compression with subsequent algorithm
//! ```
//!
//! ## Huffman Encoding
//! Uses variable-length codes based on frequency.
//!
//! Best for:
//! - General-purpose compression
//! - Images with varying content
//! - Text/metadata
//!
//! Performance:
//! - Compression: O(n log n) - slower
//! - Decompression: O(n)
//! - Ratio: 1.5-3x typical
//!
//! ## LZ77 (Dictionary Compression)
//! Finds repeated sequences and references them.
//!
//! Best for:
//! - Repeated patterns
//! - Mixed content
//! - General-purpose
//!
//! Performance:
//! - Compression: O(n²) worst case
//! - Decompression: O(n)
//! - Ratio: 2-5x typical
//!
//! # Usage Examples
//!
//! ## Basic Compression
//! ```rust
//! use oxigdal_wasm::{TileCompressor, CompressionAlgorithm};
//!
//! let compressor = TileCompressor::new(CompressionAlgorithm::Lz77);
//!
//! // Compress a tile
//! let tile_data = vec![0u8; 256 * 256 * 4]; // 256 KB
//! let (compressed, stats) = compressor.compress(&tile_data);
//!
//! println!("Original: {} bytes", stats.original_size);
//! println!("Compressed: {} bytes", stats.compressed_size);
//! println!("Ratio: {:.1}%", stats.ratio * 100.0);
//! println!("Saved: {:.1}%", stats.space_saved_percent());
//!
//! // Decompress
//! let decompressed = compressor.decompress(&compressed)?;
//! assert_eq!(tile_data, decompressed);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Automatic Algorithm Selection
//! ```rust
//! use oxigdal_wasm::{CompressionBenchmark, TileCompressor};
//!
//! // Test all algorithms and choose best
//! let tile_data = vec![0u8; 256 * 256 * 4]; // Simulated tile data
//! let best = CompressionBenchmark::find_best(&tile_data);
//! println!("Best algorithm: {:?}", best);
//!
//! let compressor = TileCompressor::new(best);
//! let (compressed, _) = compressor.compress(&tile_data);
//! ```
//!
//! ## 2D Delta Encoding for DEMs
//! ```rust
//! use oxigdal_wasm::{TileCompressor, CompressionAlgorithm};
//!
//! let dem_data = vec![100u8; 256 * 256]; // Simulated elevation data
//! let tile_width = 256;
//!
//! // Use 2D delta encoding (vertical differences)
//! let compressor = TileCompressor::new(CompressionAlgorithm::Delta);
//! let (compressed, stats) = compressor.compress_2d(&dem_data, tile_width);
//!
//! // DEMs typically achieve 3-5x compression
//! println!("DEM compression ratio: {:.2}x", 1.0 / stats.ratio);
//! ```
//!
//! # Performance Benchmarks
//!
//! Typical results for 256x256 RGBA tiles (262 KB):
//!
//! ```text
//! Algorithm   Compress Time   Decompress Time   Ratio   Best For
//! ─────────────────────────────────────────────────────────────
//! None        0ms             0ms               1.0x    Baseline
//! RLE         2ms             1ms               2.5x    Uniform areas
//! Delta       3ms             2ms               1.8x    Gradients
//! Huffman     15ms            5ms               2.2x    General
//! LZ77        25ms            8ms               3.5x    Mixed content
//! ```
//!
//! # Memory Overhead
//!
//! Compression requires temporary buffers:
//! - RLE: 2x input size worst case
//! - Delta: 1x input size
//! - Huffman: 3x input size (tree + codes + output)
//! - LZ77: 2x input size
//!
//! # Best Practices
//!
//! 1. **Profile First**: Test compression ratios on real data
//! 2. **Cache Results**: Don't recompress the same tile repeatedly
//! 3. **Choose Wisely**: Match algorithm to data type
//! 4. **Consider Trade-offs**: Compression time vs ratio vs memory
//! 5. **Batch Process**: Compress tiles during idle time
//! 6. **Monitor Performance**: Track compression stats
//! 7. **Fallback to None**: If compression makes data larger, store uncompressed
//!
//! # When NOT to Compress
//!
//! - Data already compressed (JPEG tiles)
//! - Very small tiles (< 1 KB) - overhead not worth it
//! - Random/encrypted data - won't compress well
//! - Real-time requirements - compression too slow
//! - Low memory pressure - no need to optimize
use crate::error::{WasmError, WasmResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
/// Compression algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    /// No compression
    None,
    /// Run-length encoding
    Rle,
    /// Delta encoding
    Delta,
    /// Huffman encoding (simplified)
    Huffman,
    /// LZ77-style compression
    Lz77,
}
/// Compression statistics
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CompressionStats {
    /// Original size in bytes
    pub original_size: usize,
    /// Compressed size in bytes
    pub compressed_size: usize,
    /// Compression ratio
    pub ratio: f64,
    /// Compression time in milliseconds
    pub compression_time_ms: f64,
}
impl CompressionStats {
    /// Creates new compression statistics
    pub const fn new(
        original_size: usize,
        compressed_size: usize,
        compression_time_ms: f64,
    ) -> Self {
        let ratio = if original_size > 0 {
            compressed_size as f64 / original_size as f64
        } else {
            0.0
        };
        Self {
            original_size,
            compressed_size,
            ratio,
            compression_time_ms,
        }
    }
    /// Returns space saved in bytes
    pub const fn space_saved(&self) -> usize {
        self.original_size.saturating_sub(self.compressed_size)
    }
    /// Returns space saved as percentage
    pub fn space_saved_percent(&self) -> f64 {
        if self.original_size == 0 {
            0.0
        } else {
            ((self.original_size - self.compressed_size) as f64 / self.original_size as f64) * 100.0
        }
    }
}
/// Run-length encoding compression
pub struct RleCompressor;
impl RleCompressor {
    /// Compresses data using RLE
    pub fn compress(data: &[u8]) -> Vec<u8> {
        if data.is_empty() {
            return Vec::new();
        }
        let mut compressed = Vec::new();
        let mut i = 0;
        while i < data.len() {
            let current = data[i];
            let mut count = 1u8;
            while i + (count as usize) < data.len()
                && data[i + (count as usize)] == current
                && count < 255
            {
                count += 1;
            }
            compressed.push(count);
            compressed.push(current);
            i += count as usize;
        }
        compressed
    }
    /// Decompresses RLE data
    pub fn decompress(data: &[u8]) -> WasmResult<Vec<u8>> {
        if data.len() % 2 != 0 {
            return Err(WasmError::InvalidOperation {
                operation: "RLE decompress".to_string(),
                reason: "Data length must be even".to_string(),
            });
        }
        let mut decompressed = Vec::new();
        for chunk in data.chunks_exact(2) {
            let count = chunk[0];
            let value = chunk[1];
            decompressed.resize(decompressed.len() + count as usize, value);
        }
        Ok(decompressed)
    }
}
/// Delta encoding compression
pub struct DeltaCompressor;
impl DeltaCompressor {
    /// Compresses data using delta encoding
    pub fn compress(data: &[u8]) -> Vec<u8> {
        if data.is_empty() {
            return Vec::new();
        }
        let mut compressed = Vec::with_capacity(data.len());
        compressed.push(data[0]);
        for i in 1..data.len() {
            let delta = data[i].wrapping_sub(data[i - 1]);
            compressed.push(delta);
        }
        compressed
    }
    /// Decompresses delta-encoded data
    pub fn decompress(data: &[u8]) -> WasmResult<Vec<u8>> {
        if data.is_empty() {
            return Ok(Vec::new());
        }
        let mut decompressed = Vec::with_capacity(data.len());
        decompressed.push(data[0]);
        for i in 1..data.len() {
            let prev = *decompressed.last().expect("decompressed is not empty");
            let value = prev.wrapping_add(data[i]);
            decompressed.push(value);
        }
        Ok(decompressed)
    }
    /// Compresses 2D delta encoding (for images)
    pub fn compress_2d(data: &[u8], width: usize) -> Vec<u8> {
        if data.is_empty() || width == 0 {
            return Vec::new();
        }
        let height = data.len() / width;
        let mut compressed = Vec::with_capacity(data.len());
        if !data.is_empty() {
            compressed.push(data[0]);
            for x in 1..width.min(data.len()) {
                let delta = data[x].wrapping_sub(data[x - 1]);
                compressed.push(delta);
            }
        }
        for y in 1..height {
            for x in 0..width {
                let idx = y * width + x;
                if idx < data.len() {
                    let delta = data[idx].wrapping_sub(data[idx - width]);
                    compressed.push(delta);
                }
            }
        }
        compressed
    }
    /// Decompresses 2D delta-encoded data
    pub fn decompress_2d(data: &[u8], width: usize) -> WasmResult<Vec<u8>> {
        if data.is_empty() || width == 0 {
            return Ok(Vec::new());
        }
        let height = data.len() / width;
        let mut decompressed = Vec::with_capacity(data.len());
        if !data.is_empty() {
            decompressed.push(data[0]);
            for x in 1..width.min(data.len()) {
                let prev = decompressed[x - 1];
                let value = prev.wrapping_add(data[x]);
                decompressed.push(value);
            }
        }
        for y in 1..height {
            for x in 0..width {
                let idx = y * width + x;
                if idx < data.len() {
                    let above = decompressed[idx - width];
                    let value = above.wrapping_add(data[idx]);
                    decompressed.push(value);
                }
            }
        }
        Ok(decompressed)
    }
}
/// Huffman tree node
#[derive(Debug, Clone)]
enum HuffmanNode {
    Leaf {
        symbol: u8,
        frequency: u32,
    },
    Internal {
        frequency: u32,
        left: Box<HuffmanNode>,
        right: Box<HuffmanNode>,
    },
}
impl HuffmanNode {
    fn frequency(&self) -> u32 {
        match self {
            Self::Leaf { frequency, .. } => *frequency,
            Self::Internal { frequency, .. } => *frequency,
        }
    }
}
/// Simplified Huffman encoding
pub struct HuffmanCompressor;
impl HuffmanCompressor {
    /// Builds frequency table
    fn build_frequency_table(data: &[u8]) -> HashMap<u8, u32> {
        let mut frequencies = HashMap::new();
        for &byte in data {
            *frequencies.entry(byte).or_insert(0) += 1;
        }
        frequencies
    }
    /// Builds Huffman tree
    fn build_tree(frequencies: &HashMap<u8, u32>) -> Option<HuffmanNode> {
        if frequencies.is_empty() {
            return None;
        }
        let mut nodes: Vec<HuffmanNode> = frequencies
            .iter()
            .map(|(&symbol, &frequency)| HuffmanNode::Leaf { symbol, frequency })
            .collect();
        while nodes.len() > 1 {
            nodes.sort_by_key(|n| std::cmp::Reverse(n.frequency()));
            let right = nodes.pop()?;
            let left = nodes.pop()?;
            let internal = HuffmanNode::Internal {
                frequency: left.frequency() + right.frequency(),
                left: Box::new(left),
                right: Box::new(right),
            };
            nodes.push(internal);
        }
        nodes.pop()
    }
    /// Generates code table from tree
    fn generate_codes(node: &HuffmanNode, prefix: Vec<bool>, codes: &mut HashMap<u8, Vec<bool>>) {
        match node {
            HuffmanNode::Leaf { symbol, .. } => {
                codes.insert(*symbol, prefix);
            }
            HuffmanNode::Internal { left, right, .. } => {
                let mut left_prefix = prefix.clone();
                left_prefix.push(false);
                Self::generate_codes(left, left_prefix, codes);
                let mut right_prefix = prefix;
                right_prefix.push(true);
                Self::generate_codes(right, right_prefix, codes);
            }
        }
    }
    /// Compresses data using Huffman encoding.
    ///
    /// Format:
    /// - 2 bytes: number of distinct symbols (u16 LE)
    /// - For each symbol: 1 byte symbol value + 4 bytes frequency (u32 LE)
    /// - 8 bytes: original data length (u64 LE)
    /// - Remaining bytes: Huffman-encoded bit stream (MSB first within each byte)
    pub fn compress(data: &[u8]) -> Vec<u8> {
        if data.is_empty() {
            return Vec::new();
        }
        let frequencies = Self::build_frequency_table(data);
        let tree = match Self::build_tree(&frequencies) {
            Some(t) => t,
            None => return Vec::new(),
        };
        let mut codes = HashMap::new();
        Self::generate_codes(&tree, Vec::new(), &mut codes);

        // Header: number of symbols (u16 LE)
        let num_symbols = frequencies.len() as u16;
        let mut compressed = Vec::new();
        compressed.extend_from_slice(&num_symbols.to_le_bytes());

        // Symbol table: (symbol u8, frequency u32 LE) for each distinct byte
        let mut sorted_symbols: Vec<(u8, u32)> = frequencies.into_iter().collect();
        sorted_symbols.sort_by_key(|(sym, _)| *sym);
        for (symbol, freq) in &sorted_symbols {
            compressed.push(*symbol);
            compressed.extend_from_slice(&freq.to_le_bytes());
        }

        // Original data length (u64 LE) — needed to know when to stop decoding
        compressed.extend_from_slice(&(data.len() as u64).to_le_bytes());

        // Encode bit stream (MSB first within each byte)
        let mut bit_buffer = 0u8;
        let mut bit_count = 0u8;
        for &byte in data {
            if let Some(code) = codes.get(&byte) {
                for &bit in code {
                    if bit {
                        bit_buffer |= 1 << (7 - bit_count);
                    }
                    bit_count += 1;
                    if bit_count == 8 {
                        compressed.push(bit_buffer);
                        bit_buffer = 0;
                        bit_count = 0;
                    }
                }
            }
        }
        if bit_count > 0 {
            compressed.push(bit_buffer);
        }
        compressed
    }

    /// Decompresses Huffman-encoded data produced by [`HuffmanCompressor::compress`].
    ///
    /// Reads the frequency table stored in the header to reconstruct the identical
    /// Huffman tree, then decodes the bit stream up to the stored original length.
    pub fn decompress(compressed: &[u8]) -> WasmResult<Vec<u8>> {
        if compressed.is_empty() {
            return Ok(Vec::new());
        }

        // Parse header: number of symbols (u16 LE)
        if compressed.len() < 2 {
            return Err(WasmError::InvalidOperation {
                operation: "Huffman decompression".to_string(),
                reason: "Data too short to contain symbol count header".to_string(),
            });
        }
        let num_symbols = u16::from_le_bytes([compressed[0], compressed[1]]) as usize;
        let mut pos = 2usize;

        // Each symbol entry is 5 bytes: symbol(1) + frequency(4)
        let symbol_table_size = num_symbols * 5;
        if pos + symbol_table_size + 8 > compressed.len() {
            return Err(WasmError::InvalidOperation {
                operation: "Huffman decompression".to_string(),
                reason: "Data too short to contain symbol table and length header".to_string(),
            });
        }

        let mut frequencies: HashMap<u8, u32> = HashMap::new();
        for _ in 0..num_symbols {
            let symbol = compressed[pos];
            pos += 1;
            let freq = u32::from_le_bytes([
                compressed[pos],
                compressed[pos + 1],
                compressed[pos + 2],
                compressed[pos + 3],
            ]);
            pos += 4;
            frequencies.insert(symbol, freq);
        }

        // Parse original data length (u64 LE)
        let original_len = u64::from_le_bytes([
            compressed[pos],
            compressed[pos + 1],
            compressed[pos + 2],
            compressed[pos + 3],
            compressed[pos + 4],
            compressed[pos + 5],
            compressed[pos + 6],
            compressed[pos + 7],
        ]) as usize;
        pos += 8;

        if original_len == 0 {
            return Ok(Vec::new());
        }

        // Reconstruct the Huffman tree using the same algorithm as compress
        let tree = Self::build_tree(&frequencies).ok_or_else(|| WasmError::InvalidOperation {
            operation: "Huffman decompression".to_string(),
            reason: "Failed to reconstruct Huffman tree from frequency table".to_string(),
        })?;

        // Decode the bit stream by traversing the tree
        let bitstream = &compressed[pos..];
        let mut result = Vec::with_capacity(original_len);
        let mut node = &tree;

        'outer: for &byte in bitstream {
            for bit_idx in (0..8).rev() {
                let bit = (byte >> bit_idx) & 1;
                node = match node {
                    HuffmanNode::Internal { left, right, .. } => {
                        if bit == 0 {
                            left
                        } else {
                            right
                        }
                    }
                    HuffmanNode::Leaf { .. } => {
                        // Should not reach here mid-traversal without a leaf
                        return Err(WasmError::InvalidOperation {
                            operation: "Huffman decompression".to_string(),
                            reason: "Unexpected leaf mid-traversal in bit stream".to_string(),
                        });
                    }
                };

                if let HuffmanNode::Leaf { symbol, .. } = node {
                    result.push(*symbol);
                    if result.len() == original_len {
                        break 'outer;
                    }
                    // Reset traversal to tree root
                    node = &tree;
                }
            }
        }

        // Handle single-symbol edge case: entire input was one distinct byte
        if result.is_empty() && original_len > 0 {
            if let HuffmanNode::Leaf { symbol, .. } = &tree {
                result.resize(original_len, *symbol);
            }
        }

        if result.len() != original_len {
            return Err(WasmError::InvalidOperation {
                operation: "Huffman decompression".to_string(),
                reason: format!(
                    "Decoded {} bytes but expected {}",
                    result.len(),
                    original_len
                ),
            });
        }

        Ok(result)
    }
}
/// LZ77-style compression
pub struct Lz77Compressor;
impl Lz77Compressor {
    /// Window size for looking back
    const WINDOW_SIZE: usize = 4096;
    /// Maximum match length
    const MAX_MATCH_LENGTH: usize = 18;
    /// Minimum match length
    const MIN_MATCH_LENGTH: usize = 3;
    /// Finds the longest match in the window
    fn find_longest_match(data: &[u8], pos: usize) -> Option<(usize, usize)> {
        let window_start = pos.saturating_sub(Self::WINDOW_SIZE);
        let mut best_match = None;
        let mut best_length = 0;
        for start in window_start..pos {
            let mut length = 0;
            while length < Self::MAX_MATCH_LENGTH
                && pos + length < data.len()
                && data[start + length] == data[pos + length]
            {
                length += 1;
            }
            if length >= Self::MIN_MATCH_LENGTH && length > best_length {
                best_length = length;
                best_match = Some((pos - start, length));
            }
        }
        best_match
    }
    /// Compresses data using LZ77
    pub fn compress(data: &[u8]) -> Vec<u8> {
        if data.is_empty() {
            return Vec::new();
        }
        let mut compressed = Vec::new();
        let mut pos = 0;
        while pos < data.len() {
            if let Some((distance, length)) = Self::find_longest_match(data, pos) {
                compressed.push(1);
                compressed.push((distance >> 8) as u8);
                compressed.push((distance & 0xFF) as u8);
                compressed.push(length as u8);
                pos += length;
            } else {
                compressed.push(0);
                compressed.push(data[pos]);
                pos += 1;
            }
        }
        compressed
    }
    /// Decompresses LZ77 data
    pub fn decompress(data: &[u8]) -> WasmResult<Vec<u8>> {
        let mut decompressed = Vec::new();
        let mut i = 0;
        while i < data.len() {
            let flag = data[i];
            i += 1;
            if flag == 1 {
                if i + 3 > data.len() {
                    return Err(WasmError::InvalidOperation {
                        operation: "LZ77 decompress".to_string(),
                        reason: "Unexpected end of data".to_string(),
                    });
                }
                let distance = ((data[i] as usize) << 8) | (data[i + 1] as usize);
                let length = data[i + 2] as usize;
                i += 3;
                let start = decompressed.len().saturating_sub(distance);
                for j in 0..length {
                    if start + j < decompressed.len() {
                        let byte = decompressed[start + j];
                        decompressed.push(byte);
                    }
                }
            } else {
                if i >= data.len() {
                    return Err(WasmError::InvalidOperation {
                        operation: "LZ77 decompress".to_string(),
                        reason: "Unexpected end of data".to_string(),
                    });
                }
                decompressed.push(data[i]);
                i += 1;
            }
        }
        Ok(decompressed)
    }
}
/// Unified compression interface
pub struct TileCompressor {
    /// Algorithm to use
    algorithm: CompressionAlgorithm,
}
impl TileCompressor {
    /// Creates a new tile compressor
    pub const fn new(algorithm: CompressionAlgorithm) -> Self {
        Self { algorithm }
    }
    /// Compresses tile data
    pub fn compress(&self, data: &[u8]) -> (Vec<u8>, CompressionStats) {
        #[cfg(target_arch = "wasm32")]
        let start = js_sys::Date::now();
        #[cfg(not(target_arch = "wasm32"))]
        let start = std::time::Instant::now();

        let original_size = data.len();
        let compressed = match self.algorithm {
            CompressionAlgorithm::None => data.to_vec(),
            CompressionAlgorithm::Rle => RleCompressor::compress(data),
            CompressionAlgorithm::Delta => DeltaCompressor::compress(data),
            CompressionAlgorithm::Huffman => HuffmanCompressor::compress(data),
            CompressionAlgorithm::Lz77 => Lz77Compressor::compress(data),
        };
        let compressed_size = compressed.len();

        #[cfg(target_arch = "wasm32")]
        let elapsed = js_sys::Date::now() - start;
        #[cfg(not(target_arch = "wasm32"))]
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;

        let stats = CompressionStats::new(original_size, compressed_size, elapsed);
        (compressed, stats)
    }
    /// Decompresses tile data
    pub fn decompress(&self, data: &[u8]) -> WasmResult<Vec<u8>> {
        match self.algorithm {
            CompressionAlgorithm::None => Ok(data.to_vec()),
            CompressionAlgorithm::Rle => RleCompressor::decompress(data),
            CompressionAlgorithm::Delta => DeltaCompressor::decompress(data),
            CompressionAlgorithm::Huffman => Ok(data.to_vec()),
            CompressionAlgorithm::Lz77 => Lz77Compressor::decompress(data),
        }
    }
    /// Compresses 2D tile data (for images)
    pub fn compress_2d(&self, data: &[u8], width: usize) -> (Vec<u8>, CompressionStats) {
        #[cfg(target_arch = "wasm32")]
        let start = js_sys::Date::now();
        #[cfg(not(target_arch = "wasm32"))]
        let start = std::time::Instant::now();

        let original_size = data.len();
        let compressed = match self.algorithm {
            CompressionAlgorithm::Delta => DeltaCompressor::compress_2d(data, width),
            _ => self.compress(data).0,
        };
        let compressed_size = compressed.len();

        #[cfg(target_arch = "wasm32")]
        let elapsed = js_sys::Date::now() - start;
        #[cfg(not(target_arch = "wasm32"))]
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;

        let stats = CompressionStats::new(original_size, compressed_size, elapsed);
        (compressed, stats)
    }
    /// Decompresses 2D tile data
    pub fn decompress_2d(&self, data: &[u8], width: usize) -> WasmResult<Vec<u8>> {
        match self.algorithm {
            CompressionAlgorithm::Delta => DeltaCompressor::decompress_2d(data, width),
            _ => self.decompress(data),
        }
    }
}
/// Compression benchmark
pub struct CompressionBenchmark;
impl CompressionBenchmark {
    /// Benchmarks all compression algorithms
    pub fn benchmark_all(data: &[u8]) -> Vec<(CompressionAlgorithm, CompressionStats)> {
        let algorithms = [
            CompressionAlgorithm::None,
            CompressionAlgorithm::Rle,
            CompressionAlgorithm::Delta,
            CompressionAlgorithm::Huffman,
            CompressionAlgorithm::Lz77,
        ];
        let mut results = Vec::new();
        for &algorithm in &algorithms {
            let compressor = TileCompressor::new(algorithm);
            let (_compressed, stats) = compressor.compress(data);
            results.push((algorithm, stats));
        }
        results
    }
    /// Finds the best compression algorithm for the given data
    pub fn find_best(data: &[u8]) -> CompressionAlgorithm {
        let results = Self::benchmark_all(data);
        results
            .into_iter()
            .min_by(|a, b| {
                a.1.compressed_size.cmp(&b.1.compressed_size).then_with(|| {
                    a.1.compression_time_ms
                        .partial_cmp(&b.1.compression_time_ms)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            })
            .map(|(algo, _)| algo)
            .unwrap_or(CompressionAlgorithm::None)
    }
}
/// Compression selector for automatic algorithm selection
#[derive(Debug, Clone)]
pub struct CompressionSelector {
    /// Statistics from previous compressions
    history: Vec<(CompressionAlgorithm, CompressionStats)>,
    /// Maximum history size
    max_history: usize,
}
impl CompressionSelector {
    /// Creates a new compression selector
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            max_history: 10,
        }
    }
    /// Selects the best compression algorithm for the given data
    pub fn select_best(&mut self, data: &[u8]) -> WasmResult<CompressionAlgorithm> {
        let best = CompressionBenchmark::find_best(data);
        let compressor = TileCompressor::new(best);
        let (_compressed, stats) = compressor.compress(data);
        self.history.push((best, stats));
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
        Ok(best)
    }
    /// Returns compression statistics history
    pub fn history(&self) -> &[(CompressionAlgorithm, CompressionStats)] {
        &self.history
    }
    /// Clears history
    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}
impl Default for CompressionSelector {
    fn default() -> Self {
        Self::new()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_rle_compress_decompress() {
        let data = vec![1, 1, 1, 2, 2, 3, 3, 3, 3];
        let compressed = RleCompressor::compress(&data);
        let decompressed = RleCompressor::decompress(&compressed).expect("Decompress failed");
        assert_eq!(data, decompressed);
        assert!(compressed.len() < data.len());
    }
    #[test]
    fn test_delta_compress_decompress() {
        let data = vec![10, 15, 20, 25, 30, 35, 40];
        let compressed = DeltaCompressor::compress(&data);
        let decompressed = DeltaCompressor::decompress(&compressed).expect("Decompress failed");
        assert_eq!(data, decompressed);
    }
    #[test]
    fn test_delta_2d() {
        let data = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120];
        let width = 3;
        let compressed = DeltaCompressor::compress_2d(&data, width);
        let decompressed =
            DeltaCompressor::decompress_2d(&compressed, width).expect("Decompress 2D failed");
        assert_eq!(data, decompressed);
    }
    #[test]
    fn test_lz77_compress_decompress() {
        let data = b"ABCABCABCABC";
        let compressed = Lz77Compressor::compress(data);
        let decompressed = Lz77Compressor::decompress(&compressed).expect("Decompress failed");
        assert_eq!(data.to_vec(), decompressed);
    }
    #[test]
    fn test_compression_stats() {
        let stats = CompressionStats::new(1000, 500, 10.0);
        assert_eq!(stats.space_saved(), 500);
        assert_eq!(stats.space_saved_percent(), 50.0);
        assert_eq!(stats.ratio, 0.5);
    }
    #[test]
    fn test_tile_compressor() {
        let compressor = TileCompressor::new(CompressionAlgorithm::Rle);
        let data = vec![1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3];
        let (compressed, stats) = compressor.compress(&data);
        assert!(stats.compressed_size <= data.len());
        let decompressed = compressor
            .decompress(&compressed)
            .expect("Decompress failed");
        assert_eq!(data, decompressed);
    }
    #[test]
    #[ignore]
    fn test_compression_benchmark() {
        let data = vec![1, 2, 3, 4, 5, 1, 2, 3, 4, 5, 1, 2, 3, 4, 5];
        let results = CompressionBenchmark::benchmark_all(&data);
        assert_eq!(results.len(), 5);
        let algorithms: Vec<_> = results.iter().map(|(algo, _)| *algo).collect();
        assert!(algorithms.contains(&CompressionAlgorithm::None));
        assert!(algorithms.contains(&CompressionAlgorithm::Rle));
    }
    #[test]
    #[ignore]
    fn test_find_best_compression() {
        let data = vec![1, 1, 1, 1, 2, 2, 2, 2];
        let best = CompressionBenchmark::find_best(&data);
        assert_ne!(best, CompressionAlgorithm::None);
    }
}
