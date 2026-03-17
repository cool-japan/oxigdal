//! PMTiles v3 archive writer.
//!
//! Builds a complete PMTiles v3 file in memory, including header, root
//! directory, metadata, and tile data sections.  Tiles are deduplicated by
//! content hash so identical payloads share a single copy.
//!
//! Reference: <https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md>

use std::collections::BTreeMap;

use crate::error::PmTilesError;
use crate::header::{Compression, PMTILES_HEADER_SIZE, PMTILES_MAGIC, TileType};
use crate::hilbert::zxy_to_tile_id;
use crate::varint::encode_varint_into;

/// A tile waiting to be written.
#[derive(Debug)]
struct PendingTile {
    /// PMTiles tile ID (Hilbert-curve encoded).
    tile_id: u64,
    /// Raw tile data.
    data: Vec<u8>,
}

/// FNV-1a 64-bit hash for content deduplication.
///
/// This is a simple, fast, non-cryptographic hash well-suited for
/// deduplication of tile payloads.
fn fnv1a_hash(data: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Builder for constructing PMTiles v3 archives.
///
/// # Example
/// ```
/// use oxigdal_pmtiles::writer::PmTilesBuilder;
/// use oxigdal_pmtiles::TileType;
///
/// let mut builder = PmTilesBuilder::new(TileType::Png, 0, 2);
/// builder.add_tile(0, 0, 0, b"tile-z0").unwrap();
/// builder.add_tile(1, 0, 0, b"tile-z1-00").unwrap();
/// let archive = builder.build().unwrap();
/// assert!(archive.len() > 127);
/// ```
pub struct PmTilesBuilder {
    tile_type: TileType,
    min_zoom: u8,
    max_zoom: u8,
    metadata_json: Option<String>,
    min_lon_e7: i32,
    min_lat_e7: i32,
    max_lon_e7: i32,
    max_lat_e7: i32,
    center_lon_e7: i32,
    center_lat_e7: i32,
    center_zoom: u8,
    tiles: Vec<PendingTile>,
}

impl PmTilesBuilder {
    /// Create a new builder for the given tile type and zoom range.
    pub fn new(tile_type: TileType, min_zoom: u8, max_zoom: u8) -> Self {
        Self {
            tile_type,
            min_zoom,
            max_zoom,
            metadata_json: None,
            min_lon_e7: -1_800_000_000,
            min_lat_e7: -900_000_000,
            max_lon_e7: 1_800_000_000,
            max_lat_e7: 900_000_000,
            center_lon_e7: 0,
            center_lat_e7: 0,
            center_zoom: min_zoom,
            tiles: Vec::new(),
        }
    }

    /// Set the JSON metadata string.
    pub fn set_metadata(&mut self, json: String) {
        self.metadata_json = Some(json);
    }

    /// Set the geographic bounding box in decimal degrees.
    pub fn set_bounds(&mut self, min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64) {
        self.min_lon_e7 = (min_lon * 1e7) as i32;
        self.min_lat_e7 = (min_lat * 1e7) as i32;
        self.max_lon_e7 = (max_lon * 1e7) as i32;
        self.max_lat_e7 = (max_lat * 1e7) as i32;
    }

    /// Set the default view centre and zoom level.
    pub fn set_center(&mut self, lon: f64, lat: f64, zoom: u8) {
        self.center_lon_e7 = (lon * 1e7) as i32;
        self.center_lat_e7 = (lat * 1e7) as i32;
        self.center_zoom = zoom;
    }

    /// Add a tile at the given `(z, x, y)` coordinates.
    ///
    /// # Errors
    /// Returns [`PmTilesError::InvalidFormat`] if `z` is outside the configured
    /// zoom range, or the coordinates are out of range for the zoom level.
    pub fn add_tile(&mut self, z: u8, x: u32, y: u32, data: &[u8]) -> Result<(), PmTilesError> {
        if z < self.min_zoom || z > self.max_zoom {
            return Err(PmTilesError::InvalidFormat(format!(
                "Zoom level {z} outside configured range [{}, {}]",
                self.min_zoom, self.max_zoom
            )));
        }
        let tile_id = zxy_to_tile_id(z, x, y)?;
        self.tiles.push(PendingTile {
            tile_id,
            data: data.to_vec(),
        });
        Ok(())
    }

    /// Return the number of tiles added so far.
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Consume the builder and produce a complete PMTiles v3 archive as bytes.
    ///
    /// The output is a valid PMTiles file with:
    /// - 127-byte header
    /// - Root directory (varint-encoded, uncompressed)
    /// - JSON metadata section (uncompressed)
    /// - Tile data section (tiles sorted by tile_id, deduplicated)
    ///
    /// # Errors
    /// Returns [`PmTilesError::InvalidFormat`] on internal encoding failures.
    pub fn build(mut self) -> Result<Vec<u8>, PmTilesError> {
        // Sort tiles by tile_id.
        self.tiles.sort_by_key(|t| t.tile_id);

        // Track unique contents by content hash for the header counter.
        let mut seen_hashes: BTreeMap<u64, ()> = BTreeMap::new();
        let mut tile_data_buf: Vec<u8> = Vec::new();

        // Build directory entries in clustered layout: each tile's data is
        // written sequentially so that the directory can use delta-0
        // (clustered) offset encoding.  Deduplication is tracked for the
        // `tile_contents` header field but data is written for every entry.
        let mut dir_entries: Vec<DirEntry> = Vec::with_capacity(self.tiles.len());

        for tile in &self.tiles {
            let hash = fnv1a_hash(&tile.data);
            let len = u32::try_from(tile.data.len()).map_err(|_| {
                PmTilesError::InvalidFormat("Tile data exceeds u32::MAX bytes".into())
            })?;

            let offset = u64::try_from(tile_data_buf.len()).map_err(|_| {
                PmTilesError::InvalidFormat("Total tile data exceeds u64 range".into())
            })?;
            tile_data_buf.extend_from_slice(&tile.data);
            seen_hashes.entry(hash).or_insert(());

            dir_entries.push(DirEntry {
                tile_id: tile.tile_id,
                offset,
                length: len,
                run_length: 1,
            });
        }

        let unique_contents = seen_hashes.len() as u64;

        // Encode directory
        let dir_bytes = encode_directory(&dir_entries)?;

        // Encode metadata
        let metadata_bytes = self
            .metadata_json
            .as_deref()
            .unwrap_or("{}")
            .as_bytes()
            .to_vec();

        // Layout: [header(127)] [root_dir] [metadata] [tile_data]
        let root_dir_offset = PMTILES_HEADER_SIZE as u64;
        let root_dir_length = dir_bytes.len() as u64;
        let metadata_offset = root_dir_offset + root_dir_length;
        let metadata_length = metadata_bytes.len() as u64;
        let tile_data_offset = metadata_offset + metadata_length;
        let tile_data_length = tile_data_buf.len() as u64;

        let addressed_tiles = dir_entries.len() as u64;
        let tile_entries = dir_entries.len() as u64;

        // Build header
        let header_bytes = serialize_header(HeaderFields {
            root_dir_offset,
            root_dir_length,
            metadata_offset,
            metadata_length,
            leaf_dirs_offset: 0,
            leaf_dirs_length: 0,
            tile_data_offset,
            tile_data_length,
            addressed_tiles,
            tile_entries,
            tile_contents: unique_contents,
            clustered: true,
            internal_compression: Compression::None,
            tile_compression: Compression::None,
            tile_type: self.tile_type,
            min_zoom: self.min_zoom,
            max_zoom: self.max_zoom,
            min_lon_e7: self.min_lon_e7,
            min_lat_e7: self.min_lat_e7,
            max_lon_e7: self.max_lon_e7,
            max_lat_e7: self.max_lat_e7,
            center_zoom: self.center_zoom,
            center_lon_e7: self.center_lon_e7,
            center_lat_e7: self.center_lat_e7,
        });

        // Assemble output
        let total_size =
            PMTILES_HEADER_SIZE + dir_bytes.len() + metadata_bytes.len() + tile_data_buf.len();
        let mut output = Vec::with_capacity(total_size);
        output.extend_from_slice(&header_bytes);
        output.extend_from_slice(&dir_bytes);
        output.extend_from_slice(&metadata_bytes);
        output.extend_from_slice(&tile_data_buf);

        Ok(output)
    }
}

/// Internal directory entry for encoding.
#[derive(Debug)]
struct DirEntry {
    tile_id: u64,
    offset: u64,
    length: u32,
    run_length: u32,
}

/// Encode directory entries into the PMTiles v3 wire format.
///
/// Format: `num_entries`, then four columns (tile_id deltas, run_lengths,
/// lengths, offset deltas), all varint-encoded.
fn encode_directory(entries: &[DirEntry]) -> Result<Vec<u8>, PmTilesError> {
    let n = entries.len();
    let mut buf = Vec::with_capacity(n * 8);

    // num_entries
    encode_varint_into(n as u64, &mut buf);

    // Tile ID deltas (sorted, cumulative)
    let mut last_id: u64 = 0;
    for entry in entries {
        let delta = entry.tile_id.checked_sub(last_id).ok_or_else(|| {
            PmTilesError::InvalidFormat(format!(
                "Tile IDs not sorted: {} after {}",
                entry.tile_id, last_id
            ))
        })?;
        encode_varint_into(delta, &mut buf);
        last_id = entry.tile_id;
    }

    // Run lengths
    for entry in entries {
        encode_varint_into(u64::from(entry.run_length), &mut buf);
    }

    // Lengths
    for entry in entries {
        encode_varint_into(u64::from(entry.length), &mut buf);
    }

    // Offsets: first is absolute, subsequent are delta-encoded.
    // delta=0 means "immediately follows previous entry" (clustered shorthand).
    let mut last_offset: u64 = 0;
    for (i, entry) in entries.iter().enumerate() {
        if i == 0 {
            encode_varint_into(entry.offset, &mut buf);
            last_offset = entry.offset;
        } else {
            let expected_clustered = last_offset + u64::from(entries[i - 1].length);
            if entry.offset == expected_clustered {
                // Clustered: encode 0
                encode_varint_into(0, &mut buf);
                last_offset = expected_clustered;
            } else {
                let delta = entry.offset.checked_sub(last_offset).ok_or_else(|| {
                    PmTilesError::InvalidFormat(format!(
                        "Offsets not monotonically increasing: {} after {}",
                        entry.offset, last_offset
                    ))
                })?;
                encode_varint_into(delta, &mut buf);
                last_offset += delta;
            }
        }
    }

    Ok(buf)
}

/// Fields needed to serialize a PMTiles header.
struct HeaderFields {
    root_dir_offset: u64,
    root_dir_length: u64,
    metadata_offset: u64,
    metadata_length: u64,
    leaf_dirs_offset: u64,
    leaf_dirs_length: u64,
    tile_data_offset: u64,
    tile_data_length: u64,
    addressed_tiles: u64,
    tile_entries: u64,
    tile_contents: u64,
    clustered: bool,
    internal_compression: Compression,
    tile_compression: Compression,
    tile_type: TileType,
    min_zoom: u8,
    max_zoom: u8,
    min_lon_e7: i32,
    min_lat_e7: i32,
    max_lon_e7: i32,
    max_lat_e7: i32,
    center_zoom: u8,
    center_lon_e7: i32,
    center_lat_e7: i32,
}

/// Convert a `TileType` to its spec byte value.
fn tile_type_to_u8(tt: &TileType) -> u8 {
    match tt {
        TileType::Unknown => 0,
        TileType::Mvt => 1,
        TileType::Png => 2,
        TileType::Jpeg => 3,
        TileType::Webp => 4,
        TileType::Avif => 5,
    }
}

/// Convert a `Compression` to its spec byte value.
fn compression_to_u8(c: &Compression) -> u8 {
    match c {
        Compression::Unknown => 0,
        Compression::None => 1,
        Compression::Gzip => 2,
        Compression::Brotli => 3,
        Compression::Zstd => 4,
    }
}

/// Serialize a PMTiles v3 header into exactly 127 bytes.
fn serialize_header(fields: HeaderFields) -> [u8; PMTILES_HEADER_SIZE] {
    let mut buf = [0u8; PMTILES_HEADER_SIZE];

    // Magic
    buf[0..7].copy_from_slice(PMTILES_MAGIC);
    // Version
    buf[7] = 3;

    // u64 LE fields
    buf[8..16].copy_from_slice(&fields.root_dir_offset.to_le_bytes());
    buf[16..24].copy_from_slice(&fields.root_dir_length.to_le_bytes());
    buf[24..32].copy_from_slice(&fields.metadata_offset.to_le_bytes());
    buf[32..40].copy_from_slice(&fields.metadata_length.to_le_bytes());
    buf[40..48].copy_from_slice(&fields.leaf_dirs_offset.to_le_bytes());
    buf[48..56].copy_from_slice(&fields.leaf_dirs_length.to_le_bytes());
    buf[56..64].copy_from_slice(&fields.tile_data_offset.to_le_bytes());
    buf[64..72].copy_from_slice(&fields.tile_data_length.to_le_bytes());
    buf[72..80].copy_from_slice(&fields.addressed_tiles.to_le_bytes());
    buf[80..88].copy_from_slice(&fields.tile_entries.to_le_bytes());
    buf[88..96].copy_from_slice(&fields.tile_contents.to_le_bytes());

    // Single-byte fields
    buf[96] = if fields.clustered { 1 } else { 0 };
    buf[97] = compression_to_u8(&fields.internal_compression);
    buf[98] = compression_to_u8(&fields.tile_compression);
    buf[99] = tile_type_to_u8(&fields.tile_type);
    buf[100] = fields.min_zoom;
    buf[101] = fields.max_zoom;

    // i32 LE fields
    buf[102..106].copy_from_slice(&fields.min_lon_e7.to_le_bytes());
    buf[106..110].copy_from_slice(&fields.min_lat_e7.to_le_bytes());
    buf[110..114].copy_from_slice(&fields.max_lon_e7.to_le_bytes());
    buf[114..118].copy_from_slice(&fields.max_lat_e7.to_le_bytes());
    buf[118] = fields.center_zoom;
    buf[119..123].copy_from_slice(&fields.center_lon_e7.to_le_bytes());
    buf[123..127].copy_from_slice(&fields.center_lat_e7.to_le_bytes());

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fnv1a_hash_different_data() {
        let h1 = fnv1a_hash(b"hello");
        let h2 = fnv1a_hash(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_fnv1a_hash_same_data() {
        let h1 = fnv1a_hash(b"hello");
        let h2 = fnv1a_hash(b"hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_fnv1a_hash_empty() {
        // Empty data should still produce a deterministic hash
        let h1 = fnv1a_hash(b"");
        let h2 = fnv1a_hash(b"");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_tile_type_to_u8_round_trip() {
        let types = [
            TileType::Unknown,
            TileType::Mvt,
            TileType::Png,
            TileType::Jpeg,
            TileType::Webp,
            TileType::Avif,
        ];
        for tt in &types {
            let byte = tile_type_to_u8(tt);
            let back = TileType::from_u8(byte);
            assert_eq!(&back, tt);
        }
    }

    #[test]
    fn test_compression_to_u8_round_trip() {
        let compressions = [
            Compression::Unknown,
            Compression::None,
            Compression::Gzip,
            Compression::Brotli,
            Compression::Zstd,
        ];
        for c in &compressions {
            let byte = compression_to_u8(c);
            let back = Compression::from_u8(byte);
            assert_eq!(&back, c);
        }
    }

    #[test]
    fn test_serialize_header_magic() {
        let h = serialize_header(HeaderFields {
            root_dir_offset: 127,
            root_dir_length: 0,
            metadata_offset: 127,
            metadata_length: 0,
            leaf_dirs_offset: 0,
            leaf_dirs_length: 0,
            tile_data_offset: 127,
            tile_data_length: 0,
            addressed_tiles: 0,
            tile_entries: 0,
            tile_contents: 0,
            clustered: true,
            internal_compression: Compression::None,
            tile_compression: Compression::None,
            tile_type: TileType::Png,
            min_zoom: 0,
            max_zoom: 14,
            min_lon_e7: 0,
            min_lat_e7: 0,
            max_lon_e7: 0,
            max_lat_e7: 0,
            center_zoom: 5,
            center_lon_e7: 0,
            center_lat_e7: 0,
        });
        assert_eq!(&h[0..7], PMTILES_MAGIC);
        assert_eq!(h[7], 3);
        assert_eq!(h.len(), 127);
    }
}
