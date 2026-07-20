//! Cookbook: Terrain Analysis
//!
//! Complete workflow for terrain analysis from a DEM:
//! - Slope calculation
//! - Aspect calculation
//! - Hillshade rendering
//! - Viewshed analysis
//!
//! Run with:
//! ```bash
//! cargo run --example cookbook_terrain_analysis
//! ```

use oxigdal_algorithms::raster::{
    HillshadeParams, compute_aspect_degrees, compute_slope_degrees, compute_viewshed, hillshade,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::tiff::Compression;
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use std::env;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cookbook: Terrain Analysis ===\n");

    let temp_dir = env::temp_dir();
    let output_dir = temp_dir.join("terrain_output");
    std::fs::create_dir_all(&output_dir)?;

    println!("Output directory: {:?}\n", output_dir);

    println!("Step 1: Loading DEM");
    println!("-------------------");

    let width = 512u64;
    let height = 512u64;
    let cell_size = 30.0; // 30 meters

    println!("Creating synthetic mountainous terrain...");
    let dem = create_synthetic_dem(width, height)?;

    let dem_stats = dem.compute_statistics()?;
    println!("  Dimensions: {}x{}", width, height);
    println!("  Cell size: {} meters", cell_size);
    println!(
        "  Elevation range: {:.1}m to {:.1}m",
        dem_stats.min, dem_stats.max
    );
    println!("  Mean elevation: {:.1}m", dem_stats.mean);

    let gt = create_terrain_geotransform(width, height, cell_size)?;

    save_raster(&dem, &output_dir.join("dem.tif"), &gt)?;

    // Step 2: Slope Calculation
    println!("\n\nStep 2: Slope Calculation");
    println!("-------------------------");

    println!("Computing slope in degrees...");

    let slope_result = compute_slope_degrees(&dem, cell_size)?;

    let slope_stats = slope_result.compute_statistics()?;
    println!(
        "  Slope range: {:.2} to {:.2} degrees",
        slope_stats.min, slope_stats.max
    );
    println!("  Mean slope: {:.2} degrees", slope_stats.mean);

    let flat_pct = count_in_range(&slope_result, 0.0, 5.0)?;
    let gentle_pct = count_in_range(&slope_result, 5.0, 15.0)?;
    let moderate_pct = count_in_range(&slope_result, 15.0, 30.0)?;
    let steep_pct = count_in_range(&slope_result, 30.0, 90.0)?;

    println!("\nSlope classification:");
    println!("  Flat (0-5 deg): {:.2}%", flat_pct * 100.0);
    println!("  Gentle (5-15 deg): {:.2}%", gentle_pct * 100.0);
    println!("  Moderate (15-30 deg): {:.2}%", moderate_pct * 100.0);
    println!("  Steep (>30 deg): {:.2}%", steep_pct * 100.0);

    save_raster(&slope_result, &output_dir.join("slope.tif"), &gt)?;

    // Step 3: Aspect Calculation
    println!("\n\nStep 3: Aspect Calculation");
    println!("--------------------------");

    println!("Computing aspect (direction of slope)...");

    let aspect_result = compute_aspect_degrees(&dem, cell_size)?;

    let aspect_stats = aspect_result.compute_statistics()?;
    println!(
        "  Aspect range: {:.2} to {:.2} degrees",
        aspect_stats.min, aspect_stats.max
    );

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

    let illuminations = [
        (315.0, 45.0, "hillshade_nw.tif"),
        (135.0, 45.0, "hillshade_se.tif"),
    ];

    for (azimuth, altitude, filename) in illuminations {
        println!("\nRendering hillshade:");
        println!("  Azimuth: {:.0} deg", azimuth);
        println!("  Altitude: {:.0} deg", altitude);

        let params = HillshadeParams {
            azimuth,
            altitude,
            z_factor: 1.0,
            pixel_size: cell_size,
            scale: 255.0,
        };
        let hillshade_result = hillshade(&dem, params)?;

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

    let viewshed_result = compute_viewshed(
        &dem,
        observer_x,
        observer_y,
        observer_height,
        0.0,
        Some(max_distance),
        cell_size,
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
    println!("  Mean slope: {:.1} deg", slope_stats.mean);
    println!("  Steep terrain (>30 deg): {:.1}%", steep_pct * 100.0);
    println!("  Visible area: {:.1}%", visible_pct * 100.0);

    Ok(())
}

fn create_synthetic_dem(
    width: u64,
    height: u64,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut dem = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let nx = (x as f64 / width as f64) * 6.0;
            let ny = (y as f64 / height as f64) * 6.0;

            let peak1 = 500.0 * (-(((nx - 2.0).powi(2) + (ny - 2.0).powi(2)) / 2.0)).exp();
            let peak2 = 400.0 * (-(((nx - 4.0).powi(2) + (ny - 3.0).powi(2)) / 2.0)).exp();
            let peak3 = 300.0 * (-(((nx - 3.0).powi(2) + (ny - 4.0).powi(2)) / 2.0)).exp();

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
            let value = buffer.get_pixel(x, y)?;

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
    width: u64,
    height: u64,
    cell_size: f64,
) -> Result<GeoTransform, Box<dyn std::error::Error>> {
    let bbox = BoundingBox::new(
        -120.0,
        35.0,
        -120.0 + (width as f64 * cell_size / 111_320.0),
        35.0 + (height as f64 * cell_size / 111_320.0),
    )?;

    Ok(GeoTransform::from_bounds(&bbox, width, height)?)
}

fn save_raster(
    buffer: &RasterBuffer,
    path: &Path,
    geo_transform: &GeoTransform,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = WriterConfig::new(buffer.width(), buffer.height(), 1, buffer.data_type())
        .with_compression(Compression::Lzw)
        .with_tile_size(256, 256)
        .with_geo_transform(*geo_transform)
        .with_epsg_code(4326);

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())?;
    writer.write(buffer.as_bytes())?;

    Ok(())
}
