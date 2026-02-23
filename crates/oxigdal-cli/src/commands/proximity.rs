//! Proximity command - Compute proximity raster
//!
//! Generates a raster proximity map indicating the distance from each pixel
//! to the nearest pixel with target values.
//!
//! Examples:
//! ```bash
//! # Compute distance to nearest non-zero pixels
//! oxigdal proximity input.tif distance.tif -values 1
//!
//! # Compute distance in geographic coordinates
//! oxigdal proximity roads.tif road_distance.tif -values 1 -distunits GEO
//! ```

use crate::OutputFormat;
use crate::util::{progress, raster};
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use serde::Serialize;
use std::path::PathBuf;

/// Compute proximity raster
#[derive(Args, Debug)]
pub struct ProximityArgs {
    /// Input raster file
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output proximity raster file
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Target pixel values to compute proximity to (comma-separated)
    #[arg(long, value_delimiter = ',')]
    values: Option<Vec<f64>>,

    /// Distance units (PIXEL or GEO)
    #[arg(long, default_value = "pixel")]
    distunits: DistanceUnits,

    /// Maximum distance to search
    #[arg(long)]
    max_distance: Option<f64>,

    /// NoData value for output
    #[arg(long)]
    no_data: Option<f64>,

    /// Fixed value to use instead of distance
    #[arg(long)]
    fixed_buf_val: Option<f64>,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,
}

#[derive(Debug, Clone, Copy)]
enum DistanceUnits {
    Pixel,
    Geo,
}

impl std::str::FromStr for DistanceUnits {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "PIXEL" | "PIX" => Ok(DistanceUnits::Pixel),
            "GEO" | "GEOGRAPHIC" => Ok(DistanceUnits::Geo),
            _ => Err(format!("Invalid distance units: {}. Use PIXEL or GEO", s)),
        }
    }
}

#[derive(Serialize)]
struct ProximityResult {
    input_file: String,
    output_file: String,
    width: u64,
    height: u64,
    max_distance_computed: f64,
    processing_time_ms: u128,
}

pub fn execute(args: ProximityArgs, format: OutputFormat) -> Result<()> {
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

    let input_data =
        raster::read_band_region(&args.input, 0, 0, 0, raster_info.width, raster_info.height)
            .context("Failed to read raster data")?;
    pb.finish_and_clear();

    let width = raster_info.width as usize;
    let height = raster_info.height as usize;

    // Compute pixel resolution for geographic distance
    let (pixel_res_x, pixel_res_y) = if let Some(ref gt) = raster_info.geo_transform {
        (gt.pixel_width.abs(), gt.pixel_height.abs())
    } else {
        (1.0, 1.0)
    };

    // Compute proximity
    let pb = progress::create_spinner("Computing proximity");

    let proximity_data =
        compute_proximity_map(&input_data, width, height, &args, pixel_res_x, pixel_res_y)
            .context("Failed to compute proximity")?;

    let max_distance_computed = proximity_data
        .iter()
        .filter(|&&v| !v.is_infinite())
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);

    pb.finish_and_clear();

    // Create output raster buffer
    let no_data_value = if let Some(nd) = args.no_data {
        oxigdal_core::types::NoDataValue::from_float(nd)
    } else {
        oxigdal_core::types::NoDataValue::from_float(f64::INFINITY)
    };

    let output_band = f64_to_raster_buffer(
        &proximity_data,
        raster_info.width,
        raster_info.height,
        RasterDataType::Float32,
        no_data_value,
    )
    .context("Failed to create output raster")?;

    // Write output
    let pb = progress::create_spinner("Writing output");
    raster::write_single_band(
        &args.output,
        &output_band,
        raster_info.geo_transform,
        raster_info.epsg_code,
        args.no_data,
    )
    .context("Failed to write output raster")?;
    pb.finish_with_message("Proximity raster written successfully");

    // Output results
    let result = ProximityResult {
        input_file: args.input.display().to_string(),
        output_file: args.output.display().to_string(),
        width: raster_info.width,
        height: raster_info.height,
        max_distance_computed,
        processing_time_ms: start.elapsed().as_millis(),
    };

    match format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&result).context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            println!("{}", style("Proximity computation complete").green().bold());
            println!("  Input:        {}", result.input_file);
            println!("  Output:       {}", result.output_file);
            println!("  Dimensions:   {} x {}", result.width, result.height);
            println!("  Max distance: {:.2}", result.max_distance_computed);
            println!("  Time:         {} ms", result.processing_time_ms);
        }
    }

    Ok(())
}

/// Compute proximity map using euclidean distance transform
fn compute_proximity_map(
    input_band: &RasterBuffer,
    width: usize,
    height: usize,
    args: &ProximityArgs,
    pixel_res_x: f64,
    pixel_res_y: f64,
) -> Result<Vec<f64>> {
    // Convert input data to f64 values
    let input_values = raster_buffer_to_f64(input_band)?;

    let mut proximity = vec![f64::INFINITY; width * height];

    // Determine target values
    let target_values = if let Some(ref values) = args.values {
        values.clone()
    } else {
        // Default to non-zero values
        vec![1.0]
    };

    // First pass: Initialize proximity for target pixels
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let value = input_values[idx];

            // Check if this pixel is a target
            let is_target = target_values
                .iter()
                .any(|&tv| (value - tv).abs() < f64::EPSILON);

            if is_target {
                proximity[idx] = args.fixed_buf_val.unwrap_or(0.0);
            }
        }
    }

    // Simple proximity algorithm using brute force
    // For production, should use optimized distance transform algorithms
    // like Chamfer distance or Euclidean distance transform

    let max_search_distance = args
        .max_distance
        .unwrap_or((width.max(height) as f64).sqrt() * 2.0);

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;

            if proximity[idx].is_finite() {
                continue; // Already a target pixel
            }

            let mut min_distance = f64::INFINITY;

            // Search for nearest target pixel
            for ty in 0..height {
                for tx in 0..width {
                    let tidx = ty * width + tx;

                    if proximity[tidx].is_finite() && proximity[tidx] == 0.0 {
                        // This is a target pixel
                        let dx = (x as f64 - tx as f64) * pixel_res_x;
                        let dy = (y as f64 - ty as f64) * pixel_res_y;

                        let distance = match args.distunits {
                            DistanceUnits::Pixel => {
                                ((x as isize - tx as isize).pow(2)
                                    + (y as isize - ty as isize).pow(2))
                                    as f64
                            }
                            DistanceUnits::Geo => (dx * dx + dy * dy).sqrt(),
                        };

                        if distance <= max_search_distance {
                            min_distance = min_distance.min(distance);
                        }
                    }
                }
            }

            proximity[idx] = if min_distance.is_infinite() {
                args.no_data.unwrap_or(f64::INFINITY)
            } else {
                min_distance
            };
        }
    }

    Ok(proximity)
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
        _ => anyhow::bail!("Unsupported data type for proximity: {:?}", data_type),
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
        _ => anyhow::bail!("Unsupported data type for proximity: {:?}", data_type),
    }

    RasterBuffer::new(data, width, height, data_type, no_data)
        .map_err(|e| anyhow::anyhow!("Failed to create RasterBuffer: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distance_units_parsing() {
        use std::str::FromStr;

        assert!(matches!(
            DistanceUnits::from_str("PIXEL"),
            Ok(DistanceUnits::Pixel)
        ));
        assert!(matches!(
            DistanceUnits::from_str("GEO"),
            Ok(DistanceUnits::Geo)
        ));
        assert!(DistanceUnits::from_str("invalid").is_err());
    }
}
