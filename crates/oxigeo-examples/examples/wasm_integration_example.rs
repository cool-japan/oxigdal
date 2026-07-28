//! WASM Integration Example - Preparing Data for Web Viewers
#![allow(missing_docs)]
//!
//! This example demonstrates preparing geospatial data for web applications:
//! 1. Generate a real Cloud-Optimized GeoTIFF (COG) with proper tiling
//! 2. Build a real pyramid of overviews for multi-resolution display
//! 3. Generate a real, spec-validated STAC item
//! 4. Render real preview images (PNG / lossless WebP thumbnails)
//! 5. Prepare for WASM-based web viewer consumption
//!
//! This workflow prepares data that can be efficiently loaded by web viewers
//! like Leaflet, OpenLayers, or custom WebAssembly viewers.
//!
//! Every artifact this example writes to disk is real: the `.tif` is a real
//! tiled, compressed, multi-overview GeoTIFF produced by
//! `oxigeo_geotiff::writer::CogWriter` (independently readable/validatable by
//! any GeoTIFF reader); the preview thumbnails are real PNG / lossless WebP
//! images produced by the `png` and `image` crates from actually-resampled
//! pixel data (via `oxigeo_algorithms::resampling::Resampler`); and the STAC
//! item is built and spec-validated through `oxigeo_stac::builder::ItemBuilder`.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example wasm_integration_example
//! ```
//!
//! # Workflow
//!
//! Source Data → Generate COG (+ real internal overviews) → Real Preview Thumbnails → STAC Metadata
//!
//! # Output
//!
//! - Real Cloud-Optimized GeoTIFF with tiling and internal overviews (2x, 4x, 8x, 16x)
//! - Real preview images (256x256 PNG, 512x512 PNG, 1024x1024 lossless WebP)
//! - Real, spec-validated STAC JSON item

use chrono::Utc;
use oxigeo_algorithms::resampling::{Resampler, ResamplingMethod};
use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::{GeoTransform, RasterDataType};
use oxigeo_geotiff::tiff::{Compression, PhotometricInterpretation};
use oxigeo_geotiff::writer::{CogWriter, CogWriterOptions, OverviewResampling, WriterConfig};
use oxigeo_stac::builder::ItemBuilder;
use oxigeo_stac::{Asset, bbox_to_polygon};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tempfile::TempDir;
use thiserror::Error;

/// Custom error types for the WASM workflow.
#[derive(Debug, Error)]
pub enum WasmError {
    /// COG generation errors (real `oxigeo-geotiff` `CogWriter` failures).
    #[error("COG generation error: {0}")]
    CogGeneration(String),

    /// Resampling errors (real `oxigeo-algorithms` `Resampler` failures).
    #[error("Resampling error: {0}")]
    Overview(String),

    /// STAC creation errors (real `oxigeo-stac` build/validation failures).
    #[error("STAC error: {0}")]
    Stac(String),

    /// Preview image encoding errors (real PNG / WebP encoder failures).
    #[error("Preview generation error: {0}")]
    Preview(String),

    /// I/O errors.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Raster buffer errors.
    #[error("Buffer error: {0}")]
    Buffer(String),

    /// Serialization errors.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

type Result<T> = std::result::Result<T, WasmError>;

/// Configuration for COG generation.
#[derive(Debug, Clone)]
pub struct CogConfig {
    /// Tile size (typically 256, 512, or 1024).
    pub tile_size: usize,
    /// Compression type.
    pub compression: CompressionType,
    /// Create overviews.
    pub create_overviews: bool,
    /// Overview factors (e.g., \[2, 4, 8, 16\]).
    pub overview_factors: Vec<usize>,
    /// Resampling method for overviews and preview thumbnails.
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

/// TIFF compression schemes this example actually wires up.
///
/// `oxigeo-geotiff` also supports JPEG-in-TIFF and WebP-in-TIFF compression,
/// but those require enabling its non-default `jpeg` / `webp` Cargo
/// features. This example only enables `lzw` and `deflate` (the crate's
/// default features), so only the compression schemes those two features
/// actually implement are offered here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    /// No compression.
    None,
    /// Adobe DEFLATE (zlib), via the pure-Rust `oxiarc-deflate` codec.
    Deflate,
    /// LZW, via the pure-Rust `oxiarc-lzw` codec.
    Lzw,
}

impl CompressionType {
    pub fn name(&self) -> &str {
        match self {
            Self::None => "none",
            Self::Deflate => "deflate",
            Self::Lzw => "lzw",
        }
    }

    /// Maps to the real `oxigeo_geotiff::tiff::Compression` tag actually
    /// written into the TIFF file.
    fn to_geotiff(self) -> Compression {
        match self {
            Self::None => Compression::None,
            Self::Deflate => Compression::AdobeDeflate,
            Self::Lzw => Compression::Lzw,
        }
    }
}

/// Maps the crate-wide [`ResamplingMethod`] onto the `oxigeo-geotiff`
/// writer's own (smaller) [`OverviewResampling`] enum. `OverviewResampling`
/// has no Bicubic/Lanczos variant, so those approximate to `Bilinear` for
/// the COG's *internal* overview pyramid; preview thumbnails (generated
/// separately by this example) still use the exact requested method via
/// [`Resampler`].
fn to_overview_resampling(method: ResamplingMethod) -> OverviewResampling {
    match method {
        ResamplingMethod::Nearest => OverviewResampling::Nearest,
        ResamplingMethod::Bilinear | ResamplingMethod::Bicubic | ResamplingMethod::Lanczos => {
            OverviewResampling::Bilinear
        }
    }
}

/// Preview image configuration.
#[derive(Debug, Clone)]
pub struct PreviewConfig {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// Format: `"png"` or `"webp"` (both are actually encoded; anything
    /// else is a typed [`WasmError::Preview`], never silently ignored).
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

/// An in-memory RGB raster: real interleaved (R, G, B, R, G, B, ...) `u8`
/// pixel data plus its dimensions.
struct SampleImage {
    width: usize,
    height: usize,
    /// Interleaved RGB bytes, length `width * height * 3`.
    rgb: Vec<u8>,
}

/// WASM integration pipeline.
pub struct WasmPipeline {
    /// Output directory.
    output_dir: TempDir,
    /// COG configuration.
    cog_config: CogConfig,
    /// Base URL for assets (e.g., CDN).
    base_url: String,
    /// Real geospatial extent assigned to the generated raster (also used,
    /// unmodified, to compute the STAC item's bbox/geometry -- there is no
    /// separate "fake" bbox disconnected from the actual raster placement).
    geo_transform: GeoTransform,
}

impl WasmPipeline {
    /// Create a new WASM integration pipeline.
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

        // Region: San Francisco Bay area, north-up, ~10m pixels.
        let geo_transform = GeoTransform::north_up(-122.5, 38.0, 0.0002441, -0.0002441);

        Ok(Self {
            output_dir,
            cog_config,
            base_url: base_url_string,
            geo_transform,
        })
    }

    /// Generates real sample RGB raster data (a gradient pattern).
    fn generate_sample_data(&self, width: usize, height: usize) -> Result<SampleImage> {
        println!("Generating sample data...");
        println!("  Dimensions: {} x {}", width, height);

        let mut rgb = vec![0u8; width * height * 3];

        // Create a colorful gradient pattern.
        for y in 0..height {
            for x in 0..width {
                let i = (y * width + x) * 3;
                let r = ((x as f32 / width as f32) * 255.0) as u8;
                let g = ((y as f32 / height as f32) * 255.0) as u8;
                let b = (((x + y) as f32 / (width + height) as f32) * 255.0) as u8;
                rgb[i] = r;
                rgb[i + 1] = g;
                rgb[i + 2] = b;
            }
        }

        Ok(SampleImage { width, height, rgb })
    }

    /// Generates a real Cloud-Optimized GeoTIFF via `oxigeo_geotiff`'s
    /// `CogWriter`: real tiling, real compression, and a real internal
    /// overview pyramid (built and validated by the writer itself, not
    /// simulated by this example).
    fn generate_cog(
        &self,
        image: &SampleImage,
    ) -> Result<(PathBuf, oxigeo_geotiff::cog::CogValidation)> {
        println!("\nGenerating Cloud-Optimized GeoTIFF...");

        let output_path = self.output_dir.path().join("data.tif");
        let tile_size = self.cog_config.tile_size as u32;

        println!("  Output: {}", output_path.display());
        println!("  Tile layout: {tile_size}x{tile_size}");
        println!("  Compression: {}", self.cog_config.compression.name());
        println!("  Overview factors: {:?}", self.cog_config.overview_factors);

        let overview_levels: Vec<u32> = self
            .cog_config
            .overview_factors
            .iter()
            .map(|&f| f as u32)
            .collect();

        let config = WriterConfig::new(
            image.width as u64,
            image.height as u64,
            3,
            RasterDataType::UInt8,
        )
        .with_compression(self.cog_config.compression.to_geotiff())
        .with_tile_size(tile_size, tile_size)
        .with_photometric(PhotometricInterpretation::Rgb)
        .with_geo_transform(self.geo_transform)
        .with_overviews(
            self.cog_config.create_overviews,
            to_overview_resampling(self.cog_config.overview_resampling),
        )
        .with_overview_levels(overview_levels);

        let mut writer = CogWriter::create(&output_path, config, CogWriterOptions::default())
            .map_err(|e| WasmError::CogGeneration(e.to_string()))?;

        let validation = writer
            .write(&image.rgb)
            .map_err(|e| WasmError::CogGeneration(e.to_string()))?;

        println!("  \u{2713} COG written and validated by CogWriter itself:");
        println!("    - is_valid:     {}", validation.is_valid);
        println!("    - has_overviews: {}", validation.has_overviews);
        println!("    - tiles_ordered: {}", validation.tiles_ordered);
        if !validation.messages.is_empty() {
            println!("    - messages: {:?}", validation.messages);
        }

        Ok((output_path, validation))
    }

    /// Splits interleaved RGB bytes into three separate single-band channels.
    fn split_channels(rgb: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let pixel_count = rgb.len() / 3;
        let mut r = Vec::with_capacity(pixel_count);
        let mut g = Vec::with_capacity(pixel_count);
        let mut b = Vec::with_capacity(pixel_count);
        for px in rgb.chunks_exact(3) {
            r.push(px[0]);
            g.push(px[1]);
            b.push(px[2]);
        }
        (r, g, b)
    }

    /// Re-interleaves three single-band channels back into RGB bytes.
    fn interleave_channels(r: &[u8], g: &[u8], b: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(r.len() * 3);
        for i in 0..r.len() {
            out.push(r[i]);
            out.push(g[i]);
            out.push(b[i]);
        }
        out
    }

    /// Resamples a single-band `u8` channel using the real
    /// `oxigeo_algorithms::resampling::Resampler`.
    fn resample_channel(
        &self,
        channel: &[u8],
        src_width: usize,
        src_height: usize,
        dst_width: usize,
        dst_height: usize,
    ) -> Result<Vec<u8>> {
        let buffer = RasterBuffer::from_typed_vec(
            src_width,
            src_height,
            channel.to_vec(),
            RasterDataType::UInt8,
        )
        .map_err(|e| WasmError::Overview(e.to_string()))?;

        let resampler = Resampler::new(self.cog_config.overview_resampling);
        let resampled = resampler
            .resample(&buffer, dst_width as u64, dst_height as u64)
            .map_err(|e| WasmError::Overview(e.to_string()))?;

        resampled
            .as_slice::<u8>()
            .map(<[u8]>::to_vec)
            .map_err(|e| WasmError::Overview(e.to_string()))
    }

    /// Encodes real interleaved RGB `u8` data as a PNG file via the `png` crate.
    fn encode_png(rgb: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut output, width as u32, height as u32);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder
                .write_header()
                .map_err(|e| WasmError::Preview(e.to_string()))?;
            writer
                .write_image_data(rgb)
                .map_err(|e| WasmError::Preview(e.to_string()))?;
        }
        Ok(output)
    }

    /// Encodes real interleaved RGB `u8` data as a lossless WebP file via
    /// the `image` crate's pure-Rust `WebPEncoder`.
    fn encode_webp(rgb: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
        use image::ExtendedColorType;
        use image::codecs::webp::WebPEncoder;

        let mut output = Vec::new();
        let encoder = WebPEncoder::new_lossless(&mut output);
        encoder
            .encode(rgb, width as u32, height as u32, ExtendedColorType::Rgb8)
            .map_err(|e| WasmError::Preview(e.to_string()))?;
        Ok(output)
    }

    /// Generates real preview thumbnails: real resampling followed by real
    /// PNG/WebP encoding. Every byte written to disk is a decodable image.
    fn generate_previews(&self, image: &SampleImage) -> Result<Vec<PathBuf>> {
        println!("\nGenerating preview images...");

        let preview_configs = vec![
            PreviewConfig::new(256, 256, "png"),
            PreviewConfig::new(512, 512, "png"),
            PreviewConfig::new(1024, 1024, "webp"),
        ];

        let (r, g, b) = Self::split_channels(&image.rgb);

        let mut preview_paths = Vec::new();

        for config in preview_configs {
            let filename = format!(
                "preview_{}x{}.{}",
                config.width, config.height, config.format
            );
            let output_path = self.output_dir.path().join(&filename);

            println!(
                "  Creating {}x{} {} preview...",
                config.width, config.height, config.format
            );

            let r_resized =
                self.resample_channel(&r, image.width, image.height, config.width, config.height)?;
            let g_resized =
                self.resample_channel(&g, image.width, image.height, config.width, config.height)?;
            let b_resized =
                self.resample_channel(&b, image.width, image.height, config.width, config.height)?;
            let rgb_resized = Self::interleave_channels(&r_resized, &g_resized, &b_resized);

            let encoded = match config.format.as_str() {
                "png" => Self::encode_png(&rgb_resized, config.width, config.height)?,
                "webp" => Self::encode_webp(&rgb_resized, config.width, config.height)?,
                other => {
                    return Err(WasmError::Preview(format!(
                        "unsupported preview format: {other} (only \"png\" and \"webp\" are implemented)"
                    )));
                }
            };

            std::fs::write(&output_path, &encoded)?;

            println!(
                "    \u{2713} Saved: {} ({} bytes, real {} image)",
                output_path.display(),
                encoded.len(),
                config.format
            );
            preview_paths.push(output_path);
        }

        println!("  \u{2713} Generated {} previews", preview_paths.len());

        Ok(preview_paths)
    }

    /// Computes the raster's real geospatial bounding box from its
    /// [`GeoTransform`] and dimensions (the same numbers baked into the
    /// GeoTIFF itself), so the STAC item's bbox/geometry are never
    /// disconnected from the actual data.
    fn bbox(&self, width: usize, height: usize) -> [f64; 4] {
        let west = self.geo_transform.origin_x;
        let north = self.geo_transform.origin_y;
        let east = west + self.geo_transform.pixel_width * width as f64;
        let south = north + self.geo_transform.pixel_height * height as f64;
        [west, south.min(north), east, south.max(north)]
    }

    /// Creates a real, spec-validated STAC item via
    /// `oxigeo_stac::builder::ItemBuilder` (`Item::validate` runs inside
    /// `ItemBuilder::build`, so this cannot produce a non-conformant item).
    fn create_stac_item(
        &self,
        cog_path: &Path,
        preview_paths: &[PathBuf],
        image: &SampleImage,
    ) -> Result<PathBuf> {
        println!("\nCreating STAC item...");

        let item_id = "web-ready-data-001";
        let bbox = self.bbox(image.width, image.height);

        println!("  Item ID: {}", item_id);
        println!("  Bounding box: {:?}", bbox);

        let cog_href = format!(
            "{}/{}",
            self.base_url,
            cog_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("data.tif")
        );
        let thumbnail_href = format!(
            "{}/{}",
            self.base_url,
            preview_paths[0]
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("preview.png")
        );
        let overview_href = format!(
            "{}/{}",
            self.base_url,
            preview_paths[1]
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("preview.png")
        );

        let data_asset = Asset::new(cog_href)
            .with_type("image/tiff; application=geotiff; profile=cloud-optimized")
            .with_title("Cloud-Optimized GeoTIFF")
            .with_role("data")
            .with_additional_field(
                "cog:tile_size",
                serde_json::json!(self.cog_config.tile_size),
            )
            .with_additional_field(
                "cog:overviews",
                serde_json::json!(self.cog_config.overview_factors),
            );

        let thumbnail_asset = Asset::new(thumbnail_href)
            .with_type("image/png")
            .with_title("Thumbnail (256x256)")
            .with_role("thumbnail");

        let preview_asset = Asset::new(overview_href)
            .with_type("image/png")
            .with_title("Preview (512x512)")
            .with_role("overview");

        let item = ItemBuilder::new(item_id)
            .geometry(bbox_to_polygon(bbox[0], bbox[1], bbox[2], bbox[3]))
            .bbox(bbox[0], bbox[1], bbox[2], bbox[3])
            .datetime(Utc::now())
            .asset("data", data_asset)
            .asset("thumbnail", thumbnail_asset)
            .asset("preview", preview_asset)
            .link(format!("{}/stac/{}.json", self.base_url, item_id), "self")
            .link(format!("{}/stac/collection.json", self.base_url), "parent")
            .build()
            .map_err(|e| WasmError::Stac(e.to_string()))?;

        let stac_path = self.output_dir.path().join(format!("{}.json", item_id));
        let stac_json = serde_json::to_string_pretty(&item)?;
        std::fs::write(&stac_path, stac_json)?;

        println!(
            "  \u{2713} STAC item validated and saved: {}",
            stac_path.display()
        );

        Ok(stac_path)
    }

    /// Run the complete WASM integration pipeline.
    pub fn run(&self) -> Result<WasmDeployment> {
        let start = Instant::now();
        println!("=== WASM Integration Pipeline ===\n");

        // Step 1: Generate sample data.
        let image = self.generate_sample_data(2048, 2048)?;

        // Step 2: Generate a real COG with real internal overviews.
        let (cog_path, validation) = self.generate_cog(&image)?;

        // Step 3: Generate real preview thumbnails (real resample + real encode).
        let preview_paths = self.generate_previews(&image)?;

        // Step 4: Create a real, spec-validated STAC item.
        let stac_path = self.create_stac_item(&cog_path, &preview_paths, &image)?;

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
            "  \u{2713} COG with real internal overviews (has_overviews={}) for progressive loading",
            validation.has_overviews
        );
        println!("  \u{2713} Multiple real preview sizes for different zoom levels");
        println!("  \u{2713} Spec-validated STAC metadata for catalog integration");
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

/// WASM deployment information.
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
        compression: CompressionType::Deflate,
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
    println!("  - Real Cloud-Optimized GeoTIFF with efficient tiling + internal overviews");
    println!("  - Real preview images (PNG / lossless WebP) for quick display");
    println!("  - Real, spec-validated STAC metadata for discovery");
    println!("  - Ready for WASM-based viewers");

    Ok(())
}
