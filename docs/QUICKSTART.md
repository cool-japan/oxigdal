# OxiGDAL Quickstart Guide

Welcome to OxiGDAL - a pure Rust reimplementation of GDAL for cloud-native geospatial computing!

## What is OxiGDAL?

OxiGDAL is a modern, pure-Rust geospatial data processing library that provides:

- **Cloud-Native**: Optimized for Cloud Optimized GeoTIFF (COG) and cloud storage
- **Pure Rust**: No C/C++/Fortran dependencies - 100% memory-safe Rust
- **High Performance**: SIMD optimizations, parallel processing, zero-copy where possible
- **WebAssembly**: Run geospatial processing in the browser
- **Production Ready**: Comprehensive error handling, no unwrap(), extensive testing

## Installation

Add OxiGDAL to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-core = "0.1"
oxigdal-algorithms = "0.1"
oxigdal-geotiff = "0.1"
oxigdal-geojson = "0.1"
oxigdal-geoparquet = "0.1"
oxigdal-zarr = "0.1"
oxigdal-flatgeobuf = "0.1"
```

Or for the most common use case (GeoTIFF/COG):

```toml
[dependencies]
oxigdal-core = "0.1"
oxigdal-geotiff = "0.1"
oxigdal-algorithms = "0.1"
```

## Quick Start Examples

### Reading a Cloud Optimized GeoTIFF (COG)

```rust
use oxigdal_geotiff::CogReader;
use oxigdal_core::io::FileDataSource;

fn read_cog_example() -> Result<(), Box<dyn std::error::Error>> {
    // Open the COG file
    let source = FileDataSource::open("image.tif")?;
    let reader = CogReader::open(source)?;

    // Get image metadata
    println!("Image size: {}x{}", reader.width(), reader.height());
    println!("Tile size: {:?}", reader.tile_size());
    println!("Overview count: {}", reader.overview_count());
    println!("EPSG code: {:?}", reader.epsg_code());

    // Read a tile (level 0, tile x=0, y=0)
    let tile_data = reader.read_tile(0, 0, 0)?;
    println!("Read {} bytes", tile_data.len());

    Ok(())
}
```

### Reading GeoJSON

```rust
use oxigdal_geojson::{GeoJsonReader, FeatureCollection};
use std::fs::File;
use std::io::BufReader;

fn read_geojson_example() -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open("features.geojson")?;
    let reader = BufReader::new(file);

    let mut geojson_reader = GeoJsonReader::new(reader);
    let collection = geojson_reader.read_feature_collection()?;

    println!("Read {} features", collection.features.len());

    for feature in &collection.features {
        if let Some(geom) = &feature.geometry {
            println!("Feature type: {:?}", geom.geometry_type);
        }
    }

    Ok(())
}
```

### Writing GeoJSON

```rust
use oxigdal_geojson::{GeoJsonWriter, FeatureCollection, Feature, Geometry, GeometryType};
use std::fs::File;
use std::io::BufWriter;

fn write_geojson_example() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let output_path = temp_dir.join("output.geojson");

    // Create a feature collection
    let mut collection = FeatureCollection {
        features: vec![],
        bbox: None,
        foreign_members: Default::default(),
    };

    // Add a point feature
    let point = Feature {
        geometry: Some(Geometry {
            geometry_type: GeometryType::Point,
            coordinates: vec![vec![-122.4194, 37.7749]], // San Francisco
            bbox: None,
        }),
        properties: Default::default(),
        id: None,
        bbox: None,
        foreign_members: Default::default(),
    };

    collection.features.push(point);

    // Write to file
    let file = File::create(&output_path)?;
    let writer = BufWriter::new(file);
    let mut geojson_writer = GeoJsonWriter::new(writer);
    geojson_writer.write_feature_collection(&collection)?;

    println!("Wrote GeoJSON to {:?}", output_path);
    Ok(())
}
```

### Reading GeoParquet

```rust
use oxigdal_geoparquet::GeoParquetReader;
use std::fs::File;

fn read_geoparquet_example() -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open("data.parquet")?;
    let reader = GeoParquetReader::new(file)?;

    // Get metadata
    let metadata = reader.metadata()?;
    println!("Columns: {}", metadata.schema.fields().len());
    println!("Geometry column: {:?}", metadata.primary_column);

    // Read features (returns Arrow record batches)
    let batches = reader.read_all()?;
    println!("Read {} batches", batches.len());

    Ok(())
}
```

### Resampling Raster Data

```rust
use oxigdal_algorithms::resampling::{Resampler, ResamplingMethod};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

fn resample_example() -> Result<(), Box<dyn std::error::Error>> {
    // Create a source raster (1000x1000)
    let src = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);

    // Resample to 500x500 using bilinear interpolation
    let resampler = Resampler::new(ResamplingMethod::Bilinear);
    let dst = resampler.resample(&src, 500, 500)?;

    println!("Resampled from {}x{} to {}x{}",
        src.width(), src.height(),
        dst.width(), dst.height());

    Ok(())
}
```

### Working with Bounding Boxes

```rust
use oxigdal_core::types::{BoundingBox, GeoTransform};

fn bbox_example() -> Result<(), Box<dyn std::error::Error>> {
    // Create a bounding box (Web Mercator extent for San Francisco)
    let bbox = BoundingBox::new(-122.5, 37.7, -122.3, 37.8)?;

    println!("Bounds: {:?}", bbox);
    println!("Width: {}, Height: {}", bbox.width(), bbox.height());
    println!("Center: {:?}", bbox.center());

    // Create a geotransform from bounds
    let gt = GeoTransform::from_bounds(&bbox, 1000, 1000)?;
    println!("Resolution: {:?}", gt.resolution());

    // Convert pixel to geo coordinates
    let (geo_x, geo_y) = gt.pixel_to_geo(500.0, 500.0);
    println!("Center pixel maps to: ({}, {})", geo_x, geo_y);

    Ok(())
}
```

## WebAssembly Usage

OxiGDAL can run in the browser via WebAssembly:

```javascript
import init, { WasmCogViewer } from 'oxigdal-wasm';

async function viewCog(url) {
    await init();

    const viewer = new WasmCogViewer();
    await viewer.open(url);

    console.log(`Image size: ${viewer.width()}x${viewer.height()}`);
    console.log(`Tile size: ${viewer.tile_width()}x${viewer.tile_height()}`);
    console.log(`EPSG: ${viewer.epsg_code()}`);

    // Read a tile as ImageData for canvas
    const imageData = await viewer.read_tile_as_image_data(0, 0, 0);

    // Draw on canvas
    const canvas = document.getElementById('map');
    const ctx = canvas.getContext('2d');
    ctx.putImageData(imageData, 0, 0);
}
```

## Error Handling

OxiGDAL uses comprehensive error handling with descriptive errors:

```rust
use oxigdal_geotiff::CogReader;
use oxigdal_core::error::OxiGdalError;
use oxigdal_core::io::FileDataSource;

fn error_handling_example() {
    match FileDataSource::open("nonexistent.tif") {
        Ok(source) => {
            match CogReader::open(source) {
                Ok(reader) => println!("Success: {}x{}", reader.width(), reader.height()),
                Err(e) => eprintln!("Failed to parse TIFF: {}", e),
            }
        }
        Err(e) => eprintln!("Failed to open file: {}", e),
    }
}
```

## Feature Flags

Each crate supports various feature flags:

### oxigdal-core
- `std` (default) - Standard library support
- `alloc` - Allocation support without std
- `arrow` - Apache Arrow integration
- `async` - Async I/O traits

### oxigdal-geotiff
- `deflate` (default) - DEFLATE/zlib compression
- `lzw` (default) - LZW compression
- `zstd` - ZSTD compression
- `jpeg` - JPEG compression (planned)

### oxigdal-algorithms
- `simd` - SIMD optimizations (AVX2, NEON)
- `parallel` - Parallel processing with rayon

## Performance Tips

1. **Use appropriate resampling methods**:
   - `Nearest` for categorical data (fastest)
   - `Bilinear` for continuous data (balanced)
   - `Bicubic` or `Lanczos` for high-quality imagery (slower)

2. **Enable SIMD optimizations**:
   ```toml
   [dependencies]
   oxigdal-algorithms = { version = "0.1", features = ["simd"] }
   ```

3. **Use zero-copy where possible**:
   - Enable `arrow` feature for Arrow integration
   - Use `RasterBuffer::as_bytes()` instead of copying

4. **For cloud data, use range requests**:
   - COG format is optimized for HTTP range requests
   - Only necessary tiles are fetched

## Next Steps

- Read the [Driver Guide](oxigdal_driver_guide.md) for detailed driver documentation
- Read the [Algorithm Guide](oxigdal_algorithm_guide.md) for processing algorithms
- Read the [WASM Guide](oxigdal_wasm_guide.md) for browser usage
- Check the examples in the repository

## Getting Help

- **Documentation**: https://docs.rs/oxigdal
- **Repository**: https://github.com/cool-japan/oxigdal
- **Issues**: https://github.com/cool-japan/oxigdal/issues

## License

OxiGDAL is licensed under Apache-2.0.

Copyright © 2025 COOLJAPAN OU (Team Kitasan)
