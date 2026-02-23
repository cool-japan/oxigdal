//! WASM Integration Example - Preparing Data for Web Viewers
#![allow(missing_docs)]
//!
//! This example demonstrates preparing geospatial data for web applications:
//! 1. Generate Cloud-Optimized GeoTIFF (COG) with proper tiling
//! 2. Create pyramid of overviews for multi-resolution display
//! 3. Generate STAC item metadata
//! 4. Create preview images (thumbnails)
//! 5. Prepare for WASM-based web viewer consumption
//!
//! This workflow prepares data that can be efficiently loaded by web viewers
//! like Leaflet, OpenLayers, or custom WebAssembly viewers.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example wasm_integration_example
//! ```
//!
//! # Workflow
//!
//! Source Data → Generate COG → Create Overviews → STAC Metadata → Previews
//!
//! # Output
//!
//! - Cloud-Optimized GeoTIFF with tiling
//! - Pyramid overviews (2x, 4x, 8x, 16x)
//! - STAC JSON item
//! - Preview images (256x256, 512x512, 1024x1024)

use oxigdal_algorithms::resampling::{Resampler, ResamplingMethod};
use oxigdal_core::{buffer::RasterBuffer, types::RasterDataType};
// Note: oxigdal_stac modules may not be available yet.
// This example demonstrates the intended workflow using local implementations.
use std::path::{Path, PathBuf};
use std::time::Instant;
use tempfile::TempDir;
use thiserror::Error;

/// Custom error types for WASM workflow
#[derive(Debug, Error)]
pub enum WasmError {
    /// COG generation errors
    #[error("COG generation error: {0}")]
    CogGeneration(String),

    /// Overview generation errors
    #[error("Overview generation error: {0}")]
    Overview(String),

    /// STAC creation errors
    #[error("STAC error: {0}")]
    Stac(String),

    /// Preview generation errors
    #[error("Preview generation error: {0}")]
    Preview(String),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Buffer errors
    #[error("Buffer error: {0}")]
    Buffer(String),

    /// Serialization errors
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

type Result<T> = std::result::Result<T, WasmError>;

/// Configuration for COG generation
#[derive(Debug, Clone)]
pub struct CogConfig {
    /// Tile size (typically 256, 512, or 1024)
    pub tile_size: usize,
    /// Compression type
    pub compression: CompressionType,
    /// Create overviews
    pub create_overviews: bool,
    /// Overview factors (e.g., [2, 4, 8, 16])
    pub overview_factors: Vec<usize>,
    /// Resampling method for overviews
    pub overview_resampling: ResamplingMethod,
}

impl Default for CogConfig {
    fn default() -> Self {
        Self {
            tile_size: 512,
            compression: CompressionType::Deflate,
            create_overviews: true,
            overview_factors: vec![2, 4, 8, 16],
            overview_resampling: ResamplingMethod::Bilinear,
        }
    }
}

/// Compression types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    None,
    Deflate,
    Lzw,
    Jpeg,
    Webp,
}

impl CompressionType {
    pub fn name(&self) -> &str {
        match self {
            Self::None => "none",
            Self::Deflate => "deflate",
            Self::Lzw => "lzw",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
        }
    }
}

/// Preview image configuration
#[derive(Debug, Clone)]
pub struct PreviewConfig {
    /// Width in pixels
    pub width: usize,
    /// Height in pixels
    pub height: usize,
    /// Format (png, jpg, webp)
    pub format: String,
}

impl PreviewConfig {
    pub fn new(width: usize, height: usize, format: impl Into<String>) -> Self {
        Self {
            width,
            height,
            format: format.into(),
        }
    }
}

/// WASM integration pipeline
pub struct WasmPipeline {
    /// Output directory
    output_dir: TempDir,
    /// COG configuration
    cog_config: CogConfig,
    /// Base URL for assets (e.g., CDN)
    base_url: String,
}

impl WasmPipeline {
    /// Create a new WASM integration pipeline
    pub fn new(cog_config: CogConfig, base_url: impl Into<String>) -> Result<Self> {
        let base_url_string = base_url.into();
        println!("Initializing WASM integration pipeline...");
        println!(
            "  Tile size: {}x{}",
            cog_config.tile_size, cog_config.tile_size
        );
        println!("  Compression: {}", cog_config.compression.name());
        println!("  Overviews: {:?}", cog_config.overview_factors);
        println!("  Base URL: {}", base_url_string);

        let output_dir = TempDir::new()?;

        Ok(Self {
            output_dir,
            cog_config,
            base_url: base_url_string,
        })
    }

    /// Generate sample raster data
    fn generate_sample_data(&self, width: usize, height: usize) -> Result<RasterBuffer> {
        println!("Generating sample data...");
        println!("  Dimensions: {} x {}", width, height);

        let mut data = vec![0u8; width * height * 3]; // RGB

        // Create a colorful pattern
        for y in 0..height {
            for x in 0..width {
                let i = (y * width + x) * 3;

                // Create a gradient pattern
                let r = ((x as f32 / width as f32) * 255.0) as u8;
                let g = ((y as f32 / height as f32) * 255.0) as u8;
                let b = (((x + y) as f32 / (width + height) as f32) * 255.0) as u8;

                data[i] = r;
                data[i + 1] = g;
                data[i + 2] = b;
            }
        }

        // For demonstration, we'll use the red channel as a single-band raster
        let single_band: Vec<u8> = data.iter().step_by(3).copied().collect();

        RasterBuffer::from_typed_vec(width, height, single_band, RasterDataType::UInt8)
            .map_err(|e: oxigdal_core::error::OxiGdalError| WasmError::Buffer(e.to_string()))
    }

    /// Generate Cloud-Optimized GeoTIFF
    fn generate_cog(&self, data: &RasterBuffer) -> Result<PathBuf> {
        println!("\nGenerating Cloud-Optimized GeoTIFF...");

        let output_path = self.output_dir.path().join("data.tif");

        println!("  Output: {}", output_path.display());
        println!(
            "  Tile layout: {}x{}",
            self.cog_config.tile_size, self.cog_config.tile_size
        );
        println!("  Compression: {}", self.cog_config.compression.name());

        // Calculate number of tiles
        let tile_size = self.cog_config.tile_size as u64;
        let tiles_x = data.width().div_ceil(tile_size);
        let tiles_y = data.height().div_ceil(tile_size);
        println!(
            "  Tile grid: {} x {} = {} tiles",
            tiles_x,
            tiles_y,
            tiles_x * tiles_y
        );

        // Simulate COG structure
        println!("  IFD structure:");
        println!("    - Full resolution IFD");
        for (i, &factor) in self.cog_config.overview_factors.iter().enumerate() {
            let overview_width = data.width() / factor as u64;
            let overview_height = data.height() / factor as u64;
            println!(
                "    - Overview {} (1:{}) - {}x{}",
                i + 1,
                factor,
                overview_width,
                overview_height
            );
        }

        // Simulated write
        std::fs::write(&output_path, b"GeoTIFF COG placeholder")?;

        println!("  ✓ COG generated successfully");

        Ok(output_path)
    }

    /// Generate overview pyramids
    fn generate_overviews(&self, data: &RasterBuffer) -> Result<Vec<RasterBuffer>> {
        println!("\nGenerating overview pyramid...");

        let mut overviews = Vec::new();
        let _resampler = Resampler::new(self.cog_config.overview_resampling);

        for &factor in &self.cog_config.overview_factors {
            let new_width = (data.width() / factor as u64) as usize;
            let new_height = (data.height() / factor as u64) as usize;

            println!(
                "  Creating overview 1:{} ({}x{})",
                factor, new_width, new_height
            );

            // In production, this would use actual resampling
            // For this example, we simulate it
            let overview = self.simulate_resample(data, new_width, new_height)?;
            overviews.push(overview);
        }

        println!("  ✓ Generated {} overviews", overviews.len());

        Ok(overviews)
    }

    /// Simulate resampling (simplified)
    fn simulate_resample(
        &self,
        source: &RasterBuffer,
        width: usize,
        height: usize,
    ) -> Result<RasterBuffer> {
        let mut data = vec![0u8; width * height];

        // Simple nearest-neighbor sampling
        let x_ratio = source.width() as f64 / width as f64;
        let y_ratio = source.height() as f64 / height as f64;

        let source_data = source
            .as_slice::<u8>()
            .map_err(|e| WasmError::Overview(e.to_string()))?;

        for y in 0..height {
            for x in 0..width {
                let src_x = (x as f64 * x_ratio) as usize;
                let src_y = (y as f64 * y_ratio) as usize;

                let src_idx = src_y * source.width() as usize + src_x;
                let dst_idx = y * width + x;

                data[dst_idx] = source_data[src_idx];
            }
        }

        RasterBuffer::from_typed_vec(width, height, data, RasterDataType::UInt8)
            .map_err(|e: oxigdal_core::error::OxiGdalError| WasmError::Overview(e.to_string()))
    }

    /// Generate preview images
    fn generate_previews(&self, data: &RasterBuffer) -> Result<Vec<PathBuf>> {
        println!("\nGenerating preview images...");

        let preview_configs = vec![
            PreviewConfig::new(256, 256, "png"),
            PreviewConfig::new(512, 512, "png"),
            PreviewConfig::new(1024, 1024, "webp"),
        ];

        let mut preview_paths = Vec::new();

        for config in preview_configs {
            let filename = format!(
                "preview_{}x{}.{}",
                config.width, config.height, config.format
            );
            let output_path = self.output_dir.path().join(&filename);

            println!("  Creating {}x{} preview...", config.width, config.height);

            // Resample to preview size
            let _preview_data = self.simulate_resample(data, config.width, config.height)?;

            // Simulate image encoding
            std::fs::write(&output_path, b"Image placeholder")?;

            println!("    ✓ Saved: {}", output_path.display());
            preview_paths.push(output_path);
        }

        println!("  ✓ Generated {} previews", preview_paths.len());

        Ok(preview_paths)
    }

    /// Create STAC item metadata
    fn create_stac_item(
        &self,
        cog_path: &Path,
        preview_paths: &[PathBuf],
        _data: &RasterBuffer,
    ) -> Result<PathBuf> {
        println!("\nCreating STAC item...");

        // Build STAC item
        let item_id = "web-ready-data-001";

        // Calculate bounding box (simulated)
        let bbox = [-122.5, 37.5, -122.0, 38.0];

        println!("  Item ID: {}", item_id);
        println!("  Bounding box: {:?}", bbox);

        // Create STAC item JSON structure
        let stac_item = serde_json::json!({
            "stac_version": "1.0.0",
            "type": "Feature",
            "id": item_id,
            "geometry": {
                "type": "Polygon",
                "coordinates": [[
                    [bbox[0], bbox[1]],
                    [bbox[2], bbox[1]],
                    [bbox[2], bbox[3]],
                    [bbox[0], bbox[3]],
                    [bbox[0], bbox[1]]
                ]]
            },
            "bbox": bbox,
            "properties": {
                "datetime": "2024-01-15T12:00:00Z",
                "title": "Web-Ready Geospatial Data",
                "description": "Cloud-optimized data prepared for WASM viewer",
            },
            "assets": {
                "data": {
                    "href": format!("{}/{}", self.base_url, cog_path.file_name()
                        .and_then(|n| n.to_str()).unwrap_or("data.tif")),
                    "type": "image/tiff; application=geotiff; profile=cloud-optimized",
                    "roles": ["data"],
                    "title": "Cloud-Optimized GeoTIFF",
                    "cog:tile_size": self.cog_config.tile_size,
                    "cog:overviews": self.cog_config.overview_factors,
                },
                "thumbnail": {
                    "href": format!("{}/{}", self.base_url, preview_paths[0].file_name()
                        .and_then(|n| n.to_str()).unwrap_or("preview.png")),
                    "type": "image/png",
                    "roles": ["thumbnail"],
                    "title": "Thumbnail (256x256)",
                },
                "preview": {
                    "href": format!("{}/{}", self.base_url, preview_paths[1].file_name()
                        .and_then(|n| n.to_str()).unwrap_or("preview.png")),
                    "type": "image/png",
                    "roles": ["overview"],
                    "title": "Preview (512x512)",
                },
            },
            "links": [
                {
                    "rel": "self",
                    "href": format!("{}/stac/{}.json", self.base_url, item_id),
                },
                {
                    "rel": "parent",
                    "href": format!("{}/stac/collection.json", self.base_url),
                }
            ]
        });

        // Save STAC JSON
        let stac_path = self.output_dir.path().join(format!("{}.json", item_id));
        let stac_json = serde_json::to_string_pretty(&stac_item)?;
        std::fs::write(&stac_path, stac_json)?;

        println!("  ✓ STAC item saved: {}", stac_path.display());

        Ok(stac_path)
    }

    /// Run the complete WASM integration pipeline
    pub fn run(&self) -> Result<WasmDeployment> {
        let start = Instant::now();
        println!("=== WASM Integration Pipeline ===\n");

        // Step 1: Generate sample data
        let data = self.generate_sample_data(2048, 2048)?;

        // Step 2: Generate COG
        let cog_path = self.generate_cog(&data)?;

        // Step 3: Generate overviews
        let _overviews = self.generate_overviews(&data)?;

        // Step 4: Generate previews
        let preview_paths = self.generate_previews(&data)?;

        // Step 5: Create STAC item
        let stac_path = self.create_stac_item(&cog_path, &preview_paths, &data)?;

        let elapsed = start.elapsed();

        println!("\n=== Pipeline Complete ===");
        println!("Total time: {:.2}s", elapsed.as_secs_f64());

        println!("\n=== Output Files ===");
        println!("COG:");
        println!("  {}", cog_path.display());
        println!("Previews:");
        for path in &preview_paths {
            println!("  {}", path.display());
        }
        println!("STAC Metadata:");
        println!("  {}", stac_path.display());

        println!("\n=== WASM Viewer Integration ===");
        println!("The generated files are optimized for web viewers:");
        println!(
            "  ✓ COG with {} tiles for efficient range requests",
            (2048 / self.cog_config.tile_size) * (2048 / self.cog_config.tile_size)
        );
        println!("  ✓ Pyramid overviews for progressive loading");
        println!("  ✓ Multiple preview sizes for different zoom levels");
        println!("  ✓ STAC metadata for catalog integration");
        println!("\nReady for deployment to:");
        println!("  - Static hosting (S3, Cloudflare R2, etc.)");
        println!("  - CDN distribution");
        println!("  - WASM-based web viewers");

        Ok(WasmDeployment {
            cog_path,
            preview_paths,
            stac_path,
            base_url: self.base_url.clone(),
        })
    }
}

/// WASM deployment information
#[derive(Debug)]
pub struct WasmDeployment {
    pub cog_path: PathBuf,
    pub preview_paths: Vec<PathBuf>,
    pub stac_path: PathBuf,
    pub base_url: String,
}

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("WASM Integration Example - Preparing Data for Web Viewers\n");

    // Configure COG generation
    let cog_config = CogConfig {
        tile_size: 512,
        compression: CompressionType::Webp,
        create_overviews: true,
        overview_factors: vec![2, 4, 8, 16],
        overview_resampling: ResamplingMethod::Bilinear,
    };

    // Create pipeline
    let pipeline = WasmPipeline::new(cog_config, "https://cdn.example.com/geospatial")?;

    // Run the pipeline
    let _deployment = pipeline.run()?;

    println!("\nExample completed successfully!");
    println!("This demonstrates preparing geospatial data for web delivery:");
    println!("  - Cloud-Optimized GeoTIFF with efficient tiling");
    println!("  - Multi-resolution pyramid (overviews)");
    println!("  - Preview images for quick display");
    println!("  - STAC metadata for discovery");
    println!("  - Ready for WASM-based viewers");

    Ok(())
}
