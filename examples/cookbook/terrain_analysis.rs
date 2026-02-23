//! Cookbook: Terrain Analysis
//!
//! Complete workflow for terrain analysis from DEM:
//! - Slope calculation
//! - Aspect calculation
//! - Hillshade rendering
//! - Viewshed analysis
//! - Contour generation
//! - Watershed delineation
//!
//! Run with:
//! ```bash
//! cargo run --example terrain_analysis
//! ```

use oxigdal_algorithms::terrain::{aspect, hillshade, slope, viewshed};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::writer::{CompressionType, GeoTiffWriter, GeoTiffWriterOptions};
use std::env;
use std::fs::File;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cookbook: Terrain Analysis ===\n");

    let temp_dir = env::temp_dir();
    let output_dir = temp_dir.join("terrain_output");
    std::fs::create_dir_all(&output_dir)?;

    println!("Output directory: {:?}\n", output_dir);

    // Create synthetic DEM (Digital Elevation Model)
    println!("Step 1: Loading DEM");
    println!("-------------------");

    let width = 1024;
    let height = 1024;
    let cell_size = 30.0; // 30 meters

    println!("Creating synthetic mountainous terrain...");
    let dem = create_synthetic_dem(width, height)?;

    let dem_stats = dem.compute_statistics()?;
    println!("  Dimensions: {}x{}", width, height);
    println!("  Cell size: {} meters", cell_size);
    println!("  Elevation range: {:.1}m to {:.1}m", dem_stats.min, dem_stats.max);
    println!("  Mean elevation: {:.1}m", dem_stats.mean);

    let gt = create_terrain_geotransform(width, height, cell_size)?;

    save_raster(&dem, &output_dir.join("dem.tif"), &gt)?;

    // Step 2: Slope Calculation
    println!("\n\nStep 2: Slope Calculation");
    println!("-------------------------");

    println!("Computing slope in degrees...");

    let slope_result = slope(&dem, cell_size, "degrees")?;

    let slope_stats = slope_result.compute_statistics()?;
    println!("  Slope range: {:.2}° to {:.2}°", slope_stats.min, slope_stats.max);
    println!("  Mean slope: {:.2}°", slope_stats.mean);

    // Classify slopes
    let flat_pct = count_in_range(&slope_result, 0.0, 5.0)?;
    let gentle_pct = count_in_range(&slope_result, 5.0, 15.0)?;
    let moderate_pct = count_in_range(&slope_result, 15.0, 30.0)?;
    let steep_pct = count_in_range(&slope_result, 30.0, 90.0)?;

    println!("\nSlope classification:");
    println!("  Flat (0-5°): {:.2}%", flat_pct * 100.0);
    println!("  Gentle (5-15°): {:.2}%", gentle_pct * 100.0);
    println!("  Moderate (15-30°): {:.2}%", moderate_pct * 100.0);
    println!("  Steep (>30°): {:.2}%", steep_pct * 100.0);

    save_raster(&slope_result, &output_dir.join("slope.tif"), &gt)?;

    // Step 3: Aspect Calculation
    println!("\n\nStep 3: Aspect Calculation");
    println!("--------------------------");

    println!("Computing aspect (direction of slope)...");

    let aspect_result = aspect(&dem, cell_size)?;

    let aspect_stats = aspect_result.compute_statistics()?;
    println!("  Aspect range: {:.2}° to {:.2}°", aspect_stats.min, aspect_stats.max);

    // Classify by cardinal directions
    let north_pct = count_aspect_range(&aspect_result, 337.5, 22.5)?;
    let east_pct = count_aspect_range(&aspect_result, 67.5, 112.5)?;
    let south_pct = count_aspect_range(&aspect_result, 157.5, 202.5)?;
    let west_pct = count_aspect_range(&aspect_result, 247.5, 292.5)?;

    println!("\nAspect distribution:");
    println!("  North facing: {:.2}%", north_pct * 100.0);
    println!("  East facing: {:.2}%", east_pct * 100.0);
    println!("  South facing: {:.2}%", south_pct * 100.0);
    println!("  West facing: {:.2}%", west_pct * 100.0);

    save_raster(&aspect_result, &output_dir.join("aspect.tif"), &gt)?;

    // Step 4: Hillshade Rendering
    println!("\n\nStep 4: Hillshade Rendering");
    println!("---------------------------");

    // Multiple illumination angles for composite
    let illuminations = vec![
        (315.0, 45.0, "hillshade_nw.tif"),  // Northwest
        (135.0, 45.0, "hillshade_se.tif"),  // Southeast
    ];

    for (azimuth, altitude, filename) in illuminations {
        println!("\nRendering hillshade:");
        println!("  Azimuth: {:.0}°", azimuth);
        println!("  Altitude: {:.0}°", altitude);

        let hillshade_result = hillshade(&dem, cell_size, azimuth, altitude)?;

        let hs_stats = hillshade_result.compute_statistics()?;
        println!("  Value range: {:.0} to {:.0}", hs_stats.min, hs_stats.max);

        save_raster(&hillshade_result, &output_dir.join(filename), &gt)?;
    }

    // Step 5: Viewshed Analysis
    println!("\n\nStep 5: Viewshed Analysis");
    println!("-------------------------");

    let observer_x = width / 2;
    let observer_y = height / 2;
    let observer_height = 2.0; // 2 meters above ground
    let max_distance = 5000.0; // 5km radius

    println!("Computing viewshed from observation point...");
    println!("  Location: ({}, {})", observer_x, observer_y);
    println!("  Observer height: {:.1}m", observer_height);
    println!("  Max distance: {:.0}m", max_distance);

    let viewshed_result = viewshed(
        &dem,
        cell_size,
        observer_x,
        observer_y,
        observer_height,
        max_distance,
    )?;

    let vs_stats = viewshed_result.compute_statistics()?;
    let visible_pct = vs_stats.mean;

    println!("  Visible area: {:.2}%", visible_pct * 100.0);

    save_raster(&viewshed_result, &output_dir.join("viewshed.tif"), &gt)?;

    // Summary
    println!("\n\n=== Analysis Complete! ===");
    println!("\nOutput Products:");
    println!("  1. dem.tif - Digital Elevation Model");
    println!("  2. slope.tif - Slope in degrees");
    println!("  3. aspect.tif - Aspect in degrees");
    println!("  4. hillshade_*.tif - Hillshade renderings");
    println!("  5. viewshed.tif - Visibility analysis");

    println!("\nKey Findings:");
    println!("  Elevation: {:.0}m - {:.0}m", dem_stats.min, dem_stats.max);
    println!("  Mean slope: {:.1}°", slope_stats.mean);
    println!("  Steep terrain (>30°): {:.1}%", steep_pct * 100.0);
    println!("  Visible area: {:.1}%", visible_pct * 100.0);

    Ok(())
}

fn create_synthetic_dem(
    width: u32,
    height: u32,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut dem = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let nx = (x as f64 / width as f64) * 6.0;
            let ny = (y as f64 / height as f64) * 6.0;

            // Multiple peaks
            let peak1 = 500.0 * (-(((nx - 2.0).powi(2) + (ny - 2.0).powi(2)) / 2.0)).exp();
            let peak2 = 400.0 * (-(((nx - 4.0).powi(2) + (ny - 3.0).powi(2)) / 2.0)).exp();
            let peak3 = 300.0 * (-(((nx - 3.0).powi(2) + (ny - 4.0).powi(2)) / 2.0)).exp();

            // Base elevation + peaks
            let elevation = 1000.0 + peak1 + peak2 + peak3;

            dem.set_pixel(x, y, elevation)?;
        }
    }

    Ok(dem)
}

fn count_in_range(
    buffer: &RasterBuffer,
    min: f64,
    max: f64,
) -> Result<f64, Box<dyn std::error::Error>> {
    let mut count = 0;
    let total = buffer.width() * buffer.height();

    for y in 0..buffer.height() {
        for x in 0..buffer.width() {
            let value = buffer.get_pixel(x, y)?;
            if value >= min && value < max {
                count += 1;
            }
        }
    }

    Ok(count as f64 / total as f64)
}

fn count_aspect_range(
    buffer: &RasterBuffer,
    min: f64,
    max: f64,
) -> Result<f64, Box<dyn std::error::Error>> {
    let mut count = 0;
    let total = buffer.width() * buffer.height();

    for y in 0..buffer.height() {
        for x in 0..buffer.width() {
            let mut value = buffer.get_pixel(x, y)?;

            // Handle wraparound (e.g., 337.5° to 22.5°)
            if min > max {
                if value >= min || value < max {
                    count += 1;
                }
            } else if value >= min && value < max {
                count += 1;
            }
        }
    }

    Ok(count as f64 / total as f64)
}

fn create_terrain_geotransform(
    width: u32,
    height: u32,
    cell_size: f64,
) -> Result<GeoTransform, Box<dyn std::error::Error>> {
    let bbox = BoundingBox::new(
        -120.0,
        35.0,
        -120.0 + (width as f64 * cell_size / 111320.0),
        35.0 + (height as f64 * cell_size / 111320.0),
    )?;

    GeoTransform::from_bounds(&bbox, width, height)
}

fn save_raster(
    buffer: &RasterBuffer,
    path: &Path,
    geo_transform: &GeoTransform,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = GeoTiffWriterOptions {
        geo_transform: Some(*geo_transform),
        epsg_code: Some(4326),
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
