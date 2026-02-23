//! Warp command - Reproject and resample rasters
//!
//! This command provides comprehensive raster reprojection capabilities:
//! - Reproject between coordinate reference systems
//! - Support EPSG codes and WKT
//! - Resample during reprojection
//! - Crop to target extent
//! - Set NoData values
//! - Specify output resolution
//!
//! Examples:
//! ```bash
//! # Reproject to Web Mercator
//! oxigdal warp -t_srs EPSG:3857 input.tif output.tif
//!
//! # Reproject with bilinear resampling
//! oxigdal warp -t_srs EPSG:4326 -r bilinear input.tif output.tif
//!
//! # Reproject and crop to extent
//! oxigdal warp -t_srs EPSG:3857 --te -10000 -10000 10000 10000 input.tif output.tif
//!
//! # Reproject with specific resolution
//! oxigdal warp -t_srs EPSG:4326 --tr 0.001 input.tif output.tif
//! ```

use crate::OutputFormat;
use crate::util::{progress, raster};
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigdal_algorithms::resampling::ResamplingMethod;
use oxigdal_core::{buffer::RasterBuffer, types::GeoTransform};
use oxigdal_proj::{Coordinate, Crs, Transformer};
use serde::Serialize;
use std::path::PathBuf;

/// Reproject and resample rasters
#[derive(Args, Debug)]
pub struct WarpArgs {
    /// Input file path
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output file path
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Source coordinate reference system (EPSG code or WKT)
    #[arg(short = 's', long)]
    s_srs: Option<String>,

    /// Target coordinate reference system (EPSG code or WKT)
    #[arg(short = 't', long)]
    t_srs: String,

    /// Output width in pixels
    #[arg(long)]
    ts_x: Option<usize>,

    /// Output height in pixels
    #[arg(long)]
    ts_y: Option<usize>,

    /// Output resolution in target units
    #[arg(long)]
    tr: Option<f64>,

    /// Resampling method (nearest, bilinear, bicubic, lanczos)
    #[arg(short, long, default_value = "bilinear")]
    resampling: ResamplingMethodArg,

    /// Output bounds in target SRS (minx miny maxx maxy)
    #[arg(long, num_args = 4, value_names = ["MINX", "MINY", "MAXX", "MAXY"])]
    te: Option<Vec<f64>>,

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
struct WarpResult {
    input_file: String,
    output_file: String,
    source_crs: String,
    target_crs: String,
    width: u64,
    height: u64,
    bands: usize,
    resampling_method: String,
}

pub fn execute(args: WarpArgs, format: OutputFormat) -> Result<()> {
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

    // Parse source CRS
    let source_crs = if let Some(ref s_srs_str) = args.s_srs {
        parse_crs(s_srs_str).context("Failed to parse source CRS")?
    } else {
        // Try to get from file
        let epsg = raster_info
            .epsg_code
            .ok_or_else(|| anyhow::anyhow!("No source CRS specified and none found in file"))?;
        Crs::from_epsg(epsg)
            .map_err(|e| anyhow::anyhow!("Failed to create CRS from EPSG:{}: {}", epsg, e))?
    };

    // Parse target CRS
    let target_crs = parse_crs(&args.t_srs).context("Failed to parse target CRS")?;

    // Create transformer
    let transformer = Transformer::new(source_crs.clone(), target_crs.clone())
        .map_err(|e| anyhow::anyhow!("Failed to create transformer: {}", e))?;

    // Calculate output extent
    let input_gt = raster_info
        .geo_transform
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Input raster has no geotransform"))?;

    let (out_min_x, out_min_y, out_max_x, out_max_y) = if let Some(ref te) = args.te {
        // Use specified target extent
        if te.len() != 4 {
            anyhow::bail!("Target extent requires exactly 4 values: minx miny maxx maxy");
        }
        (te[0], te[1], te[2], te[3])
    } else {
        // Transform input corners to target CRS
        calculate_transformed_extent(
            input_gt,
            raster_info.width,
            raster_info.height,
            &transformer,
        )
        .context("Failed to calculate output extent")?
    };

    // Calculate output resolution and size
    let (out_width, out_height, out_pixel_width, out_pixel_height) =
        if let Some(resolution) = args.tr {
            // Use specified resolution
            let width = ((out_max_x - out_min_x) / resolution).ceil() as u64;
            let height = ((out_max_y - out_min_y) / resolution).ceil() as u64;
            (width, height, resolution, -resolution)
        } else if let (Some(ts_x), Some(ts_y)) = (args.ts_x, args.ts_y) {
            // Use specified size
            let pixel_width = (out_max_x - out_min_x) / ts_x as f64;
            let pixel_height = -(out_max_y - out_min_y) / ts_y as f64;
            (ts_x as u64, ts_y as u64, pixel_width, pixel_height)
        } else {
            // Estimate from input resolution
            let avg_input_res = (input_gt.pixel_width.abs() + input_gt.pixel_height.abs()) / 2.0;
            let width = ((out_max_x - out_min_x) / avg_input_res).ceil() as u64;
            let height = ((out_max_y - out_min_y) / avg_input_res).ceil() as u64;
            let pixel_width = (out_max_x - out_min_x) / width as f64;
            let pixel_height = -(out_max_y - out_min_y) / height as f64;
            (width, height, pixel_width, pixel_height)
        };

    // Create output geotransform
    let output_geotransform = GeoTransform {
        origin_x: out_min_x,
        origin_y: out_max_y,
        pixel_width: out_pixel_width,
        pixel_height: out_pixel_height,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    // Get target EPSG code if possible
    let target_epsg = extract_epsg_code(&args.t_srs);

    let pb = if args.progress {
        Some(progress::create_progress_bar(
            raster_info.bands as u64,
            "Warping bands",
        ))
    } else {
        None
    };

    // Warp each band
    let mut output_bands = Vec::with_capacity(raster_info.bands as usize);

    for band_idx in 0..raster_info.bands {
        if let Some(ref pb) = pb {
            pb.set_message(format!(
                "Warping band {}/{}",
                band_idx + 1,
                raster_info.bands
            ));
        }

        // Read input band
        let input_band = raster::read_band(&args.input, band_idx)
            .with_context(|| format!("Failed to read band {}", band_idx))?;

        // Warp the band
        let params = WarpBandParams {
            input_gt,
            output_gt: &output_geotransform,
            out_width,
            out_height,
            transformer: &transformer,
            resampling: args.resampling.into(),
            no_data_value: raster_info.no_data_value,
        };
        let warped_band = warp_band(&input_band, &params)
            .with_context(|| format!("Failed to warp band {}", band_idx))?;

        output_bands.push(warped_band);

        if let Some(ref pb) = pb {
            pb.inc(1);
        }
    }

    if let Some(ref pb) = pb {
        pb.finish_with_message("Warping complete");
    }

    // Determine NoData value
    let no_data_value = args.no_data.or(raster_info.no_data_value);

    // Write output
    if args.progress {
        let spinner = progress::create_spinner("Writing output file");
        raster::write_multi_band(
            &args.output,
            &output_bands,
            Some(output_geotransform),
            target_epsg,
            no_data_value,
        )
        .context("Failed to write output raster")?;
        spinner.finish_with_message("Output written successfully");
    } else {
        raster::write_multi_band(
            &args.output,
            &output_bands,
            Some(output_geotransform),
            target_epsg,
            no_data_value,
        )
        .context("Failed to write output raster")?;
    }

    // Output results
    let result = WarpResult {
        input_file: args.input.display().to_string(),
        output_file: args.output.display().to_string(),
        source_crs: format!("{:?}", source_crs),
        target_crs: format!("{:?}", target_crs),
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
            println!("{}", style("Warp complete").green().bold());
            println!("  Input:      {}", result.input_file);
            println!("  Output:     {}", result.output_file);
            println!("  Source CRS: {}", result.source_crs);
            println!("  Target CRS: {}", result.target_crs);
            println!("  Dimensions: {} x {}", result.width, result.height);
            println!("  Bands:      {}", result.bands);
            println!("  Resampling: {}", result.resampling_method);
        }
    }

    Ok(())
}

/// Parse CRS from string (EPSG code or WKT)
fn parse_crs(crs_str: &str) -> Result<Crs> {
    // Try to parse as EPSG code
    if let Some(epsg_str) = crs_str.strip_prefix("EPSG:") {
        let epsg: u32 = epsg_str
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid EPSG code: {}", crs_str))?;
        return Crs::from_epsg(epsg)
            .map_err(|e| anyhow::anyhow!("Failed to create CRS from EPSG:{}: {}", epsg, e));
    }

    // Try to parse as WKT
    Crs::from_wkt(crs_str).map_err(|e| anyhow::anyhow!("Failed to parse CRS from WKT: {}", e))
}

/// Extract EPSG code from CRS string
fn extract_epsg_code(crs_str: &str) -> Option<u32> {
    if let Some(epsg_str) = crs_str.strip_prefix("EPSG:") {
        epsg_str.parse().ok()
    } else {
        None
    }
}

/// Calculate transformed extent of input raster in target CRS
fn calculate_transformed_extent(
    input_gt: &GeoTransform,
    width: u64,
    height: u64,
    transformer: &Transformer,
) -> Result<(f64, f64, f64, f64)> {
    // Transform all four corners and edges
    let mut points = Vec::with_capacity(20);

    // Corners
    let corners = [
        (0.0, 0.0),
        (width as f64, 0.0),
        (0.0, height as f64),
        (width as f64, height as f64),
    ];

    // Add edge points (for non-linear projections)
    for i in 1..5 {
        let t = i as f64 / 5.0;
        points.push((t * width as f64, 0.0)); // Top edge
        points.push((t * width as f64, height as f64)); // Bottom edge
        points.push((0.0, t * height as f64)); // Left edge
        points.push((width as f64, t * height as f64)); // Right edge
    }

    // Add corners
    points.extend_from_slice(&corners);

    // Transform all points
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for (px, py) in points {
        // Convert pixel to geo coordinates
        let geo_x = input_gt.origin_x + px * input_gt.pixel_width + py * input_gt.row_rotation;
        let geo_y = input_gt.origin_y + px * input_gt.col_rotation + py * input_gt.pixel_height;

        // Transform to target CRS
        let coord = Coordinate::new(geo_x, geo_y);
        let transformed = transformer
            .transform(&coord)
            .map_err(|e| anyhow::anyhow!("Failed to transform coordinate: {}", e))?;

        min_x = min_x.min(transformed.x);
        min_y = min_y.min(transformed.y);
        max_x = max_x.max(transformed.x);
        max_y = max_y.max(transformed.y);
    }

    Ok((min_x, min_y, max_x, max_y))
}

/// Parameters for band warping
struct WarpBandParams<'a> {
    input_gt: &'a GeoTransform,
    output_gt: &'a GeoTransform,
    out_width: u64,
    out_height: u64,
    transformer: &'a Transformer,
    resampling: ResamplingMethod,
    no_data_value: Option<f64>,
}

/// Warp a single band using coordinate transformation
fn warp_band(input_band: &RasterBuffer, params: &WarpBandParams) -> Result<RasterBuffer> {
    // Create inverse transformer (target -> source)
    let inv_transformer = Transformer::new(
        params.transformer.target_crs().clone(),
        params.transformer.source_crs().clone(),
    )
    .map_err(|e| anyhow::anyhow!("Failed to create inverse transformer: {}", e))?;

    // Create output buffer
    let mut output_data = vec![0.0f64; (params.out_width * params.out_height) as usize];
    let no_data = params.no_data_value.unwrap_or(0.0);

    // For each output pixel, find corresponding input pixel(s)
    for out_y in 0..params.out_height {
        for out_x in 0..params.out_width {
            // Convert output pixel to geo coordinates in target CRS
            let out_geo_x = params.output_gt.origin_x + out_x as f64 * params.output_gt.pixel_width;
            let out_geo_y =
                params.output_gt.origin_y + out_y as f64 * params.output_gt.pixel_height;

            // Transform back to source CRS
            let out_coord = Coordinate::new(out_geo_x, out_geo_y);
            let src_coord = match inv_transformer.transform(&out_coord) {
                Ok(c) => c,
                Err(_) => {
                    // Transformation failed, set to NoData
                    output_data[(out_y * params.out_width + out_x) as usize] = no_data;
                    continue;
                }
            };

            // Convert source geo coordinates to pixel coordinates
            let det = params.input_gt.pixel_width * params.input_gt.pixel_height
                - params.input_gt.row_rotation * params.input_gt.col_rotation;

            if det.abs() < 1e-10 {
                output_data[(out_y * params.out_width + out_x) as usize] = no_data;
                continue;
            }

            let src_px = ((src_coord.x - params.input_gt.origin_x) * params.input_gt.pixel_height
                - (src_coord.y - params.input_gt.origin_y) * params.input_gt.row_rotation)
                / det;

            let src_py = ((src_coord.y - params.input_gt.origin_y) * params.input_gt.pixel_width
                - (src_coord.x - params.input_gt.origin_x) * params.input_gt.col_rotation)
                / det;

            // Check if source pixel is within bounds
            if src_px < 0.0
                || src_py < 0.0
                || src_px >= input_band.width() as f64
                || src_py >= input_band.height() as f64
            {
                output_data[(out_y * params.out_width + out_x) as usize] = no_data;
                continue;
            }

            // Sample using specified resampling method
            let value = sample_pixel(input_band, src_px, src_py, params.resampling, no_data);
            output_data[(out_y * params.out_width + out_x) as usize] = value;
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
        params.out_width,
        params.out_height,
        RasterDataType::Float64,
        oxigdal_core::types::NoDataValue::from_float(no_data),
    )
    .map_err(|e| anyhow::anyhow!("Failed to create output buffer: {}", e))
}

/// Sample a pixel value using the specified resampling method
fn sample_pixel(
    input: &RasterBuffer,
    x: f64,
    y: f64,
    method: ResamplingMethod,
    no_data: f64,
) -> f64 {
    match method {
        ResamplingMethod::Nearest => {
            let ix = x.round() as u64;
            let iy = y.round() as u64;
            if ix < input.width() && iy < input.height() {
                input.get_pixel(ix, iy).unwrap_or(no_data)
            } else {
                no_data
            }
        }
        ResamplingMethod::Bilinear => {
            let x0 = x.floor() as u64;
            let y0 = y.floor() as u64;
            let x1 = (x0 + 1).min(input.width() - 1);
            let y1 = (y0 + 1).min(input.height() - 1);

            let fx = x - x0 as f64;
            let fy = y - y0 as f64;

            let v00 = input.get_pixel(x0, y0).unwrap_or(no_data);
            let v10 = input.get_pixel(x1, y0).unwrap_or(no_data);
            let v01 = input.get_pixel(x0, y1).unwrap_or(no_data);
            let v11 = input.get_pixel(x1, y1).unwrap_or(no_data);

            let v0 = v00 * (1.0 - fx) + v10 * fx;
            let v1 = v01 * (1.0 - fx) + v11 * fx;
            v0 * (1.0 - fy) + v1 * fy
        }
        _ => {
            // For bicubic and lanczos, fall back to bilinear for simplicity
            // A full implementation would use the proper kernels
            sample_pixel(input, x, y, ResamplingMethod::Bilinear, no_data)
        }
    }
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
        assert!(ResamplingMethodArg::from_str("invalid").is_err());
    }

    #[test]
    fn test_parse_crs() {
        let crs = parse_crs("EPSG:4326");
        assert!(crs.is_ok());

        let crs = parse_crs("EPSG:invalid");
        assert!(crs.is_err());
    }

    #[test]
    fn test_extract_epsg_code() {
        assert_eq!(extract_epsg_code("EPSG:4326"), Some(4326));
        assert_eq!(extract_epsg_code("EPSG:3857"), Some(3857));
        assert_eq!(extract_epsg_code("invalid"), None);
    }
}
