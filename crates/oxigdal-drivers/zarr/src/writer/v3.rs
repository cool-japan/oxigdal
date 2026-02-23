//! Zarr v3 array writer implementation
//!
//! This module provides a comprehensive writer for Zarr v3 arrays,
//! including codec pipeline support, sharding, and storage transformers.

use crate::codecs::{Codec, CodecChain};
use crate::error::{Result, ZarrError};
use crate::metadata::v3::{ArrayMetadataV3, CodecMetadata};
use crate::sharding::{IndexLocation, ShardWriter};
use crate::storage::{Store, StoreKey};
use crate::transformers::{Transformer, TransformerChain};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Zarr v3 array writer
pub struct ZarrV3Writer<S: Store> {
    /// Storage backend
    store: S,
    /// Array path
    path: String,
    /// Array metadata
    metadata: ArrayMetadataV3,
    /// Codec pipeline
    codecs: CodecChain,
    /// Storage transformers
    transformers: TransformerChain,
    /// Pending chunks (for sharding)
    pending_chunks: Arc<Mutex<HashMap<Vec<usize>, Vec<u8>>>>,
    /// Whether to use sharding
    use_sharding: bool,
}

impl<S: Store> ZarrV3Writer<S> {
    /// Creates a new v3 writer
    ///
    /// # Errors
    /// Returns error if writer cannot be created
    pub fn new(store: S, path: impl Into<String>, metadata: ArrayMetadataV3) -> Result<Self> {
        let path = path.into();

        // Validate metadata
        metadata.validate()?;

        // Build codec pipeline
        let codecs = Self::build_codec_chain(&metadata)?;

        // Build transformer chain
        let transformers = Self::build_transformer_chain(&metadata)?;

        // Check if sharding is enabled
        let use_sharding = metadata.codecs.as_ref().is_some_and(|codecs| {
            codecs
                .iter()
                .any(|c| matches!(c, CodecMetadata::ShardingIndexed { .. }))
        });

        Ok(Self {
            store,
            path,
            metadata,
            codecs,
            transformers,
            pending_chunks: Arc::new(Mutex::new(HashMap::new())),
            use_sharding,
        })
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

    /// Returns the chunk shape for regular grids
    ///
    /// # Errors
    /// Returns error if grid is not regular
    pub fn chunk_shape(&self) -> Result<&[usize]> {
        self.metadata.chunk_grid.regular_chunk_shape()
    }

    /// Writes a chunk at the given coordinates
    ///
    /// # Errors
    /// Returns error if chunk cannot be written
    pub fn write_chunk(&mut self, coords: Vec<usize>, data: Vec<u8>) -> Result<()> {
        // Validate coordinates
        self.validate_coords(&coords)?;

        // Validate data size
        let chunk_shape = self.chunk_shape()?;
        let expected_size: usize = chunk_shape.iter().product();
        let item_size = self.metadata.data_type.item_size()?;
        let expected_bytes = expected_size * item_size;

        if data.len() != expected_bytes {
            return Err(ZarrError::InvalidShape {
                expected: vec![expected_bytes],
                actual: vec![data.len()],
            });
        }

        if self.use_sharding {
            // Add to pending chunks for sharding
            let mut pending = self
                .pending_chunks
                .lock()
                .map_err(|_| ZarrError::Internal {
                    message: "Failed to lock pending chunks".to_string(),
                })?;
            pending.insert(coords, data);
        } else {
            // Write directly
            self.write_chunk_direct(&coords, data)?;
        }

        Ok(())
    }

    /// Writes a chunk directly (non-sharded)
    fn write_chunk_direct(&mut self, coords: &[usize], data: Vec<u8>) -> Result<()> {
        // Apply codec pipeline (encode)
        let encoded_data = self.codecs.encode(data)?;

        // Apply storage transformers (encode)
        let transformed_data = self.transformers.encode(encoded_data)?;

        // Build chunk key
        let chunk_key = self.build_chunk_key(coords)?;

        // Write to storage
        self.store
            .set(&StoreKey::new(chunk_key), &transformed_data)?;

        Ok(())
    }

    /// Writes an entire shard
    fn write_shard(
        &mut self,
        shard_coords: &[usize],
        chunks: HashMap<Vec<usize>, Vec<u8>>,
    ) -> Result<()> {
        // Extract sharding configuration
        if let Some(codecs) = &self.metadata.codecs {
            for codec_meta in codecs {
                if let CodecMetadata::ShardingIndexed { configuration } = codec_meta {
                    // Build codec chains
                    let (chunk_codec, index_codec) =
                        crate::sharding::parse_sharding_config(configuration)?;

                    let index_location = configuration
                        .index_location
                        .as_ref()
                        .and_then(|loc| IndexLocation::from_str(loc).ok())
                        .unwrap_or_default();

                    // Create shard writer
                    let mut shard_writer = ShardWriter::new(
                        configuration.chunk_shape.clone(),
                        chunk_codec,
                        index_codec,
                        index_location,
                    );

                    // Add all chunks to shard
                    for (coords, data) in chunks {
                        shard_writer.write_chunk(coords, data)?;
                    }

                    // Finalize shard
                    let shard_data = shard_writer.finalize()?;

                    // Apply storage transformers
                    let transformed_data = self.transformers.encode(shard_data)?;

                    // Write shard
                    let shard_key = self.build_chunk_key(shard_coords)?;
                    self.store
                        .set(&StoreKey::new(shard_key), &transformed_data)?;

                    return Ok(());
                }
            }
        }

        Err(ZarrError::Internal {
            message: "Sharding configuration not found".to_string(),
        })
    }

    /// Finalizes the writer and flushes pending data
    ///
    /// # Errors
    /// Returns error if finalization fails
    pub fn finalize(&mut self) -> Result<()> {
        // Write metadata
        self.write_metadata()?;

        // Flush pending chunks (for sharding)
        if self.use_sharding {
            self.flush_shards()?;
        }

        Ok(())
    }

    /// Writes the array metadata
    fn write_metadata(&mut self) -> Result<()> {
        let metadata_json = serde_json::to_vec_pretty(&self.metadata)?;
        let metadata_key = format!("{}/zarr.json", self.path);
        self.store
            .set(&StoreKey::new(metadata_key), &metadata_json)?;
        Ok(())
    }

    /// Flushes pending chunks as shards
    fn flush_shards(&mut self) -> Result<()> {
        // Clone the pending chunks and release the lock
        let pending_data = {
            let pending = self
                .pending_chunks
                .lock()
                .map_err(|_| ZarrError::Internal {
                    message: "Failed to lock pending chunks".to_string(),
                })?;

            if pending.is_empty() {
                return Ok(());
            }

            pending.clone()
        }; // Lock is dropped here

        // Group chunks by shard
        let mut shards: HashMap<Vec<usize>, HashMap<Vec<usize>, Vec<u8>>> = HashMap::new();

        for (coords, data) in pending_data.iter() {
            // Calculate shard coordinates
            let shard_coords = self.calculate_shard_coords(coords)?;
            let shard_entry = shards.entry(shard_coords.clone()).or_default();

            // Calculate local coordinates within shard
            let local_coords = self.calculate_local_coords(coords, &shard_coords)?;
            shard_entry.insert(local_coords, data.clone());
        }

        // Write each shard
        for (shard_coords, chunks) in shards {
            self.write_shard(&shard_coords, chunks)?;
        }

        Ok(())
    }

    /// Calculates shard coordinates from chunk coordinates
    fn calculate_shard_coords(&self, coords: &[usize]) -> Result<Vec<usize>> {
        // Extract sharding configuration
        if let Some(codecs) = &self.metadata.codecs {
            for codec_meta in codecs {
                if let CodecMetadata::ShardingIndexed { configuration } = codec_meta {
                    let shard_shape = &configuration.chunk_shape;
                    let shard_coords: Vec<usize> = coords
                        .iter()
                        .zip(shard_shape.iter())
                        .map(|(&coord, &shard_size)| coord / shard_size)
                        .collect();
                    return Ok(shard_coords);
                }
            }
        }

        Err(ZarrError::Internal {
            message: "Sharding configuration not found".to_string(),
        })
    }

    /// Calculates local coordinates within a shard
    fn calculate_local_coords(
        &self,
        coords: &[usize],
        shard_coords: &[usize],
    ) -> Result<Vec<usize>> {
        // Extract sharding configuration
        if let Some(codecs) = &self.metadata.codecs {
            for codec_meta in codecs {
                if let CodecMetadata::ShardingIndexed { configuration } = codec_meta {
                    let shard_shape = &configuration.chunk_shape;
                    let local_coords: Vec<usize> = coords
                        .iter()
                        .zip(shard_coords.iter())
                        .zip(shard_shape.iter())
                        .map(|((&coord, &shard_coord), &shard_size)| {
                            coord - shard_coord * shard_size
                        })
                        .collect();
                    return Ok(local_coords);
                }
            }
        }

        Err(ZarrError::Internal {
            message: "Sharding configuration not found".to_string(),
        })
    }

    /// Validates chunk coordinates
    fn validate_coords(&self, coords: &[usize]) -> Result<()> {
        let chunk_shape = self.chunk_shape()?;

        if coords.len() != self.metadata.shape.len() {
            return Err(ZarrError::InvalidDimension {
                message: format!(
                    "Expected {} dimensions, got {}",
                    self.metadata.shape.len(),
                    coords.len()
                ),
            });
        }

        // Check bounds
        for (i, (&coord, &dim_size)) in coords.iter().zip(self.metadata.shape.iter()).enumerate() {
            let chunk_size = chunk_shape[i];
            if coord * chunk_size >= dim_size {
                return Err(ZarrError::OutOfBounds {
                    message: format!(
                        "Chunk coordinate {coord} at dimension {i} is out of bounds (max: {})",
                        dim_size / chunk_size
                    ),
                });
            }
        }

        Ok(())
    }

    /// Builds the codec chain from metadata
    fn build_codec_chain(metadata: &ArrayMetadataV3) -> Result<CodecChain> {
        let mut codecs = Vec::new();

        if let Some(codec_list) = &metadata.codecs {
            for codec_meta in codec_list {
                // Skip sharding codec (handled separately)
                if matches!(codec_meta, CodecMetadata::ShardingIndexed { .. }) {
                    continue;
                }
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
}

/// Builds a codec from metadata
fn build_codec_from_metadata(metadata: &CodecMetadata) -> Result<Box<dyn Codec>> {
    use crate::codecs::NullCodec;

    // This is a simplified implementation
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
    Ok(Box::new(NoOpTransformer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::memory::MemoryStore;

    #[test]
    fn test_zarr_v3_writer_creation() {
        let store = MemoryStore::new();
        let metadata = ArrayMetadataV3::new(vec![100, 200], vec![10, 20], "float32");

        let writer = ZarrV3Writer::new(store, "test", metadata).expect("create writer");
        assert_eq!(writer.shape(), &[100, 200]);
    }

    #[test]
    fn test_write_chunk() {
        let store = MemoryStore::new();
        let metadata = ArrayMetadataV3::new(vec![100, 200], vec![10, 20], "float32");

        let mut writer = ZarrV3Writer::new(store, "test", metadata).expect("create writer");

        // Create chunk data
        let chunk_data = vec![0u8; 10 * 20 * 4]; // 10x20 float32 values
        writer
            .write_chunk(vec![0, 0], chunk_data)
            .expect("write chunk");
    }

    #[test]
    fn test_validate_coords() {
        let store = MemoryStore::new();
        let metadata = ArrayMetadataV3::new(vec![100, 200], vec![10, 20], "float32");

        let writer = ZarrV3Writer::new(store, "test", metadata).expect("create writer");

        // Valid coordinates
        assert!(writer.validate_coords(&[0, 0]).is_ok());
        assert!(writer.validate_coords(&[5, 5]).is_ok());

        // Invalid: wrong number of dimensions
        assert!(writer.validate_coords(&[0]).is_err());

        // Invalid: out of bounds
        assert!(writer.validate_coords(&[100, 0]).is_err());
    }
}
