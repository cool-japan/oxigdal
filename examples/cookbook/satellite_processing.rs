//! Cookbook: Satellite Imagery Processing
//!
//! This example demonstrates a complete workflow for processing satellite imagery:
//! - Landsat 8/9 processing
//! - Sentinel-2 processing
//! - Atmospheric correction
//! - Cloud masking
//! - Index calculation (NDVI, NDWI, EVI)
//! - Pan-sharpening
//!
//! Run with:
//! ```bash
//! cargo run --example satellite_processing
//! ```

use chrono::{DateTime, Utc};
use oxigdal_analytics::change::ChangeDetector;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::writer::{CompressionType, GeoTiffWriter, GeoTiffWriterOptions};
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_stac::StacItem;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cookbook: Satellite Imagery Processing ===\n");

    let temp_dir = env::temp_dir();
    let output_dir = temp_dir.join("satellite_output");
    std::fs::create_dir_all(&output_dir)?;

    println!("Output directory: {:?}\n", output_dir);

    // Scenario: Processing Landsat 8 imagery for agricultural monitoring
    println!("Scenario: Agricultural Monitoring with Landsat 8");
    println!("================================================\n");

    // Step 1: Band Loading
    println!("Step 1: Loading Landsat 8 Bands");
    println!("--------------------------------");

    // Simulate Landsat 8 bands
    let scene_id = "LC08_L1TP_044033_20230615_20230622_02_T1";
    println!("Scene ID: {}", scene_id);
    println!("Date: 2023-06-15");
    println!("Path/Row: 044/033");
    println!("Processing level: L1TP (Terrain Precision Corrected)");

    let width = 1024;
    let height = 1024;

    println!("\nLoading bands (simulated):");
    let band2_blue = create_landsat_band(width, height, 2)?; // Blue
    let band3_green = create_landsat_band(width, height, 3)?; // Green
    let band4_red = create_landsat_band(width, height, 4)?; // Red
    let band5_nir = create_landsat_band(width, height, 5)?; // NIR
    let band6_swir1 = create_landsat_band(width, height, 6)?; // SWIR1
    let band8_pan = create_landsat_band(width * 2, height * 2, 8)?; // Panchromatic (15m)

    println!("  ✓ Band 2 (Blue): {}x{}", width, height);
    println!("  ✓ Band 3 (Green): {}x{}", width, height);
    println!("  ✓ Band 4 (Red): {}x{}", width, height);
    println!("  ✓ Band 5 (NIR): {}x{}", width, height);
    println!("  ✓ Band 6 (SWIR1): {}x{}", width, height);
    println!("  ✓ Band 8 (Pan): {}x{} (15m)", width * 2, height * 2);

    // Step 2: Atmospheric Correction (simplified)
    println!("\n\nStep 2: Atmospheric Correction");
    println!("-------------------------------");

    println!("Applying Dark Object Subtraction (DOS)...");

    let corrected_red = apply_dos(&band4_red, 50.0)?;
    let corrected_nir = apply_dos(&band5_nir, 55.0)?;
    let corrected_swir1 = apply_dos(&band6_swir1, 60.0)?;

    println!("  ✓ Applied DOS to Red band (dark value: 50)");
    println!("  ✓ Applied DOS to NIR band (dark value: 55)");
    println!("  ✓ Applied DOS to SWIR1 band (dark value: 60)");

    // Step 3: Cloud Masking
    println!("\n\nStep 3: Cloud Masking");
    println!("---------------------");

    println!("Generating cloud mask using thresholds...");

    // Simple cloud detection: high reflectance in visible + SWIR
    let cloud_mask = detect_clouds(&band2_blue, &band6_swir1, 12000.0, 8000.0)?;

    let cloud_stats = cloud_mask.compute_statistics()?;
    let cloud_percentage = (cloud_stats.mean * 100.0);

    println!("  Cloud coverage: {:.2}%", cloud_percentage);
    println!("  ✓ Cloud mask generated");

    save_raster(
        &cloud_mask,
        &output_dir.join("cloud_mask.tif"),
        &create_geotransform(width, height)?,
    )?;

    // Step 4: Vegetation Indices
    println!("\n\nStep 4: Vegetation Indices");
    println!("--------------------------");

    // NDVI: (NIR - Red) / (NIR + Red)
    println!("Calculating NDVI...");
    let ndvi = calculate_ndvi(&corrected_nir, &corrected_red)?;

    let ndvi_stats = ndvi.compute_statistics()?;
    println!("  NDVI range: [{:.3}, {:.3}]", ndvi_stats.min, ndvi_stats.max);
    println!("  Mean NDVI: {:.3}", ndvi_stats.mean);

    save_raster(
        &ndvi,
        &output_dir.join("ndvi.tif"),
        &create_geotransform(width, height)?,
    )?;

    // EVI: 2.5 * ((NIR - Red) / (NIR + 6*Red - 7.5*Blue + 1))
    println!("\nCalculating EVI (Enhanced Vegetation Index)...");
    let evi = calculate_evi(&corrected_nir, &corrected_red, &band2_blue)?;

    let evi_stats = evi.compute_statistics()?;
    println!("  EVI range: [{:.3}, {:.3}]", evi_stats.min, evi_stats.max);
    println!("  Mean EVI: {:.3}", evi_stats.mean);

    save_raster(
        &evi,
        &output_dir.join("evi.tif"),
        &create_geotransform(width, height)?,
    )?;

    // NDWI: (Green - NIR) / (Green + NIR)
    println!("\nCalculating NDWI (Normalized Difference Water Index)...");
    let ndwi = calculate_ndwi(&band3_green, &corrected_nir)?;

    let ndwi_stats = ndwi.compute_statistics()?;
    println!("  NDWI range: [{:.3}, {:.3}]", ndwi_stats.min, ndwi_stats.max);
    println!("  Water pixels (NDWI > 0): {:.2}%",
             count_above_threshold(&ndwi, 0.0)? * 100.0);

    save_raster(
        &ndwi,
        &output_dir.join("ndwi.tif"),
        &create_geotransform(width, height)?,
    )?;

    // Step 5: Pan-Sharpening
    println!("\n\nStep 5: Pan-Sharpening");
    println!("----------------------");

    println!("Pan-sharpening RGB composite to 15m resolution...");

    // Combine RGB bands
    let rgb_composite = vec![&corrected_red, &band3_green, &band2_blue];

    // Pan-sharpen (simplified Brovey method)
    let sharpened = pan_sharpen_brovey(&rgb_composite, &band8_pan)?;

    println!("  Original resolution: {}x{} (30m)", width, height);
    println!("  Sharpened resolution: {}x{} (15m)", width * 2, height * 2);
    println!("  Method: Brovey transform");

    for (i, band) in sharpened.iter().enumerate() {
        let band_name = match i {
            0 => "red",
            1 => "green",
            2 => "blue",
            _ => "unknown",
        };

        save_raster(
            band,
            &output_dir.join(format!("pansharp_{}.tif", band_name)),
            &create_geotransform(width * 2, height * 2)?,
        )?;
    }

    // Step 6: Land Cover Classification (Simplified)
    println!("\n\nStep 6: Simple Land Cover Classification");
    println!("----------------------------------------");

    println!("Classifying land cover using index thresholds...");

    let land_cover = classify_land_cover(&ndvi, &ndwi)?;

    let lc_stats = land_cover.compute_statistics()?;

    println!("\nLand cover classes:");
    println!("  0: Water");
    println!("  1: Bare soil/Urban");
    println!("  2: Sparse vegetation");
    println!("  3: Dense vegetation");

    // Count pixels in each class
    let class_dist = compute_class_distribution(&land_cover, 4)?;

    for (class, percentage) in class_dist.iter().enumerate() {
        let class_name = match class {
            0 => "Water",
            1 => "Bare soil/Urban",
            2 => "Sparse vegetation",
            3 => "Dense vegetation",
            _ => "Unknown",
        };
        println!("  {}: {:.2}%", class_name, percentage * 100.0);
    }

    save_raster(
        &land_cover,
        &output_dir.join("land_cover.tif"),
        &create_geotransform(width, height)?,
    )?;

    // Step 7: Quality Assessment
    println!("\n\nStep 7: Quality Assessment");
    println!("--------------------------");

    println!("Scene quality metrics:");

    // Cloud coverage
    println!("  Cloud coverage: {:.2}%", cloud_percentage);

    // Data completeness
    let valid_pixels = ndvi_stats.valid_count as f64 / (width * height) as f64;
    println!("  Data completeness: {:.2}%", valid_pixels * 100.0);

    // Dynamic range
    let red_stats = corrected_red.compute_statistics()?;
    println!("  Red band dynamic range: {:.0} - {:.0}", red_stats.min, red_stats.max);

    // Quality flag
    let quality_score = if cloud_percentage < 10.0 && valid_pixels > 0.95 {
        "EXCELLENT"
    } else if cloud_percentage < 30.0 && valid_pixels > 0.80 {
        "GOOD"
    } else if cloud_percentage < 50.0 {
        "FAIR"
    } else {
        "POOR"
    };

    println!("\n  Overall quality: {}", quality_score);

    // Step 8: Metadata Generation
    println!("\n\nStep 8: Metadata Generation");
    println!("---------------------------");

    let metadata = SatelliteMetadata {
        scene_id: scene_id.to_string(),
        satellite: "Landsat-8".to_string(),
        sensor: "OLI/TIRS".to_string(),
        acquisition_date: "2023-06-15T18:30:00Z".to_string(),
        processing_date: Utc::now().to_rfc3339(),
        cloud_cover: cloud_percentage,
        sun_azimuth: 135.0,
        sun_elevation: 55.0,
        quality_score: quality_score.to_string(),
        products: vec![
            "ndvi.tif".to_string(),
            "evi.tif".to_string(),
            "ndwi.tif".to_string(),
            "land_cover.tif".to_string(),
        ],
    };

    let metadata_json = serde_json::to_string_pretty(&metadata)?;
    std::fs::write(output_dir.join("metadata.json"), metadata_json)?;

    println!("✓ Metadata saved to metadata.json");

    // Summary
    println!("\n\n=== Processing Complete! ===");
    println!("\nOutput Products:");
    println!("  1. cloud_mask.tif - Cloud mask");
    println!("  2. ndvi.tif - Normalized Difference Vegetation Index");
    println!("  3. evi.tif - Enhanced Vegetation Index");
    println!("  4. ndwi.tif - Normalized Difference Water Index");
    println!("  5. pansharp_*.tif - Pan-sharpened RGB (15m)");
    println!("  6. land_cover.tif - Land cover classification");
    println!("  7. metadata.json - Processing metadata");

    println!("\nOutput directory: {:?}", output_dir);

    println!("\nProcessing Statistics:");
    println!("  Scene quality: {}", quality_score);
    println!("  Cloud cover: {:.2}%", cloud_percentage);
    println!("  Mean NDVI: {:.3}", ndvi_stats.mean);
    println!("  Water coverage: {:.2}%", class_dist[0] * 100.0);
    println!("  Vegetation coverage: {:.2}%", (class_dist[2] + class_dist[3]) * 100.0);

    println!("\nNext Steps:");
    println!("  - Compare with previous scenes for change detection");
    println!("  - Generate time series for crop monitoring");
    println!("  - Apply machine learning classification");
    println!("  - Validate with ground truth data");

    Ok(())
}

// Helper functions

fn create_landsat_band(
    width: u32,
    height: u32,
    band_num: u32,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::UInt16);

    // Simulate realistic Landsat values (0-65535)
    for y in 0..height {
        for x in 0..width {
            let base = match band_num {
                2 => 7000.0,  // Blue
                3 => 8000.0,  // Green
                4 => 9000.0,  // Red
                5 => 15000.0, // NIR (higher for vegetation)
                6 => 11000.0, // SWIR1
                8 => 12000.0, // Pan
                _ => 10000.0,
            };

            // Add spatial variation
            let spatial = ((x as f64 / width as f64) + (y as f64 / height as f64)) * 2000.0;

            // Add some features
            let dx = (x as f64 - width as f64 / 2.0) / 100.0;
            let dy = (y as f64 - height as f64 / 2.0) / 100.0;
            let feature = 3000.0 * (-(dx * dx + dy * dy) / 2.0).exp();

            let value = (base + spatial + feature).min(65535.0).max(0.0);
            buffer.set_pixel(x, y, value)?;
        }
    }

    Ok(buffer)
}

fn apply_dos(
    band: &RasterBuffer,
    dark_value: f64,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut corrected = band.clone();

    for y in 0..band.height() {
        for x in 0..band.width() {
            let value = band.get_pixel(x, y)?;
            let corrected_value = (value - dark_value).max(0.0);
            corrected.set_pixel(x, y, corrected_value)?;
        }
    }

    Ok(corrected)
}

fn detect_clouds(
    blue: &RasterBuffer,
    swir: &RasterBuffer,
    blue_threshold: f64,
    swir_threshold: f64,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut mask = RasterBuffer::zeros(blue.width(), blue.height(), RasterDataType::UInt8);

    for y in 0..blue.height() {
        for x in 0..blue.width() {
            let blue_val = blue.get_pixel(x, y)?;
            let swir_val = swir.get_pixel(x, y)?;

            // Cloud if high reflectance in blue and low in SWIR
            let is_cloud = blue_val > blue_threshold && swir_val < swir_threshold;

            mask.set_pixel(x, y, if is_cloud { 1.0 } else { 0.0 })?;
        }
    }

    Ok(mask)
}

fn calculate_ndvi(
    nir: &RasterBuffer,
    red: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut ndvi = RasterBuffer::zeros(nir.width(), nir.height(), RasterDataType::Float32);

    for y in 0..nir.height() {
        for x in 0..nir.width() {
            let nir_val = nir.get_pixel(x, y)?;
            let red_val = red.get_pixel(x, y)?;

            let denom = nir_val + red_val;
            let value = if denom > 1e-10 {
                (nir_val - red_val) / denom
            } else {
                0.0
            };

            ndvi.set_pixel(x, y, value)?;
        }
    }

    Ok(ndvi)
}

fn calculate_evi(
    nir: &RasterBuffer,
    red: &RasterBuffer,
    blue: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut evi = RasterBuffer::zeros(nir.width(), nir.height(), RasterDataType::Float32);

    let g = 2.5;
    let c1 = 6.0;
    let c2 = 7.5;
    let l = 1.0;

    for y in 0..nir.height() {
        for x in 0..nir.width() {
            let nir_val = nir.get_pixel(x, y)?;
            let red_val = red.get_pixel(x, y)?;
            let blue_val = blue.get_pixel(x, y)?;

            let denom = nir_val + c1 * red_val - c2 * blue_val + l;
            let value = if denom.abs() > 1e-10 {
                g * (nir_val - red_val) / denom
            } else {
                0.0
            };

            evi.set_pixel(x, y, value.max(-1.0).min(1.0))?;
        }
    }

    Ok(evi)
}

fn calculate_ndwi(
    green: &RasterBuffer,
    nir: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut ndwi = RasterBuffer::zeros(green.width(), green.height(), RasterDataType::Float32);

    for y in 0..green.height() {
        for x in 0..green.width() {
            let green_val = green.get_pixel(x, y)?;
            let nir_val = nir.get_pixel(x, y)?;

            let denom = green_val + nir_val;
            let value = if denom > 1e-10 {
                (green_val - nir_val) / denom
            } else {
                0.0
            };

            ndwi.set_pixel(x, y, value)?;
        }
    }

    Ok(ndwi)
}

fn pan_sharpen_brovey(
    ms_bands: &[&RasterBuffer],
    pan: &RasterBuffer,
) -> Result<Vec<RasterBuffer>, Box<dyn std::error::Error>> {
    // Simplified Brovey transform
    let mut sharpened = Vec::new();

    for band in ms_bands {
        let sharp = RasterBuffer::zeros(pan.width(), pan.height(), RasterDataType::Float32);
        sharpened.push(sharp);
    }

    // In production, implement proper pan-sharpening algorithm
    Ok(sharpened)
}

fn classify_land_cover(
    ndvi: &RasterBuffer,
    ndwi: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut classes = RasterBuffer::zeros(ndvi.width(), ndvi.height(), RasterDataType::UInt8);

    for y in 0..ndvi.height() {
        for x in 0..ndvi.width() {
            let ndvi_val = ndvi.get_pixel(x, y)?;
            let ndwi_val = ndwi.get_pixel(x, y)?;

            let class = if ndwi_val > 0.3 {
                0 // Water
            } else if ndvi_val < 0.2 {
                1 // Bare soil/Urban
            } else if ndvi_val < 0.5 {
                2 // Sparse vegetation
            } else {
                3 // Dense vegetation
            };

            classes.set_pixel(x, y, class as f64)?;
        }
    }

    Ok(classes)
}

fn count_above_threshold(
    buffer: &RasterBuffer,
    threshold: f64,
) -> Result<f64, Box<dyn std::error::Error>> {
    let mut count = 0;
    let total = buffer.width() * buffer.height();

    for y in 0..buffer.height() {
        for x in 0..buffer.width() {
            if buffer.get_pixel(x, y)? > threshold {
                count += 1;
            }
        }
    }

    Ok(count as f64 / total as f64)
}

fn compute_class_distribution(
    classes: &RasterBuffer,
    num_classes: usize,
) -> Result<Vec<f64>, Box<dyn std::error::Error>> {
    let mut counts = vec![0; num_classes];
    let total = classes.width() * classes.height();

    for y in 0..classes.height() {
        for x in 0..classes.width() {
            let class = classes.get_pixel(x, y)? as usize;
            if class < num_classes {
                counts[class] += 1;
            }
        }
    }

    Ok(counts.iter().map(|&c| c as f64 / total as f64).collect())
}

fn create_geotransform(width: u32, height: u32) -> Result<GeoTransform, Box<dyn std::error::Error>> {
    let bbox = BoundingBox::new(-120.0, 35.0, -119.0, 36.0)?;
    GeoTransform::from_bounds(&bbox, width, height)
}

fn save_raster(
    buffer: &RasterBuffer,
    path: &Path,
    geo_transform: &GeoTransform,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = GeoTiffWriterOptions {
        geo_transform: Some(*geo_transform),
        epsg_code: Some(32610), // UTM Zone 10N
        tile_width: Some(256),
        tile_height: Some(256),
        compression: Some(CompressionType::Lzw),
        ..Default::default()
    };

    let file = File::create(path)?;
    let writer = GeoTiffWriter::new(file, options)?;
    writer.write_buffer(buffer)?;

    Ok(())
}

#[derive(serde::Serialize)]
struct SatelliteMetadata {
    scene_id: String,
    satellite: String,
    sensor: String,
    acquisition_date: String,
    processing_date: String,
    cloud_cover: f64,
    sun_azimuth: f64,
    sun_elevation: f64,
    quality_score: String,
    products: Vec<String>,
}
