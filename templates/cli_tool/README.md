# CLI Tool Template for OxiGDAL

A project template for building command-line geospatial processing tools powered by OxiGDAL.

## What This Template Provides

- Pre-configured CLI argument parsing with [clap](https://docs.rs/clap) (derive API)
- Progress bar support via [indicatif](https://docs.rs/indicatif) and styled terminal output via [console](https://docs.rs/console)
- OxiGDAL core, algorithms, and GeoTIFF driver dependencies ready to use
- Structured error handling with `anyhow` and `thiserror`
- Subcommand-based CLI structure (easily extensible)

## Getting Started

1. Copy this template directory to your workspace
2. Update `Cargo.toml` with your project name, authors, and any additional dependencies
3. Implement your subcommands in `src/main.rs`
4. Build and run:

```sh
cargo build --release
./target/release/my-cli process input.tif output.tif
```

## Example Usage

```rust
use clap::{Parser, Subcommand};
use anyhow::Result;

#[derive(Parser)]
#[command(name = "my-cli")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Process { input: String, output: String },
    Info { file: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // Handle commands...
    Ok(())
}
```

## Extending the Template

- Add new subcommands by extending the `Commands` enum
- Add driver dependencies (`oxigdal-geojson`, `oxigdal-shapefile`, etc.) as needed
- Use `indicatif::ProgressBar` for long-running operations
- Add logging with `tracing` and `tracing-subscriber`

## License

Apache-2.0

Part of the [OxiGDAL](https://github.com/cool-japan/oxigdal) project by COOLJAPAN OU.
