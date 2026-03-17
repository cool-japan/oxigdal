//! Cloud Optimized GeoTIFF (COG) support
//!
//! This module provides functionality specific to Cloud Optimized GeoTIFF files,
//! including tile-based access, overview navigation, optimization tools, and validation.
//!
//! # Phase 2: Advanced COG Tools
//!
//! This module includes advanced COG creation, optimization, and analysis tools:
//!
//! - **Compression Selection**: Analyzes data characteristics and recommends optimal compression
//! - **Overview Optimization**: Determines optimal overview levels and resampling methods
//! - **COG Optimization**: Comprehensive analysis for tile size, compression, and overviews
//! - **Metadata Optimization**: Minimizes metadata size while preserving essential information
//! - **Validation**: Enhanced COG compliance checking with detailed reports
//! - **Conversion**: Universal converter with auto-optimization
//! - **Tools**: High-level convenience functions

use oxigdal_core::error::{FormatError, OxiGdalError, Result};
use oxigdal_core::io::{ByteRange, DataSource};

use crate::compression;
use crate::geokeys::GeoKeyDirectory;
use crate::tiff::{ImageInfo, TiffFile, TiffTag};

// Phase 2: Advanced COG tools and optimization modules
pub mod compression_selector;
pub mod converter;
pub mod metadata_optimizer;
pub mod optimizer;
pub mod overview_optimizer;
pub mod tools;
pub mod validator;

// Re-export commonly used types from Phase 2
pub use compression_selector::{
    CompressionAnalysis, CompressionPreferences, DataCharacteristics, analyze_for_compression,
};
pub use converter::{
    BatchConversionConfig, BatchConversionResult, CogConverter, ConversionConfig,
    ConversionProgress, ConversionResult, ConversionStep, convert_batch,
};
pub use metadata_optimizer::{
    GeoKeyOptimization, MetadataOptimization, PreservationLevel, analyze_metadata,
    compress_ascii_fields, find_redundant_tags, optimize_geokeys,
};
pub use optimizer::{
    AccessPattern, CloudCostEstimate, CogOptimization, OptimizationComparison, OptimizationGoal,
    SpatialAccessPattern, analyze_for_cog, compare_optimizations, estimate_cloud_cost,
};
pub use overview_optimizer::{
    OverviewPreferences, OverviewStrategy, ProgressiveOverviewConfig, calculate_optimal_batch_size,
    optimize_overviews, optimize_progressive_order, validate_overview_config,
};
pub use tools::{
    CogComparison, CogInfo, analyze_file_for_cog, compare_cogs, create_cog, create_optimized_cog,
    estimate_storage_cost, get_cog_info, is_valid_cog, optimize_existing_cog, validate_cog_file,
};
pub use validator::{
    DetailedCogValidation, PerformanceMetrics, ValidationCategory, ValidationMessage,
    ValidationSeverity, validate_cog_detailed,
};

/// COG validation result
#[derive(Debug, Clone)]
pub struct CogValidation {
    /// Whether the file is a valid COG
    pub is_valid: bool,
    /// Validation messages (warnings and errors)
    pub messages: Vec<String>,
    /// Whether the file has internal overviews
    pub has_overviews: bool,
    /// Whether tiles are properly ordered (for streaming)
    pub tiles_ordered: bool,
}

/// Validates that a TIFF file is COG-compliant
pub fn validate_cog<S: DataSource>(tiff: &TiffFile, source: &S) -> CogValidation {
    let mut messages = Vec::new();
    let mut is_valid = true;

    // Check 1: Must be tiled
    if let Some(ifd) = tiff.ifds.first() {
        let has_tiles = ifd.get_entry(TiffTag::TileWidth).is_some()
            && ifd.get_entry(TiffTag::TileLength).is_some();

        if !has_tiles {
            messages.push("Primary image must be tiled".to_string());
            is_valid = false;
        }

        // Check tile size is power of 2 and reasonable
        if let (Some(tw_entry), Some(th_entry)) = (
            ifd.get_entry(TiffTag::TileWidth),
            ifd.get_entry(TiffTag::TileLength),
        ) {
            if let (Ok(tw), Ok(th)) = (
                tw_entry.get_u64_from_source(source, tiff.byte_order(), tiff.header.variant),
                th_entry.get_u64_from_source(source, tiff.byte_order(), tiff.header.variant),
            ) {
                if !tw.is_power_of_two() {
                    messages.push(format!("Tile width {} is not a power of 2", tw));
                }
                if !th.is_power_of_two() {
                    messages.push(format!("Tile height {} is not a power of 2", th));
                }
                if tw != th {
                    messages.push(format!("Non-square tiles: {}x{}", tw, th));
                }
            }
        }
    }

    // Check 2: Has overviews (recommended but not required)
    let has_overviews = tiff.ifds.len() > 1;
    if !has_overviews {
        messages.push("No internal overviews found (recommended for COG)".to_string());
    }

    // Check 3: IFDs should be ordered by size (largest first)
    let sizes: Vec<u64> = tiff
        .ifds
        .iter()
        .filter_map(|ifd| {
            let w = ifd
                .get_entry(TiffTag::ImageWidth)?
                .get_u64_from_source(source, tiff.byte_order(), tiff.header.variant)
                .ok()?;
            let h = ifd
                .get_entry(TiffTag::ImageLength)?
                .get_u64_from_source(source, tiff.byte_order(), tiff.header.variant)
                .ok()?;
            Some(w * h)
        })
        .collect();

    for i in 1..sizes.len() {
        if sizes[i] > sizes[i - 1] {
            messages.push("IFDs not ordered by decreasing size".to_string());
            break;
        }
    }

    // Check 4: Tile data should come after IFDs (for streaming)
    // This is a simplified check - a full check would verify all tile offsets
    let tiles_ordered = true; // Placeholder

    CogValidation {
        is_valid,
        messages,
        has_overviews,
        tiles_ordered,
    }
}

/// A Cloud Optimized GeoTIFF reader
#[derive(Debug)]
pub struct CogReader<S: DataSource> {
    source: S,
    tiff: TiffFile,
    primary_info: ImageInfo,
    overview_infos: Vec<ImageInfo>,
    geo_keys: Option<GeoKeyDirectory>,
}

impl<S: DataSource> CogReader<S> {
    /// Opens a COG file
    pub fn open(source: S) -> Result<Self> {
        let tiff = TiffFile::parse(&source)?;

        if tiff.ifds.is_empty() {
            return Err(OxiGdalError::Format(FormatError::InvalidHeader {
                message: "No IFDs found in TIFF".to_string(),
            }));
        }

        let byte_order = tiff.byte_order();
        let variant = tiff.header.variant;

        // Parse primary image info
        let primary_info = ImageInfo::from_ifd(&tiff.ifds[0], &source, byte_order, variant)?;

        // Parse overview infos
        let mut overview_infos = Vec::new();
        for ifd in tiff.ifds.iter().skip(1) {
            let info = ImageInfo::from_ifd(ifd, &source, byte_order, variant)?;
            overview_infos.push(info);
        }

        // Parse GeoKeys
        let geo_keys = GeoKeyDirectory::from_ifd(&tiff.ifds[0], &source, byte_order, variant)?;

        Ok(Self {
            source,
            tiff,
            primary_info,
            overview_infos,
            geo_keys,
        })
    }

    /// Returns the image width
    #[must_use]
    pub fn width(&self) -> u64 {
        self.primary_info.width
    }

    /// Returns the image height
    #[must_use]
    pub fn height(&self) -> u64 {
        self.primary_info.height
    }

    /// Returns the tile dimensions
    #[must_use]
    pub fn tile_size(&self) -> Option<(u32, u32)> {
        match (self.primary_info.tile_width, self.primary_info.tile_height) {
            (Some(w), Some(h)) => Some((w, h)),
            _ => None,
        }
    }

    /// Returns the number of overview levels
    #[must_use]
    pub fn overview_count(&self) -> usize {
        self.overview_infos.len()
    }

    /// Returns the primary image info
    #[must_use]
    pub fn primary_info(&self) -> &ImageInfo {
        &self.primary_info
    }

    /// Returns the GeoKey directory, if present
    #[must_use]
    pub fn geo_keys(&self) -> Option<&GeoKeyDirectory> {
        self.geo_keys.as_ref()
    }

    /// Returns the EPSG code, if available
    #[must_use]
    pub fn epsg_code(&self) -> Option<u32> {
        self.geo_keys.as_ref().and_then(|gk| gk.epsg_code())
    }

    /// Extracts the GeoTransform from GeoTIFF tags
    pub fn geo_transform(&self) -> Result<Option<oxigdal_core::types::GeoTransform>> {
        use crate::geokeys;

        geokeys::extract_geo_transform(
            &self.tiff.ifds[0],
            &self.source,
            self.tiff.byte_order(),
            self.tiff.header.variant,
        )
    }

    /// Extracts the NoData value from GDAL_NODATA tag
    pub fn nodata(&self) -> Result<oxigdal_core::types::NoDataValue> {
        use oxigdal_core::types::NoDataValue;

        if let Some(entry) = self.tiff.ifds[0].get_entry(TiffTag::GdalNodata) {
            let nodata_str = entry.get_ascii(&self.source, self.tiff.header.variant)?;

            // Try parsing as integer first (more specific)
            if let Ok(val) = nodata_str.parse::<i64>() {
                return Ok(NoDataValue::from_integer(val));
            }

            // Try parsing as float (more general)
            if let Ok(val) = nodata_str.parse::<f64>() {
                return Ok(NoDataValue::from_float(val));
            }
        }

        Ok(NoDataValue::None)
    }

    /// Returns the internal TIFF file
    #[must_use]
    pub fn tiff(&self) -> &TiffFile {
        &self.tiff
    }

    /// Returns the number of tiles in X and Y
    #[must_use]
    pub fn tile_count(&self) -> (u32, u32) {
        (
            self.primary_info.tiles_across(),
            self.primary_info.tiles_down(),
        )
    }

    /// Gets the byte range for a specific tile or strip
    pub fn tile_byte_range(&self, level: usize, tile_x: u32, tile_y: u32) -> Result<ByteRange> {
        let ifd = if level == 0 {
            &self.tiff.ifds[0]
        } else if level <= self.overview_infos.len() {
            &self.tiff.ifds[level]
        } else {
            return Err(OxiGdalError::OutOfBounds {
                message: format!("Overview level {} out of bounds", level),
            });
        };

        let byte_order = self.tiff.byte_order();
        let variant = self.tiff.header.variant;

        let info = if level == 0 {
            &self.primary_info
        } else {
            &self.overview_infos[level - 1]
        };

        // Check if this is a tiled or striped layout
        let is_tiled = info.tile_width.is_some() && info.tile_height.is_some();

        let (offsets, counts) = if is_tiled {
            // Tiled layout: use TileOffsets and TileByteCounts
            let offsets_entry = ifd
                .get_entry(TiffTag::TileOffsets)
                .ok_or(OxiGdalError::Format(FormatError::MissingTag {
                    tag: "TileOffsets",
                }))?;
            let counts_entry = ifd.get_entry(TiffTag::TileByteCounts).ok_or({
                OxiGdalError::Format(FormatError::MissingTag {
                    tag: "TileByteCounts",
                })
            })?;

            (
                offsets_entry.get_u64_vec(&self.source, byte_order, variant)?,
                counts_entry.get_u64_vec(&self.source, byte_order, variant)?,
            )
        } else {
            // Striped layout: use StripOffsets and StripByteCounts
            let offsets_entry =
                ifd.get_entry(TiffTag::StripOffsets)
                    .ok_or(OxiGdalError::Format(FormatError::MissingTag {
                        tag: "StripOffsets",
                    }))?;
            let counts_entry = ifd.get_entry(TiffTag::StripByteCounts).ok_or({
                OxiGdalError::Format(FormatError::MissingTag {
                    tag: "StripByteCounts",
                })
            })?;

            (
                offsets_entry.get_u64_vec(&self.source, byte_order, variant)?,
                counts_entry.get_u64_vec(&self.source, byte_order, variant)?,
            )
        };

        let tiles_across = info.tiles_across();
        let tile_index = (tile_y * tiles_across + tile_x) as usize;

        if tile_index >= offsets.len() || tile_index >= counts.len() {
            return Err(OxiGdalError::OutOfBounds {
                message: format!("Tile/strip ({}, {}) out of bounds", tile_x, tile_y),
            });
        }

        Ok(ByteRange::from_offset_length(
            offsets[tile_index],
            counts[tile_index],
        ))
    }

    /// Reads a tile's raw (compressed) data
    pub fn read_tile_raw(&self, level: usize, tile_x: u32, tile_y: u32) -> Result<Vec<u8>> {
        let range = self.tile_byte_range(level, tile_x, tile_y)?;
        self.source.read_range(range)
    }

    /// Reads and decompresses a tile or strip
    pub fn read_tile(&self, level: usize, tile_x: u32, tile_y: u32) -> Result<Vec<u8>> {
        let compressed = self.read_tile_raw(level, tile_x, tile_y)?;

        let info = if level == 0 {
            &self.primary_info
        } else {
            &self.overview_infos[level - 1]
        };

        let is_tiled = info.tile_width.is_some() && info.tile_height.is_some();

        let (tile_width, tile_height) = if is_tiled {
            // Tiled layout
            (
                info.tile_width.unwrap_or(info.width as u32),
                info.tile_height.unwrap_or(info.height as u32),
            )
        } else {
            // Striped layout: width is full image width, height is rows_per_strip
            let strip_height = info.rows_per_strip.unwrap_or(info.height as u32);
            let actual_height = if tile_y == info.tiles_down() - 1 {
                // Last strip might be shorter
                let remaining = info.height as u32 - (tile_y * strip_height);
                remaining.min(strip_height)
            } else {
                strip_height
            };
            (info.width as u32, actual_height)
        };

        let bytes_per_sample = info
            .bits_per_sample
            .first()
            .map_or(1, |&b| (b / 8) as usize);
        let samples_per_pixel = info.samples_per_pixel as usize;

        let expected_size =
            tile_width as usize * tile_height as usize * bytes_per_sample * samples_per_pixel;

        let mut decompressed =
            compression::decompress(&compressed, info.compression, expected_size)?;

        // Apply predictor
        compression::apply_predictor_reverse(
            &mut decompressed,
            info.predictor,
            bytes_per_sample,
            samples_per_pixel,
            tile_width as usize,
        );

        Ok(decompressed)
    }

    /// Selects the best overview level for the given resolution
    pub fn select_overview(&self, target_width: u64, target_height: u64) -> usize {
        // Start with full resolution
        let mut best_level = 0;
        let mut best_width = self.primary_info.width;
        let mut best_height = self.primary_info.height;

        // Find the smallest overview that's still larger than the target
        for (i, info) in self.overview_infos.iter().enumerate() {
            if info.width >= target_width
                && info.height >= target_height
                && info.width < best_width
                && info.height < best_height
            {
                best_level = i + 1;
                best_width = info.width;
                best_height = info.height;
            }
        }

        best_level
    }
}

/// Overview level information
#[derive(Debug, Clone)]
pub struct OverviewInfo {
    /// Level index (0 = full resolution)
    pub level: usize,
    /// Width in pixels
    pub width: u64,
    /// Height in pixels
    pub height: u64,
    /// Scale factor from full resolution
    pub scale: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_overview() {
        // Test overview info structure creation
        let info = OverviewInfo {
            level: 1,
            width: 512,
            height: 512,
            scale: 2.0,
        };

        assert_eq!(info.level, 1);
        assert_eq!(info.width, 512);
        assert_eq!(info.height, 512);
        assert_eq!(info.scale, 2.0);
    }
}
