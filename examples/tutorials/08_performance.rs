//! Tutorial 08: Performance Optimization
//!
//! This tutorial demonstrates performance optimization techniques:
//! - Parallel processing with Rayon
//! - SIMD vectorization
//! - Memory optimization
//! - I/O optimization
//! - Profiling and benchmarking
//! - Cache optimization
//!
//! Run with:
//! ```bash
//! cargo run --release --example 08_performance
//! ```

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::simd_buffer::SimdBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use rayon::prelude::*;
use std::env;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 08: Performance Optimization ===\n");

    let temp_dir = env::temp_dir();

    // Step 1: Baseline Performance
    println!("Step 1: Establishing Baseline");
    println!("------------------------------");

    let width = 4096;
    let height = 4096;

    println!("Creating test raster: {}x{} pixels", width, height);

    let start = Instant::now();
    let buffer = create_test_buffer(width, height)?;
    let creation_time = start.elapsed();

    println!("  Buffer created in: {:.2}ms", creation_time.as_secs_f64() * 1000.0);
    println!("  Memory size: {:.2} MB",
             (width * height * 4) as f64 / 1_048_576.0);

    // Baseline: Simple statistics computation
    println!("\nBaseline: Computing statistics (serial)...");

    let start = Instant::now();
    let stats = buffer.compute_statistics()?;
    let serial_time = start.elapsed();

    println!("  Time: {:.2}ms", serial_time.as_secs_f64() * 1000.0);
    println!("  Min: {:.2}, Max: {:.2}, Mean: {:.2}",
             stats.min, stats.max, stats.mean);

    // Step 2: Parallel Processing with Rayon
    println!("\n\nStep 2: Parallel Processing");
    println!("----------------------------");

    println!("Using Rayon for parallel computation...");

    let start = Instant::now();
    let parallel_stats = compute_statistics_parallel(&buffer)?;
    let parallel_time = start.elapsed();

    println!("  Time: {:.2}ms", parallel_time.as_secs_f64() * 1000.0);
    println!("  Speedup: {:.2}x", serial_time.as_secs_f64() / parallel_time.as_secs_f64());
    println!("  Min: {:.2}, Max: {:.2}, Mean: {:.2}",
             parallel_stats.min, parallel_stats.max, parallel_stats.mean);

    // Parallel tile processing
    println!("\nParallel tile processing:");

    let tile_size = 512;
    let num_tiles = (width / tile_size) * (height / tile_size);

    println!("  Tile size: {}x{}", tile_size, tile_size);
    println!("  Number of tiles: {}", num_tiles);

    let start = Instant::now();
    process_tiles_serial(&buffer, tile_size)?;
    let serial_tile_time = start.elapsed();

    println!("\n  Serial processing: {:.2}ms", serial_tile_time.as_secs_f64() * 1000.0);

    let start = Instant::now();
    process_tiles_parallel(&buffer, tile_size)?;
    let parallel_tile_time = start.elapsed();

    println!("  Parallel processing: {:.2}ms", parallel_tile_time.as_secs_f64() * 1000.0);
    println!("  Speedup: {:.2}x",
             serial_tile_time.as_secs_f64() / parallel_tile_time.as_secs_f64());

    // Step 3: SIMD Vectorization
    println!("\n\nStep 3: SIMD Vectorization");
    println!("--------------------------");

    println!("SIMD (Single Instruction Multiple Data) processing:");

    // Create SIMD-optimized buffer
    let simd_buffer = SimdBuffer::from_buffer(&buffer)?;

    println!("  SIMD buffer created");
    println!("  Vector width: 256-bit (AVX2)");
    println!("  Processes {} pixels per instruction", 256 / 32);

    // Scalar addition
    println!("\nScalar addition (add 10.0 to each pixel):");

    let start = Instant::now();
    let mut scalar_result = buffer.clone();
    scalar_add(&mut scalar_result, 10.0)?;
    let scalar_add_time = start.elapsed();

    println!("  Time: {:.2}ms", scalar_add_time.as_secs_f64() * 1000.0);

    // SIMD addition
    println!("\nSIMD addition (add 10.0 to each pixel):");

    let start = Instant::now();
    let simd_result = simd_buffer.add_scalar(10.0)?;
    let simd_add_time = start.elapsed();

    println!("  Time: {:.2}ms", simd_add_time.as_secs_f64() * 1000.0);
    println!("  Speedup: {:.2}x",
             scalar_add_time.as_secs_f64() / simd_add_time.as_secs_f64());

    // Verify results match
    let scalar_stats = scalar_result.compute_statistics()?;
    let simd_stats = simd_result.to_buffer()?.compute_statistics()?;

    println!("  Results match: {}",
             (scalar_stats.mean - simd_stats.mean).abs() < 0.01);

    // SIMD operations
    println!("\nOther SIMD operations:");

    let start = Instant::now();
    let _ = simd_buffer.multiply_scalar(2.0)?;
    let multiply_time = start.elapsed();

    println!("  Multiply: {:.2}ms", multiply_time.as_secs_f64() * 1000.0);

    let start = Instant::now();
    let _ = simd_buffer.sqrt()?;
    let sqrt_time = start.elapsed();

    println!("  Square root: {:.2}ms", sqrt_time.as_secs_f64() * 1000.0);

    let simd_buffer2 = SimdBuffer::from_buffer(&buffer)?;

    let start = Instant::now();
    let _ = simd_buffer.add(&simd_buffer2)?;
    let add_buffer_time = start.elapsed();

    println!("  Buffer addition: {:.2}ms", add_buffer_time.as_secs_f64() * 1000.0);

    // Step 4: Memory Optimization
    println!("\n\nStep 4: Memory Optimization");
    println!("---------------------------");

    println!("Memory layout considerations:");

    println!("\n1. Data alignment:");
    println!("   - Align buffers to 32-byte boundaries (AVX)");
    println!("   - Align to 64-byte for cache line efficiency");
    println!("   - Use aligned allocations for SIMD");

    println!("\n2. Memory access patterns:");
    println!("   - Sequential access preferred (cache-friendly)");
    println!("   - Avoid strided access when possible");
    println!("   - Process in tile/block order");

    println!("\n3. Memory pooling:");
    println!("   - Reuse buffers to reduce allocations");
    println!("   - Pre-allocate for known sizes");
    println!("   - Use memory pools for temporary buffers");

    // Demonstrate memory pooling
    println!("\nMemory pooling example:");

    let pool_size = 10;
    let mut buffer_pool: Vec<RasterBuffer> = Vec::new();

    let start = Instant::now();
    for _ in 0..pool_size {
        buffer_pool.push(RasterBuffer::zeros(tile_size, tile_size, RasterDataType::Float32));
    }
    let pool_creation_time = start.elapsed();

    println!("  Created pool of {} buffers", pool_size);
    println!("  Time: {:.2}ms", pool_creation_time.as_secs_f64() * 1000.0);

    // Reuse from pool
    let start = Instant::now();
    for buffer in &mut buffer_pool {
        // Reuse buffer (just reset to zeros)
        *buffer = RasterBuffer::zeros(tile_size, tile_size, RasterDataType::Float32);
    }
    let reuse_time = start.elapsed();

    println!("  Reused {} buffers", pool_size);
    println!("  Time: {:.2}ms", reuse_time.as_secs_f64() * 1000.0);
    println!("  Speedup factor: {:.2}x (for repeated operations)",
             pool_creation_time.as_secs_f64() / reuse_time.as_secs_f64());

    // Step 5: I/O Optimization
    println!("\n\nStep 5: I/O Optimization");
    println!("------------------------");

    println!("I/O optimization strategies:");

    println!("\n1. Buffered I/O:");
    println!("   - Use larger read buffers (64KB - 1MB)");
    println!("   - Reduce number of system calls");
    println!("   - Align reads to page boundaries");

    println!("\n2. Async I/O:");
    println!("   - Overlap I/O with computation");
    println!("   - Prefetch tiles before processing");
    println!("   - Use tokio/async-std for async operations");

    println!("\n3. Memory-mapped I/O:");
    println!("   - mmap for random access patterns");
    println!("   - Let OS handle paging");
    println!("   - Good for read-heavy workloads");

    println!("\n4. Direct I/O:");
    println!("   - Bypass OS page cache when appropriate");
    println!("   - Useful for streaming large datasets");
    println!("   - Requires aligned buffers");

    // Step 6: Cache Optimization
    println!("\n\nStep 6: Cache Optimization");
    println!("--------------------------");

    println!("CPU cache hierarchy:");
    println!("  L1: 32KB data + 32KB instruction, ~4 cycles");
    println!("  L2: 256KB-512KB, ~12 cycles");
    println!("  L3: 8MB-32MB (shared), ~40 cycles");
    println!("  RAM: GBs, ~200 cycles");

    println!("\nCache-friendly patterns:");

    println!("\n1. Spatial locality:");
    println!("   - Access nearby memory locations");
    println!("   - Process data in sequential order");
    println!("   - Use blocking/tiling for 2D arrays");

    let cache_line_size = 64; // bytes
    println!("\n2. Cache line efficiency:");
    println!("   - Cache line size: {} bytes", cache_line_size);
    println!("   - Fits {} float32 values per line", cache_line_size / 4);
    println!("   - Avoid false sharing in parallel code");

    println!("\n3. Blocking/Tiling:");
    println!("   - Process data in cache-sized blocks");
    println!("   - Optimal tile size: ~L2 cache size");
    println!("   - Balance between cache use and overhead");

    // Demonstrate tiling effect
    println!("\nTiling comparison:");

    let matrix_size = 2048;

    let start = Instant::now();
    matrix_transpose_naive(matrix_size)?;
    let naive_time = start.elapsed();

    println!("  Naive transpose: {:.2}ms", naive_time.as_secs_f64() * 1000.0);

    let start = Instant::now();
    matrix_transpose_tiled(matrix_size, 64)?;
    let tiled_time = start.elapsed();

    println!("  Tiled transpose (64x64): {:.2}ms", tiled_time.as_secs_f64() * 1000.0);
    println!("  Speedup: {:.2}x", naive_time.as_secs_f64() / tiled_time.as_secs_f64());

    // Step 7: Profiling and Benchmarking
    println!("\n\nStep 7: Profiling and Benchmarking");
    println!("-----------------------------------");

    println!("Profiling tools:");

    println!("\n1. perf (Linux):");
    println!("   perf record --call-graph dwarf ./target/release/app");
    println!("   perf report");

    println!("\n2. Instruments (macOS):");
    println!("   - Time Profiler");
    println!("   - Allocations");
    println!("   - System Trace");

    println!("\n3. cargo flamegraph:");
    println!("   cargo flamegraph --example 08_performance");

    println!("\n4. criterion (micro-benchmarks):");
    println!("   cargo bench --bench performance_bench");

    println!("\nKey metrics to track:");
    println!("  - CPU cycles per pixel");
    println!("  - Cache miss rate");
    println!("  - Memory bandwidth utilization");
    println!("  - Instructions per cycle (IPC)");
    println!("  - SIMD instruction usage");

    // Step 8: Best Practices Summary
    println!("\n\nStep 8: Best Practices");
    println!("----------------------");

    println!("\n1. Algorithm choice:");
    println!("   - Choose O(n log n) over O(n²) when possible");
    println!("   - Consider approximate algorithms for large data");
    println!("   - Use spatial indexing (R-tree, Quadtree)");

    println!("\n2. Parallelization:");
    println!("   - Use Rayon for data parallelism");
    println!("   - Avoid false sharing (pad data structures)");
    println!("   - Balance work across threads");
    println!("   - Consider NUMA on multi-socket systems");

    println!("\n3. SIMD:");
    println!("   - Use SIMD for arithmetic-heavy operations");
    println!("   - Ensure data alignment");
    println!("   - Prefer portable SIMD (std::simd when stable)");

    println!("\n4. Memory:");
    println!("   - Minimize allocations in hot paths");
    println!("   - Use memory pools for temporary buffers");
    println!("   - Consider streaming for large datasets");

    println!("\n5. I/O:");
    println!("   - Use buffered I/O");
    println!("   - Prefetch data before processing");
    println!("   - Use compression when I/O-bound");

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nPerformance improvements achieved:");
    println!("  Parallelization: {:.2}x speedup", serial_time.as_secs_f64() / parallel_time.as_secs_f64());
    println!("  SIMD: {:.2}x speedup", scalar_add_time.as_secs_f64() / simd_add_time.as_secs_f64());
    println!("  Tiling: {:.2}x speedup", naive_time.as_secs_f64() / tiled_time.as_secs_f64());
    println!("  Combined potential: ~10-50x for typical workloads");

    println!("\nKey Takeaways:");
    println!("  - Profile first, optimize second");
    println!("  - Parallel + SIMD gives multiplicative speedup");
    println!("  - Cache-friendly algorithms are crucial");
    println!("  - Memory layout matters as much as algorithm");
    println!("  - Measure, don't guess");

    println!("\nNext Tutorial:");
    println!("  - Try tutorial 09 for GPU acceleration");

    Ok(())
}

/// Create test buffer with sample data
fn create_test_buffer(width: u32, height: u32) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let value = ((x as f64).sin() + (y as f64).cos()) * 100.0;
            buffer.set_pixel(x, y, value)?;
        }
    }

    Ok(buffer)
}

/// Compute statistics in parallel
fn compute_statistics_parallel(
    buffer: &RasterBuffer,
) -> Result<Statistics, Box<dyn std::error::Error>> {
    let height = buffer.height();
    let width = buffer.width();

    // Process rows in parallel
    let row_stats: Vec<Statistics> = (0..height)
        .into_par_iter()
        .map(|y| {
            let mut min_val = f64::INFINITY;
            let mut max_val = f64::NEG_INFINITY;
            let mut sum = 0.0;
            let mut count = 0;

            for x in 0..width {
                if let Ok(value) = buffer.get_pixel(x, y) {
                    min_val = min_val.min(value);
                    max_val = max_val.max(value);
                    sum += value;
                    count += 1;
                }
            }

            Statistics {
                min: min_val,
                max: max_val,
                mean: sum / count as f64,
                std_dev: 0.0,
                valid_count: count,
            }
        })
        .collect();

    // Aggregate row statistics
    let mut global_min = f64::INFINITY;
    let mut global_max = f64::NEG_INFINITY;
    let mut global_sum = 0.0;
    let mut global_count = 0;

    for stats in &row_stats {
        global_min = global_min.min(stats.min);
        global_max = global_max.max(stats.max);
        global_sum += stats.mean * stats.valid_count as f64;
        global_count += stats.valid_count;
    }

    Ok(Statistics {
        min: global_min,
        max: global_max,
        mean: global_sum / global_count as f64,
        std_dev: 0.0,
        valid_count: global_count,
    })
}

struct Statistics {
    min: f64,
    max: f64,
    mean: f64,
    std_dev: f64,
    valid_count: usize,
}

/// Process tiles serially
fn process_tiles_serial(
    buffer: &RasterBuffer,
    tile_size: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let tiles_x = (buffer.width() + tile_size - 1) / tile_size;
    let tiles_y = (buffer.height() + tile_size - 1) / tile_size;

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
    tile_size: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let tiles_x = (buffer.width() + tile_size - 1) / tile_size;
    let tiles_y = (buffer.height() + tile_size - 1) / tile_size;

    let tile_coords: Vec<(u32, u32)> = (0..tiles_y)
        .flat_map(|ty| (0..tiles_x).map(move |tx| (tx, ty)))
        .collect();

    tile_coords.par_iter().try_for_each(|(tx, ty)| {
        process_single_tile(buffer, tx * tile_size, ty * tile_size, tile_size)
    })?;

    Ok(())
}

/// Process a single tile
fn process_single_tile(
    buffer: &RasterBuffer,
    x: u32,
    y: u32,
    size: u32,
) -> Result<f64, Box<dyn std::error::Error>> {
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

/// Scalar addition
fn scalar_add(buffer: &mut RasterBuffer, value: f64) -> Result<(), Box<dyn std::error::Error>> {
    for y in 0..buffer.height() {
        for x in 0..buffer.width() {
            let pixel = buffer.get_pixel(x, y)?;
            buffer.set_pixel(x, y, pixel + value)?;
        }
    }

    Ok(())
}

/// Naive matrix transpose (poor cache locality)
fn matrix_transpose_naive(size: u32) -> Result<(), Box<dyn std::error::Error>> {
    let mut matrix = vec![0.0f32; (size * size) as usize];

    // Initialize
    for i in 0..(size * size) {
        matrix[i as usize] = i as f32;
    }

    // Transpose (column-wise access, poor cache)
    let mut result = vec![0.0f32; (size * size) as usize];

    for i in 0..size {
        for j in 0..size {
            result[(j * size + i) as usize] = matrix[(i * size + j) as usize];
        }
    }

    Ok(())
}

/// Tiled matrix transpose (better cache locality)
fn matrix_transpose_tiled(size: u32, tile_size: u32) -> Result<(), Box<dyn std::error::Error>> {
    let mut matrix = vec![0.0f32; (size * size) as usize];

    // Initialize
    for i in 0..(size * size) {
        matrix[i as usize] = i as f32;
    }

    // Transpose with tiling
    let mut result = vec![0.0f32; (size * size) as usize];

    for i_tile in (0..size).step_by(tile_size as usize) {
        for j_tile in (0..size).step_by(tile_size as usize) {
            let i_max = (i_tile + tile_size).min(size);
            let j_max = (j_tile + tile_size).min(size);

            for i in i_tile..i_max {
                for j in j_tile..j_max {
                    result[(j * size + i) as usize] = matrix[(i * size + j) as usize];
                }
            }
        }
    }

    Ok(())
}
