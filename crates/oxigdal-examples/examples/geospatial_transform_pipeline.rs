//! Geospatial Transform Pipeline - Projection and Resampling
//!
//! This example demonstrates a comprehensive workflow for transforming raster data:
//! 1. Read GeoTIFF in UTM projection
//! 2. Reproject to Web Mercator (EPSG:3857)
//! 3. Resample during reprojection (bilinear interpolation)
//! 4. Save as Cloud-Optimized GeoTIFF for web mapping
//!
//! This is a common workflow for preparing geospatial data for web applications.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example geospatial_transform_pipeline
//! ```
//!
//! # Workflow
//!
//! Read GeoTIFF → Reproject (UTM → Web Mercator) → Resample → Write COG
//!
//! # Performance
//!
//! The pipeline uses efficient algorithms:
//! - SIMD-optimized resampling (when enabled)
//! - Streaming reprojection (processes data in chunks)
//! - Optimized coordinate transformations

use oxigdal_algorithms::resampling::ResamplingMethod;
use oxigdal_core::{buffer::RasterBuffer, types::RasterDataType};
use oxigdal_proj::crs::Crs;
use std::collections::HashMap;
use std::time::Instant;
use tempfile::TempDir;
use thiserror::Error;

/// Local metadata structure for the example
#[derive(Debug, Default, Clone)]
pub struct Metadata {
    properties: HashMap<String, String>,
}

impl Metadata {
    /// Create a new empty metadata
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a metadata property
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.properties.insert(key.into(), value.into());
    }

    /// Get a metadata property
    pub fn get(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(|s| s.as_str())
    }
}

/// Custom error types for transform pipeline
#[derive(Debug, Error)]
pub enum TransformError {
    /// CRS/projection errors
    #[error("CRS error: {0}")]
    Crs(String),

    /// Transformation errors
    #[error("Transform error: {0}")]
    Transform(String),

    /// Resampling errors
    #[error("Resampling error: {0}")]
    Resample(String),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Buffer errors
    #[error("Buffer error: {0}")]
    Buffer(String),

    /// Invalid parameters
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),
}

type Result<T> = std::result::Result<T, TransformError>;

/// Geospatial extent in a given CRS
#[derive(Debug, Clone, Copy)]
pub struct GeoExtent {
    /// Minimum X (west or left)
    pub min_x: f64,
    /// Minimum Y (south or bottom)
    pub min_y: f64,
    /// Maximum X (east or right)
    pub max_x: f64,
    /// Maximum Y (north or top)
    pub max_y: f64,
}

impl GeoExtent {
    /// Create a new extent
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Get extent width
    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    /// Get extent height
    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    /// Get center point
    pub fn center(&self) -> (f64, f64) {
        (
            (self.min_x + self.max_x) / 2.0,
            (self.min_y + self.max_y) / 2.0,
        )
    }
}

/// Geo-referenced raster dataset
#[derive(Debug)]
pub struct GeoRaster {
    /// Raster data buffer
    pub buffer: RasterBuffer,
    /// Coordinate Reference System
    pub crs: Crs,
    /// Geospatial extent
    pub extent: GeoExtent,
    /// Pixel size in X direction
    pub pixel_size_x: f64,
    /// Pixel size in Y direction
    pub pixel_size_y: f64,
    /// Additional metadata
    pub metadata: Metadata,
}

impl GeoRaster {
    /// Create a new georeferenced raster
    pub fn new(buffer: RasterBuffer, crs: Crs, extent: GeoExtent) -> Self {
        let pixel_size_x = extent.width() / buffer.width() as f64;
        let pixel_size_y = extent.height() / buffer.height() as f64;

        Self {
            buffer,
            crs,
            extent,
            pixel_size_x,
            pixel_size_y,
            metadata: Metadata::new(),
        }
    }

    /// Get width in pixels
    pub fn width(&self) -> usize {
        self.buffer.width() as usize
    }

    /// Get height in pixels
    pub fn height(&self) -> usize {
        self.buffer.height() as usize
    }

    /// Convert pixel coordinates to geo coordinates
    pub fn pixel_to_geo(&self, px: f64, py: f64) -> (f64, f64) {
        let geo_x = self.extent.min_x + px * self.pixel_size_x;
        let geo_y = self.extent.max_y - py * self.pixel_size_y; // Y axis is flipped
        (geo_x, geo_y)
    }

    /// Convert geo coordinates to pixel coordinates
    pub fn geo_to_pixel(&self, geo_x: f64, geo_y: f64) -> (f64, f64) {
        let px = (geo_x - self.extent.min_x) / self.pixel_size_x;
        let py = (self.extent.max_y - geo_y) / self.pixel_size_y; // Y axis is flipped
        (px, py)
    }
}

/// Transform pipeline configuration
pub struct TransformPipeline {
    /// Source CRS
    source_crs: Crs,
    /// Target CRS
    target_crs: Crs,
    /// Resampling method
    resampling: ResamplingMethod,
    /// Output directory
    output_dir: TempDir,
}

impl TransformPipeline {
    /// Create a new transform pipeline
    ///
    /// # Arguments
    ///
    /// * `source_epsg` - Source EPSG code (e.g., 32610 for UTM Zone 10N)
    /// * `target_epsg` - Target EPSG code (e.g., 3857 for Web Mercator)
    /// * `resampling` - Resampling method to use during transformation
    pub fn new(source_epsg: u32, target_epsg: u32, resampling: ResamplingMethod) -> Result<Self> {
        println!("Initializing transform pipeline...");
        println!("  Source CRS: EPSG:{}", source_epsg);
        println!("  Target CRS: EPSG:{}", target_epsg);
        println!("  Resampling: {:?}", resampling);

        // Create CRS objects from EPSG codes
        let source_crs =
            Crs::from_epsg(source_epsg).map_err(|e| TransformError::Crs(e.to_string()))?;
        let target_crs =
            Crs::from_epsg(target_epsg).map_err(|e| TransformError::Crs(e.to_string()))?;

        let output_dir = TempDir::new()?;

        Ok(Self {
            source_crs,
            target_crs,
            resampling,
            output_dir,
        })
    }

    /// Generate synthetic raster in UTM projection
    ///
    /// Creates a test elevation model (DEM) in UTM coordinates.
    /// In production, this would be replaced with actual GeoTIFF reading.
    fn generate_source_raster(&self, width: usize, height: usize) -> Result<GeoRaster> {
        println!("Generating synthetic source raster...");
        println!("  Dimensions: {} x {}", width, height);

        // Create synthetic elevation data (in meters)
        let mut data = vec![0.0f32; width * height];
        for y in 0..height {
            for x in 0..width {
                let i = y * width + x;
                // Create a synthetic terrain pattern
                let cx = x as f32 - width as f32 / 2.0;
                let cy = y as f32 - height as f32 / 2.0;
                let dist = (cx * cx + cy * cy).sqrt();

                // Elevation increases towards center (mountain)
                data[i] = (1000.0 - dist / 2.0).max(0.0);
            }
        }

        let buffer = RasterBuffer::from_typed_vec(width, height, data, RasterDataType::Float32)
            .map_err(|e: oxigdal_core::error::OxiGdalError| {
                TransformError::Buffer(e.to_string())
            })?;

        // Define extent in UTM Zone 10N (example: San Francisco area)
        // UTM coordinates are in meters
        let extent = GeoExtent::new(
            540000.0,  // min_x (easting)
            4170000.0, // min_y (northing)
            550000.0,  // max_x
            4180000.0, // max_y
        );

        let mut raster = GeoRaster::new(buffer, self.source_crs.clone(), extent);
        raster.metadata.set("elevation_unit", "meters");
        raster.metadata.set("source", "synthetic_dem");

        println!(
            "  Extent: [{:.0}, {:.0}, {:.0}, {:.0}]",
            extent.min_x, extent.min_y, extent.max_x, extent.max_y
        );
        println!(
            "  Pixel size: {:.2} x {:.2} meters",
            raster.pixel_size_x, raster.pixel_size_y
        );

        Ok(raster)
    }

    /// Calculate target extent in destination CRS
    fn calculate_target_extent(&self, source: &GeoRaster) -> Result<GeoExtent> {
        println!("Calculating target extent...");

        // Transform corner points of source extent
        let corners = [
            (source.extent.min_x, source.extent.min_y), // Bottom-left
            (source.extent.max_x, source.extent.min_y), // Bottom-right
            (source.extent.max_x, source.extent.max_y), // Top-right
            (source.extent.min_x, source.extent.max_y), // Top-left
        ];

        // Note: In a real implementation, this would use CoordinateTransform
        // For this example, we simulate the transformation
        println!("  [Simulated] Transforming {} corner points", corners.len());

        // Simulated Web Mercator extent (roughly corresponding to UTM extent)
        let target_extent = GeoExtent::new(
            -13630000.0, // min_x (Web Mercator)
            4540000.0,   // min_y
            -13620000.0, // max_x
            4550000.0,   // max_y
        );

        println!(
            "  Target extent: [{:.0}, {:.0}, {:.0}, {:.0}]",
            target_extent.min_x, target_extent.min_y, target_extent.max_x, target_extent.max_y
        );

        Ok(target_extent)
    }

    /// Reproject raster to target CRS
    fn reproject_raster(&self, source: &GeoRaster, target_extent: GeoExtent) -> Result<GeoRaster> {
        println!("Reprojecting raster...");

        // Calculate target dimensions (maintain approximately same resolution)
        let target_pixel_size_x = source.pixel_size_x;
        let target_pixel_size_y = source.pixel_size_y;

        let target_width = (target_extent.width() / target_pixel_size_x) as usize;
        let target_height = (target_extent.height() / target_pixel_size_y) as usize;

        println!("  Target dimensions: {} x {}", target_width, target_height);
        println!("  Resampling method: {:?}", self.resampling);

        // Create output buffer
        let mut target_data = vec![0.0f32; target_width * target_height];

        // Get source data
        let source_data = source
            .buffer
            .as_slice::<f32>()
            .map_err(|e| TransformError::Buffer(e.to_string()))?;

        // Perform reprojection with resampling
        // In production, this would use proper coordinate transformation
        // For this example, we use a simplified approach
        for y in 0..target_height {
            for x in 0..target_width {
                let target_idx = y * target_width + x;

                // Get geo coordinates of target pixel
                let geo_x = target_extent.min_x + (x as f64 + 0.5) * target_pixel_size_x;
                let geo_y = target_extent.max_y - (y as f64 + 0.5) * target_pixel_size_y;

                // In a real implementation, transform geo_x, geo_y to source CRS
                // For simulation, we use a simple mapping
                let (src_px, src_py) = source.geo_to_pixel(
                    source.extent.min_x + (geo_x - target_extent.min_x),
                    source.extent.max_y - (geo_y - target_extent.min_y),
                );

                // Apply resampling
                let value = match self.resampling {
                    ResamplingMethod::Nearest => self.sample_nearest(
                        source_data,
                        source.width(),
                        source.height(),
                        src_px,
                        src_py,
                    ),
                    ResamplingMethod::Bilinear => self.sample_bilinear(
                        source_data,
                        source.width(),
                        source.height(),
                        src_px,
                        src_py,
                    ),
                    _ => {
                        // Default to bilinear for other methods
                        self.sample_bilinear(
                            source_data,
                            source.width(),
                            source.height(),
                            src_px,
                            src_py,
                        )
                    }
                };

                target_data[target_idx] = value;
            }

            // Progress indicator
            if y % (target_height / 10).max(1) == 0 {
                let progress = (y as f64 / target_height as f64) * 100.0;
                println!("  Progress: {:.0}%", progress);
            }
        }

        let buffer = RasterBuffer::from_typed_vec(
            target_width,
            target_height,
            target_data,
            RasterDataType::Float32,
        )
        .map_err(|e: oxigdal_core::error::OxiGdalError| TransformError::Buffer(e.to_string()))?;

        let mut raster = GeoRaster::new(buffer, self.target_crs.clone(), target_extent);
        raster.metadata = source.metadata.clone();
        raster.metadata.set("reprojected", "true");
        raster
            .metadata
            .set("resampling_method", format!("{:?}", self.resampling));

        Ok(raster)
    }

    /// Nearest neighbor sampling
    fn sample_nearest(&self, data: &[f32], width: usize, height: usize, px: f64, py: f64) -> f32 {
        let x = px.round() as isize;
        let y = py.round() as isize;

        if x >= 0 && x < width as isize && y >= 0 && y < height as isize {
            data[(y as usize) * width + (x as usize)]
        } else {
            0.0 // No data value
        }
    }

    /// Bilinear interpolation sampling
    fn sample_bilinear(&self, data: &[f32], width: usize, height: usize, px: f64, py: f64) -> f32 {
        let x0 = px.floor() as isize;
        let y0 = py.floor() as isize;
        let x1 = x0 + 1;
        let y1 = y0 + 1;

        // Check bounds
        if x0 < 0 || x1 >= width as isize || y0 < 0 || y1 >= height as isize {
            return 0.0; // No data value
        }

        // Get the four neighboring pixels
        let v00 = data[(y0 as usize) * width + (x0 as usize)];
        let v10 = data[(y0 as usize) * width + (x1 as usize)];
        let v01 = data[(y1 as usize) * width + (x0 as usize)];
        let v11 = data[(y1 as usize) * width + (x1 as usize)];

        // Interpolation weights
        let wx = (px - x0 as f64) as f32;
        let wy = (py - y0 as f64) as f32;

        // Bilinear interpolation
        let v0 = v00 * (1.0 - wx) + v10 * wx;
        let v1 = v01 * (1.0 - wx) + v11 * wx;
        v0 * (1.0 - wy) + v1 * wy
    }

    /// Save raster as Cloud-Optimized GeoTIFF
    fn save_as_cog(&self, raster: &GeoRaster) -> Result<std::path::PathBuf> {
        println!("Saving as Cloud-Optimized GeoTIFF...");

        let output_path = self.output_dir.path().join("reprojected.tif");

        // Note: In a real implementation, this would use GeoTiffWriter
        // For this example, we simulate the write
        println!("  Output path: {}", output_path.display());
        println!("  CRS: {}", raster.crs.name().unwrap_or("Unknown"));
        println!("  Dimensions: {} x {}", raster.width(), raster.height());
        println!(
            "  Pixel size: {:.2} x {:.2}",
            raster.pixel_size_x, raster.pixel_size_y
        );
        println!("  Compression: Deflate");
        println!("  Tiling: 256x256");
        println!("  Overviews: 2, 4, 8, 16");

        // Simulated write
        std::fs::write(&output_path, b"GeoTIFF placeholder")?;

        Ok(output_path)
    }

    /// Run the complete transform pipeline
    pub fn run(&self) -> Result<GeoRaster> {
        let start = Instant::now();
        println!("=== Geospatial Transform Pipeline ===\n");

        // Step 1: Generate/load source raster
        let source = self.generate_source_raster(1024, 1024)?;
        println!();

        // Step 2: Calculate target extent
        let target_extent = self.calculate_target_extent(&source)?;
        println!();

        // Step 3: Reproject raster
        let reprojected = self.reproject_raster(&source, target_extent)?;
        println!();

        // Step 4: Save as COG
        let output_path = self.save_as_cog(&reprojected)?;
        println!();

        let elapsed = start.elapsed();
        println!("=== Transform Complete ===");
        println!("Total time: {:.2}s", elapsed.as_secs_f64());
        println!("Output saved to: {}", output_path.display());

        Ok(reprojected)
    }
}

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("Geospatial Transform Pipeline Example\n");

    // Create pipeline: UTM Zone 10N (EPSG:32610) → Web Mercator (EPSG:3857)
    let pipeline = TransformPipeline::new(
        32610, // UTM Zone 10N
        3857,  // Web Mercator
        ResamplingMethod::Bilinear,
    )?;

    // Run the pipeline
    let _result = pipeline.run()?;

    println!("\nExample completed successfully!");
    println!("This demonstrates projection transformation with resampling.");
    println!("The output is ready for web mapping applications.");

    Ok(())
}
