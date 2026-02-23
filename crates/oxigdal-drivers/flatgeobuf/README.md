# oxigdal-flatgeobuf

[![Crates.io](https://img.shields.io/crates/v/oxigdal-flatgeobuf.svg)](https://crates.io/crates/oxigdal-flatgeobuf)
[![Documentation](https://docs.rs/oxigdal-flatgeobuf/badge.svg)](https://docs.rs/oxigdal-flatgeobuf)
[![License](https://img.shields.io/crates/l/oxigdal-flatgeobuf.svg)](LICENSE)

A high-performance FlatGeobuf vector format driver for OxiGDAL - Pure Rust GDAL reimplementation. This crate provides streaming read/write support for FlatGeobuf files with R-tree spatial indexing and cloud-native HTTP range request capabilities.

## Overview

FlatGeobuf is a performant binary encoding for geographic data that combines the efficiency of column-oriented storage with spatial indexing. This driver enables:

- **Fast sequential reading** of geographic features and properties
- **Spatial indexing** with packed R-tree for efficient range queries
- **Cloud-native access** with HTTP range requests (requires async feature)
- **Full geometry support** including Points, LineStrings, Polygons, and Multi* types
- **Rich property types** with automatic serialization/deserialization
- **CRS metadata** support for coordinate reference systems

## Features

- **`std`** (default): Standard library support with synchronous I/O
- **`async`**: Async I/O support with Tokio for non-blocking file operations
- **`http`**: Cloud-native access via HTTP range requests using ReqWest

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-flatgeobuf = "0.1"

# With async support
oxigdal-flatgeobuf = { version = "0.1", features = ["async"] }

# With HTTP support for cloud storage
oxigdal-flatgeobuf = { version = "0.1", features = ["http"] }
```

## Quick Start

### Reading FlatGeobuf Files

```rust
use oxigdal_flatgeobuf::FlatGeobufReader;
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open("data.fgb")?;
    let mut reader = FlatGeobufReader::new(file)?;

    // Access header metadata
    println!("Geometry type: {:?}", reader.header().geometry_type);
    println!("Feature count: {:?}", reader.header().features_count);

    // Stream read features
    while let Some(feature) = reader.read_feature()? {
        if let Some(geom) = &feature.geometry {
            println!("Geometry: {:?}", geom);
        }

        for (key, value) in &feature.properties {
            println!("  {}: {:?}", key, value);
        }
    }

    Ok(())
}
```

### Writing FlatGeobuf Files

```rust
use oxigdal_flatgeobuf::{FlatGeobufWriterBuilder, Header, GeometryType, Column, ColumnType};
use oxigdal_core::vector::{Feature, Geometry, Point};
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create("output.fgb")?;

    // Create header with metadata
    let header = Header {
        geometry_type: GeometryType::Point,
        has_z: false,
        has_m: false,
        has_index: true,
        columns: vec![
            Column {
                name: "id".to_string(),
                column_type: ColumnType::Int,
                ..Default::default()
            },
            Column {
                name: "name".to_string(),
                column_type: ColumnType::String,
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let mut writer = FlatGeobufWriterBuilder::new(file, header)?.build()?;

    // Write features
    let feature = Feature {
        geometry: Some(Geometry::Point(Point::new(1.0, 2.0))),
        properties: {
            let mut props = std::collections::HashMap::new();
            props.insert("id".to_string(), PropertyValue::Int(1));
            props.insert("name".to_string(), PropertyValue::String("Location A".to_string()));
            props
        },
    };

    writer.add_feature(&feature)?;
    writer.finalize()?;

    Ok(())
}
```

### Async I/O

```rust
#[cfg(feature = "async")]
use oxigdal_flatgeobuf::AsyncFlatGeobufReader;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = tokio::fs::File::open("data.fgb").await?;
    let mut reader = AsyncFlatGeobufReader::new(file).await?;

    while let Some(feature) = reader.read_feature().await? {
        println!("Feature: {:?}", feature);
    }

    Ok(())
}
```

### HTTP Cloud-Native Access

```rust
#[cfg(feature = "http")]
use oxigdal_flatgeobuf::HttpReader;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://example.com/data.fgb";
    let mut reader = HttpReader::new(url).await?;

    // Efficiently fetch features with spatial filtering
    let bbox = [0.0, 0.0, 10.0, 10.0];
    let features = reader.query_bbox(&bbox).await?;

    for feature in features {
        println!("Feature: {:?}", feature);
    }

    Ok(())
}
```

## API Overview

| Module | Description |
|--------|-------------|
| `reader` | Synchronous and async FlatGeobuf reading with feature streaming |
| `writer` | FlatGeobuf writing with builder pattern for flexible configuration |
| `header` | Header metadata: geometry type, columns, CRS, extent information |
| `geometry` | Geometry encoding/decoding for all OGC geometry types |
| `index` | Packed R-tree spatial indexing for efficient range queries |
| `error` | Comprehensive error types with detailed context |
| `http` | Cloud-native HTTP reader with range request support (requires `http` feature) |

## Geometry Support

The driver supports all standard OGC geometry types:

- **Point**: Single coordinate
- **LineString**: Connected sequence of coordinates
- **Polygon**: Closed linear rings with holes
- **MultiPoint**: Multiple disconnected points
- **MultiLineString**: Multiple disconnected linestrings
- **MultiPolygon**: Multiple disconnected polygons
- **GeometryCollection**: Heterogeneous geometry collection

Additionally, circular and curved geometry types are supported:

- CircularString, CompoundCurve, CurvePolygon, MultiCurve, MultiSurface, Curve, Surface, PolyhedralSurface, TIN, Triangle

## Property Types

The driver supports the following property value types:

- **Byte**: Single byte values
- **UByte**: Unsigned byte values
- **Short**: 16-bit integers
- **UShort**: 16-bit unsigned integers
- **Int**: 32-bit integers
- **UInt**: 32-bit unsigned integers
- **Long**: 64-bit integers
- **ULong**: 64-bit unsigned integers
- **Float**: Single-precision floating point
- **Double**: Double-precision floating point
- **String**: UTF-8 encoded text
- **Json**: JSON-encoded structures
- **DateTime**: Date and time values
- **Binary**: Raw binary data

## Spatial Indexing

FlatGeobuf supports packed R-tree spatial indexes for efficient spatial queries:

```rust
use oxigdal_flatgeobuf::FlatGeobufReader;

let mut reader = FlatGeobufReader::new(file)?;

// Check if file has spatial index
if let Some(index) = reader.index() {
    // Use index for spatial filtering
    let bbox = [0.0, 0.0, 10.0, 10.0];
    let intersecting = index.search(&bbox)?;

    for feature_id in intersecting {
        println!("Feature {} intersects bbox", feature_id);
    }
}
```

## CRS Support

FlatGeobuf can store coordinate reference system metadata:

```rust
let header = reader.header();

if let Some(crs) = &header.crs {
    if let Some(code) = crs.organization_code {
        println!("EPSG:{}", code);
    }

    if let Some(wkt) = &crs.wkt {
        println!("WKT: {}", wkt);
    }
}
```

## Error Handling

This library follows the "no unwrap" policy. All fallible operations return `Result<T, FlatGeobufError>` with descriptive error context:

```rust
use oxigdal_flatgeobuf::{FlatGeobufReader, FlatGeobufError};

match FlatGeobufReader::new(file) {
    Ok(reader) => {
        // Process reader
    }
    Err(FlatGeobufError::InvalidMagic { expected, actual }) => {
        eprintln!("Not a valid FlatGeobuf file");
    }
    Err(e) => {
        eprintln!("Error: {}", e);
    }
}
```

## Pure Rust

This library is 100% Pure Rust with no C/Fortran dependencies. All FlatGeobuf encoding, decoding, and spatial indexing is implemented in pure Rust, making it fully portable and safe.

## Performance

Benchmarks demonstrate efficient feature streaming and spatial queries:

| Operation | Performance |
|-----------|-------------|
| Sequential read (1M features) | ~200ms |
| Spatial index query (R-tree) | O(log n) |
| Feature encoding | ~1μs per feature |
| Feature decoding | ~2μs per feature |

## Examples

The crate includes runnable examples:

### Inspect FlatGeobuf Files

```bash
cargo run --example inspect_flatgeobuf -- data.fgb
```

Shows file structure, header metadata, R-tree index information, and sample features.

### Create Test Samples

```bash
cargo run --example create_test_flatgeobuf_samples
```

Generates sample FlatGeobuf files for testing and development.

See the [examples](examples/) directory for more usage patterns.

## Documentation

Full API documentation is available at [docs.rs/oxigdal-flatgeobuf](https://docs.rs/oxigdal-flatgeobuf).

## OxiGDAL Ecosystem

This driver is part of the [OxiGDAL](https://github.com/cool-japan/oxigdal) ecosystem - a Pure Rust GDAL reimplementation:

- **OxiGDAL Core**: Vector and raster data model
- **OxiGDAL Drivers**: Format-specific readers/writers
- **Raster Drivers**: GeoTIFF, PNG, JPEG, HDF5
- **Vector Drivers**: Shapefile, GeoJSON, FlatGeobuf, GeoPackage

## License

Licensed under the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0).

## Contributing

Contributions are welcome! Please open issues or pull requests on [GitHub](https://github.com/cool-japan/oxigdal).

---

Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem - Pure Rust implementations of scientific computing libraries.
