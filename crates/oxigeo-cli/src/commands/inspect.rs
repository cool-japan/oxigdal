//! File inspection command

use anyhow::Result;
use clap::Args;

/// Inspect a geospatial file
#[derive(Args, Debug)]
pub struct InspectArgs {
    /// Input file path
    #[arg(value_name = "INPUT")]
    pub input: String,

    /// Show detailed information
    #[arg(long, short = 'd')]
    pub detailed: bool,
}

/// Execute inspect command
pub fn execute(_args: InspectArgs, _output_format: crate::OutputFormat) -> Result<()> {
    let report = crate::util::inspect_file(&_args.input, _args.detailed)?;

    match _output_format {
        crate::OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&report)?;
            println!("{json}");
        }
        crate::OutputFormat::Text => {
            print_text_report(&report, _args.detailed);
        }
    }

    Ok(())
}

/// Renders an `InspectionReport` as a human-readable multi-line summary.
fn print_text_report(report: &crate::util::InspectionReport, detailed: bool) {
    use crate::util::format_size;
    use console::style;

    println!("{}", style("File Inspection").bold().cyan());
    println!("  Path:      {}", report.path);
    println!("  Format:    {}", report.format);
    println!("  Extension: {}", report.extension);
    if report.is_cloud {
        println!("  Location:  cloud URI");
        println!("  Size:      unknown (cloud)");
    } else {
        println!("  Location:  local file");
        println!("  Size:      {}", format_size(report.file_size));
    }
    if let Some(ref crs) = report.crs {
        println!("  CRS:       {crs}");
    }

    if let Some(ref raster) = report.raster {
        print_raster_section(raster, detailed);
    }

    if let Some(ref vector) = report.vector {
        print_vector_section(vector, detailed);
    }
}

/// Renders the raster portion of an inspection report.
fn print_raster_section(raster: &crate::util::RasterSummary, detailed: bool) {
    use console::style;

    println!();
    println!("{}", style("Raster Structure").bold().cyan());
    println!("  Dimensions: {} x {}", raster.width, raster.height);
    println!("  Bands:      {}", raster.band_count);
    if !raster.data_types.is_empty() {
        println!("  Data Types: {}", raster.data_types.join(", "));
    }
    if detailed {
        if let Some(gt) = raster.geo_transform {
            println!(
                "  GeoTransform: [{}, {}, {}, {}, {}, {}]",
                gt[0], gt[1], gt[2], gt[3], gt[4], gt[5]
            );
        }
        match raster.nodata {
            Some(nd) => println!("  NoData:     {nd}"),
            None => println!("  NoData:     (none)"),
        }
    }
}

/// Renders the vector portion of an inspection report.
fn print_vector_section(vector: &crate::util::VectorSummary, detailed: bool) {
    use console::style;

    println!();
    println!("{}", style("Vector Structure").bold().cyan());
    match vector.feature_count {
        Some(count) => println!("  Features:   {count}"),
        None => println!("  Features:   (unknown)"),
    }
    if detailed {
        println!("  Layers:     {}", vector.layer_count);
    }
    if let Some(bounds) = vector.bounds {
        println!(
            "  Bounds:     [{}, {}, {}, {}]",
            bounds[0], bounds[1], bounds[2], bounds[3]
        );
    }
}
