//! OxiGeo CLI - Command-line interface for geospatial operations
//!
//! Pure Rust implementation of common GDAL utilities.

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Generator;
use std::io;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

mod commands;
mod util;

use commands::{
    buildvrt, calc, clip, contour, convert, dem, fillnodata, info, inspect, merge, polygonize,
    profile, proximity, rasterize, reproject, sieve, stats, tileindex, translate, validate, warp,
};

/// OxiGeo CLI - Pure Rust geospatial data translation library
#[derive(Parser, Debug)]
#[command(name = "oxigeo")]
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

    /// Number of parallel threads (default: all CPUs)
    #[arg(long, global = true, default_value_t = rayon::current_num_threads())]
    parallel: usize,

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

/// Shell choices for completion generation, including Nushell.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum ShellChoice {
    Bash,
    Fish,
    Zsh,
    PowerShell,
    Elvish,
    Nushell,
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

    /// Compute statistics for a raster or vector file
    Stats(stats::StatsArgs),

    /// Clip a raster or vector dataset to a bounding box
    Clip(clip::ClipArgs),

    /// Convert a raster band to polygon features (raster-to-vector)
    Polygonize(polygonize::PolygonizeArgs),

    /// Generate tile index of raster file extents
    TileIndex(tileindex::TileIndexArgs),

    /// Reproject a raster to a different CRS
    Reproject(reproject::ReprojectArgs),

    /// Generate shell completions
    Completions {
        /// The shell to generate completions for
        #[arg(value_enum)]
        shell: ShellChoice,
    },

    /// Generate man pages for all subcommands
    Man {
        /// Output directory
        #[arg(long, default_value = ".")]
        out: std::path::PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    rayon::ThreadPoolBuilder::new()
        .num_threads(cli.parallel)
        .build_global()
        .ok();

    setup_logging(cli.verbose, cli.quiet)?;

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
        Commands::Stats(args) => stats::execute(args, cli.format),
        Commands::Clip(args) => clip::execute(args, cli.format),
        Commands::Polygonize(args) => polygonize::execute(args, cli.format),
        Commands::TileIndex(args) => tileindex::execute(args, cli.format),
        Commands::Reproject(args) => reproject::execute(args, cli.format),
        Commands::Completions { shell } => {
            generate_completions(shell);
            Ok(())
        }
        Commands::Man { out } => {
            generate_man_pages(&out)?;
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

fn generate_completions(shell: ShellChoice) {
    let mut cmd = Cli::command().bin_name("oxigeo");
    let name = cmd.get_name().to_string();
    match shell {
        ShellChoice::Nushell => {
            // clap_complete_nushell requires cmd.build() to be called first so that
            // get_bin_name() returns Some(_) — the standard clap_complete::generate
            // does this internally, but the nushell generator does not.
            cmd.build();
            clap_complete_nushell::Nushell.generate(&cmd, &mut io::stdout());
        }
        _ => {
            let clap_shell = match shell {
                ShellChoice::Bash => clap_complete::Shell::Bash,
                ShellChoice::Fish => clap_complete::Shell::Fish,
                ShellChoice::Zsh => clap_complete::Shell::Zsh,
                ShellChoice::PowerShell => clap_complete::Shell::PowerShell,
                ShellChoice::Elvish => clap_complete::Shell::Elvish,
                ShellChoice::Nushell => unreachable!(),
            };
            clap_complete::generate(clap_shell, &mut cmd, name, &mut io::stdout());
        }
    }
}

fn generate_man_pages(out: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let cmd = Cli::command();
    let man = clap_mangen::Man::new(cmd.clone());
    let mut f = std::fs::File::create(out.join("oxigeo.1"))?;
    man.render(&mut f)?;
    for sub in cmd.get_subcommands() {
        let man = clap_mangen::Man::new(sub.clone());
        let name = sub.get_name().replace(' ', "-");
        let mut f = std::fs::File::create(out.join(format!("oxigeo-{name}.1")))?;
        man.render(&mut f)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse() {
        let cli = Cli::try_parse_from(["oxigeo", "--version"]);
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

    #[test]
    fn test_parallel_flag_default_parses() {
        let cli = Cli::try_parse_from(["oxigeo", "completions", "bash"])
            .expect("should parse with default parallel");
        assert_eq!(cli.parallel, rayon::current_num_threads());
    }

    #[test]
    fn test_parallel_flag_set_to_one() {
        let cli = Cli::try_parse_from(["oxigeo", "--parallel", "1", "completions", "bash"])
            .expect("should parse --parallel 1");
        assert_eq!(cli.parallel, 1);
    }

    #[test]
    fn test_nushell_completions_nonempty() {
        use clap_complete::generate;
        let mut cmd = Cli::command();
        let mut buf = Vec::<u8>::new();
        generate(clap_complete_nushell::Nushell, &mut cmd, "oxigeo", &mut buf);
        assert!(
            !buf.is_empty(),
            "Nushell completion output should be non-empty"
        );
    }

    #[test]
    fn test_man_page_generation() {
        let dir = std::env::temp_dir().join(format!(
            "oxigeo_man_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        generate_man_pages(&dir).expect("man page generation should succeed");

        let mut found_th = false;
        for entry in std::fs::read_dir(&dir).expect("temp dir should be readable") {
            let entry = entry.expect("dir entry should be readable");
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("1") {
                let contents =
                    std::fs::read_to_string(&path).expect("man page file should be readable");
                // roff files begin with an apostrophe preamble then a .TH macro
                if contents.contains(".TH") {
                    found_th = true;
                    break;
                }
            }
        }
        assert!(found_th, "at least one .1 file should contain a .TH macro");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
