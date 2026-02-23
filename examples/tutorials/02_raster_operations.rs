//! Tutorial 02: Raster Operations
//!
//! This tutorial demonstrates common raster operations:
//! - Reprojection between coordinate systems
//! - Resampling (nearest neighbor, bilinear, cubic)
//! - Clipping to regions of interest
//! - Warping and transformations
//!
//! Run with:
//! ```bash
//! cargo run --example 02_raster_operations
//! ```

use oxigdal_algorithms::reproject::{ReprojectOptions, Resampling};
use oxigdal_algorithms::resample::ResampleOptions;
use oxigdal_algorithms::warp::WarpOptions;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::writer::{CompressionType, GeoTiffWriter, GeoTiffWriterOptions};
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_proj::Projection;
use std::env;
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 02: Raster Operations ===\n");

    let temp_dir = env::temp_dir();

    // Step 1: Create a test raster in WGS84 (EPSG:4326)
    println!("Step 1: Creating Test Data (WGS84)");
    println!("-----------------------------------");

    let source_path = temp_dir.join("raster_ops_source.tif");
    create_test_raster(&source_path)?;
    println!("Created source raster: {:?}", source_path);

    // Read the source
    let source = FileDataSource::open(&source_path)?;
    let reader = GeoTiffReader::open(source)?;

    println!("Source raster properties:");
    println!("  Size: {}x{}", reader.width(), reader.height());
    if let Some(epsg) = reader.epsg_code() {
        println!("  EPSG: {}", epsg);
    }
    if let Some(gt) = reader.geo_transform() {
        let bounds = gt.compute_bounds(reader.width(), reader.height());
        println!("  Bounds: [{:.2}, {:.2}, {:.2}, {:.2}]",
                 bounds.min_x(), bounds.min_y(),
                 bounds.max_x(), bounds.max_y());
    }

    // Step 2: Resampling (change resolution)
    println!("\n\nStep 2: Resampling Operations");
    println!("------------------------------");

    // Downsample to half size using different algorithms
    let resampling_methods = vec![
        (Resampling::NearestNeighbor, "nearest_neighbor"),
        (Resampling::Bilinear, "bilinear"),
        (Resampling::Cubic, "cubic"),
        (Resampling::Average, "average"),
    ];

    for (method, name) in resampling_methods {
        println!("\nResampling with {:?}...", method);

        let buffer = reader.read_tile_buffer(0, 0, 0)?;
        let new_width = buffer.width() / 2;
        let new_height = buffer.height() / 2;

        let options = ResampleOptions {
            target_width: new_width,
            target_height: new_height,
            resampling: method,
        };

        let resampled = oxigdal_algorithms::resample::resample(&buffer, &options)?;

        println!("  Original: {}x{}", buffer.width(), buffer.height());
        println!("  Resampled: {}x{}", resampled.width(), resampled.height());

        // Compute statistics to see the effect
        let orig_stats = buffer.compute_statistics()?;
        let resamp_stats = resampled.compute_statistics()?;

        println!("  Original mean: {:.2}, resampled mean: {:.2}",
                 orig_stats.mean, resamp_stats.mean);

        // Write result
        let output_path = temp_dir.join(format!("resampled_{}.tif", name));
        write_buffer(&resampled, &output_path, reader.geo_transform(), reader.epsg_code())?;
        println!("  Saved to: {:?}", output_path);
    }

    // Step 3: Reprojection
    println!("\n\nStep 3: Coordinate System Reprojection");
    println!("---------------------------------------");

    // Reproject from WGS84 (EPSG:4326) to Web Mercator (EPSG:3857)
    println!("Reprojecting from WGS84 (EPSG:4326) to Web Mercator (EPSG:3857)...");

    let buffer = reader.read_tile_buffer(0, 0, 0)?;
    let src_projection = Projection::from_epsg(4326)?;
    let dst_projection = Projection::from_epsg(3857)?;

    let options = ReprojectOptions {
        src_projection: &src_projection,
        dst_projection: &dst_projection,
        src_geo_transform: reader.geo_transform().ok_or("No geotransform")?,
        dst_width: buffer.width(),
        dst_height: buffer.height(),
        resampling: Resampling::Bilinear,
        nodata: None,
    };

    let reprojected = oxigdal_algorithms::reproject::reproject(&buffer, &options)?;

    println!("Reprojection complete!");
    println!("  Size: {}x{}", reprojected.width(), reprojected.height());

    let reproj_stats = reprojected.compute_statistics()?;
    println!("  Statistics - Min: {:.2}, Max: {:.2}, Mean: {:.2}",
             reproj_stats.min, reproj_stats.max, reproj_stats.mean);

    // Calculate new geotransform in Web Mercator
    // For simplicity, we compute approximate bounds
    let orig_gt = reader.geo_transform().ok_or("No geotransform")?;
    let orig_bounds = orig_gt.compute_bounds(reader.width(), reader.height());

    // Transform bounds to Web Mercator
    let (min_x, min_y) = src_projection.transform_point(
        orig_bounds.min_x(), orig_bounds.min_y(), &dst_projection
    )?;
    let (max_x, max_y) = src_projection.transform_point(
        orig_bounds.max_x(), orig_bounds.max_y(), &dst_projection
    )?;

    let new_bounds = BoundingBox::new(min_x, min_y, max_x, max_y)?;
    let new_gt = GeoTransform::from_bounds(&new_bounds, reprojected.width(), reprojected.height())?;

    println!("  New bounds: [{:.2}, {:.2}, {:.2}, {:.2}]",
             new_bounds.min_x(), new_bounds.min_y(),
             new_bounds.max_x(), new_bounds.max_y());

    let reproj_path = temp_dir.join("reprojected_3857.tif");
    write_buffer(&reprojected, &reproj_path, Some(new_gt), Some(3857))?;
    println!("  Saved to: {:?}", reproj_path);

    // Step 4: Clipping to Region of Interest
    println!("\n\nStep 4: Clipping to Region of Interest");
    println!("---------------------------------------");

    let buffer = reader.read_tile_buffer(0, 0, 0)?;

    // Clip to the center quarter of the image
    let clip_x = buffer.width() / 4;
    let clip_y = buffer.height() / 4;
    let clip_width = buffer.width() / 2;
    let clip_height = buffer.height() / 2;

    println!("Clipping to region: x={}, y={}, width={}, height={}",
             clip_x, clip_y, clip_width, clip_height);

    let clipped = buffer.window(clip_x, clip_y, clip_width, clip_height)?;

    println!("Clipped buffer size: {}x{}", clipped.width(), clipped.height());

    let clipped_stats = clipped.compute_statistics()?;
    println!("Clipped statistics:");
    println!("  Min: {:.2}", clipped_stats.min);
    println!("  Max: {:.2}", clipped_stats.max);
    println!("  Mean: {:.2}", clipped_stats.mean);

    // Calculate new geotransform for clipped region
    if let Some(gt) = reader.geo_transform() {
        let clip_gt = gt.offset(clip_x as f64, clip_y as f64)?;
        let clip_path = temp_dir.join("clipped.tif");
        write_buffer(&clipped, &clip_path, Some(clip_gt), reader.epsg_code())?;
        println!("Saved to: {:?}", clip_path);
    }

    // Step 5: Warping with Custom Transform
    println!("\n\nStep 5: Warping and Transformation");
    println!("-----------------------------------");

    let buffer = reader.read_tile_buffer(0, 0, 0)?;

    println!("Applying rotation warp...");

    // Create a rotated geotransform
    let gt = reader.geo_transform().ok_or("No geotransform")?;
    let bounds = gt.compute_bounds(reader.width(), reader.height());

    // Rotate by 15 degrees
    let angle = 15.0_f64.to_radians();
    let cos_a = angle.cos();
    let sin_a = angle.sin();

    let center_x = (bounds.min_x() + bounds.max_x()) / 2.0;
    let center_y = (bounds.min_y() + bounds.max_y()) / 2.0;

    let rotated_gt = GeoTransform::new(
        center_x - (buffer.width() as f64) * gt.pixel_width() * cos_a / 2.0,
        gt.pixel_width() * cos_a,
        -gt.pixel_width() * sin_a,
        center_y + (buffer.height() as f64) * gt.pixel_height() * sin_a / 2.0,
        gt.pixel_height() * sin_a,
        gt.pixel_height() * cos_a,
    )?;

    let warp_options = WarpOptions {
        src_geo_transform: gt,
        dst_geo_transform: rotated_gt,
        dst_width: buffer.width(),
        dst_height: buffer.height(),
        resampling: Resampling::Cubic,
        src_projection: &src_projection,
        dst_projection: &src_projection,
        nodata: None,
    };

    let warped = oxigdal_algorithms::warp::warp(&buffer, &warp_options)?;

    println!("Warp complete!");
    println!("  Output size: {}x{}", warped.width(), warped.height());

    let warped_stats = warped.compute_statistics()?;
    println!("  Statistics - Min: {:.2}, Max: {:.2}, Mean: {:.2}",
             warped_stats.min, warped_stats.max, warped_stats.mean);

    let warped_path = temp_dir.join("warped.tif");
    write_buffer(&warped, &warped_path, Some(rotated_gt), reader.epsg_code())?;
    println!("Saved to: {:?}", warped_path);

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nOperations Covered:");
    println!("  1. Resampling with multiple algorithms");
    println!("  2. Reprojection between coordinate systems");
    println!("  3. Clipping to regions of interest");
    println!("  4. Warping with custom transformations");

    println!("\nKey Points:");
    println!("  - Different resampling methods have different trade-offs");
    println!("  - Reprojection changes both pixel values and coordinates");
    println!("  - Clipping requires updating the geotransform");
    println!("  - Warping can apply complex geometric transformations");

    println!("\nNext Tutorial:");
    println!("  - Try tutorial 03 for vector operations");

    Ok(())
}

/// Create a test raster with gradient pattern
fn create_test_raster(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let width = 512;
    let height = 512;
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    // Create a diagonal gradient with some features
    for y in 0..height {
        for x in 0..width {
            let dx = x as f64 / width as f64;
            let dy = y as f64 / height as f64;

            // Diagonal gradient
            let base = (dx + dy) * 128.0;

            // Add circular features
            let cx = (x as f64 - width as f64 / 4.0) / 50.0;
            let cy = (y as f64 - height as f64 / 4.0) / 50.0;
            let circle1 = 50.0 * (-0.5 * (cx * cx + cy * cy)).exp();

            let cx = (x as f64 - 3.0 * width as f64 / 4.0) / 50.0;
            let cy = (y as f64 - 3.0 * height as f64 / 4.0) / 50.0;
            let circle2 = 50.0 * (-0.5 * (cx * cx + cy * cy)).exp();

            let value = base + circle1 + circle2;
            buffer.set_pixel(x, y, value)?;
        }
    }

    // Create geotransform (small area in WGS84)
    let bbox = BoundingBox::new(-5.0, 45.0, 5.0, 55.0)?;
    let geo_transform = GeoTransform::from_bounds(&bbox, width, height)?;

    write_buffer(&buffer, path, Some(geo_transform), Some(4326))?;

    Ok(())
}

/// Helper function to write a buffer to a GeoTIFF
fn write_buffer(
    buffer: &RasterBuffer,
    path: &std::path::Path,
    geo_transform: Option<GeoTransform>,
    epsg_code: Option<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = GeoTiffWriterOptions {
        geo_transform,
        epsg_code,
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
