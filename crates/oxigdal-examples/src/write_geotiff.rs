//! Example: Writing GeoTIFF files with various configurations
//!
//! This example demonstrates:
//! - Basic GeoTIFF writing
//! - Different compression schemes
//! - Tiled vs striped layouts
//! - Adding georeferencing information
//! - Setting NoData values

use std::env;

use oxigdal_core::types::{GeoTransform, NoDataValue, RasterDataType};
use oxigdal_geotiff::tiff::Compression;
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("OxiGDAL GeoTIFF Writer Example");
    println!("===============================\n");

    // Use temp directory for output
    let mut output_dir = env::temp_dir();
    output_dir.push("oxigdal_examples");
    std::fs::create_dir_all(&output_dir)?;

    // Example 1: Basic single-band GeoTIFF with LZW compression
    {
        println!("1. Creating basic single-band GeoTIFF with LZW compression...");
        let mut path = output_dir.clone();
        path.push("basic_grayscale.tif");

        let width = 256u64;
        let height = 256u64;

        // Create test data (simple gradient)
        let mut data = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let value = ((x + y) / 2) as u8;
                data.push(value);
            }
        }

        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(256, 256);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;
        writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Size: {}x{} pixels, 1 band, UInt8", width, height);
        println!("   Compression: LZW");
        println!("   Layout: Tiled (256x256)\n");
    }

    // Example 2: RGB image with DEFLATE compression
    {
        println!("2. Creating RGB image with DEFLATE compression...");
        let mut path = output_dir.clone();
        path.push("rgb_image.tif");

        let width = 200u64;
        let height = 200u64;

        // Create RGB test data (color gradient)
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                data.push(((x * 255) / width) as u8); // R
                data.push(((y * 255) / height) as u8); // G
                data.push((((x + y) * 128) / (width + height)) as u8); // B
            }
        }

        // Use LZW compression for RGB (widely supported)
        let config = WriterConfig::new(width, height, 3, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(100, 100);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;
        writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Size: {}x{} pixels, 3 bands (RGB), UInt8", width, height);
        println!("   Compression: LZW");
        println!("   Layout: Tiled (100x100)\n");
    }

    // Example 3: Georeferenced GeoTIFF
    {
        println!("3. Creating georeferenced GeoTIFF (WGS84)...");
        let mut path = output_dir.clone();
        path.push("georeferenced.tif");

        let width = 300u64;
        let height = 200u64;

        // Create elevation-like test data
        let mut data = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let value = ((x * x + y * y) / 100) as u16;
                data.extend_from_slice(&value.to_le_bytes());
            }
        }

        // Set up georeferencing for a small area in California
        // Origin: -122°W, 37°N
        // Pixel size: 0.001° (approximately 100m)
        let geo_transform = GeoTransform::north_up(-122.0, 37.0, 0.001, -0.001);

        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt16)
            .with_compression(Compression::Lzw)
            .with_tile_size(256, 256)
            .with_geo_transform(geo_transform)
            .with_epsg_code(4326); // WGS84

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;
        writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Size: {}x{} pixels, 1 band, UInt16", width, height);
        println!("   Compression: LZW");
        println!("   Coordinate System: EPSG:4326 (WGS84)");
        println!("   Origin: -122°W, 37°N");
        println!("   Pixel Size: 0.001° x 0.001°\n");
    }

    // Example 4: Float32 data with NoData value
    {
        println!("4. Creating Float32 GeoTIFF with NoData value...");
        let mut path = output_dir.clone();
        path.push("float32_with_nodata.tif");

        let width = 128u64;
        let height = 128u64;

        // Create Float32 test data with some "no data" pixels
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let value = if x < 10 || y < 10 {
                    -9999.0_f32 // NoData value
                } else {
                    ((x as f32) * (y as f32)).sqrt()
                };
                data.extend_from_slice(&value.to_le_bytes());
            }
        }

        let config = WriterConfig::new(width, height, 1, RasterDataType::Float32)
            .with_compression(Compression::Lzw)
            .with_tile_size(64, 64)
            .with_nodata(NoDataValue::Float(-9999.0));

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;
        writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Size: {}x{} pixels, 1 band, Float32", width, height);
        println!("   Compression: LZW");
        println!("   NoData Value: -9999.0\n");
    }

    // Example 5: Striped TIFF (no tiling)
    {
        println!("5. Creating striped TIFF (non-tiled layout)...");
        let mut path = output_dir.clone();
        path.push("striped.tif");

        let width = 150u64;
        let height = 100u64;

        // Create test data
        let mut data = Vec::with_capacity((width * height) as usize);
        for _y in 0..height {
            for x in 0..width {
                data.push(((x * 2) % 256) as u8);
            }
        }

        let mut config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw);
        // Set tile dimensions to None for striped layout
        config.tile_width = None;
        config.tile_height = None;

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;
        writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Size: {}x{} pixels, 1 band, UInt8", width, height);
        println!("   Compression: LZW");
        println!("   Layout: Striped (non-tiled)\n");
    }

    // Example 6: ZSTD compression (if available)
    #[cfg(feature = "zstd")]
    {
        println!("6. Creating GeoTIFF with ZSTD compression...");
        let mut path = output_dir.clone();
        path.push("zstd_compressed.tif");

        let width = 200u64;
        let height = 200u64;

        // Create test data
        let mut data = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                data.push((((x + y) * 3) % 256) as u8);
            }
        }

        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Zstd)
            .with_tile_size(128, 128);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;
        writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Size: {}x{} pixels, 1 band, UInt8", width, height);
        println!("   Compression: ZSTD");
        println!("   Layout: Tiled (128x128)\n");
    }

    // Example 7: PackBits compression
    {
        println!("7. Creating GeoTIFF with PackBits compression...");
        let mut path = output_dir.clone();
        path.push("packbits_compressed.tif");

        let width = 128u64;
        let height = 128u64;

        // Create test data with repeating patterns (good for PackBits)
        let mut data = Vec::with_capacity((width * height) as usize);
        for _y in 0..height {
            for x in 0..width {
                // Create alternating stripes
                data.push(if (x / 16) % 2 == 0 { 255 } else { 0 });
            }
        }

        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Packbits)
            .with_tile_size(64, 64);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;
        writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Size: {}x{} pixels, 1 band, UInt8", width, height);
        println!("   Compression: PackBits");
        println!("   Layout: Tiled (64x64)\n");
    }

    // Example 8: BigTIFF format
    {
        println!("8. Creating BigTIFF format file...");
        let mut path = output_dir.clone();
        path.push("bigtiff.tif");

        let width = 100u64;
        let height = 100u64;

        // Create test data
        let data = vec![42u8; (width * height) as usize];

        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(64, 64)
            .with_bigtiff(true); // Enable BigTIFF format

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;
        writer.write(&data)?;

        println!("   Created: {}", path.display());
        println!("   Size: {}x{} pixels, 1 band, UInt8", width, height);
        println!("   Format: BigTIFF");
        println!("   Compression: LZW\n");
    }

    println!("\nAll examples completed successfully!");
    println!("Output directory: {}", output_dir.display());
    println!("\nYou can inspect these files with:");
    println!("  - gdalinfo <filename>");
    println!("  - rio info <filename>");
    println!("  - The read-geotiff example in this package");

    Ok(())
}
