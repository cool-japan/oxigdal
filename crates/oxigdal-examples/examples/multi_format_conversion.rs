//! Multi-Format Conversion Pipeline
//!
//! This example demonstrates format conversion workflows:
//! 1. Read data from multiple formats (GeoTIFF, Zarr, NetCDF)
//! 2. Apply filters and enhancements

#![allow(missing_docs)]
//! 3. Convert between formats
//! 4. Preserve metadata during conversion
//!
//! Common use cases:
//! - Converting legacy formats to cloud-native formats
//! - Preparing data for different analysis tools
//! - Optimizing storage and access patterns
//!
//! # Usage
//!
//! ```bash
//! cargo run --example multi_format_conversion
//! ```
//!
//! # Workflow
//!
//! Read (Any Format) → Process → Write (Target Format)
//!
//! # Supported Formats
//!
//! - GeoTIFF (raster)
//! - Zarr (array storage)
//! - NetCDF (scientific data)
//! - COG (cloud-optimized GeoTIFF)

use oxigdal_core::{buffer::RasterBuffer, types::RasterDataType};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
use tempfile::TempDir;
use thiserror::Error;

/// Simple metadata container for the example
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    values: HashMap<String, String>,
}

impl Metadata {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.values.insert(key.to_string(), value.to_string());
    }

    #[allow(dead_code)]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|s| s.as_str())
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// Custom error types for conversion pipeline
#[derive(Debug, Error)]
pub enum ConversionError {
    /// Format not supported
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    /// Read errors
    #[error("Failed to read from {0}: {1}")]
    Read(String, String),

    /// Write errors
    #[error("Failed to write to {0}: {1}")]
    Write(String, String),

    /// Conversion errors
    #[error("Conversion error: {0}")]
    Conversion(String),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Buffer errors
    #[error("Buffer error: {0}")]
    Buffer(String),

    /// Metadata errors
    #[error("Metadata error: {0}")]
    Metadata(String),
}

type Result<T> = std::result::Result<T, ConversionError>;

/// Supported raster formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterFormat {
    /// GeoTIFF format
    GeoTiff,
    /// Cloud-Optimized GeoTIFF
    Cog,
    /// Zarr array storage
    Zarr,
    /// NetCDF format
    NetCdf,
    /// HDF5 format
    Hdf5,
}

impl RasterFormat {
    /// Get format from file extension
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "tif" | "tiff" => Some(Self::GeoTiff),
            "zarr" => Some(Self::Zarr),
            "nc" | "netcdf" => Some(Self::NetCdf),
            "h5" | "hdf5" => Some(Self::Hdf5),
            _ => None,
        }
    }

    /// Get file extension for format
    pub fn extension(&self) -> &str {
        match self {
            Self::GeoTiff => "tif",
            Self::Cog => "tif",
            Self::Zarr => "zarr",
            Self::NetCdf => "nc",
            Self::Hdf5 => "h5",
        }
    }

    /// Get format name
    pub fn name(&self) -> &str {
        match self {
            Self::GeoTiff => "GeoTIFF",
            Self::Cog => "Cloud-Optimized GeoTIFF",
            Self::Zarr => "Zarr",
            Self::NetCdf => "NetCDF",
            Self::Hdf5 => "HDF5",
        }
    }

    /// Check if format is cloud-native
    pub fn is_cloud_native(&self) -> bool {
        matches!(self, Self::Cog | Self::Zarr)
    }
}

/// Raster dataset with format information
#[derive(Debug)]
pub struct RasterDataset {
    /// Raster data buffer
    pub buffer: RasterBuffer,
    /// Source format
    pub source_format: RasterFormat,
    /// Metadata
    pub metadata: Metadata,
    /// Dataset name
    pub name: String,
}

impl RasterDataset {
    /// Create a new raster dataset
    pub fn new(buffer: RasterBuffer, source_format: RasterFormat, name: impl Into<String>) -> Self {
        Self {
            buffer,
            source_format,
            metadata: Metadata::new(),
            name: name.into(),
        }
    }

    /// Get dimensions
    pub fn dimensions(&self) -> (usize, usize) {
        (self.buffer.width() as usize, self.buffer.height() as usize)
    }

    /// Get data type
    pub fn data_type(&self) -> RasterDataType {
        self.buffer.data_type()
    }

    /// Calculate statistics
    pub fn calculate_statistics(&self) -> Result<DatasetStatistics> {
        println!("  Calculating statistics for '{}'...", self.name);

        let (width, height) = self.dimensions();
        let data_type = self.data_type();

        // Calculate min, max, mean for demonstration
        let (min, max, mean) = match data_type {
            RasterDataType::Float32 => {
                let data = self
                    .buffer
                    .as_slice::<f32>()
                    .map_err(|e| ConversionError::Buffer(e.to_string()))?;
                self.calculate_stats_f32(data)
            }
            RasterDataType::UInt8 => {
                let data = self
                    .buffer
                    .as_slice::<u8>()
                    .map_err(|e| ConversionError::Buffer(e.to_string()))?;
                self.calculate_stats_u8(data)
            }
            _ => (0.0, 0.0, 0.0),
        };

        Ok(DatasetStatistics {
            width,
            height,
            min,
            max,
            mean,
        })
    }

    fn calculate_stats_f32(&self, data: &[f32]) -> (f64, f64, f64) {
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        let mut sum = 0.0f64;
        let mut count = 0;

        for &val in data {
            if val.is_finite() {
                min = min.min(val);
                max = max.max(val);
                sum += val as f64;
                count += 1;
            }
        }

        let mean = if count > 0 { sum / count as f64 } else { 0.0 };
        (min as f64, max as f64, mean)
    }

    fn calculate_stats_u8(&self, data: &[u8]) -> (f64, f64, f64) {
        let mut min = u8::MAX;
        let mut max = u8::MIN;
        let mut sum = 0u64;

        for &val in data {
            min = min.min(val);
            max = max.max(val);
            sum += val as u64;
        }

        let mean = sum as f64 / data.len() as f64;
        (min as f64, max as f64, mean)
    }
}

/// Dataset statistics
#[derive(Debug)]
pub struct DatasetStatistics {
    pub width: usize,
    pub height: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
}

/// Format conversion pipeline
pub struct ConversionPipeline {
    /// Output directory
    output_dir: TempDir,
    /// Processing options
    options: ConversionOptions,
}

/// Conversion options
#[derive(Debug, Clone)]
pub struct ConversionOptions {
    /// Apply histogram equalization
    pub apply_enhancement: bool,
    /// Preserve all metadata
    pub preserve_metadata: bool,
    /// Target compression (if applicable)
    pub compression: Option<String>,
    /// Create overviews (for GeoTIFF/COG)
    pub create_overviews: bool,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            apply_enhancement: false,
            preserve_metadata: true,
            compression: Some("deflate".to_string()),
            create_overviews: true,
        }
    }
}

impl ConversionPipeline {
    /// Create a new conversion pipeline
    pub fn new(options: ConversionOptions) -> Result<Self> {
        let output_dir = TempDir::new()?;
        Ok(Self {
            output_dir,
            options,
        })
    }

    /// Generate sample data in GeoTIFF format
    fn generate_geotiff_data(&self) -> Result<RasterDataset> {
        println!("Generating sample GeoTIFF data...");

        let width = 512;
        let height = 512;
        let mut data = vec![0.0f32; width * height];

        // Create a gradient pattern
        for y in 0..height {
            for x in 0..width {
                let i = y * width + x;
                let val = ((x + y) as f32 / (width + height) as f32) * 255.0;
                data[i] = val;
            }
        }

        let buffer = RasterBuffer::from_typed_vec(width, height, data, RasterDataType::Float32)
            .map_err(|e| ConversionError::Buffer(e.to_string()))?;

        let mut dataset = RasterDataset::new(buffer, RasterFormat::GeoTiff, "elevation");
        dataset.metadata.set("units", "meters");
        dataset.metadata.set("nodata", "-9999");

        println!("  Created {} x {} GeoTIFF", width, height);

        Ok(dataset)
    }

    /// Generate sample data in Zarr format
    fn generate_zarr_data(&self) -> Result<RasterDataset> {
        println!("Generating sample Zarr data...");

        let width = 256;
        let height = 256;
        let mut data = vec![0u8; width * height];

        // Create a checkerboard pattern
        for y in 0..height {
            for x in 0..width {
                let i = y * width + x;
                data[i] = if (x / 32 + y / 32) % 2 == 0 { 255 } else { 0 };
            }
        }

        let buffer = RasterBuffer::from_typed_vec(width, height, data, RasterDataType::UInt8)
            .map_err(|e| ConversionError::Buffer(e.to_string()))?;

        let mut dataset = RasterDataset::new(buffer, RasterFormat::Zarr, "landcover");
        dataset.metadata.set("chunks", "64x64");
        dataset.metadata.set("compressor", "blosc");

        println!("  Created {} x {} Zarr array", width, height);

        Ok(dataset)
    }

    /// Generate sample data in NetCDF format
    fn generate_netcdf_data(&self) -> Result<RasterDataset> {
        println!("Generating sample NetCDF data...");

        let width = 360;
        let height = 180;
        let mut data = vec![0.0f32; width * height];

        // Create a sinusoidal pattern (simulating temperature)
        for y in 0..height {
            for x in 0..width {
                let i = y * width + x;
                let lat = (y as f32 / height as f32 - 0.5) * std::f32::consts::PI;
                let lon = (x as f32 / width as f32) * 2.0 * std::f32::consts::PI;
                data[i] = 20.0 + 15.0 * lat.cos() + 5.0 * lon.sin();
            }
        }

        let buffer = RasterBuffer::from_typed_vec(width, height, data, RasterDataType::Float32)
            .map_err(|e| ConversionError::Buffer(e.to_string()))?;

        let mut dataset = RasterDataset::new(buffer, RasterFormat::NetCdf, "temperature");
        dataset.metadata.set("units", "celsius");
        dataset.metadata.set("standard_name", "air_temperature");
        dataset.metadata.set("time", "2024-01-15T00:00:00Z");

        println!("  Created {} x {} NetCDF variable", width, height);

        Ok(dataset)
    }

    /// Apply enhancements to dataset
    fn apply_enhancements(&self, dataset: &mut RasterDataset) -> Result<()> {
        if !self.options.apply_enhancement {
            return Ok(());
        }

        println!("  Applying enhancements...");

        // Calculate statistics first (requires immutable borrow)
        let stats = dataset.calculate_statistics()?;
        let range = stats.max - stats.min;

        // Then apply enhancement (requires mutable borrow)
        if range > 0.0 && dataset.data_type() == RasterDataType::Float32 {
            let data = dataset
                .buffer
                .as_slice_mut::<f32>()
                .map_err(|e| ConversionError::Buffer(e.to_string()))?;

            // Simple contrast stretch
            for val in data.iter_mut() {
                if val.is_finite() {
                    *val = ((*val as f64 - stats.min) / range * 255.0) as f32;
                }
            }
        }

        println!("    Enhancement applied");
        Ok(())
    }

    /// Convert dataset to target format
    fn convert_to_format(
        &self,
        dataset: &RasterDataset,
        target_format: RasterFormat,
    ) -> Result<PathBuf> {
        println!(
            "Converting {} to {}...",
            dataset.source_format.name(),
            target_format.name()
        );

        let filename = format!(
            "{}_{}.{}",
            dataset.name,
            target_format.name().to_lowercase().replace(' ', "_"),
            target_format.extension()
        );

        let output_path = self.output_dir.path().join(&filename);

        // Simulate format-specific writing
        match target_format {
            RasterFormat::GeoTiff | RasterFormat::Cog => {
                println!("  Writing GeoTIFF...");
                println!(
                    "    Dimensions: {} x {}",
                    dataset.buffer.width(),
                    dataset.buffer.height()
                );
                println!("    Data type: {:?}", dataset.data_type());
                if let Some(ref compression) = self.options.compression {
                    println!("    Compression: {}", compression);
                }
                if self.options.create_overviews {
                    println!("    Overviews: 2, 4, 8, 16");
                }
                if target_format == RasterFormat::Cog {
                    println!("    Tiling: 512x512");
                }
            }
            RasterFormat::Zarr => {
                println!("  Writing Zarr...");
                println!(
                    "    Array shape: ({}, {})",
                    dataset.buffer.height(),
                    dataset.buffer.width()
                );
                println!("    Chunks: (64, 64)");
                println!("    Compressor: blosc");
            }
            RasterFormat::NetCdf => {
                println!("  Writing NetCDF...");
                println!("    Variable: {}", dataset.name);
                println!(
                    "    Dimensions: lat({}), lon({})",
                    dataset.buffer.height(),
                    dataset.buffer.width()
                );
                println!("    Compression: 6");
            }
            RasterFormat::Hdf5 => {
                println!("  Writing HDF5...");
                println!("    Dataset: /{}", dataset.name);
                println!(
                    "    Shape: ({}, {})",
                    dataset.buffer.height(),
                    dataset.buffer.width()
                );
            }
        }

        // Preserve metadata if requested
        if self.options.preserve_metadata {
            println!("    Preserving metadata: {} keys", dataset.metadata.len());
        }

        // Simulated write
        std::fs::write(&output_path, b"Data placeholder")?;
        println!("  ✓ Saved to: {}", output_path.display());

        Ok(output_path)
    }

    /// Run conversion workflow
    pub fn run_conversion(
        &self,
        mut dataset: RasterDataset,
        target_format: RasterFormat,
    ) -> Result<PathBuf> {
        println!("\n--- Converting {} ---", dataset.name);

        // Calculate and print statistics
        let stats = dataset.calculate_statistics()?;
        println!("  Dimensions: {} x {}", stats.width, stats.height);
        println!("  Data range: [{:.2}, {:.2}]", stats.min, stats.max);
        println!("  Mean value: {:.2}", stats.mean);

        // Apply enhancements
        self.apply_enhancements(&mut dataset)?;

        // Convert to target format
        let output_path = self.convert_to_format(&dataset, target_format)?;

        Ok(output_path)
    }

    /// Run the complete multi-format conversion pipeline
    pub fn run(&self) -> Result<Vec<PathBuf>> {
        let start = Instant::now();
        println!("=== Multi-Format Conversion Pipeline ===\n");

        let mut output_files = Vec::new();

        // Scenario 1: GeoTIFF → COG (cloud optimization)
        println!("Scenario 1: GeoTIFF → Cloud-Optimized GeoTIFF");
        let geotiff_data = self.generate_geotiff_data()?;
        let cog_output = self.run_conversion(geotiff_data, RasterFormat::Cog)?;
        output_files.push(cog_output);

        // Scenario 2: Zarr → NetCDF (scientific format)
        println!("\nScenario 2: Zarr → NetCDF");
        let zarr_data = self.generate_zarr_data()?;
        let netcdf_output = self.run_conversion(zarr_data, RasterFormat::NetCdf)?;
        output_files.push(netcdf_output);

        // Scenario 3: NetCDF → Zarr (cloud-native)
        println!("\nScenario 3: NetCDF → Zarr");
        let netcdf_data = self.generate_netcdf_data()?;
        let zarr_output = self.run_conversion(netcdf_data, RasterFormat::Zarr)?;
        output_files.push(zarr_output);

        let elapsed = start.elapsed();
        println!("\n=== Conversion Complete ===");
        println!("Converted {} datasets", output_files.len());
        println!("Total time: {:.2}s", elapsed.as_secs_f64());
        println!("\nOutput files:");
        for (i, path) in output_files.iter().enumerate() {
            println!("  {}. {}", i + 1, path.display());
        }

        Ok(output_files)
    }
}

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("Multi-Format Conversion Pipeline Example\n");

    // Configure conversion options
    let options = ConversionOptions {
        apply_enhancement: true,
        preserve_metadata: true,
        compression: Some("deflate".to_string()),
        create_overviews: true,
    };

    // Create pipeline
    let pipeline = ConversionPipeline::new(options)?;

    // Run conversions
    let _outputs = pipeline.run()?;

    println!("\nExample completed successfully!");
    println!("This demonstrates format conversion workflows:");
    println!("  - GeoTIFF ↔ COG");
    println!("  - Zarr ↔ NetCDF");
    println!("  - Metadata preservation");
    println!("  - Format-specific optimizations");

    Ok(())
}
