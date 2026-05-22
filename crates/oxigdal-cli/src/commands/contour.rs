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
use oxigdal_algorithms::raster::contour::{ContourConfig, ContourLine, generate_contours};
use oxigdal_geojson::types::{Feature, Geometry, LineString};
use oxigdal_geojson::writer::GeoJsonWriter;
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

pub fn execute(args: ContourArgs, _format: OutputFormat) -> Result<()> {
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

    // Convert dimensions to usize for algorithm functions
    let grid_width = raster_info.width as usize;
    let grid_height = raster_info.height as usize;

    // Build ContourConfig
    let mut contour_config = if let Some(interval) = args.interval {
        ContourConfig::new(interval).context("Invalid contour interval")?
    } else {
        // Fixed levels mode: interval is irrelevant — individual levels handled below.
        ContourConfig::new(1.0).context("Internal contour config error")?
    };

    contour_config = contour_config.with_base(args.base);
    if let Some(nd) = args.no_data {
        contour_config = contour_config.with_nodata(nd);
    }

    // Generate contours
    let pb = progress::create_progress_bar(levels.len() as u64, "Generating contours");

    let dem_values = raster_buffer_to_f64(&dem_data)?;

    let all_contours: Vec<ContourLine> = if args.fixed_levels.is_some() {
        // For fixed levels: generate contours per level and collect.
        let mut result = Vec::new();
        for &level in &levels {
            let contours_at = generate_contours_at_level(
                &dem_values,
                grid_width,
                grid_height,
                level,
                args.no_data,
            )?;
            result.extend(contours_at);
            pb.inc(1);
        }
        result
    } else {
        let contours = generate_contours(&dem_values, grid_width, grid_height, &contour_config)
            .context("Contour generation failed")?;
        pb.inc(levels.len() as u64);
        contours
    };

    pb.finish_and_clear();

    // Convert contours to GeoJSON features, applying GeoTransform if available.
    let geo_transform = raster_info.geo_transform;
    let mut features: Vec<Feature> = Vec::with_capacity(all_contours.len());

    for contour in &all_contours {
        let coords: Vec<Vec<f64>> = contour
            .points
            .iter()
            .map(|p| {
                if let Some(gt) = geo_transform {
                    let (gx, gy) = pixel_to_geo(p.x, p.y, &gt);
                    vec![gx, gy]
                } else {
                    vec![p.x, p.y]
                }
            })
            .collect();

        // A contour needs at least 2 points to form a valid LineString.
        // Skip degenerate contours silently.
        let ls = match LineString::new(coords) {
            Ok(ls) => ls,
            Err(_) => continue,
        };
        let geometry = Geometry::LineString(ls);

        let mut props = serde_json::Map::new();
        props.insert(
            args.attribute.clone(),
            serde_json::Value::Number(
                serde_json::Number::from_f64(contour.level).unwrap_or(serde_json::Number::from(0)),
            ),
        );
        props.insert(
            "closed".to_string(),
            serde_json::Value::Bool(contour.is_closed),
        );

        features.push(Feature::new(Some(geometry), Some(props)));
    }

    // Write GeoJSON output using an in-memory buffer.
    let mut buf = Vec::with_capacity(features.len() * 128);
    let mut writer = GeoJsonWriter::compact(&mut buf);
    writer
        .write_features(features)
        .context("Failed to serialise GeoJSON features")?;
    drop(writer);

    std::fs::write(&args.output, &buf)
        .with_context(|| format!("Failed to write output: {}", args.output.display()))?;

    let elapsed_ms = start.elapsed().as_millis();
    println!(
        "Generated {} contour lines across {} levels -> {} ({}ms)",
        all_contours.len(),
        levels.len(),
        args.output.display(),
        elapsed_ms,
    );

    Ok(())
}

/// Generate contours at a single specific elevation level using marching squares.
///
/// This wraps `generate_contours` using a narrow interval centred on the target
/// level so that only that one level is extracted.
fn generate_contours_at_level(
    data: &[f64],
    width: usize,
    height: usize,
    level: f64,
    nodata: Option<f64>,
) -> Result<Vec<ContourLine>> {
    // Use a tiny interval so that only `level` falls within min..max.
    // Base is set to level - epsilon/2 so that `level` is the first valid
    // contour level above the base.
    let epsilon = 1e-6_f64.max(level.abs() * 1e-9);
    let mut config = ContourConfig::new(epsilon.max(1e-9))
        .context("Internal contour config error")?
        .with_base(level - epsilon * 0.5);

    if let Some(nd) = nodata {
        config = config.with_nodata(nd);
    }

    let all =
        generate_contours(data, width, height, &config).context("Contour generation failed")?;

    // Filter to only the lines near the requested level.
    let tolerance = epsilon * 2.0;
    Ok(all
        .into_iter()
        .filter(|c| (c.level - level).abs() <= tolerance)
        .map(|mut c| {
            // Snap level to the requested value.
            c.level = level;
            c
        })
        .collect())
}

/// Convert pixel coordinates to geographic coordinates
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
