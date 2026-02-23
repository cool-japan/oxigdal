//! Contour command - Generate contour lines from DEM
//!
//! Creates vector contours (isolines) from a raster surface.
//! Supports fixed interval contours and custom contour levels.
//!
//! Examples:
//! ```bash
//! # Generate contours at 10m intervals
//! oxigdal contour dem.tif contours.geojson -i 10
//!
//! # Generate specific elevation contours
//! oxigdal contour dem.tif contours.geojson -fl 100,200,500,1000
//!
//! # Include elevation attribute
//! oxigdal contour dem.tif contours.geojson -i 25 -a elevation
//! ```

use crate::OutputFormat;
use crate::util::{progress, raster};
use anyhow::{Context, Result};
use clap::Args;
// Note: contour generation is not yet implemented in oxigdal-algorithms
// use oxigdal_algorithms::contour::generate_contours;
use std::path::PathBuf;

/// Generate contour lines from raster
#[derive(Args, Debug)]
pub struct ContourArgs {
    /// Input raster file (DEM)
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output vector file
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Contour interval
    #[arg(short, long)]
    interval: Option<f64>,

    /// Fixed levels (comma-separated)
    #[arg(long = "fl", value_delimiter = ',')]
    fixed_levels: Option<Vec<f64>>,

    /// Base level (offset)
    #[arg(short, long, default_value = "0.0")]
    base: f64,

    /// Attribute name for elevation values
    #[arg(short, long, default_value = "elev")]
    attribute: String,

    /// Minimum elevation to contour
    #[arg(long)]
    min_elev: Option<f64>,

    /// Maximum elevation to contour
    #[arg(long)]
    max_elev: Option<f64>,

    /// NoData value to ignore
    #[arg(long)]
    no_data: Option<f64>,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,
}

// ContourResult will be used when contour generation is implemented
#[allow(dead_code)]
struct ContourResult {
    input_file: String,
    output_file: String,
    contour_count: usize,
    levels_generated: usize,
    processing_time_ms: u128,
}

pub fn execute(args: ContourArgs, _format: OutputFormat) -> Result<()> {
    let _start = std::time::Instant::now();

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

    if args.interval.is_none() && args.fixed_levels.is_none() {
        anyhow::bail!("Must specify either --interval or --fixed-levels");
    }

    // Read DEM
    let pb = progress::create_spinner("Reading DEM");
    let raster_info =
        raster::read_raster_info(&args.input).context("Failed to read DEM metadata")?;

    let dem_data =
        raster::read_band_region(&args.input, 0, 0, 0, raster_info.width, raster_info.height)
            .context("Failed to read DEM data")?;
    pb.finish_and_clear();

    // Determine contour levels
    let levels = if let Some(ref fixed) = args.fixed_levels {
        fixed.clone()
    } else if let Some(interval) = args.interval {
        // Calculate levels from interval
        let dem_values = raster_buffer_to_f64(&dem_data)?;

        let mut min_val = f64::INFINITY;
        let mut max_val = f64::NEG_INFINITY;

        for &val in &dem_values {
            if let Some(nd) = args.no_data {
                if (val - nd).abs() < f64::EPSILON {
                    continue;
                }
            }

            min_val = min_val.min(val);
            max_val = max_val.max(val);
        }

        let min_elev = args.min_elev.unwrap_or(min_val);
        let max_elev = args.max_elev.unwrap_or(max_val);

        let mut levels = Vec::new();
        let mut level = ((min_elev - args.base) / interval).ceil() * interval + args.base;

        while level <= max_elev {
            levels.push(level);
            level += interval;
        }

        levels
    } else {
        unreachable!();
    };

    if levels.is_empty() {
        anyhow::bail!("No contour levels to generate");
    }

    // Generate contours
    // Note: Progress bar and features are created but currently unused because
    // contour generation is not yet implemented
    let _pb = progress::create_progress_bar(levels.len() as u64, "Generating contours");
    let _features: Vec<()> = Vec::new();

    // Note: Contour generation is not yet implemented in oxigdal-algorithms
    // The actual contour generation algorithm needs to be implemented
    // For now, we skip the contour generation to allow the CLI to compile
    anyhow::bail!("Contour generation is not yet implemented. This feature is coming soon!")
}

/// Convert pixel coordinates to geographic coordinates
#[allow(dead_code)]
fn pixel_to_geo(px: f64, py: f64, gt: &oxigdal_core::types::GeoTransform) -> (f64, f64) {
    let x = gt.origin_x + px * gt.pixel_width + py * gt.row_rotation;
    let y = gt.origin_y + px * gt.col_rotation + py * gt.pixel_height;
    (x, y)
}

/// Convert RasterBuffer to `Vec<f64>`
fn raster_buffer_to_f64(buffer: &oxigdal_core::buffer::RasterBuffer) -> Result<Vec<f64>> {
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
        _ => anyhow::bail!("Unsupported data type for contour: {:?}", data_type),
    }

    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::GeoTransform;

    #[test]
    fn test_pixel_to_geo() {
        let gt = GeoTransform {
            origin_x: 0.0,
            pixel_width: 1.0,
            row_rotation: 0.0,
            origin_y: 100.0,
            col_rotation: 0.0,
            pixel_height: -1.0,
        };

        let (x, y) = pixel_to_geo(10.0, 20.0, &gt);
        assert_eq!(x, 10.0);
        assert_eq!(y, 80.0);
    }
}
