//! Example: Image Resampling
//!
//! This example demonstrates how to:
//! - Resample raster data to different dimensions
//! - Use different resampling methods (nearest, bilinear, bicubic, lanczos)
//! - Compare quality and performance
//!
//! Run with:
//! ```bash
//! cargo run --example image_resampling --release
//! ```

use oxigdal_algorithms::resampling::{Resampler, ResamplingMethod};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Image Resampling Example ===");
    println!();

    // Create a test raster with a pattern
    println!("Creating test raster (2048x2048)...");
    let src = create_test_pattern(2048, 2048)?;
    println!("Created {} bytes of test data", src.as_bytes().len());
    println!();

    // Target dimensions
    let target_width = 512;
    let target_height = 512;

    println!("Resampling to {}x{} using different methods:", target_width, target_height);
    println!();

    // Test each resampling method
    let methods = vec![
        ResamplingMethod::Nearest,
        ResamplingMethod::Bilinear,
        ResamplingMethod::Bicubic,
        ResamplingMethod::Lanczos,
    ];

    for method in methods {
        println!("--- {} ---", method.name());

        let resampler = Resampler::new(method);

        // Measure time
        let start = Instant::now();
        let dst = resampler.resample(&src, target_width, target_height)?;
        let elapsed = start.elapsed();

        println!("Result: {}x{}", dst.width(), dst.height());
        println!("Data type: {:?}", dst.data_type());
        println!("Time: {:.3}ms", elapsed.as_secs_f64() * 1000.0);

        // Compute statistics on result
        let stats = dst.compute_statistics()?;
        println!("Stats:");
        println!("  Min: {:.2}", stats.min);
        println!("  Max: {:.2}", stats.max);
        println!("  Mean: {:.2}", stats.mean);
        println!("  Std Dev: {:.2}", stats.std_dev);
        println!("  Valid pixels: {}", stats.valid_count);

        println!();
    }

    // Demonstrate upsampling
    println!("=== Upsampling ===");
    println!("Upsampling 256x256 to 1024x1024...");
    println!();

    let small = create_test_pattern(256, 256)?;

    for method in &[
        ResamplingMethod::Nearest,
        ResamplingMethod::Bilinear,
        ResamplingMethod::Bicubic,
    ] {
        println!("--- {} ---", method.name());

        let resampler = Resampler::new(*method);
        let start = Instant::now();
        let large = resampler.resample(&small, 1024, 1024)?;
        let elapsed = start.elapsed();

        println!("Result: {}x{}", large.width(), large.height());
        println!("Time: {:.3}ms", elapsed.as_secs_f64() * 1000.0);
        println!();
    }

    // Performance comparison
    println!("=== Performance Comparison ===");
    println!("Resampling 4096x4096 to 1024x1024 (10 iterations)");
    println!();

    let large_src = create_test_pattern(4096, 4096)?;
    let iterations = 10;

    for method in &[
        ResamplingMethod::Nearest,
        ResamplingMethod::Bilinear,
        ResamplingMethod::Bicubic,
    ] {
        let resampler = Resampler::new(*method);
        let mut total_time = 0.0;

        for _ in 0..iterations {
            let start = Instant::now();
            let _ = resampler.resample(&large_src, 1024, 1024)?;
            total_time += start.elapsed().as_secs_f64();
        }

        let avg_time = total_time / iterations as f64;
        println!("{}: {:.3}ms average", method.name(), avg_time * 1000.0);
    }

    println!();
    println!("=== Done ===");
    println!("Note: Run with --release for realistic performance measurements");

    Ok(())
}

/// Create a test pattern raster
fn create_test_pattern(width: u64, height: u64) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::UInt8);

    // Create a checkerboard + gradient pattern
    for y in 0..height {
        for x in 0..width {
            // Checkerboard
            let checker = if (x / 64 + y / 64) % 2 == 0 {
                128.0
            } else {
                64.0
            };

            // Gradient
            let grad_x = (x as f64 / width as f64) * 127.0;
            let grad_y = (y as f64 / height as f64) * 127.0;

            // Combine
            let value = ((checker + grad_x + grad_y) / 3.0).min(255.0);

            buffer.set_pixel(x, y, value)?;
        }
    }

    Ok(buffer)
}
