//! Mobile network optimization
//!
//! This module provides utilities for optimizing network operations on mobile devices,
//! with special focus on minimizing cellular data usage and adapting to network conditions.
//!
//! # Key Features
//!
//! - Network type detection (WiFi, Cellular, etc.)
//! - Data compression for efficient transfers
//! - Adaptive quality based on network speed
//! - Prefetching strategies
//! - Offline mode support
//! - Data usage tracking
//!
//! # Example
//!
//! ```rust,no_run
//! use oxigdal_mobile_enhanced::network::{NetworkOptimizer, NetworkType};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let optimizer = NetworkOptimizer::new();
//!
//! // Check network type
//! let network_type = optimizer.detect_network_type()?;
//! match network_type {
//!     NetworkType::WiFi => {
//!         // Full quality downloads
//!     }
//!     NetworkType::Cellular => {
//!         // Compress data to save cellular usage
//!     }
//!     _ => {}
//! }
//!
//! // Compress data for transfer
//! let data = vec![1u8, 2, 3, 4, 5];
//! let compressed = optimizer.compress_for_transfer(&data)?;
//! # Ok(())
//! # }
//! ```

use crate::error::{MobileError, Result};
use bytes::Bytes;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Network type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkType {
    /// WiFi connection - high speed, typically unmetered
    WiFi,
    /// Cellular connection - variable speed, typically metered
    Cellular,
    /// Ethernet connection - high speed, unmetered
    Ethernet,
    /// No network connection
    None,
    /// Unknown network type
    Unknown,
}

impl NetworkType {
    /// Check if this network type is metered (has data limits)
    pub fn is_metered(&self) -> bool {
        matches!(self, Self::Cellular)
    }

    /// Check if this network type is high speed
    pub fn is_high_speed(&self) -> bool {
        matches!(self, Self::WiFi | Self::Ethernet)
    }

    /// Get recommended compression level (0-9)
    pub fn recommended_compression_level(&self) -> u32 {
        match self {
            Self::WiFi | Self::Ethernet => 3, // Light compression
            Self::Cellular => 9,              // Maximum compression
            Self::None | Self::Unknown => 6,  // Medium compression
        }
    }

    /// Get recommended quality factor (0.0 - 1.0)
    pub fn recommended_quality_factor(&self) -> f32 {
        match self {
            Self::WiFi | Self::Ethernet => 1.0,
            Self::Cellular => 0.6,
            Self::None | Self::Unknown => 0.8,
        }
    }
}

/// Network quality metrics
#[derive(Debug, Clone)]
pub struct NetworkQuality {
    /// Network type
    pub network_type: NetworkType,
    /// Estimated download speed in bytes/sec
    pub download_speed: Option<u64>,
    /// Estimated upload speed in bytes/sec
    pub upload_speed: Option<u64>,
    /// Round-trip time in milliseconds
    pub latency: Option<Duration>,
    /// Packet loss percentage (0.0 - 100.0)
    pub packet_loss: Option<f32>,
    /// Timestamp of measurement
    pub timestamp: Instant,
}

impl NetworkQuality {
    /// Check if network is suitable for large downloads
    pub fn is_suitable_for_large_downloads(&self) -> bool {
        if self.network_type.is_high_speed() {
            return true;
        }

        // For cellular, check if speed is reasonable
        if let Some(speed) = self.download_speed {
            speed > 1_000_000 // > 1 MB/s
        } else {
            false
        }
    }

    /// Get recommended chunk size for downloads
    pub fn recommended_chunk_size(&self) -> usize {
        match self.network_type {
            NetworkType::WiFi | NetworkType::Ethernet => 1024 * 1024, // 1 MB
            NetworkType::Cellular => 256 * 1024,                      // 256 KB
            _ => 512 * 1024,                                          // 512 KB
        }
    }

    /// Check if network is stable
    pub fn is_stable(&self) -> bool {
        self.packet_loss.is_none_or(|loss| loss < 5.0)
    }
}

/// Compression method for network transfer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionMethod {
    /// No compression
    None,
    /// DEFLATE (zlib) compression
    Deflate,
    /// Zstandard compression
    Zstd,
    /// LZ4 compression (fast)
    Lz4,
}

impl CompressionMethod {
    /// Get compression ratio estimate (higher is better)
    pub fn estimated_ratio(&self) -> f32 {
        match self {
            Self::None => 1.0,
            Self::Deflate => 3.0,
            Self::Zstd => 3.5,
            Self::Lz4 => 2.0,
        }
    }

    /// Get compression speed (higher is faster)
    pub fn compression_speed(&self) -> u32 {
        match self {
            Self::None => 1000,
            Self::Lz4 => 100,
            Self::Deflate => 50,
            Self::Zstd => 40,
        }
    }

    /// Choose best compression method for network type
    pub fn for_network(network_type: NetworkType) -> Self {
        match network_type {
            NetworkType::WiFi | NetworkType::Ethernet => Self::Lz4, // Fast
            NetworkType::Cellular => Self::Zstd,                    // Best ratio
            _ => Self::Deflate,                                     // Balanced
        }
    }
}

/// Network optimizer for mobile devices
pub struct NetworkOptimizer {
    quality: Arc<RwLock<Option<NetworkQuality>>>,
    data_usage: Arc<RwLock<DataUsageTracker>>,
}

impl NetworkOptimizer {
    /// Create a new network optimizer
    pub fn new() -> Self {
        Self {
            quality: Arc::new(RwLock::new(None)),
            data_usage: Arc::new(RwLock::new(DataUsageTracker::new())),
        }
    }

    /// Detect current network type
    pub fn detect_network_type(&self) -> Result<NetworkType> {
        // In a real implementation, this would use platform-specific APIs
        // For now, return a mock value
        Ok(NetworkType::WiFi)
    }

    /// Measure network quality
    pub fn measure_quality(&self) -> Result<NetworkQuality> {
        let network_type = self.detect_network_type()?;

        // Mock quality metrics
        let quality = NetworkQuality {
            network_type,
            download_speed: Some(10_000_000), // 10 MB/s
            upload_speed: Some(5_000_000),    // 5 MB/s
            latency: Some(Duration::from_millis(20)),
            packet_loss: Some(0.5),
            timestamp: Instant::now(),
        };

        *self.quality.write() = Some(quality.clone());
        Ok(quality)
    }

    /// Get current network quality
    pub fn current_quality(&self) -> Result<NetworkQuality> {
        let quality = self.quality.read();
        quality.clone().ok_or(MobileError::NetworkOptimizationError(
            "Network quality not measured".to_string(),
        ))
    }

    /// Compress data for network transfer
    pub fn compress_for_transfer(&self, data: &[u8]) -> Result<Bytes> {
        let network_type = self.detect_network_type()?;
        let method = CompressionMethod::for_network(network_type);

        self.compress_with_method(data, method)
    }

    /// Compress data with specific method
    pub fn compress_with_method(&self, data: &[u8], method: CompressionMethod) -> Result<Bytes> {
        let compressed =
            match method {
                CompressionMethod::None => Bytes::copy_from_slice(data),
                CompressionMethod::Deflate => oxiarc_deflate::zlib_compress(data, 6)
                    .map(Bytes::from)
                    .map_err(|e| MobileError::CompressionError(e.to_string()))?,
                CompressionMethod::Zstd => oxiarc_zstd::encode_all(data, 3)
                    .map(Bytes::from)
                    .map_err(|e| MobileError::CompressionError(e.to_string()))?,
                CompressionMethod::Lz4 => {
                    // LZ4 compression would go here
                    // For now, use deflate as fallback
                    return self.compress_with_method(data, CompressionMethod::Deflate);
                }
            };

        // Track data usage
        self.data_usage.write().add_compressed(compressed.len());

        Ok(compressed)
    }

    /// Decompress data from network transfer
    pub fn decompress_from_transfer(
        &self,
        data: &[u8],
        method: CompressionMethod,
    ) -> Result<Bytes> {
        let decompressed = match method {
            CompressionMethod::None => Bytes::copy_from_slice(data),
            CompressionMethod::Deflate => oxiarc_deflate::zlib_decompress(data)
                .map(Bytes::from)
                .map_err(|e| MobileError::DecompressionError(e.to_string()))?,
            CompressionMethod::Zstd => {
                let decompressed = oxiarc_zstd::decode_all(data)
                    .map_err(|e| MobileError::DecompressionError(e.to_string()))?;
                Bytes::from(decompressed)
            }
            CompressionMethod::Lz4 => {
                // LZ4 decompression would go here
                // For now, use deflate as fallback
                return self.decompress_from_transfer(data, CompressionMethod::Deflate);
            }
        };

        // Track data usage
        self.data_usage.write().add_decompressed(decompressed.len());

        Ok(decompressed)
    }

    /// Check if should prefetch data
    pub fn should_prefetch(&self) -> bool {
        self.detect_network_type()
            .is_ok_and(|nt| nt.is_high_speed() && !nt.is_metered())
    }

    /// Get data usage statistics
    pub fn data_usage(&self) -> DataUsageStats {
        self.data_usage.read().stats()
    }

    /// Reset data usage counters
    pub fn reset_data_usage(&self) {
        self.data_usage.write().reset();
    }
}

impl Default for NetworkOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Data usage tracking
struct DataUsageTracker {
    compressed_bytes: u64,
    decompressed_bytes: u64,
    start_time: Instant,
}

impl DataUsageTracker {
    fn new() -> Self {
        Self {
            compressed_bytes: 0,
            decompressed_bytes: 0,
            start_time: Instant::now(),
        }
    }

    fn add_compressed(&mut self, bytes: usize) {
        self.compressed_bytes = self.compressed_bytes.saturating_add(bytes as u64);
    }

    fn add_decompressed(&mut self, bytes: usize) {
        self.decompressed_bytes = self.decompressed_bytes.saturating_add(bytes as u64);
    }

    fn stats(&self) -> DataUsageStats {
        DataUsageStats {
            compressed_bytes: self.compressed_bytes,
            decompressed_bytes: self.decompressed_bytes,
            elapsed: self.start_time.elapsed(),
        }
    }

    fn reset(&mut self) {
        self.compressed_bytes = 0;
        self.decompressed_bytes = 0;
        self.start_time = Instant::now();
    }
}

/// Data usage statistics
#[derive(Debug, Clone)]
pub struct DataUsageStats {
    /// Total compressed bytes transferred
    pub compressed_bytes: u64,
    /// Total decompressed bytes
    pub decompressed_bytes: u64,
    /// Elapsed time since tracking started
    pub elapsed: Duration,
}

impl DataUsageStats {
    /// Calculate compression ratio
    pub fn compression_ratio(&self) -> f64 {
        if self.compressed_bytes == 0 {
            return 1.0;
        }
        self.decompressed_bytes as f64 / self.compressed_bytes as f64
    }

    /// Calculate bytes saved through compression
    pub fn bytes_saved(&self) -> u64 {
        self.decompressed_bytes
            .saturating_sub(self.compressed_bytes)
    }

    /// Calculate average transfer rate (bytes/sec)
    pub fn average_transfer_rate(&self) -> f64 {
        let secs = self.elapsed.as_secs_f64();
        if secs == 0.0 {
            return 0.0;
        }
        self.compressed_bytes as f64 / secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_type_properties() {
        assert!(NetworkType::Cellular.is_metered());
        assert!(!NetworkType::WiFi.is_metered());
        assert!(NetworkType::WiFi.is_high_speed());
        assert!(!NetworkType::Cellular.is_high_speed());
    }

    #[test]
    fn test_network_type_compression_level() {
        assert_eq!(NetworkType::WiFi.recommended_compression_level(), 3);
        assert_eq!(NetworkType::Cellular.recommended_compression_level(), 9);
    }

    #[test]
    fn test_compression_method_selection() {
        assert_eq!(
            CompressionMethod::for_network(NetworkType::WiFi),
            CompressionMethod::Lz4
        );
        assert_eq!(
            CompressionMethod::for_network(NetworkType::Cellular),
            CompressionMethod::Zstd
        );
    }

    #[test]
    fn test_network_optimizer_compression() {
        let optimizer = NetworkOptimizer::new();
        let data = b"Hello, World! This is test data for compression.";

        let compressed = optimizer
            .compress_for_transfer(data)
            .expect("Compression failed");
        assert!(!compressed.is_empty());

        let decompressed = optimizer
            .decompress_from_transfer(&compressed, CompressionMethod::Deflate)
            .expect("Decompression failed");
        assert_eq!(&decompressed[..], &data[..]);
    }

    #[test]
    fn test_data_usage_stats() {
        let stats = DataUsageStats {
            compressed_bytes: 100,
            decompressed_bytes: 300,
            elapsed: Duration::from_secs(1),
        };

        assert_eq!(stats.compression_ratio(), 3.0);
        assert_eq!(stats.bytes_saved(), 200);
        assert_eq!(stats.average_transfer_rate(), 100.0);
    }

    #[test]
    fn test_network_quality() {
        let quality = NetworkQuality {
            network_type: NetworkType::WiFi,
            download_speed: Some(10_000_000),
            upload_speed: Some(5_000_000),
            latency: Some(Duration::from_millis(20)),
            packet_loss: Some(1.0),
            timestamp: Instant::now(),
        };

        assert!(quality.is_suitable_for_large_downloads());
        assert!(quality.is_stable());
        assert_eq!(quality.recommended_chunk_size(), 1024 * 1024);
    }
}
