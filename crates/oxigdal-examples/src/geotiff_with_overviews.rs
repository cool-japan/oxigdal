//! Example: Creating GeoTIFF files with overview pyramids
//!
//! This example demonstrates:
//! - Overview/pyramid generation
//! - Different resampling algorithms
//! - Custom overview levels
//! - Performance considerations
//! - When to use overviews vs COG

use std::env;

use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::{GeoTransform, NoDataValue, RasterDataType};
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_geotiff::tiff::Compression;
use oxigdal_geotiff::writer::{
    GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("GeoTIFF with Overview Pyramids Example");
    println!("======================================\n");

    // Use temp directory for output
    let mut output_dir = env::temp_dir();
    output_dir.push("oxigdal_examples");
    std::fs::create_dir_all(&output_dir)?;

    // Example 1: Understanding overview levels
    {
        println!("1. Creating GeoTIFF with explanation of overview levels...");
        let mut path = output_dir.clone();
        path.push("overview_levels_demo.tif");

        let width = 1024u64;
        let height = 1024u64;

        // Create test data with gradients
        let mut data = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let value = ((x + y) / 8) as u8;
                data.push(value);
            }
        }

        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(true, OverviewResampling::Average)
            .with_overview_levels(vec![2, 4, 8]);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;
        writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Original: 1024x1024 (full resolution)");
        println!("   Overview level 2: 512x512 (1/2 resolution)");
        println!("   Overview level 4: 256x256 (1/4 resolution)");
        println!("   Overview level 8: 128x128 (1/8 resolution)");
        println!("\n   This allows efficient rendering at different zoom levels.");
        println!();
    }

    // Example 2: AVERAGE resampling (for continuous data)
    {
        println!("2. AVERAGE resampling for continuous data (temperature)...");
        let mut path = output_dir.clone();
        path.push("temperature_average.tif");

        let width = 512u64;
        let height = 512u64;

        // Create temperature-like data (Float32)
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                // Simulate temperature distribution
                let temp = 20.0 + 10.0 * ((x as f32 / 50.0).sin() * (y as f32 / 50.0).cos());
                data.extend_from_slice(&temp.to_le_bytes());
            }
        }

        let config = WriterConfig::new(width, height, 1, RasterDataType::Float32)
            .with_compression(Compression::Lzw)
            .with_tile_size(128, 128)
            .with_overviews(true, OverviewResampling::Average)
            .with_overview_levels(vec![2, 4, 8]);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;
        writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Data Type: Float32 (temperature in °C)");
        println!("   Resampling: AVERAGE");
        println!("   Best for: Continuous data where averaging makes sense");
        println!("   Examples: Temperature, precipitation, elevation");
        println!();
    }

    // Example 3: NEAREST resampling (for categorical data)
    {
        println!("3. NEAREST resampling for categorical data (land use)...");
        let mut path = output_dir.clone();
        path.push("landuse_nearest.tif");

        let width = 512u64;
        let height = 512u64;

        // Create land use classification data
        let mut data = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                // Define different land use categories
                let category = match ((x / 64) % 5, (y / 64) % 5) {
                    (0, _) => 1u8, // Urban
                    (1, _) => 2u8, // Forest
                    (2, _) => 3u8, // Agriculture
                    (3, _) => 4u8, // Water
                    _ => 5u8,      // Grassland
                };
                data.push(category);
            }
        }

        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(128, 128)
            .with_overviews(true, OverviewResampling::Nearest)
            .with_overview_levels(vec![2, 4, 8])
            .with_nodata(NoDataValue::Integer(0));

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;
        writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Data Type: UInt8 (classification codes)");
        println!("   Resampling: NEAREST");
        println!("   Best for: Categorical/discrete data");
        println!("   Examples: Land cover, zoning, soil types");
        println!("   Note: Preserves original class values, no interpolation");
        println!();
    }

    // Example 4: Custom overview levels for specific use cases
    {
        println!("4. Custom overview levels for web map tiling...");
        let mut path = output_dir.clone();
        path.push("webmap_overviews.tif");

        let width = 2048u64;
        let height = 1024u64; // 2:1 aspect ratio

        // Create test data
        let mut data = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let value = (((x / 4) + (y / 2)) % 256) as u8;
                data.push(value);
            }
        }

        // Custom levels matching web map zoom levels
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(true, OverviewResampling::Average)
            .with_overview_levels(vec![2, 4, 8, 16, 32, 64]);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;
        writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Original: 2048x1024");
        println!("   Overview levels: 2, 4, 8, 16, 32, 64");
        println!("   Purpose: Optimized for web map viewers");
        println!("   Each level corresponds to a zoom level reduction");
        println!();
    }

    // Example 5: Georeferenced data with overviews
    {
        println!("5. Georeferenced elevation data with overviews...");
        let mut path = output_dir.clone();
        path.push("dem_with_overviews.tif");

        let width = 600u64;
        let height = 400u64;

        // Create DEM-like data
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                // Simulate terrain
                let elevation = 1000.0
                    + 200.0 * ((x as f32 / 60.0).sin())
                    + 150.0 * ((y as f32 / 40.0).cos())
                    + 50.0 * ((x as f32 * y as f32 / 10000.0).sin());
                data.extend_from_slice(&elevation.to_le_bytes());
            }
        }

        // Mount Rainier area, Washington State
        let geo_transform = GeoTransform::north_up(-121.75, 46.85, 0.0001, -0.0001);

        let config = WriterConfig::new(width, height, 1, RasterDataType::Float32)
            .with_compression(Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(true, OverviewResampling::Average)
            .with_overview_levels(vec![2, 4])
            .with_geo_transform(geo_transform)
            .with_epsg_code(4326)
            .with_nodata(NoDataValue::Float(-9999.0));

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;
        writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Location: Mount Rainier area, WA");
        println!("   Data: Digital Elevation Model (meters)");
        println!("   Coordinate System: WGS84 (EPSG:4326)");
        println!("   NoData: -9999.0");
        println!("   Overviews: 2, 4");
        println!();
    }

    // Example 6: Compare file sizes with and without overviews
    {
        println!("6. Comparing file sizes with and without overviews...");

        // Without overviews
        let mut path_no_ov = output_dir.clone();
        path_no_ov.push("no_overviews.tif");

        let width = 1024u64;
        let height = 1024u64;
        let data = vec![128u8; (width * height) as usize];

        {
            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Lzw)
                .with_tile_size(256, 256)
                .with_overviews(false, OverviewResampling::Average);

            let mut writer =
                GeoTiffWriter::create(&path_no_ov, config, GeoTiffWriterOptions::default())?;
            writer.write(&data)?;
        }

        // With overviews
        let mut path_with_ov = output_dir.clone();
        path_with_ov.push("with_overviews.tif");

        {
            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Lzw)
                .with_tile_size(256, 256)
                .with_overviews(true, OverviewResampling::Average)
                .with_overview_levels(vec![2, 4, 8]);

            let mut writer =
                GeoTiffWriter::create(&path_with_ov, config, GeoTiffWriterOptions::default())?;
            writer.write(&data)?;
        }

        let size_no_ov = std::fs::metadata(&path_no_ov)?.len();
        let size_with_ov = std::fs::metadata(&path_with_ov)?.len();
        let overhead_pct = ((size_with_ov - size_no_ov) as f64 / size_no_ov as f64) * 100.0;

        println!("   Without overviews: {} bytes", size_no_ov);
        println!("   With overviews (2,4,8): {} bytes", size_with_ov);
        println!("   Overhead: {:.1}%", overhead_pct);
        println!("\n   Note: Overviews increase file size but improve rendering performance");
        println!();
    }

    // Example 7: Inspect overview structure
    {
        println!("7. Inspecting overview structure...");
        let mut path = output_dir.clone();
        path.push("with_overviews.tif");

        let source = FileDataSource::open(&path)?;
        let reader = GeoTiffReader::new(source)?;

        println!("   File: {}", path.display());
        println!("   Main Image: {}x{}", reader.width(), reader.height());
        println!("   Overview Count: {}", reader.overview_count());

        // Calculate expected overview sizes
        let overview_levels = vec![2, 4, 8];
        println!("\n   Expected Overview Dimensions:");
        for level in overview_levels {
            let ov_width = reader.width() / level;
            let ov_height = reader.height() / level;
            println!("     Level {}: {}x{}", level, ov_width, ov_height);
        }
        println!();
    }

    println!("\n=== Overview Best Practices ===\n");
    println!("When to Use Overviews:");
    println!("  - Large rasters (>1000x1000 pixels)");
    println!("  - Interactive visualization applications");
    println!("  - Web mapping services");
    println!("  - Desktop GIS applications");
    println!();
    println!("Resampling Method Selection:");
    println!("  - AVERAGE: Continuous data (DEM, temperature, NDVI)");
    println!("  - NEAREST: Categorical data (land cover, classification)");
    println!();
    println!("Overview Levels:");
    println!("  - Start with powers of 2: [2, 4, 8, 16]");
    println!("  - Add more levels for very large images (32, 64, 128)");
    println!("  - Consider zoom levels needed for your application");
    println!();
    println!("Performance Considerations:");
    println!("  - Overviews increase file size by ~33% (for levels 2,4,8,16)");
    println!("  - Dramatically improve rendering speed for zoomed-out views");
    println!("  - Essential for good user experience in visualization apps");
    println!();
    println!("COG vs Regular GeoTIFF with Overviews:");
    println!("  - COG: Optimized for cloud/HTTP range-request access");
    println!("  - Regular + Overviews: Good for local file access");
    println!("  - Use COG when serving over network, especially cloud storage");
    println!();

    println!("All overview examples completed successfully!");
    println!("Output directory: {}", output_dir.display());

    Ok(())
}
