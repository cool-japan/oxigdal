//! Zarr v3 array reader implementation
//!
//! This module provides a comprehensive reader for Zarr v3 arrays,
//! including codec pipeline support, sharding, and storage transformers.

use crate::codecs::CodecChain;
use crate::codecs::dispatch::build_codec_from_metadata;
use crate::error::{Result, StorageError, ZarrError};
use crate::metadata::v3::{ArrayMetadataV3, CodecMetadata, ShardingConfig};
use crate::sharding::{IndexLocation, ShardReader};
use crate::storage::{Store, StoreKey};
use crate::transformers::{TransformerChain, build_transformer_from_metadata};
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
        if let Some(cache) = &self.cache
            && let Some(data) = cache.get(coords)
        {
            return Ok(data.clone());
        }

        // For sharded arrays, delegate to the sharding read path BEFORE building
        // the chunk key so that shard-file coords vs inner-chunk coords are kept
        // distinct.
        if let Some(shard_config) = self.find_sharding_config() {
            let shard_config = shard_config.clone();
            return self.read_sharded_chunk(coords, &shard_config);
        }

        // Non-sharded path: build chunk key and read directly
        let chunk_key = self.build_chunk_key(coords)?;
        let encoded_data = match self.store.get(&StoreKey::new(chunk_key)) {
            Ok(data) => data,
            // Only a genuinely absent chunk yields the fill value. Every other
            // storage failure (network, permission, throttling, corrupted
            // backend response) must propagate -- silently returning fill data
            // for a transient I/O error is data corruption for any consumer
            // that cannot tell the difference.
            Err(ZarrError::Storage(StorageError::KeyNotFound { .. })) => {
                return self.create_fill_chunk();
            }
            Err(e) => return Err(e),
        };

        // Apply storage transformers (decode)
        let transformed_data = self.transformers.decode(encoded_data)?;

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

    /// Returns the sharding configuration if this array uses the
    /// `sharding_indexed` codec, otherwise `None`.
    fn find_sharding_config(&self) -> Option<&ShardingConfig> {
        self.metadata.codecs.as_ref()?.iter().find_map(|c| {
            if let CodecMetadata::ShardingIndexed { configuration } = c {
                Some(configuration)
            } else {
                None
            }
        })
    }

    /// Reads an inner chunk from a sharded array.
    ///
    /// `coords` are global inner-chunk coordinates at the finest grid
    /// resolution.  This method computes the shard-file coordinates
    /// (`shard_coords = coords / chunks_per_shard`) and the coordinates
    /// within the shard (`inner_coords = coords % chunks_per_shard`) before
    /// fetching the shard file and delegating to [`ShardReader`].
    fn read_sharded_chunk(
        &self,
        coords: &[usize],
        shard_config: &ShardingConfig,
    ) -> Result<Vec<u8>> {
        let chunks_per_shard = &shard_config.chunk_shape;

        if coords.len() != chunks_per_shard.len() {
            return Err(ZarrError::InvalidDimension {
                message: format!(
                    "coords ndim {} != chunks_per_shard ndim {}",
                    coords.len(),
                    chunks_per_shard.len()
                ),
            });
        }

        // Compute which shard file to read
        let shard_coords: Vec<usize> = coords
            .iter()
            .zip(chunks_per_shard.iter())
            .map(|(&c, &n)| c / n)
            .collect();

        // Compute position of inner chunk within the shard
        let inner_coords: Vec<usize> = coords
            .iter()
            .zip(chunks_per_shard.iter())
            .map(|(&c, &n)| c % n)
            .collect();

        let shard_key = self.build_chunk_key(&shard_coords)?;

        // Build codec chains for inner chunks and index
        let (chunk_codec, index_codec) = crate::sharding::parse_sharding_config(shard_config)?;

        let index_location = shard_config
            .index_location
            .as_deref()
            .and_then(|loc| IndexLocation::from_str(loc).ok())
            .unwrap_or_default();

        // Cloud-efficient fast path: when there are no storage transformers
        // (which would require the whole shard object to decode), read only
        // the shard's fixed-size index and then only the one inner chunk via
        // storage byte-range reads, instead of downloading the whole shard.
        if self.transformers.is_empty() {
            let key = StoreKey::new(shard_key);
            return match crate::sharding::read_inner_chunk_ranged(
                self.store.as_ref(),
                &key,
                &inner_coords,
                chunks_per_shard,
                &chunk_codec,
                &index_codec,
                index_location,
            ) {
                Ok(Some(data)) => Ok(data),
                Ok(None) => self.create_fill_chunk(),
                // A genuinely absent shard yields fill values; every other
                // storage error must propagate.
                Err(ZarrError::Storage(StorageError::KeyNotFound { .. })) => {
                    self.create_fill_chunk()
                }
                Err(e) => Err(e),
            };
        }

        // Slow path: storage transformers (e.g. encryption) require the whole
        // shard object before the index can be parsed.
        let shard_bytes = match self.store.get(&StoreKey::new(shard_key)) {
            Ok(data) => data,
            Err(ZarrError::Storage(StorageError::KeyNotFound { .. })) => {
                return self.create_fill_chunk();
            }
            Err(e) => return Err(e),
        };

        // Apply storage transformers to shard bytes
        let transformed = self.transformers.decode(shard_bytes)?;

        // Create shard reader (parses the index footer/header)
        let shard_reader = ShardReader::new(
            transformed,
            chunks_per_shard.clone(),
            chunk_codec,
            index_codec,
            index_location,
        )?;

        // Read inner chunk using inner (shard-relative) coordinates
        match shard_reader.read_chunk(&inner_coords)? {
            Some(data) => Ok(data),
            None => self.create_fill_chunk(),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::memory::MemoryStore;

    /// A store that serves metadata normally but fails every *chunk* read with
    /// a transient network error (not KeyNotFound), used to prove the reader
    /// propagates real I/O failures instead of masking them as fill values.
    struct ChunkFaultStore {
        inner: MemoryStore,
    }

    impl crate::storage::Store for ChunkFaultStore {
        fn exists(&self, key: &StoreKey) -> Result<bool> {
            self.inner.exists(key)
        }
        fn get(&self, key: &StoreKey) -> Result<Vec<u8>> {
            if key.as_str().ends_with("zarr.json") {
                self.inner.get(key)
            } else {
                Err(ZarrError::Storage(StorageError::Network {
                    message: "simulated transient failure".to_string(),
                }))
            }
        }
        fn set(&mut self, key: &StoreKey, value: &[u8]) -> Result<()> {
            self.inner.set(key, value)
        }
        fn delete(&mut self, key: &StoreKey) -> Result<()> {
            self.inner.delete(key)
        }
        fn list_prefix(&self, prefix: &StoreKey) -> Result<Vec<StoreKey>> {
            self.inner.list_prefix(prefix)
        }
    }

    #[test]
    fn test_read_chunk_propagates_storage_error_not_fill() {
        let mut inner = MemoryStore::new();
        let metadata = ArrayMetadataV3::new(vec![4], vec![2], "float32");
        let metadata_json = serde_json::to_vec(&metadata).expect("serialize");
        inner
            .set(&StoreKey::new("arr/zarr.json".to_string()), &metadata_json)
            .expect("set meta");

        let store = ChunkFaultStore { inner };
        let reader = ZarrV3Reader::new(store, "arr").expect("create reader");

        // The chunk read must surface the network error, NOT silently return a
        // plausible-looking fill chunk.
        let result = reader.read_chunk(&[0]);
        assert!(
            matches!(
                result,
                Err(ZarrError::Storage(StorageError::Network { .. }))
            ),
            "expected the transient storage error to propagate, got {result:?}"
        );
    }

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

    #[test]
    fn test_zarr_v3_reader_sharded_array() {
        use crate::codecs::CodecChain;
        use crate::sharding::{IndexLocation, ShardWriter};

        // Build a sharded array: shape=[4], outer_chunk=[4] (1 shard), chunks_per_shard=[2]
        // So 2 inner chunks of 2 elements each (f32 = 4 bytes each = 8 bytes per inner chunk)
        let mut store = MemoryStore::new();

        let shard_config = ShardingConfig {
            chunk_shape: vec![2],
            codecs: vec![CodecMetadata::Bytes {
                configuration: None,
            }],
            index_codecs: vec![CodecMetadata::Bytes {
                configuration: None,
            }],
            index_location: Some("end".to_string()),
        };
        let codecs = vec![CodecMetadata::ShardingIndexed {
            configuration: shard_config.clone(),
        }];
        let metadata = ArrayMetadataV3::new(vec![4], vec![4], "float32").with_codecs(codecs);

        let metadata_json = serde_json::to_vec(&metadata).expect("serialize");
        store
            .set(&StoreKey::new("arr/zarr.json".to_string()), &metadata_json)
            .expect("set metadata");

        // Build shard: inner_chunk[0] = [1.0f32, 2.0f32], inner_chunk[1] = [3.0f32, 4.0f32]
        let inner0: Vec<u8> = [1.0f32, 2.0f32]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let inner1: Vec<u8> = [3.0f32, 4.0f32]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();

        let mut writer = ShardWriter::new(
            vec![2],
            CodecChain::empty(),
            CodecChain::empty(),
            IndexLocation::End,
        );
        writer
            .write_chunk(vec![0], inner0.clone())
            .expect("write inner 0");
        writer
            .write_chunk(vec![1], inner1.clone())
            .expect("write inner 1");
        let shard_bytes = writer.finalize().expect("finalize shard");

        // Store shard at key "arr/c/0" (default chunk key encoding for coords [0])
        store
            .set(&StoreKey::new("arr/c/0".to_string()), &shard_bytes)
            .expect("set shard");

        let reader = ZarrV3Reader::new(store, "arr").expect("create reader");

        // read_chunk([0]) should return inner_chunk[0] (first 2 f32 values)
        let data0 = reader.read_chunk(&[0]).expect("read chunk 0");
        assert_eq!(data0, inner0, "inner chunk 0 mismatch");

        // read_chunk([1]) should return inner_chunk[1] (second 2 f32 values)
        let data1 = reader.read_chunk(&[1]).expect("read chunk 1");
        assert_eq!(data1, inner1, "inner chunk 1 mismatch");
    }

    #[test]
    fn test_zarr_v3_reader_blosc_codec_never_silently_passes_through() {
        use crate::metadata::v3::BloscConfig;

        // An intentionally-invalid Blosc compressor name guarantees a
        // deterministic `Err` from `BloscCodec::new` whether or not the
        // `blosc` cargo feature happens to be enabled: with the feature on,
        // construction itself fails on the bad `cname`; with it off, the
        // dispatcher returns `CodecNotAvailable` before ever looking at
        // `cname`. Either way, opening a reader over an array declaring
        // `blosc` compression must error -- it must never silently
        // construct an identity codec and hand back still-compressed bytes
        // as if they were decoded.
        let mut store = MemoryStore::new();
        let metadata = ArrayMetadataV3::new(vec![10], vec![10], "uint8").with_codecs(vec![
            CodecMetadata::Blosc {
                configuration: BloscConfig {
                    cname: "not-a-real-blosc-compressor".to_string(),
                    clevel: 5,
                    shuffle: 0,
                    typesize: None,
                    blocksize: None,
                },
            },
        ]);

        let metadata_json = serde_json::to_vec(&metadata).expect("serialize");
        store
            .set(&StoreKey::new("arr/zarr.json".to_string()), &metadata_json)
            .expect("set metadata");

        let result = ZarrV3Reader::new(store, "arr");
        assert!(
            result.is_err(),
            "reader construction over a blosc-compressed array must error, not silently \
             build an identity codec"
        );
    }

    #[test]
    fn test_zarr_v3_reader_encryption_transformer_never_silently_no_ops() {
        use crate::metadata::v3::{EncryptionConfig, StorageTransformer};

        // No `key` parameter is supplied, so the encryption transformer
        // cannot be constructed. Opening the reader must fail loudly rather
        // than silently substituting a NoOpTransformer -- otherwise
        // encrypted-at-rest chunk data would be handed back to callers as
        // if it were plaintext.
        let mut store = MemoryStore::new();
        let metadata =
            ArrayMetadataV3::new(vec![10], vec![10], "uint8").with_storage_transformers(vec![
                StorageTransformer::Encryption {
                    configuration: EncryptionConfig {
                        algorithm: "AES-256-GCM".to_string(),
                        key_id: "test-key".to_string(),
                        params: HashMap::new(),
                    },
                },
            ]);

        let metadata_json = serde_json::to_vec(&metadata).expect("serialize");
        store
            .set(&StoreKey::new("arr/zarr.json".to_string()), &metadata_json)
            .expect("set metadata");

        let result = ZarrV3Reader::new(store, "arr");
        assert!(
            result.is_err(),
            "reader construction over an encrypted array with no key material must error, \
             not silently build a NoOpTransformer"
        );
    }

    #[test]
    fn test_zarr_v3_reader_encryption_transformer_roundtrips_with_key() {
        use crate::metadata::v3::{EncryptionConfig, StorageTransformer};
        use crate::transformers::{AesGcmTransformer, TransformerChain};

        // Full round trip: a chunk encrypted with the same key material
        // declared in `storage_transformers` metadata must be readable back
        // out through the public reader API (not just at the transformer
        // unit level).
        let mut store = MemoryStore::new();
        let mut params = HashMap::new();
        params.insert(
            "key".to_string(),
            serde_json::Value::String("ab".repeat(32)),
        );
        let metadata =
            ArrayMetadataV3::new(vec![2], vec![2], "float32").with_storage_transformers(vec![
                StorageTransformer::Encryption {
                    configuration: EncryptionConfig {
                        algorithm: "AES-256-GCM".to_string(),
                        key_id: "test-key".to_string(),
                        params,
                    },
                },
            ]);

        let metadata_json = serde_json::to_vec(&metadata).expect("serialize");
        store
            .set(&StoreKey::new("arr/zarr.json".to_string()), &metadata_json)
            .expect("set metadata");

        let plaintext_chunk: Vec<u8> = [1.0f32, 2.0f32]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();

        let key = (0..32).map(|_| 0xAB).collect::<Vec<u8>>();
        let encryptor = AesGcmTransformer::new(key, "test-key").expect("build encryptor");
        let chain = TransformerChain::new(vec![Box::new(encryptor)]);
        let encrypted_chunk = chain
            .encode(plaintext_chunk.clone())
            .expect("encrypt chunk");
        assert_ne!(encrypted_chunk, plaintext_chunk);

        store
            .set(&StoreKey::new("arr/c/0".to_string()), &encrypted_chunk)
            .expect("set chunk");

        let reader = ZarrV3Reader::new(store, "arr").expect("create reader");
        let decoded = reader.read_chunk(&[0]).expect("read chunk");
        assert_eq!(decoded, plaintext_chunk);
    }
}
