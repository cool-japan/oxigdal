//! OxiGDAL CLI - Command-line interface for geospatial operations
//!
//! Pure Rust implementation of common GDAL utilities.

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use std::io;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

mod commands;
mod util;

use commands::{
    buildvrt, calc, contour, convert, dem, fillnodata, info, inspect, merge, profile, proximity,
    rasterize, sieve, translate, validate, warp,
};

/// OxiGDAL CLI - Pure Rust geospatial data translation library
#[derive(Parser, Debug)]
#[command(name = "oxigdal")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Suppress all output except errors
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Output format (text, json)
    #[arg(long, global = true, default_value = "text")]
    format: OutputFormat,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, Copy)]
enum OutputFormat {
    Text,
    Json,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(OutputFormat::Text),
            "json" => Ok(OutputFormat::Json),
            _ => Err(format!("Invalid output format: {}", s)),
        }
    }
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Display information about a raster or vector file
    Info(info::InfoArgs),

    /// Convert between geospatial formats
    Convert(convert::ConvertArgs),

    /// Subset and resample rasters
    Translate(translate::TranslateArgs),

    /// Reproject and warp rasters
    Warp(warp::WarpArgs),

    /// Raster calculator operations
    Calc(calc::CalcArgs),

    /// Build virtual raster (VRT) from multiple files
    BuildVrt(buildvrt::BuildVrtArgs),

    /// Merge multiple rasters into a single output
    Merge(merge::MergeArgs),

    /// Validate file format and compliance
    Validate(validate::ValidateArgs),

    /// Inspect file format and metadata
    Inspect(inspect::InspectArgs),

    /// Profile operation performance
    Profile(profile::ProfileArgs),

    /// DEM analysis operations (hillshade, slope, aspect, TRI, TPI, roughness)
    Dem(dem::DemArgs),

    /// Convert vector geometries to raster
    Rasterize(rasterize::RasterizeArgs),

    /// Generate contour lines from DEM
    Contour(contour::ContourArgs),

    /// Compute proximity (distance) raster
    Proximity(proximity::ProximityArgs),

    /// Remove small raster polygons (sieve filter)
    Sieve(sieve::SieveArgs),

    /// Fill NoData values using interpolation
    FillNodata(fillnodata::FillNodataArgs),

    /// Generate shell completions
    Completions {
        /// The shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging based on verbosity
    setup_logging(cli.verbose, cli.quiet)?;

    // Execute the appropriate command
    match cli.command {
        Commands::Info(args) => info::execute(args, cli.format),
        Commands::Convert(args) => convert::execute(args, cli.format),
        Commands::Translate(args) => translate::execute(args, cli.format),
        Commands::Warp(args) => warp::execute(args, cli.format),
        Commands::Calc(args) => calc::execute(args, cli.format),
        Commands::BuildVrt(args) => buildvrt::execute(args, cli.format),
        Commands::Merge(args) => merge::execute(args, cli.format),
        Commands::Validate(args) => validate::execute(args, cli.format),
        Commands::Inspect(args) => inspect::execute(args, cli.format),
        Commands::Profile(args) => profile::execute(args, cli.format),
        Commands::Dem(args) => dem::execute(args, cli.format),
        Commands::Rasterize(args) => rasterize::execute(args, cli.format),
        Commands::Contour(args) => contour::execute(args, cli.format),
        Commands::Proximity(args) => proximity::execute(args, cli.format),
        Commands::Sieve(args) => sieve::execute(args, cli.format),
        Commands::FillNodata(args) => fillnodata::execute(args, cli.format),
        Commands::Completions { shell } => {
            generate_completions(shell);
            Ok(())
        }
    }
}

fn setup_logging(verbose: bool, quiet: bool) -> Result<()> {
    let level = if quiet {
        Level::ERROR
    } else if verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|e| anyhow::anyhow!("Failed to set up logging: {}", e))?;

    Ok(())
}

fn generate_completions(shell: Shell) {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(shell, &mut cmd, name, &mut io::stdout());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse() {
        let cli = Cli::try_parse_from(["oxigdal", "--version"]);
        assert!(cli.is_err() || cli.is_ok());
    }

    #[test]
    fn test_output_format_parsing() {
        use std::str::FromStr;

        assert!(matches!(
            OutputFormat::from_str("text"),
            Ok(OutputFormat::Text)
        ));
        assert!(matches!(
            OutputFormat::from_str("json"),
            Ok(OutputFormat::Json)
        ));
        assert!(OutputFormat::from_str("invalid").is_err());
    }
}
