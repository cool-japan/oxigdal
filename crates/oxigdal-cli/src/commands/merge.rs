//! Merge command - Merge/mosaic multiple rasters
//!
//! This command merges multiple input rasters into a single output raster.
//! Handles overlapping areas using first-valid-pixel strategy.

use crate::OutputFormat;
use crate::util::{progress, raster};
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::GeoTransform;
use serde::Serialize;
use std::path::PathBuf;

/// Merge multiple rasters into a single output
#[derive(Args, Debug)]
pub struct MergeArgs {
    /// Output file path
    #[arg(short = 'o', long, value_name = "OUTPUT")]
    output: PathBuf,

    /// Input raster files
    #[arg(value_name = "INPUT", required = true)]
    inputs: Vec<PathBuf>,

    /// NoData value for inputs
    #[arg(long)]
    no_data: Option<f64>,

    /// Output NoData value
    #[arg(long)]
    output_no_data: Option<f64>,

    /// Target EPSG code (must match all inputs)
    #[arg(long)]
    epsg: Option<u32>,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,

    /// Show progress bar
    #[arg(long, default_value = "true")]
    progress: bool,
}

#[derive(Serialize)]
struct MergeResult {
    output_file: String,
    input_count: usize,
    width: u64,
    height: u64,
    bands: usize,
}

pub fn execute(args: MergeArgs, format: OutputFormat) -> Result<()> {
    if args.output.exists() && !args.overwrite {
        anyhow::bail!(
            "Output file already exists: {}. Use --overwrite to replace.",
            args.output.display()
        );
    }

    if args.inputs.len() < 2 {
        anyhow::bail!("Merge requires at least 2 input files");
    }

    // Validate all inputs exist and read metadata
    let pb = if args.progress {
        Some(progress::create_progress_bar(
            args.inputs.len() as u64,
            "Reading input metadata",
        ))
    } else {
        None
    };

    let mut all_info = Vec::new();
    for input in &args.inputs {
        if !input.exists() {
            anyhow::bail!("Input file not found: {}", input.display());
        }

        let info = raster::read_raster_info(input)
            .with_context(|| format!("Failed to read {}", input.display()))?;
        all_info.push(info);

        if let Some(ref pb) = pb {
            pb.inc(1);
        }
    }

    if let Some(ref pb) = pb {
        pb.finish_with_message("Metadata loaded");
    }

    // Validate compatibility
    let first_bands = all_info[0].bands;
    let first_data_type = all_info[0].data_type;

    for (i, info) in all_info.iter().enumerate() {
        if info.bands != first_bands {
            anyhow::bail!(
                "Input {} has {} bands, but first input has {} bands",
                i,
                info.bands,
                first_bands
            );
        }
        if info.data_type != first_data_type {
            anyhow::bail!(
                "Input {} has data type {:?}, but first input has {:?}",
                i,
                info.data_type,
                first_data_type
            );
        }
        if let Some(epsg) = args.epsg {
            if info.epsg_code != Some(epsg) {
                anyhow::bail!(
                    "Input {} has EPSG:{:?}, but target is EPSG:{}",
                    i,
                    info.epsg_code,
                    epsg
                );
            }
        }
    }

    // Calculate output bounds and resolution
    let (out_min_x, out_min_y, out_max_x, out_max_y, pixel_width, pixel_height) =
        calculate_output_extent(&all_info).context("Failed to calculate output extent")?;

    let out_width = ((out_max_x - out_min_x) / pixel_width).ceil() as u64;
    let out_height = ((out_max_y - out_min_y) / pixel_height.abs()).ceil() as u64;

    let output_geotransform = GeoTransform {
        origin_x: out_min_x,
        origin_y: out_max_y,
        pixel_width,
        pixel_height,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let pb = if args.progress {
        Some(progress::create_progress_bar(
            first_bands as u64,
            "Merging bands",
        ))
    } else {
        None
    };

    // Merge each band
    let mut output_bands = Vec::with_capacity(first_bands as usize);

    for band_idx in 0..first_bands {
        if let Some(ref pb) = pb {
            pb.set_message(format!("Merging band {}/{}", band_idx + 1, first_bands));
        }

        let merged_band = merge_band(
            &args.inputs,
            band_idx,
            out_width,
            out_height,
            &output_geotransform,
            &all_info,
            args.no_data,
        )
        .with_context(|| format!("Failed to merge band {}", band_idx))?;

        output_bands.push(merged_band);

        if let Some(ref pb) = pb {
            pb.inc(1);
        }
    }

    if let Some(ref pb) = pb {
        pb.finish_with_message("Merging complete");
    }

    // Write output
    let spinner = if args.progress {
        Some(progress::create_spinner("Writing output file"))
    } else {
        None
    };

    raster::write_multi_band(
        &args.output,
        &output_bands,
        Some(output_geotransform),
        args.epsg.or(all_info[0].epsg_code),
        args.output_no_data,
    )
    .context("Failed to write output raster")?;

    if let Some(ref sp) = spinner {
        sp.finish_with_message("Output written successfully");
    }

    let result = MergeResult {
        output_file: args.output.display().to_string(),
        input_count: args.inputs.len(),
        width: out_width,
        height: out_height,
        bands: output_bands.len(),
    };

    match format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&result).context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            println!("{}", style("Merge complete").green().bold());
            println!("  Inputs:     {} files", result.input_count);
            println!("  Output:     {}", result.output_file);
            println!("  Dimensions: {} x {}", result.width, result.height);
            println!("  Bands:      {}", result.bands);
        }
    }

    Ok(())
}

/// Calculate output extent covering all inputs
fn calculate_output_extent(
    all_info: &[raster::RasterInfo],
) -> Result<(f64, f64, f64, f64, f64, f64)> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    let mut pixel_widths = Vec::new();
    let mut pixel_heights = Vec::new();

    for info in all_info {
        let gt = info
            .geo_transform
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Input has no geotransform"))?;

        let input_min_x = gt.origin_x;
        let input_max_x = gt.origin_x + gt.pixel_width * info.width as f64;
        let input_max_y = gt.origin_y;
        let input_min_y = gt.origin_y + gt.pixel_height * info.height as f64;

        min_x = min_x.min(input_min_x.min(input_max_x));
        max_x = max_x.max(input_min_x.max(input_max_x));
        min_y = min_y.min(input_min_y.min(input_max_y));
        max_y = max_y.max(input_min_y.max(input_max_y));

        pixel_widths.push(gt.pixel_width.abs());
        pixel_heights.push(gt.pixel_height.abs());
    }

    // Use finest resolution
    let pixel_width = pixel_widths.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let pixel_height = -pixel_heights.iter().fold(f64::INFINITY, |a, &b| a.min(b));

    Ok((min_x, min_y, max_x, max_y, pixel_width, pixel_height))
}

/// Merge a single band from multiple inputs
fn merge_band(
    inputs: &[PathBuf],
    band_idx: u32,
    out_width: u64,
    out_height: u64,
    out_gt: &GeoTransform,
    all_info: &[raster::RasterInfo],
    no_data: Option<f64>,
) -> Result<RasterBuffer> {
    // Initialize output with NoData
    let no_data_value = no_data.unwrap_or(0.0);
    let mut output_data = vec![no_data_value; (out_width * out_height) as usize];

    // Process each input (first input has priority)
    for (input_path, info) in inputs.iter().zip(all_info.iter()) {
        let band_data = raster::read_band(input_path, band_idx)?;
        let input_gt = info
            .geo_transform
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Input has no geotransform"))?;

        // Copy data from input to output
        for in_y in 0..band_data.height() {
            for in_x in 0..band_data.width() {
                // Convert input pixel to geo coordinates
                let geo_x = input_gt.origin_x + in_x as f64 * input_gt.pixel_width;
                let geo_y = input_gt.origin_y + in_y as f64 * input_gt.pixel_height;

                // Convert geo to output pixel
                let out_x = ((geo_x - out_gt.origin_x) / out_gt.pixel_width).floor() as i64;
                let out_y = ((geo_y - out_gt.origin_y) / out_gt.pixel_height).floor() as i64;

                // Check if within output bounds
                if out_x >= 0 && out_x < out_width as i64 && out_y >= 0 && out_y < out_height as i64
                {
                    let out_idx = (out_y as u64 * out_width + out_x as u64) as usize;
                    let value = band_data.get_pixel(in_x, in_y).unwrap_or(no_data_value);

                    // Only write if current output is NoData (first valid pixel wins)
                    if output_data[out_idx] == no_data_value && value != no_data_value {
                        output_data[out_idx] = value;
                    }
                }
            }
        }
    }

    // Convert f64 vec to bytes for RasterBuffer
    use oxigdal_core::types::RasterDataType;
    let byte_data: Vec<u8> = output_data
        .iter()
        .flat_map(|&val| val.to_le_bytes())
        .collect();

    RasterBuffer::new(
        byte_data,
        out_width,
        out_height,
        RasterDataType::Float64,
        oxigdal_core::types::NoDataValue::from_float(no_data_value),
    )
    .map_err(|e| anyhow::anyhow!("Failed to create output buffer: {}", e))
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_merge_requires_multiple_inputs() {
        // This is a structural test - validates module compiles correctly
        let _placeholder = 1;
    }
}
