//! Cookbook: Multi-Sensor Data Fusion
//!
//! Complete workflow for fusing data from multiple remote sensing sources:
//! - Data alignment and resampling
//! - Radiometric normalization
//! - Image fusion techniques (Brovey, PCA, IHS)
//! - Quality assessment of fused products
//!
//! Real-world scenarios:
//! - Pan-sharpening (high-res panchromatic + lower-res multispectral)
//! - Landsat + Sentinel-2 fusion
//! - Radar + optical fusion
//! - Multi-temporal composites
//!
//! Run with:
//! ```bash
//! cargo run --example data_fusion
//! ```

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::writer::{CompressionType, GeoTiffWriter, GeoTiffWriterOptions};
use std::env;
use std::fs::File;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cookbook: Multi-Sensor Data Fusion ===\n");

    let temp_dir = env::temp_dir();
    let output_dir = temp_dir.join("data_fusion_output");
    std::fs::create_dir_all(&output_dir)?;

    println!("Output directory: {:?}\n", output_dir);

    // Scenario: Pan-sharpening Landsat 8 with Panchromatic band
    println!("Scenario: Pan-Sharpening Landsat 8");
    println!("==================================\n");

    let width_ms = 512;  // Multispectral resolution (30m)
    let height_ms = 512;
    let width_pan = 1024; // Panchromatic resolution (15m)
    let height_pan = 1024;

    // Step 1: Load multisensor data
    println!("Step 1: Load Multisensor Data");
    println!("-----------------------------");

    // Simulate Landsat 8 multispectral bands
    let band_red = create_synthetic_band(width_ms, height_ms, 0.3)?;
    let band_green = create_synthetic_band(width_ms, height_ms, 0.35)?;
    let band_blue = create_synthetic_band(width_ms, height_ms, 0.25)?;
    let band_nir = create_synthetic_band(width_ms, height_ms, 0.5)?;

    println!("Landsat 8 Multispectral (30m resolution):");
    println!("  ✓ Band 2 (Blue): {}x{}", width_ms, height_ms);
    println!("  ✓ Band 3 (Green): {}x{}", width_ms, height_ms);
    println!("  ✓ Band 4 (Red): {}x{}", width_ms, height_ms);
    println!("  ✓ Band 5 (NIR): {}x{}", width_ms, height_ms);

    // Simulate panchromatic band
    let band_pan = create_synthetic_panchromatic(width_pan, height_pan)?;

    println!("Landsat 8 Panchromatic (15m resolution):");
    println!("  ✓ Band 8 (Pan): {}x{}", width_pan, height_pan);

    // Step 2: Data preparation
    println!("\n\nStep 2: Data Preparation");
    println!("------------------------");

    // Resample multispectral to panchromatic resolution
    println!("Resampling multispectral to panchromatic resolution...");

    let red_resampled = resample_nearest_neighbor(
        &band_red,
        width_pan,
        height_pan,
    )?;
    let green_resampled = resample_nearest_neighbor(
        &band_green,
        width_pan,
        height_pan,
    )?;
    let blue_resampled = resample_nearest_neighbor(
        &band_blue,
        width_pan,
        height_pan,
    )?;
    let nir_resampled = resample_nearest_neighbor(
        &band_nir,
        width_pan,
        height_pan,
    )?;

    println!("  ✓ Red resampled to {}x{}", width_pan, height_pan);
    println!("  ✓ Green resampled to {}x{}", width_pan, height_pan);
    println!("  ✓ Blue resampled to {}x{}", width_pan, height_pan);
    println!("  ✓ NIR resampled to {}x{}", width_pan, height_pan);

    // Radiometric normalization
    println!("\nPerforming radiometric normalization...");

    let red_norm = normalize_band(&red_resampled)?;
    let green_norm = normalize_band(&green_resampled)?;
    let blue_norm = normalize_band(&blue_resampled)?;
    let nir_norm = normalize_band(&nir_resampled)?;
    let pan_norm = normalize_band(&band_pan)?;

    println!("  ✓ All bands normalized to [0, 1] range");

    // Step 3: Brovey Transform (multiplicative fusion)
    println!("\n\nStep 3: Brovey Transform Pan-Sharpening");
    println!("---------------------------------------");

    println!("Applying Brovey transform...");

    let ms_mean = calculate_mean(&[
        &red_norm,
        &green_norm,
        &blue_norm,
    ])?;

    let red_brovey = brovey_transform(&red_norm, &pan_norm, &ms_mean)?;
    let green_brovey = brovey_transform(&green_norm, &pan_norm, &ms_mean)?;
    let blue_brovey = brovey_transform(&blue_norm, &pan_norm, &ms_mean)?;

    println!("  ✓ Brovey transform completed");

    // Denormalize
    let red_sharpened = denormalize_band(&red_brovey, 0.0, 10000.0)?;
    let green_sharpened = denormalize_band(&green_brovey, 0.0, 10000.0)?;
    let blue_sharpened = denormalize_band(&blue_brovey, 0.0, 10000.0)?;

    let gt_pan = create_geotransform(width_pan, height_pan)?;

    save_raster(&red_sharpened, &output_dir.join("brovey_red.tif"), &gt_pan)?;
    save_raster(&green_sharpened, &output_dir.join("brovey_green.tif"), &gt_pan)?;
    save_raster(&blue_sharpened, &output_dir.join("brovey_blue.tif"), &gt_pan)?;

    // Step 4: Create RGB composite
    println!("\n\nStep 4: Create Pan-Sharpened RGB Composite");
    println!("------------------------------------------");

    let rgb_composite = create_rgb_composite(
        &red_sharpened,
        &green_sharpened,
        &blue_sharpened,
    )?;

    println!("  ✓ RGB composite created ({}x{})", width_pan, height_pan);

    // Step 5: Compare before/after
    println!("\n\nStep 5: Quality Comparison");
    println!("--------------------------");

    let original_rgb = create_rgb_composite(
        &red_resampled,
        &green_resampled,
        &blue_resampled,
    )?;

    let spatial_corr_orig = calculate_spatial_correlation(&original_rgb)?;
    let spatial_corr_sharpened = calculate_spatial_correlation(&rgb_composite)?;

    println!("Original (resampled) spatial correlation: {:.4}", spatial_corr_orig);
    println!("Pan-sharpened spatial correlation: {:.4}", spatial_corr_sharpened);

    // NDVI calculation
    println!("\nVegetation index comparison:");

    let ndvi_original = calculate_ndvi(&nir_resampled, &red_resampled)?;
    let ndvi_sharpened = calculate_ndvi(&nir_resampled, &red_sharpened)?;

    let ndvi_orig_stats = ndvi_original.compute_statistics()?;
    let ndvi_sharp_stats = ndvi_sharpened.compute_statistics()?;

    println!(
        "  Original NDVI range: [{:.4}, {:.4}]",
        ndvi_orig_stats.min, ndvi_orig_stats.max
    );
    println!(
        "  Sharpened NDVI range: [{:.4}, {:.4}]",
        ndvi_sharp_stats.min, ndvi_sharp_stats.max
    );

    // Calculate spectral angle mapper (SAM) to assess spectral distortion
    let sam = calculate_spectral_angle_mapper(
        &[&red_resampled, &green_resampled, &blue_resampled],
        &[&red_sharpened, &green_sharpened, &blue_sharpened],
    )?;

    println!("  Spectral Angle Mapper (SAM): {:.4}°", sam);

    // Step 6: Alternative method - IHS Transform
    println!("\n\nStep 6: IHS (Intensity-Hue-Saturation) Pan-Sharpening");
    println!("----------------------------------------------------");

    // Calculate intensity from RGB
    let intensity = calculate_intensity(&red_norm, &green_norm, &blue_norm)?;

    // IHS fusion: replace intensity with pan
    let red_ihs = ihs_inverse_red(&pan_norm, &green_norm, &blue_norm)?;
    let green_ihs = ihs_inverse_green(&pan_norm, &green_norm, &blue_norm)?;
    let blue_ihs = ihs_inverse_blue(&pan_norm, &green_norm, &blue_norm)?;

    println!("  ✓ IHS pan-sharpening completed");

    let red_ihs_denorm = denormalize_band(&red_ihs, 0.0, 10000.0)?;
    let green_ihs_denorm = denormalize_band(&green_ihs, 0.0, 10000.0)?;
    let blue_ihs_denorm = denormalize_band(&blue_ihs, 0.0, 10000.0)?;

    save_raster(&red_ihs_denorm, &output_dir.join("ihs_red.tif"), &gt_pan)?;
    save_raster(&green_ihs_denorm, &output_dir.join("ihs_green.tif"), &gt_pan)?;
    save_raster(&blue_ihs_denorm, &output_dir.join("ihs_blue.tif"), &gt_pan)?;

    // Step 7: Quality metrics summary
    println!("\n\nStep 7: Quality Metrics Summary");
    println!("--------------------------------");

    println!("Spatial enhancement:");
    println!("  Spatial correlation increase: {:.2}%",
        ((spatial_corr_sharpened / spatial_corr_orig) - 1.0) * 100.0
    );

    println!("\nSpectral preservation:");
    println!("  SAM (Spectral Angle Mapper): {:.4}°", sam);
    println!("  Lower SAM indicates better spectral preservation");

    println!("\nFusion method comparison:");
    println!("  Brovey: Good for multispectral, preserves brightness");
    println!("  IHS: Good for natural color, simple to implement");
    println!("  PCA: Best spectral preservation (not implemented in this example)");

    println!("\nRecommendations:");
    if sam < 0.5 {
        println!("  ✓ Spectral distortion is minimal");
    } else {
        println!("  ⚠ Consider using IHS or PCA method");
    }

    println!("\nOutput files saved to: {:?}", output_dir);

    Ok(())
}

// Helper functions

fn create_synthetic_band(
    width: usize,
    height: usize,
    base_value: f32,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut data = vec![0.0f32; width * height];

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            // Create a synthetic band with spatial variation
            let nx = x as f32 / width as f32;
            let ny = y as f32 / height as f32;

            let pattern = (nx.sin() + ny.cos()) / 2.0;
            data[idx] = (base_value + pattern * 0.2).clamp(0.0, 1.0);
        }
    }

    Ok(RasterBuffer::from_vec(
        data,
        width,
        height,
        RasterDataType::Float32,
    )?)
}

fn create_synthetic_panchromatic(
    width: usize,
    height: usize,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut data = vec![0.0f32; width * height];

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let nx = x as f32 / width as f32;
            let ny = y as f32 / height as f32;

            // Panchromatic: weighted sum of all bands
            let pattern = (nx.sin() * 0.3 + ny.cos() * 0.4 + (nx + ny).sin() * 0.3) / 2.0;
            data[idx] = (0.35 + pattern * 0.3).clamp(0.0, 1.0);
        }
    }

    Ok(RasterBuffer::from_vec(
        data,
        width,
        height,
        RasterDataType::Float32,
    )?)
}

fn resample_nearest_neighbor(
    raster: &RasterBuffer,
    new_width: usize,
    new_height: usize,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;
    let mut resampled = vec![0.0f32; new_width * new_height];

    let scale_x = raster.width() as f32 / new_width as f32;
    let scale_y = raster.height() as f32 / new_height as f32;

    for y in 0..new_height {
        for x in 0..new_width {
            let src_x = ((x as f32 * scale_x) as usize).min(raster.width() - 1);
            let src_y = ((y as f32 * scale_y) as usize).min(raster.height() - 1);

            resampled[y * new_width + x] = data[src_y * raster.width() + src_x];
        }
    }

    Ok(RasterBuffer::from_vec(
        resampled,
        new_width,
        new_height,
        RasterDataType::Float32,
    )?)
}

fn normalize_band(
    raster: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;
    let stats = raster.compute_statistics()?;

    let normalized: Vec<f32> = data
        .iter()
        .map(|&x| {
            if stats.max > stats.min {
                (x - stats.min) / (stats.max - stats.min)
            } else {
                x
            }
        })
        .collect();

    Ok(RasterBuffer::from_vec(
        normalized,
        raster.width(),
        raster.height(),
        RasterDataType::Float32,
    )?)
}

fn denormalize_band(
    raster: &RasterBuffer,
    min: f32,
    max: f32,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;

    let denormalized: Vec<f32> = data
        .iter()
        .map(|&x| x * (max - min) + min)
        .collect();

    Ok(RasterBuffer::from_vec(
        denormalized,
        raster.width(),
        raster.height(),
        RasterDataType::Float32,
    )?)
}

fn brovey_transform(
    band: &RasterBuffer,
    pan: &RasterBuffer,
    ms_mean: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let band_data = band.get_data_as_f32()?;
    let pan_data = pan.get_data_as_f32()?;
    let mean_data = ms_mean.get_data_as_f32()?;

    let result: Vec<f32> = band_data
        .iter()
        .zip(pan_data.iter())
        .zip(mean_data.iter())
        .map(|((&b, &p), &m)| {
            if m > 1e-6 {
                (b * p / m).clamp(0.0, 1.0)
            } else {
                b
            }
        })
        .collect();

    Ok(RasterBuffer::from_vec(
        result,
        band.width(),
        band.height(),
        RasterDataType::Float32,
    )?)
}

fn calculate_mean(
    rasters: &[&RasterBuffer],
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let first_data = rasters[0].get_data_as_f32()?;
    let mut result = vec![0.0f32; first_data.len()];

    for raster in rasters {
        let data = raster.get_data_as_f32()?;
        for (i, &val) in data.iter().enumerate() {
            result[i] += val;
        }
    }

    let n = rasters.len() as f32;
    for val in result.iter_mut() {
        *val /= n;
    }

    Ok(RasterBuffer::from_vec(
        result,
        rasters[0].width(),
        rasters[0].height(),
        RasterDataType::Float32,
    )?)
}

fn create_rgb_composite(
    red: &RasterBuffer,
    green: &RasterBuffer,
    blue: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let r_data = red.get_data_as_f32()?;
    let g_data = green.get_data_as_f32()?;
    let b_data = blue.get_data_as_f32()?;

    // For simplicity, store as R+G+B averaged
    let composite: Vec<f32> = r_data
        .iter()
        .zip(g_data.iter())
        .zip(b_data.iter())
        .map(|((&r, &g), &b)| (r + g + b) / 3.0)
        .collect();

    Ok(RasterBuffer::from_vec(
        composite,
        red.width(),
        red.height(),
        RasterDataType::Float32,
    )?)
}

fn calculate_spatial_correlation(
    raster: &RasterBuffer,
) -> Result<f32, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;
    let stats = raster.compute_statistics()?;

    let mut correlation = 0.0f32;
    let mut count = 0;

    for y in 1..raster.height() - 1 {
        for x in 1..raster.width() - 1 {
            let idx = y * raster.width() + x;

            // Calculate local variance
            let center = data[idx];
            let neighbors = [
                data[(y - 1) * raster.width() + x],
                data[(y + 1) * raster.width() + x],
                data[y * raster.width() + (x - 1)],
                data[y * raster.width() + (x + 1)],
            ];

            let neighbor_mean = neighbors.iter().sum::<f32>() / 4.0;
            let diff = (center - neighbor_mean).abs();

            correlation += diff;
            count += 1;
        }
    }

    Ok((correlation / count as f32) / stats.stdev.max(1e-6))
}

fn calculate_ndvi(
    nir: &RasterBuffer,
    red: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let nir_data = nir.get_data_as_f32()?;
    let red_data = red.get_data_as_f32()?;

    let ndvi: Vec<f32> = nir_data
        .iter()
        .zip(red_data.iter())
        .map(|(&n, &r)| {
            let sum = n + r;
            if sum > 1e-6 {
                (n - r) / sum
            } else {
                0.0
            }
        })
        .collect();

    Ok(RasterBuffer::from_vec(
        ndvi,
        nir.width(),
        nir.height(),
        RasterDataType::Float32,
    )?)
}

fn calculate_intensity(
    red: &RasterBuffer,
    green: &RasterBuffer,
    blue: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let r_data = red.get_data_as_f32()?;
    let g_data = green.get_data_as_f32()?;
    let b_data = blue.get_data_as_f32()?;

    let intensity: Vec<f32> = r_data
        .iter()
        .zip(g_data.iter())
        .zip(b_data.iter())
        .map(|((&r, &g), &b)| (r + g + b) / 3.0)
        .collect();

    Ok(RasterBuffer::from_vec(
        intensity,
        red.width(),
        red.height(),
        RasterDataType::Float32,
    )?)
}

fn ihs_inverse_red(
    intensity: &RasterBuffer,
    green: &RasterBuffer,
    blue: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let i_data = intensity.get_data_as_f32()?;
    let g_data = green.get_data_as_f32()?;
    let b_data = blue.get_data_as_f32()?;

    let red: Vec<f32> = i_data
        .iter()
        .zip(g_data.iter())
        .zip(b_data.iter())
        .map(|((&i, &g), &b)| (i * 1.5 - g * 0.25 - b * 0.25).clamp(0.0, 1.0))
        .collect();

    Ok(RasterBuffer::from_vec(
        red,
        intensity.width(),
        intensity.height(),
        RasterDataType::Float32,
    )?)
}

fn ihs_inverse_green(
    intensity: &RasterBuffer,
    _green: &RasterBuffer,
    blue: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let i_data = intensity.get_data_as_f32()?;
    let b_data = blue.get_data_as_f32()?;

    let green: Vec<f32> = i_data
        .iter()
        .zip(b_data.iter())
        .map(|(&i, &b)| (i * 1.5 - b * 0.75).clamp(0.0, 1.0))
        .collect();

    Ok(RasterBuffer::from_vec(
        green,
        intensity.width(),
        intensity.height(),
        RasterDataType::Float32,
    )?)
}

fn ihs_inverse_blue(
    intensity: &RasterBuffer,
    green: &RasterBuffer,
    _blue: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let i_data = intensity.get_data_as_f32()?;
    let g_data = green.get_data_as_f32()?;

    let blue: Vec<f32> = i_data
        .iter()
        .zip(g_data.iter())
        .map(|(&i, &g)| (i * 1.5 - g * 0.75).clamp(0.0, 1.0))
        .collect();

    Ok(RasterBuffer::from_vec(
        blue,
        intensity.width(),
        intensity.height(),
        RasterDataType::Float32,
    )?)
}

fn calculate_spectral_angle_mapper(
    original_bands: &[&RasterBuffer],
    fused_bands: &[&RasterBuffer],
) -> Result<f32, Box<dyn std::error::Error>> {
    let mut total_sam = 0.0f32;
    let mut count = 0u32;

    let orig_data: Vec<Vec<f32>> = original_bands
        .iter()
        .map(|b| b.get_data_as_f32().map(|d| d.clone()).unwrap_or_default())
        .collect();

    let fused_data: Vec<Vec<f32>> = fused_bands
        .iter()
        .map(|b| b.get_data_as_f32().map(|d| d.clone()).unwrap_or_default())
        .collect();

    for i in 0..orig_data[0].len() {
        let mut orig_vec = vec![0.0; original_bands.len()];
        let mut fused_vec = vec![0.0; fused_bands.len()];

        for (j, band_data) in orig_data.iter().enumerate() {
            orig_vec[j] = band_data[i];
        }

        for (j, band_data) in fused_data.iter().enumerate() {
            fused_vec[j] = band_data[i];
        }

        let dot_product: f32 = orig_vec.iter().zip(&fused_vec).map(|(a, b)| a * b).sum();

        let orig_norm = (orig_vec.iter().map(|x| x * x).sum::<f32>()).sqrt();
        let fused_norm = (fused_vec.iter().map(|x| x * x).sum::<f32>()).sqrt();

        if orig_norm > 1e-6 && fused_norm > 1e-6 {
            let cos_angle = (dot_product / (orig_norm * fused_norm)).clamp(-1.0, 1.0);
            let angle = cos_angle.acos().to_degrees();
            total_sam += angle;
            count += 1;
        }
    }

    Ok(if count > 0 { total_sam / count as f32 } else { 0.0 })
}

fn create_geotransform(
    width: usize,
    height: usize,
) -> Result<GeoTransform, Box<dyn std::error::Error>> {
    let bbox = BoundingBox::new(0.0, 0.0, width as f64 * 15.0, height as f64 * 15.0)?;
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

    println!("  Saved: {}", path.display());
    Ok(())
}
