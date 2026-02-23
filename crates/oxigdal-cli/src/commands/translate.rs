//! Translate command - Subset and resample rasters
//!
//! This command provides comprehensive raster manipulation capabilities:
//! - Subset by bounding box (geographic coordinates)
//! - Subset by pixel window
//! - Band selection
//! - Resize with resampling
//! - Format conversion
//!
//! Examples:
//! ```bash
//! # Subset by bounding box
//! oxigdal translate -projwin -10 40 10 60 input.tif output.tif
//!
//! # Subset by pixel window
//! oxigdal translate --srcwin 100 100 512 512 input.tif output.tif
//!
//! # Select specific bands
//! oxigdal translate -b 1,2,3 input.tif rgb.tif
//!
//! # Resize with bilinear resampling
//! oxigdal translate --outsize-x 1024 --outsize-y 1024 -r bilinear input.tif output.tif
//! ```

use crate::OutputFormat;
use crate::util::{progress, raster};
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigdal_algorithms::resampling::{Resampler, ResamplingMethod};
use serde::Serialize;
use std::path::PathBuf;

/// Subset and resample rasters
#[derive(Args, Debug)]
pub struct TranslateArgs {
    /// Input file path
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output file path
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Output width in pixels
    #[arg(long)]
    outsize_x: Option<usize>,

    /// Output height in pixels
    #[arg(long)]
    outsize_y: Option<usize>,

    /// Subset by bounding box (minx miny maxx maxy)
    #[arg(long, num_args = 4, value_names = ["MINX", "MINY", "MAXX", "MAXY"])]
    projwin: Option<Vec<f64>>,

    /// Subset by pixel coordinates (xoff yoff xsize ysize)
    #[arg(long, num_args = 4, value_names = ["XOFF", "YOFF", "XSIZE", "YSIZE"])]
    srcwin: Option<Vec<usize>>,

    /// Select specific bands (comma-separated, 0-indexed)
    #[arg(short, long, value_delimiter = ',')]
    bands: Option<Vec<usize>>,

    /// Resampling method (nearest, bilinear, bicubic, lanczos)
    #[arg(short, long, default_value = "nearest")]
    resampling: ResamplingMethodArg,

    /// Set NoData value for output
    #[arg(long)]
    no_data: Option<f64>,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,

    /// Show progress bar
    #[arg(long, default_value = "true")]
    progress: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum ResamplingMethodArg {
    Nearest,
    Bilinear,
    Bicubic,
    Lanczos,
}

impl std::str::FromStr for ResamplingMethodArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "nearest" => Ok(ResamplingMethodArg::Nearest),
            "bilinear" => Ok(ResamplingMethodArg::Bilinear),
            "bicubic" => Ok(ResamplingMethodArg::Bicubic),
            "lanczos" => Ok(ResamplingMethodArg::Lanczos),
            _ => Err(format!("Invalid resampling method: {}", s)),
        }
    }
}

impl From<ResamplingMethodArg> for ResamplingMethod {
    fn from(arg: ResamplingMethodArg) -> Self {
        match arg {
            ResamplingMethodArg::Nearest => ResamplingMethod::Nearest,
            ResamplingMethodArg::Bilinear => ResamplingMethod::Bilinear,
            ResamplingMethodArg::Bicubic => ResamplingMethod::Bicubic,
            ResamplingMethodArg::Lanczos => ResamplingMethod::Lanczos,
        }
    }
}

#[derive(Serialize)]
struct TranslateResult {
    input_file: String,
    output_file: String,
    width: u64,
    height: u64,
    bands: usize,
    resampling_method: String,
}

pub fn execute(args: TranslateArgs, format: OutputFormat) -> Result<()> {
    // Check if input exists
    if !args.input.exists() {
        anyhow::bail!("Input file not found: {}", args.input.display());
    }

    // Check if output exists and overwrite flag
    if args.output.exists() && !args.overwrite {
        anyhow::bail!(
            "Output file already exists: {}. Use --overwrite to replace.",
            args.output.display()
        );
    }

    // Read input raster metadata
    let raster_info =
        raster::read_raster_info(&args.input).context("Failed to read input raster metadata")?;

    // Determine pixel window to read
    let (x_offset, y_offset, read_width, read_height) = if let Some(ref projwin) = args.projwin {
        // Subset by geographic bounding box
        if projwin.len() != 4 {
            anyhow::bail!("projwin requires exactly 4 values: minx miny maxx maxy");
        }

        let geo_transform = raster_info
            .geo_transform
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Input raster has no geotransform"))?;

        raster::geo_to_pixel_window(
            geo_transform,
            projwin[0],
            projwin[1],
            projwin[2],
            projwin[3],
            raster_info.width,
            raster_info.height,
        )
        .context("Failed to calculate pixel window from bounding box")?
    } else if let Some(ref srcwin) = args.srcwin {
        // Subset by pixel window
        if srcwin.len() != 4 {
            anyhow::bail!("srcwin requires exactly 4 values: xoff yoff xsize ysize");
        }

        let x_off = srcwin[0] as u64;
        let y_off = srcwin[1] as u64;
        let width = srcwin[2] as u64;
        let height = srcwin[3] as u64;

        // Validate window bounds
        if x_off + width > raster_info.width {
            anyhow::bail!(
                "Source window extends beyond raster width ({} + {} > {})",
                x_off,
                width,
                raster_info.width
            );
        }
        if y_off + height > raster_info.height {
            anyhow::bail!(
                "Source window extends beyond raster height ({} + {} > {})",
                y_off,
                height,
                raster_info.height
            );
        }

        (x_off, y_off, width, height)
    } else {
        // Read entire raster
        (0, 0, raster_info.width, raster_info.height)
    };

    // Determine output size
    let (out_width, out_height) = match (args.outsize_x, args.outsize_y) {
        (Some(w), Some(h)) => (w as u64, h as u64),
        (Some(w), None) => {
            // Maintain aspect ratio
            let aspect = read_height as f64 / read_width as f64;
            let h = (w as f64 * aspect).round() as u64;
            (w as u64, h)
        }
        (None, Some(h)) => {
            // Maintain aspect ratio
            let aspect = read_width as f64 / read_height as f64;
            let w = (h as f64 * aspect).round() as u64;
            (w, h as u64)
        }
        (None, None) => (read_width, read_height),
    };

    // Determine which bands to process
    let band_indices: Vec<usize> = if let Some(ref bands) = args.bands {
        // Validate band indices
        for &band_idx in bands {
            if band_idx >= raster_info.bands as usize {
                anyhow::bail!(
                    "Band index {} out of range (file has {} bands)",
                    band_idx,
                    raster_info.bands
                );
            }
        }
        bands.clone()
    } else {
        // Use all bands
        (0..raster_info.bands as usize).collect()
    };

    let pb = if args.progress {
        Some(progress::create_progress_bar(
            band_indices.len() as u64,
            "Processing bands",
        ))
    } else {
        None
    };

    // Process each band
    let mut output_bands = Vec::with_capacity(band_indices.len());

    for (i, &band_idx) in band_indices.iter().enumerate() {
        if let Some(ref pb) = pb {
            pb.set_message(format!("Processing band {}/{}", i + 1, band_indices.len()));
        }

        // Read band or region
        let mut band_data = raster::read_band_region(
            &args.input,
            band_idx as u32,
            x_offset,
            y_offset,
            read_width,
            read_height,
        )
        .with_context(|| format!("Failed to read band {}", band_idx))?;

        // Resample if output size differs
        if out_width != read_width || out_height != read_height {
            let resampler = Resampler::new(args.resampling.into());
            band_data = resampler
                .resample(&band_data, out_width, out_height)
                .with_context(|| format!("Failed to resample band {}", band_idx))?;
        }

        output_bands.push(band_data);

        if let Some(ref pb) = pb {
            pb.inc(1);
        }
    }

    if let Some(ref pb) = pb {
        pb.finish_with_message("Band processing complete");
    }

    // Calculate output geotransform
    let output_geotransform = if let Some(mut gt) = raster_info.geo_transform {
        // Adjust origin if subset
        if x_offset != 0 || y_offset != 0 {
            gt = raster::calculate_subset_geotransform(&gt, x_offset, y_offset);
        }

        // Adjust pixel size if resampled
        if out_width != read_width || out_height != read_height {
            let scale_x = read_width as f64 / out_width as f64;
            let scale_y = read_height as f64 / out_height as f64;
            gt.pixel_width *= scale_x;
            gt.pixel_height *= scale_y;
        }

        Some(gt)
    } else {
        None
    };

    // Determine NoData value
    let no_data_value = args.no_data.or(raster_info.no_data_value);

    // Write output
    if args.progress {
        let spinner = progress::create_spinner("Writing output file");
        raster::write_multi_band(
            &args.output,
            &output_bands,
            output_geotransform,
            raster_info.epsg_code,
            no_data_value,
        )
        .context("Failed to write output raster")?;
        spinner.finish_with_message("Output written successfully");
    } else {
        raster::write_multi_band(
            &args.output,
            &output_bands,
            output_geotransform,
            raster_info.epsg_code,
            no_data_value,
        )
        .context("Failed to write output raster")?;
    }

    // Output results
    let result = TranslateResult {
        input_file: args.input.display().to_string(),
        output_file: args.output.display().to_string(),
        width: out_width,
        height: out_height,
        bands: output_bands.len(),
        resampling_method: format!("{:?}", args.resampling),
    };

    match format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&result).context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            println!("{}", style("Translation complete").green().bold());
            println!("  Input:      {}", result.input_file);
            println!("  Output:     {}", result.output_file);
            println!("  Dimensions: {} x {}", result.width, result.height);
            println!("  Bands:      {}", result.bands);
            if out_width != read_width || out_height != read_height {
                println!("  Resampling: {}", result.resampling_method);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resampling_method_parsing() {
        use std::str::FromStr;

        assert!(matches!(
            ResamplingMethodArg::from_str("nearest"),
            Ok(ResamplingMethodArg::Nearest)
        ));
        assert!(matches!(
            ResamplingMethodArg::from_str("bilinear"),
            Ok(ResamplingMethodArg::Bilinear)
        ));
        assert!(matches!(
            ResamplingMethodArg::from_str("bicubic"),
            Ok(ResamplingMethodArg::Bicubic)
        ));
        assert!(matches!(
            ResamplingMethodArg::from_str("lanczos"),
            Ok(ResamplingMethodArg::Lanczos)
        ));
        assert!(ResamplingMethodArg::from_str("invalid").is_err());
    }

    #[test]
    fn test_resampling_method_conversion() {
        let method: ResamplingMethod = ResamplingMethodArg::Bilinear.into();
        assert!(matches!(method, ResamplingMethod::Bilinear));
    }
}
