//! GeoTIFF and COG writing functionality
//!
//! This module provides writers for creating GeoTIFF and Cloud Optimized GeoTIFF files.

mod cog_writer;
mod geokeys_writer;
mod geotiff_writer;
mod ifd_writer;
mod overviews;
mod tiles;

pub use cog_writer::{CogWriter, CogWriterOptions};
pub use geotiff_writer::{GeoTiffWriter, GeoTiffWriterOptions};
pub use overviews::{OverviewGenerator, OverviewResampling};

use oxigdal_core::error::{OxiGdalError, Result};
use oxigdal_core::types::{GeoTransform, NoDataValue, RasterDataType};

use crate::tiff::{Compression, PhotometricInterpretation, Predictor, SampleFormat};

/// Writer configuration shared between GeoTIFF and COG writers
#[derive(Debug, Clone)]
pub struct WriterConfig {
    /// Image width in pixels
    pub width: u64,
    /// Image height in pixels
    pub height: u64,
    /// Number of bands (samples per pixel)
    pub band_count: u16,
    /// Data type
    pub data_type: RasterDataType,
    /// Compression scheme
    pub compression: Compression,
    /// Predictor for compression
    pub predictor: Predictor,
    /// Tile width (None for striped)
    pub tile_width: Option<u32>,
    /// Tile height (None for striped)
    pub tile_height: Option<u32>,
    /// Photometric interpretation
    pub photometric: PhotometricInterpretation,
    /// GeoTransform
    pub geo_transform: Option<GeoTransform>,
    /// EPSG code
    pub epsg_code: Option<u32>,
    /// NoData value
    pub nodata: NoDataValue,
    /// Use BigTIFF (required for files > 4GB)
    pub use_bigtiff: bool,
    /// Generate overviews
    pub generate_overviews: bool,
    /// Overview resampling method
    pub overview_resampling: OverviewResampling,
    /// Overview levels (e.g., [2, 4, 8, 16])
    pub overview_levels: Vec<u32>,
}

impl WriterConfig {
    /// Creates a new writer configuration
    #[must_use]
    pub fn new(width: u64, height: u64, band_count: u16, data_type: RasterDataType) -> Self {
        Self {
            width,
            height,
            band_count,
            data_type,
            compression: Compression::Lzw,
            predictor: Predictor::HorizontalDifferencing,
            tile_width: Some(256),
            tile_height: Some(256),
            photometric: PhotometricInterpretation::BlackIsZero,
            geo_transform: None,
            epsg_code: None,
            nodata: NoDataValue::None,
            use_bigtiff: false,
            generate_overviews: true,
            overview_resampling: OverviewResampling::Average,
            overview_levels: vec![2, 4, 8, 16],
        }
    }

    /// Sets the compression scheme
    #[must_use]
    pub const fn with_compression(mut self, compression: Compression) -> Self {
        self.compression = compression;
        self
    }

    /// Sets the predictor
    #[must_use]
    pub const fn with_predictor(mut self, predictor: Predictor) -> Self {
        self.predictor = predictor;
        self
    }

    /// Sets the tile size
    #[must_use]
    pub const fn with_tile_size(mut self, width: u32, height: u32) -> Self {
        self.tile_width = Some(width);
        self.tile_height = Some(height);
        self
    }

    /// Sets the photometric interpretation
    #[must_use]
    pub const fn with_photometric(mut self, photometric: PhotometricInterpretation) -> Self {
        self.photometric = photometric;
        self
    }

    /// Sets the GeoTransform
    #[must_use]
    pub const fn with_geo_transform(mut self, geo_transform: GeoTransform) -> Self {
        self.geo_transform = Some(geo_transform);
        self
    }

    /// Sets the EPSG code
    #[must_use]
    pub const fn with_epsg_code(mut self, epsg_code: u32) -> Self {
        self.epsg_code = Some(epsg_code);
        self
    }

    /// Sets the NoData value
    #[must_use]
    pub const fn with_nodata(mut self, nodata: NoDataValue) -> Self {
        self.nodata = nodata;
        self
    }

    /// Enables BigTIFF
    #[must_use]
    pub const fn with_bigtiff(mut self, use_bigtiff: bool) -> Self {
        self.use_bigtiff = use_bigtiff;
        self
    }

    /// Sets overview generation
    #[must_use]
    pub const fn with_overviews(mut self, generate: bool, resampling: OverviewResampling) -> Self {
        self.generate_overviews = generate;
        self.overview_resampling = resampling;
        self
    }

    /// Sets overview levels
    #[must_use]
    pub fn with_overview_levels(mut self, levels: Vec<u32>) -> Self {
        self.overview_levels = levels;
        self
    }

    /// Validates the configuration
    pub fn validate(&self) -> Result<()> {
        if self.width == 0 || self.height == 0 {
            return Err(OxiGdalError::invalid_parameter_builder(
                "width/height",
                "Image dimensions must be > 0",
            )
            .with_operation("validate_writer_config")
            .with_parameter("width", self.width.to_string())
            .with_parameter("height", self.height.to_string())
            .with_suggestion("Set valid image dimensions before writing TIFF")
            .build());
        }

        if self.band_count == 0 {
            return Err(OxiGdalError::invalid_parameter_builder(
                "band_count",
                "Band count must be > 0",
            )
            .with_operation("validate_writer_config")
            .with_parameter("band_count", "0")
            .with_suggestion("Set number of bands (e.g., 1 for grayscale, 3 for RGB)")
            .build());
        }

        if let (Some(tw), Some(th)) = (self.tile_width, self.tile_height) {
            if tw == 0 || th == 0 {
                return Err(OxiGdalError::invalid_parameter_builder(
                    "tile_size",
                    "Tile dimensions must be > 0",
                )
                .with_operation("validate_writer_config")
                .with_parameter("tile_width", tw.to_string())
                .with_parameter("tile_height", th.to_string())
                .with_suggestion("Use standard tile sizes like 256x256 or 512x512 (power of 2)")
                .build());
            }

            // Check if power of 2 for COG compliance
            if !tw.is_power_of_two() || !th.is_power_of_two() {
                tracing::warn!(
                    "Tile dimensions {}x{} are not power of 2, may not be COG-compliant",
                    tw,
                    th
                );
            }
        }

        Ok(())
    }

    /// Returns the sample format for the data type
    #[must_use]
    pub const fn sample_format(&self) -> SampleFormat {
        match self.data_type {
            RasterDataType::UInt8
            | RasterDataType::UInt16
            | RasterDataType::UInt32
            | RasterDataType::UInt64 => SampleFormat::UnsignedInteger,
            RasterDataType::Int8
            | RasterDataType::Int16
            | RasterDataType::Int32
            | RasterDataType::Int64 => SampleFormat::SignedInteger,
            RasterDataType::Float32 | RasterDataType::Float64 => SampleFormat::IeeeFloatingPoint,
            RasterDataType::CFloat32 | RasterDataType::CFloat64 => {
                SampleFormat::ComplexFloatingPoint
            }
        }
    }

    /// Returns the bits per sample for the data type
    #[must_use]
    pub const fn bits_per_sample(&self) -> u16 {
        match self.data_type {
            RasterDataType::UInt8 | RasterDataType::Int8 => 8,
            RasterDataType::UInt16 | RasterDataType::Int16 => 16,
            RasterDataType::UInt32 | RasterDataType::Int32 | RasterDataType::Float32 => 32,
            RasterDataType::UInt64
            | RasterDataType::Int64
            | RasterDataType::Float64
            | RasterDataType::CFloat32 => 64,
            RasterDataType::CFloat64 => 128,
        }
    }

    /// Returns the bytes per sample
    #[must_use]
    pub const fn bytes_per_sample(&self) -> usize {
        (self.bits_per_sample() / 8) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_writer_config_validation() {
        let config = WriterConfig::new(1024, 1024, 1, RasterDataType::UInt8);
        assert!(config.validate().is_ok());

        let invalid = WriterConfig::new(0, 1024, 1, RasterDataType::UInt8);
        assert!(invalid.validate().is_err());

        let invalid_bands = WriterConfig::new(1024, 1024, 0, RasterDataType::UInt8);
        assert!(invalid_bands.validate().is_err());
    }

    #[test]
    fn test_writer_config_builder() {
        let config = WriterConfig::new(1024, 1024, 3, RasterDataType::UInt8)
            .with_compression(Compression::Deflate)
            .with_predictor(Predictor::HorizontalDifferencing)
            .with_tile_size(512, 512)
            .with_epsg_code(4326);

        assert_eq!(config.compression, Compression::Deflate);
        assert_eq!(config.predictor, Predictor::HorizontalDifferencing);
        assert_eq!(config.tile_width, Some(512));
        assert_eq!(config.tile_height, Some(512));
        assert_eq!(config.epsg_code, Some(4326));
    }

    #[test]
    fn test_sample_format() {
        let uint8 = WriterConfig::new(1, 1, 1, RasterDataType::UInt8);
        assert_eq!(uint8.sample_format(), SampleFormat::UnsignedInteger);

        let float32 = WriterConfig::new(1, 1, 1, RasterDataType::Float32);
        assert_eq!(float32.sample_format(), SampleFormat::IeeeFloatingPoint);

        let int16 = WriterConfig::new(1, 1, 1, RasterDataType::Int16);
        assert_eq!(int16.sample_format(), SampleFormat::SignedInteger);
    }

    #[test]
    fn test_bits_per_sample() {
        assert_eq!(
            WriterConfig::new(1, 1, 1, RasterDataType::UInt8).bits_per_sample(),
            8
        );
        assert_eq!(
            WriterConfig::new(1, 1, 1, RasterDataType::UInt16).bits_per_sample(),
            16
        );
        assert_eq!(
            WriterConfig::new(1, 1, 1, RasterDataType::Float32).bits_per_sample(),
            32
        );
        assert_eq!(
            WriterConfig::new(1, 1, 1, RasterDataType::Float64).bits_per_sample(),
            64
        );
    }
}
