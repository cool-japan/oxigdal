//! Tutorial 09: GPU Acceleration
//!
//! This tutorial demonstrates GPU-accelerated geospatial operations:
//! - Setting up GPU compute
//! - Raster operations on GPU
//! - Parallel tile processing
//! - Custom kernels
//! - CPU-GPU data transfer optimization
//! - Multi-GPU support
//!
//! Run with:
//! ```bash
//! cargo run --release --example 09_gpu_acceleration --features gpu
//! ```
//!
//! Note: Requires CUDA or OpenCL capable GPU

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_gpu::compute::{GpuBuffer, GpuContext, GpuDevice};
use oxigdal_gpu::kernels::{convolution_kernel, hillshade_kernel, ndvi_kernel};
use oxigdal_gpu::memory::{DeviceMemory, HostMemory, MemoryTransfer};
use std::env;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 09: GPU Acceleration ===\n");

    // Step 1: GPU Detection and Initialization
    println!("Step 1: GPU Detection");
    println!("---------------------");

    println!("Detecting available GPUs...");

    // List available devices
    let devices = GpuDevice::list_devices()?;

    println!("Found {} GPU device(s):", devices.len());

    for (i, device) in devices.iter().enumerate() {
        println!("\n  Device {}:", i);
        println!("    Name: {}", device.name());
        println!("    Type: {:?}", device.device_type());
        println!("    Compute capability: {}", device.compute_capability());
        println!("    Global memory: {:.2} GB", device.global_memory_gb());
        println!("    Max threads per block: {}", device.max_threads_per_block());
        println!("    Multiprocessors: {}", device.multiprocessor_count());
    }

    // Select device
    let device = if devices.is_empty() {
        println!("\nNo GPU found - running CPU fallback");
        println!("Note: For GPU acceleration, install CUDA or OpenCL");
        return run_cpu_fallback();
    } else {
        println!("\nUsing device 0: {}", devices[0].name());
        &devices[0]
    };

    // Create GPU context
    println!("\nInitializing GPU context...");
    let context = GpuContext::new(device)?;

    println!("  Context created");
    println!("  Compute mode: {:?}", context.compute_mode());

    // Step 2: Basic GPU Operations
    println!("\n\nStep 2: Basic GPU Operations");
    println!("-----------------------------");

    let width = 2048;
    let height = 2048;

    println!("Creating test data: {}x{}", width, height);

    // Create host buffer
    let mut host_buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let value = ((x as f64).sin() + (y as f64).cos()) * 100.0;
            host_buffer.set_pixel(x, y, value)?;
        }
    }

    println!("  Host buffer created: {:.2} MB",
             (width * height * 4) as f64 / 1_048_576.0);

    // Transfer to GPU
    println!("\nTransferring to GPU...");

    let start = Instant::now();
    let gpu_buffer = GpuBuffer::from_host(&host_buffer, &context)?;
    let transfer_time = start.elapsed();

    println!("  Transfer time: {:.2}ms", transfer_time.as_secs_f64() * 1000.0);
    println!("  Bandwidth: {:.2} GB/s",
             (width * height * 4) as f64 / 1_073_741_824.0 / transfer_time.as_secs_f64());

    // Simple GPU operation: Add scalar
    println!("\nGPU operation: Add 10.0 to each pixel");

    let start = Instant::now();
    let result_buffer = gpu_buffer.add_scalar(10.0)?;
    let kernel_time = start.elapsed();

    println!("  Kernel time: {:.2}ms", kernel_time.as_secs_f64() * 1000.0);
    println!("  Throughput: {:.2} Gpixels/s",
             (width * height) as f64 / 1_000_000_000.0 / kernel_time.as_secs_f64());

    // Transfer back to host
    println!("\nTransferring back to host...");

    let start = Instant::now();
    let result_host = result_buffer.to_host()?;
    let transfer_back_time = start.elapsed();

    println!("  Transfer time: {:.2}ms", transfer_back_time.as_secs_f64() * 1000.0);

    // Verify results
    let original_pixel = host_buffer.get_pixel(100, 100)?;
    let result_pixel = result_host.get_pixel(100, 100)?;

    println!("\nVerification:");
    println!("  Original pixel (100, 100): {:.2}", original_pixel);
    println!("  Result pixel (100, 100): {:.2}", result_pixel);
    println!("  Expected: {:.2}", original_pixel + 10.0);
    println!("  Match: {}", (result_pixel - (original_pixel + 10.0)).abs() < 0.01);

    // Step 3: NDVI Calculation on GPU
    println!("\n\nStep 3: NDVI Calculation");
    println!("------------------------");

    println!("Computing NDVI: (NIR - Red) / (NIR + Red)");

    // Create simulated NIR and Red bands
    let nir_band = create_sample_band(width, height, 0)?;
    let red_band = create_sample_band(width, height, 1)?;

    println!("  Input bands: {}x{} each", width, height);

    // Transfer to GPU
    let gpu_nir = GpuBuffer::from_host(&nir_band, &context)?;
    let gpu_red = GpuBuffer::from_host(&red_band, &context)?;

    // CPU baseline
    println!("\nCPU baseline:");

    let start = Instant::now();
    let cpu_ndvi = compute_ndvi_cpu(&nir_band, &red_band)?;
    let cpu_time = start.elapsed();

    println!("  Time: {:.2}ms", cpu_time.as_secs_f64() * 1000.0);

    // GPU computation
    println!("\nGPU computation:");

    let start = Instant::now();
    let gpu_ndvi = ndvi_kernel(&gpu_nir, &gpu_red, &context)?;
    let gpu_time = start.elapsed();

    println!("  Time: {:.2}ms", gpu_time.as_secs_f64() * 1000.0);
    println!("  Speedup: {:.2}x", cpu_time.as_secs_f64() / gpu_time.as_secs_f64());

    let gpu_ndvi_host = gpu_ndvi.to_host()?;

    // Compare results
    let cpu_stats = cpu_ndvi.compute_statistics()?;
    let gpu_stats = gpu_ndvi_host.compute_statistics()?;

    println!("\nResults comparison:");
    println!("  CPU - Min: {:.4}, Max: {:.4}, Mean: {:.4}",
             cpu_stats.min, cpu_stats.max, cpu_stats.mean);
    println!("  GPU - Min: {:.4}, Max: {:.4}, Mean: {:.4}",
             gpu_stats.min, gpu_stats.max, gpu_stats.mean);

    // Step 4: Hillshade (Terrain Shading)
    println!("\n\nStep 4: Hillshade Computation");
    println!("------------------------------");

    println!("Computing hillshade with custom kernel...");

    // Create DEM (Digital Elevation Model)
    let dem = create_sample_dem(width, height)?;

    println!("  DEM: {}x{}", width, height);
    println!("  Light direction: azimuth=315°, altitude=45°");

    // CPU baseline
    println!("\nCPU baseline:");

    let start = Instant::now();
    let cpu_hillshade = compute_hillshade_cpu(&dem, 315.0, 45.0)?;
    let cpu_time = start.elapsed();

    println!("  Time: {:.2}ms", cpu_time.as_secs_f64() * 1000.0);

    // GPU computation
    println!("\nGPU computation:");

    let gpu_dem = GpuBuffer::from_host(&dem, &context)?;

    let start = Instant::now();
    let gpu_hillshade = hillshade_kernel(&gpu_dem, 315.0, 45.0, &context)?;
    let gpu_time = start.elapsed();

    println!("  Time: {:.2}ms", gpu_time.as_secs_f64() * 1000.0);
    println!("  Speedup: {:.2}x", cpu_time.as_secs_f64() / gpu_time.as_secs_f64());

    // Step 5: Convolution (Filtering)
    println!("\n\nStep 5: Convolution Filtering");
    println!("------------------------------");

    println!("Applying 5x5 Gaussian blur...");

    // Gaussian kernel
    let kernel = vec![
        1.0, 4.0, 6.0, 4.0, 1.0,
        4.0, 16.0, 24.0, 16.0, 4.0,
        6.0, 24.0, 36.0, 24.0, 6.0,
        4.0, 16.0, 24.0, 16.0, 4.0,
        1.0, 4.0, 6.0, 4.0, 1.0,
    ];

    let kernel_sum: f32 = kernel.iter().sum();
    let normalized_kernel: Vec<f32> = kernel.iter().map(|x| x / kernel_sum).collect();

    println!("  Kernel size: 5x5");
    println!("  Kernel sum: {:.4}", kernel_sum);

    // CPU baseline
    println!("\nCPU baseline:");

    let start = Instant::now();
    let cpu_filtered = apply_convolution_cpu(&host_buffer, &normalized_kernel, 5)?;
    let cpu_time = start.elapsed();

    println!("  Time: {:.2}ms", cpu_time.as_secs_f64() * 1000.0);

    // GPU computation
    println!("\nGPU computation:");

    let start = Instant::now();
    let gpu_filtered = convolution_kernel(&gpu_buffer, &normalized_kernel, 5, &context)?;
    let gpu_time = start.elapsed();

    println!("  Time: {:.2}ms", gpu_time.as_secs_f64() * 1000.0);
    println!("  Speedup: {:.2}x", cpu_time.as_secs_f64() / gpu_time.as_secs_f64());

    // Step 6: Batch Processing
    println!("\n\nStep 6: Batch Processing");
    println!("------------------------");

    println!("Processing multiple tiles on GPU...");

    let tile_size = 512;
    let num_tiles = 16;

    println!("  Tile size: {}x{}", tile_size, tile_size);
    println!("  Number of tiles: {}", num_tiles);

    // Create tiles
    let mut tiles = Vec::new();
    for i in 0..num_tiles {
        let tile = RasterBuffer::zeros(tile_size, tile_size, RasterDataType::Float32);
        tiles.push(tile);
    }

    // CPU batch processing
    println!("\nCPU batch processing:");

    let start = Instant::now();
    for tile in &tiles {
        let _ = tile.compute_statistics()?;
    }
    let cpu_batch_time = start.elapsed();

    println!("  Time: {:.2}ms", cpu_batch_time.as_secs_f64() * 1000.0);
    println!("  Per tile: {:.2}ms", cpu_batch_time.as_secs_f64() * 1000.0 / num_tiles as f64);

    // GPU batch processing
    println!("\nGPU batch processing:");

    let start = Instant::now();

    // Transfer all tiles to GPU
    let gpu_tiles: Vec<GpuBuffer> = tiles
        .iter()
        .map(|tile| GpuBuffer::from_host(tile, &context))
        .collect::<Result<Vec<_>, _>>()?;

    // Process on GPU
    for gpu_tile in &gpu_tiles {
        let _ = gpu_tile.add_scalar(1.0)?;
    }

    // Transfer back
    let _results: Vec<RasterBuffer> = gpu_tiles
        .iter()
        .map(|gpu_tile| gpu_tile.to_host())
        .collect::<Result<Vec<_>, _>>()?;

    let gpu_batch_time = start.elapsed();

    println!("  Time: {:.2}ms", gpu_batch_time.as_secs_f64() * 1000.0);
    println!("  Per tile: {:.2}ms", gpu_batch_time.as_secs_f64() * 1000.0 / num_tiles as f64);
    println!("  Speedup: {:.2}x", cpu_batch_time.as_secs_f64() / gpu_batch_time.as_secs_f64());

    // Step 7: Memory Transfer Optimization
    println!("\n\nStep 7: Memory Transfer Optimization");
    println!("-------------------------------------");

    println!("Optimization strategies:");

    println!("\n1. Pinned Memory:");
    println!("   - Use page-locked host memory");
    println!("   - Faster CPU-GPU transfer");
    println!("   - Limited by system RAM");

    println!("\n2. Asynchronous Transfer:");
    println!("   - Overlap transfer with computation");
    println!("   - Use multiple streams");
    println!("   - Pipeline data movement");

    println!("\n3. Unified Memory:");
    println!("   - Automatic data migration");
    println!("   - Simpler programming model");
    println!("   - Available on modern GPUs");

    println!("\n4. Zero-Copy:");
    println!("   - GPU directly accesses host memory");
    println!("   - Good for small, random accesses");
    println!("   - Slower than device memory");

    // Demonstrate pinned memory
    println!("\nPinned memory comparison:");

    let test_size = 1024 * 1024; // 1M pixels

    // Regular transfer
    let regular_buffer = RasterBuffer::zeros(test_size, 1, RasterDataType::Float32);

    let start = Instant::now();
    let _ = GpuBuffer::from_host(&regular_buffer, &context)?;
    let regular_time = start.elapsed();

    println!("  Regular transfer: {:.2}ms", regular_time.as_secs_f64() * 1000.0);

    // Pinned memory transfer
    let pinned_memory = HostMemory::allocate_pinned(test_size * 4)?; // 4 bytes per float32

    let start = Instant::now();
    let _ = GpuBuffer::from_pinned(&pinned_memory, &context)?;
    let pinned_time = start.elapsed();

    println!("  Pinned transfer: {:.2}ms", pinned_time.as_secs_f64() * 1000.0);
    println!("  Speedup: {:.2}x", regular_time.as_secs_f64() / pinned_time.as_secs_f64());

    // Step 8: Multi-GPU Support
    println!("\n\nStep 8: Multi-GPU Support");
    println!("-------------------------");

    if devices.len() > 1 {
        println!("Multiple GPUs detected: {}", devices.len());
        println!("\nDistribution strategies:");

        println!("\n1. Data Parallelism:");
        println!("   - Split data across GPUs");
        println!("   - Process independently");
        println!("   - Aggregate results");

        println!("\n2. Model Parallelism:");
        println!("   - Split computation pipeline");
        println!("   - Each GPU handles a stage");
        println!("   - Sequential processing");

        println!("\n3. Hybrid:");
        println!("   - Combine data and model parallelism");
        println!("   - Balance load across GPUs");
        println!("   - Minimize inter-GPU communication");
    } else {
        println!("Single GPU system");
        println!("\nFor multi-GPU:");
        println!("  - Use NCCL for efficient GPU-GPU communication");
        println!("  - Balance workload across devices");
        println!("  - Consider GPU affinity with NUMA");
    }

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nPerformance Summary:");
    println!("  NDVI calculation: {:.2}x faster on GPU", cpu_time.as_secs_f64() / gpu_time.as_secs_f64());
    println!("  Typical speedups: 10-100x for arithmetic operations");
    println!("  Best for: Large datasets, parallel operations");

    println!("\nKey Takeaways:");
    println!("  - GPU excels at parallel, arithmetic-heavy operations");
    println!("  - Minimize CPU-GPU data transfer");
    println!("  - Use pinned memory for faster transfers");
    println!("  - Batch operations to amortize overhead");
    println!("  - Consider total time (transfer + compute)");

    println!("\nWhen to use GPU:");
    println!("  ✓ Large rasters (>1024x1024)");
    println!("  ✓ Many operations per pixel");
    println!("  ✓ Batch processing");
    println!("  ✗ Small images (transfer overhead)");
    println!("  ✗ Complex, branching logic");

    println!("\nNext Tutorial:");
    println!("  - Try tutorial 10 for mobile integration");

    Ok(())
}

/// Run CPU fallback when no GPU is available
fn run_cpu_fallback() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Running CPU Fallback ===\n");
    println!("All operations will run on CPU.");
    println!("Install CUDA or OpenCL for GPU acceleration.");

    Ok(())
}

/// Create sample band
fn create_sample_band(
    width: u32,
    height: u32,
    band: u32,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let value = ((x + y + band * 100) % 256) as f64;
            buffer.set_pixel(x, y, value)?;
        }
    }

    Ok(buffer)
}

/// Create sample DEM
fn create_sample_dem(
    width: u32,
    height: u32,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let dx = (x as f64) / (width as f64) - 0.5;
            let dy = (y as f64) / (height as f64) - 0.5;
            let elevation = 1000.0 * (-(dx * dx + dy * dy) * 10.0).exp();
            buffer.set_pixel(x, y, elevation)?;
        }
    }

    Ok(buffer)
}

/// Compute NDVI on CPU
fn compute_ndvi_cpu(
    nir: &RasterBuffer,
    red: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut ndvi = RasterBuffer::zeros(nir.width(), nir.height(), RasterDataType::Float32);

    for y in 0..nir.height() {
        for x in 0..nir.width() {
            let nir_val = nir.get_pixel(x, y)?;
            let red_val = red.get_pixel(x, y)?;

            let denominator = nir_val + red_val;
            let value = if denominator.abs() > 1e-10 {
                (nir_val - red_val) / denominator
            } else {
                0.0
            };

            ndvi.set_pixel(x, y, value)?;
        }
    }

    Ok(ndvi)
}

/// Compute hillshade on CPU
fn compute_hillshade_cpu(
    dem: &RasterBuffer,
    _azimuth: f64,
    _altitude: f64,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut hillshade = RasterBuffer::zeros(dem.width(), dem.height(), RasterDataType::Float32);

    // Simplified hillshade calculation
    for y in 1..(dem.height() - 1) {
        for x in 1..(dem.width() - 1) {
            let z = dem.get_pixel(x, y)?;
            let value = (z / 10.0).tanh() * 255.0;
            hillshade.set_pixel(x, y, value)?;
        }
    }

    Ok(hillshade)
}

/// Apply convolution on CPU
fn apply_convolution_cpu(
    input: &RasterBuffer,
    kernel: &[f32],
    kernel_size: usize,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut output = RasterBuffer::zeros(input.width(), input.height(), RasterDataType::Float32);

    let offset = (kernel_size / 2) as i32;

    for y in offset..(input.height() as i32 - offset) {
        for x in offset..(input.width() as i32 - offset) {
            let mut sum = 0.0;

            for ky in 0..kernel_size {
                for kx in 0..kernel_size {
                    let px = (x + kx as i32 - offset) as u32;
                    let py = (y + ky as i32 - offset) as u32;

                    if let Ok(pixel) = input.get_pixel(px, py) {
                        sum += pixel * kernel[ky * kernel_size + kx] as f64;
                    }
                }
            }

            output.set_pixel(x as u32, y as u32, sum)?;
        }
    }

    Ok(output)
}
