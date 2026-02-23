# OxiGDAL GeoParquet Driver

[![Crates.io](https://img.shields.io/crates/v/oxigdal-geoparquet.svg)](https://crates.io/crates/oxigdal-geoparquet)
[![Documentation](https://docs.rs/oxigdal-geoparquet/badge.svg)](https://docs.rs/oxigdal-geoparquet)
[![License](https://img.shields.io/crates/l/oxigdal-geoparquet.svg)](LICENSE)
[![COOLJAPAN](https://img.shields.io/badge/Part%20of-COOLJAPAN-blue)](https://github.com/cool-japan)

A pure Rust implementation of the GeoParquet 1.0 specification for OxiGDAL. This driver enables efficient reading and writing of geospatial vector data in Apache Parquet format with full support for spatial indexing and compression.

## Features

- **GeoParquet 1.0 Compliance**: Full implementation of the GeoParquet specification for encoding geospatial data in Parquet files
- **WKB Geometry Support**: Encode and decode all geometry types (Point, LineString, Polygon, MultiPoint, MultiLineString, MultiPolygon, GeometryCollection)
- **Spatial Indexing**: Build R-tree spatial indexes for efficient row-group level spatial queries
- **Zero-Copy Operations**: Leverages Apache Arrow for in-memory data processing without unnecessary copies
- **Flexible Compression**: Support for Snappy, Gzip, Zstd, LZ4, and Brotli compression codecs
- **Spatial Statistics**: Store and query bounding box metadata at row-group level
- **CRS Support**: Handle Coordinate Reference Systems (WGS84, custom EPSG codes, etc.)
- **Async I/O**: Optional async support for non-blocking file operations
- **Pure Rust**: 100% Pure Rust implementation with no C/Fortran dependencies

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-geoparquet = "0.1"
```

### Feature Flags

```toml
[dependencies]
oxigdal-geoparquet = { version = "0.1", features = ["async", "zstd"] }
```

Available features:
- `std` (default): Standard library support
- `async` (optional): Async I/O support with Tokio
- `snappy` (default): Snappy compression
- `gzip` (optional): Gzip compression via flate2
- `brotli` (optional): Brotli compression
- `lz4` (optional): LZ4 compression
- `zstd` (default): Zstd compression

## Quick Start

### Writing GeoParquet Files

```rust
use oxigdal_geoparquet::{GeoParquetWriter, GeoParquetReader};
use oxigdal_geoparquet::geometry::{Point, Geometry};
use oxigdal_geoparquet::metadata::{Crs, GeometryColumnMetadata};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create metadata for WGS84 geometry column
    let metadata = GeometryColumnMetadata::new_wkb()
        .with_crs(Crs::wgs84());

    // Create writer
    let mut writer = GeoParquetWriter::new(
        "cities.parquet",
        "geometry",
        metadata,
    )?;

    // Add geometries
    let san_francisco = Geometry::Point(Point::new_2d(-122.4194, 37.7749));
    writer.add_geometry(&san_francisco)?;

    let new_york = Geometry::Point(Point::new_2d(-74.0060, 40.7128));
    writer.add_geometry(&new_york)?;

    // Finalize the file
    writer.finish()?;

    Ok(())
}
```

### Reading GeoParquet Files

```rust
use oxigdal_geoparquet::GeoParquetReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open the file
    let reader = GeoParquetReader::open("cities.parquet")?;

    // Check metadata
    println!("Total rows: {}", reader.num_rows());
    println!("Row groups: {}", reader.num_row_groups());

    let metadata = reader.metadata();
    println!("CRS: {:?}", metadata.primary_column_metadata()?.crs);

    // Read geometries from first row group
    let geometries = reader.read_geometries(0)?;
    for (i, geom) in geometries.iter().enumerate() {
        println!("Geometry {}: {:?}", i, geom);
    }

    Ok(())
}
```

## Usage

### Working with Geometries

```rust
use oxigdal_geoparquet::geometry::{
    Point, Coordinate, LineString, Polygon, Geometry,
};

// Create a point
let point = Geometry::Point(Point::new_2d(-122.4, 37.8));

// Create a line string
let coords = vec![
    Coordinate::new_2d(0.0, 0.0),
    Coordinate::new_2d(1.0, 1.0),
    Coordinate::new_2d(2.0, 0.0),
];
let linestring = Geometry::LineString(LineString::new(coords));

// Create a polygon (exterior ring, then holes)
let exterior = vec![
    Coordinate::new_2d(0.0, 0.0),
    Coordinate::new_2d(10.0, 0.0),
    Coordinate::new_2d(10.0, 10.0),
    Coordinate::new_2d(0.0, 10.0),
    Coordinate::new_2d(0.0, 0.0),
];
let polygon = Geometry::Polygon(Polygon::new(exterior, vec![]));
```

### Spatial Indexing

```rust
use oxigdal_geoparquet::GeoParquetReader;
use oxigdal_core::types::BoundingBox;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = GeoParquetReader::open("data.parquet")?;

    // Build spatial index for faster queries
    reader.build_spatial_index()?;

    // Query geometries in a bounding box
    let bbox = BoundingBox::new(-122.5, 37.7, -122.3, 37.9)?;
    let row_groups = reader.query_spatial_index(&bbox)?;

    // Read only relevant row groups
    for row_group_idx in row_groups {
        let geometries = reader.read_geometries(row_group_idx)?;
        println!("Found {} geometries in row group {}",
                 geometries.len(), row_group_idx);
    }

    Ok(())
}
```

### Custom CRS

```rust
use oxigdal_geoparquet::metadata::{Crs, GeometryColumnMetadata};

// WGS84 (latitude/longitude)
let wgs84 = Crs::wgs84();

// Custom EPSG code
let custom_crs = Crs::epsg(3857); // Web Mercator

// Custom WKT string
let custom_crs = Crs::wkt("PROJCS[\"Custom\",...]")?;
```

### Multiple Compression Codecs

```rust
use oxigdal_geoparquet::{GeoParquetWriter, CompressionType};
use oxigdal_geoparquet::metadata::GeometryColumnMetadata;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let metadata = GeometryColumnMetadata::new_wkb();

    // Default uses Snappy compression
    let writer = GeoParquetWriter::new("output.parquet", "geometry", metadata)?;

    // Compression is set at creation time
    // Use cargo features to enable different codecs:
    // - snappy: Fast, moderate compression (default)
    // - gzip: Good compression ratio
    // - zstd: Better compression than snappy with good speed
    // - lz4: Very fast compression
    // - brotli: Best compression ratio

    Ok(())
}
```

## API Overview

| Module | Description |
|--------|-------------|
| `reader` | High-level API for reading GeoParquet files |
| `writer` | High-level API for writing GeoParquet files |
| `geometry` | Geometry types and WKB encoding/decoding |
| `metadata` | GeoParquet metadata structures and CRS handling |
| `spatial` | Spatial indexing and partitioning utilities |
| `arrow_ext` | Arrow schema extensions for GeoParquet metadata |
| `error` | Error types and result handling |

### Key Types

#### GeoParquetReader
Main interface for reading GeoParquet files:
- `open(path)` - Open a GeoParquet file
- `metadata()` - Get GeoParquet metadata
- `schema()` - Get Arrow schema
- `num_rows()` - Total number of rows
- `num_row_groups()` - Number of row groups
- `build_spatial_index()` - Build R-tree index
- `read_geometries(row_group)` - Read geometries from a row group
- `query_spatial_index(bbox)` - Find row groups intersecting a bounding box

#### GeoParquetWriter
Main interface for writing GeoParquet files:
- `new(path, column_name, metadata)` - Create a writer
- `add_geometry(geometry)` - Add a geometry to the file
- `finish()` - Finalize and close the file

#### Geometry Hierarchy
- `Geometry` - Enum containing all geometry types
  - `Point` - Single coordinate
  - `LineString` - Sequence of coordinates
  - `Polygon` - Exterior ring with optional holes
  - `MultiPoint` - Multiple points
  - `MultiLineString` - Multiple line strings
  - `MultiPolygon` - Multiple polygons
  - `GeometryCollection` - Mixed geometry types

#### Metadata
- `GeoParquetMetadata` - File-level GeoParquet metadata
- `GeometryColumnMetadata` - Column-specific geometry metadata
- `Crs` - Coordinate Reference System definition

## Error Handling

This library follows the **no unwrap policy** - all fallible operations return `Result<T, GeoParquetError>` with descriptive error types:

```rust
use oxigdal_geoparquet::{GeoParquetReader, GeoParquetError};

match GeoParquetReader::open("missing.parquet") {
    Ok(reader) => println!("File opened successfully"),
    Err(GeoParquetError::Io(e)) => eprintln!("I/O error: {}", e),
    Err(GeoParquetError::InvalidMetadata { message }) => {
        eprintln!("Invalid metadata: {}", message);
    }
    Err(e) => eprintln!("Error: {}", e),
}
```

Error variants include:
- `Arrow` - Arrow library errors
- `Parquet` - Parquet library errors
- `Core` - OxiGDAL core errors
- `Io` - File I/O errors
- `Json` - Metadata JSON errors
- `InvalidMetadata` - Non-compliant GeoParquet metadata
- `InvalidGeometry` - Malformed geometry data
- `InvalidWkb` - Invalid WKB encoding
- `InvalidCrs` - Invalid CRS specification
- `Unsupported` - Unsupported features
- `TypeMismatch` - Type mismatch errors
- And many more...

## Examples

Comprehensive examples of common tasks:

### Round-Trip Read/Write
```rust
use oxigdal_geoparquet::{GeoParquetWriter, GeoParquetReader};
use oxigdal_geoparquet::geometry::{Point, Geometry};
use oxigdal_geoparquet::metadata::GeometryColumnMetadata;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Write
    {
        let metadata = GeometryColumnMetadata::new_wkb();
        let mut writer = GeoParquetWriter::new("test.parquet", "geom", metadata)?;

        writer.add_geometry(&Geometry::Point(Point::new_2d(1.0, 2.0)))?;
        writer.add_geometry(&Geometry::Point(Point::new_2d(3.0, 4.0)))?;

        writer.finish()?;
    }

    // Read
    {
        let reader = GeoParquetReader::open("test.parquet")?;
        assert_eq!(reader.num_rows(), 2);

        let geometries = reader.read_geometries(0)?;
        for geom in geometries {
            println!("{:?}", geom);
        }
    }

    Ok(())
}
```

See the [tests directory](tests/) for more examples of:
- Reading and writing different geometry types
- Spatial indexing and queries
- Metadata handling
- WKB encoding/decoding

## Supported Geometry Types

All geometry types defined in the GeoParquet specification are fully supported:

- **Point** - Single coordinate with optional Z and M dimensions
- **LineString** - Sequence of 2+ coordinates
- **Polygon** - Exterior ring with optional interior rings (holes)
- **MultiPoint** - Collection of points
- **MultiLineString** - Collection of line strings
- **MultiPolygon** - Collection of polygons
- **GeometryCollection** - Heterogeneous collection of geometries

All types support:
- 2D (XY) coordinates
- 3D (XYZ) coordinates with elevation
- M (measure) dimension
- 4D (XYZM) coordinates

## Coordinate Reference Systems (CRS)

Fully support arbitrary CRS definitions:

```rust
use oxigdal_geoparquet::metadata::Crs;

// Standard definitions
let wgs84 = Crs::wgs84();           // EPSG:4326
let web_mercator = Crs::epsg(3857); // Web Mercator

// Custom WKT (Well-Known Text)
let custom = Crs::wkt("PROJCS[...]")?;
```

## Performance Characteristics

The geoparquet crate is optimized for:

- **I/O Efficiency**: Streaming read/write of Parquet row groups
- **Memory Efficiency**: Zero-copy Arrow operations where possible
- **Spatial Efficiency**: R-tree indexing for fast spatial queries
- **Compression**: Multiple codec options for different performance/compression tradeoffs

Key statistics (on 4,022 lines of code):
- Fast compilation
- Minimal dependencies (only Arrow/Parquet ecosystem)
- Row-group level spatial filtering
- Lazy geometry decoding

## Pure Rust

This library is **100% Pure Rust** with no C/Fortran dependencies. All functionality works out of the box on any platform supported by Rust:

- No external libraries required
- Works with `no_std` (with `alloc`) for embedded systems
- WASM compatible (with appropriate feature flags)

## Related Projects

Part of the **OxiGDAL** ecosystem - a pure Rust reimplementation of GDAL:

- **[OxiGDAL-Core](https://github.com/cool-japan/oxigdal)** - Core types and utilities
- **[OxiBLAS](https://github.com/cool-japan/oxiblas)** - Linear algebra operations
- **[OxiFFT](https://github.com/cool-japan/oxifft)** - Fast Fourier transforms
- **[SciRS2](https://github.com/cool-japan/scirs2)** - Scientific computing ecosystem

Other COOLJAPAN projects:
- **[NumRS2](https://github.com/cool-japan/numrs2)** - Numerical computing (NumPy-like)
- **[ToRSh](https://github.com/cool-japan/torsh)** - Deep learning framework
- **[QuantRS2](https://github.com/cool-japan/quantrs2)** - Quantum computing
- **[OxiCode](https://github.com/cool-japan/oxicode)** - Pure Rust serialization (bincode replacement)

## Documentation

Full documentation is available at [docs.rs/oxigdal-geoparquet](https://docs.rs/oxigdal-geoparquet).

Additional resources:
- [GeoParquet Specification](https://github.com/opengeospatial/geoparquet)
- [Apache Parquet](https://parquet.apache.org/)
- [Apache Arrow](https://arrow.apache.org/)

## Testing

Run the test suite:

```bash
cargo test --all-features
```

Run specific tests:

```bash
cargo test --lib geometry
cargo test --doc
```

Run tests with logging:

```bash
RUST_LOG=debug cargo test -- --nocapture
```

## Contributing

Contributions are welcome! Please ensure:

1. All tests pass: `cargo test --all-features`
2. No clippy warnings: `cargo clippy --all-features`
3. Code is formatted: `cargo fmt`
4. No unwrap() calls (see no unwrap policy)
5. Comprehensive error handling

## License

Licensed under the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0).

---

**Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem**

Developed by COOLJAPAN OU with focus on pure Rust, high-performance geospatial computing.
