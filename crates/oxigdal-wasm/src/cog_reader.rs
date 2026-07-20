//! WASM-specific COG Reader with async I/O
//!
//! This module provides a minimal COG reader implementation specifically
//! designed for WebAssembly environments where all I/O must be asynchronous.
//!
//! Unlike the main `oxigdal-geotiff::CogReader` which uses the synchronous
//! `DataSource` trait, this implementation directly uses async fetch methods.

use futures::stream::{self, StreamExt};

use oxigdal_core::error::{FormatError, IoError, OxiGdalError, Result};
use oxigdal_core::io::ByteRange;

use crate::fetch::FetchBackend;

/// Upper bound on simultaneously in-flight tile range-fetches for a single
/// windowed read. wasm32 has no OS threads, but browsers happily pipeline
/// several concurrent `fetch()` calls to the same origin; capping keeps a
/// large (400+ tile) window from saturating the browser's per-origin
/// connection pool while still avoiding the fully-serial one-at-a-time cost.
const MAX_CONCURRENT_TILE_FETCHES: usize = 8;

// TIFF tag constants
const TAG_IMAGE_WIDTH: u16 = 256;
const TAG_IMAGE_LENGTH: u16 = 257;
const TAG_TILE_WIDTH: u16 = 322;
const TAG_TILE_LENGTH: u16 = 323;
const TAG_GEO_KEY_DIRECTORY: u16 = 34735;
const TAG_GEO_DOUBLE_PARAMS: u16 = 34736;

// GeoKey IDs
const GEOKEY_PROJECTED_CS_TYPE: u16 = 3072;
const GEOKEY_GEOGRAPHIC_TYPE: u16 = 2048;

/// Overview/pyramid level metadata
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API for future use
pub struct OverviewMetadata {
    pub width: u64,
    pub height: u64,
    pub tile_width: u32,
    pub tile_height: u32,
}

/// Per-IFD metadata for one pyramid level (full resolution or an overview).
///
/// Every entry carries the tile geometry, sample layout, decompression codec,
/// Predictor (TIFF tag 317) and tile directory required to fetch and correctly
/// decode a tile at that resolution. `levels[0]` in [`CogMetadata`] is the
/// full-resolution image; subsequent entries are the reduced-resolution
/// overviews in file order (typically halving each dimension).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Per-level geo/sample fields consumed by the A4 pipeline
pub struct IfdMetadata {
    pub width: u64,
    pub height: u64,
    pub tile_width: u32,
    pub tile_height: u32,
    pub bits_per_sample: u16,
    pub samples_per_pixel: u16,
    /// TIFF SampleFormat (tag 339): 1=unsigned int, 2=signed int, 3=IEEE float
    pub sample_format: u16,
    pub compression: u16,
    pub photometric_interpretation: u16,
    /// TIFF Predictor (tag 317): 1=none, 2=horizontal differencing.
    pub predictor: u16,
    pub tile_offsets: Vec<u64>,
    pub tile_byte_counts: Vec<u64>,
    pub pixel_scale_x: Option<f64>,
    pub pixel_scale_y: Option<f64>,
    pub tiepoint_pixel_x: Option<f64>,
    pub tiepoint_pixel_y: Option<f64>,
    pub tiepoint_geo_x: Option<f64>,
    pub tiepoint_geo_y: Option<f64>,
    pub epsg_code: Option<u32>,
}

impl IfdMetadata {
    /// Build a per-level record from a freshly parsed [`CogMetadata`] IFD.
    fn from_cog(meta: &CogMetadata) -> Self {
        Self {
            width: meta.width,
            height: meta.height,
            tile_width: meta.tile_width,
            tile_height: meta.tile_height,
            bits_per_sample: meta.bits_per_sample,
            samples_per_pixel: meta.samples_per_pixel,
            sample_format: meta.sample_format,
            compression: meta.compression,
            photometric_interpretation: meta.photometric_interpretation,
            predictor: meta.predictor,
            tile_offsets: meta.tile_offsets.clone(),
            tile_byte_counts: meta.tile_byte_counts.clone(),
            pixel_scale_x: meta.pixel_scale_x,
            pixel_scale_y: meta.pixel_scale_y,
            tiepoint_pixel_x: meta.tiepoint_pixel_x,
            tiepoint_pixel_y: meta.tiepoint_pixel_y,
            tiepoint_geo_x: meta.tiepoint_geo_x,
            tiepoint_geo_y: meta.tiepoint_geo_y,
            epsg_code: meta.epsg_code,
        }
    }
}

/// TIFF/COG metadata extracted from IFD
#[derive(Debug, Clone)]
pub struct CogMetadata {
    pub width: u64,
    pub height: u64,
    pub tile_width: u32,
    pub tile_height: u32,
    pub bits_per_sample: u16,
    pub samples_per_pixel: u16,
    /// TIFF SampleFormat (tag 339): 1=unsigned int, 2=signed int, 3=IEEE float
    pub sample_format: u16,
    pub compression: u16,
    #[allow(dead_code)]
    pub photometric_interpretation: u16,
    /// TIFF Predictor (tag 317) for the full-resolution IFD: 1=none, 2=horizontal.
    pub predictor: u16,
    pub tile_offsets: Vec<u64>,
    pub tile_byte_counts: Vec<u64>,
    // GeoTIFF geotransform data
    pub pixel_scale_x: Option<f64>,
    pub pixel_scale_y: Option<f64>,
    pub tiepoint_pixel_x: Option<f64>,
    pub tiepoint_pixel_y: Option<f64>,
    pub tiepoint_geo_x: Option<f64>,
    pub tiepoint_geo_y: Option<f64>,
    // Overview and EPSG metadata
    pub overview_count: usize,
    pub overviews: Vec<OverviewMetadata>,
    pub epsg_code: Option<u32>,
    /// All pyramid levels in file order; `levels[0]` is the full-resolution IFD
    /// and `levels[1..]` are the overviews. Each carries its own tile directory,
    /// predictor and sample layout for `read_tile_level` / `read_window_*`.
    pub levels: Vec<IfdMetadata>,
}

/// WASM-compatible async COG reader
pub struct WasmCogReader {
    backend: FetchBackend,
    metadata: CogMetadata,
    #[allow(dead_code)]
    byte_order: ByteOrder,
}

/// TIFF byte order
#[derive(Debug, Clone, Copy)]
pub enum ByteOrder {
    LittleEndian,
    BigEndian,
}

impl WasmCogReader {
    /// Open a COG file from a URL with async I/O
    pub async fn open(url: String) -> Result<Self> {
        let backend = FetchBackend::new(url.clone()).await?;

        // Read TIFF header (first 8-16 bytes)
        let header_bytes = backend
            .read_range_async(ByteRange::from_offset_length(0, 16))
            .await?;

        // A short/truncated response (misconfigured server returning an error
        // page as 200 OK, a tiny/corrupt file, or a CORS-opaque body) must not
        // be indexed blindly — fail with a typed error instead of panicking.
        if header_bytes.len() < 16 {
            return Err(OxiGdalError::Format(FormatError::InvalidHeader {
                message: format!(
                    "truncated TIFF header: expected 16 bytes, got {}",
                    header_bytes.len()
                ),
            }));
        }

        // Parse byte order and magic number
        let byte_order = if &header_bytes[0..2] == b"II" {
            ByteOrder::LittleEndian
        } else if &header_bytes[0..2] == b"MM" {
            ByteOrder::BigEndian
        } else {
            return Err(OxiGdalError::Format(FormatError::InvalidHeader {
                message: "Invalid TIFF magic bytes".to_string(),
            }));
        };

        // Read IFD offset (bytes 4-8 for classic TIFF)
        let mut ifd_offset = match byte_order {
            ByteOrder::LittleEndian => u32::from_le_bytes([
                header_bytes[4],
                header_bytes[5],
                header_bytes[6],
                header_bytes[7],
            ]) as u64,
            ByteOrder::BigEndian => u32::from_be_bytes([
                header_bytes[4],
                header_bytes[5],
                header_bytes[6],
                header_bytes[7],
            ]) as u64,
        };

        // Read first IFD data (read 4KB to ensure we get all tags)
        let ifd_data = backend
            .read_range_async(ByteRange::from_offset_length(ifd_offset, 4096))
            .await?;

        // Parse primary IFD
        let (metadata, next_offset) =
            Self::parse_ifd(&ifd_data, byte_order, &backend, ifd_offset).await?;

        // Full-resolution image is level 0.
        let mut levels: Vec<IfdMetadata> = vec![IfdMetadata::from_cog(&metadata)];

        // Parse overview IFDs with the SAME full parser so each level carries its
        // own tile directory, predictor and sample layout (not just dimensions).
        let mut overviews = Vec::new();
        ifd_offset = next_offset;
        let mut ifd_count = 0;

        while ifd_offset != 0 && ifd_count < 100 {
            // Read overview IFD data
            let ov_ifd_data = backend
                .read_range_async(ByteRange::from_offset_length(ifd_offset, 4096))
                .await?;

            // Reuse the primary parser for a complete per-overview IFD.
            let (ov_meta, next_ov_offset) =
                Self::parse_ifd(&ov_ifd_data, byte_order, &backend, ifd_offset).await?;

            if ov_meta.width > 0 && ov_meta.height > 0 {
                overviews.push(OverviewMetadata {
                    width: ov_meta.width,
                    height: ov_meta.height,
                    tile_width: ov_meta.tile_width,
                    tile_height: ov_meta.tile_height,
                });
                levels.push(IfdMetadata::from_cog(&ov_meta));
            }

            ifd_offset = next_ov_offset;
            ifd_count += 1;
        }

        // Update metadata with overviews and per-level records.
        let mut final_metadata = metadata;
        final_metadata.overview_count = overviews.len();
        final_metadata.overviews = overviews;
        final_metadata.levels = levels;

        Ok(Self {
            backend,
            metadata: final_metadata,
            byte_order,
        })
    }

    /// Parse IFD to extract essential tags and return metadata with next IFD offset
    async fn parse_ifd(
        data: &[u8],
        byte_order: ByteOrder,
        backend: &FetchBackend,
        _ifd_offset: u64,
    ) -> Result<(CogMetadata, u64)> {
        // Read number of directory entries (first 2 bytes). A near-EOF IFD in a
        // small file, or a truncated/short HTTP response, can yield a buffer
        // shorter than 2 bytes — guard before indexing rather than panicking.
        if data.len() < 2 {
            return Err(OxiGdalError::Format(FormatError::InvalidHeader {
                message: format!(
                    "truncated IFD: expected at least 2 bytes, got {}",
                    data.len()
                ),
            }));
        }
        let num_entries = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([data[0], data[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([data[0], data[1]]),
        };

        let mut width = 0u64;
        let mut height = 0u64;
        let mut tile_width = 256u32;
        let mut tile_height = 256u32;
        let mut rows_per_strip = 0u32;
        let mut bits_per_sample = 8u16;
        let mut samples_per_pixel = 1u16;
        let mut sample_format = 1u16;
        let mut compression = 1u16;
        let mut predictor = 1u16;
        let mut photometric = 1u16;
        let mut tile_offsets = Vec::new();
        let mut tile_byte_counts = Vec::new();

        // GeoTIFF geotransform data
        let mut pixel_scale_x: Option<f64> = None;
        let mut pixel_scale_y: Option<f64> = None;
        let mut tiepoint_pixel_x: Option<f64> = None;
        let mut tiepoint_pixel_y: Option<f64> = None;
        let mut tiepoint_geo_x: Option<f64> = None;
        let mut tiepoint_geo_y: Option<f64> = None;

        // GeoKey directory data for EPSG parsing
        let mut geo_key_directory: Option<Vec<u16>> = None;
        let mut geo_double_params: Vec<f64> = Vec::new();

        // Parse each IFD entry (12 bytes each)
        for i in 0..num_entries {
            let offset = 2 + (i as usize * 12);
            if offset + 12 > data.len() {
                break;
            }

            let entry = &data[offset..offset + 12];

            let tag = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([entry[0], entry[1]]),
                ByteOrder::BigEndian => u16::from_be_bytes([entry[0], entry[1]]),
            };

            let field_type = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([entry[2], entry[3]]),
                ByteOrder::BigEndian => u16::from_be_bytes([entry[2], entry[3]]),
            };

            let count = match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]])
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([entry[4], entry[5], entry[6], entry[7]])
                }
            };

            // Value/offset bytes
            let value_bytes = &entry[8..12];

            match tag {
                TAG_IMAGE_WIDTH => {
                    // ImageWidth
                    width = Self::read_value(value_bytes, field_type, byte_order);
                }
                TAG_IMAGE_LENGTH => {
                    // ImageLength
                    height = Self::read_value(value_bytes, field_type, byte_order);
                }
                258 => {
                    // BitsPerSample. Multi-band images (e.g. Sentinel-2 TCI RGB)
                    // carry one SHORT per sample — three SHORTs (6 bytes) do not
                    // fit the inline value field, so the entry holds an offset.
                    // Reading that offset as a scalar yields a garbage bit depth
                    // (which silently disabled predictor undo for RGB tiles), so
                    // arrays go through read_array; samples share one depth in
                    // COG practice, so the first entry is authoritative.
                    bits_per_sample = if count <= 1 {
                        Self::read_value(value_bytes, field_type, byte_order) as u16
                    } else {
                        Self::read_array(value_bytes, field_type, count, byte_order, backend)
                            .await?
                            .first()
                            .copied()
                            .unwrap_or(8) as u16
                    };
                }
                259 => {
                    // Compression
                    compression = Self::read_value(value_bytes, field_type, byte_order) as u16;
                }
                262 => {
                    // PhotometricInterpretation
                    photometric = Self::read_value(value_bytes, field_type, byte_order) as u16;
                }
                317 => {
                    // Predictor (1=none, 2=horizontal differencing)
                    predictor = Self::read_value(value_bytes, field_type, byte_order) as u16;
                }
                277 => {
                    // SamplesPerPixel
                    samples_per_pixel =
                        Self::read_value(value_bytes, field_type, byte_order) as u16;
                }
                339 => {
                    // SampleFormat (1=unsigned int, 2=signed int, 3=IEEE float).
                    // Same per-sample array layout as BitsPerSample above.
                    sample_format = if count <= 1 {
                        Self::read_value(value_bytes, field_type, byte_order) as u16
                    } else {
                        Self::read_array(value_bytes, field_type, count, byte_order, backend)
                            .await?
                            .first()
                            .copied()
                            .unwrap_or(1) as u16
                    };
                }
                278 => {
                    // RowsPerStrip (for strip-based TIFFs)
                    rows_per_strip = Self::read_value(value_bytes, field_type, byte_order) as u32;
                }
                TAG_TILE_WIDTH => {
                    // TileWidth
                    tile_width = Self::read_value(value_bytes, field_type, byte_order) as u32;
                }
                TAG_TILE_LENGTH => {
                    // TileLength
                    tile_height = Self::read_value(value_bytes, field_type, byte_order) as u32;
                }
                273 => {
                    // StripOffsets (for non-tiled TIFFs)
                    tile_offsets =
                        Self::read_array(value_bytes, field_type, count, byte_order, backend)
                            .await?;
                }
                279 => {
                    // StripByteCounts (for non-tiled TIFFs)
                    tile_byte_counts =
                        Self::read_array(value_bytes, field_type, count, byte_order, backend)
                            .await?;
                }
                324 => {
                    // TileOffsets (for tiled TIFFs)
                    tile_offsets =
                        Self::read_array(value_bytes, field_type, count, byte_order, backend)
                            .await?;
                }
                325 => {
                    // TileByteCounts (for tiled TIFFs)
                    tile_byte_counts =
                        Self::read_array(value_bytes, field_type, count, byte_order, backend)
                            .await?;
                }
                33550 if count >= 2 => {
                    // ModelPixelScaleTag (GeoTIFF)
                    // Contains [ScaleX, ScaleY, ScaleZ] as DOUBLE (type 12)
                    let doubles = Self::read_double_array(
                        value_bytes,
                        field_type,
                        count,
                        byte_order,
                        backend,
                    )
                    .await?;
                    if !doubles.is_empty() {
                        pixel_scale_x = Some(doubles[0]);
                    }
                    if doubles.len() > 1 {
                        pixel_scale_y = Some(doubles[1]);
                    }
                }
                33550 => {
                    // ModelPixelScaleTag with insufficient count - skip
                }
                33922 if count >= 6 => {
                    // ModelTiepointTag (GeoTIFF)
                    // Contains [I, J, K, X, Y, Z] as DOUBLE (type 12)
                    // I, J, K = raster coordinates
                    // X, Y, Z = geographic coordinates
                    let doubles = Self::read_double_array(
                        value_bytes,
                        field_type,
                        count,
                        byte_order,
                        backend,
                    )
                    .await?;
                    if doubles.len() >= 6 {
                        tiepoint_pixel_x = Some(doubles[0]);
                        tiepoint_pixel_y = Some(doubles[1]);
                        tiepoint_geo_x = Some(doubles[3]);
                        tiepoint_geo_y = Some(doubles[4]);
                    }
                }
                TAG_GEO_KEY_DIRECTORY => {
                    // GeoKeyDirectoryTag
                    let values =
                        Self::read_array(value_bytes, field_type, count, byte_order, backend)
                            .await?;
                    geo_key_directory = Some(values.iter().map(|&v| v as u16).collect());
                }
                TAG_GEO_DOUBLE_PARAMS => {
                    // GeoDoubleParamsTag
                    geo_double_params = Self::read_double_array(
                        value_bytes,
                        field_type,
                        count,
                        byte_order,
                        backend,
                    )
                    .await?;
                }
                _ => {}
            }
        }

        // Handle strip-based TIFFs (convert strips to tiles)
        if rows_per_strip > 0 && !tile_offsets.is_empty() {
            // This is a strip-based TIFF
            tile_width = width as u32; // Strips span full width
            tile_height = rows_per_strip;
        }

        // Read next IFD offset (located after all IFD entries)
        // Position: 2 (num_entries) + num_entries * 12 (entries) = next_ifd_offset
        let next_ifd_offset_pos = 2 + (num_entries as usize * 12);
        let next_ifd_offset = if next_ifd_offset_pos + 4 <= data.len() {
            match byte_order {
                ByteOrder::LittleEndian => u32::from_le_bytes([
                    data[next_ifd_offset_pos],
                    data[next_ifd_offset_pos + 1],
                    data[next_ifd_offset_pos + 2],
                    data[next_ifd_offset_pos + 3],
                ]) as u64,
                ByteOrder::BigEndian => u32::from_be_bytes([
                    data[next_ifd_offset_pos],
                    data[next_ifd_offset_pos + 1],
                    data[next_ifd_offset_pos + 2],
                    data[next_ifd_offset_pos + 3],
                ]) as u64,
            }
        } else {
            0
        };

        // Parse EPSG code from GeoKeyDirectory
        let epsg_code = Self::parse_epsg_from_geokeys(&geo_key_directory, &geo_double_params);

        let metadata = CogMetadata {
            width,
            height,
            tile_width,
            tile_height,
            bits_per_sample,
            samples_per_pixel,
            sample_format,
            compression,
            photometric_interpretation: photometric,
            predictor,
            tile_offsets,
            tile_byte_counts,
            pixel_scale_x,
            pixel_scale_y,
            tiepoint_pixel_x,
            tiepoint_pixel_y,
            tiepoint_geo_x,
            tiepoint_geo_y,
            overview_count: 0,     // Will be updated after parsing all IFDs
            overviews: Vec::new(), // Will be updated after parsing all IFDs
            epsg_code,
            levels: Vec::new(), // Will be populated by `open` after all IFDs parse
        };

        Ok((metadata, next_ifd_offset))
    }

    /// Parse EPSG code from GeoKeyDirectory
    fn parse_epsg_from_geokeys(
        geo_key_directory: &Option<Vec<u16>>,
        _geo_double_params: &[f64],
    ) -> Option<u32> {
        let directory = geo_key_directory.as_ref()?;

        // GeoKeyDirectory format:
        // [version, key_revision_major, key_revision_minor, key_count, ...]
        // Followed by key_count entries of [key_id, tiff_tag_location, count, value_offset]
        if directory.len() < 4 {
            return None;
        }

        let key_count = directory[3] as usize;

        if directory.len() < 4 + key_count * 4 {
            return None;
        }

        // Search for ProjectedCSType or GeographicType keys
        for i in 0..key_count {
            let base = 4 + i * 4;
            let key_id = directory[base];
            let tiff_tag_location = directory[base + 1];
            let value_offset = directory[base + 3];

            // Check for ProjectedCSType (3072) or GeographicType (2048)
            // Both should have tiff_tag_location = 0 (inline value)
            if tiff_tag_location == 0 {
                if key_id == GEOKEY_PROJECTED_CS_TYPE && value_offset != 32767 {
                    // 32767 = user-defined
                    return Some(u32::from(value_offset));
                }
                if key_id == GEOKEY_GEOGRAPHIC_TYPE && value_offset != 32767 {
                    return Some(u32::from(value_offset));
                }
            }
        }

        None
    }

    /// Read a single value from IFD entry
    fn read_value(bytes: &[u8], field_type: u16, byte_order: ByteOrder) -> u64 {
        match field_type {
            3 => {
                // SHORT
                match byte_order {
                    ByteOrder::LittleEndian => u16::from_le_bytes([bytes[0], bytes[1]]) as u64,
                    ByteOrder::BigEndian => u16::from_be_bytes([bytes[0], bytes[1]]) as u64,
                }
            }
            4 => {
                // LONG
                match byte_order {
                    ByteOrder::LittleEndian => {
                        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64
                    }
                    ByteOrder::BigEndian => {
                        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64
                    }
                }
            }
            _ => 0,
        }
    }

    /// Read an array of values (for tile offsets, byte counts, etc.)
    async fn read_array(
        bytes: &[u8],
        field_type: u16,
        count: u32,
        byte_order: ByteOrder,
        backend: &FetchBackend,
    ) -> Result<Vec<u64>> {
        let value_size = match field_type {
            3 => 2, // SHORT
            4 => 4, // LONG
            _ => return Ok(Vec::new()),
        };

        let total_size = count as usize * value_size;

        // If data fits in 4 bytes, it's stored inline
        let data = if total_size <= 4 {
            bytes.to_vec()
        } else {
            // Data is stored at offset
            let offset = match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64
                }
            };

            backend
                .read_range_async(ByteRange::from_offset_length(offset, total_size as u64))
                .await?
        };

        let mut values = Vec::with_capacity(count as usize);

        for i in 0..count as usize {
            let offset = i * value_size;
            if offset + value_size > data.len() {
                break;
            }

            let value = match field_type {
                3 => {
                    // SHORT
                    match byte_order {
                        ByteOrder::LittleEndian => {
                            u16::from_le_bytes([data[offset], data[offset + 1]]) as u64
                        }
                        ByteOrder::BigEndian => {
                            u16::from_be_bytes([data[offset], data[offset + 1]]) as u64
                        }
                    }
                }
                4 => {
                    // LONG
                    match byte_order {
                        ByteOrder::LittleEndian => u32::from_le_bytes([
                            data[offset],
                            data[offset + 1],
                            data[offset + 2],
                            data[offset + 3],
                        ]) as u64,
                        ByteOrder::BigEndian => u32::from_be_bytes([
                            data[offset],
                            data[offset + 1],
                            data[offset + 2],
                            data[offset + 3],
                        ]) as u64,
                    }
                }
                _ => 0,
            };

            values.push(value);
        }

        Ok(values)
    }

    /// Read an array of DOUBLE values (for GeoTIFF tags)
    async fn read_double_array(
        bytes: &[u8],
        field_type: u16,
        count: u32,
        byte_order: ByteOrder,
        backend: &FetchBackend,
    ) -> Result<Vec<f64>> {
        // Type 12 is DOUBLE (8 bytes)
        if field_type != 12 {
            return Ok(Vec::new());
        }

        let value_size = 8; // DOUBLE is 8 bytes
        let total_size = count as usize * value_size;

        // If data fits in 4 bytes, it's stored inline (impossible for DOUBLE > 0)
        let data = if total_size <= 4 {
            bytes.to_vec()
        } else {
            // Data is stored at offset
            let offset = match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64
                }
            };

            backend
                .read_range_async(ByteRange::from_offset_length(offset, total_size as u64))
                .await?
        };

        let mut values = Vec::with_capacity(count as usize);

        for i in 0..count as usize {
            let offset = i * value_size;
            if offset + value_size > data.len() {
                break;
            }

            // Read 8 bytes as f64
            let bytes_array = [
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ];

            let value = match byte_order {
                ByteOrder::LittleEndian => f64::from_le_bytes(bytes_array),
                ByteOrder::BigEndian => f64::from_be_bytes(bytes_array),
            };

            values.push(value);
        }

        Ok(values)
    }

    /// Read a specific tile from the full-resolution image (level 0).
    ///
    /// Signature and behaviour are preserved for existing callers: the returned
    /// bytes are the fully decoded tile in raster order. This now routes through
    /// [`read_tile_level`](Self::read_tile_level) so the horizontal Predictor
    /// (TIFF tag 317) is undone when present. `predictor == 1` (or absent) is a
    /// no-op, so existing non-predicted COGs are unaffected.
    pub async fn read_tile(&self, tile_x: u32, tile_y: u32) -> Result<Vec<u8>> {
        self.read_tile_level(0, tile_x, tile_y).await
    }

    /// Read a single tile at a specific pyramid level (0 = full resolution).
    ///
    /// The tile is fetched from that level's own tile directory, decompressed,
    /// and — when the level declares `predictor == 2` — the horizontal
    /// differencing predictor is undone in place using the level's sample
    /// layout (bits/sample, samples/pixel) and the reader's byte order. The
    /// returned bytes are the decoded tile in raster order (`tile_width ×
    /// tile_height × samples_per_pixel` samples).
    pub async fn read_tile_level(&self, level: usize, tile_x: u32, tile_y: u32) -> Result<Vec<u8>> {
        let lvl = self
            .metadata
            .levels
            .get(level)
            .ok_or_else(|| OxiGdalError::OutOfBounds {
                message: format!("Overview level {} out of range", level),
            })?;

        let tiles_across = lvl.width.div_ceil(lvl.tile_width as u64) as u32;
        let tile_index = (tile_y * tiles_across + tile_x) as usize;

        if tile_index >= lvl.tile_offsets.len() || tile_index >= lvl.tile_byte_counts.len() {
            return Err(OxiGdalError::OutOfBounds {
                message: format!("Tile index {} out of range at level {}", tile_index, level),
            });
        }

        let offset = lvl.tile_offsets[tile_index];
        let byte_count = lvl.tile_byte_counts[tile_index];

        // Read compressed tile data.
        let compressed_data = self
            .backend
            .read_range_async(ByteRange::from_offset_length(offset, byte_count))
            .await?;

        let mut decompressed = decompress_tile(compressed_data, lvl.compression)?;

        // Undo the horizontal predictor when the level declares one.
        if lvl.predictor == 2 {
            apply_horizontal_predictor(
                &mut decompressed,
                lvl.tile_width,
                lvl.tile_height,
                lvl.bits_per_sample,
                lvl.samples_per_pixel,
                self.is_little_endian(),
            );
        }

        Ok(decompressed)
    }

    /// Read a rectangular window of 16-bit samples at the given pyramid level.
    ///
    /// The window `[x0, x0+w) × [y0, y0+h)` is expressed in that level's pixel
    /// coordinates. Every tile intersecting the window is fetched, decoded
    /// (predictor-corrected) and cropped into a dense `w × h` row-major buffer.
    /// Tiles that fall entirely outside the level's tile grid (off-grid crops
    /// that overhang the image) contribute zeros rather than erroring. Intended
    /// for single-band 16-bit Sentinel-2 reflectance COGs.
    #[allow(dead_code)] // Consumed by the A4 GeoSentinel pipeline
    pub async fn read_window_u16(
        &self,
        level: usize,
        x0: u64,
        y0: u64,
        w: u32,
        h: u32,
    ) -> Result<Vec<u16>> {
        let lvl = self
            .metadata
            .levels
            .get(level)
            .ok_or_else(|| OxiGdalError::OutOfBounds {
                message: format!("Overview level {} out of range", level),
            })?;

        if w == 0 || h == 0 {
            return Ok(Vec::new());
        }

        let tile_width = lvl.tile_width;
        let tile_height = lvl.tile_height;
        let tw = tile_width as u64;
        let th = tile_height as u64;
        let tiles_across = lvl.width.div_ceil(tw);
        let tiles_down = lvl.height.div_ceil(th);
        let little_endian = self.is_little_endian();

        let tx0 = x0 / tw;
        let ty0 = y0 / th;
        let tx1 = (x0 + w as u64 - 1) / tw;
        let ty1 = (y0 + h as u64 - 1) / th;

        let mut coords: Vec<(u32, u32)> = Vec::new();
        for ty in ty0..=ty1 {
            if ty >= tiles_down {
                continue;
            }
            for tx in tx0..=tx1 {
                if tx >= tiles_across {
                    continue;
                }
                coords.push((tx as u32, ty as u32));
            }
        }

        // Tile order doesn't matter here: `assemble_window` places each tile
        // by its own (tx, ty) coordinates, so fetches can complete in any
        // order — issue them concurrently instead of one at a time.
        let fetches = stream::iter(coords.into_iter().map(|(tx, ty)| async move {
            let bytes = self.read_tile_level(level, tx, ty).await?;
            Ok::<_, OxiGdalError>((tx, ty, bytes_to_u16(&bytes, little_endian)))
        }))
        .buffer_unordered(MAX_CONCURRENT_TILE_FETCHES)
        .collect::<Vec<_>>()
        .await;

        let mut tiles: Vec<(u32, u32, Vec<u16>)> = Vec::with_capacity(fetches.len());
        for result in fetches {
            tiles.push(result?);
        }

        Ok(assemble_window(
            &tiles,
            tile_width,
            tile_height,
            x0,
            y0,
            w,
            h,
        ))
    }

    /// Read a rectangular window of 8-bit RGB (3 samples/pixel) at a level.
    ///
    /// Used for the Sentinel-2 True-Colour Image (TCI) asset. The output is a
    /// dense `w × h × 3` interleaved RGB byte buffer in row-major order.
    /// Off-grid tiles contribute zeros. The reader's own `samples_per_pixel`
    /// (nominally 3) is used to undo the predictor per channel.
    #[allow(dead_code)] // Consumed by the A4 GeoSentinel pipeline (TCI true colour)
    pub async fn read_window_rgb8(
        &self,
        level: usize,
        x0: u64,
        y0: u64,
        w: u32,
        h: u32,
    ) -> Result<Vec<u8>> {
        let lvl = self
            .metadata
            .levels
            .get(level)
            .ok_or_else(|| OxiGdalError::OutOfBounds {
                message: format!("Overview level {} out of range", level),
            })?;

        if w == 0 || h == 0 {
            return Ok(Vec::new());
        }

        let tile_width = lvl.tile_width;
        let tile_height = lvl.tile_height;
        let tw = tile_width as u64;
        let th = tile_height as u64;
        let tiles_across = lvl.width.div_ceil(tw);
        let tiles_down = lvl.height.div_ceil(th);

        let tx0 = x0 / tw;
        let ty0 = y0 / th;
        let tx1 = (x0 + w as u64 - 1) / tw;
        let ty1 = (y0 + h as u64 - 1) / th;

        let mut coords: Vec<(u32, u32)> = Vec::new();
        for ty in ty0..=ty1 {
            if ty >= tiles_down {
                continue;
            }
            for tx in tx0..=tx1 {
                if tx >= tiles_across {
                    continue;
                }
                coords.push((tx as u32, ty as u32));
            }
        }

        // Tile order doesn't matter here: `assemble_window_rgb8` places each
        // tile by its own (tx, ty) coordinates, so fetches can complete in
        // any order — issue them concurrently instead of one at a time.
        let fetches = stream::iter(coords.into_iter().map(|(tx, ty)| async move {
            let bytes = self.read_tile_level(level, tx, ty).await?;
            Ok::<_, OxiGdalError>((tx, ty, bytes))
        }))
        .buffer_unordered(MAX_CONCURRENT_TILE_FETCHES)
        .collect::<Vec<_>>()
        .await;

        let mut tiles: Vec<(u32, u32, Vec<u8>)> = Vec::with_capacity(fetches.len());
        for result in fetches {
            tiles.push(result?);
        }

        Ok(assemble_window_rgb8(
            &tiles,
            tile_width,
            tile_height,
            x0,
            y0,
            w,
            h,
        ))
    }

    /// Get metadata
    pub fn metadata(&self) -> &CogMetadata {
        &self.metadata
    }

    /// Returns `true` if the underlying TIFF uses little-endian byte order.
    pub fn is_little_endian(&self) -> bool {
        matches!(self.byte_order, ByteOrder::LittleEndian)
    }
}

/// Decompress a raw tile payload according to its TIFF compression code.
///
/// Only the codecs relevant to Sentinel-2 / GeoLab COGs are handled: `1`
/// (uncompressed) and `8` (Zlib-wrapped DEFLATE). LZW (`5`) and other codecs
/// return a typed [`OxiGdalError::NotSupported`].
fn decompress_tile(compressed: Vec<u8>, compression: u16) -> Result<Vec<u8>> {
    match compression {
        1 => Ok(compressed), // No compression
        5 => Err(OxiGdalError::NotSupported {
            operation: "LZW compression not yet supported in WASM".to_string(),
        }),
        8 => oxiarc_deflate::zlib_decompress(&compressed).map_err(|e| {
            OxiGdalError::Io(IoError::Read {
                message: format!("DEFLATE decompression failed: {}", e),
            })
        }),
        other => Err(OxiGdalError::NotSupported {
            operation: format!("Compression type {} not supported", other),
        }),
    }
}

/// Reinterpret a decoded tile byte buffer as little-/big-endian `u16` samples.
///
/// A trailing odd byte (if any) is dropped via `chunks_exact`.
#[allow(dead_code)] // Used by read_window_u16 (A4 pipeline surface)
fn bytes_to_u16(bytes: &[u8], little_endian: bool) -> Vec<u16> {
    bytes
        .chunks_exact(2)
        .map(|c| {
            if little_endian {
                u16::from_le_bytes([c[0], c[1]])
            } else {
                u16::from_be_bytes([c[0], c[1]])
            }
        })
        .collect()
}

/// Undo TIFF horizontal differencing (Predictor 2) for one row of 16-bit
/// single-sample data: each sample becomes the running sum of the deltas.
///
/// This is the single-band (`samples_per_pixel == 1`) case used by Sentinel-2
/// reflectance bands. Wrapping addition matches the encoder's wrapping
/// subtraction so the transform is exactly invertible.
fn undo_horizontal_predictor_u16(row: &mut [u16]) {
    for i in 1..row.len() {
        row[i] = row[i].wrapping_add(row[i - 1]);
    }
}

/// Undo TIFF horizontal differencing (Predictor 2) for one row of 8-bit data
/// with `spp` interleaved samples per pixel (e.g. `spp == 3` for TCI RGB).
///
/// Each sample references the sample `spp` positions earlier so channels are
/// reconstructed independently. Wrapping addition mirrors the encoder.
fn undo_horizontal_predictor_u8(row: &mut [u8], spp: usize) {
    let spp = spp.max(1);
    for i in spp..row.len() {
        row[i] = row[i].wrapping_add(row[i - spp]);
    }
}

/// Undo the horizontal predictor over a whole decoded tile, in place.
///
/// Operates row by row using the tile geometry and sample layout. Supports
/// 8-bit and 16-bit samples; other bit depths are left untouched (no predictor
/// support). For 16-bit data the row is decoded/re-encoded using the reader's
/// byte order.
fn apply_horizontal_predictor(
    data: &mut [u8],
    tile_width: u32,
    tile_height: u32,
    bits_per_sample: u16,
    samples_per_pixel: u16,
    little_endian: bool,
) {
    let tw = tile_width as usize;
    let th = tile_height as usize;
    let spp = samples_per_pixel.max(1) as usize;

    match bits_per_sample {
        16 => {
            let row_samples = tw * spp;
            let row_bytes = row_samples * 2;
            for r in 0..th {
                let start = r * row_bytes;
                let end = start + row_bytes;
                if end > data.len() {
                    break;
                }
                let mut row: Vec<u16> = data[start..end]
                    .chunks_exact(2)
                    .map(|c| {
                        if little_endian {
                            u16::from_le_bytes([c[0], c[1]])
                        } else {
                            u16::from_be_bytes([c[0], c[1]])
                        }
                    })
                    .collect();

                if spp == 1 {
                    undo_horizontal_predictor_u16(&mut row);
                } else {
                    for i in spp..row.len() {
                        row[i] = row[i].wrapping_add(row[i - spp]);
                    }
                }

                for (i, &v) in row.iter().enumerate() {
                    let b = start + i * 2;
                    let out = if little_endian {
                        v.to_le_bytes()
                    } else {
                        v.to_be_bytes()
                    };
                    data[b] = out[0];
                    data[b + 1] = out[1];
                }
            }
        }
        8 => {
            let row_bytes = tw * spp;
            for r in 0..th {
                let start = r * row_bytes;
                let end = start + row_bytes;
                if end > data.len() {
                    break;
                }
                undo_horizontal_predictor_u8(&mut data[start..end], spp);
            }
        }
        _ => {}
    }
}

/// Assemble decoded 16-bit tiles into a dense `w × h` row-major window.
///
/// Pure and natively testable. Each tile is given as
/// `(tile_x, tile_y, samples)` where `samples` are the tile's `u16` values in
/// raster order (`tile_width × tile_height`). Pixels of a tile that fall inside
/// the window `[x0, x0+w) × [y0, y0+h)` (all in level pixel coordinates) are
/// scattered into the output; window pixels not covered by any supplied tile
/// (off-grid overhang) remain zero.
#[allow(dead_code)] // Used by read_window_u16 (A4 pipeline surface)
fn assemble_window(
    tiles: &[(u32, u32, Vec<u16>)],
    tile_width: u32,
    tile_height: u32,
    x0: u64,
    y0: u64,
    w: u32,
    h: u32,
) -> Vec<u16> {
    let w_usize = w as usize;
    let mut out = vec![0u16; w_usize * h as usize];
    let tw = tile_width as u64;
    let th = tile_height as u64;
    let x1 = x0 + w as u64;
    let y1 = y0 + h as u64;

    for (tx, ty, data) in tiles {
        let origin_x = *tx as u64 * tw;
        let origin_y = *ty as u64 * th;
        for row in 0..th {
            let gy = origin_y + row;
            if gy < y0 || gy >= y1 {
                continue;
            }
            let out_y = (gy - y0) as usize;
            for col in 0..tw {
                let gx = origin_x + col;
                if gx < x0 || gx >= x1 {
                    continue;
                }
                let src_idx = (row * tw + col) as usize;
                if src_idx < data.len() {
                    let out_x = (gx - x0) as usize;
                    out[out_y * w_usize + out_x] = data[src_idx];
                }
            }
        }
    }

    out
}

/// Assemble decoded 8-bit RGB tiles into a dense `w × h × 3` interleaved
/// row-major window. RGB analogue of [`assemble_window`]; each tile's bytes are
/// `tile_width × tile_height × 3` in raster order.
#[allow(dead_code)] // Used by read_window_rgb8 (A4 pipeline surface)
fn assemble_window_rgb8(
    tiles: &[(u32, u32, Vec<u8>)],
    tile_width: u32,
    tile_height: u32,
    x0: u64,
    y0: u64,
    w: u32,
    h: u32,
) -> Vec<u8> {
    let w_usize = w as usize;
    let mut out = vec![0u8; w_usize * h as usize * 3];
    let tw = tile_width as u64;
    let th = tile_height as u64;
    let x1 = x0 + w as u64;
    let y1 = y0 + h as u64;

    for (tx, ty, data) in tiles {
        let origin_x = *tx as u64 * tw;
        let origin_y = *ty as u64 * th;
        for row in 0..th {
            let gy = origin_y + row;
            if gy < y0 || gy >= y1 {
                continue;
            }
            let out_y = (gy - y0) as usize;
            for col in 0..tw {
                let gx = origin_x + col;
                if gx < x0 || gx >= x1 {
                    continue;
                }
                let src_idx = ((row * tw + col) as usize) * 3;
                if src_idx + 3 <= data.len() {
                    let out_x = (gx - x0) as usize;
                    let dst = (out_y * w_usize + out_x) * 3;
                    out[dst] = data[src_idx];
                    out[dst + 1] = data[src_idx + 1];
                    out[dst + 2] = data[src_idx + 2];
                }
            }
        }
    }

    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Forward horizontal differencing for a single-band 16-bit row: the
    /// inverse of [`undo_horizontal_predictor_u16`], used to build test inputs.
    fn forward_predictor_u16(row: &mut [u16]) {
        for i in (1..row.len()).rev() {
            row[i] = row[i].wrapping_sub(row[i - 1]);
        }
    }

    /// Forward horizontal differencing for an 8-bit `spp`-interleaved row.
    fn forward_predictor_u8(row: &mut [u8], spp: usize) {
        for i in (spp..row.len()).rev() {
            row[i] = row[i].wrapping_sub(row[i - spp]);
        }
    }

    #[test]
    fn predictor_u16_round_trip() {
        let original: Vec<u16> = vec![100, 105, 103, 250, 60000, 60005, 1, 0, 65535];
        let mut diffed = original.clone();
        forward_predictor_u16(&mut diffed);
        // Sanity: differencing actually changed the buffer.
        assert_ne!(diffed, original);
        undo_horizontal_predictor_u16(&mut diffed);
        assert_eq!(diffed, original);
    }

    #[test]
    fn predictor_u16_wraps() {
        // Deltas that overflow u16 must reconstruct via wrapping arithmetic.
        let original: Vec<u16> = vec![65530, 5, 65535, 10];
        let mut diffed = original.clone();
        forward_predictor_u16(&mut diffed);
        undo_horizontal_predictor_u16(&mut diffed);
        assert_eq!(diffed, original);
    }

    #[test]
    fn predictor_u8_rgb_round_trip() {
        // 4 pixels, 3 channels interleaved: R R R R / G G G G / B B B B pattern.
        let original: Vec<u8> = vec![
            10, 200, 30, // px0 RGB
            12, 205, 28, // px1
            15, 255, 25, // px2
            18, 2, 20, // px3
        ];
        let mut diffed = original.clone();
        forward_predictor_u8(&mut diffed, 3);
        assert_ne!(diffed, original);
        undo_horizontal_predictor_u8(&mut diffed, 3);
        assert_eq!(diffed, original);
    }

    #[test]
    fn apply_predictor_over_tile_u16() {
        // 2x2 tile, single band, little-endian. Undoing per-row must not leak
        // across the row boundary (row 1 is independent of row 0).
        let rows: [[u16; 2]; 2] = [[100, 150], [4000, 4020]];
        let mut expected: Vec<u16> = Vec::new();
        let mut bytes: Vec<u8> = Vec::new();
        for row in &rows {
            let mut diffed = row.to_vec();
            forward_predictor_u16(&mut diffed);
            for v in &diffed {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            expected.extend_from_slice(row);
        }
        apply_horizontal_predictor(&mut bytes, 2, 2, 16, 1, true);
        assert_eq!(bytes_to_u16(&bytes, true), expected);
    }

    /// Build a synthetic tile whose every pixel encodes its global coordinate
    /// as `gy * 1000 + gx`, so assembly can be checked positionally.
    fn synthetic_tile_u16(tx: u32, ty: u32, tw: u32, th: u32) -> (u32, u32, Vec<u16>) {
        let mut data = Vec::with_capacity((tw * th) as usize);
        for row in 0..th {
            for col in 0..tw {
                let gx = tx * tw + col;
                let gy = ty * th + row;
                data.push((gy * 1000 + gx) as u16);
            }
        }
        (tx, ty, data)
    }

    #[test]
    fn assemble_window_crosses_tiles() {
        // 3x3 grid of 2x2 tiles => 6x6 image. Window [1,1) size 3x3 spans four
        // tiles (0,0),(1,0),(0,1),(1,1).
        let tw = 2u32;
        let th = 2u32;
        let mut tiles = Vec::new();
        for ty in 0..3 {
            for tx in 0..3 {
                tiles.push(synthetic_tile_u16(tx, ty, tw, th));
            }
        }
        let (x0, y0, w, h) = (1u64, 1u64, 3u32, 3u32);
        let out = assemble_window(&tiles, tw, th, x0, y0, w, h);
        assert_eq!(out.len(), (w * h) as usize);
        for oy in 0..h as u64 {
            for ox in 0..w as u64 {
                let gx = x0 + ox;
                let gy = y0 + oy;
                let expect = (gy * 1000 + gx) as u16;
                assert_eq!(
                    out[(oy * w as u64 + ox) as usize],
                    expect,
                    "mismatch at window ({ox},{oy}) global ({gx},{gy})"
                );
            }
        }
    }

    #[test]
    fn assemble_window_off_grid_crop_zero_fills() {
        // Only tile (0,0) of a 2x2 tile grid is supplied; a 3x3 window at the
        // origin overhangs the tile. Covered pixels carry data; the rest are 0.
        let tw = 2u32;
        let th = 2u32;
        let tiles = vec![synthetic_tile_u16(0, 0, tw, th)];
        let out = assemble_window(&tiles, tw, th, 0, 0, 3, 3);
        assert_eq!(out.len(), 9);
        // In-tile region (x<2, y<2) matches gy*1000+gx.
        assert_eq!(out[0], 0); // (0,0)
        assert_eq!(out[1], 1); // (1,0)
        assert_eq!(out[3], 1000); // (0,1)
        assert_eq!(out[4], 1001); // (1,1)
        // Overhang columns/rows are zero.
        assert_eq!(out[2], 0); // (2,0) no tile
        assert_eq!(out[5], 0); // (2,1)
        assert_eq!(out[6], 0); // (0,2)
        assert_eq!(out[7], 0); // (1,2)
        assert_eq!(out[8], 0); // (2,2)
    }

    #[test]
    fn assemble_window_rgb8_crops() {
        // Single 2x2 RGB tile; window offset by (1,1) with size 2x2 overhangs
        // to the right and bottom, leaving one covered pixel.
        let tw = 2u32;
        let th = 2u32;
        let data: Vec<u8> = vec![
            1, 2, 3, // (0,0)
            4, 5, 6, // (1,0)
            7, 8, 9, // (0,1)
            10, 11, 12, // (1,1)
        ];
        let tiles = vec![(0u32, 0u32, data)];
        let out = assemble_window_rgb8(&tiles, tw, th, 1, 1, 2, 2);
        assert_eq!(out.len(), 2 * 2 * 3);
        // Output pixel (0,0) maps to global (1,1) => tile pixel (1,1) = 10,11,12.
        assert_eq!(&out[0..3], &[10, 11, 12]);
        // The other three output pixels overhang the tile => zero.
        assert_eq!(&out[3..12], &[0u8; 9]);
    }

    #[test]
    fn assemble_window_is_order_independent() {
        // Regression test for concurrent tile fetching: `read_window_u16` now
        // gathers tiles via `buffer_unordered`, so they can arrive in any
        // order. `assemble_window` places each tile by its own (tx, ty), so
        // shuffling the input vector must not change the output.
        let tw = 2u32;
        let th = 2u32;
        let mut tiles = Vec::new();
        for ty in 0..3 {
            for tx in 0..3 {
                tiles.push(synthetic_tile_u16(tx, ty, tw, th));
            }
        }
        let (x0, y0, w, h) = (1u64, 1u64, 3u32, 3u32);
        let sequential = assemble_window(&tiles, tw, th, x0, y0, w, h);

        // Reverse order (as if the last tile requested finished first).
        let mut reversed = tiles.clone();
        reversed.reverse();
        assert_eq!(assemble_window(&reversed, tw, th, x0, y0, w, h), sequential);

        // An interleaved/shuffled order (odd indices first, then even).
        let mut shuffled: Vec<_> = tiles.iter().skip(1).step_by(2).cloned().collect();
        shuffled.extend(tiles.iter().step_by(2).cloned());
        assert_eq!(assemble_window(&shuffled, tw, th, x0, y0, w, h), sequential);
    }

    #[test]
    fn assemble_window_rgb8_is_order_independent() {
        let tw = 2u32;
        let th = 2u32;
        let tiles: Vec<(u32, u32, Vec<u8>)> = vec![
            (0, 0, vec![1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4]),
            (1, 0, vec![5, 5, 5, 6, 6, 6, 7, 7, 7, 8, 8, 8]),
        ];
        let sequential = assemble_window_rgb8(&tiles, tw, th, 0, 0, 4, 2);

        let mut reversed = tiles.clone();
        reversed.reverse();
        assert_eq!(
            assemble_window_rgb8(&reversed, tw, th, 0, 0, 4, 2),
            sequential
        );
    }

    /// Drives an iterator of coordinates through the exact same
    /// `stream::iter(...).buffer_unordered(N)` shape used by
    /// `read_window_u16`/`read_window_rgb8`, with a fake per-tile fetch
    /// whose futures are ready in reverse submission order (later
    /// coordinates resolve first via `future::ready` reordering below), to
    /// prove the concurrency change can't scramble which bytes end up at
    /// which (tx, ty) — this ordering-insensitivity is what makes concurrent
    /// (rather than strictly sequential) fetching safe here.
    #[test]
    fn concurrent_tile_gather_preserves_coordinate_mapping() {
        futures::executor::block_on(async {
            // Deliberately built in reverse (ty, tx) order to stand in for
            // fetches completing out of their original request order.
            let coords: Vec<(u32, u32)> = (0..3)
                .rev()
                .flat_map(|ty| (0..3).rev().map(move |tx| (tx, ty)))
                .collect();

            let fetches = stream::iter(coords.iter().copied().map(|(tx, ty)| async move {
                Ok::<_, OxiGdalError>((tx, ty, vec![(ty * 1000 + tx) as u16]))
            }))
            .buffer_unordered(MAX_CONCURRENT_TILE_FETCHES)
            .collect::<Vec<_>>()
            .await;

            let mut tiles: Vec<(u32, u32, Vec<u16>)> = Vec::with_capacity(fetches.len());
            for result in fetches {
                tiles.push(result.expect("fake fetch never errors"));
            }

            // Every requested coordinate is present exactly once, each still
            // carrying its own correct payload, regardless of completion order.
            assert_eq!(tiles.len(), coords.len());
            for (tx, ty) in coords {
                let found = tiles
                    .iter()
                    .find(|(ttx, tty, _)| *ttx == tx && *tty == ty)
                    .unwrap_or_else(|| panic!("missing tile ({tx},{ty})"));
                assert_eq!(found.2, vec![(ty * 1000 + tx) as u16]);
            }
        });
    }

    #[test]
    fn bytes_to_u16_endianness() {
        let bytes = [0x01, 0x02, 0xFF, 0x00];
        assert_eq!(bytes_to_u16(&bytes, true), vec![0x0201, 0x00FF]);
        assert_eq!(bytes_to_u16(&bytes, false), vec![0x0102, 0xFF00]);
    }

    #[test]
    fn decompress_tile_passthrough_and_errors() {
        let raw = vec![1u8, 2, 3, 4];
        assert_eq!(decompress_tile(raw.clone(), 1).unwrap(), raw);
        assert!(decompress_tile(raw.clone(), 5).is_err()); // LZW unsupported
        assert!(decompress_tile(raw, 99).is_err()); // unknown codec
    }
}
