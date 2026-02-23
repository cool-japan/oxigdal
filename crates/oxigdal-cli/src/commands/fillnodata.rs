//! Fill NoData command - Interpolate NoData values
//!
//! Fills raster regions with NoData values by interpolating from valid pixels.
//! Uses inverse distance weighting or other interpolation methods.
//!
//! Examples:
//! ```bash
//! # Fill NoData with default settings
//! oxigdal fillnodata input.tif output.tif
//!
//! # Fill with custom search distance
//! oxigdal fillnodata input.tif output.tif --max-distance 100
//!
//! # Use smoothing iterations
//! oxigdal fillnodata input.tif output.tif --smoothing-iterations 3
//! ```

use crate::OutputFormat;
use crate::util::{progress, raster};
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigdal_core::buffer::RasterBuffer;
use serde::Serialize;
use std::path::PathBuf;

/// Fill NoData values using interpolation
#[derive(Args, Debug)]
pub struct FillNodataArgs {
    /// Input raster file
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output raster file
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Maximum distance to search for valid pixels
    #[arg(long, default_value = "100")]
    max_distance: usize,

    /// Number of smoothing iterations
    #[arg(long, default_value = "0")]
    smoothing_iterations: usize,

    /// Band to operate on (0-indexed)
    #[arg(short, long, default_value = "0")]
    band: u32,

    /// Mask band (NoData defined by this band)
    #[arg(long)]
    mask_band: Option<u32>,

    /// NoData value (if not defined in file)
    #[arg(long)]
    no_data: Option<f64>,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,
}

#[derive(Serialize)]
struct FillNodataResult {
    input_file: String,
    output_file: String,
    width: u64,
    height: u64,
    pixels_filled: usize,
    processing_time_ms: u128,
}

pub fn execute(args: FillNodataArgs, format: OutputFormat) -> Result<()> {
    let start = std::time::Instant::now();

    // Validate inputs
    if !args.input.exists() {
        anyhow::bail!("Input file not found: {}", args.input.display());
    }

    if args.output.exists() && !args.overwrite {
        anyhow::bail!(
            "Output file already exists: {}. Use --overwrite to replace.",
            args.output.display()
        );
    }

    // Read input raster
    let pb = progress::create_spinner("Reading input raster");
    let raster_info =
        raster::read_raster_info(&args.input).context("Failed to read raster metadata")?;

    if args.band >= raster_info.bands {
        anyhow::bail!(
            "Band {} out of range (file has {} bands)",
            args.band,
            raster_info.bands
        );
    }

    let input_data = raster::read_band_region(
        &args.input,
        args.band,
        0,
        0,
        raster_info.width,
        raster_info.height,
    )
    .context("Failed to read raster data")?;
    pb.finish_and_clear();

    let width = raster_info.width as usize;
    let height = raster_info.height as usize;

    // Determine NoData value
    let no_data_value = args
        .no_data
        .or(raster_info.no_data_value)
        .ok_or_else(|| anyhow::anyhow!("NoData value not specified and not found in file"))?;

    // Fill NoData
    let pb = progress::create_spinner("Filling NoData values");

    let (filled_data, pixels_filled) = fill_nodata(
        &input_data,
        width,
        height,
        no_data_value,
        args.max_distance,
        args.smoothing_iterations,
    )
    .context("Failed to fill NoData")?;

    pb.finish_and_clear();

    // Write output
    let pb = progress::create_spinner("Writing output");
    raster::write_single_band(
        &args.output,
        &filled_data,
        raster_info.geo_transform,
        raster_info.epsg_code,
        Some(no_data_value),
    )
    .context("Failed to write output raster")?;
    pb.finish_with_message("NoData filling complete");

    // Output results
    let result = FillNodataResult {
        input_file: args.input.display().to_string(),
        output_file: args.output.display().to_string(),
        width: raster_info.width,
        height: raster_info.height,
        pixels_filled,
        processing_time_ms: start.elapsed().as_millis(),
    };

    match format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&result).context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            println!("{}", style("NoData filling complete").green().bold());
            println!("  Input:         {}", result.input_file);
            println!("  Output:        {}", result.output_file);
            println!("  Dimensions:    {} x {}", result.width, result.height);
            println!("  Pixels filled: {}", result.pixels_filled);
            println!("  Time:          {} ms", result.processing_time_ms);
        }
    }

    Ok(())
}

/// Fill NoData using inverse distance weighting
fn fill_nodata(
    input_band: &RasterBuffer,
    width: usize,
    height: usize,
    no_data_value: f64,
    max_distance: usize,
    smoothing_iterations: usize,
) -> Result<(RasterBuffer, usize)> {
    // Convert input data to f64 values
    let input_values = raster_buffer_to_f64(input_band)?;

    let mut output_values = input_values.clone();
    let mut pixels_filled = 0;

    // Identify NoData pixels
    let mut nodata_pixels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if (input_values[idx] - no_data_value).abs() < f64::EPSILON {
                nodata_pixels.push((x, y, idx));
            }
        }
    }

    // Fill NoData pixels using inverse distance weighting
    for &(x, y, idx) in &nodata_pixels {
        let mut sum_weights = 0.0;
        let mut sum_values = 0.0;
        let mut found_valid = false;

        // Search in expanding squares
        'search: for distance in 1..=max_distance {
            for dy in -(distance as isize)..=(distance as isize) {
                for dx in -(distance as isize)..=(distance as isize) {
                    // Only check perimeter of square
                    if dy.abs() != distance as isize && dx.abs() != distance as isize {
                        continue;
                    }

                    let nx = x as isize + dx;
                    let ny = y as isize + dy;

                    if nx >= 0 && ny >= 0 && (nx as usize) < width && (ny as usize) < height {
                        let nidx = (ny as usize) * width + (nx as usize);
                        let nvalue = input_values[nidx];

                        // Check if valid pixel
                        if (nvalue - no_data_value).abs() > f64::EPSILON {
                            let dist = ((dx * dx + dy * dy) as f64).sqrt();
                            let weight = 1.0 / (dist + 1.0);

                            sum_weights += weight;
                            sum_values += weight * nvalue;
                            found_valid = true;
                        }
                    }
                }
            }

            if found_valid {
                break 'search;
            }
        }

        if found_valid && sum_weights > 0.0 {
            output_values[idx] = sum_values / sum_weights;
            pixels_filled += 1;
        }
    }

    // Apply smoothing iterations
    for _ in 0..smoothing_iterations {
        let mut smoothed = output_values.clone();

        for &(x, y, idx) in &nodata_pixels {
            if (output_values[idx] - no_data_value).abs() > f64::EPSILON {
                // Average with neighbors
                let mut sum = 0.0;
                let mut count = 0;

                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }

                        let nx = x as isize + dx;
                        let ny = y as isize + dy;

                        if nx >= 0 && ny >= 0 && (nx as usize) < width && (ny as usize) < height {
                            let nidx = (ny as usize) * width + (nx as usize);
                            let nvalue = output_values[nidx];

                            if (nvalue - no_data_value).abs() > f64::EPSILON {
                                sum += nvalue;
                                count += 1;
                            }
                        }
                    }
                }

                if count > 0 {
                    smoothed[idx] = sum / count as f64;
                }
            }
        }

        output_values = smoothed;
    }

    // Convert back to RasterBuffer
    let output_band = f64_to_raster_buffer(
        &output_values,
        width as u64,
        height as u64,
        input_band.data_type(),
        input_band.nodata(),
    )?;

    Ok((output_band, pixels_filled))
}

/// Convert RasterBuffer to `Vec<f64>`
fn raster_buffer_to_f64(buffer: &RasterBuffer) -> Result<Vec<f64>> {
    let data = buffer.as_bytes();
    let data_type = buffer.data_type();
    let pixel_count = (buffer.width() * buffer.height()) as usize;

    let mut values = Vec::with_capacity(pixel_count);

    match data_type {
        oxigdal_core::types::RasterDataType::UInt8 => {
            for &byte in data {
                values.push(byte as f64);
            }
        }
        oxigdal_core::types::RasterDataType::Float64 => {
            for chunk in data.chunks_exact(8) {
                let value = f64::from_ne_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ]);
                values.push(value);
            }
        }
        oxigdal_core::types::RasterDataType::Float32 => {
            for chunk in data.chunks_exact(4) {
                let value = f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                values.push(value as f64);
            }
        }
        _ => anyhow::bail!("Unsupported data type for fillnodata: {:?}", data_type),
    }

    Ok(values)
}

/// Convert `Vec<f64>` to RasterBuffer
fn f64_to_raster_buffer(
    values: &[f64],
    width: u64,
    height: u64,
    data_type: oxigdal_core::types::RasterDataType,
    no_data: oxigdal_core::types::NoDataValue,
) -> Result<RasterBuffer> {
    let mut data = Vec::new();

    match data_type {
        oxigdal_core::types::RasterDataType::UInt8 => {
            for &val in values {
                data.push(val as u8);
            }
        }
        oxigdal_core::types::RasterDataType::Float64 => {
            for &val in values {
                data.extend_from_slice(&val.to_ne_bytes());
            }
        }
        oxigdal_core::types::RasterDataType::Float32 => {
            for &val in values {
                let val_f32 = val as f32;
                data.extend_from_slice(&val_f32.to_ne_bytes());
            }
        }
        _ => anyhow::bail!("Unsupported data type for fillnodata: {:?}", data_type),
    }

    RasterBuffer::new(data, width, height, data_type, no_data)
        .map_err(|e| anyhow::anyhow!("Failed to create RasterBuffer: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_fill_nodata_simple() {
        // Create a simple 3x3 raster with center pixel as NoData
        let data = vec![1.0, 2.0, 3.0, 4.0, -9999.0, 6.0, 7.0, 8.0, 9.0];
        let band = f64_to_raster_buffer(
            &data,
            3,
            3,
            RasterDataType::Float64,
            oxigdal_core::types::NoDataValue::Float(-9999.0),
        )
        .expect("Failed to create buffer");

        let (filled, count) =
            fill_nodata(&band, 3, 3, -9999.0, 10, 0).expect("Failed to fill nodata");

        assert_eq!(count, 1);

        let values = raster_buffer_to_f64(&filled).expect("Failed to get values");
        // Center pixel should be filled with average of neighbors
        assert!(values[4] > 0.0 && values[4] < 10.0);
        assert!((values[4] - (-9999.0)).abs() > f64::EPSILON);
    }
}
