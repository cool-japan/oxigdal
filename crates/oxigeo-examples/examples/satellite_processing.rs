//! Real-world Satellite Processing Example
//!
//! This example demonstrates a satellite data processing pipeline built on the
//! real `oxigeo-sensors` crate:
//! - Radiometric calibration (DN -> TOA radiance -> reflectance)
//! - Atmospheric correction (Dark Object Subtraction)
//! - Spectral indices (NDVI, NDWI, EVI, SAVI)
//! - Pan-sharpening (Brovey transform)
//! - Exporting results as tiled, compressed GeoTIFFs
//!
//! Run with:
//! ```bash
//! cargo run --example satellite_processing
//! ```

use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigeo_geotiff::tiff::Compression;
use oxigeo_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use oxigeo_sensors::indices::{evi, ndvi, ndwi, savi};
use oxigeo_sensors::pan_sharpening::{BroveyTransform, PanSharpening};
use oxigeo_sensors::radiometry::{
    AtmosphericCorrection, DarkObjectSubtraction, RadiometricCalibration,
};
use oxigeo_sensors::sensors::landsat::landsat8_oli_tirs;
use scirs2_core::ndarray::Array2;
use std::path::Path;
use tracing::info;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("satellite_processing=info")
        .init();

    info!("Starting satellite data processing pipeline");

    let output_dir = std::env::temp_dir().join("satellite_processing_output");
    std::fs::create_dir_all(&output_dir)?;

    // Step 1: Sensor metadata
    info!("Step 1: Loading sensor definition");

    let sensor = landsat8_oli_tirs();
    info!("  Platform: {} ({})", sensor.platform, sensor.sensor_type);
    info!("  Bands: {}", sensor.bands.len());
    if let Some(res) = sensor.temporal_resolution {
        info!("  Revisit: {} days", res);
    }

    // Step 2: Simulate a scene (synthetic DN values for Red/NIR/Green/Blue/Pan)
    info!("Step 2: Preparing synthetic scene (DN values, 0-65535)");

    let width = 256u64;
    let height = 256u64;

    let dn_red = create_dn_band(width, height, 9000.0);
    let dn_green = create_dn_band(width, height, 8000.0);
    let dn_blue = create_dn_band(width, height, 7000.0);
    let dn_nir = create_dn_band(width, height, 15000.0);
    let dn_pan = create_dn_band(width * 2, height * 2, 12000.0);

    // Step 3: Radiometric calibration (DN -> radiance -> TOA reflectance)
    info!("Step 3: Radiometric calibration");

    let calibration = RadiometricCalibration::new(0.0002, -0.1).with_solar_irradiance(1997.0);
    let solar_zenith = 90.0 - 62.4; // sun elevation 62.4 deg
    let earth_sun_distance = 1.0;

    let red_reflectance = calibrate_band(&dn_red, &calibration, solar_zenith, earth_sun_distance)?;
    let green_reflectance =
        calibrate_band(&dn_green, &calibration, solar_zenith, earth_sun_distance)?;
    let blue_reflectance =
        calibrate_band(&dn_blue, &calibration, solar_zenith, earth_sun_distance)?;
    let nir_reflectance = calibrate_band(&dn_nir, &calibration, solar_zenith, earth_sun_distance)?;

    info!("  Converted DN to TOA reflectance for Red/Green/Blue/NIR");

    // Step 4: Atmospheric correction (Dark Object Subtraction)
    info!("Step 4: Applying atmospheric correction (Dark Object Subtraction)");

    let dos = DarkObjectSubtraction::default_params();
    let red_corrected = dos.correct(&red_reflectance.view())?;
    let green_corrected = dos.correct(&green_reflectance.view())?;
    let nir_corrected = dos.correct(&nir_reflectance.view())?;

    info!("  Dark-object subtraction applied to Red/Green/NIR");

    // Step 5: Spectral indices
    info!("Step 5: Computing spectral indices");

    let ndvi_result = ndvi(&nir_corrected.view(), &red_corrected.view())?;
    let ndwi_result = ndwi(&nir_corrected.view(), &green_corrected.view())?;
    let evi_result = evi(
        &nir_corrected.view(),
        &red_corrected.view(),
        &blue_reflectance.view(),
    )?;
    let savi_result = savi(&nir_corrected.view(), &red_corrected.view(), 0.5)?;

    for (name, arr) in [
        ("NDVI", &ndvi_result),
        ("NDWI", &ndwi_result),
        ("EVI", &evi_result),
        ("SAVI", &savi_result),
    ] {
        let (min, max, mean) = array_stats(arr);
        info!(
            "  {}: range [{:.4}, {:.4}], mean {:.4}",
            name, min, max, mean
        );
    }

    // Step 6: Pan-sharpening (Brovey transform)
    info!("Step 6: Pan-sharpening RGB with panchromatic band");

    let dn_pan_array = array2_from_buffer(&dn_pan);
    let pan_reflectance = calibration.dn_to_radiance(&dn_pan_array.view());

    // Resample RGB bands up to the pan resolution (nearest neighbor)
    let red_hi = resample_to(&red_corrected, dn_pan.width(), dn_pan.height());
    let green_hi = resample_to(&green_corrected, dn_pan.width(), dn_pan.height());
    let blue_hi = resample_to(&blue_reflectance, dn_pan.width(), dn_pan.height());

    let sharpener = BroveyTransform;
    let sharpened = sharpener.sharpen(
        &[red_hi.view(), green_hi.view(), blue_hi.view()],
        &pan_reflectance.view(),
    )?;

    info!(
        "  Sharpened resolution: {}x{} (from {}x{})",
        dn_pan.width(),
        dn_pan.height(),
        width,
        height
    );

    // Step 7: Export results
    info!("Step 7: Exporting results as GeoTIFFs");

    let bbox = BoundingBox::new(-120.0, 35.0, -119.0, 36.0)?;
    let gt = GeoTransform::from_bounds(&bbox, width, height)?;
    let gt_pan = GeoTransform::from_bounds(&bbox, dn_pan.width(), dn_pan.height())?;

    save_array(&ndvi_result, &output_dir.join("ndvi.tif"), &gt)?;
    save_array(&ndwi_result, &output_dir.join("ndwi.tif"), &gt)?;
    save_array(&evi_result, &output_dir.join("evi.tif"), &gt)?;
    save_array(&savi_result, &output_dir.join("savi.tif"), &gt)?;

    let band_names = ["red", "green", "blue"];
    for (band, name) in sharpened.iter().zip(band_names.iter()) {
        save_array(
            band,
            &output_dir.join(format!("pansharp_{name}.tif")),
            &gt_pan,
        )?;
    }

    info!(
        "  Wrote {} output files to {:?}",
        4 + sharpened.len(),
        output_dir
    );

    // Step 8: Processing report
    info!("Step 8: Generating processing report");

    let report = ProcessingReport {
        sensor: sensor.name.clone(),
        platform: sensor.platform.clone(),
        scene_size: (width, height),
        pansharp_size: (dn_pan.width(), dn_pan.height()),
        indices_calculated: vec![
            "NDVI".to_string(),
            "NDWI".to_string(),
            "EVI".to_string(),
            "SAVI".to_string(),
        ],
        output_files: 4 + sharpened.len(),
    };

    let report_path = output_dir.join("report.json");
    std::fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;
    info!("  Report saved to {}", report_path.display());

    info!("");
    info!("Pipeline completed successfully!");
    info!(
        "  Processed {} spectral indices",
        report.indices_calculated.len()
    );
    info!("  Output directory: {}", output_dir.display());

    Ok(())
}

#[derive(Debug, serde::Serialize)]
struct ProcessingReport {
    sensor: String,
    platform: String,
    scene_size: (u64, u64),
    pansharp_size: (u64, u64),
    indices_calculated: Vec<String>,
    output_files: usize,
}

/// Create a synthetic band of raw digital numbers (DN)
fn create_dn_band(width: u64, height: u64, base: f64) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let spatial = ((x as f64 / width as f64) + (y as f64 / height as f64)) * 2000.0;
            let dx = (x as f64 - width as f64 / 2.0) / 80.0;
            let dy = (y as f64 - height as f64 / 2.0) / 80.0;
            let feature = 3000.0 * (-(dx * dx + dy * dy) / 2.0).exp();
            let value = (base + spatial + feature).clamp(0.0, 65535.0);
            let _ = buffer.set_pixel(x, y, value);
        }
    }

    buffer
}

fn array2_from_buffer(buffer: &RasterBuffer) -> Array2<f64> {
    let width = buffer.width() as usize;
    let height = buffer.height() as usize;
    let mut arr = Array2::zeros((height, width));

    for y in 0..height {
        for x in 0..width {
            arr[[y, x]] = buffer.get_pixel(x as u64, y as u64).unwrap_or(0.0);
        }
    }

    arr
}

fn calibrate_band(
    dn_buffer: &RasterBuffer,
    calibration: &RadiometricCalibration,
    solar_zenith: f64,
    earth_sun_distance: f64,
) -> Result<Array2<f64>, Box<dyn std::error::Error>> {
    let dn = array2_from_buffer(dn_buffer);
    let radiance = calibration.dn_to_radiance(&dn.view());
    Ok(calibration.radiance_to_reflectance(&radiance.view(), solar_zenith, earth_sun_distance)?)
}

fn array_stats(arr: &Array2<f64>) -> (f64, f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut sum = 0.0;
    let mut count = 0usize;

    for &v in arr.iter() {
        if v.is_finite() {
            min = min.min(v);
            max = max.max(v);
            sum += v;
            count += 1;
        }
    }

    (min, max, sum / count.max(1) as f64)
}

/// Nearest-neighbor resample an `Array2<f64>` up to a new pixel grid
fn resample_to(src: &Array2<f64>, new_width: u64, new_height: u64) -> Array2<f64> {
    let (src_height, src_width) = src.dim();
    let mut dst = Array2::zeros((new_height as usize, new_width as usize));

    let scale_x = src_width as f64 / new_width as f64;
    let scale_y = src_height as f64 / new_height as f64;

    for y in 0..new_height as usize {
        for x in 0..new_width as usize {
            let sx = ((x as f64 * scale_x) as usize).min(src_width - 1);
            let sy = ((y as f64 * scale_y) as usize).min(src_height - 1);
            dst[[y, x]] = src[[sy, sx]];
        }
    }

    dst
}

fn save_array(
    arr: &Array2<f64>,
    path: &Path,
    gt: &GeoTransform,
) -> Result<(), Box<dyn std::error::Error>> {
    let (height, width) = arr.dim();
    let mut buffer = RasterBuffer::zeros(width as u64, height as u64, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            buffer.set_pixel(x as u64, y as u64, arr[[y, x]])?;
        }
    }

    let config = WriterConfig::new(buffer.width(), buffer.height(), 1, buffer.data_type())
        .with_compression(Compression::Deflate)
        .with_tile_size(64, 64)
        .with_geo_transform(*gt)
        .with_epsg_code(4326);

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())?;
    writer.write(buffer.as_bytes())?;

    Ok(())
}
