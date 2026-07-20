//! Tutorial 08: Performance Optimization
//!
//! This tutorial demonstrates performance optimization techniques:
//! - Parallel processing with Rayon
//! - Contiguous-slice ("SIMD-friendly") access vs per-pixel API calls
//! - Memory pooling
//! - Cache-friendly tiling (blocked matrix transpose)
//!
//! Run with:
//! ```bash
//! cargo run --release --example tutorial_08_performance
//! ```

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use rayon::prelude::*;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 08: Performance Optimization ===\n");

    // Step 1: Baseline Performance
    println!("Step 1: Establishing Baseline");
    println!("------------------------------");

    let width = 2048u64;
    let height = 2048u64;

    println!("Creating test raster: {}x{} pixels", width, height);

    let start = Instant::now();
    let buffer = create_test_buffer(width, height)?;
    let creation_time = start.elapsed();

    println!(
        "  Buffer created in: {:.2}ms",
        creation_time.as_secs_f64() * 1000.0
    );
    println!(
        "  Memory size: {:.2} MB",
        (width * height * 4) as f64 / 1_048_576.0
    );

    // Baseline: Simple statistics computation
    println!("\nBaseline: Computing statistics (serial)...");

    let start = Instant::now();
    let stats = buffer.compute_statistics()?;
    let serial_time = start.elapsed();

    println!("  Time: {:.2}ms", serial_time.as_secs_f64() * 1000.0);
    println!(
        "  Min: {:.2}, Max: {:.2}, Mean: {:.2}",
        stats.min, stats.max, stats.mean
    );

    // Step 2: Parallel Processing with Rayon
    println!("\n\nStep 2: Parallel Processing");
    println!("----------------------------");

    println!("Using Rayon for parallel row-wise statistics...");

    let start = Instant::now();
    let parallel_stats = compute_statistics_parallel(&buffer)?;
    let parallel_time = start.elapsed();

    println!("  Time: {:.2}ms", parallel_time.as_secs_f64() * 1000.0);
    println!(
        "  Speedup: {:.2}x",
        serial_time.as_secs_f64() / parallel_time.as_secs_f64().max(1e-9)
    );
    println!(
        "  Min: {:.2}, Max: {:.2}, Mean: {:.2}",
        parallel_stats.min, parallel_stats.max, parallel_stats.mean
    );

    // Parallel tile processing
    println!("\nParallel tile processing:");

    let tile_size = 512u64;
    let num_tiles = (width / tile_size) * (height / tile_size);

    println!("  Tile size: {}x{}", tile_size, tile_size);
    println!("  Number of tiles: {}", num_tiles);

    let start = Instant::now();
    process_tiles_serial(&buffer, tile_size)?;
    let serial_tile_time = start.elapsed();

    println!(
        "\n  Serial processing: {:.2}ms",
        serial_tile_time.as_secs_f64() * 1000.0
    );

    let start = Instant::now();
    process_tiles_parallel(&buffer, tile_size)?;
    let parallel_tile_time = start.elapsed();

    println!(
        "  Parallel processing: {:.2}ms",
        parallel_tile_time.as_secs_f64() * 1000.0
    );
    println!(
        "  Speedup: {:.2}x",
        serial_tile_time.as_secs_f64() / parallel_tile_time.as_secs_f64().max(1e-9)
    );

    // Step 3: Contiguous slice access ("SIMD-friendly")
    println!("\n\nStep 3: Contiguous Slice Access");
    println!("--------------------------------");

    println!("Comparing per-pixel API access vs. raw contiguous slice access:");

    // Scalar addition via the per-pixel API
    println!("\nPer-pixel addition (add 10.0 to each pixel):");

    let start = Instant::now();
    let mut per_pixel_result = buffer.clone();
    scalar_add_per_pixel(&mut per_pixel_result, 10.0)?;
    let per_pixel_time = start.elapsed();

    println!("  Time: {:.2}ms", per_pixel_time.as_secs_f64() * 1000.0);

    // Same operation via a raw contiguous `f32` slice
    println!("\nContiguous slice addition (add 10.0 to each pixel):");

    let start = Instant::now();
    let mut slice_result = buffer.clone();
    scalar_add_slice(&mut slice_result, 10.0)?;
    let slice_time = start.elapsed();

    println!("  Time: {:.2}ms", slice_time.as_secs_f64() * 1000.0);
    println!(
        "  Speedup: {:.2}x",
        per_pixel_time.as_secs_f64() / slice_time.as_secs_f64().max(1e-9)
    );

    // Verify results match
    let per_pixel_stats = per_pixel_result.compute_statistics()?;
    let slice_stats = slice_result.compute_statistics()?;

    println!(
        "  Results match: {}",
        (per_pixel_stats.mean - slice_stats.mean).abs() < 0.01
    );

    // Step 4: Memory Pooling
    println!("\n\nStep 4: Memory Pooling");
    println!("-----------------------");

    println!("Memory pooling example (reusing buffers instead of allocating fresh ones):");

    let pool_size = 10;

    let start = Instant::now();
    let mut buffer_pool: Vec<RasterBuffer> = Vec::with_capacity(pool_size);
    for _ in 0..pool_size {
        buffer_pool.push(RasterBuffer::zeros(
            tile_size,
            tile_size,
            RasterDataType::Float32,
        ));
    }
    let pool_creation_time = start.elapsed();

    println!("  Created pool of {} buffers", pool_size);
    println!("  Time: {:.2}ms", pool_creation_time.as_secs_f64() * 1000.0);

    // Reuse from pool (fill in place rather than reallocating)
    let start = Instant::now();
    for buf in &mut buffer_pool {
        buf.fill_value(0.0);
    }
    let reuse_time = start.elapsed();

    println!("  Reused {} buffers", pool_size);
    println!("  Time: {:.2}ms", reuse_time.as_secs_f64() * 1000.0);
    println!(
        "  Speedup factor: {:.2}x (fill vs. fresh allocation)",
        pool_creation_time.as_secs_f64() / reuse_time.as_secs_f64().max(1e-9)
    );

    // Step 5: Cache Optimization (Blocked Matrix Transpose)
    println!("\n\nStep 5: Cache Optimization");
    println!("---------------------------");

    println!("Comparing naive vs. tiled (cache-blocked) matrix transpose:");

    let matrix_size = 2048;

    let start = Instant::now();
    matrix_transpose_naive(matrix_size);
    let naive_time = start.elapsed();

    println!(
        "  Naive transpose: {:.2}ms",
        naive_time.as_secs_f64() * 1000.0
    );

    let start = Instant::now();
    matrix_transpose_tiled(matrix_size, 64);
    let tiled_time = start.elapsed();

    println!(
        "  Tiled transpose (64x64): {:.2}ms",
        tiled_time.as_secs_f64() * 1000.0
    );
    println!(
        "  Speedup: {:.2}x",
        naive_time.as_secs_f64() / tiled_time.as_secs_f64().max(1e-9)
    );

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nPerformance improvements achieved:");
    println!(
        "  Parallelization: {:.2}x speedup",
        serial_time.as_secs_f64() / parallel_time.as_secs_f64().max(1e-9)
    );
    println!(
        "  Contiguous access: {:.2}x speedup",
        per_pixel_time.as_secs_f64() / slice_time.as_secs_f64().max(1e-9)
    );
    println!(
        "  Tiling: {:.2}x speedup",
        naive_time.as_secs_f64() / tiled_time.as_secs_f64().max(1e-9)
    );

    println!("\nKey Takeaways:");
    println!("  - Profile first, optimize second");
    println!("  - Rayon parallelism is close to free for embarrassingly parallel work");
    println!("  - Contiguous slice access avoids per-call bounds/type-conversion overhead");
    println!("  - Cache-friendly (blocked) algorithms are crucial for large data");
    println!("  - Measure, don't guess");

    Ok(())
}

/// Create test buffer with sample data
fn create_test_buffer(width: u64, height: u64) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let value = ((x as f64).sin() + (y as f64).cos()) * 100.0;
            buffer.set_pixel(x, y, value)?;
        }
    }

    Ok(buffer)
}

struct Statistics {
    min: f64,
    max: f64,
    mean: f64,
}

/// Compute statistics in parallel (row-wise, aggregated afterwards)
fn compute_statistics_parallel(
    buffer: &RasterBuffer,
) -> Result<Statistics, Box<dyn std::error::Error>> {
    let height = buffer.height();
    let width = buffer.width();

    let row_stats: Vec<(f64, f64, f64, u64)> = (0..height)
        .into_par_iter()
        .map(|y| {
            let mut min_val = f64::INFINITY;
            let mut max_val = f64::NEG_INFINITY;
            let mut sum = 0.0;
            let mut count = 0u64;

            for x in 0..width {
                if let Ok(value) = buffer.get_pixel(x, y) {
                    min_val = min_val.min(value);
                    max_val = max_val.max(value);
                    sum += value;
                    count += 1;
                }
            }

            (min_val, max_val, sum, count)
        })
        .collect();

    let mut global_min = f64::INFINITY;
    let mut global_max = f64::NEG_INFINITY;
    let mut global_sum = 0.0;
    let mut global_count = 0u64;

    for (min_val, max_val, sum, count) in row_stats {
        global_min = global_min.min(min_val);
        global_max = global_max.max(max_val);
        global_sum += sum;
        global_count += count;
    }

    Ok(Statistics {
        min: global_min,
        max: global_max,
        mean: global_sum / global_count.max(1) as f64,
    })
}

/// Process tiles serially
fn process_tiles_serial(
    buffer: &RasterBuffer,
    tile_size: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let tiles_x = buffer.width().div_ceil(tile_size);
    let tiles_y = buffer.height().div_ceil(tile_size);

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let _ = process_single_tile(buffer, tx * tile_size, ty * tile_size, tile_size)?;
        }
    }

    Ok(())
}

/// Process tiles in parallel
fn process_tiles_parallel(
    buffer: &RasterBuffer,
    tile_size: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let tiles_x = buffer.width().div_ceil(tile_size);
    let tiles_y = buffer.height().div_ceil(tile_size);

    let tile_coords: Vec<(u64, u64)> = (0..tiles_y)
        .flat_map(|ty| (0..tiles_x).map(move |tx| (tx, ty)))
        .collect();

    tile_coords.par_iter().try_for_each(|(tx, ty)| {
        process_single_tile(buffer, tx * tile_size, ty * tile_size, tile_size).map(|_| ())
    })?;

    Ok(())
}

/// Process a single tile
fn process_single_tile(
    buffer: &RasterBuffer,
    x: u64,
    y: u64,
    size: u64,
) -> oxigdal_core::Result<f64> {
    let mut sum = 0.0;

    let max_x = (x + size).min(buffer.width());
    let max_y = (y + size).min(buffer.height());

    for py in y..max_y {
        for px in x..max_x {
            sum += buffer.get_pixel(px, py)?;
        }
    }

    Ok(sum)
}

/// Add a scalar to every pixel using the safe per-pixel API
fn scalar_add_per_pixel(
    buffer: &mut RasterBuffer,
    value: f64,
) -> Result<(), Box<dyn std::error::Error>> {
    for y in 0..buffer.height() {
        for x in 0..buffer.width() {
            let pixel = buffer.get_pixel(x, y)?;
            buffer.set_pixel(x, y, pixel + value)?;
        }
    }

    Ok(())
}

/// Add a scalar to every pixel via a raw contiguous `f32` slice
fn scalar_add_slice(
    buffer: &mut RasterBuffer,
    value: f64,
) -> Result<(), Box<dyn std::error::Error>> {
    let value = value as f32;
    let slice: &mut [f32] = buffer.as_slice_mut()?;
    for pixel in slice.iter_mut() {
        *pixel += value;
    }
    Ok(())
}

/// Naive matrix transpose (poor cache locality)
fn matrix_transpose_naive(size: usize) {
    let mut matrix = vec![0.0f32; size * size];
    for (i, value) in matrix.iter_mut().enumerate() {
        *value = i as f32;
    }

    let mut result = vec![0.0f32; size * size];
    for i in 0..size {
        for j in 0..size {
            result[j * size + i] = matrix[i * size + j];
        }
    }
    std::hint::black_box(&result);
}

/// Tiled matrix transpose (better cache locality)
fn matrix_transpose_tiled(size: usize, tile_size: usize) {
    let mut matrix = vec![0.0f32; size * size];
    for (i, value) in matrix.iter_mut().enumerate() {
        *value = i as f32;
    }

    let mut result = vec![0.0f32; size * size];

    for i_tile in (0..size).step_by(tile_size) {
        for j_tile in (0..size).step_by(tile_size) {
            let i_max = (i_tile + tile_size).min(size);
            let j_max = (j_tile + tile_size).min(size);

            for i in i_tile..i_max {
                for j in j_tile..j_max {
                    result[j * size + i] = matrix[i * size + j];
                }
            }
        }
    }
    std::hint::black_box(&result);
}
