//! Sieve command - Remove small raster polygons
//!
//! Removes raster polygons smaller than a provided threshold size (in pixels)
//! and replaces them with the pixel value of the largest neighbor polygon.
//!
//! Examples:
//! ```bash
//! # Remove polygons smaller than 100 pixels
//! oxigdal sieve input.tif output.tif -threshold 100
//!
//! # Apply 8-connectivity
//! oxigdal sieve input.tif output.tif -threshold 50 --eight-connectedness
//! ```

use crate::OutputFormat;
use crate::util::{progress, raster};
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigdal_core::buffer::RasterBuffer;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

/// Remove small raster polygons
#[derive(Args, Debug)]
pub struct SieveArgs {
    /// Input raster file
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output raster file
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Minimum polygon size in pixels
    #[arg(short, long, default_value = "10")]
    threshold: usize,

    /// Use 8-connectedness instead of 4-connectedness
    #[arg(long)]
    eight_connectedness: bool,

    /// Do not use the NoData value
    #[arg(long)]
    no_mask: bool,

    /// Band to operate on (0-indexed)
    #[arg(short, long, default_value = "0")]
    band: u32,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,
}

#[derive(Serialize)]
struct SieveResult {
    input_file: String,
    output_file: String,
    width: u64,
    height: u64,
    threshold: usize,
    polygons_removed: usize,
    processing_time_ms: u128,
}

pub fn execute(args: SieveArgs, format: OutputFormat) -> Result<()> {
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

    // Apply sieve filter
    let pb = progress::create_spinner("Applying sieve filter");

    let (sieved_data, polygons_removed) = apply_sieve_filter(
        &input_data,
        width,
        height,
        args.threshold,
        args.eight_connectedness,
        raster_info.no_data_value,
    )
    .context("Failed to apply sieve filter")?;

    pb.finish_and_clear();

    // Write output
    let pb = progress::create_spinner("Writing output");
    raster::write_single_band(
        &args.output,
        &sieved_data,
        raster_info.geo_transform,
        raster_info.epsg_code,
        raster_info.no_data_value,
    )
    .context("Failed to write output raster")?;
    pb.finish_with_message("Sieve filter applied successfully");

    // Output results
    let result = SieveResult {
        input_file: args.input.display().to_string(),
        output_file: args.output.display().to_string(),
        width: raster_info.width,
        height: raster_info.height,
        threshold: args.threshold,
        polygons_removed,
        processing_time_ms: start.elapsed().as_millis(),
    };

    match format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&result).context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            println!("{}", style("Sieve filter complete").green().bold());
            println!("  Input:             {}", result.input_file);
            println!("  Output:            {}", result.output_file);
            println!("  Dimensions:        {} x {}", result.width, result.height);
            println!("  Threshold:         {} pixels", result.threshold);
            println!("  Polygons removed:  {}", result.polygons_removed);
            println!("  Time:              {} ms", result.processing_time_ms);
        }
    }

    Ok(())
}

/// Apply sieve filter using connected component labeling
fn apply_sieve_filter(
    input_band: &RasterBuffer,
    width: usize,
    height: usize,
    threshold: usize,
    eight_connect: bool,
    no_data: Option<f64>,
) -> Result<(RasterBuffer, usize)> {
    // Convert input data to f64 values
    let input_values = raster_buffer_to_f64(input_band)?;

    let mut output_values = input_values.clone();
    let mut visited = vec![false; width * height];

    let mut polygons_removed = 0;

    // Process each pixel
    for start_y in 0..height {
        for start_x in 0..width {
            let start_idx = start_y * width + start_x;

            if visited[start_idx] {
                continue;
            }

            let pixel_value = input_values[start_idx];

            // Skip NoData pixels
            if let Some(nd) = no_data {
                if (pixel_value - nd).abs() < f64::EPSILON {
                    visited[start_idx] = true;
                    continue;
                }
            }

            // Find connected component using BFS
            let mut polygon_pixels = Vec::new();
            let mut queue = VecDeque::new();
            queue.push_back((start_x, start_y));
            visited[start_idx] = true;

            while let Some((x, y)) = queue.pop_front() {
                polygon_pixels.push((x, y));

                // Check neighbors
                let neighbors = if eight_connect {
                    get_eight_neighbors(x, y, width, height)
                } else {
                    get_four_neighbors(x, y, width, height)
                };

                for (nx, ny) in neighbors {
                    let nidx = ny * width + nx;

                    if !visited[nidx] {
                        let neighbor_value = input_values[nidx];

                        // Check if same value (same polygon)
                        if (neighbor_value - pixel_value).abs() < f64::EPSILON {
                            visited[nidx] = true;
                            queue.push_back((nx, ny));
                        }
                    }
                }
            }

            // If polygon is smaller than threshold, replace with neighbor value
            if polygon_pixels.len() < threshold {
                // Find the most common value among neighbors
                let replacement_value = find_replacement_value(
                    &polygon_pixels,
                    &input_values,
                    width,
                    height,
                    eight_connect,
                    pixel_value,
                );

                // Replace all pixels in the small polygon
                for (px, py) in &polygon_pixels {
                    let idx = py * width + px;
                    output_values[idx] = replacement_value;
                }

                polygons_removed += 1;
            }
        }
    }

    // Convert back to RasterBuffer
    let output_band = f64_to_raster_buffer(
        &output_values,
        width as u64,
        height as u64,
        input_band.data_type(),
        input_band.nodata(),
    )?;

    Ok((output_band, polygons_removed))
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
        _ => anyhow::bail!("Unsupported data type for sieve filter: {:?}", data_type),
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
        _ => anyhow::bail!("Unsupported data type for sieve filter: {:?}", data_type),
    }

    RasterBuffer::new(data, width, height, data_type, no_data)
        .map_err(|e| anyhow::anyhow!("Failed to create RasterBuffer: {}", e))
}

/// Get 4-connected neighbors
fn get_four_neighbors(x: usize, y: usize, width: usize, height: usize) -> Vec<(usize, usize)> {
    let mut neighbors = Vec::new();

    if x > 0 {
        neighbors.push((x - 1, y));
    }
    if x < width - 1 {
        neighbors.push((x + 1, y));
    }
    if y > 0 {
        neighbors.push((x, y - 1));
    }
    if y < height - 1 {
        neighbors.push((x, y + 1));
    }

    neighbors
}

/// Get 8-connected neighbors
fn get_eight_neighbors(x: usize, y: usize, width: usize, height: usize) -> Vec<(usize, usize)> {
    let mut neighbors = Vec::new();

    for dy in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }

            let nx = x as isize + dx;
            let ny = y as isize + dy;

            if nx >= 0 && ny >= 0 && (nx as usize) < width && (ny as usize) < height {
                neighbors.push((nx as usize, ny as usize));
            }
        }
    }

    neighbors
}

/// Find replacement value for small polygon
fn find_replacement_value(
    polygon_pixels: &[(usize, usize)],
    input_values: &[f64],
    width: usize,
    height: usize,
    eight_connect: bool,
    current_value: f64,
) -> f64 {
    let mut neighbor_values = HashMap::new();

    // Collect all neighbor values
    let polygon_set: HashSet<(usize, usize)> = polygon_pixels.iter().cloned().collect();

    for &(x, y) in polygon_pixels {
        let neighbors = if eight_connect {
            get_eight_neighbors(x, y, width, height)
        } else {
            get_four_neighbors(x, y, width, height)
        };

        for (nx, ny) in neighbors {
            if !polygon_set.contains(&(nx, ny)) {
                let nidx = ny * width + nx;
                let nvalue = input_values[nidx];

                if (nvalue - current_value).abs() > f64::EPSILON {
                    *neighbor_values.entry(nvalue.to_bits()).or_insert(0) += 1;
                }
            }
        }
    }

    // Return most common neighbor value
    neighbor_values
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(bits, _)| f64::from_bits(bits))
        .unwrap_or(current_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_four_neighbors() {
        let neighbors = get_four_neighbors(5, 5, 10, 10);
        assert_eq!(neighbors.len(), 4);
        assert!(neighbors.contains(&(4, 5)));
        assert!(neighbors.contains(&(6, 5)));
        assert!(neighbors.contains(&(5, 4)));
        assert!(neighbors.contains(&(5, 6)));
    }

    #[test]
    fn test_get_eight_neighbors() {
        let neighbors = get_eight_neighbors(5, 5, 10, 10);
        assert_eq!(neighbors.len(), 8);
    }

    #[test]
    fn test_get_four_neighbors_edge() {
        let neighbors = get_four_neighbors(0, 0, 10, 10);
        assert_eq!(neighbors.len(), 2);
        assert!(neighbors.contains(&(1, 0)));
        assert!(neighbors.contains(&(0, 1)));
    }
}
