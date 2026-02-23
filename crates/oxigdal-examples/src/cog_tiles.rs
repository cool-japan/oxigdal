//! Example: Access individual tiles from a Cloud Optimized GeoTIFF
//!
//! This example demonstrates reading individual tiles from a COG file,
//! which is essential for efficient cloud-native geospatial workflows.
//!
//! Usage:
//!   cargo run --example cog_tiles -- `<path-to-cog>` \[tile_x\] \[tile_y\] \[level\]

use oxigdal_core::io::FileDataSource;
use oxigdal_geotiff::CogReader;

fn main() -> oxigdal_core::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <cog-file> [tile_x] [tile_y] [level]", args[0]);
        eprintln!("\nExample:");
        eprintln!("  {} /path/to/image.tif", args[0]);
        eprintln!(
            "  {} /path/to/image.tif 0 0 0  # Read tile (0,0) at level 0",
            args[0]
        );
        std::process::exit(1);
    }

    let path = &args[1];
    let tile_x = args.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let tile_y = args.get(3).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let level = args
        .get(4)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);

    println!("Opening COG: {}", path);
    println!("{}", "=".repeat(60));

    // Open the COG
    let source = FileDataSource::open(path)?;
    let reader = CogReader::open(source)?;

    // Print COG information
    println!("\n📊 COG Information:");
    println!(
        "  Image Size: {}x{} pixels",
        reader.width(),
        reader.height()
    );

    if let Some((tw, th)) = reader.tile_size() {
        println!("  Tile Size: {}x{} pixels", tw, th);
        let (nx, ny) = reader.tile_count();
        println!("  Tile Grid: {}x{} tiles ({} total)", nx, ny, nx * ny);
    } else {
        println!("  ⚠️  Not tiled - this is not a valid COG!");
        return Ok(());
    }

    println!("  Overview Levels: {}", reader.overview_count());

    if let Some(epsg) = reader.epsg_code() {
        println!("  EPSG Code: {}", epsg);
    }

    // Validate requested tile
    let (tiles_x, tiles_y) = reader.tile_count();
    if tile_x >= tiles_x || tile_y >= tiles_y {
        println!("\n❌ Tile ({}, {}) is out of bounds!", tile_x, tile_y);
        println!(
            "   Valid range: X=[0, {}], Y=[0, {}]",
            tiles_x - 1,
            tiles_y - 1
        );
        return Ok(());
    }

    if level > reader.overview_count() {
        println!("\n❌ Overview level {} is out of bounds!", level);
        println!("   Valid range: [0, {}]", reader.overview_count());
        return Ok(());
    }

    // Read the requested tile
    println!("\n🎯 Reading Tile:");
    println!(
        "  Level: {} {}",
        level,
        if level == 0 {
            "(full resolution)"
        } else {
            "(overview)"
        }
    );
    println!("  Position: ({}, {})", tile_x, tile_y);

    let tile_data = reader.read_tile(level, tile_x, tile_y)?;

    println!("\n✅ Tile Read Successfully:");
    println!("  Bytes: {}", tile_data.len());
    println!(
        "  Compression: {}",
        reader.primary_info().compression.name()
    );

    // Show byte range information
    let byte_range = reader.tile_byte_range(level, tile_x, tile_y)?;
    println!("\n📍 Tile Location in File:");
    println!("  Offset: {} bytes", byte_range.start);
    println!("  Length: {} bytes", byte_range.len());
    println!("  Range: [{}, {})", byte_range.start, byte_range.end);

    // Compute efficiency metrics
    let compressed_ratio = if !tile_data.is_empty() {
        (tile_data.len() as f64) / (byte_range.len() as f64)
    } else {
        0.0
    };
    println!("\n📊 Efficiency:");
    println!("  Compressed Size: {} bytes", byte_range.len());
    println!("  Decompressed Size: {} bytes", tile_data.len());
    println!("  Expansion Ratio: {:.2}x", compressed_ratio);

    println!("\n{}", "=".repeat(60));
    println!("💡 This demonstrates COG's efficient tile-based access!");
    println!(
        "   Only {} bytes were downloaded from cloud storage.",
        byte_range.len()
    );

    Ok(())
}
