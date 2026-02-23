# Getting Started with OxiGDAL

Quick start guide to get you up and running with OxiGDAL in minutes.

## Table of Contents

- [Installation](#installation)
- [Hello World](#hello-world)
- [Reading Raster Data](#reading-raster-data)
- [Working with Vectors](#working-with-vectors)
- [Common Tasks](#common-tasks)
- [Next Steps](#next-steps)

## Installation

### Prerequisites

- Rust 1.85 or later
- Cargo package manager
- Git (optional, for examples)

### Check Your Rust Installation

```bash
rustc --version
cargo --version
```

If not installed, follow [rustup.rs](https://rustup.rs/)

### Create a New Project

```bash
cargo new oxigdal-demo
cd oxigdal-demo
```

### Add OxiGDAL Dependencies

Add these to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-core = "0.1"
oxigdal-geotiff = "0.1"
tokio = { version = "1", features = ["full"] }
```

## Hello World

Create your first program by editing `src/main.rs`:

```rust
use oxigdal_core::io::FileDataSource;
use oxigdal_geotiff::GeoTiffReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("OxiGDAL Hello World!");

    // Open a GeoTIFF file
    let source = FileDataSource::open("sample.tif")?;
    let reader = GeoTiffReader::open(source)?;

    // Print basic information
    println!("File: sample.tif");
    println!("  Width: {} pixels", reader.width());
    println!("  Height: {} pixels", reader.height());
    println!("  Bands: {}", reader.band_count());

    if let Some(gt) = reader.geo_transform() {
        println!("  GeoTransform: {:?}", gt);
    }

    if let Some(epsg) = reader.epsg_code() {
        println!("  EPSG Code: {}", epsg);
    }

    Ok(())
}
```

Build and run:

```bash
cargo run --release
# Output:
# OxiGDAL Hello World!
# File: sample.tif
#   Width: 512 pixels
#   Height: 512 pixels
#   Bands: 1
#   GeoTransform: ...
#   EPSG Code: 4326
```

## Reading Raster Data

### Read a Complete Raster

```rust
use oxigdal_core::io::FileDataSource;
use oxigdal_geotiff::GeoTiffReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = FileDataSource::open("elevation.tif")?;
    let reader = GeoTiffReader::open(source)?;

    // Read all tiles into a single buffer
    let buffer = reader.read_tile_buffer(0, 0, 0)?;

    // Get raster information
    println!("Data type: {:?}", buffer.data_type());
    println!("Size: {}x{}", buffer.width(), buffer.height());

    // Calculate statistics
    let stats = buffer.compute_statistics()?;
    println!("Min: {:.2}, Max: {:.2}", stats.min, stats.max);
    println!("Mean: {:.2}, StdDev: {:.2}", stats.mean, stats.std_dev);

    // Access individual pixels
    let pixel_value = buffer.get_pixel(100, 100)?;
    println!("Pixel at (100, 100): {}", pixel_value);

    Ok(())
}
```

### Read a Data Window

```rust
use oxigdal_core::io::FileDataSource;
use oxigdal_geotiff::GeoTiffReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = FileDataSource::open("large_file.tif")?;
    let reader = GeoTiffReader::open(source)?;

    // Read full raster
    let full_buffer = reader.read_tile_buffer(0, 0, 0)?;

    // Extract a window (x, y, width, height)
    let window = full_buffer.window(100, 100, 256, 256)?;

    println!("Window size: {}x{}", window.width(), window.height());
    println!("Window mean: {}", window.compute_statistics()?.mean);

    Ok(())
}
```

### Iterate Over Pixels

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = FileDataSource::open("image.tif")?;
    let reader = GeoTiffReader::open(source)?;
    let buffer = reader.read_tile_buffer(0, 0, 0)?;

    let mut sum = 0.0;
    let mut count = 0;

    // Iterate over all pixels
    for pixel in buffer.iter() {
        sum += pixel;
        count += 1;
    }

    let mean = sum / count as f64;
    println!("Calculated mean: {}", mean);

    Ok(())
}
```

## Writing Raster Data

### Create and Write a GeoTIFF

```rust
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{RasterDataType, GeoTransform, BoundingBox};
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a simple raster
    let width = 512u32;
    let height = 512u32;
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    // Fill with data
    for y in 0..height {
        for x in 0..width {
            let value = (x as f64 / width as f64 + y as f64 / height as f64) * 100.0;
            buffer.set_pixel(x, y, value)?;
        }
    }

    // Create output file
    let file = File::create("output.tif")?;

    // Set up writer options
    let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)?;
    let geo_transform = GeoTransform::from_bounds(&bbox, width, height)?;

    let options = GeoTiffWriterOptions {
        geo_transform: Some(geo_transform),
        epsg_code: Some(4326),  // WGS84
        ..Default::default()
    };

    // Write file
    let writer = GeoTiffWriter::new(file, options)?;
    writer.write_buffer(&buffer)?;

    println!("Written output.tif");
    Ok(())
}
```

## Working with Vectors

### Read GeoJSON

```rust
use geojson::GeoJson;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let geojson_str = fs::read_to_string("data.geojson")?;
    let geojson = geojson_str.parse::<GeoJson>()?;

    if let GeoJson::FeatureCollection(fc) = geojson {
        println!("Features: {}", fc.features.len());

        for feature in &fc.features {
            if let Some(props) = &feature.properties {
                println!("Properties: {:?}", props);
            }

            if let Some(geom) = &feature.geometry {
                println!("Geometry type: {}", geom.value);
            }
        }
    }

    Ok(())
}
```

### Work with Geometries

```rust
use geo::geometry::{Point, LineString, Polygon};
use geo::{Area, Contains, Distance};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a point
    let point = Point::new(0.0, 0.0);

    // Create a polygon
    let poly = Polygon::new(
        LineString::from(vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]),
        vec![],
    );

    // Geometric operations
    println!("Polygon area: {}", poly.unsigned_area());
    println!("Contains point: {}", poly.contains(&point));
    println!("Distance from point: {}", poly.exterior_ring_distance(&point));

    Ok(())
}
```

## Common Tasks

### Task 1: Calculate NDVI

```rust
use oxigdal_core::io::FileDataSource;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use oxigdal_geotiff::GeoTiffReader;

fn calculate_ndvi(nir_path: &str, red_path: &str)
    -> Result<RasterBuffer, Box<dyn std::error::Error>>
{
    // Read NIR band
    let nir_source = FileDataSource::open(nir_path)?;
    let nir_reader = GeoTiffReader::open(nir_source)?;
    let nir = nir_reader.read_tile_buffer(0, 0, 0)?;

    // Read RED band
    let red_source = FileDataSource::open(red_path)?;
    let red_reader = GeoTiffReader::open(red_source)?;
    let red = red_reader.read_tile_buffer(0, 0, 0)?;

    // Calculate NDVI
    let mut ndvi = RasterBuffer::zeros(
        nir.width(),
        nir.height(),
        RasterDataType::Float32
    );

    for y in 0..nir.height() {
        for x in 0..nir.width() {
            let n = nir.get_pixel(x, y)?;
            let r = red.get_pixel(x, y)?;
            let ndvi_val = (n - r) / (n + r + 1e-10);
            ndvi.set_pixel(x, y, ndvi_val)?;
        }
    }

    Ok(ndvi)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ndvi = calculate_ndvi("nir.tif", "red.tif")?;
    println!("NDVI calculated: {}x{}", ndvi.width(), ndvi.height());
    Ok(())
}
```

### Task 2: Resample Raster

```rust
use oxigdal_core::io::FileDataSource;
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_algorithms::resample::{Resampling, ResampleOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read input
    let source = FileDataSource::open("high_res.tif")?;
    let reader = GeoTiffReader::open(source)?;
    let buffer = reader.read_tile_buffer(0, 0, 0)?;

    // Resample to lower resolution
    let options = ResampleOptions {
        width: 256,
        height: 256,
        resampling: Resampling::Bilinear,
    };

    let resampled = buffer.resample(&options)?;
    println!("Resampled to {}x{}", resampled.width(), resampled.height());

    Ok(())
}
```

### Task 3: Reproject Data

```rust
use oxigdal_proj::Projection;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Define source and destination CRS
    let from = Projection::from_epsg(4326)?;  // WGS84 (lat/lon)
    let to = Projection::from_epsg(3857)?;    // Web Mercator

    // Transform a point
    let (lon, lat) = (10.0, 20.0);
    let (x, y) = from.transform_point(lon, lat, &to)?;

    println!("WGS84: ({}, {})", lon, lat);
    println!("Web Mercator: ({}, {})", x, y);

    Ok(())
}
```

### Task 4: Cloud Data Access

```rust
use oxigdal_cloud::backends::HttpBackend;
use oxigdal_cloud::retry::RetryConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let retry_config = RetryConfig::default();
    let http = HttpBackend::new(retry_config);

    // Fetch remote GeoTIFF
    let url = "https://example.com/data/imagery.tif";
    let data = http.get(url).await?;

    println!("Downloaded {} bytes", data.len());

    Ok(())
}
```

### Task 5: Parallel Processing

```rust
use rayon::prelude::*;
use oxigdal_core::io::FileDataSource;
use oxigdal_geotiff::GeoTiffReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = FileDataSource::open("large_image.tif")?;
    let reader = GeoTiffReader::open(source)?;
    let buffer = reader.read_tile_buffer(0, 0, 0)?;

    // Process image in parallel tiles
    let tile_size = 256u32;
    let num_tiles_x = (buffer.width() + tile_size - 1) / tile_size;
    let num_tiles_y = (buffer.height() + tile_size - 1) / tile_size;

    let results: Vec<_> = (0..num_tiles_y)
        .flat_map(|ty| (0..num_tiles_x).map(move |tx| (tx, ty)))
        .par_iter()
        .map(|(tx, ty)| {
            let x = tx * tile_size;
            let y = ty * tile_size;
            let w = std::cmp::min(tile_size, buffer.width() - x);
            let h = std::cmp::min(tile_size, buffer.height() - y);

            // Process this tile
            (x, y, w, h)
        })
        .collect();

    println!("Processed {} tiles in parallel", results.len());
    Ok(())
}
```

## Error Handling

All OxiGDAL operations return `Result<T, OxiGdalError>`. Use the `?` operator for ergonomic error propagation:

```rust
fn process_file(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let source = FileDataSource::open(path)?;  // ? unwraps or propagates error
    let reader = GeoTiffReader::open(source)?;
    let buffer = reader.read_tile_buffer(0, 0, 0)?;
    Ok(())
}

fn main() {
    match process_file("input.tif") {
        Ok(()) => println!("Success!"),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

## Project Structure

Recommended organization for larger projects:

```
my_gis_app/
├── Cargo.toml
├── src/
│   ├── main.rs              # Application entry point
│   ├── lib.rs               # Reusable library code
│   ├── raster/
│   │   ├── mod.rs
│   │   ├── processing.rs    # Raster operations
│   │   └── stats.rs         # Statistics
│   ├── vector/
│   │   ├── mod.rs
│   │   └── operations.rs    # Vector operations
│   └── io/
│       ├── mod.rs
│       ├── readers.rs       # File readers
│       └── writers.rs       # File writers
├── tests/
│   └── integration_tests.rs
├── examples/
│   ├── ndvi.rs
│   ├── reproject.rs
│   └── cloud_fetch.rs
└── data/
    └── sample.tif
```

## Cargo Features

Enable specific features for your use case:

```toml
[dependencies]
oxigdal-core = { version = "0.1", features = ["std", "arrow"] }
oxigdal-geotiff = "0.1"
oxigdal-cloud = { version = "0.1", features = ["s3", "http"] }
oxigdal-algorithms = { version = "0.1", features = ["simd"] }
```

## Building for Performance

```bash
# Development build (faster compilation)
cargo build

# Release build (optimized for speed)
cargo build --release

# With native CPU optimizations
RUSTFLAGS="-C target-cpu=native" cargo build --release

# With SIMD support
RUSTFLAGS="-C target-feature=+avx2" cargo build --release
```

## Testing

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_ndvi

# Run tests in release mode
cargo test --release

# Run tests with all features
cargo test --all-features
```

## Debugging

Print debugging information:

```rust
use oxigdal_core::io::FileDataSource;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = FileDataSource::open("image.tif")?;

    // Print debug info
    dbg!(&source);

    Ok(())
}
```

Run with debug output:

```bash
RUST_LOG=debug cargo run
```

## Next Steps

1. **Read the Migration Guides**
   - [MIGRATION_FROM_GDAL.md](MIGRATION_FROM_GDAL.md)
   - [PYTHON_TO_RUST.md](PYTHON_TO_RUST.md)

2. **Explore the Examples**
   - Check the `examples/` directory in the repository
   - Try the tutorial examples

3. **Learn More**
   - [API Comparison](API_COMPARISON.md)
   - [Architecture Overview](ARCHITECTURE.md)
   - [Performance Guide](PERFORMANCE_GUIDE.md)
   - [Best Practices](BEST_PRACTICES.md)

4. **Get Help**
   - Read the [Troubleshooting Guide](TROUBLESHOOTING.md)
   - Check [GitHub Issues](https://github.com/cool-japan/oxigdal/issues)
   - Join COOLJAPAN Community

## Resources

- **Official Documentation**: https://docs.rs/oxigdal
- **Rust Book**: https://doc.rust-lang.org/book/
- **Geospatial Standards**: https://www.ogc.org/
- **GDAL API Reference**: https://gdal.org/api/

Happy geospatial computing with OxiGDAL!
