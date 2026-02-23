# Getting Started with OxiGDAL

## Introduction

OxiGDAL is a Pure Rust geospatial data abstraction library providing high-performance, memory-safe access to raster and vector geospatial data. This tutorial will guide you through the basics of using OxiGDAL.

## Prerequisites

- Rust 1.85 or later
- Basic understanding of geospatial concepts
- Familiarity with Rust async programming

## Installation

Add OxiGDAL to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-core = "0.1"
tokio = { version = "1", features = ["full"] }
```

For specific format support, add the corresponding driver crates:

```toml
oxigdal-geotiff = "0.1"
oxigdal-geojson = "0.1"
oxigdal-geoparquet = "0.1"
```

## Core Concepts

### Datasets

A dataset represents a single geospatial data source. OxiGDAL supports both raster and vector datasets.

```rust
use oxigdal_core::Dataset;
use std::path::Path;

async fn open_dataset() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(Path::new("data.tif")).await?;
    println!("Dataset opened: {} x {}", dataset.width(), dataset.height());
    Ok(())
}
```

### Raster Bands

Raster datasets contain one or more bands representing different data channels.

```rust
use oxigdal_core::{Dataset, DataType};

async fn read_band() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("image.tif").await?;
    let band = dataset.band(1)?;

    let width = band.width();
    let height = band.height();
    let data_type = band.data_type();

    println!("Band: {} x {}, type: {:?}", width, height, data_type);
    Ok(())
}
```

### Vector Layers

Vector datasets contain layers with features (points, lines, polygons).

```rust
use oxigdal_core::Dataset;

async fn read_vector() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("shapes.geojson").await?;
    let layer = dataset.layer(0)?;

    println!("Layer: {}", layer.name());
    println!("Feature count: {}", layer.feature_count()?);

    for feature in layer.features()? {
        let geometry = feature.geometry()?;
        println!("Geometry type: {:?}", geometry.geometry_type());
    }

    Ok(())
}
```

## Reading Raster Data

### Reading a Whole Band

```rust
use oxigdal_core::{Dataset, RasterBand};

async fn read_full_band() -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let dataset = Dataset::open("elevation.tif").await?;
    let band = dataset.band(1)?;

    let width = band.width() as usize;
    let height = band.height() as usize;
    let mut buffer = vec![0.0f32; width * height];

    band.read_block(0, 0, width, height, &mut buffer).await?;

    Ok(buffer)
}
```

### Reading a Subset

```rust
async fn read_subset() -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let dataset = Dataset::open("large.tif").await?;
    let band = dataset.band(1)?;

    // Read 100x100 pixels starting at (500, 500)
    let x_offset = 500;
    let y_offset = 500;
    let x_size = 100;
    let y_size = 100;

    let mut buffer = vec![0.0f32; x_size * y_size];
    band.read_block(x_offset, y_offset, x_size, y_size, &mut buffer).await?;

    Ok(buffer)
}
```

## Writing Raster Data

### Creating a New GeoTIFF

```rust
use oxigdal_core::{Dataset, Driver, DataType};
use oxigdal_geotiff::GeoTiffDriver;

async fn create_geotiff() -> Result<(), Box<dyn std::error::Error>> {
    let driver = GeoTiffDriver::new();

    let mut dataset = driver.create(
        "output.tif",
        512,  // width
        512,  // height
        1,    // bands
        DataType::Float32,
    ).await?;

    // Write data
    let data = vec![0.0f32; 512 * 512];
    let band = dataset.band_mut(1)?;
    band.write_block(0, 0, 512, 512, &data).await?;

    // Set geotransform
    dataset.set_geo_transform([0.0, 1.0, 0.0, 0.0, 0.0, -1.0])?;

    dataset.flush().await?;
    Ok(())
}
```

## Spatial Reference Systems

### Setting Projection

```rust
use oxigdal_core::Dataset;
use oxigdal_proj::SpatialRef;

async fn set_projection() -> Result<(), Box<dyn std::error::Error>> {
    let mut dataset = Dataset::create("output.tif", 100, 100, 1).await?;

    // Set WGS84 (EPSG:4326)
    let srs = SpatialRef::from_epsg(4326)?;
    dataset.set_spatial_ref(&srs)?;

    Ok(())
}
```

### Reprojecting Data

```rust
use oxigdal_proj::{SpatialRef, Transformer};

async fn reproject_dataset() -> Result<(), Box<dyn std::error::Error>> {
    let src_dataset = Dataset::open("input.tif").await?;
    let src_srs = src_dataset.spatial_ref()?;

    let dst_srs = SpatialRef::from_epsg(3857)?; // Web Mercator

    let transformer = Transformer::new(&src_srs, &dst_srs)?;
    let dst_dataset = transformer.transform_dataset(&src_dataset).await?;

    dst_dataset.save("reprojected.tif").await?;
    Ok(())
}
```

## Error Handling

OxiGDAL uses `Result` types for error handling. Always handle errors appropriately:

```rust
use oxigdal_core::{Dataset, OxiGdalError};

async fn robust_open() -> Result<(), OxiGdalError> {
    match Dataset::open("data.tif").await {
        Ok(dataset) => {
            println!("Successfully opened dataset");
            Ok(())
        }
        Err(OxiGdalError::FileNotFound(path)) => {
            eprintln!("File not found: {}", path);
            Err(OxiGdalError::FileNotFound(path))
        }
        Err(OxiGdalError::UnsupportedFormat(format)) => {
            eprintln!("Unsupported format: {}", format);
            Err(OxiGdalError::UnsupportedFormat(format))
        }
        Err(e) => {
            eprintln!("Error opening dataset: {}", e);
            Err(e)
        }
    }
}
```

## Best Practices

1. **Resource Management**: Use RAII pattern for automatic cleanup
2. **Async Operations**: Leverage async/await for I/O operations
3. **Error Handling**: Never use `unwrap()` in production code
4. **Memory Efficiency**: Process data in chunks for large datasets
5. **Type Safety**: Use strong types for coordinates and units

## Complete Example

```rust
use oxigdal_core::{Dataset, DataType};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open input dataset
    let input = Dataset::open(Path::new("input.tif")).await?;

    // Get metadata
    let width = input.width();
    let height = input.height();
    let band_count = input.band_count();

    println!("Input: {} x {} x {} bands", width, height, band_count);

    // Read first band
    let band = input.band(1)?;
    let mut data = vec![0.0f32; (width * height) as usize];
    band.read_block(0, 0, width as usize, height as usize, &mut data).await?;

    // Process data (simple scaling)
    for pixel in data.iter_mut() {
        *pixel *= 2.0;
    }

    // Create output dataset
    let mut output = Dataset::create("output.tif", width, height, 1).await?;
    output.set_geo_transform(input.geo_transform()?)?;
    output.set_spatial_ref(&input.spatial_ref()?)?;

    // Write processed data
    let out_band = output.band_mut(1)?;
    out_band.write_block(0, 0, width as usize, height as usize, &data).await?;

    output.flush().await?;
    println!("Processing complete!");

    Ok(())
}
```

## Next Steps

- Read [Tutorial 02: Reading Rasters](02_reading_rasters.md) for advanced raster operations
- Learn about [Vector Data](04_vector_data.md) processing
- Explore [Cloud Storage](06_cloud_storage.md) integration
- Check out [Performance Tuning](09_performance_tuning.md)

## Common Issues

### Issue: Dataset fails to open

**Solution**: Ensure the file exists and the appropriate driver crate is included in dependencies.

### Issue: Out of memory errors

**Solution**: Process data in tiles or use streaming operations for large datasets.

### Issue: Incorrect projection

**Solution**: Verify the spatial reference using `dataset.spatial_ref()` and reproject if needed.

## Resources

- [OxiGDAL API Documentation](https://docs.rs/oxigdal)
- [GitHub Repository](https://github.com/cool-japan/oxigdal)
- [Format Drivers Documentation](../../DRIVERS.md)
- [Performance Guide](../../PERFORMANCE_GUIDE.md)

---

Copyright (c) 2025 COOLJAPAN OU (Team Kitasan)
Licensed under Apache-2.0
