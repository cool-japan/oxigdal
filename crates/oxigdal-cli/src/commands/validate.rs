//! Validate command - Validate file format and compliance

use crate::OutputFormat;
use crate::util;
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigdal_core::io::FileDataSource;
use oxigdal_geojson::{GeoJsonReader, Validator as GeoJsonValidator};
use oxigdal_geotiff::GeoTiffReader;
use serde::Serialize;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

/// Validate file format and compliance
#[derive(Args, Debug)]
pub struct ValidateArgs {
    /// Input file path
    #[arg(value_name = "FILE")]
    input: PathBuf,

    /// Validate as Cloud-Optimized GeoTIFF
    #[arg(long)]
    cog: bool,

    /// Validate GeoJSON against specification
    #[arg(long)]
    geojson: bool,

    /// Check for common issues
    #[arg(long)]
    strict: bool,

    /// Detailed validation report
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Serialize)]
struct ValidationResult {
    file_path: String,
    format: String,
    valid: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cog_info: Option<CogInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    geojson_info: Option<GeoJsonInfo>,
}

#[derive(Serialize)]
struct CogInfo {
    is_tiled: bool,
    has_overviews: bool,
    tile_size: Option<(u32, u32)>,
    compression: String,
}

#[derive(Serialize)]
struct GeoJsonInfo {
    feature_count: usize,
    has_crs: bool,
    has_bbox: bool,
    geometry_types: Vec<String>,
}

pub fn execute(args: ValidateArgs, format: OutputFormat) -> Result<()> {
    // Check if file exists
    if !args.input.exists() {
        anyhow::bail!("File not found: {}", args.input.display());
    }

    // Detect format
    let detected_format =
        util::detect_format(&args.input).ok_or_else(|| anyhow::anyhow!("Unknown file format"))?;

    let result = match detected_format {
        "GeoTIFF" => validate_geotiff(&args)?,
        "GeoJSON" => validate_geojson(&args)?,
        _ => {
            anyhow::bail!("Validation not supported for format: {}", detected_format);
        }
    };

    // Output results
    match format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&result).context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            print_validation_result(&result);
        }
    }

    // Return error if validation failed
    if !result.valid {
        anyhow::bail!("Validation failed");
    }

    Ok(())
}

fn validate_geotiff(args: &ValidateArgs) -> Result<ValidationResult> {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    // Open file
    let source = match FileDataSource::open(&args.input) {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("Failed to open file: {}", e));
            return Ok(ValidationResult {
                file_path: args.input.display().to_string(),
                format: "GeoTIFF".to_string(),
                valid: false,
                warnings,
                errors,
                cog_info: None,
                geojson_info: None,
            });
        }
    };

    let reader = match GeoTiffReader::open(source) {
        Ok(r) => r,
        Err(e) => {
            errors.push(format!("Failed to read GeoTIFF: {}", e));
            return Ok(ValidationResult {
                file_path: args.input.display().to_string(),
                format: "GeoTIFF".to_string(),
                valid: false,
                warnings,
                errors,
                cog_info: None,
                geojson_info: None,
            });
        }
    };

    // Basic validation
    let width = reader.width();
    let height = reader.height();
    let bands = reader.band_count();

    if width == 0 || height == 0 {
        errors.push("Invalid raster dimensions".to_string());
    }

    if bands == 0 {
        errors.push("No bands found".to_string());
    }

    if reader.geo_transform().is_none() {
        warnings.push("No geotransform found".to_string());
    }

    if reader.epsg_code().is_none() {
        warnings.push("No CRS information found".to_string());
    }

    // COG validation
    let mut cog_info = None;
    if args.cog {
        let tile_size = reader.tile_size();
        let is_tiled = tile_size.is_some();

        if !is_tiled {
            warnings.push("File is not tiled (required for COG)".to_string());
        }

        let overview_count = reader.overview_count();
        let has_overviews = overview_count > 0;

        if !has_overviews {
            warnings.push("No overviews found (recommended for COG)".to_string());
        }

        let compression = format!("{:?}", reader.compression());

        cog_info = Some(CogInfo {
            is_tiled,
            has_overviews,
            tile_size,
            compression,
        });
    }

    // Strict validation
    if args.strict && (width % 16 != 0 || height % 16 != 0) {
        warnings
            .push("Dimensions are not multiples of 16 (recommended for performance)".to_string());
    }

    Ok(ValidationResult {
        file_path: args.input.display().to_string(),
        format: "GeoTIFF".to_string(),
        valid: errors.is_empty(),
        warnings,
        errors,
        cog_info,
        geojson_info: None,
    })
}

fn validate_geojson(args: &ValidateArgs) -> Result<ValidationResult> {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    // Open file
    let file = match File::open(&args.input) {
        Ok(f) => f,
        Err(e) => {
            errors.push(format!("Failed to open file: {}", e));
            return Ok(ValidationResult {
                file_path: args.input.display().to_string(),
                format: "GeoJSON".to_string(),
                valid: false,
                warnings,
                errors,
                cog_info: None,
                geojson_info: None,
            });
        }
    };

    let buf_reader = BufReader::new(file);
    let mut reader = GeoJsonReader::new(buf_reader);

    let feature_collection = match reader.read_feature_collection() {
        Ok(fc) => fc,
        Err(e) => {
            errors.push(format!("Failed to read GeoJSON: {}", e));
            return Ok(ValidationResult {
                file_path: args.input.display().to_string(),
                format: "GeoJSON".to_string(),
                valid: false,
                warnings,
                errors,
                cog_info: None,
                geojson_info: None,
            });
        }
    };

    // Validate with GeoJSON validator
    let mut validator = GeoJsonValidator::new();
    match validator.validate_feature_collection(&feature_collection) {
        Ok(_) => {}
        Err(e) => {
            errors.push(format!("GeoJSON validation failed: {}", e));
        }
    }

    // Check for CRS
    let has_crs = feature_collection.crs.is_some();
    if !has_crs && args.strict {
        warnings.push("No CRS specified (recommended for GeoJSON)".to_string());
    }

    // Check for bbox
    let has_bbox = feature_collection.bbox.is_some();
    if !has_bbox && args.strict {
        warnings.push("No bounding box specified (recommended for performance)".to_string());
    }

    // Count features
    let feature_count = feature_collection.features.len();
    if feature_count == 0 {
        warnings.push("No features found".to_string());
    }

    // Collect geometry types
    let mut geometry_types = std::collections::HashSet::new();
    for feature in &feature_collection.features {
        if let Some(ref geom) = feature.geometry {
            geometry_types.insert(format!("{:?}", geom));
        }
    }

    let geojson_info = Some(GeoJsonInfo {
        feature_count,
        has_crs,
        has_bbox,
        geometry_types: geometry_types.into_iter().collect(),
    });

    Ok(ValidationResult {
        file_path: args.input.display().to_string(),
        format: "GeoJSON".to_string(),
        valid: errors.is_empty(),
        warnings,
        errors,
        cog_info: None,
        geojson_info,
    })
}

fn print_validation_result(result: &ValidationResult) {
    println!("{}", style("Validation Result").bold().cyan());
    println!("  File:   {}", result.file_path);
    println!("  Format: {}", result.format);
    println!(
        "  Status: {}",
        if result.valid {
            style("VALID").green().bold()
        } else {
            style("INVALID").red().bold()
        }
    );
    println!();

    if !result.errors.is_empty() {
        println!("{}", style("Errors:").red().bold());
        for error in &result.errors {
            println!("  {} {}", style("✗").red(), error);
        }
        println!();
    }

    if !result.warnings.is_empty() {
        println!("{}", style("Warnings:").yellow().bold());
        for warning in &result.warnings {
            println!("  {} {}", style("⚠").yellow(), warning);
        }
        println!();
    }

    if let Some(ref cog) = result.cog_info {
        println!("{}", style("COG Information").bold().cyan());
        println!(
            "  Tiled:     {}",
            if cog.is_tiled {
                style("Yes").green()
            } else {
                style("No").red()
            }
        );
        println!(
            "  Overviews: {}",
            if cog.has_overviews {
                style("Yes").green()
            } else {
                style("No").red()
            }
        );
        if let Some((w, h)) = cog.tile_size {
            println!("  Tile size: {} x {}", w, h);
        }
        println!("  Compression: {}", cog.compression);
        println!();
    }

    if let Some(ref geojson) = result.geojson_info {
        println!("{}", style("GeoJSON Information").bold().cyan());
        println!("  Features:       {}", geojson.feature_count);
        println!(
            "  Has CRS:        {}",
            if geojson.has_crs {
                style("Yes").green()
            } else {
                style("No").yellow()
            }
        );
        println!(
            "  Has BBox:       {}",
            if geojson.has_bbox {
                style("Yes").green()
            } else {
                style("No").yellow()
            }
        );
        println!("  Geometry types: {}", geojson.geometry_types.join(", "));
        println!();
    }

    if result.errors.is_empty() && result.warnings.is_empty() {
        println!("{} File is valid with no issues", style("✓").green().bold());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_result_creation() {
        let result = ValidationResult {
            file_path: "test.tif".to_string(),
            format: "GeoTIFF".to_string(),
            valid: true,
            warnings: vec![],
            errors: vec![],
            cog_info: None,
            geojson_info: None,
        };

        assert!(result.valid);
        assert!(result.errors.is_empty());
    }
}
