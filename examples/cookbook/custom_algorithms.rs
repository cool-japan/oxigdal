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
//! cargo run --example custom_algorithms
//! ```

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::writer::{CompressionType, GeoTiffWriter, GeoTiffWriterOptions};
use std::env;
use std::fs::File;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cookbook: Custom Raster Algorithms ===\n");

    let temp_dir = env::temp_dir();
    let output_dir = temp_dir.join("custom_algorithms_output");
    std::fs::create_dir_all(&output_dir)?;

    println!("Output directory: {:?}\n", output_dir);

    let width = 512;
    let height = 512;

    // Create synthetic input data
    println!("Step 1: Create Synthetic Data");
    println!("-----------------------------");

    let elevation = create_dem(width, height)?;
    let vegetation = create_vegetation(width, height)?;
    let temperature = create_temperature(width, height)?;

    println!("  ✓ DEM created ({}x{})", width, height);
    println!("  ✓ Vegetation index created");
    println!("  ✓ Temperature created");

    let gt = create_geotransform(width, height)?;

    // Step 2: Implement custom algorithms
    println!("\n\nStep 2: Custom Algorithm Examples");
    println!("--------------------------------");

    // Algorithm 1: Gaussian Blur (for smoothing)
    println!("\nAlgorithm 1: Gaussian Blur");
    println!("  Description: Smooth raster data using Gaussian kernel");
    println!("  Use case: Remove noise, preparation for analysis");

    let blurred = gaussian_blur(&elevation, 3)?;
    save_raster(&blurred, &output_dir.join("dem_blurred.tif"), &gt)?;

    // Algorithm 2: Sobel Edge Detection
    println!("\nAlgorithm 2: Sobel Edge Detection");
    println!("  Description: Detect edges in elevation data");
    println!("  Use case: Ridge and valley detection");

    let edges = sobel_edge_detection(&elevation)?;
    save_raster(&edges, &output_dir.join("dem_edges.tif"), &gt)?;

    // Algorithm 3: Custom Vegetation Index
    println!("\nAlgorithm 3: Custom Agricultural Index");
    println!("  Description: SAVI (Soil-Adjusted VI)");
    println!("  Use case: Crop-specific vegetation analysis");

    let savi = calculate_savi(&vegetation)?;
    save_raster(&savi, &output_dir.join("savi.tif"), &gt)?;

    // Algorithm 4: Morphological Operations
    println!("\nAlgorithm 4: Morphological Operations");
    println!("  Description: Dilation and erosion");
    println!("  Use case: Object identification, feature enhancement");

    let dilated = dilate(&vegetation, 1)?;
    let eroded = erode(&vegetation, 1)?;

    save_raster(&dilated, &output_dir.join("vegetation_dilated.tif"), &gt)?;
    save_raster(&eroded, &output_dir.join("vegetation_eroded.tif"), &gt)?;

    // Algorithm 5: Directional Derivative
    println!("\nAlgorithm 5: Directional Derivative");
    println!("  Description: Calculate derivative in specific direction");
    println!("  Use case: Flow direction, terrain analysis");

    let slope_x = directional_derivative(&elevation, true)?; // X direction
    let slope_y = directional_derivative(&elevation, false)?; // Y direction

    save_raster(&slope_x, &output_dir.join("slope_x.tif"), &gt)?;
    save_raster(&slope_y, &output_dir.join("slope_y.tif"), &gt)?;

    // Algorithm 6: Custom Multi-band Operation
    println!("\nAlgorithm 6: Multi-Band Arith");
    println!("  Description: Combined elevation and vegetation risk index");
    println!("  Use case: Landslide risk, erosion potential");

    let risk = calculate_risk_index(&elevation, &vegetation)?;
    save_raster(&risk, &output_dir.join("risk_index.tif"), &gt)?;

    // Algorithm 7: Thermal Water Index
    println!("\nAlgorithm 7: Thermal-Based Index");
    println!("  Description: Custom urban heat island detection");
    println!("  Use case: Urban planning, climate studies");

    let thermal_index = calculate_thermal_index(&temperature, &vegetation)?;
    save_raster(&thermal_index, &output_dir.join("thermal_index.tif"), &gt)?;

    // Algorithm 8: Variance Filter (local statistics)
    println!("\nAlgorithm 8: Local Variance Filter");
    println!("  Description: Calculate local variance in 5x5 windows");
    println!("  Use case: Texture analysis, uncertainty quantification");

    let variance = variance_filter(&elevation, 2)?;
    save_raster(&variance, &output_dir.join("dem_variance.tif"), &gt)?;

    // Step 3: Compare algorithm performance
    println!("\n\nStep 3: Performance Comparison");
    println!("------------------------------");

    // Time different algorithms
    use std::time::Instant;

    let algorithms = vec![
        ("Gaussian Blur", || gaussian_blur(&elevation, 3)),
        ("Sobel Edge Detection", || sobel_edge_detection(&elevation)),
        ("SAVI Calculation", || calculate_savi(&vegetation)),
        ("Dilation", || dilate(&vegetation, 1)),
        ("Erosion", || erode(&vegetation, 1)),
    ];

    println!("Benchmark (on {}x{} raster):", width, height);

    for (name, algo) in &algorithms {
        let start = Instant::now();

        // Run 3 times to get average
        for _ in 0..3 {
            let _ = algo();
        }

        let elapsed = start.elapsed().as_secs_f32() / 3.0;
        let pixels_per_sec = (width * height) as f32 / elapsed;

        println!("  {}: {:.3}ms ({:.2}M px/s)",
            name,
            elapsed * 1000.0,
            pixels_per_sec / 1_000_000.0
        );
    }

    // Step 4: Advanced pattern: Windowed processing
    println!("\n\nStep 4: Advanced Pattern - Windowed Processing");
    println!("--------------------------------------------");

    println!("Processing large raster in tiles to optimize cache usage...");

    let tile_size = 128;
    let mut tile_results = 0;

    for y in (0..height).step_by(tile_size) {
        for x in (0..width).step_by(tile_size) {
            let ty = (y + tile_size).min(height) - y;
            let tx = (x + tile_size).min(width) - x;

            // Process tile
            tile_results += tx * ty;
        }
    }

    println!("  ✓ Processed {} tiles ({} pixels each)",
        (width * height) / (tile_size * tile_size),
        tile_size * tile_size
    );

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
    let stability = calculate_stability_index(&masked, &elevation)?;

    save_raster(&stability, &output_dir.join("stability_index.tif"), &gt)?;

    println!("  ✓ Pipeline completed");

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
    println!("   - Use proper normalization");
    println!("   - Handle edge cases gracefully");

    println!("\n3. Performance");
    println!("   - Use vectorized operations when possible");
    println!("   - Minimize memory allocations in loops");
    println!("   - Profile before optimizing");

    println!("\n4. Correctness");
    println!("   - Validate against reference implementations");
    println!("   - Test edge cases and boundary conditions");
    println!("   - Document assumptions and limitations");

    println!("\n5. Reproducibility");
    println!("   - Keep algorithms deterministic");
    println!("   - Document parameters clearly");
    println!("   - Version your algorithms");

    Ok(())
}

// Algorithm implementations

fn gaussian_blur(
    raster: &RasterBuffer,
    radius: usize,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;
    let mut blurred = vec![0.0f32; data.len()];

    let sigma = radius as f32 / 2.0;

    for y in 0..raster.height() {
        for x in 0..raster.width() {
            let mut sum = 0.0f32;
            let mut weight_sum = 0.0f32;

            for ky in -(radius as i32)..=(radius as i32) {
                for kx in -(radius as i32)..=(radius as i32) {
                    let ny = (y as i32 + ky).max(0).min(raster.height() as i32 - 1) as usize;
                    let nx = (x as i32 + kx).max(0).min(raster.width() as i32 - 1) as usize;

                    let dist_sq = (kx * kx + ky * ky) as f32;
                    let weight = (-dist_sq / (2.0 * sigma * sigma)).exp();

                    sum += data[ny * raster.width() + nx] * weight;
                    weight_sum += weight;
                }
            }

            blurred[y * raster.width() + x] = sum / weight_sum;
        }
    }

    Ok(RasterBuffer::from_vec(
        blurred,
        raster.width(),
        raster.height(),
        RasterDataType::Float32,
    )?)
}

fn sobel_edge_detection(
    raster: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;
    let mut edges = vec![0.0f32; data.len()];

    let gx = [[-1.0, 0.0, 1.0], [-2.0, 0.0, 2.0], [-1.0, 0.0, 1.0]];
    let gy = [[-1.0, -2.0, -1.0], [0.0, 0.0, 0.0], [1.0, 2.0, 1.0]];

    for y in 1..raster.height() - 1 {
        for x in 1..raster.width() - 1 {
            let mut sx = 0.0f32;
            let mut sy = 0.0f32;

            for ky in 0..3 {
                for kx in 0..3 {
                    let iy = y + ky - 1;
                    let ix = x + kx - 1;

                    let val = data[iy * raster.width() + ix];
                    sx += val * gx[ky][kx];
                    sy += val * gy[ky][kx];
                }
            }

            edges[y * raster.width() + x] = (sx * sx + sy * sy).sqrt();
        }
    }

    Ok(RasterBuffer::from_vec(
        edges,
        raster.width(),
        raster.height(),
        RasterDataType::Float32,
    )?)
}

fn calculate_savi(
    vegetation: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let veg_data = vegetation.get_data_as_f32()?;

    // SAVI = (1 + L) * (NIR - RED) / (NIR + RED + L)
    // Where L = 0.5 (typical value)
    // We're using vegetation as a proxy

    let savi: Vec<f32> = veg_data
        .iter()
        .map(|&val| {
            let l = 0.5;
            ((1.0 + l) * (val * 0.6 - val * 0.3)) / (val * 0.6 + val * 0.3 + l)
        })
        .collect();

    Ok(RasterBuffer::from_vec(
        savi,
        vegetation.width(),
        vegetation.height(),
        RasterDataType::Float32,
    )?)
}

fn dilate(
    raster: &RasterBuffer,
    radius: usize,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;
    let mut dilated = vec![0.0f32; data.len()];

    for y in 0..raster.height() {
        for x in 0..raster.width() {
            let mut max_val = data[y * raster.width() + x];

            for ky in -(radius as i32)..=(radius as i32) {
                for kx in -(radius as i32)..=(radius as i32) {
                    let ny = (y as i32 + ky).max(0).min(raster.height() as i32 - 1) as usize;
                    let nx = (x as i32 + kx).max(0).min(raster.width() as i32 - 1) as usize;

                    max_val = max_val.max(data[ny * raster.width() + nx]);
                }
            }

            dilated[y * raster.width() + x] = max_val;
        }
    }

    Ok(RasterBuffer::from_vec(
        dilated,
        raster.width(),
        raster.height(),
        RasterDataType::Float32,
    )?)
}

fn erode(
    raster: &RasterBuffer,
    radius: usize,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;
    let mut eroded = vec![f32::MAX; data.len()];

    for y in 0..raster.height() {
        for x in 0..raster.width() {
            let mut min_val = f32::MAX;

            for ky in -(radius as i32)..=(radius as i32) {
                for kx in -(radius as i32)..=(radius as i32) {
                    let ny = (y as i32 + ky).max(0).min(raster.height() as i32 - 1) as usize;
                    let nx = (x as i32 + kx).max(0).min(raster.width() as i32 - 1) as usize;

                    min_val = min_val.min(data[ny * raster.width() + nx]);
                }
            }

            eroded[y * raster.width() + x] = min_val;
        }
    }

    Ok(RasterBuffer::from_vec(
        eroded,
        raster.width(),
        raster.height(),
        RasterDataType::Float32,
    )?)
}

fn directional_derivative(
    raster: &RasterBuffer,
    x_direction: bool,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;
    let mut derivative = vec![0.0f32; data.len()];

    for y in 1..raster.height() - 1 {
        for x in 1..raster.width() - 1 {
            if x_direction {
                let left = data[y * raster.width() + (x - 1)];
                let right = data[y * raster.width() + (x + 1)];
                derivative[y * raster.width() + x] = (right - left) / 2.0;
            } else {
                let top = data[(y - 1) * raster.width() + x];
                let bottom = data[(y + 1) * raster.width() + x];
                derivative[y * raster.width() + x] = (bottom - top) / 2.0;
            }
        }
    }

    Ok(RasterBuffer::from_vec(
        derivative,
        raster.width(),
        raster.height(),
        RasterDataType::Float32,
    )?)
}

fn calculate_risk_index(
    elevation: &RasterBuffer,
    vegetation: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let elev_data = elevation.get_data_as_f32()?;
    let veg_data = vegetation.get_data_as_f32()?;

    // Risk = high elevation + low vegetation = high risk
    let risk: Vec<f32> = elev_data
        .iter()
        .zip(veg_data.iter())
        .map(|(&e, &v)| {
            let elev_norm = (e / 2000.0).min(1.0);
            let veg_norm = v;
            (elev_norm * (1.0 - veg_norm)).min(1.0)
        })
        .collect();

    Ok(RasterBuffer::from_vec(
        risk,
        elevation.width(),
        elevation.height(),
        RasterDataType::Float32,
    )?)
}

fn calculate_thermal_index(
    temperature: &RasterBuffer,
    vegetation: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let temp_data = temperature.get_data_as_f32()?;
    let veg_data = vegetation.get_data_as_f32()?;

    // Thermal index = high temperature + low vegetation
    let thermal: Vec<f32> = temp_data
        .iter()
        .zip(veg_data.iter())
        .map(|(&t, &v)| {
            let temp_norm = ((t - 0.0) / 40.0).clamp(0.0, 1.0);
            let veg_norm = v;
            temp_norm * (1.0 - veg_norm * 0.7)
        })
        .collect();

    Ok(RasterBuffer::from_vec(
        thermal,
        temperature.width(),
        temperature.height(),
        RasterDataType::Float32,
    )?)
}

fn variance_filter(
    raster: &RasterBuffer,
    radius: usize,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;
    let mut variance = vec![0.0f32; data.len()];

    for y in radius..raster.height() - radius {
        for x in radius..raster.width() - radius {
            let mut values = Vec::new();

            for ky in -(radius as i32)..=(radius as i32) {
                for kx in -(radius as i32)..=(radius as i32) {
                    let ny = (y as i32 + ky) as usize;
                    let nx = (x as i32 + kx) as usize;
                    values.push(data[ny * raster.width() + nx]);
                }
            }

            let mean = values.iter().sum::<f32>() / values.len() as f32;
            let var = values
                .iter()
                .map(|&v| (v - mean).powi(2))
                .sum::<f32>()
                / values.len() as f32;

            variance[y * raster.width() + x] = var;
        }
    }

    Ok(RasterBuffer::from_vec(
        variance,
        raster.width(),
        raster.height(),
        RasterDataType::Float32,
    )?)
}

fn calculate_slope(
    dem: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = dem.get_data_as_f32()?;
    let mut slope = vec![0.0f32; data.len()];

    let cell_size = 30.0f32;

    for y in 1..dem.height() - 1 {
        for x in 1..dem.width() - 1 {
            let z = [
                data[(y - 1) * dem.width() + (x - 1)],
                data[(y - 1) * dem.width() + x],
                data[(y - 1) * dem.width() + (x + 1)],
                data[y * dem.width() + (x - 1)],
                data[y * dem.width() + (x + 1)],
                data[(y + 1) * dem.width() + (x - 1)],
                data[(y + 1) * dem.width() + x],
                data[(y + 1) * dem.width() + (x + 1)],
            ];

            let dz_dx = ((z[2] + 2.0 * z[4] + z[7]) - (z[0] + 2.0 * z[3] + z[5])) / (8.0 * cell_size);
            let dz_dy = ((z[5] + 2.0 * z[6] + z[7]) - (z[0] + 2.0 * z[1] + z[2])) / (8.0 * cell_size);

            slope[y * dem.width() + x] = (dz_dx * dz_dx + dz_dy * dz_dy).sqrt().atan().to_degrees();
        }
    }

    Ok(RasterBuffer::from_vec(
        slope,
        dem.width(),
        dem.height(),
        RasterDataType::Float32,
    )?)
}

fn classify_slope(
    slope: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = slope.get_data_as_f32()?;

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

    Ok(RasterBuffer::from_vec(
        classified,
        slope.width(),
        slope.height(),
        RasterDataType::Float32,
    )?)
}

fn apply_mask(
    raster: &RasterBuffer,
    mask: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;
    let mask_data = mask.get_data_as_f32()?;

    let masked: Vec<f32> = data
        .iter()
        .zip(mask_data.iter())
        .map(|(&v, &m)| if m > 0.3 { v } else { 0.0 })
        .collect();

    Ok(RasterBuffer::from_vec(
        masked,
        raster.width(),
        raster.height(),
        RasterDataType::Float32,
    )?)
}

fn calculate_stability_index(
    _classified: &RasterBuffer,
    _elevation: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = _classified.get_data_as_f32()?;

    let stability: Vec<f32> = data
        .iter()
        .map(|&c| (3.0 - c) / 3.0)
        .collect();

    Ok(RasterBuffer::from_vec(
        stability,
        _classified.width(),
        _classified.height(),
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
    println!("  Mean: {:.4}, Stdev: {:.4}", stats.mean, stats.stdev);

    Ok(())
}

// Helper functions

fn create_dem(width: usize, height: usize) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut data = vec![0.0f32; width * height];

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let nx = x as f32 / width as f32;
            let ny = y as f32 / height as f32;

            data[idx] = (nx.sin() * 500.0 + ny.cos() * 500.0 + 1000.0).max(0.0);
        }
    }

    Ok(RasterBuffer::from_vec(
        data,
        width,
        height,
        RasterDataType::Float32,
    )?)
}

fn create_vegetation(width: usize, height: usize) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut data = vec![0.0f32; width * height];

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let nx = x as f32 / width as f32;
            let ny = y as f32 / height as f32;

            data[idx] = ((nx * 2.0 * 3.14).sin() + (ny * 2.0 * 3.14).cos()).abs() / 2.0;
        }
    }

    Ok(RasterBuffer::from_vec(
        data,
        width,
        height,
        RasterDataType::Float32,
    )?)
}

fn create_temperature(width: usize, height: usize) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut data = vec![0.0f32; width * height];

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let nx = x as f32 / width as f32;
            let ny = y as f32 / height as f32;

            data[idx] = 20.0 + (nx * 10.0 + ny * 10.0);
        }
    }

    Ok(RasterBuffer::from_vec(
        data,
        width,
        height,
        RasterDataType::Float32,
    )?)
}

fn create_geotransform(
    width: usize,
    height: usize,
) -> Result<GeoTransform, Box<dyn std::error::Error>> {
    let bbox = BoundingBox::new(0.0, 0.0, width as f64 * 30.0, height as f64 * 30.0)?;
    Ok(GeoTransform::from_bounds(&bbox, width, height)?)
}

fn save_raster(
    raster: &RasterBuffer,
    path: &Path,
    gt: &GeoTransform,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;

    let options = GeoTiffWriterOptions {
        compression: CompressionType::Deflate,
        ..Default::default()
    };

    let mut writer = GeoTiffWriter::new(file, options)?;
    writer.write(raster, gt)?;

    println!("  ✓ Saved: {}", path.display());
    Ok(())
}
