# oxigdal-vrt

[![Crates.io](https://img.shields.io/crates/v/oxigdal-vrt.svg)](https://crates.io/crates/oxigdal-vrt)
[![Documentation](https://docs.rs/oxigdal-vrt/badge.svg)](https://docs.rs/oxigdal-vrt)
[![License](https://img.shields.io/crates/l/oxigdal-vrt.svg)](LICENSE)

Pure Rust VRT (Virtual Raster) driver for OxiGDAL - create mosaics, transformations, and virtual datasets without copying data.

## Overview

VRT (Virtual Raster) is an XML-based format that references other raster files without copying data. This driver provides a pure Rust implementation enabling efficient multi-file processing and on-the-fly transformations.

## Features

- **Pure Rust** - 100% Pure Rust implementation, no GDAL dependency
- **Mosaicking** - Combine multiple tiles into a single virtual dataset
- **Band Subsetting** - Extract specific bands from multi-band rasters
- **On-the-fly Transformations** - Apply scaling, offset, and pixel functions
- **Windowing** - Create virtual subsets of large rasters
- **XML-based** - Human-readable XML format for easy editing
- **Zero-copy** - References source files without data duplication
- **Async I/O** - Optional async support for non-blocking operations
- **No unwrap policy** - All operations return `Result<T, E>`

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-vrt = "0.1.0"
```

### Feature Flags

- `std` (default) - Enable standard library support
- `async` - Enable async I/O with Tokio

Example with async support:

```toml
[dependencies]
oxigdal-vrt = { version = "0.1.0", features = ["async"] }
```

## Quick Start

### Creating a VRT Mosaic

```rust
use oxigdal_vrt::VrtBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a 2x2 tile mosaic
    let vrt = VrtBuilder::new()
        .add_tile("/data/tile1.tif", 0, 0, 512, 512)?
        .add_tile("/data/tile2.tif", 512, 0, 512, 512)?
        .add_tile("/data/tile3.tif", 0, 512, 512, 512)?
        .add_tile("/data/tile4.tif", 512, 512, 512, 512)?
        .build_file("mosaic.vrt")?;

    println!("Created VRT: {}x{}", vrt.raster_x_size, vrt.raster_y_size);
    Ok(())
}
```

### Reading from a VRT

```rust
use oxigdal_vrt::VrtReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reader = VrtReader::open("mosaic.vrt")?;

    println!("VRT dimensions: {}x{}", reader.width(), reader.height());
    println!("Bands: {}", reader.band_count());

    // Read a window
    let window = reader.read_window(0, 0, 256, 256, 1)?;

    Ok(())
}
```

## Usage

### Band Subsetting

Extract specific bands from multi-band rasters:

```rust
use oxigdal_vrt::VrtBuilder;

// Create RGB VRT from 4-band RGBA
let vrt = VrtBuilder::new()
    .add_source("input.tif")?
    .select_bands(&[1, 2, 3])? // Red, Green, Blue only
    .build_file("rgb_only.vrt")?;
```

### On-the-fly Scaling

Apply scale and offset transformations:

```rust
use oxigdal_vrt::{VrtBuilder, PixelFunction};

let vrt = VrtBuilder::new()
    .add_source("temperature_kelvin.tif")?
    .set_scale_offset(1.0, -273.15)? // Convert Kelvin to Celsius
    .build_file("temperature_celsius.vrt")?;
```

### Pixel Functions

Apply custom pixel functions:

```rust
use oxigdal_vrt::{VrtBuilder, PixelFunction};

// Calculate NDVI from red and NIR bands
let vrt = VrtBuilder::new()
    .add_source("multispectral.tif")?
    .set_pixel_function(PixelFunction::Ndvi {
        red_band: 3,
        nir_band: 4,
    })?
    .build_file("ndvi.vrt")?;
```

### Virtual Warping

Create reprojected virtual datasets:

```rust
use oxigdal_vrt::VrtBuilder;

let vrt = VrtBuilder::new()
    .add_source("utm_zone_10.tif")?
    .set_srs("EPSG:4326")? // Reproject to WGS84
    .build_file("wgs84.vrt")?;
```

### Complex Mosaics

Build mosaics with overlapping tiles:

```rust
use oxigdal_vrt::{VrtBuilder, ResampleAlg};

let vrt = VrtBuilder::new()
    .add_tile_with_options(
        "tile1.tif",
        0, 0, 1024, 1024,
        ResampleAlg::Bilinear,
    )?
    .add_tile_with_options(
        "tile2.tif",
        512, 0, 1024, 1024, // Overlaps with tile1
        ResampleAlg::Bilinear,
    )?
    .set_blend_mode(BlendMode::Average)?
    .build_file("blended_mosaic.vrt")?;
```

## API Overview

| Module | Description |
|--------|-------------|
| `builder` | VRT creation with builder pattern |
| `reader` | Reading VRT datasets |
| `dataset` | VRT dataset representation |
| `band` | Virtual band configuration |
| `source` | Source raster references |
| `mosaic` | Mosaic creation utilities |
| `xml` | XML parsing and generation |
| `error` | Error types |

### Key Types

- **`VrtBuilder`** - Builder for creating VRT datasets
- **`VrtReader`** - Read and query VRT files
- **`VrtDataset`** - In-memory VRT representation
- **`VrtBand`** - Virtual band configuration
- **`VrtSource`** - Reference to source raster
- **`PixelFunction`** - Built-in pixel transformation functions
- **`ResampleAlg`** - Resampling algorithms (Nearest, Bilinear, Cubic, etc.)

## Supported Pixel Functions

The VRT driver supports various built-in pixel functions:

| Function | Description | Example Use Case |
|----------|-------------|------------------|
| `Ndvi` | Normalized Difference Vegetation Index | Vegetation analysis |
| `Ndwi` | Normalized Difference Water Index | Water body detection |
| `Hillshade` | Terrain hillshade | Elevation visualization |
| `Slope` | Terrain slope | Gradient analysis |
| `Aspect` | Terrain aspect | Direction analysis |
| `Sum` | Sum of bands | Band arithmetic |
| `Diff` | Difference of bands | Change detection |
| `Mul` | Multiply bands | Masking operations |
| `Div` | Divide bands | Ratio indices |

## Resampling Algorithms

- **Nearest** - Nearest neighbor (fastest)
- **Bilinear** - Bilinear interpolation
- **Cubic** - Cubic convolution
- **CubicSpline** - Cubic spline
- **Lanczos** - Lanczos windowed sinc (best quality)
- **Average** - Average of all contributing pixels
- **Mode** - Most common value
- **Max** - Maximum value
- **Min** - Minimum value

## VRT XML Format

VRT files are XML documents with the following structure:

```xml
<VRTDataset rasterXSize="1024" rasterYSize="1024">
  <SRS>EPSG:4326</SRS>
  <GeoTransform>-180.0, 0.35, 0.0, 90.0, 0.0, -0.35</GeoTransform>

  <VRTRasterBand dataType="Float32" band="1">
    <ColorInterp>Gray</ColorInterp>
    <NoDataValue>-9999</NoDataValue>

    <SimpleSource>
      <SourceFilename relativeToVRT="1">tile1.tif</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="0" yOff="0" xSize="512" ySize="512"/>
      <DstRect xOff="0" yOff="0" xSize="512" ySize="512"/>
    </SimpleSource>
  </VRTRasterBand>
</VRTDataset>
```

## Performance

VRT datasets provide zero-copy access to source rasters:

| Operation | Performance | Notes |
|-----------|-------------|-------|
| VRT Creation | <1ms | XML generation only |
| VRT Parsing | 1-10ms | Depends on complexity |
| Pixel Access | Same as source | No overhead for simple VRTs |
| Mosaic Access | Linear with sources | Cached for repeated access |

### Optimization Tips

1. **Use relative paths** - Enables portability
2. **Enable caching** - For repeated window reads
3. **Align windows to tiles** - Minimizes source file access
4. **Use overviews** - For multi-resolution access

## Error Handling

The VRT driver follows the "no unwrap" policy. All operations return `Result<T, VrtError>`:

```rust
use oxigdal_vrt::{VrtReader, VrtError};

match VrtReader::open("data.vrt") {
    Ok(reader) => {
        println!("Opened VRT: {}x{}", reader.width(), reader.height());
    }
    Err(VrtError::FileNotFound { path }) => {
        eprintln!("VRT file not found: {}", path);
    }
    Err(VrtError::InvalidXml { message }) => {
        eprintln!("Invalid VRT XML: {}", message);
    }
    Err(e) => {
        eprintln!("VRT error: {}", e);
    }
}
```

## Pure Rust

This library is **100% Pure Rust** with no GDAL or C dependencies. All VRT parsing, generation, and processing is implemented in Rust.

Benefits:
- **Cross-platform** - Works on any platform Rust supports
- **WebAssembly** - Compiles to WASM
- **Embedded** - Suitable for embedded systems
- **Memory-safe** - Guaranteed memory safety

## Examples

See the `tests/` directory for comprehensive examples:

- `create_vrt_mosaic.rs` - Creating tile mosaics
- `vrt_band_subset.rs` - Band extraction
- `vrt_pixel_functions.rs` - Pixel transformations
- `vrt_virtual_warp.rs` - Reprojection

Run examples:
```bash
cargo test --package oxigdal-vrt --test create_vrt_mosaic -- --nocapture
```

## OxiGDAL Integration

VRT integrates seamlessly with other OxiGDAL drivers:

```rust
use oxigdal_core::DataSource;
use oxigdal_vrt::VrtReader;
use oxigdal_geotiff::GeoTiffWriter;

// Read from VRT, write to GeoTIFF
let vrt = VrtReader::open("mosaic.vrt")?;
let data = vrt.read_all()?;

let writer = GeoTiffWriter::create("output.tif")?;
writer.write_band(&data, 1)?;
writer.close()?;
```

## Testing

Run the test suite:

```bash
# All tests
cargo test --package oxigdal-vrt

# With async support
cargo test --package oxigdal-vrt --features async
```

## Documentation

- **API Documentation**: [docs.rs/oxigdal-vrt](https://docs.rs/oxigdal-vrt)
- **GDAL VRT Format**: [GDAL VRT Specification](https://gdal.org/drivers/raster/vrt.html)
- **Main Project**: [github.com/cool-japan/oxigdal](https://github.com/cool-japan/oxigdal)

## Contributing

Contributions are welcome! Please ensure:

- All tests pass
- No `unwrap()` in production code
- Comprehensive error handling
- Documentation for public APIs
- Follow Rust naming conventions

## Related Projects

**OxiGDAL Ecosystem:**
- [oxigdal-core](../oxigdal-core) - Core types and traits
- [oxigdal-geotiff](../geotiff) - GeoTIFF driver
- [oxigdal-cloud](../../oxigdal-cloud) - Cloud storage backends

**COOLJAPAN Ecosystem:**
- [SciRS2](https://github.com/cool-japan/scirs) - Scientific computing
- [OxiBLAS](https://github.com/cool-japan/oxirs) - Pure Rust BLAS
- [Oxicode](https://github.com/cool-japan/oxirs) - Serialization

## License

Licensed under the Apache License, Version 2.0 ([LICENSE-APACHE](../../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0).

## Authors

COOLJAPAN OU (Team Kitasan)

---

**Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem** - Building the future of geospatial computing in Pure Rust.
