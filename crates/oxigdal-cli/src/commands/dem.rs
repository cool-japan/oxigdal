//! DEM (Digital Elevation Model) analysis command
//!
//! Provides terrain analysis operations:
//! - Hillshade generation
//! - Slope calculation
//! - Aspect calculation
//! - Terrain Ruggedness Index (TRI)
//! - Topographic Position Index (TPI)
//! - Roughness calculation
//!
//! Examples:
//! ```bash
//! # Generate hillshade
//! oxigdal dem hillshade input.tif hillshade.tif -az 315 -alt 45
//!
//! # Calculate slope
//! oxigdal dem slope input.tif slope.tif --slope-format degree
//!
//! # Calculate aspect
//! oxigdal dem aspect input.tif aspect.tif
//! ```

use crate::OutputFormat;
use crate::util::{progress, raster};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use console::style;
use oxigdal_algorithms::raster::{
    CombinedHillshadeParams, aspect, combined_hillshade, compute_roughness as roughness,
    compute_tpi as topographic_position_index, compute_tri as terrain_ruggedness_index, hillshade,
    slope,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// DEM analysis operations
#[derive(Args, Debug)]
pub struct DemArgs {
    #[command(subcommand)]
    operation: DemOperation,
}

#[derive(Subcommand, Debug)]
enum DemOperation {
    /// Generate hillshade from DEM
    Hillshade(HillshadeArgs),

    /// Calculate slope
    Slope(SlopeArgs),

    /// Calculate aspect
    Aspect(AspectArgs),

    /// Calculate Terrain Ruggedness Index
    Tri(TriArgs),

    /// Calculate Topographic Position Index
    Tpi(TpiArgs),

    /// Calculate roughness
    Roughness(RoughnessArgs),
}

#[derive(Args, Debug)]
struct HillshadeArgs {
    /// Input DEM file
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output hillshade file
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Azimuth of light source (0-360 degrees)
    #[arg(short = 'z', long, default_value = "315.0")]
    azimuth: f64,

    /// Altitude of light source (0-90 degrees)
    #[arg(short, long, default_value = "45.0")]
    altitude: f64,

    /// Z factor (vertical exaggeration)
    #[arg(short, long, default_value = "1.0")]
    z_factor: f64,

    /// Scale (ratio of vertical to horizontal units)
    #[arg(short, long, default_value = "1.0")]
    scale: f64,

    /// Combined shading (Multidirectional oblique weighted)
    #[arg(long)]
    combined: bool,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args, Debug)]
struct SlopeArgs {
    /// Input DEM file
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output slope file
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Slope format: degree or percent
    #[arg(long, default_value = "degree")]
    slope_format: SlopeFormat,

    /// Scale (ratio of vertical to horizontal units)
    #[arg(short, long, default_value = "1.0")]
    scale: f64,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,
}

#[derive(Debug, Clone, Copy)]
enum SlopeFormat {
    Degree,
    Percent,
}

impl std::str::FromStr for SlopeFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "degree" => Ok(SlopeFormat::Degree),
            "percent" => Ok(SlopeFormat::Percent),
            _ => Err(format!(
                "Invalid slope format: {}. Use 'degree' or 'percent'",
                s
            )),
        }
    }
}

#[derive(Args, Debug)]
struct AspectArgs {
    /// Input DEM file
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output aspect file
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Return zero for flat areas instead of -9999
    #[arg(long)]
    zero_for_flat: bool,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args, Debug)]
struct TriArgs {
    /// Input DEM file
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output TRI file
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args, Debug)]
struct TpiArgs {
    /// Input DEM file
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output TPI file
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args, Debug)]
struct RoughnessArgs {
    /// Input DEM file
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output roughness file
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,
}

#[derive(Serialize)]
struct DemResult {
    operation: String,
    input_file: String,
    output_file: String,
    width: u64,
    height: u64,
    processing_time_ms: u128,
}

pub fn execute(args: DemArgs, format: OutputFormat) -> Result<()> {
    match args.operation {
        DemOperation::Hillshade(hs_args) => execute_hillshade(hs_args, format),
        DemOperation::Slope(slope_args) => execute_slope(slope_args, format),
        DemOperation::Aspect(aspect_args) => execute_aspect(aspect_args, format),
        DemOperation::Tri(tri_args) => execute_tri(tri_args, format),
        DemOperation::Tpi(tpi_args) => execute_tpi(tpi_args, format),
        DemOperation::Roughness(rough_args) => execute_roughness(rough_args, format),
    }
}

fn execute_hillshade(args: HillshadeArgs, format: OutputFormat) -> Result<()> {
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

    if !(0.0..=360.0).contains(&args.azimuth) {
        anyhow::bail!("Azimuth must be between 0 and 360 degrees");
    }

    if !(0.0..=90.0).contains(&args.altitude) {
        anyhow::bail!("Altitude must be between 0 and 90 degrees");
    }

    // Read DEM
    let pb = progress::create_spinner("Reading DEM");
    let raster_info =
        raster::read_raster_info(&args.input).context("Failed to read DEM metadata")?;

    let dem_data =
        raster::read_band_region(&args.input, 0, 0, 0, raster_info.width, raster_info.height)
            .context("Failed to read DEM data")?;
    pb.finish_and_clear();

    // Calculate hillshade
    let pb = progress::create_spinner(if args.combined {
        "Calculating combined hillshade"
    } else {
        "Calculating hillshade"
    });

    let hillshade_band = if args.combined {
        // Use combined/multidirectional hillshade with GDAL-style weights
        let combined_params = CombinedHillshadeParams::gdal_multidirectional()
            .with_altitude(args.altitude)
            .with_z_factor(args.z_factor)
            .with_pixel_size(1.0)
            .with_scale(args.scale);

        combined_hillshade(&dem_data, combined_params)
            .context("Failed to calculate combined hillshade")?
    } else {
        // Standard single-direction hillshade
        use oxigdal_algorithms::raster::HillshadeParams;
        let params = HillshadeParams {
            azimuth: args.azimuth,
            altitude: args.altitude,
            z_factor: args.z_factor,
            pixel_size: 1.0,
            scale: args.scale,
        };
        hillshade(&dem_data, params).context("Failed to calculate hillshade")?
    };
    pb.finish_and_clear();

    // Write output
    let pb = progress::create_spinner("Writing output");
    raster::write_single_band(
        &args.output,
        &hillshade_band,
        raster_info.geo_transform,
        raster_info.epsg_code,
        None,
    )
    .context("Failed to write hillshade output")?;
    pb.finish_with_message("Hillshade generation complete");

    output_result(
        "Hillshade",
        &args.input,
        &args.output,
        raster_info.width,
        raster_info.height,
        start.elapsed().as_millis(),
        format,
    )
}

fn execute_slope(args: SlopeArgs, format: OutputFormat) -> Result<()> {
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

    // Read DEM
    let pb = progress::create_spinner("Reading DEM");
    let raster_info =
        raster::read_raster_info(&args.input).context("Failed to read DEM metadata")?;

    let dem_data =
        raster::read_band_region(&args.input, 0, 0, 0, raster_info.width, raster_info.height)
            .context("Failed to read DEM data")?;
    pb.finish_and_clear();

    // Calculate slope
    let pb = progress::create_spinner("Calculating slope");
    // slope(dem, pixel_size, z_factor) — z_factor=1.0, scale applies as pixel_size
    let slope_band = slope(&dem_data, args.scale, 1.0).context("Failed to calculate slope")?;

    // Convert to percent if requested (slope is returned in degrees, convert per-pixel)
    let slope_band = if matches!(args.slope_format, SlopeFormat::Percent) {
        use oxigdal_algorithms::raster::{SlopeUnits, convert_slope_degrees};
        let mut pct_band = oxigdal_core::buffer::RasterBuffer::zeros(
            slope_band.width(),
            slope_band.height(),
            slope_band.data_type(),
        );
        for y in 0..slope_band.height() {
            for x in 0..slope_band.width() {
                let deg = slope_band
                    .get_pixel(x, y)
                    .context("Failed to read slope pixel")?;
                let pct = convert_slope_degrees(deg, SlopeUnits::Percent);
                pct_band
                    .set_pixel(x, y, pct)
                    .context("Failed to set slope percent pixel")?;
            }
        }
        pct_band
    } else {
        slope_band
    };
    pb.finish_and_clear();

    // Write output
    let pb = progress::create_spinner("Writing output");
    raster::write_single_band(
        &args.output,
        &slope_band,
        raster_info.geo_transform,
        raster_info.epsg_code,
        None,
    )
    .context("Failed to write slope output")?;
    pb.finish_with_message("Slope calculation complete");

    output_result(
        "Slope",
        &args.input,
        &args.output,
        raster_info.width,
        raster_info.height,
        start.elapsed().as_millis(),
        format,
    )
}

fn execute_aspect(args: AspectArgs, format: OutputFormat) -> Result<()> {
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

    // Read DEM
    let pb = progress::create_spinner("Reading DEM");
    let raster_info =
        raster::read_raster_info(&args.input).context("Failed to read DEM metadata")?;

    let dem_data =
        raster::read_band_region(&args.input, 0, 0, 0, raster_info.width, raster_info.height)
            .context("Failed to read DEM data")?;
    pb.finish_and_clear();

    // Calculate aspect — aspect(dem, pixel_size, z_factor)
    let pb = progress::create_spinner("Calculating aspect");
    let mut aspect_band = aspect(&dem_data, 1.0, 1.0).context("Failed to calculate aspect")?;

    // Optionally map flat areas (-9999) to 0
    if args.zero_for_flat {
        let flat_sentinel = -9999.0_f64;
        for y in 0..aspect_band.height() {
            for x in 0..aspect_band.width() {
                if let Ok(v) = aspect_band.get_pixel(x, y) {
                    if (v - flat_sentinel).abs() < 1.0 {
                        aspect_band
                            .set_pixel(x, y, 0.0)
                            .context("Failed to set pixel")?;
                    }
                }
            }
        }
    }
    pb.finish_and_clear();

    // Write output
    let pb = progress::create_spinner("Writing output");
    raster::write_single_band(
        &args.output,
        &aspect_band,
        raster_info.geo_transform,
        raster_info.epsg_code,
        None,
    )
    .context("Failed to write aspect output")?;
    pb.finish_with_message("Aspect calculation complete");

    output_result(
        "Aspect",
        &args.input,
        &args.output,
        raster_info.width,
        raster_info.height,
        start.elapsed().as_millis(),
        format,
    )
}

fn execute_tri(args: TriArgs, format: OutputFormat) -> Result<()> {
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

    // Read DEM
    let pb = progress::create_spinner("Reading DEM");
    let raster_info =
        raster::read_raster_info(&args.input).context("Failed to read DEM metadata")?;

    let dem_data =
        raster::read_band_region(&args.input, 0, 0, 0, raster_info.width, raster_info.height)
            .context("Failed to read DEM data")?;
    pb.finish_and_clear();

    // Calculate TRI — terrain_ruggedness_index(dem, cell_size)
    let pb = progress::create_spinner("Calculating TRI");
    let tri_band = terrain_ruggedness_index(&dem_data, 1.0).context("Failed to calculate TRI")?;
    pb.finish_and_clear();

    // Write output
    let pb = progress::create_spinner("Writing output");
    raster::write_single_band(
        &args.output,
        &tri_band,
        raster_info.geo_transform,
        raster_info.epsg_code,
        None,
    )
    .context("Failed to write TRI output")?;
    pb.finish_with_message("TRI calculation complete");

    output_result(
        "TRI",
        &args.input,
        &args.output,
        raster_info.width,
        raster_info.height,
        start.elapsed().as_millis(),
        format,
    )
}

fn execute_tpi(args: TpiArgs, format: OutputFormat) -> Result<()> {
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

    // Read DEM
    let pb = progress::create_spinner("Reading DEM");
    let raster_info =
        raster::read_raster_info(&args.input).context("Failed to read DEM metadata")?;

    let dem_data =
        raster::read_band_region(&args.input, 0, 0, 0, raster_info.width, raster_info.height)
            .context("Failed to read DEM data")?;
    pb.finish_and_clear();

    // Calculate TPI — topographic_position_index(dem, neighborhood_size, cell_size)
    let pb = progress::create_spinner("Calculating TPI");
    let tpi_band =
        topographic_position_index(&dem_data, 3, 1.0).context("Failed to calculate TPI")?;
    pb.finish_and_clear();

    // Write output
    let pb = progress::create_spinner("Writing output");
    raster::write_single_band(
        &args.output,
        &tpi_band,
        raster_info.geo_transform,
        raster_info.epsg_code,
        None,
    )
    .context("Failed to write TPI output")?;
    pb.finish_with_message("TPI calculation complete");

    output_result(
        "TPI",
        &args.input,
        &args.output,
        raster_info.width,
        raster_info.height,
        start.elapsed().as_millis(),
        format,
    )
}

fn execute_roughness(args: RoughnessArgs, format: OutputFormat) -> Result<()> {
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

    // Read DEM
    let pb = progress::create_spinner("Reading DEM");
    let raster_info =
        raster::read_raster_info(&args.input).context("Failed to read DEM metadata")?;

    let dem_data =
        raster::read_band_region(&args.input, 0, 0, 0, raster_info.width, raster_info.height)
            .context("Failed to read DEM data")?;
    pb.finish_and_clear();

    // Calculate roughness — roughness(dem, neighborhood_size)
    let pb = progress::create_spinner("Calculating roughness");
    let roughness_band = roughness(&dem_data, 3).context("Failed to calculate roughness")?;
    pb.finish_and_clear();

    // Write output
    let pb = progress::create_spinner("Writing output");
    raster::write_single_band(
        &args.output,
        &roughness_band,
        raster_info.geo_transform,
        raster_info.epsg_code,
        None,
    )
    .context("Failed to write roughness output")?;
    pb.finish_with_message("Roughness calculation complete");

    output_result(
        "Roughness",
        &args.input,
        &args.output,
        raster_info.width,
        raster_info.height,
        start.elapsed().as_millis(),
        format,
    )
}

fn output_result(
    operation: &str,
    input: &Path,
    output: &Path,
    width: u64,
    height: u64,
    processing_time_ms: u128,
    format: OutputFormat,
) -> Result<()> {
    let result = DemResult {
        operation: operation.to_string(),
        input_file: input.display().to_string(),
        output_file: output.display().to_string(),
        width,
        height,
        processing_time_ms,
    };

    match format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&result).context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            println!(
                "{}",
                style(format!("{} complete", operation)).green().bold()
            );
            println!("  Input:      {}", result.input_file);
            println!("  Output:     {}", result.output_file);
            println!("  Dimensions: {} x {}", result.width, result.height);
            println!("  Time:       {} ms", result.processing_time_ms);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slope_format_parsing() {
        use std::str::FromStr;

        assert!(matches!(
            SlopeFormat::from_str("degree"),
            Ok(SlopeFormat::Degree)
        ));
        assert!(matches!(
            SlopeFormat::from_str("percent"),
            Ok(SlopeFormat::Percent)
        ));
        assert!(SlopeFormat::from_str("invalid").is_err());
    }
}
