//! Convert command - Convert between geospatial formats

use crate::OutputFormat;
use crate::util::progress;
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigeo_geotiff::Compression;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Convert between geospatial formats
#[derive(Args, Debug)]
pub struct ConvertArgs {
    /// Input file path
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output file path
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Output format (auto-detected from extension if not specified)
    #[arg(short = 'f', long = "target-format")]
    target_format: Option<String>,

    /// Tile size for COG output
    #[arg(short, long, default_value = "512")]
    tile_size: usize,

    /// Compression method (none, lzw, deflate, zstd, jpeg)
    #[arg(short, long, default_value = "lzw")]
    compression: CompressionMethod,

    /// Compression level (1-9, format dependent)
    #[arg(long)]
    compression_level: Option<u8>,

    /// Create Cloud-Optimized GeoTIFF
    #[arg(long)]
    cog: bool,

    /// Number of overview levels
    #[arg(long, default_value = "0")]
    overviews: usize,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,

    /// Show progress bar
    #[arg(long, default_value = "true")]
    progress: bool,

    // ── Vector attribute filter flags ────────────────────────────────────
    /// Attribute filter: field name to match against
    #[arg(long = "field")]
    filter_field: Option<String>,

    /// Attribute filter: value to compare with the field
    #[arg(long = "value")]
    filter_value: Option<String>,

    /// Attribute filter operator: eq, ne, or contains (default: eq)
    #[arg(long = "op", default_value = "eq")]
    filter_op: FilterOpArg,

    /// Creation options (KEY=VALUE, GDAL-compatible)
    #[arg(long = "co", value_parser = crate::util::creation_options::parse_key_value)]
    pub creation_options: Vec<(String, String)>,
}

/// Attribute filter operator for CLI argument parsing.
#[derive(Debug, Clone, Copy, Default)]
pub enum FilterOpArg {
    /// Equality comparison (default).
    #[default]
    Eq,
    /// Inequality comparison.
    Ne,
    /// Substring containment comparison.
    Contains,
}

impl std::str::FromStr for FilterOpArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "eq" => Ok(FilterOpArg::Eq),
            "ne" => Ok(FilterOpArg::Ne),
            "contains" => Ok(FilterOpArg::Contains),
            _ => Err(format!(
                "Invalid filter operator '{}': expected eq, ne, or contains",
                s
            )),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CompressionMethod {
    None,
    Lzw,
    Deflate,
    Zstd,
    Jpeg,
}

impl std::str::FromStr for CompressionMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "none" => Ok(CompressionMethod::None),
            "lzw" => Ok(CompressionMethod::Lzw),
            "deflate" => Ok(CompressionMethod::Deflate),
            "zstd" => Ok(CompressionMethod::Zstd),
            "jpeg" => Ok(CompressionMethod::Jpeg),
            _ => Err(format!("Invalid compression method: {}", s)),
        }
    }
}

#[derive(Serialize)]
struct ConversionResult {
    input_file: String,
    output_file: String,
    input_format: String,
    output_format: String,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub fn execute(args: ConvertArgs, format: OutputFormat) -> Result<()> {
    let _co = crate::util::creation_options::map_creation_options(&args.creation_options);

    // Reject cloud output URIs early (cloud write not yet supported)
    let output_str = args.output.to_str().unwrap_or_default();
    if crate::util::cloud::is_cloud_uri(output_str) {
        return Err(crate::util::cloud::error_for_cloud_write(output_str));
    }

    // Validate input
    if !args.input.exists() {
        anyhow::bail!("Input file not found: {}", args.input.display());
    }

    // `--compression-level` cannot currently be honoured: the Pure-Rust GeoTIFF
    // writer selects a fixed per-codec level and exposes no level knob. Rather
    // than silently ignore the flag (producing output at a level the user did
    // not request), reject it with a clear message.
    if let Some(level) = args.compression_level {
        if !(1..=9).contains(&level) {
            anyhow::bail!("--compression-level must be between 1 and 9, got {level}");
        }
        anyhow::bail!(
            "--compression-level is not supported yet: the Pure-Rust GeoTIFF writer \
             uses a fixed level per codec and does not expose a level setting. \
             Re-run without --compression-level."
        );
    }

    if args.output.exists() && !args.overwrite {
        anyhow::bail!(
            "Output file already exists: {}. Use --overwrite to replace.",
            args.output.display()
        );
    }

    // Detect formats
    let input_format = detect_format(&args.input)?;
    let output_format = args
        .target_format
        .as_deref()
        .or_else(|| detect_format(&args.output).ok())
        .ok_or_else(|| anyhow::anyhow!("Cannot detect output format"))?;

    // Perform conversion
    let result = match (input_format, output_format) {
        ("GeoTIFF", "GeoTIFF") => convert_geotiff_to_geotiff(&args),
        // Vector formats: route through the vector conversion utility
        ("GeoJSON", "GeoJSON")
        | ("GeoJSON", "Shapefile")
        | ("GeoJSON", "FlatGeobuf")
        | ("Shapefile", "GeoJSON")
        | ("Shapefile", "Shapefile")
        | ("Shapefile", "FlatGeobuf")
        | ("FlatGeobuf", "GeoJSON")
        | ("FlatGeobuf", "Shapefile")
        | ("FlatGeobuf", "FlatGeobuf") => convert_vector_formats(&args),
        _ => anyhow::bail!(
            "Unsupported conversion: {} to {}",
            input_format,
            output_format
        ),
    };

    let conversion_result = match result {
        Ok(_) => ConversionResult {
            input_file: args.input.display().to_string(),
            output_file: args.output.display().to_string(),
            input_format: input_format.to_string(),
            output_format: output_format.to_string(),
            success: true,
            error: None,
        },
        Err(ref e) => ConversionResult {
            input_file: args.input.display().to_string(),
            output_file: args.output.display().to_string(),
            input_format: input_format.to_string(),
            output_format: output_format.to_string(),
            success: false,
            error: Some(e.to_string()),
        },
    };

    // Output result
    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&conversion_result)
                .context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            if conversion_result.success {
                println!(
                    "{} Converted {} to {}",
                    style("✓").green().bold(),
                    conversion_result.input_file,
                    conversion_result.output_file
                );
            } else {
                println!(
                    "{} Conversion failed: {}",
                    style("✗").red().bold(),
                    conversion_result
                        .error
                        .as_ref()
                        .map_or("Unknown error", |s| s)
                );
            }
        }
    }

    result
}

fn convert_geotiff_to_geotiff(args: &ConvertArgs) -> Result<()> {
    let pb = if args.progress {
        Some(progress::create_spinner("Reading GeoTIFF..."))
    } else {
        None
    };

    // Read source metadata — use URI-aware helper so file:// inputs work too
    let input_str = args.input.to_str().unwrap_or_default();
    let raster_info = crate::util::raster::read_raster_info_uri(input_str)
        .with_context(|| format!("Failed to read input GeoTIFF: {}", args.input.display()))?;

    let band_count = raster_info.bands;

    // Read all bands
    let mut bands = Vec::with_capacity(band_count as usize);
    for band_idx in 0..band_count {
        let buf = crate::util::raster::read_band_region(
            &args.input,
            band_idx,
            0,
            0,
            raster_info.width,
            raster_info.height,
        )
        .with_context(|| {
            format!(
                "Failed to read band {} of {}",
                band_idx,
                args.input.display()
            )
        })?;
        bands.push(buf);
    }

    if let Some(ref pb) = pb {
        pb.set_message("Writing output...");
    }

    // Map CLI compression to geotiff Compression
    let compression = match args.compression {
        CompressionMethod::None => Compression::None,
        CompressionMethod::Lzw => Compression::Lzw,
        CompressionMethod::Deflate => Compression::AdobeDeflate,
        CompressionMethod::Zstd => Compression::Zstd,
        CompressionMethod::Jpeg => Compression::Jpeg,
    };

    // Build overview levels from the overviews count arg
    let overview_levels: Vec<u32> = if args.overviews > 0 {
        (1..=args.overviews).map(|i| 1u32 << i).collect()
    } else {
        Vec::new()
    };

    let tile_size = args.tile_size as u32;

    if args.cog {
        let cog_options = crate::util::raster::CogWriteOptions {
            geo_transform: raster_info.geo_transform,
            epsg_code: raster_info.epsg_code,
            no_data_value: raster_info.no_data_value,
            overview_levels,
            tile_size,
            compression,
        };
        crate::util::raster::write_raster_cog(&args.output, &bands, cog_options)
            .with_context(|| format!("Failed to write COG output: {}", args.output.display()))?;
    } else {
        crate::util::raster::write_multi_band(
            &args.output,
            &bands,
            raster_info.geo_transform,
            raster_info.epsg_code,
            raster_info.no_data_value,
        )
        .with_context(|| format!("Failed to write GeoTIFF output: {}", args.output.display()))?;
    }

    if let Some(pb) = pb {
        pb.finish_with_message("Conversion complete");
    }

    Ok(())
}

fn detect_format(path: &Path) -> Result<&'static str> {
    crate::util::detect_format(path)
        .ok_or_else(|| anyhow::anyhow!("Unknown file format for: {}", path.display()))
}

fn convert_vector_formats(args: &ConvertArgs) -> Result<()> {
    use crate::util::vector::{AttributeFilter, FilterOp};

    let pb = if args.progress {
        Some(progress::create_spinner("Converting vector dataset..."))
    } else {
        None
    };

    // Build attribute filter if --field and --value are provided
    let filter = match (&args.filter_field, &args.filter_value) {
        (Some(field), Some(value)) => {
            let op = match args.filter_op {
                FilterOpArg::Eq => FilterOp::Eq,
                FilterOpArg::Ne => FilterOp::Ne,
                FilterOpArg::Contains => FilterOp::Contains,
            };
            Some(AttributeFilter {
                field: field.clone(),
                op,
                value: value.clone(),
            })
        }
        (Some(field), None) => {
            anyhow::bail!("--value is required when --field '{}' is specified", field)
        }
        (None, Some(_)) => {
            anyhow::bail!("--field is required when --value is specified")
        }
        (None, None) => None,
    };

    let count = crate::util::vector::convert_vector(&args.input, &args.output, filter.as_ref())
        .with_context(|| {
            format!(
                "Vector conversion failed: {} -> {}",
                args.input.display(),
                args.output.display()
            )
        })?;

    if let Some(pb) = pb {
        pb.finish_with_message(format!("Converted {count} features"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_method_parsing() {
        use std::str::FromStr;

        assert!(matches!(
            CompressionMethod::from_str("lzw"),
            Ok(CompressionMethod::Lzw)
        ));
        assert!(matches!(
            CompressionMethod::from_str("deflate"),
            Ok(CompressionMethod::Deflate)
        ));
        assert!(CompressionMethod::from_str("invalid").is_err());
    }

    #[test]
    fn test_detect_format() {
        let path = PathBuf::from("test.tif");
        assert_eq!(detect_format(&path).ok(), Some("GeoTIFF"));
    }
}
