//! Reproject command - thin wrapper over warp for common CRS transformations

use crate::OutputFormat;
use crate::commands::warp::{self, ResamplingMethodArg, WarpArgs};
use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

/// Reproject a raster to a different CRS
#[derive(Args, Debug)]
pub struct ReprojectArgs {
    /// Input raster
    pub input: PathBuf,

    /// Output raster
    pub output: PathBuf,

    /// Target CRS (e.g. EPSG:3857)
    #[arg(long)]
    pub to: String,

    /// Source CRS override (auto-detect if not set)
    #[arg(long)]
    pub from: Option<String>,

    /// Resampling method (nearest, bilinear, bicubic, lanczos)
    #[arg(short, long, default_value = "bilinear")]
    pub resampling: ResamplingMethodArg,

    /// Output resolution in target units
    #[arg(long)]
    pub resolution: Option<f64>,

    /// Overwrite existing output file
    #[arg(long)]
    pub overwrite: bool,
}

pub fn execute(args: ReprojectArgs, format: OutputFormat) -> Result<()> {
    let warp_args = WarpArgs {
        input: args.input,
        output: args.output,
        s_srs: args.from,
        t_srs: args.to,
        ts_x: None,
        ts_y: None,
        tr: args.resolution,
        resampling: args.resampling,
        te: None,
        no_data: None,
        overwrite: args.overwrite,
        progress: true,
        creation_options: Vec::new(),
    };
    warp::execute(warp_args, format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: TestCmd,
    }

    #[derive(clap::Subcommand)]
    enum TestCmd {
        Reproject(ReprojectArgs),
    }

    #[test]
    fn test_reproject_missing_to_flag_errors() {
        let result = TestCli::try_parse_from(["test", "reproject", "in.tif", "out.tif"]);
        assert!(result.is_err(), "missing --to should be a parse error");
    }

    #[test]
    fn test_reproject_parses_with_to_flag() {
        let result = TestCli::try_parse_from([
            "test",
            "reproject",
            "in.tif",
            "out.tif",
            "--to",
            "EPSG:3857",
        ]);
        assert!(result.is_ok());
        if let Ok(cli) = result {
            let TestCmd::Reproject(args) = cli.cmd;
            assert_eq!(args.to, "EPSG:3857");
        }
    }
}
