//! Zarr v3 Sharding Extension
//!
//! This module implements the sharding extension for Zarr v3, which allows
//! multiple chunks to be stored together in a single "shard" for improved
//! performance with cloud storage.

use crate::codecs::{Codec, CodecChain};
use crate::error::{Result, ShardError, ZarrError};
use crate::metadata::v3::{CodecMetadata, ShardingConfig};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::collections::HashMap;
use std::io::Cursor;

/// Shard index entry
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShardIndexEntry {
    /// Offset of chunk data in shard
    pub offset: u64,
    /// Size of chunk data in bytes
    pub size: u64,
}

impl ShardIndexEntry {
    /// Creates a new shard index entry
    #[must_use]
    pub const fn new(offset: u64, size: u64) -> Self {
        Self { offset, size }
    }

    /// Returns true if this entry represents a missing chunk
    #[must_use]
    pub const fn is_missing(&self) -> bool {
        self.offset == u64::MAX && self.size == u64::MAX
    }

    /// Creates an entry representing a missing chunk
    #[must_use]
    pub const fn missing() -> Self {
        Self {
            offset: u64::MAX,
            size: u64::MAX,
        }
    }

    /// Encodes the entry to bytes
    ///
    /// # Errors
    /// Returns error if encoding fails
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(16);
        buf.write_u64::<LittleEndian>(self.offset)
            .map_err(|e| ZarrError::Shard(ShardError::IndexEncodeFailed { source: e }))?;
        buf.write_u64::<LittleEndian>(self.size)
            .map_err(|e| ZarrError::Shard(ShardError::IndexEncodeFailed { source: e }))?;
        Ok(buf)
    }

    /// Decodes the entry from bytes
    ///
    /// # Errors
    /// Returns error if decoding fails
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 16 {
            return Err(ZarrError::Shard(ShardError::InvalidIndexEntry {
                reason: format!("Expected 16 bytes, got {}", data.len()),
            }));
        }

        let mut cursor = Cursor::new(data);
        let offset = cursor
            .read_u64::<LittleEndian>()
            .map_err(|e| ZarrError::Shard(ShardError::IndexDecodeFailed { source: e }))?;
        let size = cursor
            .read_u64::<LittleEndian>()
            .map_err(|e| ZarrError::Shard(ShardError::IndexDecodeFailed { source: e }))?;

        Ok(Self { offset, size })
    }
}

/// Shard index - maps chunk coordinates to data locations
#[derive(Debug, Clone)]
pub struct ShardIndex {
    /// Index entries (flattened chunk coordinates -> entry)
    entries: Vec<ShardIndexEntry>,
    /// Chunks per shard dimension
    chunks_per_shard: Vec<usize>,
}

impl ShardIndex {
    /// Creates a new shard index
    #[must_use]
    pub fn new(chunks_per_shard: Vec<usize>) -> Self {
        let total_chunks: usize = chunks_per_shard.iter().product();
        Self {
            entries: vec![ShardIndexEntry::missing(); total_chunks],
            chunks_per_shard,
        }
    }

    /// Returns the number of chunks in the shard
    #[must_use]
    pub fn num_chunks(&self) -> usize {
        self.entries.len()
    }

    /// Converts chunk coordinates to flat index
    ///
    /// # Errors
    /// Returns error if coordinates are invalid
    pub fn coords_to_index(&self, coords: &[usize]) -> Result<usize> {
        if coords.len() != self.chunks_per_shard.len() {
            return Err(ZarrError::Shard(ShardError::InvalidChunkCoords {
                expected_dims: self.chunks_per_shard.len(),
                found_dims: coords.len(),
            }));
        }

        let mut index = 0;
        let mut stride = 1;
        for (i, &coord) in coords.iter().enumerate().rev() {
            if coord >= self.chunks_per_shard[i] {
                return Err(ZarrError::Shard(ShardError::ChunkOutOfBounds {
                    dim: i,
                    coord,
                    max: self.chunks_per_shard[i],
                }));
            }
            index += coord * stride;
            stride *= self.chunks_per_shard[i];
        }

        Ok(index)
    }

    /// Gets an entry for the given chunk coordinates
    ///
    /// # Errors
    /// Returns error if coordinates are invalid
    pub fn get(&self, coords: &[usize]) -> Result<ShardIndexEntry> {
        let index = self.coords_to_index(coords)?;
        Ok(self.entries[index])
    }

    /// Sets an entry for the given chunk coordinates
    ///
    /// # Errors
    /// Returns error if coordinates are invalid
    pub fn set(&mut self, coords: &[usize], entry: ShardIndexEntry) -> Result<()> {
        let index = self.coords_to_index(coords)?;
        self.entries[index] = entry;
        Ok(())
    }

    /// Encodes the index to bytes
    ///
    /// # Errors
    /// Returns error if encoding fails
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(self.entries.len() * 16);
        for entry in &self.entries {
            buf.extend_from_slice(&entry.encode()?);
        }
        Ok(buf)
    }

    /// Decodes the index from bytes
    ///
    /// # Errors
    /// Returns error if decoding fails
    pub fn decode(data: &[u8], chunks_per_shard: Vec<usize>) -> Result<Self> {
        let total_chunks: usize = chunks_per_shard.iter().product();
        let expected_size = total_chunks * 16;

        if data.len() != expected_size {
            return Err(ZarrError::Shard(ShardError::InvalidIndexSize {
                expected: expected_size,
                found: data.len(),
            }));
        }

        let mut entries = Vec::with_capacity(total_chunks);
        for i in 0..total_chunks {
            let start = i * 16;
            let end = start + 16;
            let entry = ShardIndexEntry::decode(&data[start..end])?;
            entries.push(entry);
        }

        Ok(Self {
            entries,
            chunks_per_shard,
        })
    }

    /// Returns an iterator over all entries with their coordinates
    pub fn iter(&self) -> ShardIndexIter<'_> {
        ShardIndexIter {
            index: self,
            current: 0,
        }
    }
}

/// Iterator over shard index entries
pub struct ShardIndexIter<'a> {
    index: &'a ShardIndex,
    current: usize,
}

impl<'a> Iterator for ShardIndexIter<'a> {
    type Item = (Vec<usize>, ShardIndexEntry);

    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.index.num_chunks() {
            return None;
        }

        // Convert flat index to coordinates
        let mut coords = Vec::with_capacity(self.index.chunks_per_shard.len());
        let mut idx = self.current;
        for &dim_size in self.index.chunks_per_shard.iter().rev() {
            coords.push(idx % dim_size);
            idx /= dim_size;
        }
        coords.reverse();

        let entry = self.index.entries[self.current];
        self.current += 1;

        Some((coords, entry))
    }
}

/// Shard reader - reads chunks from a shard
pub struct ShardReader {
    /// Shard data
    data: Vec<u8>,
    /// Shard index
    index: ShardIndex,
    /// Codec for sub-chunks
    codec: CodecChain,
    /// Index location ("start" or "end")
    index_location: IndexLocation,
}

impl ShardReader {
    /// Creates a new shard reader
    ///
    /// # Errors
    /// Returns error if initialization fails
    pub fn new(
        shard_data: Vec<u8>,
        chunks_per_shard: Vec<usize>,
        codec: CodecChain,
        index_codec: CodecChain,
        index_location: IndexLocation,
    ) -> Result<Self> {
        // Extract and decode index
        let index_data = match index_location {
            IndexLocation::Start => {
                // Index is at the beginning, followed by chunk data
                // For now, we'll implement end-based indexing
                return Err(ZarrError::Shard(ShardError::UnsupportedIndexLocation {
                    location: "start".to_string(),
                }));
            }
            IndexLocation::End => {
                // Index is at the end
                // Last 8 bytes contain the index size
                if shard_data.len() < 8 {
                    return Err(ZarrError::Shard(ShardError::InvalidShardData {
                        reason: "Shard data too small for index size".to_string(),
                    }));
                }

                let mut cursor = Cursor::new(&shard_data[shard_data.len() - 8..]);
                let index_size = cursor
                    .read_u64::<LittleEndian>()
                    .map_err(|e| ZarrError::Shard(ShardError::IndexDecodeFailed { source: e }))?
                    as usize;

                if shard_data.len() < index_size + 8 {
                    return Err(ZarrError::Shard(ShardError::InvalidShardData {
                        reason: format!(
                            "Shard data too small for index (need {}, got {})",
                            index_size + 8,
                            shard_data.len()
                        ),
                    }));
                }

                let index_start = shard_data.len() - index_size - 8;
                let index_end = shard_data.len() - 8;
                &shard_data[index_start..index_end]
            }
        };

        // Decode index using index codec
        let decoded_index = index_codec.decode(index_data.to_vec())?;
        let index = ShardIndex::decode(&decoded_index, chunks_per_shard)?;

        Ok(Self {
            data: shard_data,
            index,
            codec,
            index_location,
        })
    }

    /// Reads a chunk from the shard
    ///
    /// # Errors
    /// Returns error if chunk cannot be read
    pub fn read_chunk(&self, coords: &[usize]) -> Result<Option<Vec<u8>>> {
        let entry = self.index.get(coords)?;

        if entry.is_missing() {
            return Ok(None);
        }

        let offset = entry.offset as usize;
        let size = entry.size as usize;

        if offset + size > self.data.len() {
            return Err(ZarrError::Shard(ShardError::InvalidChunkRange {
                offset,
                size,
                shard_size: self.data.len(),
            }));
        }

        let compressed_data = self.data[offset..offset + size].to_vec();
        let decompressed_data = self.codec.decode(compressed_data)?;

        Ok(Some(decompressed_data))
    }

    /// Returns the shard index
    #[must_use]
    pub const fn index(&self) -> &ShardIndex {
        &self.index
    }
}

/// Shard writer - writes chunks to a shard
pub struct ShardWriter {
    /// Chunk data buffer
    chunks: HashMap<Vec<usize>, Vec<u8>>,
    /// Chunks per shard dimension
    chunks_per_shard: Vec<usize>,
    /// Codec for sub-chunks
    codec: CodecChain,
    /// Codec for index
    index_codec: CodecChain,
    /// Index location
    index_location: IndexLocation,
}

impl ShardWriter {
    /// Creates a new shard writer
    #[must_use]
    pub fn new(
        chunks_per_shard: Vec<usize>,
        codec: CodecChain,
        index_codec: CodecChain,
        index_location: IndexLocation,
    ) -> Self {
        Self {
            chunks: HashMap::new(),
            chunks_per_shard,
            codec,
            index_codec,
            index_location,
        }
    }

    /// Adds a chunk to the shard
    ///
    /// # Errors
    /// Returns error if chunk cannot be added
    pub fn write_chunk(&mut self, coords: Vec<usize>, data: Vec<u8>) -> Result<()> {
        // Validate coordinates
        if coords.len() != self.chunks_per_shard.len() {
            return Err(ZarrError::Shard(ShardError::InvalidChunkCoords {
                expected_dims: self.chunks_per_shard.len(),
                found_dims: coords.len(),
            }));
        }

        for (i, &coord) in coords.iter().enumerate() {
            if coord >= self.chunks_per_shard[i] {
                return Err(ZarrError::Shard(ShardError::ChunkOutOfBounds {
                    dim: i,
                    coord,
                    max: self.chunks_per_shard[i],
                }));
            }
        }

        self.chunks.insert(coords, data);
        Ok(())
    }

    /// Finalizes the shard and returns the encoded data
    ///
    /// # Errors
    /// Returns error if finalization fails
    pub fn finalize(self) -> Result<Vec<u8>> {
        let mut index = ShardIndex::new(self.chunks_per_shard.clone());
        let mut shard_data = Vec::new();

        // Sort chunks for deterministic output
        let mut sorted_coords: Vec<_> = self.chunks.keys().cloned().collect();
        sorted_coords.sort();

        // Write chunks and build index
        for coords in sorted_coords {
            if let Some(chunk_data) = self.chunks.get(&coords) {
                let offset = shard_data.len() as u64;
                let compressed = self.codec.encode(chunk_data.clone())?;
                let size = compressed.len() as u64;

                shard_data.extend_from_slice(&compressed);
                index.set(&coords, ShardIndexEntry::new(offset, size))?;
            }
        }

        // Encode and compress index
        let index_bytes = index.encode()?;
        let compressed_index = self.index_codec.encode(index_bytes)?;

        match self.index_location {
            IndexLocation::Start => {
                // Index at start: [index_size][index][chunks]
                let mut result = Vec::new();
                result
                    .write_u64::<LittleEndian>(compressed_index.len() as u64)
                    .map_err(|e| ZarrError::Shard(ShardError::IndexEncodeFailed { source: e }))?;
                result.extend_from_slice(&compressed_index);
                result.extend_from_slice(&shard_data);
                Ok(result)
            }
            IndexLocation::End => {
                // Index at end: [chunks][index][index_size]
                shard_data.extend_from_slice(&compressed_index);
                shard_data
                    .write_u64::<LittleEndian>(compressed_index.len() as u64)
                    .map_err(|e| ZarrError::Shard(ShardError::IndexEncodeFailed { source: e }))?;
                Ok(shard_data)
            }
        }
    }

    /// Returns the number of chunks currently in the shard
    #[must_use]
    pub fn num_chunks(&self) -> usize {
        self.chunks.len()
    }
}

/// Index location in shard
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndexLocation {
    /// Index at start of shard
    Start,
    /// Index at end of shard (default)
    #[default]
    End,
}

impl IndexLocation {
    /// Parses index location from string
    ///
    /// # Errors
    /// Returns error if location string is invalid
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "start" => Ok(Self::Start),
            "end" => Ok(Self::End),
            _ => Err(ZarrError::Shard(ShardError::InvalidIndexLocation {
                location: s.to_string(),
            })),
        }
    }

    /// Returns the string representation
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
        }
    }
}

/// Parses sharding configuration to extract codec chains
///
/// # Errors
/// Returns error if configuration is invalid
pub fn parse_sharding_config(config: &ShardingConfig) -> Result<(CodecChain, CodecChain)> {
    // Build codec chain for chunks
    let mut chunk_codecs = Vec::new();
    for codec_meta in &config.codecs {
        let codec = build_codec_from_metadata(codec_meta)?;
        chunk_codecs.push(codec);
    }
    let chunk_codec_chain = CodecChain::new(chunk_codecs);

    // Build codec chain for index
    let mut index_codecs = Vec::new();
    for codec_meta in &config.index_codecs {
        let codec = build_codec_from_metadata(codec_meta)?;
        index_codecs.push(codec);
    }
    let index_codec_chain = CodecChain::new(index_codecs);

    Ok((chunk_codec_chain, index_codec_chain))
}

/// Builds a codec from metadata
fn build_codec_from_metadata(_metadata: &CodecMetadata) -> Result<Box<dyn Codec>> {
    // This is a placeholder - actual implementation would use the codec registry
    // For now, return a simple codec based on the name
    use crate::codecs::NullCodec;
    Ok(Box::new(NullCodec))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shard_index_entry() {
        let entry = ShardIndexEntry::new(100, 50);
        assert_eq!(entry.offset, 100);
        assert_eq!(entry.size, 50);
        assert!(!entry.is_missing());

        let missing = ShardIndexEntry::missing();
        assert!(missing.is_missing());
    }

    #[test]
    fn test_shard_index_entry_encode_decode() {
        let entry = ShardIndexEntry::new(12345, 6789);
        let encoded = entry.encode().expect("encode");
        let decoded = ShardIndexEntry::decode(&encoded).expect("decode");
        assert_eq!(entry, decoded);
    }

    #[test]
    fn test_shard_index_coords_to_index() {
        let index = ShardIndex::new(vec![2, 3, 4]);

        assert_eq!(index.coords_to_index(&[0, 0, 0]).expect("idx"), 0);
        assert_eq!(index.coords_to_index(&[0, 0, 1]).expect("idx"), 1);
        assert_eq!(index.coords_to_index(&[0, 1, 0]).expect("idx"), 4);
        assert_eq!(index.coords_to_index(&[1, 0, 0]).expect("idx"), 12);
    }

    #[test]
    fn test_shard_index_set_get() {
        let mut index = ShardIndex::new(vec![2, 2]);
        let entry = ShardIndexEntry::new(100, 50);

        index.set(&[0, 1], entry).expect("set");
        assert_eq!(index.get(&[0, 1]).expect("get"), entry);
    }

    #[test]
    fn test_shard_index_encode_decode() {
        let mut index = ShardIndex::new(vec![2, 2]);
        index
            .set(&[0, 0], ShardIndexEntry::new(0, 10))
            .expect("set");
        index
            .set(&[0, 1], ShardIndexEntry::new(10, 20))
            .expect("set");
        index
            .set(&[1, 0], ShardIndexEntry::new(30, 15))
            .expect("set");

        let encoded = index.encode().expect("encode");
        let decoded = ShardIndex::decode(&encoded, vec![2, 2]).expect("decode");

        assert_eq!(
            decoded.get(&[0, 0]).expect("get"),
            ShardIndexEntry::new(0, 10)
        );
        assert_eq!(
            decoded.get(&[0, 1]).expect("get"),
            ShardIndexEntry::new(10, 20)
        );
        assert_eq!(
            decoded.get(&[1, 0]).expect("get"),
            ShardIndexEntry::new(30, 15)
        );
    }

    #[test]
    fn test_shard_writer_write_chunk() {
        let codec = CodecChain::empty();
        let index_codec = CodecChain::empty();
        let mut writer = ShardWriter::new(vec![2, 2], codec, index_codec, IndexLocation::End);

        let chunk_data = vec![1, 2, 3, 4];
        writer
            .write_chunk(vec![0, 0], chunk_data.clone())
            .expect("write");
        assert_eq!(writer.num_chunks(), 1);

        writer
            .write_chunk(vec![0, 1], chunk_data.clone())
            .expect("write");
        assert_eq!(writer.num_chunks(), 2);
    }

    #[test]
    fn test_shard_writer_invalid_coords() {
        let codec = CodecChain::empty();
        let index_codec = CodecChain::empty();
        let mut writer = ShardWriter::new(vec![2, 2], codec, index_codec, IndexLocation::End);

        // Wrong number of dimensions
        assert!(writer.write_chunk(vec![0], vec![1, 2, 3]).is_err());

        // Out of bounds
        assert!(writer.write_chunk(vec![2, 0], vec![1, 2, 3]).is_err());
    }

    #[test]
    fn test_index_location() {
        assert_eq!(
            IndexLocation::from_str("start").expect("start"),
            IndexLocation::Start
        );
        assert_eq!(
            IndexLocation::from_str("end").expect("end"),
            IndexLocation::End
        );
        assert!(IndexLocation::from_str("middle").is_err());

        assert_eq!(IndexLocation::Start.as_str(), "start");
        assert_eq!(IndexLocation::End.as_str(), "end");
    }
}
