//! Info command - Display file metadata and information

use crate::OutputFormat;
use crate::util;
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigdal_core::{io::FileDataSource, types::RasterDataType};
use oxigdal_geojson::GeoJsonReader;
use oxigdal_geotiff::GeoTiffReader;
use serde::Serialize;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::PathBuf;

/// Display information about a raster or vector file
#[derive(Args, Debug)]
pub struct InfoArgs {
    /// Input file path
    #[arg(value_name = "FILE")]
    input: PathBuf,

    /// Show detailed statistics
    #[arg(short, long)]
    stats: bool,

    /// Compute min/max values
    #[arg(long)]
    compute_minmax: bool,

    /// Show all metadata
    #[arg(short, long)]
    metadata: bool,

    /// Show coordinate reference system details
    #[arg(long)]
    crs: bool,

    /// Show band/layer information
    #[arg(short, long)]
    bands: bool,
}

#[derive(Serialize)]
struct FileInfo {
    file_path: String,
    file_size: String,
    format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    raster_info: Option<RasterInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vector_info: Option<VectorInfo>,
}

#[derive(Serialize)]
struct RasterInfo {
    width: u64,
    height: u64,
    bands: u32,
    data_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    geotransform: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    projection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bounds: Option<Bounds>,
}

#[derive(Serialize)]
struct VectorInfo {
    layer_count: usize,
    feature_count: usize,
    geometry_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    bounds: Option<Bounds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crs: Option<String>,
}

#[derive(Serialize)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

pub fn execute(args: InfoArgs, format: OutputFormat) -> Result<()> {
    // Check if file exists
    if !args.input.exists() {
        anyhow::bail!("File not found: {}", args.input.display());
    }

    // Get file size
    let metadata = fs::metadata(&args.input)
        .with_context(|| format!("Failed to read file metadata: {}", args.input.display()))?;
    let file_size = util::format_size(metadata.len());

    // Detect format
    let detected_format =
        util::detect_format(&args.input).ok_or_else(|| anyhow::anyhow!("Unknown file format"))?;

    // Try to read as raster or vector
    let file_info = match detected_format {
        "GeoTIFF" => {
            let raster_info = read_geotiff_info(&args)?;
            FileInfo {
                file_path: args.input.display().to_string(),
                file_size,
                format: detected_format.to_string(),
                raster_info: Some(raster_info),
                vector_info: None,
            }
        }
        "GeoJSON" => {
            let vector_info = read_geojson_info(&args)?;
            FileInfo {
                file_path: args.input.display().to_string(),
                file_size,
                format: detected_format.to_string(),
                raster_info: None,
                vector_info: Some(vector_info),
            }
        }
        _ => {
            anyhow::bail!("Unsupported format: {}", detected_format);
        }
    };

    // Output results
    match format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&file_info).context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            print_text_info(&file_info, &args);
        }
    }

    Ok(())
}

fn read_geotiff_info(args: &InfoArgs) -> Result<RasterInfo> {
    let source = FileDataSource::open(&args.input)
        .with_context(|| format!("Failed to open file: {}", args.input.display()))?;

    let reader = GeoTiffReader::open(source)
        .with_context(|| format!("Failed to read GeoTIFF: {}", args.input.display()))?;

    let width = reader.width();
    let height = reader.height();
    let bands = reader.band_count();
    let data_type = reader
        .data_type()
        .ok_or_else(|| anyhow::anyhow!("Could not determine data type"))?;

    let geotransform = reader.geo_transform().map(|gt| {
        vec![
            gt.origin_x,
            gt.pixel_width,
            gt.row_rotation,
            gt.origin_y,
            gt.col_rotation,
            gt.pixel_height,
        ]
    });

    let projection = reader.epsg_code().map(|code| format!("EPSG:{}", code));

    // Calculate bounds from geotransform
    let bounds = geotransform.as_ref().map(|gt| {
        let min_x = gt[0];
        let max_y = gt[3];
        let max_x = min_x + gt[1] * width as f64;
        let min_y = max_y + gt[5] * height as f64;

        Bounds {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    });

    Ok(RasterInfo {
        width,
        height,
        bands,
        data_type: format_data_type(data_type),
        geotransform,
        projection,
        bounds,
    })
}

fn read_geojson_info(args: &InfoArgs) -> Result<VectorInfo> {
    let file = File::open(&args.input)
        .with_context(|| format!("Failed to open file: {}", args.input.display()))?;
    let buf_reader = BufReader::new(file);
    let mut reader = GeoJsonReader::new(buf_reader);

    let feature_collection = reader
        .read_feature_collection()
        .context("Failed to read GeoJSON")?;

    let feature_count = feature_collection.features.len();

    // Determine geometry type from first feature
    let geometry_type = if let Some(first_feature) = feature_collection.features.first() {
        if let Some(ref geom) = first_feature.geometry {
            format!("{:?}", geom)
        } else {
            "Unknown".to_string()
        }
    } else {
        "Unknown".to_string()
    };

    // Get bounds from feature collection
    let bounds = feature_collection.bbox.as_ref().and_then(|bbox| {
        if bbox.len() >= 4 {
            Some(Bounds {
                min_x: bbox[0],
                min_y: bbox[1],
                max_x: bbox[2],
                max_y: bbox[3],
            })
        } else {
            None
        }
    });

    let crs = feature_collection
        .crs
        .as_ref()
        .map(|crs| format!("{:?}", crs));

    Ok(VectorInfo {
        layer_count: 1,
        feature_count,
        geometry_type,
        bounds,
        crs,
    })
}

fn format_data_type(dt: RasterDataType) -> String {
    match dt {
        RasterDataType::UInt8 => "UInt8".to_string(),
        RasterDataType::UInt16 => "UInt16".to_string(),
        RasterDataType::UInt32 => "UInt32".to_string(),
        RasterDataType::UInt64 => "UInt64".to_string(),
        RasterDataType::Int8 => "Int8".to_string(),
        RasterDataType::Int16 => "Int16".to_string(),
        RasterDataType::Int32 => "Int32".to_string(),
        RasterDataType::Int64 => "Int64".to_string(),
        RasterDataType::Float32 => "Float32".to_string(),
        RasterDataType::Float64 => "Float64".to_string(),
        RasterDataType::CFloat32 => "CFloat32".to_string(),
        RasterDataType::CFloat64 => "CFloat64".to_string(),
    }
}

fn print_text_info(info: &FileInfo, args: &InfoArgs) {
    println!("{}", style("File Information").bold().cyan());
    println!("  Path:   {}", info.file_path);
    println!("  Size:   {}", info.file_size);
    println!("  Format: {}", info.format);
    println!();

    if let Some(ref raster) = info.raster_info {
        println!("{}", style("Raster Information").bold().cyan());
        println!("  Dimensions: {} x {}", raster.width, raster.height);
        println!("  Bands:      {}", raster.bands);
        println!("  Data Type:  {}", raster.data_type);

        if args.crs || args.metadata {
            if let Some(ref proj) = raster.projection {
                println!("\n{}", style("Coordinate Reference System").bold().cyan());
                println!("  {}", proj);
            }
        }

        if let Some(ref gt) = raster.geotransform {
            println!("\n{}", style("Geotransform").bold().cyan());
            println!("  Origin:    ({}, {})", gt[0], gt[3]);
            println!("  Pixel Size: ({}, {})", gt[1], gt[5]);
        }

        if let Some(ref bounds) = raster.bounds {
            println!("\n{}", style("Bounds").bold().cyan());
            println!("  Min X: {}", bounds.min_x);
            println!("  Min Y: {}", bounds.min_y);
            println!("  Max X: {}", bounds.max_x);
            println!("  Max Y: {}", bounds.max_y);
        }
    }

    if let Some(ref vector) = info.vector_info {
        println!("{}", style("Vector Information").bold().cyan());
        println!("  Layers:   {}", vector.layer_count);
        println!("  Features: {}", vector.feature_count);
        println!("  Geometry: {}", vector.geometry_type);

        if args.crs || args.metadata {
            if let Some(ref crs) = vector.crs {
                println!("\n{}", style("Coordinate Reference System").bold().cyan());
                println!("  {}", crs);
            }
        }

        if let Some(ref bounds) = vector.bounds {
            println!("\n{}", style("Bounds").bold().cyan());
            println!("  Min X: {}", bounds.min_x);
            println!("  Min Y: {}", bounds.min_y);
            println!("  Max X: {}", bounds.max_x);
            println!("  Max Y: {}", bounds.max_y);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_data_type() {
        assert_eq!(format_data_type(RasterDataType::UInt8), "UInt8");
        assert_eq!(format_data_type(RasterDataType::Float32), "Float32");
        assert_eq!(format_data_type(RasterDataType::CFloat32), "CFloat32");
    }
}
