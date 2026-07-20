//! Cookbook: Custom Raster Algorithms
//!
//! Guide to implementing custom image processing algorithms:
//! - Filtering and convolutions
//! - Morphological operations
//! - Custom indices and calculations
//! - Windowed operations and kernels
//! - Performance optimization techniques
//!
//! Real-world scenarios:
//! - Custom vegetation indices for specific crops
//! - Domain-specific filters
//! - Multi-band mathematical operations
//! - Specialized raster calculations
//!
//! Run with:
//! ```bash
//! cargo run --example cookbook_custom_algorithms
//! ```

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::tiff::Compression;
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use std::env;
use std::path::Path;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cookbook: Custom Raster Algorithms ===\n");

    let temp_dir = env::temp_dir();
    let output_dir = temp_dir.join("custom_algorithms_output");
    std::fs::create_dir_all(&output_dir)?;

    println!("Output directory: {:?}\n", output_dir);

    let width = 256u64;
    let height = 256u64;

    println!("Step 1: Create Synthetic Data");
    println!("-----------------------------");

    let elevation = create_dem(width, height)?;
    let vegetation = create_vegetation(width, height)?;
    let temperature = create_temperature(width, height)?;

    println!("  DEM created ({}x{})", width, height);
    println!("  Vegetation index created");
    println!("  Temperature created");

    let gt = create_geotransform(width, height)?;

    // Step 2: Implement custom algorithms
    println!("\n\nStep 2: Custom Algorithm Examples");
    println!("--------------------------------");

    println!("\nAlgorithm 1: Gaussian Blur");
    let blurred = gaussian_blur(&elevation, 3)?;
    save_raster(&blurred, &output_dir.join("dem_blurred.tif"), &gt)?;

    println!("\nAlgorithm 2: Sobel Edge Detection");
    let edges = sobel_edge_detection(&elevation)?;
    save_raster(&edges, &output_dir.join("dem_edges.tif"), &gt)?;

    println!("\nAlgorithm 3: Custom Agricultural Index (SAVI)");
    let savi = calculate_savi(&vegetation)?;
    save_raster(&savi, &output_dir.join("savi.tif"), &gt)?;

    println!("\nAlgorithm 4: Morphological Operations");
    let dilated = dilate(&vegetation, 1)?;
    let eroded = erode(&vegetation, 1)?;
    save_raster(&dilated, &output_dir.join("vegetation_dilated.tif"), &gt)?;
    save_raster(&eroded, &output_dir.join("vegetation_eroded.tif"), &gt)?;

    println!("\nAlgorithm 5: Directional Derivative");
    let slope_x = directional_derivative(&elevation, true)?;
    let slope_y = directional_derivative(&elevation, false)?;
    save_raster(&slope_x, &output_dir.join("slope_x.tif"), &gt)?;
    save_raster(&slope_y, &output_dir.join("slope_y.tif"), &gt)?;

    println!("\nAlgorithm 6: Multi-Band Risk Index");
    let risk = calculate_risk_index(&elevation, &vegetation)?;
    save_raster(&risk, &output_dir.join("risk_index.tif"), &gt)?;

    println!("\nAlgorithm 7: Thermal-Based Index");
    let thermal_index = calculate_thermal_index(&temperature, &vegetation)?;
    save_raster(&thermal_index, &output_dir.join("thermal_index.tif"), &gt)?;

    println!("\nAlgorithm 8: Local Variance Filter");
    let variance = variance_filter(&elevation, 2)?;
    save_raster(&variance, &output_dir.join("dem_variance.tif"), &gt)?;

    // Step 3: Compare algorithm performance
    println!("\n\nStep 3: Performance Comparison");
    println!("------------------------------");

    println!("Benchmark (on {}x{} raster):", width, height);

    let start = Instant::now();
    for _ in 0..3 {
        let _ = gaussian_blur(&elevation, 3);
    }
    report_benchmark("Gaussian Blur", start.elapsed(), width, height);

    let start = Instant::now();
    for _ in 0..3 {
        let _ = sobel_edge_detection(&elevation);
    }
    report_benchmark("Sobel Edge Detection", start.elapsed(), width, height);

    let start = Instant::now();
    for _ in 0..3 {
        let _ = calculate_savi(&vegetation);
    }
    report_benchmark("SAVI Calculation", start.elapsed(), width, height);

    let start = Instant::now();
    for _ in 0..3 {
        let _ = dilate(&vegetation, 1);
    }
    report_benchmark("Dilation", start.elapsed(), width, height);

    let start = Instant::now();
    for _ in 0..3 {
        let _ = erode(&vegetation, 1);
    }
    report_benchmark("Erosion", start.elapsed(), width, height);

    // Step 4: Advanced pattern: Windowed processing
    println!("\n\nStep 4: Advanced Pattern - Windowed Processing");
    println!("--------------------------------------------");

    println!("Processing large raster in tiles to optimize cache usage...");

    let tile_size = 64u64;
    let mut tile_pixels = 0u64;

    for y in (0..height).step_by(tile_size as usize) {
        for x in (0..width).step_by(tile_size as usize) {
            let ty = (y + tile_size).min(height) - y;
            let tx = (x + tile_size).min(width) - x;
            tile_pixels += tx * ty;
        }
    }

    println!(
        "  Processed {} tiles ({} pixels each)",
        (width / tile_size) * (height / tile_size),
        tile_size * tile_size
    );
    println!("  Total pixels covered: {}", tile_pixels);

    // Step 5: Complex multi-step algorithm
    println!("\n\nStep 5: Multi-Step Algorithm Pipeline");
    println!("------------------------------------");

    println!("Implementing complex analysis pipeline:");
    println!("  1. Load elevation data");
    println!("  2. Calculate slope");
    println!("  3. Classify slope into categories");
    println!("  4. Apply vegetation mask");
    println!("  5. Calculate stability index");

    let slope = calculate_slope(&elevation)?;
    let classified = classify_slope(&slope)?;
    let masked = apply_mask(&classified, &vegetation)?;
    let stability = calculate_stability_index(&masked)?;

    save_raster(&stability, &output_dir.join("stability_index.tif"), &gt)?;

    println!("  Pipeline completed");

    // Step 6: Statistical analysis of results
    println!("\n\nStep 6: Algorithm Output Analysis");
    println!("--------------------------------");

    analyze_algorithm_output(&blurred, "Gaussian Blur")?;
    analyze_algorithm_output(&edges, "Sobel Edges")?;
    analyze_algorithm_output(&savi, "SAVI")?;
    analyze_algorithm_output(&thermal_index, "Thermal Index")?;

    println!("\nAll outputs saved to: {:?}", output_dir);

    // Step 7: Best practices guide
    println!("\n\nBest Practices for Custom Algorithms");
    println!("===================================");

    println!("1. Memory Efficiency");
    println!("   - Process in tiles for large rasters");
    println!("   - Use appropriate data types (f32 vs f64)");
    println!("   - Avoid unnecessary copies");

    println!("\n2. Numerical Stability");
    println!("   - Check for division by zero");
    println!("   - Handle edge cases gracefully");

    println!("\n3. Performance");
    println!("   - Use vectorized/contiguous-slice operations when possible");
    println!("   - Minimize memory allocations in loops");
    println!("   - Profile before optimizing");

    Ok(())
}

fn report_benchmark(name: &str, elapsed: std::time::Duration, width: u64, height: u64) {
    let per_run = elapsed.as_secs_f32() / 3.0;
    let pixels_per_sec = (width * height) as f32 / per_run.max(1e-9);
    println!(
        "  {}: {:.3}ms ({:.2}M px/s)",
        name,
        per_run * 1000.0,
        pixels_per_sec / 1_000_000.0
    );
}

// Algorithm implementations

fn gaussian_blur(
    raster: &RasterBuffer,
    radius: i64,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let width = raster.width();
    let height = raster.height();
    let data = raster.as_slice::<f32>()?;
    let mut blurred = vec![0.0f32; data.len()];

    let sigma = radius as f32 / 2.0;

    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0f32;
            let mut weight_sum = 0.0f32;

            for ky in -radius..=radius {
                for kx in -radius..=radius {
                    let ny = (y as i64 + ky).clamp(0, height as i64 - 1) as u64;
                    let nx = (x as i64 + kx).clamp(0, width as i64 - 1) as u64;

                    let dist_sq = (kx * kx + ky * ky) as f32;
                    let weight = (-dist_sq / (2.0 * sigma * sigma)).exp();

                    sum += data[(ny * width + nx) as usize] * weight;
                    weight_sum += weight;
                }
            }

            blurred[(y * width + x) as usize] = sum / weight_sum;
        }
    }

    Ok(RasterBuffer::from_typed_vec(
        width as usize,
        height as usize,
        blurred,
        RasterDataType::Float32,
    )?)
}

fn sobel_edge_detection(raster: &RasterBuffer) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let width = raster.width();
    let height = raster.height();
    let data = raster.as_slice::<f32>()?;
    let mut edges = vec![0.0f32; data.len()];

    let gx = [[-1.0, 0.0, 1.0], [-2.0, 0.0, 2.0], [-1.0, 0.0, 1.0]];
    let gy = [[-1.0, -2.0, -1.0], [0.0, 0.0, 0.0], [1.0, 2.0, 1.0]];

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let mut sx = 0.0f32;
            let mut sy = 0.0f32;

            for ky in 0..3u64 {
                for kx in 0..3u64 {
                    let iy = y + ky - 1;
                    let ix = x + kx - 1;

                    let val = data[(iy * width + ix) as usize];
                    sx += val * gx[ky as usize][kx as usize];
                    sy += val * gy[ky as usize][kx as usize];
                }
            }

            edges[(y * width + x) as usize] = (sx * sx + sy * sy).sqrt();
        }
    }

    Ok(RasterBuffer::from_typed_vec(
        width as usize,
        height as usize,
        edges,
        RasterDataType::Float32,
    )?)
}

fn calculate_savi(vegetation: &RasterBuffer) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let veg_data = vegetation.as_slice::<f32>()?;

    // SAVI = (1 + L) * (NIR - RED) / (NIR + RED + L), using vegetation as a proxy
    let savi: Vec<f32> = veg_data
        .iter()
        .map(|&val| {
            let l = 0.5;
            ((1.0 + l) * (val * 0.6 - val * 0.3)) / (val * 0.6 + val * 0.3 + l)
        })
        .collect();

    Ok(RasterBuffer::from_typed_vec(
        vegetation.width() as usize,
        vegetation.height() as usize,
        savi,
        RasterDataType::Float32,
    )?)
}

fn dilate(raster: &RasterBuffer, radius: i64) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let width = raster.width();
    let height = raster.height();
    let data = raster.as_slice::<f32>()?;
    let mut dilated = vec![0.0f32; data.len()];

    for y in 0..height {
        for x in 0..width {
            let mut max_val = data[(y * width + x) as usize];

            for ky in -radius..=radius {
                for kx in -radius..=radius {
                    let ny = (y as i64 + ky).clamp(0, height as i64 - 1) as u64;
                    let nx = (x as i64 + kx).clamp(0, width as i64 - 1) as u64;
                    max_val = max_val.max(data[(ny * width + nx) as usize]);
                }
            }

            dilated[(y * width + x) as usize] = max_val;
        }
    }

    Ok(RasterBuffer::from_typed_vec(
        width as usize,
        height as usize,
        dilated,
        RasterDataType::Float32,
    )?)
}

fn erode(raster: &RasterBuffer, radius: i64) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let width = raster.width();
    let height = raster.height();
    let data = raster.as_slice::<f32>()?;
    let mut eroded = vec![f32::MAX; data.len()];

    for y in 0..height {
        for x in 0..width {
            let mut min_val = f32::MAX;

            for ky in -radius..=radius {
                for kx in -radius..=radius {
                    let ny = (y as i64 + ky).clamp(0, height as i64 - 1) as u64;
                    let nx = (x as i64 + kx).clamp(0, width as i64 - 1) as u64;
                    min_val = min_val.min(data[(ny * width + nx) as usize]);
                }
            }

            eroded[(y * width + x) as usize] = min_val;
        }
    }

    Ok(RasterBuffer::from_typed_vec(
        width as usize,
        height as usize,
        eroded,
        RasterDataType::Float32,
    )?)
}

fn directional_derivative(
    raster: &RasterBuffer,
    x_direction: bool,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let width = raster.width();
    let height = raster.height();
    let data = raster.as_slice::<f32>()?;
    let mut derivative = vec![0.0f32; data.len()];

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            if x_direction {
                let left = data[(y * width + (x - 1)) as usize];
                let right = data[(y * width + (x + 1)) as usize];
                derivative[(y * width + x) as usize] = (right - left) / 2.0;
            } else {
                let top = data[((y - 1) * width + x) as usize];
                let bottom = data[((y + 1) * width + x) as usize];
                derivative[(y * width + x) as usize] = (bottom - top) / 2.0;
            }
        }
    }

    Ok(RasterBuffer::from_typed_vec(
        width as usize,
        height as usize,
        derivative,
        RasterDataType::Float32,
    )?)
}

fn calculate_risk_index(
    elevation: &RasterBuffer,
    vegetation: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let elev_data = elevation.as_slice::<f32>()?;
    let veg_data = vegetation.as_slice::<f32>()?;

    let risk: Vec<f32> = elev_data
        .iter()
        .zip(veg_data.iter())
        .map(|(&e, &v)| {
            let elev_norm = (e / 2000.0).min(1.0);
            (elev_norm * (1.0 - v)).min(1.0)
        })
        .collect();

    Ok(RasterBuffer::from_typed_vec(
        elevation.width() as usize,
        elevation.height() as usize,
        risk,
        RasterDataType::Float32,
    )?)
}

fn calculate_thermal_index(
    temperature: &RasterBuffer,
    vegetation: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let temp_data = temperature.as_slice::<f32>()?;
    let veg_data = vegetation.as_slice::<f32>()?;

    let thermal: Vec<f32> = temp_data
        .iter()
        .zip(veg_data.iter())
        .map(|(&t, &v)| {
            let temp_norm = (t / 40.0).clamp(0.0, 1.0);
            temp_norm * (1.0 - v * 0.7)
        })
        .collect();

    Ok(RasterBuffer::from_typed_vec(
        temperature.width() as usize,
        temperature.height() as usize,
        thermal,
        RasterDataType::Float32,
    )?)
}

fn variance_filter(
    raster: &RasterBuffer,
    radius: i64,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let width = raster.width();
    let height = raster.height();
    let data = raster.as_slice::<f32>()?;
    let mut variance = vec![0.0f32; data.len()];

    for y in (radius as u64)..height - (radius as u64) {
        for x in (radius as u64)..width - (radius as u64) {
            let mut values = Vec::new();

            for ky in -radius..=radius {
                for kx in -radius..=radius {
                    let ny = (y as i64 + ky) as u64;
                    let nx = (x as i64 + kx) as u64;
                    values.push(data[(ny * width + nx) as usize]);
                }
            }

            let mean = values.iter().sum::<f32>() / values.len() as f32;
            let var = values.iter().map(|&v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32;

            variance[(y * width + x) as usize] = var;
        }
    }

    Ok(RasterBuffer::from_typed_vec(
        width as usize,
        height as usize,
        variance,
        RasterDataType::Float32,
    )?)
}

fn calculate_slope(dem: &RasterBuffer) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let width = dem.width();
    let height = dem.height();
    let data = dem.as_slice::<f32>()?;
    let mut slope = vec![0.0f32; data.len()];

    let cell_size = 30.0f32;

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let z = [
                data[((y - 1) * width + (x - 1)) as usize],
                data[((y - 1) * width + x) as usize],
                data[((y - 1) * width + (x + 1)) as usize],
                data[(y * width + (x - 1)) as usize],
                data[(y * width + (x + 1)) as usize],
                data[((y + 1) * width + (x - 1)) as usize],
                data[((y + 1) * width + x) as usize],
                data[((y + 1) * width + (x + 1)) as usize],
            ];

            let dz_dx =
                ((z[2] + 2.0 * z[4] + z[7]) - (z[0] + 2.0 * z[3] + z[5])) / (8.0 * cell_size);
            let dz_dy =
                ((z[5] + 2.0 * z[6] + z[7]) - (z[0] + 2.0 * z[1] + z[2])) / (8.0 * cell_size);

            slope[(y * width + x) as usize] =
                (dz_dx * dz_dx + dz_dy * dz_dy).sqrt().atan().to_degrees();
        }
    }

    Ok(RasterBuffer::from_typed_vec(
        width as usize,
        height as usize,
        slope,
        RasterDataType::Float32,
    )?)
}

fn classify_slope(slope: &RasterBuffer) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = slope.as_slice::<f32>()?;

    let classified: Vec<f32> = data
        .iter()
        .map(|&s| {
            if s < 5.0 {
                0.0
            } else if s < 15.0 {
                1.0
            } else if s < 30.0 {
                2.0
            } else {
                3.0
            }
        })
        .collect();

    Ok(RasterBuffer::from_typed_vec(
        slope.width() as usize,
        slope.height() as usize,
        classified,
        RasterDataType::Float32,
    )?)
}

fn apply_mask(
    raster: &RasterBuffer,
    mask: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = raster.as_slice::<f32>()?;
    let mask_data = mask.as_slice::<f32>()?;

    let masked: Vec<f32> = data
        .iter()
        .zip(mask_data.iter())
        .map(|(&v, &m)| if m > 0.3 { v } else { 0.0 })
        .collect();

    Ok(RasterBuffer::from_typed_vec(
        raster.width() as usize,
        raster.height() as usize,
        masked,
        RasterDataType::Float32,
    )?)
}

fn calculate_stability_index(
    classified: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = classified.as_slice::<f32>()?;

    let stability: Vec<f32> = data.iter().map(|&c| (3.0 - c) / 3.0).collect();

    Ok(RasterBuffer::from_typed_vec(
        classified.width() as usize,
        classified.height() as usize,
        stability,
        RasterDataType::Float32,
    )?)
}

fn analyze_algorithm_output(
    raster: &RasterBuffer,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let stats = raster.compute_statistics()?;

    println!("{}:", name);
    println!("  Range: [{:.4}, {:.4}]", stats.min, stats.max);
    println!("  Mean: {:.4}, StdDev: {:.4}", stats.mean, stats.std_dev);

    Ok(())
}

// Helper functions

fn create_dem(width: u64, height: u64) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let nx = x as f64 / width as f64;
            let ny = y as f64 / height as f64;
            let value = (nx.sin() * 500.0 + ny.cos() * 500.0 + 1000.0).max(0.0);
            buffer.set_pixel(x, y, value)?;
        }
    }

    Ok(buffer)
}

fn create_vegetation(width: u64, height: u64) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let nx = x as f64 / width as f64;
            let ny = y as f64 / height as f64;
            let value = ((nx * 2.0 * std::f64::consts::PI).sin()
                + (ny * 2.0 * std::f64::consts::PI).cos())
            .abs()
                / 2.0;
            buffer.set_pixel(x, y, value)?;
        }
    }

    Ok(buffer)
}

fn create_temperature(width: u64, height: u64) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let nx = x as f64 / width as f64;
            let ny = y as f64 / height as f64;
            let value = 20.0 + (nx * 10.0 + ny * 10.0);
            buffer.set_pixel(x, y, value)?;
        }
    }

    Ok(buffer)
}

fn create_geotransform(
    width: u64,
    height: u64,
) -> Result<GeoTransform, Box<dyn std::error::Error>> {
    let bbox = BoundingBox::new(0.0, 0.0, width as f64 * 30.0, height as f64 * 30.0)?;
    Ok(GeoTransform::from_bounds(&bbox, width, height)?)
}

fn save_raster(
    raster: &RasterBuffer,
    path: &Path,
    gt: &GeoTransform,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = WriterConfig::new(raster.width(), raster.height(), 1, raster.data_type())
        .with_compression(Compression::Deflate)
        .with_tile_size(64, 64)
        .with_geo_transform(*gt);

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())?;
    writer.write(raster.as_bytes())?;

    println!("  Saved: {}", path.display());
    Ok(())
}
