//! Example: Creating Cloud Optimized GeoTIFF (COG) files
//!
//! This example demonstrates:
//! - COG creation with proper tiling
//! - Overview/pyramid generation
//! - Different resampling methods
//! - COG validation
//! - Best practices for COG creation

use std::env;

use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::{GeoTransform, RasterDataType};
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_geotiff::tiff::Compression;
use oxigdal_geotiff::writer::{CogWriter, CogWriterOptions, OverviewResampling, WriterConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Cloud Optimized GeoTIFF (COG) Creation Example");
    println!("==============================================\n");

    // Use temp directory for output
    let mut output_dir = env::temp_dir();
    output_dir.push("oxigdal_examples");
    std::fs::create_dir_all(&output_dir)?;

    // Example 1: Basic COG with default settings
    {
        println!("1. Creating basic COG with default overview levels...");
        let mut path = output_dir.clone();
        path.push("basic_cog.tif");

        let width = 1024u64;
        let height = 1024u64;

        // Create test data with a pattern
        let mut data = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                // Create a checkerboard-like pattern
                let value = if ((x / 64) + (y / 64)) % 2 == 0 {
                    200u8
                } else {
                    50u8
                };
                data.push(value);
            }
        }

        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(256, 256) // COG standard tile size
            .with_overviews(true, OverviewResampling::Average)
            .with_overview_levels(vec![2, 4, 8, 16]); // Default overview levels

        let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())?;
        let validation = writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Size: {}x{} pixels, 1 band, UInt8", width, height);
        println!("   Tile Size: 256x256 (COG standard)");
        println!("   Compression: LZW");
        println!("   Overview Levels: 2, 4, 8, 16");
        println!("   Is Valid COG: {}", validation.is_valid);
        println!("   Has Overviews: {}", validation.has_overviews);
        println!("   Tiles Ordered: {}", validation.tiles_ordered);
        println!();
    }

    // Example 2: COG with NEAREST resampling (for categorical data)
    {
        println!("2. Creating COG with NEAREST resampling (for classification data)...");
        let mut path = output_dir.clone();
        path.push("classification_cog.tif");

        let width = 512u64;
        let height = 512u64;

        // Create categorical test data (land cover classification)
        let mut data = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                // Simulate different land cover classes
                let class = match (x / 128, y / 128) {
                    (0, 0) => 1u8,          // Water
                    (0, 1) | (1, 0) => 2u8, // Forest
                    (1, 1) => 3u8,          // Urban
                    (2, _) | (_, 2) => 4u8, // Agriculture
                    _ => 0u8,               // No data
                };
                data.push(class);
            }
        }

        // Use NEAREST resampling for categorical data (preserves class values)
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(true, OverviewResampling::Nearest)
            .with_overview_levels(vec![2, 4]);

        let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())?;
        let validation = writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Size: {}x{} pixels, 1 band, UInt8", width, height);
        println!("   Use Case: Land cover classification");
        println!("   Resampling: NEAREST (preserves discrete values)");
        println!("   Overview Levels: 2, 4");
        println!("   Is Valid COG: {}", validation.is_valid);
        println!();
    }

    // Example 3: Georeferenced COG
    {
        println!("3. Creating georeferenced COG (elevation data)...");
        let mut path = output_dir.clone();
        path.push("elevation_cog.tif");

        let width = 800u64;
        let height = 600u64;

        // Create elevation-like test data (Float32)
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                // Simulate terrain elevation
                let elevation =
                    100.0 + 50.0 * ((x as f32 / 100.0).sin()) + 30.0 * ((y as f32 / 80.0).cos());
                data.extend_from_slice(&elevation.to_le_bytes());
            }
        }

        // Set up georeferencing for a region in Colorado
        // Origin: -105°W, 40°N
        // Pixel size: 0.0001° (approximately 10m)
        let geo_transform = GeoTransform::north_up(-105.0, 40.0, 0.0001, -0.0001);

        let config = WriterConfig::new(width, height, 1, RasterDataType::Float32)
            .with_compression(Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(true, OverviewResampling::Average)
            .with_overview_levels(vec![2, 4, 8])
            .with_geo_transform(geo_transform)
            .with_epsg_code(4326); // WGS84

        let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())?;
        let validation = writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Size: {}x{} pixels, 1 band, Float32", width, height);
        println!("   Use Case: Digital Elevation Model (DEM)");
        println!("   Coordinate System: EPSG:4326 (WGS84)");
        println!("   Origin: -105°W, 40°N");
        println!("   Pixel Size: 0.0001° x 0.0001° (~10m)");
        println!("   Resampling: AVERAGE (good for continuous data)");
        println!("   Overview Levels: 2, 4, 8");
        println!("   Is Valid COG: {}", validation.is_valid);
        println!();
    }

    // Example 4: RGB COG (aerial imagery style)
    {
        println!("4. Creating RGB COG (simulating aerial imagery)...");
        let mut path = output_dir.clone();
        path.push("aerial_rgb_cog.tif");

        let width = 512u64;
        let height = 512u64;

        // Create RGB test data
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                // Create a colorful pattern
                let r = ((x * 255) / width) as u8;
                let g = ((y * 255) / height) as u8;
                let b = (((x + y) * 128) / (width + height)) as u8;
                data.push(r);
                data.push(g);
                data.push(b);
            }
        }

        let config = WriterConfig::new(width, height, 3, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(true, OverviewResampling::Average)
            .with_overview_levels(vec![2, 4]);

        let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())?;
        let validation = writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Size: {}x{} pixels, 3 bands (RGB), UInt8", width, height);
        println!("   Use Case: Aerial/Satellite imagery");
        println!("   Compression: LZW");
        println!("   Overview Levels: 2, 4");
        println!("   Is Valid COG: {}", validation.is_valid);
        println!();
    }

    // Example 5: High-resolution COG with many overview levels
    {
        println!("5. Creating high-resolution COG with many overview levels...");
        let mut path = output_dir.clone();
        path.push("highres_cog.tif");

        let width = 2048u64;
        let height = 2048u64;

        // Create test data
        let mut data = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                // Create a fractal-like pattern
                let value = (((x % 256) + (y % 256)) / 2) as u8;
                data.push(value);
            }
        }

        // Many overview levels for optimal web viewing at different zoom levels
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(true, OverviewResampling::Average)
            .with_overview_levels(vec![2, 4, 8, 16, 32]);

        let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())?;
        let validation = writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Size: {}x{} pixels, 1 band, UInt8", width, height);
        println!("   Use Case: High-resolution web mapping");
        println!("   Overview Levels: 2, 4, 8, 16, 32");
        println!("   This allows efficient viewing from 1:1 down to 1:32 zoom");
        println!("   Is Valid COG: {}", validation.is_valid);
        println!();
    }

    // Example 6: Validate and inspect a COG
    {
        println!("6. Validating and inspecting COG files...");
        let mut path = output_dir.clone();
        path.push("basic_cog.tif");

        let source = FileDataSource::open(&path)?;

        // Check if it's a valid COG
        let is_cog = oxigdal_geotiff::is_cog(&source)?;
        println!("   File: {}", path.display());
        println!("   Is Valid COG: {}", is_cog);

        // Read and inspect metadata
        let reader = GeoTiffReader::new(source)?;
        println!("   Image Size: {}x{}", reader.width(), reader.height());
        println!("   Bands: {}", reader.band_count());
        println!("   Tile Size: {:?}", reader.tile_size());
        println!("   Overview Count: {}", reader.overview_count());
        println!("   Compression: {:?}", reader.compression());

        if reader.tile_size().is_some() {
            let (tiles_x, tiles_y) = reader.tile_count();
            println!("   Tile Grid: {}x{} tiles", tiles_x, tiles_y);
        }

        if let Some(gt) = reader.geo_transform() {
            println!("   GeoTransform:");
            println!("     Origin: ({}, {})", gt.origin_x, gt.origin_y);
            println!("     Pixel Size: ({}, {})", gt.pixel_width, gt.pixel_height);
        }

        if let Some(epsg) = reader.epsg_code() {
            println!("   EPSG Code: {}", epsg);
        }
        println!();
    }

    println!("\n=== COG Best Practices Summary ===\n");
    println!("1. Tile Size: Use 256x256 or 512x512 (power of 2)");
    println!("2. Compression: LZW or DEFLATE for lossless, JPEG for lossy (imagery)");
    println!("3. Overviews: Include multiple levels (e.g., 2, 4, 8, 16)");
    println!("4. Resampling:");
    println!("   - AVERAGE: Continuous data (elevation, temperature)");
    println!("   - NEAREST: Categorical data (land cover, classification)");
    println!("5. For web serving: Always include georeferencing");
    println!("6. For large files (>4GB): Use BigTIFF format");
    println!();

    println!("All COG examples completed successfully!");
    println!("Output directory: {}", output_dir.display());
    println!("\nValidate COGs with:");
    println!("  - rio cogeo validate <filename>");
    println!("  - gdal_translate -of COG <filename> /vsistdout/ > /dev/null");

    Ok(())
}
