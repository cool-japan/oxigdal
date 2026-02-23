//! WASM-specific COG Reader with async I/O
//!
//! This module provides a minimal COG reader implementation specifically
//! designed for WebAssembly environments where all I/O must be asynchronous.
//!
//! Unlike the main `oxigdal-geotiff::CogReader` which uses the synchronous
//! `DataSource` trait, this implementation directly uses async fetch methods.

use oxigdal_core::error::{FormatError, IoError, OxiGdalError, Result};
use oxigdal_core::io::ByteRange;

use crate::fetch::FetchBackend;

// TIFF tag constants (some are kept for future use)
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

/// TIFF/COG metadata extracted from IFD
#[derive(Debug, Clone)]
pub struct CogMetadata {
    pub width: u64,
    pub height: u64,
    pub tile_width: u32,
    pub tile_height: u32,
    #[allow(dead_code)]
    pub bits_per_sample: u16,
    pub samples_per_pixel: u16,
    pub compression: u16,
    #[allow(dead_code)]
    pub photometric_interpretation: u16,
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

        // Parse overview IFDs
        let mut overviews = Vec::new();
        ifd_offset = next_offset;
        let mut ifd_count = 0;

        while ifd_offset != 0 && ifd_count < 100 {
            // Read overview IFD data
            let ov_ifd_data = backend
                .read_range_async(ByteRange::from_offset_length(ifd_offset, 4096))
                .await?;

            // Parse overview IFD (only extract basic metadata)
            let (ov_meta, next_ov_offset) =
                Self::parse_overview_ifd(&ov_ifd_data, byte_order, &backend).await?;
            if let Some(ov) = ov_meta {
                overviews.push(ov);
            }

            ifd_offset = next_ov_offset;
            ifd_count += 1;
        }

        // Update metadata with overviews
        let mut final_metadata = metadata;
        final_metadata.overview_count = overviews.len();
        final_metadata.overviews = overviews;

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
        // Read number of directory entries (first 2 bytes)
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
        let mut compression = 1u16;
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
                256 => {
                    // ImageWidth
                    width = Self::read_value(value_bytes, field_type, byte_order);
                }
                257 => {
                    // ImageLength
                    height = Self::read_value(value_bytes, field_type, byte_order);
                }
                258 => {
                    // BitsPerSample
                    bits_per_sample = Self::read_value(value_bytes, field_type, byte_order) as u16;
                }
                259 => {
                    // Compression
                    compression = Self::read_value(value_bytes, field_type, byte_order) as u16;
                }
                262 => {
                    // PhotometricInterpretation
                    photometric = Self::read_value(value_bytes, field_type, byte_order) as u16;
                }
                277 => {
                    // SamplesPerPixel
                    samples_per_pixel =
                        Self::read_value(value_bytes, field_type, byte_order) as u16;
                }
                278 => {
                    // RowsPerStrip (for strip-based TIFFs)
                    rows_per_strip = Self::read_value(value_bytes, field_type, byte_order) as u32;
                }
                322 => {
                    // TileWidth
                    tile_width = Self::read_value(value_bytes, field_type, byte_order) as u32;
                }
                323 => {
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
                33550 => {
                    // ModelPixelScaleTag (GeoTIFF)
                    // Contains [ScaleX, ScaleY, ScaleZ] as DOUBLE (type 12)
                    if count >= 2 {
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
                }
                33922 => {
                    // ModelTiepointTag (GeoTIFF)
                    // Contains [I, J, K, X, Y, Z] as DOUBLE (type 12)
                    // I, J, K = raster coordinates
                    // X, Y, Z = geographic coordinates
                    if count >= 6 {
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
            compression,
            photometric_interpretation: photometric,
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
        };

        Ok((metadata, next_ifd_offset))
    }

    /// Parse overview IFD to extract basic metadata
    async fn parse_overview_ifd(
        data: &[u8],
        byte_order: ByteOrder,
        _backend: &FetchBackend,
    ) -> Result<(Option<OverviewMetadata>, u64)> {
        // Read number of directory entries (first 2 bytes)
        let num_entries = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([data[0], data[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([data[0], data[1]]),
        };

        let mut width = 0u64;
        let mut height = 0u64;
        let mut tile_width = 256u32;
        let mut tile_height = 256u32;

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

            let value_bytes = &entry[8..12];

            match tag {
                TAG_IMAGE_WIDTH => {
                    width = Self::read_value(value_bytes, field_type, byte_order);
                }
                TAG_IMAGE_LENGTH => {
                    height = Self::read_value(value_bytes, field_type, byte_order);
                }
                TAG_TILE_WIDTH => {
                    tile_width = Self::read_value(value_bytes, field_type, byte_order) as u32;
                }
                TAG_TILE_LENGTH => {
                    tile_height = Self::read_value(value_bytes, field_type, byte_order) as u32;
                }
                _ => {}
            }
        }

        // Read next IFD offset
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

        // Only return metadata if we found valid dimensions
        let overview_meta = if width > 0 && height > 0 {
            Some(OverviewMetadata {
                width,
                height,
                tile_width,
                tile_height,
            })
        } else {
            None
        };

        Ok((overview_meta, next_ifd_offset))
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

    /// Read a specific tile
    pub async fn read_tile(&self, tile_x: u32, tile_y: u32) -> Result<Vec<u8>> {
        // Calculate tile index
        let tiles_across = self
            .metadata
            .width
            .div_ceil(self.metadata.tile_width as u64) as u32;

        let tile_index = (tile_y * tiles_across + tile_x) as usize;

        if tile_index >= self.metadata.tile_offsets.len() {
            return Err(OxiGdalError::OutOfBounds {
                message: format!("Tile index {} out of range", tile_index),
            });
        }

        let offset = self.metadata.tile_offsets[tile_index];
        let byte_count = self.metadata.tile_byte_counts[tile_index];

        // Read compressed tile data
        let compressed_data = self
            .backend
            .read_range_async(ByteRange::from_offset_length(offset, byte_count))
            .await?;

        // Decompress if needed
        let decompressed = match self.metadata.compression {
            1 => compressed_data, // No compression
            5 => {
                // LZW
                return Err(OxiGdalError::NotSupported {
                    operation: "LZW compression not yet supported in WASM".to_string(),
                });
            }
            8 => {
                // DEFLATE (TIFF uses Zlib-wrapped DEFLATE)
                use flate2::read::ZlibDecoder;
                use std::io::Read;

                let mut decoder = ZlibDecoder::new(&compressed_data[..]);
                let mut decompressed = Vec::new();
                decoder.read_to_end(&mut decompressed).map_err(|e| {
                    OxiGdalError::Io(IoError::Read {
                        message: format!("DEFLATE decompression failed: {}", e),
                    })
                })?;
                decompressed
            }
            _ => {
                return Err(OxiGdalError::NotSupported {
                    operation: format!(
                        "Compression type {} not supported",
                        self.metadata.compression
                    ),
                });
            }
        };

        Ok(decompressed)
    }

    /// Get metadata
    pub fn metadata(&self) -> &CogMetadata {
        &self.metadata
    }
}
