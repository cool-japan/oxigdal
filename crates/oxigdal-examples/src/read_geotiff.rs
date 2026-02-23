//! Example: Read GeoTIFF metadata and basic information
//!
//! This example demonstrates opening a GeoTIFF file and extracting
//! metadata including dimensions, CRS, geotransform, and data type.
//!
//! Usage:
//!   cargo run --example read_geotiff -- `<path-to-geotiff>`

use oxigdal_core::io::FileDataSource;
use oxigdal_geotiff::GeoTiffReader;

fn main() -> oxigdal_core::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <geotiff-file>", args[0]);
        eprintln!("\nExample:");
        eprintln!("  {} /path/to/image.tif", args[0]);
        std::process::exit(1);
    }

    let path = &args[1];

    println!("Opening GeoTIFF: {}", path);
    println!("{}", "=".repeat(60));

    // Open the file
    let source = FileDataSource::open(path)?;
    let reader = GeoTiffReader::open(source)?;

    // Print basic information
    println!("\n📐 Dimensions:");
    println!("  Width:  {} pixels", reader.width());
    println!("  Height: {} pixels", reader.height());
    println!("  Bands:  {}", reader.band_count());

    // Print data type
    if let Some(dtype) = reader.data_type() {
        println!("\n🔢 Data Type: {}", dtype);
    }

    // Print compression
    println!("\n📦 Compression: {}", reader.compression().name());

    // Print tiling information
    if let Some((tw, th)) = reader.tile_size() {
        println!("\n🎨 Tiling:");
        println!("  Tile Size: {}x{} pixels", tw, th);
        let (tiles_x, tiles_y) = reader.tile_count();
        println!(
            "  Tile Count: {}x{} = {} tiles",
            tiles_x,
            tiles_y,
            tiles_x * tiles_y
        );
    }

    // Print overview information
    let overview_count = reader.overview_count();
    println!("\n🔍 Overviews: {}", overview_count);
    if overview_count > 0 {
        println!("  Overview pyramid available for efficient zooming");
    }

    // Print CRS information
    if let Some(epsg) = reader.epsg_code() {
        println!("\n🌍 Coordinate Reference System:");
        println!("  EPSG: {}", epsg);
    }

    // Print geotransform
    if let Some(gt) = reader.geo_transform() {
        println!("\n📍 GeoTransform:");
        println!("  Origin X: {}", gt.origin_x);
        println!("  Origin Y: {}", gt.origin_y);
        println!("  Pixel Width:  {}", gt.pixel_width);
        println!("  Pixel Height: {}", gt.pixel_height);
        if gt.has_rotation() {
            println!("  Rotation: {} degrees", gt.rotation_degrees());
        } else {
            println!("  North-Up: yes");
        }

        let (res_x, res_y) = gt.resolution();
        println!("  Resolution: {}x{} units/pixel", res_x, res_y);

        let bounds = gt.compute_bounds(reader.width(), reader.height());
        println!("\n📦 Bounding Box:");
        println!("  West:  {}", bounds.min_x);
        println!("  South: {}", bounds.min_y);
        println!("  East:  {}", bounds.max_x);
        println!("  North: {}", bounds.max_y);
        println!("  Width:  {} units", bounds.width());
        println!("  Height: {} units", bounds.height());
    }

    // Print NoData value
    let nodata = reader.nodata();
    if !nodata.is_none() {
        println!("\n🚫 NoData Value: {:?}", nodata.as_f64());
    }

    println!("\n{}", "=".repeat(60));
    println!("✅ Successfully read GeoTIFF metadata");

    Ok(())
}
