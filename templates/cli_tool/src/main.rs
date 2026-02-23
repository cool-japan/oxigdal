//! OxiGDAL CLI Tool Template

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "my-cli")]
#[command(about = "My OxiGDAL CLI tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Process a file
    Process {
        /// Input file
        input: String,
        /// Output file
        output: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Process { input, output } => {
            println!("Processing {} -> {}", input, output);
            // Process file here
            Ok(())
        }
    }
}
