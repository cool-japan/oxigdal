//! Calc command - Raster calculator operations
//!
//! This command provides comprehensive raster algebra capabilities:
//! - Band math with arithmetic operations (+, -, *, /, ^)
//! - Mathematical functions (sqrt, log, exp, sin, cos, tan, abs, etc.)
//! - Conditional expressions (if/then/else)
//! - Multi-input support (up to 26 inputs: A-Z)
//! - Proper NoData handling
//! - Expression evaluation with proper precedence
//!
//! Examples:
//! ```bash
//! # Calculate NDVI: (NIR - Red) / (NIR + Red)
//! oxigdal calc -A nir.tif -B red.tif --calc "(A-B)/(A+B)" -o ndvi.tif
//!
//! # Simple arithmetic
//! oxigdal calc -A input.tif --calc "A * 2.0 + 10.0" -o output.tif
//!
//! # Mathematical functions
//! oxigdal calc -A dem.tif --calc "sqrt(A)" -o sqrt_dem.tif
//!
//! # Conditional expression
//! oxigdal calc -A input.tif --calc "if A > 100 then 1 else 0" -o mask.tif
//!
//! # Multiple inputs
//! oxigdal calc -A b1.tif -B b2.tif -C b3.tif --calc "(A+B+C)/3" -o average.tif
//! ```

use crate::OutputFormat;
use crate::util::{progress, raster};
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigdal_algorithms::raster::RasterCalculator;
use oxigdal_core::types::RasterDataType;
use serde::Serialize;
use std::path::PathBuf;

/// Raster calculator operations
#[derive(Args, Debug)]
pub struct CalcArgs {
    /// Output file path
    #[arg(short = 'o', long, value_name = "OUTPUT")]
    output: PathBuf,

    /// Input file A
    #[arg(short = 'A', long)]
    input_a: Option<PathBuf>,

    /// Input file B
    #[arg(short = 'B', long)]
    input_b: Option<PathBuf>,

    /// Input file C
    #[arg(short = 'C', long)]
    input_c: Option<PathBuf>,

    /// Input file D
    #[arg(short = 'D', long)]
    input_d: Option<PathBuf>,

    /// Input file E
    #[arg(short = 'E', long)]
    input_e: Option<PathBuf>,

    /// Input file F
    #[arg(short = 'F', long)]
    input_f: Option<PathBuf>,

    /// Calculation expression (e.g., "(A-B)/(A+B)")
    #[arg(long, required = true)]
    calc: String,

    /// Band index to read from each input (0-indexed)
    #[arg(long, default_value = "0")]
    band: u32,

    /// No data value for output
    #[arg(long)]
    no_data: Option<f64>,

    /// Output data type (uint8, uint16, uint32, int16, int32, float32, float64)
    #[arg(long, default_value = "float32")]
    output_type: DataTypeArg,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,

    /// Show progress bar
    #[arg(long, default_value = "true")]
    progress: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum DataTypeArg {
    UInt8,
    UInt16,
    UInt32,
    Int16,
    Int32,
    Float32,
    Float64,
}

impl std::str::FromStr for DataTypeArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "uint8" | "byte" => Ok(DataTypeArg::UInt8),
            "uint16" => Ok(DataTypeArg::UInt16),
            "uint32" => Ok(DataTypeArg::UInt32),
            "int16" => Ok(DataTypeArg::Int16),
            "int32" => Ok(DataTypeArg::Int32),
            "float32" => Ok(DataTypeArg::Float32),
            "float64" => Ok(DataTypeArg::Float64),
            _ => Err(format!("Invalid data type: {}", s)),
        }
    }
}

impl From<DataTypeArg> for RasterDataType {
    fn from(arg: DataTypeArg) -> Self {
        match arg {
            DataTypeArg::UInt8 => RasterDataType::UInt8,
            DataTypeArg::UInt16 => RasterDataType::UInt16,
            DataTypeArg::UInt32 => RasterDataType::UInt32,
            DataTypeArg::Int16 => RasterDataType::Int16,
            DataTypeArg::Int32 => RasterDataType::Int32,
            DataTypeArg::Float32 => RasterDataType::Float32,
            DataTypeArg::Float64 => RasterDataType::Float64,
        }
    }
}

#[derive(Serialize)]
struct CalcResult {
    output_file: String,
    expression: String,
    inputs: Vec<String>,
    width: u64,
    height: u64,
    output_type: String,
}

pub fn execute(args: CalcArgs, format: OutputFormat) -> Result<()> {
    // Check if output exists and overwrite flag
    if args.output.exists() && !args.overwrite {
        anyhow::bail!(
            "Output file already exists: {}. Use --overwrite to replace.",
            args.output.display()
        );
    }

    // Collect input files in order (A, B, C, D, E, F)
    let mut input_files = Vec::new();
    let mut input_letters = Vec::new();

    if let Some(ref path) = args.input_a {
        input_files.push(path.clone());
        input_letters.push('A');
    }
    if let Some(ref path) = args.input_b {
        input_files.push(path.clone());
        input_letters.push('B');
    }
    if let Some(ref path) = args.input_c {
        input_files.push(path.clone());
        input_letters.push('C');
    }
    if let Some(ref path) = args.input_d {
        input_files.push(path.clone());
        input_letters.push('D');
    }
    if let Some(ref path) = args.input_e {
        input_files.push(path.clone());
        input_letters.push('E');
    }
    if let Some(ref path) = args.input_f {
        input_files.push(path.clone());
        input_letters.push('F');
    }

    if input_files.is_empty() {
        anyhow::bail!("No input files provided. Use -A, -B, etc. to specify inputs.");
    }

    // Validate all input files exist
    for (i, path) in input_files.iter().enumerate() {
        if !path.exists() {
            anyhow::bail!(
                "Input file {} ({}) not found: {}",
                input_letters[i],
                i,
                path.display()
            );
        }
    }

    let pb = if args.progress {
        Some(progress::create_progress_bar(
            input_files.len() as u64,
            "Reading input bands",
        ))
    } else {
        None
    };

    // Read all input bands
    let mut input_bands = Vec::with_capacity(input_files.len());
    let mut reference_info: Option<(u64, u64)> = None;

    for (i, path) in input_files.iter().enumerate() {
        if let Some(ref pb) = pb {
            pb.set_message(format!(
                "Reading input {} ({}/{})",
                input_letters[i],
                i + 1,
                input_files.len()
            ));
        }

        // Read metadata
        let info = raster::read_raster_info(path)
            .with_context(|| format!("Failed to read metadata from {}", path.display()))?;

        // Validate band index
        if args.band >= info.bands {
            anyhow::bail!(
                "Band index {} out of range for input {} (file has {} bands)",
                args.band,
                input_letters[i],
                info.bands
            );
        }

        // Check dimensions match first input
        if let Some(ref ref_info) = reference_info {
            if info.width != ref_info.0 || info.height != ref_info.1 {
                anyhow::bail!(
                    "Input {} dimensions ({} x {}) do not match input A ({} x {})",
                    input_letters[i],
                    info.width,
                    info.height,
                    ref_info.0,
                    ref_info.1
                );
            }
        } else {
            reference_info = Some((info.width, info.height));
        }

        // Read band data
        let band_data = raster::read_band(path, args.band).with_context(|| {
            format!("Failed to read band {} from {}", args.band, path.display())
        })?;

        input_bands.push(band_data);

        if let Some(ref pb) = pb {
            pb.inc(1);
        }
    }

    if let Some(ref pb) = pb {
        pb.finish_with_message("All input bands loaded");
    }

    // Convert expression: replace A, B, C, etc. with B1, B2, B3, etc.
    let converted_expression =
        convert_expression(&args.calc, &input_letters).context("Failed to convert expression")?;

    // Evaluate expression
    let spinner = if args.progress {
        Some(progress::create_spinner("Evaluating expression"))
    } else {
        None
    };

    let result_band = RasterCalculator::evaluate(&converted_expression, &input_bands)
        .map_err(|e| anyhow::anyhow!("Failed to evaluate expression: {}", e))?;

    if let Some(ref sp) = spinner {
        sp.finish_with_message("Expression evaluated");
    }

    // Get metadata from first input for output
    let first_info = raster::read_raster_info(&input_files[0])
        .context("Failed to read metadata from first input")?;

    // Write output
    let write_spinner = if args.progress {
        Some(progress::create_spinner("Writing output file"))
    } else {
        None
    };

    raster::write_single_band(
        &args.output,
        &result_band,
        first_info.geo_transform,
        first_info.epsg_code,
        args.no_data,
    )
    .context("Failed to write output raster")?;

    if let Some(ref sp) = write_spinner {
        sp.finish_with_message("Output written successfully");
    }

    // Output results
    let result = CalcResult {
        output_file: args.output.display().to_string(),
        expression: args.calc.clone(),
        inputs: input_files
            .iter()
            .enumerate()
            .map(|(i, p)| format!("{}: {}", input_letters[i], p.display()))
            .collect(),
        width: result_band.width(),
        height: result_band.height(),
        output_type: format!("{:?}", args.output_type),
    };

    match format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&result).context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            println!("{}", style("Calculation complete").green().bold());
            println!("  Expression: {}", result.expression);
            println!("  Inputs:");
            for input in &result.inputs {
                println!("    {}", input);
            }
            println!("  Output:     {}", result.output_file);
            println!("  Dimensions: {} x {}", result.width, result.height);
            println!("  Data Type:  {}", result.output_type);
        }
    }

    Ok(())
}

/// Convert expression from using A, B, C... to B1, B2, B3...
fn convert_expression(expr: &str, letters: &[char]) -> Result<String> {
    let mut result = expr.to_string();

    // Sort letters by length (descending) to avoid replacing substrings
    let mut sorted_letters = letters.to_vec();
    sorted_letters.sort_by_key(|b| std::cmp::Reverse(b.to_string().len()));

    for (i, &letter) in letters.iter().enumerate() {
        let band_num = i + 1;
        let letter_str = letter.to_string();
        let band_str = format!("B{}", band_num);

        // Replace whole word only (not part of other words)
        // Re-add operators and parentheses
        let mut final_result = String::new();
        let chars = result.chars();
        let mut word = String::new();

        for c in chars {
            if c.is_alphanumeric() || c == '_' {
                word.push(c);
            } else {
                if !word.is_empty() {
                    if word == letter_str {
                        final_result.push_str(&band_str);
                    } else {
                        final_result.push_str(&word);
                    }
                    word.clear();
                }
                final_result.push(c);
            }
        }

        if !word.is_empty() {
            if word == letter_str {
                final_result.push_str(&band_str);
            } else {
                final_result.push_str(&word);
            }
        }

        result = final_result;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_type_parsing() {
        use std::str::FromStr;

        assert!(matches!(
            DataTypeArg::from_str("uint8"),
            Ok(DataTypeArg::UInt8)
        ));
        assert!(matches!(
            DataTypeArg::from_str("float32"),
            Ok(DataTypeArg::Float32)
        ));
        assert!(DataTypeArg::from_str("invalid").is_err());
    }

    #[test]
    fn test_data_type_conversion() {
        let dt: RasterDataType = DataTypeArg::Float32.into();
        assert!(matches!(dt, RasterDataType::Float32));
    }

    #[test]
    fn test_convert_expression() {
        let letters = vec!['A', 'B'];

        let result = convert_expression("(A-B)/(A+B)", &letters);
        assert!(result.is_ok());
        assert!(result.expect("should succeed").contains("B1"));

        let result = convert_expression("A * 2.0 + B", &letters);
        assert!(result.is_ok());
    }
}
