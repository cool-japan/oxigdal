//! Zarr v3 Sharding Extension
//!
//! This module implements the sharding extension for Zarr v3, which allows
//! multiple chunks to be stored together in a single "shard" for improved
//! performance with cloud storage.

use crate::codecs::CodecChain;
use crate::codecs::dispatch::build_codec_from_metadata;
use crate::error::{Result, ShardError, ZarrError};
use crate::metadata::v3::ShardingConfig;
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
        // Per ZEP-0002: the index occupies exactly n_inner_chunks * 16 bytes.
        // There is no trailing/leading size field — the size is determined purely
        // by the product of chunks_per_shard dimensions.
        let n_inner_chunks: usize = chunks_per_shard.iter().product();
        let index_byte_len = n_inner_chunks * 16;

        // Extract and decode index
        let index_data: &[u8] = match index_location {
            IndexLocation::Start => {
                // Index at the beginning: shard = [index_bytes][chunk_data]
                if shard_data.len() < index_byte_len {
                    return Err(ZarrError::Shard(ShardError::InvalidShardData {
                        reason: format!(
                            "Shard data too small for start index (need {}, got {})",
                            index_byte_len,
                            shard_data.len()
                        ),
                    }));
                }
                &shard_data[..index_byte_len]
            }
            IndexLocation::End => {
                // Index at the end: shard = [chunk_data][index_bytes]
                if shard_data.len() < index_byte_len {
                    return Err(ZarrError::Shard(ShardError::InvalidShardData {
                        reason: format!(
                            "Shard data too small for end index (need {}, got {})",
                            index_byte_len,
                            shard_data.len()
                        ),
                    }));
                }
                let index_start = shard_data.len() - index_byte_len;
                &shard_data[index_start..]
            }
        };

        // Decode index using index codec
        let decoded_index = index_codec.decode(index_data.to_vec())?;
        let index = ShardIndex::decode(&decoded_index, chunks_per_shard)?;

        Ok(Self {
            data: shard_data,
            index,
            codec,
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

        // `entry.offset`/`entry.size` come straight from untrusted shard-file
        // bytes (only the exact `is_missing()` sentinel is validated above),
        // so both the `u64 -> usize` casts and the `offset + size` addition
        // must be checked -- a corrupted or malicious shard could otherwise
        // wrap the addition (spuriously passing the bounds check) or, on
        // 32-bit targets, silently truncate a huge `u64` into a small,
        // in-bounds `usize`. Either failure mode would previously panic on
        // the subsequent slice index instead of returning a typed error.
        let offset = usize::try_from(entry.offset).map_err(|_| {
            ZarrError::Shard(ShardError::InvalidChunkRange {
                offset: usize::MAX,
                size: usize::try_from(entry.size).unwrap_or(usize::MAX),
                shard_size: self.data.len(),
            })
        })?;
        let size = usize::try_from(entry.size).map_err(|_| {
            ZarrError::Shard(ShardError::InvalidChunkRange {
                offset,
                size: usize::MAX,
                shard_size: self.data.len(),
            })
        })?;

        let end =
            offset
                .checked_add(size)
                .ok_or(ZarrError::Shard(ShardError::InvalidChunkRange {
                    offset,
                    size,
                    shard_size: self.data.len(),
                }))?;

        if end > self.data.len() {
            return Err(ZarrError::Shard(ShardError::InvalidChunkRange {
                offset,
                size,
                shard_size: self.data.len(),
            }));
        }

        let compressed_data = self.data[offset..end].to_vec();
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

    /// Finalizes the shard and returns the encoded data.
    ///
    /// Per ZEP-0002, no leading/trailing 8-byte size field is written.
    /// Layout for `IndexLocation::End`:   `[chunk_data][index_bytes]`
    /// Layout for `IndexLocation::Start`: `[index_bytes][chunk_data]`
    /// For `IndexLocation::Start`, offsets must be absolute file positions, so
    /// a two-pass approach is used: first compress all chunks to learn their
    /// sizes, compute the encoded-index size, then rewrite offsets accordingly.
    ///
    /// # Errors
    /// Returns error if finalization fails
    pub fn finalize(self) -> Result<Vec<u8>> {
        // Sort chunks for deterministic output
        let mut sorted_coords: Vec<_> = self.chunks.keys().cloned().collect();
        sorted_coords.sort();

        // Pass 1: compress all chunks, record (compressed_data, size) pairs
        let mut compressed_chunks: Vec<(Vec<usize>, Vec<u8>)> = Vec::new();
        for coords in sorted_coords {
            if let Some(chunk_data) = self.chunks.get(&coords) {
                let compressed = self.codec.encode(chunk_data.clone())?;
                compressed_chunks.push((coords, compressed));
            }
        }

        match self.index_location {
            IndexLocation::End => {
                // Layout: [chunk_data][index_bytes]
                // Offsets are relative to the start of the file (before the index).
                let mut index = ShardIndex::new(self.chunks_per_shard.clone());
                let mut chunk_section = Vec::new();

                for (coords, compressed) in compressed_chunks {
                    let offset = chunk_section.len() as u64;
                    let size = compressed.len() as u64;
                    chunk_section.extend_from_slice(&compressed);
                    index.set(&coords, ShardIndexEntry::new(offset, size))?;
                }

                let index_bytes = index.encode()?;
                let encoded_index = self.index_codec.encode(index_bytes)?;

                chunk_section.extend_from_slice(&encoded_index);
                Ok(chunk_section)
            }
            IndexLocation::Start => {
                // Layout: [index_bytes][chunk_data]
                // Offsets must be absolute positions in the shard file.
                // Two-pass: first determine index size, then compute correct offsets.

                // Pass 1b: build a temporary index with placeholder offsets to
                // measure the encoded index size.
                let mut placeholder_index = ShardIndex::new(self.chunks_per_shard.clone());
                let mut chunk_sizes: Vec<(Vec<usize>, u64)> = Vec::new();
                for (coords, compressed) in &compressed_chunks {
                    let size = compressed.len() as u64;
                    // placeholder offset = 0; will be fixed after we know index_size
                    placeholder_index.set(coords, ShardIndexEntry::new(0, size))?;
                    chunk_sizes.push((coords.clone(), size));
                }

                let placeholder_raw = placeholder_index.encode()?;
                let encoded_placeholder = self.index_codec.encode(placeholder_raw)?;
                let index_size = encoded_placeholder.len() as u64;

                // Pass 2: rebuild index with correct absolute offsets
                let mut final_index = ShardIndex::new(self.chunks_per_shard.clone());
                let mut abs_offset = index_size;
                for (coords, size) in &chunk_sizes {
                    final_index.set(coords, ShardIndexEntry::new(abs_offset, *size))?;
                    abs_offset += size;
                }

                let final_index_raw = final_index.encode()?;
                let final_encoded_index = self.index_codec.encode(final_index_raw)?;

                // Assemble: [index_bytes][chunk_data]
                let mut result = final_encoded_index;
                for (_coords, compressed) in compressed_chunks {
                    result.extend_from_slice(&compressed);
                }
                Ok(result)
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

    #[test]
    fn test_shard_reader_end_location_two_inner_chunks() {
        // Two inner chunks, each 4 bytes
        // chunk0=[1,2,3,4] at offset 0, chunk1=[5,6,7,8] at offset 4
        // Index: entry0=(offset=0, nbytes=4), entry1=(offset=4, nbytes=4)
        // Layout: [chunk_data(8 bytes)][index(32 bytes)]
        let mut shard_data = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        // index entry 0: offset=0 (LE u64), size=4 (LE u64)
        shard_data.extend_from_slice(&0u64.to_le_bytes());
        shard_data.extend_from_slice(&4u64.to_le_bytes());
        // index entry 1: offset=4 (LE u64), size=4 (LE u64)
        shard_data.extend_from_slice(&4u64.to_le_bytes());
        shard_data.extend_from_slice(&4u64.to_le_bytes());

        let reader = ShardReader::new(
            shard_data,
            vec![2],
            CodecChain::empty(),
            CodecChain::empty(),
            IndexLocation::End,
        )
        .expect("create shard reader");

        assert_eq!(
            reader.read_chunk(&[0]).expect("read 0"),
            Some(vec![1u8, 2, 3, 4])
        );
        assert_eq!(
            reader.read_chunk(&[1]).expect("read 1"),
            Some(vec![5u8, 6, 7, 8])
        );
    }

    #[test]
    fn test_shard_reader_missing_slot_returns_none() {
        let mut shard_data = vec![1u8, 2, 3, 4]; // only chunk0 data
        // entry 0: offset=0, size=4
        shard_data.extend_from_slice(&0u64.to_le_bytes());
        shard_data.extend_from_slice(&4u64.to_le_bytes());
        // entry 1: sentinel (u64::MAX, u64::MAX)
        shard_data.extend_from_slice(&u64::MAX.to_le_bytes());
        shard_data.extend_from_slice(&u64::MAX.to_le_bytes());

        let reader = ShardReader::new(
            shard_data,
            vec![2],
            CodecChain::empty(),
            CodecChain::empty(),
            IndexLocation::End,
        )
        .expect("create shard reader");

        assert_eq!(
            reader.read_chunk(&[0]).expect("read 0"),
            Some(vec![1u8, 2, 3, 4])
        );
        assert_eq!(reader.read_chunk(&[1]).expect("read 1"), None);
    }

    #[test]
    fn test_shard_reader_start_location() {
        // Index at START: entry0=(offset=32, size=2), entry1=(offset=34, size=2)
        // Then chunk data: [10,20] at byte 32, [30,40] at byte 34
        let mut shard_data = Vec::new();
        // entry 0: offset=32, size=2
        shard_data.extend_from_slice(&32u64.to_le_bytes());
        shard_data.extend_from_slice(&2u64.to_le_bytes());
        // entry 1: offset=34, size=2
        shard_data.extend_from_slice(&34u64.to_le_bytes());
        shard_data.extend_from_slice(&2u64.to_le_bytes());
        // chunk data
        shard_data.extend_from_slice(&[10u8, 20]);
        shard_data.extend_from_slice(&[30u8, 40]);

        let reader = ShardReader::new(
            shard_data,
            vec![2],
            CodecChain::empty(),
            CodecChain::empty(),
            IndexLocation::Start,
        )
        .expect("create shard reader start");

        assert_eq!(
            reader.read_chunk(&[0]).expect("read 0"),
            Some(vec![10u8, 20])
        );
        assert_eq!(
            reader.read_chunk(&[1]).expect("read 1"),
            Some(vec![30u8, 40])
        );
    }

    #[test]
    fn test_shard_reader_2d_inner_chunks() {
        // chunks_per_shard=[2,2] => 4 inner chunks, C-order:
        // [0,0]=0, [0,1]=1, [1,0]=2, [1,1]=3
        // Each chunk = 4 bytes. Shard data = 16 bytes + 64-byte index (4*16)
        let chunk0 = [1u8, 2, 3, 4];
        let chunk1 = [5u8, 6, 7, 8];
        let chunk2 = [9u8, 10, 11, 12];
        let chunk3 = [13u8, 14, 15, 16];

        let mut shard_data = Vec::new();
        shard_data.extend_from_slice(&chunk0);
        shard_data.extend_from_slice(&chunk1);
        shard_data.extend_from_slice(&chunk2);
        shard_data.extend_from_slice(&chunk3);

        // Index: 4 entries, each (offset, size) as LE u64 pairs
        for i in 0..4u64 {
            shard_data.extend_from_slice(&(i * 4).to_le_bytes());
            shard_data.extend_from_slice(&4u64.to_le_bytes());
        }

        let reader = ShardReader::new(
            shard_data,
            vec![2, 2],
            CodecChain::empty(),
            CodecChain::empty(),
            IndexLocation::End,
        )
        .expect("create 2d reader");

        assert_eq!(
            reader.read_chunk(&[0, 0]).expect("00"),
            Some(chunk0.to_vec())
        );
        assert_eq!(
            reader.read_chunk(&[0, 1]).expect("01"),
            Some(chunk1.to_vec())
        );
        assert_eq!(
            reader.read_chunk(&[1, 0]).expect("10"),
            Some(chunk2.to_vec())
        );
        assert_eq!(
            reader.read_chunk(&[1, 1]).expect("11"),
            Some(chunk3.to_vec())
        );
    }

    #[test]
    fn test_shard_writer_roundtrip_end() {
        let codec = CodecChain::empty();
        let index_codec = CodecChain::empty();
        let mut writer = ShardWriter::new(vec![2], codec, index_codec, IndexLocation::End);
        writer
            .write_chunk(vec![0], vec![1u8, 2, 3, 4])
            .expect("write 0");
        writer
            .write_chunk(vec![1], vec![5u8, 6, 7, 8])
            .expect("write 1");
        let shard_bytes = writer.finalize().expect("finalize");

        let reader = ShardReader::new(
            shard_bytes,
            vec![2],
            CodecChain::empty(),
            CodecChain::empty(),
            IndexLocation::End,
        )
        .expect("create reader");

        assert_eq!(
            reader.read_chunk(&[0]).expect("read 0"),
            Some(vec![1u8, 2, 3, 4])
        );
        assert_eq!(
            reader.read_chunk(&[1]).expect("read 1"),
            Some(vec![5u8, 6, 7, 8])
        );
    }

    #[test]
    fn test_shard_writer_roundtrip_start() {
        let codec = CodecChain::empty();
        let index_codec = CodecChain::empty();
        let mut writer = ShardWriter::new(vec![2], codec, index_codec, IndexLocation::Start);
        writer
            .write_chunk(vec![0], vec![10u8, 20])
            .expect("write 0");
        writer
            .write_chunk(vec![1], vec![30u8, 40])
            .expect("write 1");
        let shard_bytes = writer.finalize().expect("finalize");

        let reader = ShardReader::new(
            shard_bytes,
            vec![2],
            CodecChain::empty(),
            CodecChain::empty(),
            IndexLocation::Start,
        )
        .expect("create reader start");

        assert_eq!(
            reader.read_chunk(&[0]).expect("read 0"),
            Some(vec![10u8, 20])
        );
        assert_eq!(
            reader.read_chunk(&[1]).expect("read 1"),
            Some(vec![30u8, 40])
        );
    }

    #[test]
    fn test_shard_reader_overflowing_offset_size_errors_not_panics() {
        // entry.offset = u64::MAX - 10, entry.size = 20: not the `missing`
        // sentinel (that requires both fields == u64::MAX), but
        // offset + size overflows u64/usize arithmetic. This must return a
        // typed error, not panic (debug-mode overflow) or wrap into a
        // spurious in-bounds slice (release-mode overflow).
        let mut shard_data = vec![0u8; 8]; // small backing buffer
        shard_data.extend_from_slice(&(u64::MAX - 10).to_le_bytes());
        shard_data.extend_from_slice(&20u64.to_le_bytes());

        let reader = ShardReader::new(
            shard_data,
            vec![1],
            CodecChain::empty(),
            CodecChain::empty(),
            IndexLocation::End,
        )
        .expect("create shard reader");

        let result = reader.read_chunk(&[0]);
        assert!(
            matches!(
                result,
                Err(ZarrError::Shard(ShardError::InvalidChunkRange { .. }))
            ),
            "expected InvalidChunkRange error, got {result:?}"
        );
    }
}
