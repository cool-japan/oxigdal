//! HDF5 filter pipeline for data transformation and compression.
//!
//! This module provides comprehensive filter support including:
//! - GZIP compression (Pure Rust via flate2)
//! - Shuffle filter for better compression (Pure Rust)
//! - Fletcher32 checksum filter (Pure Rust)
//! - ScaleOffset filter (Pure Rust)
//! - N-Bit filter (Pure Rust)
//! - SZIP compression (Pure Rust AEC, feature-gated)
//! - Custom filter chains

pub mod bitpack;
pub mod nbit;
pub mod scale_offset;

#[cfg(feature = "szip")]
pub mod szip;

use crate::datatype::Datatype;
use crate::error::{Hdf5Error, Result};
use byteorder::{ByteOrder, LittleEndian};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// HDF5 filter identifier
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterId {
    /// DEFLATE/GZIP compression (ID: 1)
    Deflate = 1,
    /// Shuffle filter (ID: 2)
    Shuffle = 2,
    /// Fletcher32 checksum (ID: 3)
    Fletcher32 = 3,
    /// SZIP compression (ID: 4)
    Szip = 4,
    /// N-Bit filter (ID: 5)
    NBit = 5,
    /// ScaleOffset filter (ID: 6)
    ScaleOffset = 6,
    /// Custom filter
    Custom(u16),
}

impl FilterId {
    /// Get the numeric ID
    pub fn id(&self) -> u16 {
        match self {
            FilterId::Deflate => 1,
            FilterId::Shuffle => 2,
            FilterId::Fletcher32 => 3,
            FilterId::Szip => 4,
            FilterId::NBit => 5,
            FilterId::ScaleOffset => 6,
            FilterId::Custom(id) => *id,
        }
    }

    /// Create from numeric ID
    pub fn from_id(id: u16) -> Self {
        match id {
            1 => FilterId::Deflate,
            2 => FilterId::Shuffle,
            3 => FilterId::Fletcher32,
            4 => FilterId::Szip,
            5 => FilterId::NBit,
            6 => FilterId::ScaleOffset,
            _ => FilterId::Custom(id),
        }
    }
}

/// Filter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    /// Filter identifier
    id: FilterId,
    /// Filter name
    name: String,
    /// Filter parameters
    params: Vec<u32>,
    /// Whether filter is optional
    optional: bool,
}

impl Filter {
    /// Create a new filter
    pub fn new(id: FilterId, name: String, params: Vec<u32>, optional: bool) -> Self {
        Self {
            id,
            name,
            params,
            optional,
        }
    }

    /// Create DEFLATE/GZIP filter
    pub fn deflate(level: u8) -> Self {
        let level = level.clamp(1, 9);
        Self {
            id: FilterId::Deflate,
            name: "deflate".to_string(),
            params: vec![level as u32],
            optional: false,
        }
    }

    /// Create shuffle filter
    pub fn shuffle(element_size: usize) -> Self {
        Self {
            id: FilterId::Shuffle,
            name: "shuffle".to_string(),
            params: vec![element_size as u32],
            optional: false,
        }
    }

    /// Create Fletcher32 checksum filter
    pub fn fletcher32() -> Self {
        Self {
            id: FilterId::Fletcher32,
            name: "fletcher32".to_string(),
            params: vec![],
            optional: false,
        }
    }

    /// Create SZIP filter (feature-gated)
    #[cfg(feature = "szip")]
    pub fn szip(options_mask: u32, pixels_per_block: u32) -> Self {
        Self {
            id: FilterId::Szip,
            name: "szip".to_string(),
            params: vec![options_mask, pixels_per_block],
            optional: false,
        }
    }

    /// Create ScaleOffset filter
    ///
    /// # Arguments
    /// * `scale_type` - 0 = float decimal scale, 2 = integer auto
    /// * `scale_factor` - For floats: decimal digits of precision; for integers: unused (auto)
    pub fn scale_offset(scale_type: u32, scale_factor: i32) -> Self {
        Self {
            id: FilterId::ScaleOffset,
            name: "scaleoffset".to_string(),
            params: vec![scale_type, scale_factor as u32],
            optional: false,
        }
    }

    /// Create N-Bit filter
    pub fn nbit() -> Self {
        Self {
            id: FilterId::NBit,
            name: "nbit".to_string(),
            params: vec![],
            optional: false,
        }
    }

    /// Get filter ID
    pub fn id(&self) -> FilterId {
        self.id
    }

    /// Get filter name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get filter parameters
    pub fn params(&self) -> &[u32] {
        &self.params
    }

    /// Check if filter is optional
    pub fn is_optional(&self) -> bool {
        self.optional
    }

    /// Set optional flag
    pub fn set_optional(&mut self, optional: bool) {
        self.optional = optional;
    }
}

/// Filter pipeline - ordered sequence of filters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterPipeline {
    /// Ordered list of filters
    filters: Vec<Filter>,
}

impl FilterPipeline {
    /// Create an empty filter pipeline
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
        }
    }

    /// Add a filter to the pipeline
    pub fn add_filter(&mut self, filter: Filter) {
        self.filters.push(filter);
    }

    /// Get all filters
    pub fn filters(&self) -> &[Filter] {
        &self.filters
    }

    /// Check if pipeline is empty
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    /// Get the number of filters
    pub fn len(&self) -> usize {
        self.filters.len()
    }

    /// Apply all filters in forward direction (encoding)
    pub fn apply_forward(
        &self,
        data: &[u8],
        datatype: &Datatype,
        chunk_dims: &[usize],
    ) -> Result<Vec<u8>> {
        let mut buffer = data.to_vec();

        for filter in &self.filters {
            buffer = apply_filter_forward(filter, &buffer, datatype, chunk_dims)?;
        }

        Ok(buffer)
    }

    /// Apply all filters in reverse direction (decoding)
    pub fn apply_reverse(
        &self,
        data: &[u8],
        datatype: &Datatype,
        chunk_dims: &[usize],
    ) -> Result<Vec<u8>> {
        let mut buffer = data.to_vec();

        for filter in self.filters.iter().rev() {
            buffer = apply_filter_reverse(filter, &buffer, datatype, chunk_dims)?;
        }

        Ok(buffer)
    }

    /// Create a standard compression pipeline
    pub fn standard_compression(level: u8, element_size: usize) -> Self {
        let mut pipeline = Self::new();
        pipeline.add_filter(Filter::shuffle(element_size));
        pipeline.add_filter(Filter::deflate(level));
        pipeline
    }

    /// Create a checksum pipeline
    pub fn with_checksum() -> Self {
        let mut pipeline = Self::new();
        pipeline.add_filter(Filter::fletcher32());
        pipeline
    }
}

impl Default for FilterPipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// Apply a single filter in forward direction (encoding)
fn apply_filter_forward(
    filter: &Filter,
    data: &[u8],
    datatype: &Datatype,
    _chunk_dims: &[usize],
) -> Result<Vec<u8>> {
    match filter.id {
        FilterId::Deflate => apply_deflate_forward(data, filter.params()),
        FilterId::Shuffle => apply_shuffle_forward(data, datatype.size()),
        FilterId::Fletcher32 => apply_fletcher32_forward(data),
        FilterId::ScaleOffset => {
            scale_offset::apply_scale_offset_forward(data, filter.params(), datatype)
        }
        FilterId::NBit => nbit::apply_nbit_forward(data, datatype),
        #[cfg(feature = "szip")]
        FilterId::Szip => szip::apply_szip_forward(data, filter.params(), datatype),
        _ => Err(Hdf5Error::UnsupportedCompressionFilter(format!(
            "Filter {:?} not supported",
            filter.id
        ))),
    }
}

/// Apply a single filter in reverse direction (decoding)
fn apply_filter_reverse(
    filter: &Filter,
    data: &[u8],
    datatype: &Datatype,
    _chunk_dims: &[usize],
) -> Result<Vec<u8>> {
    match filter.id {
        FilterId::Deflate => apply_deflate_reverse(data),
        FilterId::Shuffle => apply_shuffle_reverse(data, datatype.size()),
        FilterId::Fletcher32 => apply_fletcher32_reverse(data),
        FilterId::ScaleOffset => {
            scale_offset::apply_scale_offset_reverse(data, filter.params(), datatype)
        }
        FilterId::NBit => nbit::apply_nbit_reverse(data, datatype),
        #[cfg(feature = "szip")]
        FilterId::Szip => szip::apply_szip_reverse(data, filter.params()),
        _ => Err(Hdf5Error::UnsupportedCompressionFilter(format!(
            "Filter {:?} not supported",
            filter.id
        ))),
    }
}

// =============================================================================
// DEFLATE (GZIP) filter
// =============================================================================

/// Apply DEFLATE compression
fn apply_deflate_forward(data: &[u8], params: &[u32]) -> Result<Vec<u8>> {
    let level = params.first().copied().unwrap_or(6);
    let level = level.clamp(1, 9);

    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(level));
    encoder
        .write_all(data)
        .map_err(|e| Hdf5Error::Compression(e.to_string()))?;
    encoder
        .finish()
        .map_err(|e| Hdf5Error::Compression(e.to_string()))
}

/// Apply DEFLATE decompression
fn apply_deflate_reverse(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| Hdf5Error::Decompression(e.to_string()))?;
    Ok(decompressed)
}

// =============================================================================
// Shuffle filter
// =============================================================================

/// Apply shuffle filter forward
///
/// The shuffle filter rearranges the bytes in the data buffer to improve
/// compression. It groups bytes by their position within each element.
fn apply_shuffle_forward(data: &[u8], element_size: usize) -> Result<Vec<u8>> {
    if element_size <= 1 {
        return Ok(data.to_vec());
    }

    let num_elements = data.len() / element_size;
    if data.len() % element_size != 0 {
        return Err(Hdf5Error::InvalidSize(format!(
            "Data size ({}) is not a multiple of element size ({})",
            data.len(),
            element_size
        )));
    }

    let mut shuffled = vec![0u8; data.len()];

    for byte_pos in 0..element_size {
        for elem_idx in 0..num_elements {
            shuffled[byte_pos * num_elements + elem_idx] = data[elem_idx * element_size + byte_pos];
        }
    }

    Ok(shuffled)
}

/// Apply shuffle filter reverse (unshuffle)
fn apply_shuffle_reverse(data: &[u8], element_size: usize) -> Result<Vec<u8>> {
    if element_size <= 1 {
        return Ok(data.to_vec());
    }

    let num_elements = data.len() / element_size;
    if data.len() % element_size != 0 {
        return Err(Hdf5Error::InvalidSize(format!(
            "Data size ({}) is not a multiple of element size ({})",
            data.len(),
            element_size
        )));
    }

    let mut unshuffled = vec![0u8; data.len()];

    for byte_pos in 0..element_size {
        for elem_idx in 0..num_elements {
            unshuffled[elem_idx * element_size + byte_pos] =
                data[byte_pos * num_elements + elem_idx];
        }
    }

    Ok(unshuffled)
}

// =============================================================================
// Fletcher32 checksum filter
// =============================================================================

/// Apply Fletcher32 checksum forward
fn apply_fletcher32_forward(data: &[u8]) -> Result<Vec<u8>> {
    let checksum = calculate_fletcher32(data);
    let mut result = data.to_vec();
    let mut checksum_bytes = [0u8; 4];
    LittleEndian::write_u32(&mut checksum_bytes, checksum);
    result.extend_from_slice(&checksum_bytes);
    Ok(result)
}

/// Apply Fletcher32 checksum reverse (verify and remove checksum)
fn apply_fletcher32_reverse(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 4 {
        return Err(Hdf5Error::InvalidSize(
            "Data too short for Fletcher32 checksum".to_string(),
        ));
    }

    let data_len = data.len() - 4;
    let data_part = &data[..data_len];
    let checksum_bytes = &data[data_len..];

    let expected_checksum = LittleEndian::read_u32(checksum_bytes);
    let actual_checksum = calculate_fletcher32(data_part);

    if expected_checksum != actual_checksum {
        return Err(Hdf5Error::ChecksumMismatch {
            expected: expected_checksum,
            actual: actual_checksum,
        });
    }

    Ok(data_part.to_vec())
}

/// Calculate Fletcher32 checksum
fn calculate_fletcher32(data: &[u8]) -> u32 {
    let mut sum1: u32 = 0;
    let mut sum2: u32 = 0;

    // Process data in 16-bit chunks
    for chunk in data.chunks(2) {
        let value = if chunk.len() == 2 {
            LittleEndian::read_u16(chunk) as u32
        } else {
            chunk[0] as u32
        };

        sum1 = (sum1 + value) % 65535;
        sum2 = (sum2 + sum1) % 65535;
    }

    (sum2 << 16) | sum1
}

// =============================================================================
// Filter mask utility functions
// =============================================================================

/// Filter mask utility functions
pub mod mask {
    /// Check if a filter was skipped (bit is set in mask)
    pub fn is_filter_skipped(mask: u32, filter_index: usize) -> bool {
        if filter_index >= 32 {
            return false;
        }
        (mask & (1 << filter_index)) != 0
    }

    /// Set filter skipped bit in mask
    pub fn set_filter_skipped(mask: u32, filter_index: usize) -> u32 {
        if filter_index >= 32 {
            return mask;
        }
        mask | (1 << filter_index)
    }

    /// Clear filter skipped bit in mask
    pub fn clear_filter_skipped(mask: u32, filter_index: usize) -> u32 {
        if filter_index >= 32 {
            return mask;
        }
        mask & !(1 << filter_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_creation() {
        let filter = Filter::deflate(6);
        assert_eq!(filter.id(), FilterId::Deflate);
        assert_eq!(filter.name(), "deflate");
        assert_eq!(filter.params(), &[6]);
        assert!(!filter.is_optional());
    }

    #[test]
    fn test_filter_pipeline() {
        let mut pipeline = FilterPipeline::new();
        assert!(pipeline.is_empty());

        pipeline.add_filter(Filter::shuffle(4));
        pipeline.add_filter(Filter::deflate(6));

        assert_eq!(pipeline.len(), 2);
        assert!(!pipeline.is_empty());
    }

    #[test]
    fn test_deflate_roundtrip() {
        let data = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let compressed = apply_deflate_forward(&data, &[6]).expect("Compression failed");
        // GZIP adds headers/trailers (~18+ bytes), so compressed can be larger for small data
        assert!(!compressed.is_empty());

        let decompressed = apply_deflate_reverse(&compressed).expect("Decompression failed");
        assert_eq!(decompressed, data);

        // Verify with larger data where compression is effective
        let large_data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
        let large_compressed =
            apply_deflate_forward(&large_data, &[6]).expect("Large compression failed");
        assert!(large_compressed.len() < large_data.len());
        let large_decompressed =
            apply_deflate_reverse(&large_compressed).expect("Large decompression failed");
        assert_eq!(large_decompressed, large_data);
    }

    #[test]
    fn test_shuffle_forward() {
        let data = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let shuffled = apply_shuffle_forward(&data, 4).expect("Shuffle failed");
        assert_eq!(shuffled.len(), data.len());
        assert_ne!(shuffled, data);
    }

    #[test]
    fn test_shuffle_roundtrip() {
        let data = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let shuffled = apply_shuffle_forward(&data, 4).expect("Shuffle failed");
        let unshuffled = apply_shuffle_reverse(&shuffled, 4).expect("Unshuffle failed");
        assert_eq!(unshuffled, data);
    }

    #[test]
    fn test_fletcher32_calculation() {
        let data = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let checksum = calculate_fletcher32(&data);
        assert!(checksum > 0);

        // Same data should produce same checksum
        let checksum2 = calculate_fletcher32(&data);
        assert_eq!(checksum, checksum2);
    }

    #[test]
    fn test_fletcher32_roundtrip() {
        let data = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let with_checksum = apply_fletcher32_forward(&data).expect("Checksum failed");
        assert_eq!(with_checksum.len(), data.len() + 4);

        let verified = apply_fletcher32_reverse(&with_checksum).expect("Verification failed");
        assert_eq!(verified, data);
    }

    #[test]
    fn test_fletcher32_bad_checksum() {
        let data = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut with_checksum = apply_fletcher32_forward(&data).expect("Checksum failed");

        // Corrupt the checksum
        let last = with_checksum.len() - 1;
        with_checksum[last] ^= 0xFF;

        let result = apply_fletcher32_reverse(&with_checksum);
        assert!(result.is_err());
    }

    #[test]
    fn test_pipeline_roundtrip() {
        let data = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let datatype = Datatype::Float32;
        let chunk_dims = vec![4];

        let pipeline = FilterPipeline::standard_compression(6, 4);

        let encoded = pipeline
            .apply_forward(&data, &datatype, &chunk_dims)
            .expect("Encoding failed");

        let decoded = pipeline
            .apply_reverse(&encoded, &datatype, &chunk_dims)
            .expect("Decoding failed");

        assert_eq!(decoded, data);
    }

    #[test]
    fn test_filter_mask() {
        let mut mask_val = 0u32;
        assert!(!mask::is_filter_skipped(mask_val, 0));

        mask_val = mask::set_filter_skipped(mask_val, 0);
        assert!(mask::is_filter_skipped(mask_val, 0));
        assert!(!mask::is_filter_skipped(mask_val, 1));

        mask_val = mask::set_filter_skipped(mask_val, 3);
        assert!(mask::is_filter_skipped(mask_val, 0));
        assert!(mask::is_filter_skipped(mask_val, 3));

        mask_val = mask::clear_filter_skipped(mask_val, 0);
        assert!(!mask::is_filter_skipped(mask_val, 0));
        assert!(mask::is_filter_skipped(mask_val, 3));
    }
}
