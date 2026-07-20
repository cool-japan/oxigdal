# Basic OxiGeo Project

This is a template for a basic OxiGeo project.

## Getting Started

1. Update `Cargo.toml` with your project name and details
2. Implement your geospatial processing logic in `src/main.rs`
3. Run with `cargo run`

## Features

- Read/write GeoTIFF files
- Process raster data with algorithms
- Export to various formats

## Example Usage

```rust
use oxigeo_core::Dataset;
use oxigeo_geotiff;

fn main() -> Result<()> {
    // Read raster
    let dataset = oxigeo_geotiff::read("input.tif")?;

    // Process data
    // ...

    // Write output
    oxigeo_geotiff::write("output.tif", &dataset)?;

    Ok(())
}
```

## License

Apache-2.0
