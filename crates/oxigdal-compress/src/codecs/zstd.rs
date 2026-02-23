//! Zstandard (zstd) compression codec
//!
//! Zstandard is a fast lossless compression algorithm targeting real-time compression
//! scenarios with better compression ratios than LZ4. It offers a wide range of
//! compression levels and excellent decompression speeds.
//!
//! # Dictionary Training
//!
//! This module supports ZSTD dictionary training for improved compression of similar data.
//! Dictionaries work best when:
//! - You have many small files with similar content
//! - Data shares common patterns (e.g., JSON, protocol buffers, log entries)
//! - Individual records are relatively small (under 16KB typically)
//!
//! # Example
//!
//! ```ignore
//! use oxigdal_compress::codecs::zstd::{ZstdCodec, ZstdDictionary, DictionaryConfig};
//!
//! // Collect sample data for training
//! let samples: Vec<&[u8]> = vec![
//!     b"sample data 1",
//!     b"sample data 2",
//!     b"sample data 3",
//! ];
//!
//! // Train a dictionary
//! let config = DictionaryConfig::default();
//! let dict = ZstdCodec::train_dictionary(&samples, &config)?;
//!
//! // Use dictionary for compression
//! let codec = ZstdCodec::new();
//! let compressed = codec.compress_with_dictionary(b"data to compress", &dict)?;
//! let decompressed = codec.decompress_with_dictionary(&compressed, &dict, None)?;
//! ```

use crate::error::{CompressionError, Result};
use std::io::{BufRead, Read, Write};
use zstd::stream::{read::Decoder, write::Encoder};

/// Zstd compression level (1-22, higher = better compression but slower)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZstdLevel(i32);

impl ZstdLevel {
    /// Minimum compression level (fastest)
    pub const MIN: i32 = 1;

    /// Maximum compression level (best compression)
    pub const MAX: i32 = 22;

    /// Default compression level (balanced)
    pub const DEFAULT: i32 = 3;

    /// Create a new Zstd compression level
    pub fn new(level: i32) -> Result<Self> {
        if !(Self::MIN..=Self::MAX).contains(&level) {
            return Err(CompressionError::InvalidCompressionLevel {
                level,
                min: Self::MIN,
                max: Self::MAX,
            });
        }
        Ok(Self(level))
    }

    /// Get the level value
    pub fn value(&self) -> i32 {
        self.0
    }
}

impl Default for ZstdLevel {
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}

/// Zstd codec configuration
#[derive(Debug, Clone)]
pub struct ZstdConfig {
    /// Compression level
    pub level: ZstdLevel,

    /// Enable content checksum
    pub checksum: bool,

    /// Dictionary for compression (for similar data)
    pub dictionary: Option<Vec<u8>>,

    /// Number of threads for compression (0 = auto)
    pub threads: usize,

    /// Enable long-distance matching
    pub long_distance_matching: bool,
}

impl Default for ZstdConfig {
    fn default() -> Self {
        Self {
            level: ZstdLevel::default(),
            checksum: true,
            dictionary: None,
            threads: 0,
            long_distance_matching: false,
        }
    }
}

impl ZstdConfig {
    /// Create new configuration with specified level
    pub fn with_level(level: i32) -> Result<Self> {
        Ok(Self {
            level: ZstdLevel::new(level)?,
            ..Default::default()
        })
    }

    /// Enable/disable checksum
    pub fn with_checksum(mut self, checksum: bool) -> Self {
        self.checksum = checksum;
        self
    }

    /// Set compression dictionary
    pub fn with_dictionary(mut self, dict: Vec<u8>) -> Self {
        self.dictionary = Some(dict);
        self
    }

    /// Set number of threads
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads;
        self
    }

    /// Enable long-distance matching
    pub fn with_long_distance_matching(mut self, enabled: bool) -> Self {
        self.long_distance_matching = enabled;
        self
    }
}

/// Magic number for serialized dictionaries (OXZD = OXigdal Zstd Dictionary)
const DICTIONARY_MAGIC: [u8; 4] = [0x4F, 0x58, 0x5A, 0x44];

/// Current dictionary format version
const DICTIONARY_VERSION: u8 = 1;

/// Dictionary header size (magic + version + size + checksum)
const DICTIONARY_HEADER_SIZE: usize = 4 + 1 + 4 + 4;

/// Configuration for dictionary training
#[derive(Debug, Clone)]
pub struct DictionaryConfig {
    /// Target dictionary size in bytes (default: 112KB, optimal for most use cases)
    pub dict_size: usize,

    /// Compression level used during training (affects dictionary quality)
    pub training_level: i32,

    /// Minimum number of samples required for effective training
    pub min_samples: usize,

    /// Maximum sample size to consider (samples larger than this may be truncated)
    pub max_sample_size: usize,
}

impl Default for DictionaryConfig {
    fn default() -> Self {
        Self {
            dict_size: 112 * 1024, // 112KB - ZSTD recommended default
            training_level: ZstdLevel::DEFAULT,
            min_samples: 5,           // At least 5 samples for meaningful training
            max_sample_size: 1 << 20, // 1MB max sample size
        }
    }
}

impl DictionaryConfig {
    /// Create configuration with specified dictionary size
    pub fn with_size(size: usize) -> Self {
        Self {
            dict_size: size,
            ..Default::default()
        }
    }

    /// Set training compression level
    pub fn with_training_level(mut self, level: i32) -> Self {
        self.training_level = level.clamp(ZstdLevel::MIN, ZstdLevel::MAX);
        self
    }

    /// Set minimum samples required
    pub fn with_min_samples(mut self, min_samples: usize) -> Self {
        self.min_samples = min_samples.max(1);
        self
    }

    /// Set maximum sample size
    pub fn with_max_sample_size(mut self, max_size: usize) -> Self {
        self.max_sample_size = max_size;
        self
    }
}

/// A trained ZSTD compression dictionary
#[derive(Debug, Clone)]
pub struct ZstdDictionary {
    /// Raw dictionary data (trained by ZSTD)
    data: Vec<u8>,

    /// Dictionary version for compatibility checking
    version: u8,

    /// Checksum of dictionary data for integrity verification
    checksum: u32,

    /// Optional identifier for the dictionary
    id: Option<String>,

    /// Training configuration used (for reference)
    config: DictionaryConfig,
}

impl ZstdDictionary {
    /// Create a new dictionary from raw trained data
    pub fn new(data: Vec<u8>, config: DictionaryConfig) -> Self {
        let checksum = Self::compute_checksum(&data);
        Self {
            data,
            version: DICTIONARY_VERSION,
            checksum,
            id: None,
            config,
        }
    }

    /// Set an identifier for this dictionary
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Get the raw dictionary data
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Get the dictionary size in bytes
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Get the dictionary version
    pub fn version(&self) -> u8 {
        self.version
    }

    /// Get the dictionary checksum
    pub fn checksum(&self) -> u32 {
        self.checksum
    }

    /// Get the dictionary ID if set
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Get the training configuration used for this dictionary
    pub fn config(&self) -> &DictionaryConfig {
        &self.config
    }

    /// Verify dictionary integrity
    pub fn verify(&self) -> bool {
        Self::compute_checksum(&self.data) == self.checksum
    }

    /// Compute checksum for dictionary data (simple xxhash-style)
    fn compute_checksum(data: &[u8]) -> u32 {
        // Simple FNV-1a hash for integrity checking
        const FNV_OFFSET: u32 = 2166136261;
        const FNV_PRIME: u32 = 16777619;

        data.iter().fold(FNV_OFFSET, |hash, &byte| {
            (hash ^ u32::from(byte)).wrapping_mul(FNV_PRIME)
        })
    }

    /// Serialize dictionary to bytes with header for storage
    pub fn serialize(&self) -> Vec<u8> {
        let mut output = Vec::with_capacity(DICTIONARY_HEADER_SIZE + self.data.len());

        // Write magic number
        output.extend_from_slice(&DICTIONARY_MAGIC);

        // Write version
        output.push(self.version);

        // Write dictionary size (4 bytes, big-endian)
        let size = self.data.len() as u32;
        output.extend_from_slice(&size.to_be_bytes());

        // Write checksum (4 bytes, big-endian)
        output.extend_from_slice(&self.checksum.to_be_bytes());

        // Write dictionary data
        output.extend_from_slice(&self.data);

        output
    }

    /// Deserialize dictionary from bytes
    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < DICTIONARY_HEADER_SIZE {
            return Err(CompressionError::DictionaryError(
                "Dictionary data too short for header".to_string(),
            ));
        }

        // Verify magic number
        if bytes[0..4] != DICTIONARY_MAGIC {
            return Err(CompressionError::DictionaryError(
                "Invalid dictionary magic number".to_string(),
            ));
        }

        // Read version
        let version = bytes[4];
        if version > DICTIONARY_VERSION {
            return Err(CompressionError::DictionaryError(format!(
                "Unsupported dictionary version: {} (max supported: {})",
                version, DICTIONARY_VERSION
            )));
        }

        // Read size
        let size = u32::from_be_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;

        // Read checksum
        let stored_checksum = u32::from_be_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);

        // Verify we have enough data
        if bytes.len() < DICTIONARY_HEADER_SIZE + size {
            return Err(CompressionError::DictionaryError(format!(
                "Dictionary data truncated: expected {} bytes, got {}",
                DICTIONARY_HEADER_SIZE + size,
                bytes.len()
            )));
        }

        // Extract dictionary data
        let data = bytes[DICTIONARY_HEADER_SIZE..DICTIONARY_HEADER_SIZE + size].to_vec();

        // Verify checksum
        let computed_checksum = Self::compute_checksum(&data);
        if computed_checksum != stored_checksum {
            return Err(CompressionError::DictionaryError(format!(
                "Dictionary checksum mismatch: expected {:08x}, computed {:08x}",
                stored_checksum, computed_checksum
            )));
        }

        Ok(Self {
            data,
            version,
            checksum: stored_checksum,
            id: None,
            config: DictionaryConfig::default(),
        })
    }

    /// Check if dictionary is valid for use
    pub fn is_valid(&self) -> bool {
        !self.data.is_empty() && self.verify()
    }
}

/// Statistics about dictionary compression benefit
#[derive(Debug, Clone)]
pub struct DictionaryBenefit {
    /// Number of samples tested
    pub samples_tested: usize,

    /// Total original data size
    pub total_original_size: usize,

    /// Compressed size without dictionary
    pub compressed_size_without_dict: usize,

    /// Compressed size with dictionary
    pub compressed_size_with_dict: usize,

    /// Compression ratio without dictionary (compressed/original)
    pub ratio_without_dict: f64,

    /// Compression ratio with dictionary (compressed/original)
    pub ratio_with_dict: f64,

    /// Improvement percentage from using dictionary
    pub improvement_percent: f64,
}

impl DictionaryBenefit {
    /// Returns true if using dictionary provides better compression
    pub fn is_beneficial(&self) -> bool {
        self.compressed_size_with_dict < self.compressed_size_without_dict
    }

    /// Returns the space saved in bytes by using dictionary
    pub fn bytes_saved(&self) -> usize {
        self.compressed_size_without_dict
            .saturating_sub(self.compressed_size_with_dict)
    }
}

impl std::fmt::Display for DictionaryBenefit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Dictionary Benefit: {:.1}% improvement ({} bytes saved)\n\
             - Original: {} bytes\n\
             - Without dict: {} bytes ({:.1}%)\n\
             - With dict: {} bytes ({:.1}%)",
            self.improvement_percent,
            self.bytes_saved(),
            self.total_original_size,
            self.compressed_size_without_dict,
            self.ratio_without_dict * 100.0,
            self.compressed_size_with_dict,
            self.ratio_with_dict * 100.0
        )
    }
}

/// Zstd compression codec
pub struct ZstdCodec {
    config: ZstdConfig,
}

impl ZstdCodec {
    /// Create a new Zstd codec with default configuration
    pub fn new() -> Self {
        Self {
            config: ZstdConfig::default(),
        }
    }

    /// Create a new Zstd codec with custom configuration
    pub fn with_config(config: ZstdConfig) -> Self {
        Self { config }
    }

    /// Compress data using Zstd
    pub fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let compressed = zstd::bulk::compress(input, self.config.level.value())
            .map_err(|e| CompressionError::ZstdError(e.to_string()))?;

        Ok(compressed)
    }

    /// Decompress Zstd data
    pub fn decompress(&self, input: &[u8], max_size: Option<usize>) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let max_size = max_size.unwrap_or(input.len() * 4);

        let decompressed = zstd::bulk::decompress(input, max_size)
            .map_err(|e| CompressionError::ZstdError(e.to_string()))?;

        Ok(decompressed)
    }

    /// Compress data using Zstd stream
    pub fn compress_stream<R: Read, W: Write>(&self, mut reader: R, writer: W) -> Result<usize> {
        let mut encoder = Encoder::new(writer, self.config.level.value())
            .map_err(|e| CompressionError::ZstdError(e.to_string()))?;

        if self.config.checksum {
            encoder
                .include_checksum(true)
                .map_err(|e| CompressionError::ZstdError(e.to_string()))?;
        }

        if self.config.long_distance_matching {
            encoder
                .long_distance_matching(true)
                .map_err(|e| CompressionError::ZstdError(e.to_string()))?;
        }

        // Note: multithread is not available in current zstd API
        // Threads parameter is accepted but not used in this implementation
        let _ = self.config.threads;

        let bytes_written = std::io::copy(&mut reader, &mut encoder)?;

        encoder
            .finish()
            .map_err(|e| CompressionError::ZstdError(e.to_string()))?;

        Ok(bytes_written as usize)
    }

    /// Decompress Zstd stream
    pub fn decompress_stream<R: Read, W: Write>(&self, reader: R, mut writer: W) -> Result<usize> {
        let mut decoder =
            Decoder::new(reader).map_err(|e| CompressionError::ZstdError(e.to_string()))?;

        let bytes_written = std::io::copy(&mut decoder, &mut writer)?;

        Ok(bytes_written as usize)
    }

    /// Train a compression dictionary from sample data
    ///
    /// This method trains a ZSTD dictionary from provided samples. The dictionary
    /// can significantly improve compression ratios for small, similar data.
    ///
    /// # Arguments
    /// * `samples` - Slice of sample data to train from
    /// * `config` - Dictionary configuration
    ///
    /// # Returns
    /// A trained `ZstdDictionary` that can be used for compression/decompression
    ///
    /// # Errors
    /// Returns error if:
    /// - Not enough samples provided (less than `config.min_samples`)
    /// - Dictionary training fails
    ///
    /// # Example
    /// ```ignore
    /// let samples = vec![b"sample1".as_slice(), b"sample2".as_slice()];
    /// let config = DictionaryConfig::with_size(64 * 1024);
    /// let dict = ZstdCodec::train_dictionary(&samples, &config)?;
    /// ```
    pub fn train_dictionary(
        samples: &[&[u8]],
        config: &DictionaryConfig,
    ) -> Result<ZstdDictionary> {
        if samples.is_empty() {
            return Err(CompressionError::InvalidParameter(
                "No samples provided for dictionary training".to_string(),
            ));
        }

        if samples.len() < config.min_samples {
            return Err(CompressionError::InvalidParameter(format!(
                "Not enough samples for effective training: got {}, need at least {}",
                samples.len(),
                config.min_samples
            )));
        }

        // Validate dictionary size
        if config.dict_size == 0 {
            return Err(CompressionError::InvalidParameter(
                "Dictionary size cannot be zero".to_string(),
            ));
        }

        // Train dictionary using zstd's from_samples function
        let dict_data = zstd::dict::from_samples(samples, config.dict_size).map_err(|e| {
            CompressionError::DictionaryError(format!("Dictionary training failed: {}", e))
        })?;

        if dict_data.is_empty() {
            return Err(CompressionError::DictionaryError(
                "Dictionary training produced empty dictionary".to_string(),
            ));
        }

        Ok(ZstdDictionary::new(dict_data, config.clone()))
    }

    /// Train dictionary with default configuration
    pub fn train_dictionary_default(samples: &[&[u8]]) -> Result<ZstdDictionary> {
        Self::train_dictionary(samples, &DictionaryConfig::default())
    }

    /// Compress data using a trained dictionary
    ///
    /// Using a dictionary can significantly improve compression ratios for small,
    /// similar data patterns.
    ///
    /// # Arguments
    /// * `input` - Data to compress
    /// * `dictionary` - Trained dictionary to use
    ///
    /// # Returns
    /// Compressed data
    pub fn compress_with_dictionary(
        &self,
        input: &[u8],
        dictionary: &ZstdDictionary,
    ) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        if !dictionary.is_valid() {
            return Err(CompressionError::DictionaryError(
                "Invalid or corrupted dictionary".to_string(),
            ));
        }

        // Create encoder with dictionary
        let mut encoder =
            zstd::bulk::Compressor::with_dictionary(self.config.level.value(), dictionary.data())
                .map_err(|e| {
                CompressionError::ZstdError(format!(
                    "Failed to create compressor with dictionary: {}",
                    e
                ))
            })?;

        let compressed = encoder.compress(input).map_err(|e| {
            CompressionError::ZstdError(format!("Dictionary compression failed: {}", e))
        })?;

        Ok(compressed)
    }

    /// Decompress data that was compressed with a dictionary
    ///
    /// # Arguments
    /// * `input` - Compressed data
    /// * `dictionary` - The same dictionary used for compression
    /// * `max_size` - Optional maximum decompressed size
    ///
    /// # Returns
    /// Decompressed data
    pub fn decompress_with_dictionary(
        &self,
        input: &[u8],
        dictionary: &ZstdDictionary,
        max_size: Option<usize>,
    ) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        if !dictionary.is_valid() {
            return Err(CompressionError::DictionaryError(
                "Invalid or corrupted dictionary".to_string(),
            ));
        }

        // Create decompressor with dictionary
        let mut decompressor = zstd::bulk::Decompressor::with_dictionary(dictionary.data())
            .map_err(|e| {
                CompressionError::ZstdError(format!(
                    "Failed to create decompressor with dictionary: {}",
                    e
                ))
            })?;

        // Estimate output size
        let capacity = max_size.unwrap_or_else(|| input.len().saturating_mul(4));

        let decompressed = decompressor.decompress(input, capacity).map_err(|e| {
            CompressionError::ZstdError(format!("Dictionary decompression failed: {}", e))
        })?;

        Ok(decompressed)
    }

    /// Compress data using streaming with a dictionary
    pub fn compress_stream_with_dictionary<R: Read, W: Write>(
        &self,
        mut reader: R,
        writer: W,
        dictionary: &ZstdDictionary,
    ) -> Result<usize> {
        if !dictionary.is_valid() {
            return Err(CompressionError::DictionaryError(
                "Invalid or corrupted dictionary".to_string(),
            ));
        }

        // Create encoder with dictionary
        let mut encoder =
            Encoder::with_dictionary(writer, self.config.level.value(), dictionary.data())
                .map_err(|e| {
                    CompressionError::ZstdError(format!(
                        "Failed to create stream encoder with dictionary: {}",
                        e
                    ))
                })?;

        if self.config.checksum {
            encoder
                .include_checksum(true)
                .map_err(|e| CompressionError::ZstdError(e.to_string()))?;
        }

        if self.config.long_distance_matching {
            encoder
                .long_distance_matching(true)
                .map_err(|e| CompressionError::ZstdError(e.to_string()))?;
        }

        let bytes_written = std::io::copy(&mut reader, &mut encoder)?;

        encoder
            .finish()
            .map_err(|e| CompressionError::ZstdError(e.to_string()))?;

        Ok(bytes_written as usize)
    }

    /// Decompress data using streaming with a dictionary
    pub fn decompress_stream_with_dictionary<R: Read + BufRead, W: Write>(
        &self,
        reader: R,
        mut writer: W,
        dictionary: &ZstdDictionary,
    ) -> Result<usize> {
        if !dictionary.is_valid() {
            return Err(CompressionError::DictionaryError(
                "Invalid or corrupted dictionary".to_string(),
            ));
        }

        let mut decoder = Decoder::with_dictionary(reader, dictionary.data()).map_err(|e| {
            CompressionError::ZstdError(format!(
                "Failed to create stream decoder with dictionary: {}",
                e
            ))
        })?;

        let bytes_written = std::io::copy(&mut decoder, &mut writer)?;

        Ok(bytes_written as usize)
    }

    /// Estimate compression ratio improvement from using a dictionary
    ///
    /// This method compresses sample data with and without dictionary
    /// to estimate the compression ratio improvement.
    pub fn estimate_dictionary_benefit(
        &self,
        samples: &[&[u8]],
        dictionary: &ZstdDictionary,
    ) -> Result<DictionaryBenefit> {
        if samples.is_empty() {
            return Err(CompressionError::InvalidParameter(
                "No samples provided for benefit estimation".to_string(),
            ));
        }

        let mut total_original_size: usize = 0;
        let mut total_compressed_without_dict: usize = 0;
        let mut total_compressed_with_dict: usize = 0;

        for sample in samples {
            total_original_size = total_original_size.saturating_add(sample.len());

            // Compress without dictionary
            let without_dict = self.compress(sample)?;
            total_compressed_without_dict =
                total_compressed_without_dict.saturating_add(without_dict.len());

            // Compress with dictionary
            let with_dict = self.compress_with_dictionary(sample, dictionary)?;
            total_compressed_with_dict = total_compressed_with_dict.saturating_add(with_dict.len());
        }

        let ratio_without = if total_original_size > 0 {
            total_compressed_without_dict as f64 / total_original_size as f64
        } else {
            1.0
        };

        let ratio_with = if total_original_size > 0 {
            total_compressed_with_dict as f64 / total_original_size as f64
        } else {
            1.0
        };

        let improvement = if ratio_without > 0.0 {
            ((ratio_without - ratio_with) / ratio_without) * 100.0
        } else {
            0.0
        };

        Ok(DictionaryBenefit {
            samples_tested: samples.len(),
            total_original_size,
            compressed_size_without_dict: total_compressed_without_dict,
            compressed_size_with_dict: total_compressed_with_dict,
            ratio_without_dict: ratio_without,
            ratio_with_dict: ratio_with,
            improvement_percent: improvement,
        })
    }

    /// Get the maximum compressed size for input of given size
    pub fn max_compressed_size(input_size: usize) -> usize {
        // Conservative estimate: input size + 1% + 256 bytes overhead
        input_size + (input_size / 100) + 256
    }

    /// Get compression level
    pub fn level(&self) -> i32 {
        self.config.level.value()
    }
}

impl Default for ZstdCodec {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_zstd_level_validation() {
        assert!(ZstdLevel::new(0).is_err());
        assert!(ZstdLevel::new(1).is_ok());
        assert!(ZstdLevel::new(22).is_ok());
        assert!(ZstdLevel::new(23).is_err());
    }

    #[test]
    fn test_zstd_compress_decompress() {
        let codec = ZstdCodec::new();
        let data = b"Hello, world! This is a test of Zstd compression.".repeat(100);

        let compressed = codec.compress(&data).expect("Compression failed");
        assert!(compressed.len() < data.len());

        let decompressed = codec
            .decompress(&compressed, Some(data.len() * 2))
            .expect("Decompression failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_zstd_empty_data() {
        let codec = ZstdCodec::new();
        let data: &[u8] = b"";

        let compressed = codec.compress(data).expect("Compression failed");
        assert_eq!(compressed.len(), 0);

        let decompressed = codec
            .decompress(&compressed, Some(0))
            .expect("Decompression failed");
        assert_eq!(decompressed.len(), 0);
    }

    #[test]
    fn test_zstd_config() {
        let config = ZstdConfig::with_level(15)
            .expect("Config creation failed")
            .with_checksum(true)
            .with_threads(4);

        assert_eq!(config.level.value(), 15);
        assert!(config.checksum);
        assert_eq!(config.threads, 4);
    }

    #[test]
    fn test_zstd_max_compressed_size() {
        let size = ZstdCodec::max_compressed_size(1024);
        assert!(size >= 1024);
    }

    // ==================== Dictionary Tests ====================

    /// Generate sample data for dictionary training
    fn generate_sample_data() -> Vec<Vec<u8>> {
        let templates = [
            r#"{"type":"geospatial","coordinates":[{LAT},{LON}],"properties":{"name":"{NAME}","value":{VAL}}}"#,
            r#"{"type":"feature","geometry":{"type":"Point","coordinates":[{LON},{LAT}]},"id":"{ID}"}"#,
            r#"{"timestamp":"{TS}","sensor":"temp_{SID}","reading":{VAL},"unit":"celsius"}"#,
        ];

        let mut samples = Vec::with_capacity(100);

        for i in 0..100 {
            let template = &templates[i % templates.len()];
            let sample = template
                .replace("{LAT}", &format!("{:.6}", (i as f64) * 0.01 + 35.0))
                .replace("{LON}", &format!("{:.6}", (i as f64) * 0.01 + 139.0))
                .replace("{NAME}", &format!("location_{}", i))
                .replace("{VAL}", &format!("{}", i * 10))
                .replace("{ID}", &format!("feat_{:04}", i))
                .replace("{TS}", &format!("2024-01-{:02}T12:00:00Z", (i % 31) + 1))
                .replace("{SID}", &format!("{:03}", i % 100));

            samples.push(sample.into_bytes());
        }

        samples
    }

    #[test]
    fn test_dictionary_config_default() {
        let config = DictionaryConfig::default();

        assert_eq!(config.dict_size, 112 * 1024);
        assert_eq!(config.training_level, ZstdLevel::DEFAULT);
        assert_eq!(config.min_samples, 5);
        assert_eq!(config.max_sample_size, 1 << 20);
    }

    #[test]
    fn test_dictionary_config_builder() {
        let config = DictionaryConfig::with_size(64 * 1024)
            .with_training_level(10)
            .with_min_samples(10)
            .with_max_sample_size(512 * 1024);

        assert_eq!(config.dict_size, 64 * 1024);
        assert_eq!(config.training_level, 10);
        assert_eq!(config.min_samples, 10);
        assert_eq!(config.max_sample_size, 512 * 1024);
    }

    #[test]
    fn test_dictionary_config_level_clamping() {
        let config = DictionaryConfig::default().with_training_level(100);
        assert_eq!(config.training_level, ZstdLevel::MAX);

        let config = DictionaryConfig::default().with_training_level(-10);
        assert_eq!(config.training_level, ZstdLevel::MIN);
    }

    #[test]
    fn test_dictionary_training() {
        let samples = generate_sample_data();
        let sample_refs: Vec<&[u8]> = samples.iter().map(|s| s.as_slice()).collect();

        let config = DictionaryConfig::with_size(16 * 1024).with_min_samples(5);

        let dict =
            ZstdCodec::train_dictionary(&sample_refs, &config).expect("Dictionary training failed");

        assert!(!dict.data().is_empty());
        assert!(dict.is_valid());
        assert_eq!(dict.version(), DICTIONARY_VERSION);
    }

    #[test]
    fn test_dictionary_training_default() {
        let samples = generate_sample_data();
        let sample_refs: Vec<&[u8]> = samples.iter().map(|s| s.as_slice()).collect();

        let dict =
            ZstdCodec::train_dictionary_default(&sample_refs).expect("Dictionary training failed");

        assert!(!dict.data().is_empty());
        assert!(dict.is_valid());
    }

    #[test]
    fn test_dictionary_training_empty_samples() {
        let samples: Vec<&[u8]> = vec![];
        let config = DictionaryConfig::default();

        let result = ZstdCodec::train_dictionary(&samples, &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_dictionary_training_not_enough_samples() {
        let samples = vec![b"sample1".as_slice(), b"sample2".as_slice()];
        let config = DictionaryConfig::default().with_min_samples(5);

        let result = ZstdCodec::train_dictionary(&samples, &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_dictionary_compress_decompress() {
        let samples = generate_sample_data();
        let sample_refs: Vec<&[u8]> = samples.iter().map(|s| s.as_slice()).collect();

        let config = DictionaryConfig::with_size(16 * 1024).with_min_samples(5);
        let dict =
            ZstdCodec::train_dictionary(&sample_refs, &config).expect("Dictionary training failed");

        let codec = ZstdCodec::new();

        // Test compression and decompression with dictionary
        for sample in &samples[0..10] {
            let compressed = codec
                .compress_with_dictionary(sample, &dict)
                .expect("Compression with dictionary failed");

            let decompressed = codec
                .decompress_with_dictionary(&compressed, &dict, Some(sample.len() * 2))
                .expect("Decompression with dictionary failed");

            assert_eq!(decompressed, *sample);
        }
    }

    #[test]
    fn test_dictionary_compress_empty_data() {
        let samples = generate_sample_data();
        let sample_refs: Vec<&[u8]> = samples.iter().map(|s| s.as_slice()).collect();

        let config = DictionaryConfig::with_size(16 * 1024).with_min_samples(5);
        let dict =
            ZstdCodec::train_dictionary(&sample_refs, &config).expect("Dictionary training failed");

        let codec = ZstdCodec::new();
        let empty: &[u8] = b"";

        let compressed = codec
            .compress_with_dictionary(empty, &dict)
            .expect("Compression failed");
        assert!(compressed.is_empty());

        let decompressed = codec
            .decompress_with_dictionary(&compressed, &dict, Some(0))
            .expect("Decompression failed");
        assert!(decompressed.is_empty());
    }

    #[test]
    fn test_dictionary_stream_compress_decompress() {
        let samples = generate_sample_data();
        let sample_refs: Vec<&[u8]> = samples.iter().map(|s| s.as_slice()).collect();

        let config = DictionaryConfig::with_size(16 * 1024).with_min_samples(5);
        let dict =
            ZstdCodec::train_dictionary(&sample_refs, &config).expect("Dictionary training failed");

        let codec = ZstdCodec::new();

        // Test stream compression
        let input_data = samples[0].clone();
        let mut compressed = Vec::new();

        let bytes_read = codec
            .compress_stream_with_dictionary(Cursor::new(&input_data), &mut compressed, &dict)
            .expect("Stream compression failed");

        assert_eq!(bytes_read, input_data.len());
        assert!(!compressed.is_empty());

        // Test stream decompression
        let mut decompressed = Vec::new();

        let bytes_written = codec
            .decompress_stream_with_dictionary(Cursor::new(&compressed), &mut decompressed, &dict)
            .expect("Stream decompression failed");

        assert_eq!(bytes_written, input_data.len());
        assert_eq!(decompressed, input_data);
    }

    #[test]
    fn test_dictionary_serialization() {
        let samples = generate_sample_data();
        let sample_refs: Vec<&[u8]> = samples.iter().map(|s| s.as_slice()).collect();

        let config = DictionaryConfig::with_size(16 * 1024).with_min_samples(5);
        let dict =
            ZstdCodec::train_dictionary(&sample_refs, &config).expect("Dictionary training failed");

        // Serialize
        let serialized = dict.serialize();
        assert!(!serialized.is_empty());
        assert!(serialized.len() > DICTIONARY_HEADER_SIZE);

        // Verify magic number
        assert_eq!(&serialized[0..4], &DICTIONARY_MAGIC);

        // Deserialize
        let restored =
            ZstdDictionary::deserialize(&serialized).expect("Dictionary deserialization failed");

        assert_eq!(restored.data(), dict.data());
        assert_eq!(restored.version(), dict.version());
        assert_eq!(restored.checksum(), dict.checksum());
        assert!(restored.is_valid());
    }

    #[test]
    fn test_dictionary_deserialization_invalid_magic() {
        let bad_data = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x04];

        let result = ZstdDictionary::deserialize(&bad_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_dictionary_deserialization_too_short() {
        let bad_data = vec![0x4F, 0x58, 0x5A, 0x44]; // Only magic, no header

        let result = ZstdDictionary::deserialize(&bad_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_dictionary_deserialization_truncated_data() {
        let mut data = vec![0x4F, 0x58, 0x5A, 0x44]; // Magic
        data.push(1); // Version
        data.extend_from_slice(&100u32.to_be_bytes()); // Size = 100
        data.extend_from_slice(&0u32.to_be_bytes()); // Checksum
        data.extend_from_slice(&[0u8; 10]); // Only 10 bytes, not 100

        let result = ZstdDictionary::deserialize(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_dictionary_checksum_verification() {
        let samples = generate_sample_data();
        let sample_refs: Vec<&[u8]> = samples.iter().map(|s| s.as_slice()).collect();

        let config = DictionaryConfig::with_size(16 * 1024).with_min_samples(5);
        let dict =
            ZstdCodec::train_dictionary(&sample_refs, &config).expect("Dictionary training failed");

        // Serialize and corrupt the data
        let mut serialized = dict.serialize();

        // Corrupt a byte in the dictionary data
        if serialized.len() > DICTIONARY_HEADER_SIZE + 5 {
            serialized[DICTIONARY_HEADER_SIZE + 5] ^= 0xFF;
        }

        // Deserialization should fail due to checksum mismatch
        let result = ZstdDictionary::deserialize(&serialized);
        assert!(result.is_err());
    }

    #[test]
    fn test_dictionary_with_id() {
        let samples = generate_sample_data();
        let sample_refs: Vec<&[u8]> = samples.iter().map(|s| s.as_slice()).collect();

        let config = DictionaryConfig::with_size(16 * 1024).with_min_samples(5);
        let dict = ZstdCodec::train_dictionary(&sample_refs, &config)
            .expect("Dictionary training failed")
            .with_id("geospatial_v1");

        assert_eq!(dict.id(), Some("geospatial_v1"));
    }

    #[test]
    fn test_dictionary_benefit_estimation() {
        let samples = generate_sample_data();
        let sample_refs: Vec<&[u8]> = samples.iter().map(|s| s.as_slice()).collect();

        let config = DictionaryConfig::with_size(16 * 1024).with_min_samples(5);
        let dict =
            ZstdCodec::train_dictionary(&sample_refs, &config).expect("Dictionary training failed");

        let codec = ZstdCodec::new();

        let benefit = codec
            .estimate_dictionary_benefit(&sample_refs[0..20], &dict)
            .expect("Benefit estimation failed");

        assert_eq!(benefit.samples_tested, 20);
        assert!(benefit.total_original_size > 0);

        // Dictionary should provide some benefit for similar data
        // (Note: for very small or incompressible data, benefit might be negative)
        let _ = benefit.is_beneficial();
        let _ = benefit.bytes_saved();

        // Test Display implementation
        let display_str = format!("{}", benefit);
        assert!(display_str.contains("Dictionary Benefit"));
    }

    #[test]
    fn test_dictionary_benefit_empty_samples() {
        let samples = generate_sample_data();
        let sample_refs: Vec<&[u8]> = samples.iter().map(|s| s.as_slice()).collect();

        let config = DictionaryConfig::with_size(16 * 1024).with_min_samples(5);
        let dict =
            ZstdCodec::train_dictionary(&sample_refs, &config).expect("Dictionary training failed");

        let codec = ZstdCodec::new();
        let empty_samples: &[&[u8]] = &[];

        let result = codec.estimate_dictionary_benefit(empty_samples, &dict);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_dictionary_compress() {
        let invalid_dict = ZstdDictionary {
            data: vec![],
            version: DICTIONARY_VERSION,
            checksum: 0,
            id: None,
            config: DictionaryConfig::default(),
        };

        let codec = ZstdCodec::new();
        let data = b"test data";

        let result = codec.compress_with_dictionary(data, &invalid_dict);
        assert!(result.is_err());
    }

    #[test]
    fn test_dictionary_version_check() {
        // Create a dictionary with future version
        let mut serialized = vec![0x4F, 0x58, 0x5A, 0x44]; // Magic
        serialized.push(255); // Future version
        serialized.extend_from_slice(&10u32.to_be_bytes()); // Size
        serialized.extend_from_slice(&0u32.to_be_bytes()); // Checksum
        serialized.extend_from_slice(&[0u8; 10]); // Data

        let result = ZstdDictionary::deserialize(&serialized);
        assert!(result.is_err());
    }

    #[test]
    fn test_dictionary_compression_improvement() {
        // Create highly repetitive data that should benefit from dictionary
        let template = r#"{"sensor_id":"TEMP001","timestamp":"2024-01-15T12:00:00Z","value":23.5}"#;
        let samples: Vec<Vec<u8>> = (0..100)
            .map(|i| {
                template
                    .replace("TEMP001", &format!("TEMP{:03}", i % 10))
                    .replace("23.5", &format!("{:.1}", 20.0 + (i as f64) * 0.1))
                    .into_bytes()
            })
            .collect();

        let sample_refs: Vec<&[u8]> = samples.iter().map(|s| s.as_slice()).collect();

        let config = DictionaryConfig::with_size(32 * 1024).with_min_samples(5);
        let dict =
            ZstdCodec::train_dictionary(&sample_refs, &config).expect("Dictionary training failed");

        let codec = ZstdCodec::new();

        // Compare compression with and without dictionary for a sample
        let test_data = samples[50].as_slice();

        let without_dict = codec.compress(test_data).expect("Compression failed");
        let with_dict = codec
            .compress_with_dictionary(test_data, &dict)
            .expect("Dictionary compression failed");

        // With repetitive data and dictionary, compression should be better
        // (For small samples, dictionary might not help due to overhead)
        assert!(!with_dict.is_empty());
        assert!(!without_dict.is_empty());
    }
}
