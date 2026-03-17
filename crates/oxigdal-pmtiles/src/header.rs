//! PMTiles v3 file header.
//!
//! Reference: <https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md>

use crate::error::PmTilesError;

/// Seven-byte magic string that starts every PMTiles file.
pub const PMTILES_MAGIC: &[u8] = b"PMTiles";

/// Total size of the fixed PMTiles v3 header in bytes.
pub const PMTILES_HEADER_SIZE: usize = 127;

/// The type of data stored in the tile archive.
#[derive(Debug, Clone, PartialEq)]
pub enum TileType {
    /// Unknown / unrecognised type.
    Unknown,
    /// Mapbox Vector Tile (protocol buffer).
    Mvt,
    /// PNG raster tile.
    Png,
    /// JPEG raster tile.
    Jpeg,
    /// WebP raster tile.
    Webp,
    /// AVIF raster tile.
    Avif,
}

impl TileType {
    /// Decode the `tile_type` byte from the PMTiles header.
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Mvt,
            2 => Self::Png,
            3 => Self::Jpeg,
            4 => Self::Webp,
            5 => Self::Avif,
            _ => Self::Unknown,
        }
    }

    /// Return the MIME type string for this tile type.
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Mvt => "application/x-protobuf",
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Avif => "image/avif",
            Self::Unknown => "application/octet-stream",
        }
    }

    /// Return `true` for vector tile formats.
    pub fn is_vector(&self) -> bool {
        matches!(self, Self::Mvt)
    }

    /// Return `true` for raster tile formats.
    pub fn is_raster(&self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Webp | Self::Avif)
    }
}

/// Compression algorithm used for internal data structures or tile payloads.
#[derive(Debug, Clone, PartialEq)]
pub enum Compression {
    /// Unknown / unrecognised algorithm.
    Unknown,
    /// No compression.
    None,
    /// Gzip (RFC 1952).
    Gzip,
    /// Brotli (RFC 7932).
    Brotli,
    /// Zstandard.
    Zstd,
}

impl Compression {
    /// Decode the compression byte from the PMTiles header.
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::None,
            2 => Self::Gzip,
            3 => Self::Brotli,
            4 => Self::Zstd,
            _ => Self::Unknown,
        }
    }
}

/// Parsed 127-byte PMTiles v3 file header.
#[derive(Debug, Clone)]
pub struct PmTilesHeader {
    /// Specification version (must be 3).
    pub spec_version: u8,
    /// Byte offset of the root directory within the file.
    pub root_dir_offset: u64,
    /// Byte length of the root directory.
    pub root_dir_length: u64,
    /// Byte offset of the JSON metadata block.
    pub metadata_offset: u64,
    /// Byte length of the JSON metadata block.
    pub metadata_length: u64,
    /// Byte offset of the leaf directory section.
    pub leaf_dirs_offset: u64,
    /// Byte length of the leaf directory section.
    pub leaf_dirs_length: u64,
    /// Byte offset of the tile data section.
    pub tile_data_offset: u64,
    /// Byte length of the tile data section.
    pub tile_data_length: u64,
    /// Number of addressed (unique) tiles.
    pub addressed_tiles: u64,
    /// Number of tile entries in the directory.
    pub tile_entries: u64,
    /// Number of unique tile contents (de-duplicated payloads).
    pub tile_contents: u64,
    /// When `true`, tile data is written in tile-id order (enables delta encoding).
    pub clustered: bool,
    /// Compression applied to directory and metadata blocks.
    pub internal_compression: Compression,
    /// Compression applied to individual tile payloads.
    pub tile_compression: Compression,
    /// The tile data type.
    pub tile_type: TileType,
    /// Minimum zoom level present in the archive.
    pub min_zoom: u8,
    /// Maximum zoom level present in the archive.
    pub max_zoom: u8,
    /// Western bound in degrees × 10⁷ (integer).
    pub min_lon_e7: i32,
    /// Southern bound in degrees × 10⁷ (integer).
    pub min_lat_e7: i32,
    /// Eastern bound in degrees × 10⁷ (integer).
    pub max_lon_e7: i32,
    /// Northern bound in degrees × 10⁷ (integer).
    pub max_lat_e7: i32,
    /// Default view zoom level.
    pub center_zoom: u8,
    /// Default view longitude in degrees × 10⁷.
    pub center_lon_e7: i32,
    /// Default view latitude in degrees × 10⁷.
    pub center_lat_e7: i32,
}

impl PmTilesHeader {
    /// Parse a PMTiles v3 header from the first 127 bytes of a file.
    ///
    /// # Errors
    /// - [`PmTilesError::InvalidFormat`] when `data` is too short or the magic
    ///   is missing.
    /// - [`PmTilesError::UnsupportedVersion`] when the spec version is not 3.
    pub fn parse(data: &[u8]) -> Result<Self, PmTilesError> {
        if data.len() < PMTILES_HEADER_SIZE {
            return Err(PmTilesError::InvalidFormat(format!(
                "Data too short for PMTiles header: {} bytes (need {})",
                data.len(),
                PMTILES_HEADER_SIZE
            )));
        }
        if !data.starts_with(PMTILES_MAGIC) {
            return Err(PmTilesError::InvalidFormat("Not a PMTiles file".into()));
        }
        let version = data[7];
        if version != 3 {
            return Err(PmTilesError::UnsupportedVersion(version));
        }

        let u64_le = |o: usize| -> u64 {
            u64::from_le_bytes([
                data[o],
                data[o + 1],
                data[o + 2],
                data[o + 3],
                data[o + 4],
                data[o + 5],
                data[o + 6],
                data[o + 7],
            ])
        };
        let i32_le = |o: usize| -> i32 {
            i32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]])
        };

        Ok(Self {
            spec_version: version,
            root_dir_offset: u64_le(8),
            root_dir_length: u64_le(16),
            metadata_offset: u64_le(24),
            metadata_length: u64_le(32),
            leaf_dirs_offset: u64_le(40),
            leaf_dirs_length: u64_le(48),
            tile_data_offset: u64_le(56),
            tile_data_length: u64_le(64),
            addressed_tiles: u64_le(72),
            tile_entries: u64_le(80),
            tile_contents: u64_le(88),
            clustered: data[96] == 1,
            internal_compression: Compression::from_u8(data[97]),
            tile_compression: Compression::from_u8(data[98]),
            tile_type: TileType::from_u8(data[99]),
            min_zoom: data[100],
            max_zoom: data[101],
            min_lon_e7: i32_le(102),
            min_lat_e7: i32_le(106),
            max_lon_e7: i32_le(110),
            max_lat_e7: i32_le(114),
            center_zoom: data[118],
            center_lon_e7: i32_le(119),
            center_lat_e7: i32_le(123),
        })
    }

    /// Western bound in decimal degrees.
    pub fn min_lon(&self) -> f64 {
        self.min_lon_e7 as f64 / 1e7
    }
    /// Southern bound in decimal degrees.
    pub fn min_lat(&self) -> f64 {
        self.min_lat_e7 as f64 / 1e7
    }
    /// Eastern bound in decimal degrees.
    pub fn max_lon(&self) -> f64 {
        self.max_lon_e7 as f64 / 1e7
    }
    /// Northern bound in decimal degrees.
    pub fn max_lat(&self) -> f64 {
        self.max_lat_e7 as f64 / 1e7
    }
    /// Default view longitude in decimal degrees.
    pub fn center_lon(&self) -> f64 {
        self.center_lon_e7 as f64 / 1e7
    }
    /// Default view latitude in decimal degrees.
    pub fn center_lat(&self) -> f64 {
        self.center_lat_e7 as f64 / 1e7
    }
    /// Return `[min_lon, min_lat, max_lon, max_lat]` bounding box in decimal degrees.
    pub fn bounds(&self) -> [f64; 4] {
        [
            self.min_lon(),
            self.min_lat(),
            self.max_lon(),
            self.max_lat(),
        ]
    }
}
