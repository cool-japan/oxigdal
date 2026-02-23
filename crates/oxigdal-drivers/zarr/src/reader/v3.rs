//! Zarr v3 array reader implementation
//!
//! This module provides a comprehensive reader for Zarr v3 arrays,
//! including codec pipeline support, sharding, and storage transformers.

use crate::codecs::{Codec, CodecChain};
use crate::error::{Result, ZarrError};
use crate::metadata::v3::{ArrayMetadataV3, CodecMetadata};
use crate::sharding::{IndexLocation, ShardReader};
use crate::storage::{Store, StoreKey};
use crate::transformers::{Transformer, TransformerChain};
use std::collections::HashMap;
use std::sync::Arc;

/// Zarr v3 array reader
pub struct ZarrV3Reader<S: Store> {
    /// Storage backend
    store: Arc<S>,
    /// Array path
    path: String,
    /// Array metadata
    metadata: ArrayMetadataV3,
    /// Codec pipeline
    codecs: CodecChain,
    /// Storage transformers
    transformers: TransformerChain,
    /// Chunk cache
    cache: Option<HashMap<Vec<usize>, Vec<u8>>>,
}

impl<S: Store> ZarrV3Reader<S> {
    /// Creates a new v3 reader
    ///
    /// # Errors
    /// Returns error if metadata cannot be loaded or parsed
    pub fn new(store: S, path: impl Into<String>) -> Result<Self> {
        let path = path.into();
        let store = Arc::new(store);

        // Load metadata
        let metadata_key = format!("{}/zarr.json", path);
        let metadata_bytes = store.get(&StoreKey::new(metadata_key))?;
        let metadata: ArrayMetadataV3 = serde_json::from_slice(&metadata_bytes)?;

        // Validate metadata
        metadata.validate()?;

        // Build codec pipeline
        let codecs = Self::build_codec_chain(&metadata)?;

        // Build transformer chain
        let transformers = Self::build_transformer_chain(&metadata)?;

        Ok(Self {
            store,
            path,
            metadata,
            codecs,
            transformers,
            cache: None,
        })
    }

    /// Enables chunk caching
    pub fn with_cache(&mut self, enabled: bool) {
        if enabled {
            self.cache = Some(HashMap::new());
        } else {
            self.cache = None;
        }
    }

    /// Returns the array metadata
    #[must_use]
    pub const fn metadata(&self) -> &ArrayMetadataV3 {
        &self.metadata
    }

    /// Returns the array shape
    #[must_use]
    pub fn shape(&self) -> &[usize] {
        &self.metadata.shape
    }

    /// Returns the number of dimensions
    #[must_use]
    pub fn ndim(&self) -> usize {
        self.metadata.shape.len()
    }

    /// Returns the chunk shape for regular grids
    ///
    /// # Errors
    /// Returns error if grid is not regular
    pub fn chunk_shape(&self) -> Result<&[usize]> {
        self.metadata.chunk_grid.regular_chunk_shape()
    }

    /// Returns the fill value
    #[must_use]
    pub const fn fill_value(&self) -> &crate::metadata::v3::FillValue {
        &self.metadata.fill_value
    }

    /// Returns the data type
    #[must_use]
    pub const fn data_type(&self) -> &crate::metadata::v3::DataType {
        &self.metadata.data_type
    }

    /// Reads a chunk at the given coordinates
    ///
    /// # Errors
    /// Returns error if chunk cannot be read or decoded
    pub fn read_chunk(&self, coords: &[usize]) -> Result<Vec<u8>> {
        // Check cache first
        if let Some(cache) = &self.cache {
            if let Some(data) = cache.get(coords) {
                return Ok(data.clone());
            }
        }

        // Build chunk key
        let chunk_key = self.build_chunk_key(coords)?;

        // Read from storage
        let encoded_data = match self.store.get(&StoreKey::new(chunk_key)) {
            Ok(data) => data,
            Err(_) => {
                // Chunk doesn't exist, return fill value
                return self.create_fill_chunk();
            }
        };

        // Apply storage transformers (decode)
        let transformed_data = self.transformers.decode(encoded_data)?;

        // Check if this is a sharded array
        if let Some(codecs) = &self.metadata.codecs {
            if codecs
                .iter()
                .any(|c| matches!(c, CodecMetadata::ShardingIndexed { .. }))
            {
                return self.read_from_shard(&transformed_data, coords);
            }
        }

        // Apply codec pipeline (decode)
        let decoded_data = self.codecs.decode(transformed_data)?;

        Ok(decoded_data)
    }

    /// Reads a slice from the array
    ///
    /// # Errors
    /// Returns error if slice cannot be read
    pub fn read_slice(&self, ranges: &[std::ops::Range<usize>]) -> Result<Vec<u8>> {
        if ranges.len() != self.ndim() {
            return Err(ZarrError::InvalidDimension {
                message: format!("Expected {} dimensions, got {}", self.ndim(), ranges.len()),
            });
        }

        // Calculate slice shape
        let slice_shape: Vec<usize> = ranges.iter().map(|r| r.end - r.start).collect();
        let slice_size: usize = slice_shape.iter().product();

        // Get item size
        let item_size = self.metadata.data_type.item_size()?;
        let mut result = vec![0u8; slice_size * item_size];

        // Calculate which chunks we need
        let chunk_shape = self.chunk_shape()?;
        let chunk_ranges = self.calculate_chunk_ranges(ranges, chunk_shape)?;

        let ndim = self.ndim();

        // Precompute strides for the result array (row-major / C order)
        let result_strides = compute_strides(&slice_shape);
        // Precompute strides for each chunk
        let chunk_strides = compute_strides(chunk_shape);

        // Read each chunk and extract the relevant data
        for chunk_coords in chunk_ranges {
            let chunk_data = self.read_chunk(&chunk_coords)?;

            // Calculate the overlap between this chunk and the requested ranges
            let mut chunk_region_start = Vec::with_capacity(ndim);
            let mut chunk_region_end = Vec::with_capacity(ndim);
            let mut result_offset_start = Vec::with_capacity(ndim);

            for dim in 0..ndim {
                let chunk_global_start = chunk_coords[dim] * chunk_shape[dim];
                let chunk_global_end = chunk_global_start + chunk_shape[dim];

                // Overlap region in global coordinates
                let overlap_start = ranges[dim].start.max(chunk_global_start);
                let overlap_end = ranges[dim].end.min(chunk_global_end);

                // Region within the chunk (local coordinates)
                chunk_region_start.push(overlap_start - chunk_global_start);
                chunk_region_end.push(overlap_end - chunk_global_start);

                // Offset in the result buffer
                result_offset_start.push(overlap_start - ranges[dim].start);
            }

            // Calculate the size of the overlap region
            let overlap_shape: Vec<usize> = (0..ndim)
                .map(|d| chunk_region_end[d] - chunk_region_start[d])
                .collect();

            // Skip if overlap is empty in any dimension
            if overlap_shape.contains(&0) {
                continue;
            }

            // Copy data from chunk to result using multi-dimensional iteration
            // We iterate over all elements in the overlap region
            let overlap_size: usize = overlap_shape.iter().product();
            for linear_idx in 0..overlap_size {
                // Convert linear index to multi-dimensional coordinates within the overlap
                let mut overlap_coords = vec![0usize; ndim];
                let mut remaining = linear_idx;
                for dim in (0..ndim).rev() {
                    overlap_coords[dim] = remaining % overlap_shape[dim];
                    remaining /= overlap_shape[dim];
                }

                // Calculate source offset within chunk (flat index)
                let mut chunk_flat_idx = 0;
                for dim in 0..ndim {
                    chunk_flat_idx +=
                        (chunk_region_start[dim] + overlap_coords[dim]) * chunk_strides[dim];
                }

                // Calculate destination offset within result (flat index)
                let mut result_flat_idx = 0;
                for dim in 0..ndim {
                    result_flat_idx +=
                        (result_offset_start[dim] + overlap_coords[dim]) * result_strides[dim];
                }

                // Copy item_size bytes
                let src_byte_offset = chunk_flat_idx * item_size;
                let dst_byte_offset = result_flat_idx * item_size;

                if src_byte_offset + item_size <= chunk_data.len()
                    && dst_byte_offset + item_size <= result.len()
                {
                    result[dst_byte_offset..dst_byte_offset + item_size]
                        .copy_from_slice(&chunk_data[src_byte_offset..src_byte_offset + item_size]);
                }
            }
        }

        Ok(result)
    }

    /// Reads the entire array
    ///
    /// # Errors
    /// Returns error if array cannot be read
    pub fn read_all(&self) -> Result<Vec<u8>> {
        let ranges: Vec<_> = self.metadata.shape.iter().map(|&s| 0..s).collect();
        self.read_slice(&ranges)
    }

    /// Builds the codec chain from metadata
    fn build_codec_chain(metadata: &ArrayMetadataV3) -> Result<CodecChain> {
        let mut codecs = Vec::new();

        if let Some(codec_list) = &metadata.codecs {
            for codec_meta in codec_list {
                let codec = build_codec_from_metadata(codec_meta)?;
                codecs.push(codec);
            }
        }

        Ok(CodecChain::new(codecs))
    }

    /// Builds the transformer chain from metadata
    fn build_transformer_chain(metadata: &ArrayMetadataV3) -> Result<TransformerChain> {
        let mut transformers = Vec::new();

        if let Some(transformer_list) = &metadata.storage_transformers {
            for transformer_meta in transformer_list {
                let transformer = build_transformer_from_metadata(transformer_meta)?;
                transformers.push(transformer);
            }
        }

        Ok(TransformerChain::new(transformers))
    }

    /// Builds a chunk key from coordinates
    fn build_chunk_key(&self, coords: &[usize]) -> Result<String> {
        use crate::metadata::v3::ChunkKeyEncoding;

        let encoding = &self.metadata.chunk_key_encoding;
        let key = match encoding {
            ChunkKeyEncoding::Default { configuration } => {
                let separator = configuration
                    .as_ref()
                    .map(|c| c.separator.as_str())
                    .unwrap_or("/");
                let coord_str = coords
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(separator);
                if self.path.is_empty() {
                    format!("c{}{}", separator, coord_str)
                } else {
                    format!("{}/c{}{}", self.path, separator, coord_str)
                }
            }
            ChunkKeyEncoding::V2 { configuration } => {
                let separator = configuration
                    .as_ref()
                    .map(|c| c.separator.as_str())
                    .unwrap_or(".");
                let coord_str = coords
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(separator);
                format!("{}/{}", self.path, coord_str)
            }
        };

        Ok(key)
    }

    /// Creates a chunk filled with fill values
    fn create_fill_chunk(&self) -> Result<Vec<u8>> {
        let chunk_shape = self.chunk_shape()?;
        let chunk_size: usize = chunk_shape.iter().product();
        let item_size = self.metadata.data_type.item_size()?;

        // Serialize the fill value to bytes
        let fill_bytes = self.metadata.fill_value.to_bytes(item_size)?;

        // Create the full chunk by repeating the fill value pattern
        let total_bytes = chunk_size * item_size;
        let mut result = Vec::with_capacity(total_bytes);
        for _ in 0..chunk_size {
            result.extend_from_slice(&fill_bytes);
        }
        Ok(result)
    }

    /// Reads data from a shard
    fn read_from_shard(&self, shard_data: &[u8], coords: &[usize]) -> Result<Vec<u8>> {
        // Extract sharding configuration
        if let Some(codecs) = &self.metadata.codecs {
            for codec_meta in codecs {
                if let CodecMetadata::ShardingIndexed { configuration } = codec_meta {
                    // Build codec chains for shard
                    let (chunk_codec, index_codec) =
                        crate::sharding::parse_sharding_config(configuration)?;

                    let index_location = configuration
                        .index_location
                        .as_ref()
                        .and_then(|loc| IndexLocation::from_str(loc).ok())
                        .unwrap_or_default();

                    // Create shard reader
                    let shard_reader = ShardReader::new(
                        shard_data.to_vec(),
                        configuration.chunk_shape.clone(),
                        chunk_codec,
                        index_codec,
                        index_location,
                    )?;

                    // Read chunk from shard
                    if let Some(data) = shard_reader.read_chunk(coords)? {
                        return Ok(data);
                    }
                    // Chunk not in shard, return fill value
                    return self.create_fill_chunk();
                }
            }
        }

        Err(ZarrError::Internal {
            message: "Sharding configuration not found".to_string(),
        })
    }

    /// Calculates which chunks overlap with the given ranges
    fn calculate_chunk_ranges(
        &self,
        ranges: &[std::ops::Range<usize>],
        chunk_shape: &[usize],
    ) -> Result<Vec<Vec<usize>>> {
        let mut chunk_coords = Vec::new();

        // Calculate chunk coordinate ranges
        let chunk_ranges: Vec<_> = ranges
            .iter()
            .zip(chunk_shape.iter())
            .map(|(range, &chunk_size)| {
                let start_chunk = range.start / chunk_size;
                let end_chunk = range.end.div_ceil(chunk_size);
                start_chunk..end_chunk
            })
            .collect();

        // Generate all chunk coordinate combinations
        fn generate_coords(
            ranges: &[std::ops::Range<usize>],
            current: Vec<usize>,
            result: &mut Vec<Vec<usize>>,
        ) {
            if current.len() == ranges.len() {
                result.push(current);
                return;
            }

            let dim = current.len();
            for coord in ranges[dim].clone() {
                let mut next = current.clone();
                next.push(coord);
                generate_coords(ranges, next, result);
            }
        }

        generate_coords(&chunk_ranges, Vec::new(), &mut chunk_coords);

        Ok(chunk_coords)
    }
}

/// Computes row-major (C order) strides for an array shape
///
/// The stride for dimension i is the product of all subsequent dimensions.
/// For shape [3, 4, 5], strides are [20, 5, 1].
fn compute_strides(shape: &[usize]) -> Vec<usize> {
    let ndim = shape.len();
    let mut strides = vec![1usize; ndim];
    for dim in (0..ndim.saturating_sub(1)).rev() {
        strides[dim] = strides[dim + 1] * shape[dim + 1];
    }
    strides
}

/// Builds a codec from metadata
fn build_codec_from_metadata(metadata: &CodecMetadata) -> Result<Box<dyn Codec>> {
    use crate::codecs::NullCodec;

    // This is a simplified implementation
    // In production, dispatch to appropriate codec based on metadata
    match metadata {
        CodecMetadata::Gzip { .. } => {
            #[cfg(feature = "gzip")]
            {
                use crate::codecs::gzip::GzipCodec;
                Ok(Box::new(GzipCodec::new(6)?))
            }
            #[cfg(not(feature = "gzip"))]
            {
                Err(ZarrError::NotSupported {
                    operation: "gzip codec".to_string(),
                })
            }
        }
        CodecMetadata::Zstd { .. } => {
            #[cfg(feature = "zstd")]
            {
                use crate::codecs::zstd_codec::ZstdCodec;
                Ok(Box::new(ZstdCodec::new(3)?))
            }
            #[cfg(not(feature = "zstd"))]
            {
                Err(ZarrError::NotSupported {
                    operation: "zstd codec".to_string(),
                })
            }
        }
        CodecMetadata::Bytes { .. } => Ok(Box::new(NullCodec)),
        _ => Ok(Box::new(NullCodec)),
    }
}

/// Builds a transformer from metadata
fn build_transformer_from_metadata(
    _metadata: &crate::metadata::v3::StorageTransformer,
) -> Result<Box<dyn Transformer>> {
    use crate::transformers::NoOpTransformer;

    // This is a simplified implementation
    // In production, dispatch to appropriate transformer based on metadata
    Ok(Box::new(NoOpTransformer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::memory::MemoryStore;

    #[test]
    fn test_build_chunk_key_default() {
        let mut store = MemoryStore::new();
        let metadata = ArrayMetadataV3::new(vec![100, 200], vec![10, 20], "float32");

        // Create a simple JSON for testing
        let metadata_json = serde_json::to_vec(&metadata).expect("serialize");
        store
            .set(&StoreKey::new("test/zarr.json".to_string()), &metadata_json)
            .expect("set");

        let reader = ZarrV3Reader::new(store, "test").expect("create reader");
        let key = reader.build_chunk_key(&[0, 1]).expect("build key");

        assert!(key.contains("test"));
        assert!(key.contains('0'));
        assert!(key.contains('1'));
    }

    #[test]
    fn test_create_fill_chunk_zeros() {
        let mut store = MemoryStore::new();
        let metadata = ArrayMetadataV3::new(vec![100, 200], vec![10, 20], "float32");

        let metadata_json = serde_json::to_vec(&metadata).expect("serialize");
        store
            .set(&StoreKey::new("test/zarr.json".to_string()), &metadata_json)
            .expect("set");

        let reader = ZarrV3Reader::new(store, "test").expect("create reader");
        let fill_chunk = reader.create_fill_chunk().expect("fill chunk");

        let expected_size = 10 * 20 * 4; // chunk_shape * item_size
        assert_eq!(fill_chunk.len(), expected_size);
        // Default fill value is Null, which encodes as zeros
        assert!(fill_chunk.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_create_fill_chunk_with_value() {
        use crate::metadata::v3::FillValue;

        let mut store = MemoryStore::new();
        let metadata = ArrayMetadataV3::new(vec![100, 200], vec![2, 3], "float32")
            .with_fill_value(FillValue::Float(42.0));

        let metadata_json = serde_json::to_vec(&metadata).expect("serialize");
        store
            .set(&StoreKey::new("test/zarr.json".to_string()), &metadata_json)
            .expect("set");

        let reader = ZarrV3Reader::new(store, "test").expect("create reader");
        let fill_chunk = reader.create_fill_chunk().expect("fill chunk");

        let expected_size = 2 * 3 * 4; // chunk_shape * item_size(float32=4)
        assert_eq!(fill_chunk.len(), expected_size);

        // Each 4-byte element should be 42.0f32 in little-endian
        let expected_bytes = 42.0f32.to_le_bytes();
        for i in 0..6 {
            let offset = i * 4;
            assert_eq!(
                &fill_chunk[offset..offset + 4],
                &expected_bytes,
                "element {} should be 42.0f32",
                i
            );
        }
    }

    #[test]
    fn test_compute_strides() {
        let strides = compute_strides(&[3, 4, 5]);
        assert_eq!(strides, vec![20, 5, 1]);

        let strides = compute_strides(&[10, 20]);
        assert_eq!(strides, vec![20, 1]);

        let strides = compute_strides(&[5]);
        assert_eq!(strides, vec![1]);
    }
}
