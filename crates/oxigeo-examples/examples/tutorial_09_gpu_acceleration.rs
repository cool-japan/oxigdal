//! Tutorial 09: GPU Acceleration
//!
//! This tutorial demonstrates GPU-accelerated geospatial operations using
//! `oxigeo-gpu`:
//! - Detecting and initializing a GPU context (`GpuContext::new`)
//! - Falling back to the crate's own CPU SIMD-friendly kernels
//!   (`oxigeo_gpu::cpu_fallback::cpu`) when no GPU is available
//! - NDVI band math and simple element-wise operations
//! - Batch processing multiple tiles
//!
//! Run with:
//! ```bash
//! cargo run --release --example tutorial_09_gpu_acceleration
//! ```
//!
//! Note: GPU detection requires a Vulkan/Metal/DX12 capable device and
//! driver. When none is available this example transparently uses the CPU
//! fallback kernels, which are the same code path `oxigeo-gpu` itself uses
//! when a GPU operation is unavailable or fails.

use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::RasterDataType;
use oxigeo_gpu::GpuContext;
use oxigeo_gpu::cpu_fallback::cpu;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 09: GPU Acceleration ===\n");

    // Step 1: GPU Detection and Initialization
    println!("Step 1: GPU Detection");
    println!("---------------------");

    println!("Attempting to initialize a GPU context (Vulkan/Metal/DX12)...");

    match GpuContext::new().await {
        Ok(context) => {
            let info = context.adapter_info();
            println!("GPU context created:");
            println!("  Adapter: {}", info.name);
            println!("  Backend: {:?}", info.backend);
            println!("  Device type: {:?}", info.device_type);
        }
        Err(e) => {
            println!("No GPU adapter available ({e})");
            println!("Falling back to CPU kernels (`oxigeo_gpu::cpu_fallback::cpu`)");
        }
    }

    // Step 2: Basic Element-wise Operations
    println!("\n\nStep 2: Basic Element-wise Operations");
    println!("---------------------------------------");

    let width = 1024u64;
    let height = 1024u64;

    println!("Creating test data: {}x{}", width, height);
    let host_buffer = create_sample_band(width, height, 0);
    let host_slice: &[f32] = host_buffer.as_slice()?;

    println!(
        "  Host buffer created: {:.2} MB",
        (width * height * 4) as f64 / 1_048_576.0
    );

    println!("\nCPU-kernel operation: add 10.0 to each pixel");
    let start = Instant::now();
    let added = cpu::add_scalar(host_slice, 10.0);
    let add_time = start.elapsed();

    println!("  Kernel time: {:.2}ms", add_time.as_secs_f64() * 1000.0);
    println!(
        "  Throughput: {:.2} Mpixels/s",
        (width * height) as f64 / 1_000_000.0 / add_time.as_secs_f64().max(1e-9)
    );

    // Verify results
    println!("\nVerification:");
    println!("  Original pixel (0): {:.2}", host_slice[0]);
    println!("  Result pixel (0):   {:.2}", added[0]);
    println!("  Expected:           {:.2}", host_slice[0] + 10.0);
    println!(
        "  Match: {}",
        (added[0] - (host_slice[0] + 10.0)).abs() < 0.01
    );

    // Step 3: NDVI Calculation
    println!("\n\nStep 3: NDVI Calculation");
    println!("------------------------");

    println!("Computing NDVI: (NIR - Red) / (NIR + Red)");

    let nir_band = create_sample_band(width, height, 0);
    let red_band = create_sample_band(width, height, 1);

    let nir_slice: &[f32] = nir_band.as_slice()?;
    let red_slice: &[f32] = red_band.as_slice()?;

    println!("  Input bands: {}x{} each", width, height);

    let start = Instant::now();
    let ndvi = cpu::ndvi(red_slice, nir_slice);
    let ndvi_time = start.elapsed();

    println!("  Time: {:.2}ms", ndvi_time.as_secs_f64() * 1000.0);
    println!(
        "  Throughput: {:.2} Mpixels/s",
        (width * height) as f64 / 1_000_000.0 / ndvi_time.as_secs_f64().max(1e-9)
    );

    println!("\nResults:");
    println!("  Min:  {:.4}", cpu::min_value(&ndvi));
    println!("  Max:  {:.4}", cpu::max_value(&ndvi));
    println!("  Mean: {:.4}", cpu::mean(&ndvi));

    // Step 4: Statistics on a DEM-like buffer
    println!("\n\nStep 4: Statistics");
    println!("-------------------");

    let dem = create_sample_dem(width, height);
    let dem_slice: &[f32] = dem.as_slice()?;

    println!("Computing summary statistics with the CPU kernels...");
    println!("  Sum:      {:.2}", cpu::sum(dem_slice));
    println!("  Mean:     {:.2}", cpu::mean(dem_slice));
    println!("  Std dev:  {:.2}", cpu::std_dev(dem_slice));
    println!(
        "  Min/Max:  {:.2} / {:.2}",
        cpu::min_value(dem_slice),
        cpu::max_value(dem_slice)
    );

    // Step 5: Batch Processing
    println!("\n\nStep 5: Batch Processing");
    println!("------------------------");

    println!("Processing multiple tiles...");

    let tile_size = 256u64;
    let num_tiles = 16;

    println!("  Tile size: {}x{}", tile_size, tile_size);
    println!("  Number of tiles: {}", num_tiles);

    let tiles: Vec<RasterBuffer> = (0..num_tiles)
        .map(|_| RasterBuffer::zeros(tile_size, tile_size, RasterDataType::Float32))
        .collect();

    let start = Instant::now();
    for tile in &tiles {
        let slice: &[f32] = tile.as_slice()?;
        let _ = cpu::add_scalar(slice, 1.0);
    }
    let batch_time = start.elapsed();

    println!("  Time: {:.2}ms", batch_time.as_secs_f64() * 1000.0);
    println!(
        "  Per tile: {:.2}ms",
        batch_time.as_secs_f64() * 1000.0 / num_tiles as f64
    );

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nKey Takeaways:");
    println!("  - `GpuContext::new()` detects and initializes a real GPU adapter");
    println!("  - `oxigeo_gpu::cpu_fallback::cpu` provides the same primitive ops on CPU");
    println!("  - Band math (NDVI) and statistics are simple slice operations");
    println!("  - Batch processing amortizes per-call overhead");

    println!("\nWhen to use GPU:");
    println!("  - Large rasters (>1024x1024)");
    println!("  - Many operations per pixel");
    println!("  - Batch processing");
    println!("  Small images and control-flow-heavy code are often faster on CPU");
    println!("  once transfer overhead is accounted for.");

    Ok(())
}

/// Create sample band data
fn create_sample_band(width: u64, height: u64, band: u32) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let value = ((x + y + u64::from(band) * 100) % 256) as f64;
            let _ = buffer.set_pixel(x, y, value);
        }
    }

    buffer
}

/// Create a sample DEM (Digital Elevation Model)
fn create_sample_dem(width: u64, height: u64) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let dx = (x as f64) / (width as f64) - 0.5;
            let dy = (y as f64) / (height as f64) - 0.5;
            let elevation = 1000.0 * (-(dx * dx + dy * dy) * 10.0).exp();
            let _ = buffer.set_pixel(x, y, elevation);
        }
    }

    buffer
}
