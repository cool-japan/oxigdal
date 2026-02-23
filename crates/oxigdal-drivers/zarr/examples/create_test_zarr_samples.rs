//! Zarr Sample Generator for Multi-Dimensional Raster Data
//!
//! This example generates realistic geospatial Zarr v3 datasets for demonstration purposes,
//! featuring:
//! - 2D raster arrays for Golden Triangle and Basque Country regions
//! - Cloud-native chunking for efficient partial access
//! - Zstd compression for reduced storage footprint
//! - Pure Rust implementation following COOLJAPAN policies
//!
//! # Output
//! - `demo/cog-viewer/golden-triangle.zarr/` - Southeast Asian mining region elevation data
//! - `demo/cog-viewer/iron-belt.zarr/` - Northern Spain mining region elevation data
//!
//! # Data Characteristics
//! - Golden Triangle: Mountainous terrain (300-2800m elevation)
//! - Basque Country: Hilly terrain (200-1000m elevation)
//! - Both: 512x512 pixel resolution, 64x64 chunk size
//!
//! # Usage
//! ```bash
//! cargo run --example create_test_zarr_samples --features="v3,zstd,filesystem"
//! ```

use oxigdal_zarr::error::{Result, ZarrError};
use oxigdal_zarr::metadata::v3::{
    ArrayMetadataV3, BytesConfig, ChunkKeyEncoding, CodecMetadata, FillValue, ZstdConfig,
};
use oxigdal_zarr::storage::filesystem::FilesystemStore;
use oxigdal_zarr::writer::v3::ZarrV3Writer;
use std::path::Path;

/// Configuration for a geographic region
#[derive(Debug, Clone)]
struct RegionConfig {
    /// Human-readable region name
    name: String,
    /// Output path for Zarr array
    output_path: String,
    /// Array dimensions [height, width]
    shape: Vec<usize>,
    /// Chunk dimensions [chunk_height, chunk_width]
    chunk_shape: Vec<usize>,
    /// Base elevation (meters)
    base_elevation: f32,
    /// Elevation range (meters)
    elevation_range: f32,
    /// Terrain roughness factor (0.0 = smooth, 1.0 = very rough)
    roughness: f32,
    /// Random seed for reproducible terrain generation
    seed: u64,
}

impl RegionConfig {
    /// Creates configuration for Golden Triangle region
    fn golden_triangle() -> Self {
        Self {
            name: "Golden Triangle".to_string(),
            output_path: "demo/cog-viewer/golden-triangle.zarr".to_string(),
            shape: vec![512, 512],
            chunk_shape: vec![64, 64],
            base_elevation: 300.0,
            elevation_range: 2500.0,
            roughness: 0.8,
            seed: 12345,
        }
    }

    /// Creates configuration for Basque Country region
    fn iron_belt() -> Self {
        Self {
            name: "Basque Country".to_string(),
            output_path: "demo/cog-viewer/iron-belt.zarr".to_string(),
            shape: vec![512, 512],
            chunk_shape: vec![64, 64],
            base_elevation: 200.0,
            elevation_range: 800.0,
            roughness: 0.4,
            seed: 67890,
        }
    }
}

/// Simple pseudo-random number generator (LCG algorithm)
/// Pure Rust implementation to avoid external dependencies
struct PseudoRandom {
    state: u64,
}

impl PseudoRandom {
    /// Creates a new RNG with given seed
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Generates next random u64
    fn next_u64(&mut self) -> u64 {
        // Linear Congruential Generator parameters
        const A: u64 = 6_364_136_223_846_793_005;
        const C: u64 = 1_442_695_040_888_963_407;

        self.state = self.state.wrapping_mul(A).wrapping_add(C);
        self.state
    }

    /// Generates random f32 in range [0.0, 1.0)
    fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / 16_777_216.0 // 2^24 for precision
    }

    /// Generates random f32 in range [min, max)
    fn range(&mut self, min: f32, max: f32) -> f32 {
        min + self.next_f32() * (max - min)
    }
}

/// Perlin-like noise generator for realistic terrain
struct TerrainGenerator {
    rng: PseudoRandom,
    roughness: f32,
}

impl TerrainGenerator {
    /// Creates a new terrain generator
    fn new(seed: u64, roughness: f32) -> Self {
        Self {
            rng: PseudoRandom::new(seed),
            roughness,
        }
    }

    /// Generates smooth noise value using multi-octave sampling
    fn smooth_noise(&mut self, x: f32, y: f32, scale: f32) -> f32 {
        let sample_x = x * scale;
        let sample_y = y * scale;

        // Use sine/cosine combination for smooth, deterministic noise
        let value = ((sample_x * 0.1).sin() * (sample_y * 0.1).cos()
            + (sample_x * 0.2 + sample_y * 0.2).sin() * 0.5
            + (sample_x * 0.4 - sample_y * 0.3).cos() * 0.25)
            * 0.5
            + 0.5;

        // Add controlled randomness
        let random_factor = self.rng.range(-0.1, 0.1) * self.roughness;
        (value + random_factor).clamp(0.0, 1.0)
    }

    /// Generates elevation value for given coordinates using multi-octave Perlin-like noise
    fn generate_elevation(
        &mut self,
        row: usize,
        col: usize,
        width: usize,
        height: usize,
        base: f32,
        range: f32,
    ) -> f32 {
        let x = col as f32 / width as f32;
        let y = row as f32 / height as f32;

        // Multi-octave noise (similar to fractal Brownian motion)
        let mut value = 0.0;
        let mut amplitude = 1.0;
        let mut frequency = 1.0;
        let octaves = 4;

        for _ in 0..octaves {
            value += self.smooth_noise(x, y, frequency) * amplitude;
            amplitude *= 0.5;
            frequency *= 2.0;
        }

        // Normalize to [0, 1] range
        value /= 2.0 - 0.5_f32.powi(octaves);

        // Apply elevation transformation
        base + value * range
    }
}

/// Creates Zarr v3 metadata with compression
fn create_metadata(config: &RegionConfig) -> Result<ArrayMetadataV3> {
    // Create base metadata with correct chunk shape
    let mut metadata =
        ArrayMetadataV3::new(config.shape.clone(), config.chunk_shape.clone(), "float32");

    // Set chunk key encoding (default v3 format)
    metadata.chunk_key_encoding = ChunkKeyEncoding::default_with_separator("/");

    // Configure codec pipeline with zstd compression
    metadata.codecs = Some(vec![
        // Bytes codec (converts to bytes)
        CodecMetadata::Bytes {
            configuration: Some(BytesConfig {
                endian: Some("little".to_string()),
            }),
        },
        // Zstd compression codec
        CodecMetadata::Zstd {
            configuration: Some(ZstdConfig {
                level: Some(3),       // Balanced compression level
                checksum: Some(true), // Enable integrity checking
            }),
        },
    ]);

    // Set fill value for missing chunks
    metadata.fill_value = FillValue::Float(0.0);

    // Add dimension names for clarity
    metadata.dimension_names = Some(vec![
        Some("y".to_string()), // Latitude/row
        Some("x".to_string()), // Longitude/column
    ]);

    // Add custom attributes
    let mut attributes = serde_json::Map::new();
    attributes.insert(
        "description".to_string(),
        serde_json::Value::String(format!("{} region elevation data (synthetic)", config.name)),
    );
    attributes.insert(
        "units".to_string(),
        serde_json::Value::String("meters".to_string()),
    );
    attributes.insert(
        "crs".to_string(),
        serde_json::Value::String("EPSG:4326".to_string()),
    );
    attributes.insert(
        "generator".to_string(),
        serde_json::Value::String("OxiGDAL Zarr Sample Generator".to_string()),
    );
    attributes.insert(
        "version".to_string(),
        serde_json::Value::String(env!("CARGO_PKG_VERSION").to_string()),
    );
    metadata.attributes = Some(attributes);

    // Validate metadata before returning
    metadata.validate()?;

    Ok(metadata)
}

/// Generates raster data for a single chunk
fn generate_chunk_data(
    config: &RegionConfig,
    chunk_row: usize,
    chunk_col: usize,
    generator: &mut TerrainGenerator,
) -> Result<Vec<u8>> {
    let chunk_height = config.chunk_shape[0];
    let chunk_width = config.chunk_shape[1];
    let total_height = config.shape[0];
    let total_width = config.shape[1];

    // Calculate starting pixel coordinates for this chunk
    let start_row = chunk_row * chunk_height;
    let start_col = chunk_col * chunk_width;

    // Determine actual chunk dimensions (may be smaller at edges)
    let _actual_height = chunk_height.min(total_height.saturating_sub(start_row));
    let _actual_width = chunk_width.min(total_width.saturating_sub(start_col));

    // Generate elevation data for chunk
    let mut float_data = Vec::with_capacity(chunk_height * chunk_width);

    for row in 0..chunk_height {
        for col in 0..chunk_width {
            let global_row = start_row + row;
            let global_col = start_col + col;

            let value = if global_row < total_height && global_col < total_width {
                // Generate elevation for valid pixels
                generator.generate_elevation(
                    global_row,
                    global_col,
                    total_width,
                    total_height,
                    config.base_elevation,
                    config.elevation_range,
                )
            } else {
                // Fill value for padding
                0.0
            };

            float_data.push(value);
        }
    }

    // Convert f32 values to little-endian bytes
    let mut byte_data = Vec::with_capacity(float_data.len() * 4);
    for value in float_data {
        byte_data.extend_from_slice(&value.to_le_bytes());
    }

    Ok(byte_data)
}

/// Generates complete Zarr dataset for a region
fn generate_region_dataset(config: &RegionConfig) -> Result<()> {
    println!("Generating {} dataset...", config.name);
    println!("  Output: {}", config.output_path);
    println!("  Shape: {:?}", config.shape);
    println!("  Chunk shape: {:?}", config.chunk_shape);

    // Create output directory
    let output_path = Path::new(&config.output_path);
    if output_path.exists() {
        println!("  Removing existing dataset...");
        std::fs::remove_dir_all(output_path).map_err(|e| {
            ZarrError::Io(oxigdal_core::error::IoError::Write {
                message: format!("Failed to remove existing directory: {}", e),
            })
        })?;
    }

    // Create filesystem store
    let store = FilesystemStore::create(&config.output_path)?;

    // Create metadata
    let metadata = create_metadata(config)?;

    // Create writer
    let mut writer = ZarrV3Writer::new(store, "", metadata)?;

    // Calculate number of chunks in each dimension
    let num_chunks_y = config.shape[0].div_ceil(config.chunk_shape[0]);
    let num_chunks_x = config.shape[1].div_ceil(config.chunk_shape[1]);
    let total_chunks = num_chunks_y * num_chunks_x;

    println!(
        "  Total chunks: {} ({} x {})",
        total_chunks, num_chunks_y, num_chunks_x
    );

    // Initialize terrain generator
    let mut generator = TerrainGenerator::new(config.seed, config.roughness);

    // Generate and write chunks
    let mut chunks_written = 0;
    for chunk_row in 0..num_chunks_y {
        for chunk_col in 0..num_chunks_x {
            // Generate chunk data
            let chunk_data = generate_chunk_data(config, chunk_row, chunk_col, &mut generator)?;

            // Write chunk
            let coords = vec![chunk_row, chunk_col];
            writer.write_chunk(coords, chunk_data)?;

            chunks_written += 1;
            if chunks_written % 20 == 0 {
                print!("\r  Progress: {}/{} chunks", chunks_written, total_chunks);
                std::io::Write::flush(&mut std::io::stdout()).map_err(|e| {
                    ZarrError::Io(oxigdal_core::error::IoError::Write {
                        message: format!("Failed to flush stdout: {}", e),
                    })
                })?;
            }
        }
    }

    println!(
        "\r  Progress: {}/{} chunks - Complete!",
        chunks_written, total_chunks
    );

    // Finalize writer (writes metadata and flushes pending data)
    writer.finalize()?;

    println!("  Dataset generation complete!");
    println!();

    Ok(())
}

/// Main entry point
fn main() -> Result<()> {
    println!("=== Zarr Sample Generator for Multi-Dimensional Raster Data ===");
    println!();
    println!("This tool generates synthetic elevation data for demonstration purposes.");
    println!("Following COOLJAPAN policies: Pure Rust, no unwrap(), workspace-based.");
    println!();

    // Generate Golden Triangle dataset
    let golden_triangle_config = RegionConfig::golden_triangle();
    generate_region_dataset(&golden_triangle_config)?;

    // Generate Basque Country dataset
    let iron_belt_config = RegionConfig::iron_belt();
    generate_region_dataset(&iron_belt_config)?;

    println!("=== All datasets generated successfully! ===");
    println!();
    println!("Output locations:");
    println!("  - {}", golden_triangle_config.output_path);
    println!("  - {}", iron_belt_config.output_path);
    println!();
    println!("You can now use these Zarr datasets for testing and demonstration.");
    println!("Each dataset includes:");
    println!("  - Cloud-native chunking (64x64 tiles)");
    println!("  - Zstd compression with checksums");
    println!("  - Zarr v3 specification compliance");
    println!("  - Comprehensive metadata");

    Ok(())
}
