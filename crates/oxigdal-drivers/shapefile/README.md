# OxiGDAL Shapefile Driver

[![Crates.io](https://img.shields.io/crates/v/oxigdal-shapefile.svg)](https://crates.io/crates/oxigdal-shapefile)
[![Documentation](https://docs.rs/oxigdal-shapefile/badge.svg)](https://docs.rs/oxigdal-shapefile)
[![License](https://img.shields.io/crates/l/oxigdal-shapefile.svg)](LICENSE)
[![Pure Rust](https://img.shields.io/badge/pure-rust-brightgreen)](#pure-rust)

A pure Rust implementation of ESRI Shapefile format support for the OxiGDAL ecosystem. Read and write complete Shapefiles (.shp, .dbf, .shx) with full geometry type support, attribute handling, and spatial indexing.

## Features

- **Pure Rust Implementation** - Zero C/C++/Fortran dependencies; works everywhere Rust compiles
- **Complete File Format Support** - Reads and writes all three core files (.shp geometry, .dbf attributes, .shx spatial index)
- **14 Geometry Types** - Point, PointZ, PointM, PolyLine, PolyLineZ, PolyLineM, Polygon, PolygonZ, PolygonM, MultiPoint, MultiPointZ, MultiPointM, MultiPatch, and Null types
- **All DBF Field Types** - Character, Number, Logical, Date, and Float fields with proper encoding
- **Spatial Indexing** - SHX file support for fast random access to records
- **Round-Trip Compatibility** - Read → modify → write workflow without data loss
- **No Unsafe** - Sound error handling with comprehensive `Result<T>` types; no `unwrap()` or `panic!()` in production code
- **Buffered I/O** - Efficient streaming for large files
- **OxiGDAL Integration** - Native conversion to/from OxiGDAL vector types
- **Feature Flags** - Optional async, Arrow support, and no-std compatibility

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-shapefile = "0.1"
```

### Optional Features

```toml
[dependencies]
oxigdal-shapefile = { version = "0.1", features = ["async", "arrow"] }
```

- **`std`** (default) - Standard library support
- **`async`** - Tokio-based asynchronous I/O
- **`arrow`** - Apache Arrow integration for columnar operations

## Quick Start

### Reading Shapefiles

```rust
use oxigdal_shapefile::ShapefileReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open a Shapefile (reads .shp, .dbf, and .shx automatically)
    let reader = ShapefileReader::open("path/to/shapefile")?;

    // Read all features at once
    let features = reader.read_features()?;

    // Access geometry and attributes
    for feature in &features {
        println!("Record #{}: {:?}", feature.record_number, feature.geometry);
        println!("Attributes: {:?}", feature.attributes);
    }

    Ok(())
}
```

### Writing Shapefiles

```rust
use oxigdal_shapefile::{ShapefileFeature, ShapefileWriter};
use oxigdal_shapefile::shp::shapes::ShapeType;
use oxigdal_shapefile::writer::ShapefileSchemaBuilder;
use oxigdal_core::vector::{Coordinate, Geometry, Point as CorePoint};
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create schema with attribute fields
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 50)?
        .add_numeric_field("POPULATION", 10, 0)?
        .add_numeric_field("DENSITY", 8, 2)?
        .build();

    // Create writer for Point shapefile
    let mut writer = ShapefileWriter::new(
        "cities",
        ShapeType::Point,
        schema
    )?;

    // Create features with geometry and attributes
    let mut features = vec![];

    // Feature 1
    let point = Geometry::Point(CorePoint::new_2d(-73.9352, 40.7306)?);
    let mut attrs = HashMap::new();
    attrs.insert("NAME".to_string(),
                 oxigdal_core::vector::PropertyValue::String("New York".to_string()));
    attrs.insert("POPULATION".to_string(),
                 oxigdal_core::vector::PropertyValue::Integer(8_000_000));
    attrs.insert("DENSITY".to_string(),
                 oxigdal_core::vector::PropertyValue::Float(27_000.0));

    features.push(ShapefileFeature::new(1, Some(point), attrs));

    // Write all features (creates cities.shp, cities.dbf, cities.shx)
    writer.write_features(&features)?;

    Ok(())
}
```

## Usage Examples

### Reading Individual Records

```rust
use oxigdal_shapefile::ShapefileReader;
use std::path::Path;

fn process_shapefile(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let reader = ShapefileReader::open(path)?;

    // Get all features
    let features = reader.read_features()?;

    // Process each feature
    for feature in features {
        // Access geometry type
        if let Some(geom) = &feature.geometry {
            match geom.geometry_type() {
                oxigdal_core::vector::GeometryType::Point => {
                    println!("Point feature at record {}", feature.record_number);
                }
                oxigdal_core::vector::GeometryType::LineString => {
                    println!("LineString feature at record {}", feature.record_number);
                }
                oxigdal_core::vector::GeometryType::Polygon => {
                    println!("Polygon feature at record {}", feature.record_number);
                }
                _ => {}
            }
        }

        // Access attributes
        if let Some(name) = feature.attributes.get("NAME") {
            println!("Name: {:?}", name);
        }
    }

    Ok(())
}
```

### Working with Different Geometry Types

```rust
use oxigdal_shapefile::shp::shapes::ShapeType;

// Check geometry type capabilities
let point_type = ShapeType::Point;
assert!(!point_type.has_z());
assert!(!point_type.has_m());

let point_z = ShapeType::PointZ;
assert!(point_z.has_z());
assert!(!point_z.has_m());

let point_m = ShapeType::PointM;
assert!(!point_m.has_z());
assert!(point_m.has_m());

// Polygon with Z coordinates
let polygon_z = ShapeType::PolygonZ;
assert!(polygon_z.has_z());
```

### Building Complex Schemas

```rust
use oxigdal_shapefile::writer::ShapefileSchemaBuilder;

let schema = ShapefileSchemaBuilder::new()
    // Character fields (up to 254 characters)
    .add_character_field("NAME", 100)?
    .add_character_field("DESCRIPTION", 254)?

    // Numeric fields (precision.scale)
    .add_numeric_field("ID", 10, 0)?
    .add_numeric_field("VALUE", 15, 2)?

    // Other field types
    .add_logical_field("ACTIVE")?
    .add_date_field("CREATED")?
    .add_float_field("SCORE")?

    .build();
```

### Error Handling

```rust
use oxigdal_shapefile::{ShapefileReader, ShapefileError};

fn read_safely(path: &str) -> Result<(), ShapefileError> {
    let reader = ShapefileReader::open(path)
        .map_err(|e| {
            eprintln!("Failed to open shapefile: {}", e);
            e
        })?;

    let features = reader.read_features()?;
    println!("Successfully read {} features", features.len());

    Ok(())
}

// All errors use Result<T> pattern - no unwrap() calls
match read_safely("data.shp") {
    Ok(()) => println!("Success"),
    Err(e) => eprintln!("Error: {}", e),
}
```

## API Overview

### Core Types

| Type | Description |
|------|-------------|
| `ShapefileReader` | High-level interface for reading complete Shapefiles |
| `ShapefileWriter` | High-level interface for writing Shapefiles |
| `ShapefileFeature` | Combines geometry and attributes from a Shapefile record |
| `ShapeType` | Enum of all 14 supported geometry types |
| `ShapefileError` | Comprehensive error type with contextual information |

### Geometry Modules

| Module | Description |
|--------|-------------|
| `shp` | Shapefile geometry format (.shp files) and Shape types |
| `dbf` | dBase attribute format (.dbf files), field types, and records |
| `shx` | Spatial index format (.shx files) for fast record access |
| `reader` | High-level reading interface combining all three files |
| `writer` | High-level writing interface with schema builder |
| `error` | Comprehensive error handling with Result type |

### Key Traits

- `FieldType` - Enumeration of DBF field types
- `FieldValue` - Attribute values (String, Integer, Float, Date, Logical)
- `Geometry` - OxiGDAL integration for vector geometries

## Supported Geometry Types

### 2D Geometries
- **Point** - Single coordinate point
- **PolyLine** - Connected line segments (LineString)
- **Polygon** - Closed rings with optional holes
- **MultiPoint** - Multiple disconnected points

### 3D Geometries (with Z)
- **PointZ** - Point with elevation/height
- **PolyLineZ** - LineString with Z coordinates
- **PolygonZ** - Polygon with Z coordinates
- **MultiPointZ** - MultiPoint with Z coordinates

### Measured Geometries (with M)
- **PointM** - Point with measurement value
- **PolyLineM** - LineString with M values
- **PolygonM** - Polygon with M values
- **MultiPointM** - MultiPoint with M values

### Other
- **MultiPatch** - 3D surface (limited support)
- **Null** - Empty geometry

## Supported DBF Field Types

| Type | Description | Max Length |
|------|-------------|-----------|
| Character | Text strings | 254 bytes |
| Number | Fixed-point numbers with precision | 20 digits |
| Float | Double-precision floating point | 20 digits |
| Logical | Boolean (Y/N) | 1 byte |
| Date | Calendar dates (YYYYMMDD) | 8 bytes |

## File Format Details

### Shapefile (.shp)

```
Header (100 bytes):
  [0-3]    File Code (9994 - big endian)
  [4-7]    File Length in 16-bit words (big endian)
  [8-11]   Version (1000 - little endian)
  [12-15]  Shape Type (little endian)
  [16-83]  Bounding Box (4 doubles: Xmin, Ymin, Xmax, Ymax)
  [84-99]  Z/M ranges (if 3D)

Records (variable):
  [0-3]    Record Number (big endian)
  [4-7]    Content Length in 16-bit words (big endian)
  [8+]     Shape content (little endian)
```

### dBase (.dbf)

```
Header (32 bytes):
  [0]      Version
  [1-3]    Last Update (YY, MM, DD)
  [4-7]    Record Count
  [8-9]    Header Size
  [10-11]  Record Size
  [12-31]  Reserved

Field Descriptors (32 bytes each):
  [0-10]   Field Name (null-padded)
  [11]     Field Type (C/N/F/L/D)
  [12-15]  Reserved
  [16]     Field Length
  [17]     Decimal Count
  [18-31]  Reserved

Record Data:
  [0]      Deletion Flag (0x20 = active, 0x2A = deleted)
  [1+]     Field data (fixed-length)

Terminator: 0x1A
```

### Spatial Index (.shx)

```
Header (100 bytes): Same as .shp

Index Entries (8 bytes each):
  [0-3]    Record Offset in 16-bit words (big endian)
  [4-7]    Content Length in 16-bit words (big endian)
```

## Performance Characteristics

### Reading
- **Header parsing**: Negligible (O(1))
- **Feature loading**: O(n) where n = number of features
- **Memory usage**: One feature at a time with streaming API (future)
- **Large files**: Buffered I/O optimizes disk access

### Writing
- **Feature writing**: O(n) with automatic bounding box calculation
- **File generation**: All three files (.shp, .dbf, .shx) written simultaneously
- **Validation**: Compile-time type safety + runtime error checking

## Examples

See the [examples](examples/) directory for practical demonstrations:

- `create_test_shapefile_samples.rs` - Create Shapefiles with various geometry types
- `verify_shapefile_samples.rs` - Read and validate Shapefile integrity

Run examples with:

```bash
cargo run --package oxigdal-shapefile --example create_test_shapefile_samples
cargo run --package oxigdal-shapefile --example verify_shapefile_samples
```

## Integration with OxiGDAL

Convert to OxiGDAL types:

```rust
use oxigdal_shapefile::ShapefileReader;

let reader = ShapefileReader::open("data")?;
let shapefile_features = reader.read_features()?;

// Convert to OxiGDAL Features
let oxigdal_features: Result<Vec<_>, _> = shapefile_features
    .iter()
    .map(|f| f.to_oxigdal_feature())
    .collect();
```

Create from OxiGDAL types:

```rust
use oxigdal_shapefile::{ShapefileFeature, ShapefileWriter};
use oxigdal_core::vector::Feature;

fn shapefile_from_oxigdal(
    features: &[Feature],
    output_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let shapefile_features: Vec<_> = features
        .iter()
        .enumerate()
        .map(|(idx, feature)| {
            let mut attrs = std::collections::HashMap::new();
            // Copy properties
            for (key, value) in feature.properties() {
                attrs.insert(key.clone(), value.clone());
            }
            ShapefileFeature::new((idx + 1) as i32, Some(feature.geometry().clone()), attrs)
        })
        .collect();

    let schema = ShapefileSchemaBuilder::new().build();
    let mut writer = ShapefileWriter::new(output_path, ShapeType::Point, schema)?;
    writer.write_features(&shapefile_features)?;

    Ok(())
}
```

## COOLJAPAN Policies

This library strictly adheres to COOLJAPAN ecosystem standards:

### Pure Rust
- 100% pure Rust with zero C/C++/Fortran dependencies
- Works on any platform that Rust supports
- No platform-specific code or conditional compilation (except `std` feature)

### No `unwrap()` Policy
- All fallible operations return descriptive `Result<T, ShapefileError>`
- Comprehensive error types with contextual information
- Safe error handling throughout the entire codebase

### Clean Architecture
- Single-responsibility modules (shp, dbf, shx, reader, writer)
- Clear public API with re-exports
- Extensive documentation with examples
- All files kept under 2000 lines using splitrs if needed

### Testing
- Unit tests for all major functions
- Integration tests for round-trip operations
- Property-based tests for format validation
- Comprehensive error case coverage

### Performance
- Zero-copy where possible
- Buffered I/O for large files
- Efficient spatial indexing

## Limitations

- Point geometry conversion to OxiGDAL is fully supported
- PolyLine, Polygon, and MultiPoint parsing is implemented, conversion pending
- MultiPatch (3D surfaces) has limited support
- No support for memo fields (.dbt files)
- No support for extended .prj (projection) parsing beyond reading
- Single-threaded design (async feature for I/O only)

## References

- [ESRI Shapefile Technical Description](https://www.esri.com/content/dam/esrisites/sitecore-archive/Files/Pdfs/library/whitepapers/pdfs/shapefile.pdf)
- [dBase File Format Specification](http://www.dbase.com/Knowledgebase/INT/db7_file_fmt.htm)
- [OxiGDAL Documentation](https://github.com/cool-japan/oxigdal)

## Testing

Run the full test suite:

```bash
cargo test --all-features --package oxigdal-shapefile
```

Run with no default features:

```bash
cargo test --no-default-features --package oxigdal-shapefile
```

Run specific examples:

```bash
cargo test --package oxigdal-shapefile --test '*'
```

## Documentation

Full API documentation is available at [docs.rs/oxigdal-shapefile](https://docs.rs/oxigdal-shapefile).

Generate local documentation:

```bash
cargo doc --package oxigdal-shapefile --open
```

## Contributing

Contributions are welcome! Please ensure:

- All tests pass: `cargo test --all-features`
- No clippy warnings: `cargo clippy --all-features`
- Code adheres to COOLJAPAN policies (no unwrap, pure Rust, etc.)
- Documentation is updated for public APIs

## License

Licensed under the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0).

## Related Projects

Part of the [OxiGDAL](https://github.com/cool-japan/oxigdal) ecosystem:

- **OxiGDAL Core** - Pure Rust GDAL alternative
- **OxiGDAL Drivers** - Format drivers (GeoTIFF, NetCDF, HDF5, etc.)
- **SciRS2** - Scientific computing ecosystem
- **NumRS2** - Numerical computing (NumPy-like)
- **OxiBLAS** - Pure Rust BLAS operations
- **Oxicode** - Rust serialization (bincode replacement)

---

**Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem** - Pure Rust geospatial and scientific computing.
