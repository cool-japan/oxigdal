//! Example: Raster buffer operations and statistics
//!
//! This example demonstrates creating and manipulating raster buffers,
//! computing statistics, and type conversions.
//!
//! Usage:
//!   cargo run --example buffer_ops

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{NoDataValue, RasterDataType};

fn main() -> oxigdal_core::Result<()> {
    println!("OxiGDAL Buffer Operations Example");
    println!("{}", "=".repeat(60));

    // Create a simple 100x100 buffer
    println!("\n📦 Creating 100x100 UInt8 buffer...");
    let mut buffer = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);

    // Fill with a gradient pattern
    println!("   Filling with gradient pattern...");
    for y in 0..100 {
        for x in 0..100 {
            let value = ((x + y) as f64 / 2.0).min(255.0);
            buffer.set_pixel(x, y, value)?;
        }
    }

    // Compute statistics
    println!("\n📊 Computing statistics...");
    let stats = buffer.compute_statistics()?;

    println!("\n📈 Buffer Statistics:");
    println!("  Min:         {:.2}", stats.min);
    println!("  Max:         {:.2}", stats.max);
    println!("  Mean:        {:.2}", stats.mean);
    println!("  Std Dev:     {:.2}", stats.std_dev);
    println!("  Valid Count: {}", stats.valid_count);

    // Create a buffer with NoData
    println!("\n🚫 Creating buffer with NoData value (-9999)...");
    let mut nodata_buffer = RasterBuffer::nodata_filled(
        50,
        50,
        RasterDataType::Float32,
        NoDataValue::from_float(-9999.0),
    );

    // Set some valid values
    for y in 10..40 {
        for x in 10..40 {
            let value = ((x * y) as f64).sqrt();
            nodata_buffer.set_pixel(x, y, value)?;
        }
    }

    println!("   Computing statistics (excluding NoData)...");
    let nodata_stats = nodata_buffer.compute_statistics()?;

    println!("\n📈 NoData Buffer Statistics:");
    println!("  Min:         {:.2}", nodata_stats.min);
    println!("  Max:         {:.2}", nodata_stats.max);
    println!("  Mean:        {:.2}", nodata_stats.mean);
    println!("  Std Dev:     {:.2}", nodata_stats.std_dev);
    println!("  Valid Count: {} / {}", nodata_stats.valid_count, 50 * 50);
    println!("  NoData Count: {}", 50 * 50 - nodata_stats.valid_count);

    // Type conversion
    println!("\n🔄 Converting UInt8 buffer to Float64...");
    let float_buffer = buffer.convert_to(RasterDataType::Float64)?;
    println!("   Original: {} bytes", buffer.as_bytes().len());
    println!("   Converted: {} bytes", float_buffer.as_bytes().len());
    println!(
        "   Size ratio: {:.1}x",
        float_buffer.as_bytes().len() as f64 / buffer.as_bytes().len() as f64
    );

    // Sample pixel values
    println!("\n🔬 Sample Pixel Values:");
    println!("   Position  │ Value");
    println!("  ───────────┼────────");
    for (x, y) in [(0, 0), (50, 50), (99, 99)] {
        let value = buffer.get_pixel(x, y)?;
        println!("   ({:3}, {:3}) │ {:.2}", x, y, value);
    }

    println!("\n{}", "=".repeat(60));
    println!("✅ Buffer operations completed successfully");

    Ok(())
}
