//! Tutorial 02: Raster Operations
//!
//! This tutorial demonstrates common raster operations:
//! - Resampling (nearest neighbor, bilinear, bicubic, lanczos)
//! - Reprojection between coordinate systems (manual per-pixel remap using `oxigdal-proj`)
//! - Clipping to regions of interest
//! - Warping with a custom affine transform (rotation)
//!
//! Run with:
//! ```bash
//! cargo run --example tutorial_02_raster_operations
//! ```

use oxigdal_algorithms::resampling::{Resampler, ResamplingMethod};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_geotiff::tiff::Compression;
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use oxigdal_proj::transform::{Coordinate, Transformer};
use std::env;

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
        println!(
            "  Bounds: [{:.2}, {:.2}, {:.2}, {:.2}]",
            bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y
        );
    }

    // Step 2: Resampling (change resolution)
    println!("\n\nStep 2: Resampling Operations");
    println!("------------------------------");

    let resampling_methods = [
        (ResamplingMethod::Nearest, "nearest"),
        (ResamplingMethod::Bilinear, "bilinear"),
        (ResamplingMethod::Bicubic, "bicubic"),
        (ResamplingMethod::Lanczos, "lanczos"),
    ];

    for (method, name) in resampling_methods {
        println!("\nResampling with {}...", method.name());

        let buffer = reader.read_tile_buffer(0, 0, 0)?;
        let new_width = buffer.width() / 2;
        let new_height = buffer.height() / 2;

        let resampler = Resampler::new(method);
        let resampled = resampler.resample(&buffer, new_width, new_height)?;

        println!("  Original: {}x{}", buffer.width(), buffer.height());
        println!("  Resampled: {}x{}", resampled.width(), resampled.height());

        // Compute statistics to see the effect
        let orig_stats = buffer.compute_statistics()?;
        let resamp_stats = resampled.compute_statistics()?;

        println!(
            "  Original mean: {:.2}, resampled mean: {:.2}",
            orig_stats.mean, resamp_stats.mean
        );

        // Write result
        let output_path = temp_dir.join(format!("resampled_{}.tif", name));
        write_buffer(
            &resampled,
            &output_path,
            reader.geo_transform().copied(),
            reader.epsg_code(),
        )?;
        println!("  Saved to: {:?}", output_path);
    }

    // Step 3: Reprojection
    println!("\n\nStep 3: Coordinate System Reprojection");
    println!("---------------------------------------");

    println!("Reprojecting from WGS84 (EPSG:4326) to Web Mercator (EPSG:3857)...");

    let buffer = reader.read_tile_buffer(0, 0, 0)?;
    let transformer = Transformer::from_epsg(4326, 3857)?;

    let orig_gt = reader.geo_transform().copied().ok_or("No geotransform")?;
    let orig_bounds = orig_gt.compute_bounds(reader.width(), reader.height());

    // Transform bounds to Web Mercator
    let min_corner =
        transformer.transform(&Coordinate::new(orig_bounds.min_x, orig_bounds.min_y))?;
    let max_corner =
        transformer.transform(&Coordinate::new(orig_bounds.max_x, orig_bounds.max_y))?;

    let new_bounds = BoundingBox::new(min_corner.x, min_corner.y, max_corner.x, max_corner.y)?;
    let new_gt = GeoTransform::from_bounds(&new_bounds, buffer.width(), buffer.height())?;

    // Manually remap each destination pixel back into the source raster
    // (inverse transform -> nearest-neighbor sample). This is the same
    // technique a full raster warp would use internally.
    let inverse_transformer = Transformer::from_epsg(3857, 4326)?;
    let mut reprojected = RasterBuffer::zeros(buffer.width(), buffer.height(), buffer.data_type());

    for y in 0..buffer.height() {
        for x in 0..buffer.width() {
            let (world_x, world_y) = new_gt.pixel_to_world(x as f64 + 0.5, y as f64 + 0.5);
            let src_world = inverse_transformer.transform(&Coordinate::new(world_x, world_y))?;

            if let Ok((src_px, src_py)) = orig_gt.world_to_pixel(src_world.x, src_world.y) {
                let src_x = src_px.floor();
                let src_y = src_py.floor();
                if src_x >= 0.0
                    && src_y >= 0.0
                    && (src_x as u64) < buffer.width()
                    && (src_y as u64) < buffer.height()
                {
                    let value = buffer.get_pixel(src_x as u64, src_y as u64)?;
                    reprojected.set_pixel(x, y, value)?;
                }
            }
        }
    }

    println!("Reprojection complete!");
    println!("  Size: {}x{}", reprojected.width(), reprojected.height());

    let reproj_stats = reprojected.compute_statistics()?;
    println!(
        "  Statistics - Min: {:.2}, Max: {:.2}, Mean: {:.2}",
        reproj_stats.min, reproj_stats.max, reproj_stats.mean
    );

    println!(
        "  New bounds: [{:.2}, {:.2}, {:.2}, {:.2}]",
        new_bounds.min_x, new_bounds.min_y, new_bounds.max_x, new_bounds.max_y
    );

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

    println!(
        "Clipping to region: x={}, y={}, width={}, height={}",
        clip_x, clip_y, clip_width, clip_height
    );

    let clipped = buffer.window(clip_x, clip_y, clip_width, clip_height)?;

    println!(
        "Clipped buffer size: {}x{}",
        clipped.width(),
        clipped.height()
    );

    let clipped_stats = clipped.compute_statistics()?;
    println!("Clipped statistics:");
    println!("  Min: {:.2}", clipped_stats.min);
    println!("  Max: {:.2}", clipped_stats.max);
    println!("  Mean: {:.2}", clipped_stats.mean);

    // Calculate new geotransform for clipped region (translate the origin)
    let (clip_origin_x, clip_origin_y) = orig_gt.pixel_to_world(clip_x as f64, clip_y as f64);
    let clip_gt = GeoTransform::new(
        clip_origin_x,
        orig_gt.pixel_width,
        orig_gt.row_rotation,
        clip_origin_y,
        orig_gt.col_rotation,
        orig_gt.pixel_height,
    );
    let clip_path = temp_dir.join("clipped.tif");
    write_buffer(&clipped, &clip_path, Some(clip_gt), reader.epsg_code())?;
    println!("Saved to: {:?}", clip_path);

    // Step 5: Warping with Custom Transform
    println!("\n\nStep 5: Warping and Transformation");
    println!("-----------------------------------");

    let buffer = reader.read_tile_buffer(0, 0, 0)?;

    println!("Applying rotation warp...");

    let gt = reader.geo_transform().copied().ok_or("No geotransform")?;
    let bounds = gt.compute_bounds(reader.width(), reader.height());

    // Rotate by 15 degrees
    let angle = 15.0_f64.to_radians();
    let cos_a = angle.cos();
    let sin_a = angle.sin();

    let center_x = (bounds.min_x + bounds.max_x) / 2.0;
    let center_y = (bounds.min_y + bounds.max_y) / 2.0;

    let rotated_gt = GeoTransform::new(
        center_x - (buffer.width() as f64) * gt.pixel_width * cos_a / 2.0,
        gt.pixel_width * cos_a,
        -gt.pixel_width * sin_a,
        center_y + (buffer.height() as f64) * gt.pixel_height * sin_a / 2.0,
        gt.pixel_height * sin_a,
        gt.pixel_height * cos_a,
    );

    // Manually warp: for every destination pixel, invert the rotated transform
    // and nearest-neighbor sample from the source buffer.
    let mut warped = RasterBuffer::zeros(buffer.width(), buffer.height(), buffer.data_type());
    for y in 0..buffer.height() {
        for x in 0..buffer.width() {
            let (world_x, world_y) = rotated_gt.pixel_to_world(x as f64 + 0.5, y as f64 + 0.5);
            if let Ok((src_px, src_py)) = gt.world_to_pixel(world_x, world_y) {
                let src_x = src_px.floor();
                let src_y = src_py.floor();
                if src_x >= 0.0
                    && src_y >= 0.0
                    && (src_x as u64) < buffer.width()
                    && (src_y as u64) < buffer.height()
                {
                    let value = buffer.get_pixel(src_x as u64, src_y as u64)?;
                    warped.set_pixel(x, y, value)?;
                }
            }
        }
    }

    println!("Warp complete!");
    println!("  Output size: {}x{}", warped.width(), warped.height());

    let warped_stats = warped.compute_statistics()?;
    println!(
        "  Statistics - Min: {:.2}, Max: {:.2}, Mean: {:.2}",
        warped_stats.min, warped_stats.max, warped_stats.mean
    );

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
    let mut config = WriterConfig::new(buffer.width(), buffer.height(), 1, buffer.data_type())
        .with_compression(Compression::Lzw)
        .with_tile_size(256, 256);

    if let Some(gt) = geo_transform {
        config = config.with_geo_transform(gt);
    }
    if let Some(epsg) = epsg_code {
        config = config.with_epsg_code(epsg);
    }

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())?;
    writer.write(buffer.as_bytes())?;

    Ok(())
}
