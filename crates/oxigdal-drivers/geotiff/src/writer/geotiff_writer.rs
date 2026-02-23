//! GeoTIFF writer implementation
//!
//! This module provides high-level API for writing GeoTIFF files.

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use oxigdal_core::error::{OxiGdalError, Result};
use oxigdal_core::types::NoDataValue;

use crate::tiff::{ByteOrderType, PlanarConfiguration, TiffHeader, TiffTag, TiffVariant};
use crate::writer::WriterConfig;
use crate::writer::geokeys_writer::{GeoKeysBuilder, add_geo_transform};
use crate::writer::ifd_writer::IfdBuilder;
use crate::writer::overviews::OverviewGenerator;
use crate::writer::tiles::TileProcessor;

/// Options for GeoTIFF writer
#[derive(Debug, Clone)]
pub struct GeoTiffWriterOptions {
    /// Byte order (default: little-endian)
    pub byte_order: ByteOrderType,
}

impl Default for GeoTiffWriterOptions {
    fn default() -> Self {
        Self {
            byte_order: ByteOrderType::LittleEndian,
        }
    }
}

/// GeoTIFF writer
pub struct GeoTiffWriter {
    /// Output file
    file: File,
    /// Configuration
    config: WriterConfig,
    /// Options
    options: GeoTiffWriterOptions,
    /// TIFF variant
    variant: TiffVariant,
    /// Current file position
    position: u64,
}

impl GeoTiffWriter {
    /// Creates a new GeoTIFF writer
    ///
    /// # Errors
    /// Returns an error if the file cannot be created or configuration is invalid
    pub fn create<P: AsRef<Path>>(
        path: P,
        config: WriterConfig,
        options: GeoTiffWriterOptions,
    ) -> Result<Self> {
        config.validate()?;

        let file = File::create(path).map_err(|e| OxiGdalError::Io(e.into()))?;

        let variant = if config.use_bigtiff {
            TiffVariant::BigTiff
        } else {
            TiffVariant::Classic
        };

        Ok(Self {
            file,
            config,
            options,
            variant,
            position: 0,
        })
    }

    /// Writes raster data to the file
    ///
    /// # Arguments
    /// * `data` - Raster data (row-major, interleaved if multi-band)
    ///
    /// # Errors
    /// Returns an error if writing fails
    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        // Validate data size
        let expected_size = self.config.width as usize
            * self.config.height as usize
            * self.config.bytes_per_sample()
            * self.config.band_count as usize;

        if data.len() != expected_size {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "data",
                message: format!(
                    "Data size mismatch: expected {} bytes, got {}",
                    expected_size,
                    data.len()
                ),
            });
        }

        // Write TIFF header (placeholder, will update later)
        let header = TiffHeader {
            byte_order: self.options.byte_order,
            variant: self.variant,
            first_ifd_offset: 0, // Will be updated
        };
        let header_bytes = header.to_bytes();
        self.file
            .write_all(&header_bytes)
            .map_err(|e| OxiGdalError::Io(e.into()))?;
        self.position = header_bytes.len() as u64;

        // Process tiles or strips
        let (data_offsets, data_byte_counts) = if self.config.tile_width.is_some() {
            self.write_tiled_data(data)?
        } else {
            self.write_striped_data(data)?
        };

        // Generate overviews if requested
        let overview_data = if self.config.generate_overviews {
            self.generate_overviews(data)?
        } else {
            Vec::new()
        };

        // Write all data for overviews first
        let mut overview_offsets_and_counts = Vec::new();
        for overview in &overview_data {
            let mut offsets = Vec::new();
            let mut counts = Vec::new();

            for tile in &overview.tiles {
                let offset = self.position;
                self.file
                    .write_all(&tile.data)
                    .map_err(|e| OxiGdalError::Io(e.into()))?;
                offsets.push(offset);
                counts.push(tile.data.len() as u64);
                self.position += tile.data.len() as u64;
            }

            overview_offsets_and_counts.push((offsets, counts));
        }

        // Now write IFDs (we need to know positions ahead of time)
        // First, calculate IFD positions by building temporary IFDs to get sizes
        let mut ifd_positions = Vec::new();
        let mut current_pos = self.position;

        // Calculate primary IFD position
        let temp_ifd = self.build_ifd_data(
            &data_offsets,
            &data_byte_counts,
            self.config.width,
            self.config.height,
            current_pos,
            0,
        )?;
        ifd_positions.push(current_pos);
        current_pos += temp_ifd.len() as u64;

        // Calculate overview IFD positions
        for (i, overview) in overview_data.iter().enumerate() {
            let (offsets, counts) = &overview_offsets_and_counts[i];
            let temp_ifd = self.build_ifd_data(
                offsets,
                counts,
                overview.width,
                overview.height,
                current_pos,
                0,
            )?;
            ifd_positions.push(current_pos);
            current_pos += temp_ifd.len() as u64;
        }

        // Now build final IFDs with correct ifd_offset and next_ifd_offset
        let mut ifd_data_list = Vec::new();

        // Primary IFD
        let next_offset = if ifd_positions.len() > 1 {
            ifd_positions[1]
        } else {
            0
        };
        let primary_ifd = self.build_ifd_data(
            &data_offsets,
            &data_byte_counts,
            self.config.width,
            self.config.height,
            ifd_positions[0],
            next_offset,
        )?;
        ifd_data_list.push(primary_ifd);

        // Overview IFDs
        for (i, overview) in overview_data.iter().enumerate() {
            let (offsets, counts) = &overview_offsets_and_counts[i];
            let ifd_pos = ifd_positions[i + 1];
            let next = if i + 2 < ifd_positions.len() {
                ifd_positions[i + 2]
            } else {
                0
            };
            let ifd = self.build_ifd_data(
                offsets,
                counts,
                overview.width,
                overview.height,
                ifd_pos,
                next,
            )?;
            ifd_data_list.push(ifd);
        }

        // Write all IFDs
        for ifd_data in &ifd_data_list {
            self.file
                .write_all(ifd_data)
                .map_err(|e| OxiGdalError::Io(e.into()))?;
            self.position += ifd_data.len() as u64;
        }

        // Update header with first IFD offset
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|e| OxiGdalError::Io(e.into()))?;

        let updated_header = TiffHeader {
            byte_order: self.options.byte_order,
            variant: self.variant,
            first_ifd_offset: ifd_positions[0],
        };
        let updated_header_bytes = updated_header.to_bytes();
        self.file
            .write_all(&updated_header_bytes)
            .map_err(|e| OxiGdalError::Io(e.into()))?;

        // Flush
        self.file.flush().map_err(|e| OxiGdalError::Io(e.into()))?;

        Ok(())
    }

    /// Writes tiled data
    fn write_tiled_data(&mut self, data: &[u8]) -> Result<(Vec<u64>, Vec<u64>)> {
        let tile_width = self
            .config
            .tile_width
            .ok_or_else(|| OxiGdalError::InvalidParameter {
                parameter: "tile_width",
                message: "Tile width not set".to_string(),
            })?;

        let tile_height =
            self.config
                .tile_height
                .ok_or_else(|| OxiGdalError::InvalidParameter {
                    parameter: "tile_height",
                    message: "Tile height not set".to_string(),
                })?;

        let processor = TileProcessor::new(
            tile_width,
            tile_height,
            self.config.width,
            self.config.height,
            self.config.bytes_per_sample(),
            self.config.band_count as usize,
            self.config.compression,
            self.config.predictor,
        );

        let tiles = processor.process_all_tiles(data)?;

        let mut tile_offsets = Vec::with_capacity(tiles.len());
        let mut tile_byte_counts = Vec::with_capacity(tiles.len());

        for tile in tiles {
            let offset = self.position;
            self.file
                .write_all(&tile.data)
                .map_err(|e| OxiGdalError::Io(e.into()))?;

            tile_offsets.push(offset);
            tile_byte_counts.push(tile.data.len() as u64);
            self.position += tile.data.len() as u64;
        }

        Ok((tile_offsets, tile_byte_counts))
    }

    /// Adds baseline TIFF tags
    fn add_baseline_tags(&self, ifd: &mut IfdBuilder, width: u64, height: u64) -> Result<()> {
        // Image dimensions
        match self.variant {
            TiffVariant::Classic => {
                ifd.add_long(TiffTag::ImageWidth, width as u32);
                ifd.add_long(TiffTag::ImageLength, height as u32);
            }
            TiffVariant::BigTiff => {
                ifd.add_long8(TiffTag::ImageWidth, width);
                ifd.add_long8(TiffTag::ImageLength, height);
            }
        }

        // Bits per sample
        let bps = self.config.bits_per_sample();
        if self.config.band_count == 1 {
            ifd.add_short(TiffTag::BitsPerSample, bps);
        } else {
            let bps_array = vec![bps; self.config.band_count as usize];
            ifd.add_short_array(TiffTag::BitsPerSample, bps_array);
        }

        // Sample format
        ifd.add_short(TiffTag::SampleFormat, self.config.sample_format() as u16);

        // Samples per pixel
        ifd.add_short(TiffTag::SamplesPerPixel, self.config.band_count);

        // Compression
        ifd.add_short(TiffTag::Compression, self.config.compression as u16);

        // Photometric interpretation
        ifd.add_short(
            TiffTag::PhotometricInterpretation,
            self.config.photometric as u16,
        );

        // Planar configuration
        ifd.add_short(
            TiffTag::PlanarConfiguration,
            PlanarConfiguration::Chunky as u16,
        );

        // Predictor
        if self.config.predictor as u16 != 1 {
            ifd.add_short(TiffTag::Predictor, self.config.predictor as u16);
        }

        // Software tag
        ifd.add_ascii(TiffTag::Software, "OxiGDAL GeoTIFF Writer".to_string());

        Ok(())
    }

    /// Adds tile-related tags
    fn add_tile_tags(
        &self,
        ifd: &mut IfdBuilder,
        tile_offsets: &[u64],
        tile_byte_counts: &[u64],
    ) -> Result<()> {
        let tile_width = self
            .config
            .tile_width
            .ok_or_else(|| OxiGdalError::InvalidParameter {
                parameter: "tile_width",
                message: "Tile width not set".to_string(),
            })?;

        let tile_height =
            self.config
                .tile_height
                .ok_or_else(|| OxiGdalError::InvalidParameter {
                    parameter: "tile_height",
                    message: "Tile height not set".to_string(),
                })?;

        ifd.add_long(TiffTag::TileWidth, tile_width);
        ifd.add_long(TiffTag::TileLength, tile_height);

        match self.variant {
            TiffVariant::Classic => {
                let offsets: Vec<u32> = tile_offsets.iter().map(|&o| o as u32).collect();
                let counts: Vec<u32> = tile_byte_counts.iter().map(|&c| c as u32).collect();
                ifd.add_long_array(TiffTag::TileOffsets, offsets);
                ifd.add_long_array(TiffTag::TileByteCounts, counts);
            }
            TiffVariant::BigTiff => {
                ifd.add_long8_array(TiffTag::TileOffsets, tile_offsets.to_vec());
                ifd.add_long8_array(TiffTag::TileByteCounts, tile_byte_counts.to_vec());
            }
        }

        Ok(())
    }

    /// Adds GeoTIFF tags
    fn add_geotiff_tags(&self, ifd: &mut IfdBuilder) -> Result<()> {
        // Add GeoTransform if present
        if let Some(ref gt) = self.config.geo_transform {
            add_geo_transform(ifd, gt);
        }

        // Add GeoKeys if EPSG code is present
        if let Some(epsg_code) = self.config.epsg_code {
            let mut geokeys = GeoKeysBuilder::new();

            // Determine if projected or geographic based on EPSG code
            // Common projected: 32601-32660 (UTM North), 32701-32760 (UTM South)
            // Common geographic: 4326 (WGS84), 4269 (NAD83)
            let is_projected = (32601..=32760).contains(&epsg_code);

            geokeys.set_epsg_code(epsg_code, is_projected);
            geokeys.add_to_ifd(ifd);
        }

        Ok(())
    }

    /// Adds NoData tag
    fn add_nodata_tag(&self, ifd: &mut IfdBuilder) -> Result<()> {
        match self.config.nodata {
            NoDataValue::None => {}
            NoDataValue::Integer(val) => {
                ifd.add_ascii(TiffTag::GdalNodata, val.to_string());
            }
            NoDataValue::Float(val) => {
                ifd.add_ascii(TiffTag::GdalNodata, val.to_string());
            }
        }

        Ok(())
    }

    /// Generates overviews
    fn generate_overviews(&self, data: &[u8]) -> Result<Vec<OverviewData>> {
        if self.config.overview_levels.is_empty() {
            return Ok(Vec::new());
        }

        // Skip overviews for striped mode (not tiled)
        if self.config.tile_width.is_none() || self.config.tile_height.is_none() {
            tracing::warn!(
                "Skipping overview generation for striped TIFF - overviews require tiled mode"
            );
            return Ok(Vec::new());
        }

        let generator = OverviewGenerator::new(
            self.config.width,
            self.config.height,
            self.config.bytes_per_sample(),
            self.config.band_count as usize,
            self.config.overview_resampling,
            self.config.data_type,
        );

        let overviews = generator.generate_overviews(data, &self.config.overview_levels)?;

        let mut overview_data = Vec::new();
        for overview in overviews {
            // Process overview tiles
            let tile_width = self.config.tile_width.expect("Checked above");
            let tile_height = self.config.tile_height.expect("Checked above");

            let processor = TileProcessor::new(
                tile_width,
                tile_height,
                overview.width,
                overview.height,
                self.config.bytes_per_sample(),
                self.config.band_count as usize,
                self.config.compression,
                self.config.predictor,
            );

            let tiles = processor.process_all_tiles(&overview.data)?;

            overview_data.push(OverviewData {
                width: overview.width,
                height: overview.height,
                tiles,
            });
        }

        Ok(overview_data)
    }

    /// Writes striped data
    fn write_striped_data(&mut self, data: &[u8]) -> Result<(Vec<u64>, Vec<u64>)> {
        use crate::writer::tiles::StripProcessor;

        // Default to 16 rows per strip if not specified
        let rows_per_strip = 16;

        let processor = StripProcessor::new(
            self.config.width,
            self.config.height,
            rows_per_strip,
            self.config.bytes_per_sample(),
            self.config.band_count as usize,
            self.config.compression,
            self.config.predictor,
        );

        let strips = processor.process_all_strips(data)?;

        let mut strip_offsets = Vec::with_capacity(strips.len());
        let mut strip_byte_counts = Vec::with_capacity(strips.len());

        for strip in strips {
            let offset = self.position;
            self.file
                .write_all(&strip)
                .map_err(|e| OxiGdalError::Io(e.into()))?;

            strip_offsets.push(offset);
            strip_byte_counts.push(strip.len() as u64);
            self.position += strip.len() as u64;
        }

        Ok((strip_offsets, strip_byte_counts))
    }

    /// Builds IFD data as a byte vector
    fn build_ifd_data(
        &self,
        data_offsets: &[u64],
        data_byte_counts: &[u64],
        width: u64,
        height: u64,
        ifd_offset: u64,
        next_ifd_offset: u64,
    ) -> Result<Vec<u8>> {
        let mut ifd = IfdBuilder::new(self.options.byte_order, self.variant);

        // Baseline tags
        self.add_baseline_tags(&mut ifd, width, height)?;

        // Tile or strip tags
        if self.config.tile_width.is_some() {
            self.add_tile_tags(&mut ifd, data_offsets, data_byte_counts)?;
        } else {
            self.add_strip_tags(&mut ifd, data_offsets, data_byte_counts)?;
        }

        // GeoTIFF tags (only for primary image)
        if width == self.config.width && height == self.config.height {
            self.add_geotiff_tags(&mut ifd)?;
            self.add_nodata_tag(&mut ifd)?;
        }

        // Write to buffer
        let (ifd_bytes, data_bytes, _total_size) = ifd.write(ifd_offset, next_ifd_offset)?;

        // Combine IFD and data
        let mut combined = ifd_bytes;
        combined.extend_from_slice(&data_bytes);

        Ok(combined)
    }

    /// Updates the next_ifd_offset field in IFD bytes
    fn update_next_ifd_offset(&self, ifd_data: &mut [u8], next_offset: u64) {
        // The next_ifd_offset is at the end of the IFD
        // For Classic TIFF: after entry_count (2 bytes) + entries (12 bytes each)
        // For BigTIFF: after entry_count (8 bytes) + entries (20 bytes each)

        let entry_size = self.variant.ifd_entry_size();
        let count_size = match self.variant {
            TiffVariant::Classic => 2,
            TiffVariant::BigTiff => 8,
        };

        // Read entry count
        let entry_count = match self.variant {
            TiffVariant::Classic => self.options.byte_order.read_u16(&ifd_data[0..2]) as usize,
            TiffVariant::BigTiff => self.options.byte_order.read_u64(&ifd_data[0..8]) as usize,
        };

        let offset_pos = count_size + entry_count * entry_size;

        // Write next_ifd_offset
        match self.variant {
            TiffVariant::Classic => {
                self.options.byte_order.write_u32(
                    &mut ifd_data[offset_pos..offset_pos + 4],
                    next_offset as u32,
                );
            }
            TiffVariant::BigTiff => {
                self.options
                    .byte_order
                    .write_u64(&mut ifd_data[offset_pos..offset_pos + 8], next_offset);
            }
        }
    }

    /// Adds strip-related tags
    fn add_strip_tags(
        &self,
        ifd: &mut IfdBuilder,
        strip_offsets: &[u64],
        strip_byte_counts: &[u64],
    ) -> Result<()> {
        // Rows per strip (default 16)
        ifd.add_long(TiffTag::RowsPerStrip, 16);

        match self.variant {
            TiffVariant::Classic => {
                let offsets: Vec<u32> = strip_offsets.iter().map(|&o| o as u32).collect();
                let counts: Vec<u32> = strip_byte_counts.iter().map(|&c| c as u32).collect();
                ifd.add_long_array(TiffTag::StripOffsets, offsets);
                ifd.add_long_array(TiffTag::StripByteCounts, counts);
            }
            TiffVariant::BigTiff => {
                ifd.add_long8_array(TiffTag::StripOffsets, strip_offsets.to_vec());
                ifd.add_long8_array(TiffTag::StripByteCounts, strip_byte_counts.to_vec());
            }
        }

        Ok(())
    }
}

/// Overview data for writing
struct OverviewData {
    width: u64,
    height: u64,
    tiles: Vec<crate::writer::tiles::TileInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiff::Compression;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_writer_creation() {
        let config = WriterConfig::new(256, 256, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw);

        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_geotiff.tif");

        let result = GeoTiffWriter::create(&temp_file, config, GeoTiffWriterOptions::default());

        if result.is_ok() {
            // Clean up
            let _ = std::fs::remove_file(&temp_file);
        }
    }
}
