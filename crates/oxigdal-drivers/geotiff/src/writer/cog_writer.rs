//! Cloud Optimized GeoTIFF (COG) writer implementation
//!
//! This module provides a writer that ensures COG-compliant layout:
//! - All IFDs before tile data
//! - Overviews in descending size order
//! - Tiled organization with power-of-2 tile sizes

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use oxigdal_core::error::{OxiGdalError, Result};

use crate::cog::{CogValidation, validate_cog};
use crate::tiff::{ByteOrderType, PlanarConfiguration, TiffFile, TiffHeader, TiffTag, TiffVariant};
use crate::writer::WriterConfig;
use crate::writer::geokeys_writer::{GeoKeysBuilder, add_geo_transform};
use crate::writer::ifd_writer::IfdBuilder;
use crate::writer::overviews::OverviewGenerator;
use crate::writer::tiles::TileProcessor;

/// Options for COG writer
#[derive(Debug, Clone)]
pub struct CogWriterOptions {
    /// Byte order (default: little-endian)
    pub byte_order: ByteOrderType,
    /// Validate COG compliance after writing
    pub validate_after_write: bool,
}

impl Default for CogWriterOptions {
    fn default() -> Self {
        Self {
            byte_order: ByteOrderType::LittleEndian,
            validate_after_write: true,
        }
    }
}

/// Cloud Optimized GeoTIFF writer
pub struct CogWriter {
    /// Output file path (for validation)
    path: std::path::PathBuf,
    /// Output file
    file: File,
    /// Configuration
    config: WriterConfig,
    /// Options
    options: CogWriterOptions,
    /// TIFF variant
    variant: TiffVariant,
}

impl CogWriter {
    /// Creates a new COG writer
    ///
    /// # Errors
    /// Returns an error if the file cannot be created or configuration is invalid
    pub fn create<P: AsRef<Path>>(
        path: P,
        config: WriterConfig,
        options: CogWriterOptions,
    ) -> Result<Self> {
        config.validate()?;

        // Validate COG-specific requirements
        Self::validate_cog_config(&config)?;

        let file = File::create(path.as_ref()).map_err(|e| OxiGdalError::Io(e.into()))?;

        let variant = if config.use_bigtiff {
            TiffVariant::BigTiff
        } else {
            TiffVariant::Classic
        };

        Ok(Self {
            path: path.as_ref().to_path_buf(),
            file,
            config,
            options,
            variant,
        })
    }

    /// Validates COG-specific configuration
    fn validate_cog_config(config: &WriterConfig) -> Result<()> {
        // Must be tiled
        if config.tile_width.is_none() || config.tile_height.is_none() {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "tile_size",
                message: "COG requires tiled organization".to_string(),
            });
        }

        let tile_width = config
            .tile_width
            .ok_or_else(|| OxiGdalError::InvalidParameter {
                parameter: "tile_width",
                message: "Tile width required".to_string(),
            })?;

        let tile_height = config
            .tile_height
            .ok_or_else(|| OxiGdalError::InvalidParameter {
                parameter: "tile_height",
                message: "Tile height required".to_string(),
            })?;

        // Tiles should be power of 2
        if !tile_width.is_power_of_two() || !tile_height.is_power_of_two() {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "tile_size",
                message: format!(
                    "COG requires power-of-2 tile sizes, got {}x{}",
                    tile_width, tile_height
                ),
            });
        }

        // Tiles should be square
        if tile_width != tile_height {
            tracing::warn!(
                "COG best practices recommend square tiles, got {}x{}",
                tile_width,
                tile_height
            );
        }

        // Should have overviews for large images
        if (config.width > 512 || config.height > 512) && !config.generate_overviews {
            tracing::warn!("COG best practices recommend overviews for images larger than 512x512");
        }

        Ok(())
    }

    /// Writes raster data as COG
    ///
    /// # Arguments
    /// * `data` - Raster data (row-major, interleaved if multi-band)
    ///
    /// # Errors
    /// Returns an error if writing fails
    pub fn write(&mut self, data: &[u8]) -> Result<CogValidation> {
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

        // Generate overviews first
        let overview_data = if self.config.generate_overviews {
            self.generate_overviews(data)?
        } else {
            Vec::new()
        };

        // Process all tiles for all levels
        let primary_tiles = self.process_tiles(data, self.config.width, self.config.height)?;

        let mut all_level_tiles = vec![primary_tiles];
        for overview in &overview_data {
            let tiles = self.process_tiles(&overview.data, overview.width, overview.height)?;
            all_level_tiles.push(tiles);
        }

        // Write header (placeholder)
        let header = TiffHeader {
            byte_order: self.options.byte_order,
            variant: self.variant,
            first_ifd_offset: 0,
        };
        let header_bytes = header.to_bytes();
        self.file
            .write_all(&header_bytes)
            .map_err(|e| OxiGdalError::Io(e.into()))?;
        let mut position = header_bytes.len() as u64;

        // Build temporary IFDs with DUMMY tile offsets to calculate correct sizes
        // This is critical: we need realistic tile offset arrays to get accurate IFD sizes
        let mut ifd_data_list = Vec::new();

        // Calculate number of tiles for primary image
        let tile_w = u64::from(self.config.tile_width.unwrap_or(256));
        let tile_h = u64::from(self.config.tile_height.unwrap_or(256));
        let primary_tiles_across = self.config.width.div_ceil(tile_w);
        let primary_tiles_down = self.config.height.div_ceil(tile_h);
        let primary_tile_count = (primary_tiles_across * primary_tiles_down) as usize;

        // Create dummy offsets with correct array sizes
        let dummy_primary_offsets = vec![0u64; primary_tile_count];
        let dummy_primary_counts = vec![0u64; primary_tile_count];

        // Primary IFD with realistic dummy offsets
        let primary_ifd = self.build_ifd(
            &dummy_primary_offsets,
            &dummy_primary_counts,
            self.config.width,
            self.config.height,
            0, // Temporary ifd_offset for size calculation
            0, // Temporary next_ifd_offset
        )?;
        ifd_data_list.push(primary_ifd);

        // Overview IFDs with realistic dummy offsets
        for overview in &overview_data {
            let ov_tiles_across = overview.width.div_ceil(tile_w);
            let ov_tiles_down = overview.height.div_ceil(tile_h);
            let ov_tile_count = (ov_tiles_across * ov_tiles_down) as usize;

            let dummy_ov_offsets = vec![0u64; ov_tile_count];
            let dummy_ov_counts = vec![0u64; ov_tile_count];

            let ifd = self.build_ifd(
                &dummy_ov_offsets,
                &dummy_ov_counts,
                overview.width,
                overview.height,
                0, // Temporary ifd_offset for size calculation
                0, // Temporary next_ifd_offset
            )?;
            ifd_data_list.push(ifd);
        }

        // Calculate IFD area size
        let ifd_area_start = position;
        let ifd_area_size: usize = ifd_data_list.iter().map(|ifd| ifd.len()).sum();
        position += ifd_area_size as u64;

        // Write all tile data and collect offsets
        let mut all_tile_offsets = Vec::new();
        let mut all_tile_byte_counts = Vec::new();

        for level_tiles in &all_level_tiles {
            let mut offsets = Vec::new();
            let mut byte_counts = Vec::new();

            for tile in level_tiles {
                self.file
                    .seek(SeekFrom::Start(position))
                    .map_err(|e| OxiGdalError::Io(e.into()))?;

                self.file
                    .write_all(&tile.data)
                    .map_err(|e| OxiGdalError::Io(e.into()))?;

                offsets.push(position);
                byte_counts.push(tile.data.len() as u64);
                position += tile.data.len() as u64;
            }

            all_tile_offsets.push(offsets);
            all_tile_byte_counts.push(byte_counts);
        }

        // Rebuild IFDs with actual tile offsets and correct next_ifd_offset values
        ifd_data_list.clear();

        let mut ifd_positions = Vec::new();
        let mut current_pos = ifd_area_start;

        // Calculate all IFD positions first
        let primary_ifd = self.build_ifd(
            &all_tile_offsets[0],
            &all_tile_byte_counts[0],
            self.config.width,
            self.config.height,
            current_pos, // Actual IFD offset
            0,           // Temporary next_ifd_offset
        )?;
        ifd_positions.push(current_pos);
        current_pos += primary_ifd.len() as u64;

        for (i, overview) in overview_data.iter().enumerate() {
            let ifd = self.build_ifd(
                &all_tile_offsets[i + 1],
                &all_tile_byte_counts[i + 1],
                overview.width,
                overview.height,
                current_pos, // Actual IFD offset
                0,           // Temporary next_ifd_offset
            )?;
            ifd_positions.push(current_pos);
            current_pos += ifd.len() as u64;
        }

        // Build final IFDs with correct next_ifd_offset values
        ifd_data_list.clear();

        // Primary IFD
        let next_offset = if overview_data.is_empty() {
            0
        } else {
            ifd_positions[1]
        };
        let primary_ifd = self.build_ifd(
            &all_tile_offsets[0],
            &all_tile_byte_counts[0],
            self.config.width,
            self.config.height,
            ifd_positions[0], // Actual IFD offset
            next_offset,
        )?;
        ifd_data_list.push(primary_ifd);

        // Overview IFDs
        for (i, overview) in overview_data.iter().enumerate() {
            let is_last = i == overview_data.len() - 1;
            let next = if is_last { 0 } else { ifd_positions[i + 2] };

            let ifd = self.build_ifd(
                &all_tile_offsets[i + 1],
                &all_tile_byte_counts[i + 1],
                overview.width,
                overview.height,
                ifd_positions[i + 1], // Actual IFD offset
                next,
            )?;
            ifd_data_list.push(ifd);
        }

        // Now write all IFDs
        self.file
            .seek(SeekFrom::Start(ifd_area_start))
            .map_err(|e| OxiGdalError::Io(e.into()))?;

        for ifd_data in &ifd_data_list {
            self.file
                .write_all(ifd_data)
                .map_err(|e| OxiGdalError::Io(e.into()))?;
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

        // Validate if requested
        if self.options.validate_after_write {
            self.validate()
        } else {
            Ok(CogValidation {
                is_valid: true,
                messages: vec!["Validation skipped".to_string()],
                has_overviews: !overview_data.is_empty(),
                tiles_ordered: true,
            })
        }
    }

    /// Processes tiles for a given level
    fn process_tiles(
        &self,
        data: &[u8],
        width: u64,
        height: u64,
    ) -> Result<Vec<crate::writer::tiles::TileInfo>> {
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
            width,
            height,
            self.config.bytes_per_sample(),
            self.config.band_count as usize,
            self.config.compression,
            self.config.predictor,
        );

        processor.process_all_tiles(data)
    }

    /// Builds an IFD
    fn build_ifd(
        &self,
        tile_offsets: &[u64],
        tile_byte_counts: &[u64],
        width: u64,
        height: u64,
        ifd_offset: u64,
        next_ifd_offset: u64,
    ) -> Result<Vec<u8>> {
        let mut ifd = IfdBuilder::new(self.options.byte_order, self.variant);

        // Baseline tags
        self.add_baseline_tags(&mut ifd, width, height)?;

        // Tile tags
        self.add_tile_tags(&mut ifd, tile_offsets, tile_byte_counts)?;

        // GeoTIFF tags (only for primary image)
        if width == self.config.width && height == self.config.height {
            self.add_geotiff_tags(&mut ifd)?;
            self.add_nodata_tag(&mut ifd)?;
        }

        // Write to buffer with correct IFD offset
        let (ifd_bytes, data_bytes, _size) = ifd.write(ifd_offset, next_ifd_offset)?;

        // Combine IFD and data
        let mut combined = ifd_bytes;
        combined.extend_from_slice(&data_bytes);

        Ok(combined)
    }

    /// Adds baseline tags (same as GeoTiffWriter)
    fn add_baseline_tags(&self, ifd: &mut IfdBuilder, width: u64, height: u64) -> Result<()> {
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

        let bps = self.config.bits_per_sample();
        if self.config.band_count == 1 {
            ifd.add_short(TiffTag::BitsPerSample, bps);
        } else {
            let bps_array = vec![bps; self.config.band_count as usize];
            ifd.add_short_array(TiffTag::BitsPerSample, bps_array);
        }

        ifd.add_short(TiffTag::SampleFormat, self.config.sample_format() as u16);
        ifd.add_short(TiffTag::SamplesPerPixel, self.config.band_count);
        ifd.add_short(TiffTag::Compression, self.config.compression as u16);
        ifd.add_short(
            TiffTag::PhotometricInterpretation,
            self.config.photometric as u16,
        );
        ifd.add_short(
            TiffTag::PlanarConfiguration,
            PlanarConfiguration::Chunky as u16,
        );

        if self.config.predictor as u16 != 1 {
            ifd.add_short(TiffTag::Predictor, self.config.predictor as u16);
        }

        ifd.add_ascii(TiffTag::Software, "OxiGDAL COG Writer".to_string());

        Ok(())
    }

    /// Adds tile tags
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
        if let Some(ref gt) = self.config.geo_transform {
            add_geo_transform(ifd, gt);
        }

        if let Some(epsg_code) = self.config.epsg_code {
            let mut geokeys = GeoKeysBuilder::new();
            let is_projected = (32601..=32760).contains(&epsg_code);
            geokeys.set_epsg_code(epsg_code, is_projected);
            geokeys.add_to_ifd(ifd);
        }

        Ok(())
    }

    /// Adds NoData tag
    fn add_nodata_tag(&self, ifd: &mut IfdBuilder) -> Result<()> {
        use oxigdal_core::types::NoDataValue;

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

        let generator = OverviewGenerator::new(
            self.config.width,
            self.config.height,
            self.config.bytes_per_sample(),
            self.config.band_count as usize,
            self.config.overview_resampling,
            self.config.data_type,
        );

        let overviews = generator.generate_overviews(data, &self.config.overview_levels)?;

        Ok(overviews
            .into_iter()
            .map(|ov| OverviewData {
                width: ov.width,
                height: ov.height,
                data: ov.data,
            })
            .collect())
    }

    /// Validates the written COG
    fn validate(&self) -> Result<CogValidation> {
        // Re-open and parse the file
        use oxigdal_core::io::FileDataSource;

        let source = FileDataSource::open(&self.path)?;
        let tiff = TiffFile::parse(&source)?;
        let validation = validate_cog(&tiff, &source);

        Ok(validation)
    }
}

/// Overview data
struct OverviewData {
    width: u64,
    height: u64,
    data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiff::Compression;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_cog_config_validation() {
        // Valid config
        let config = WriterConfig::new(1024, 1024, 1, RasterDataType::UInt8)
            .with_tile_size(256, 256)
            .with_compression(Compression::Lzw);
        assert!(CogWriter::validate_cog_config(&config).is_ok());

        // Invalid: not tiled
        let no_tiles = WriterConfig {
            width: 1024,
            height: 1024,
            band_count: 1,
            data_type: RasterDataType::UInt8,
            compression: Compression::Lzw,
            predictor: crate::tiff::Predictor::None,
            tile_width: None,
            tile_height: None,
            photometric: crate::tiff::PhotometricInterpretation::BlackIsZero,
            geo_transform: None,
            epsg_code: None,
            nodata: oxigdal_core::types::NoDataValue::None,
            use_bigtiff: false,
            generate_overviews: false,
            overview_resampling: crate::writer::OverviewResampling::Average,
            overview_levels: vec![],
        };
        assert!(CogWriter::validate_cog_config(&no_tiles).is_err());

        // Invalid: non-power-of-2 tiles
        let bad_tiles =
            WriterConfig::new(1024, 1024, 1, RasterDataType::UInt8).with_tile_size(100, 100);
        assert!(CogWriter::validate_cog_config(&bad_tiles).is_err());
    }
}
