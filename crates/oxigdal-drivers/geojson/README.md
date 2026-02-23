# oxigdal-geojson

Pure Rust GeoJSON (RFC 7946) driver for vector data.

[![Crates.io](https://img.shields.io/crates/v/oxigdal-geojson)](https://crates.io/crates/oxigdal-geojson)
[![Documentation](https://docs.rs/oxigdal-geojson/badge.svg)](https://docs.rs/oxigdal-geojson)
[![License](https://img.shields.io/crates/l/oxigdal-geojson)](LICENSE)

## Overview

`oxigdal-geojson` provides comprehensive support for reading and writing GeoJSON data according to RFC 7946, with extensions for CRS support and large file handling.

### Features

- ✅ RFC 7946 compliant GeoJSON parser and writer
- ✅ All geometry types (Point, LineString, Polygon, Multi*, GeometryCollection)
- ✅ Feature and FeatureCollection support
- ✅ Streaming API for large files
- ✅ CRS support (including EPSG codes)
- ✅ Bounding box computation
- ✅ Geometry validation

## Installation

```toml
[dependencies]
oxigdal-geojson = "0.1"
```

## Quick Start

### Reading GeoJSON

```rust
use oxigdal_geojson::GeoJsonReader;

let geojson = r#"{
    "type": "Feature",
    "geometry": {
        "type": "Point",
        "coordinates": [-122.4, 37.8]
    },
    "properties": {
        "name": "San Francisco"
    }
}"#;

let feature = GeoJsonReader::parse_feature(geojson)?;
println!("Feature: {:?}", feature);
```

### Writing GeoJSON

```rust
use oxigdal_geojson::GeoJsonWriter;
use oxigdal_core::vector::{Geometry, Feature};

let geometry = Geometry::Point { x: -122.4, y: 37.8 };
let mut feature = Feature::new(geometry);
feature.set_property("name", "San Francisco");

let writer = GeoJsonWriter::new();
let geojson = writer.write_feature(&feature)?;
println!("{}", geojson);
```

### Streaming Large Files

```rust
use oxigdal_geojson::GeoJsonStreamReader;
use std::fs::File;

let file = File::open("large_dataset.geojson")?;
let reader = GeoJsonStreamReader::new(file)?;

for feature in reader {
    let feature = feature?;
    println!("Processing: {:?}", feature.geometry());
}
```

## Geometry Types

All RFC 7946 geometry types are supported:

- **Point** - Single coordinate
- **LineString** - Array of coordinates
- **Polygon** - Array of linear rings
- **MultiPoint** - Array of Points
- **MultiLineString** - Array of LineStrings
- **MultiPolygon** - Array of Polygons
- **GeometryCollection** - Mixed geometry types

## Features

- **Properties** - Arbitrary JSON properties
- **Bounding boxes** - Automatic computation
- **CRS** - Coordinate reference system metadata
- **ID** - Feature identifiers

## Validation

```rust
use oxigdal_geojson::validation::GeoJsonValidator;

let validator = GeoJsonValidator::new()
    .check_geometry(true)
    .check_crs(true)
    .check_bbox(true);

let result = validator.validate_file("data.geojson")?;
if result.is_valid {
    println!("Valid GeoJSON");
} else {
    for error in result.errors {
        eprintln!("Error: {}", error);
    }
}
```

## Performance

- Zero-copy parsing where possible
- Streaming API for memory efficiency
- Lazy geometry validation
- Parallel feature processing (optional)

## COOLJAPAN Policies

- ✅ **Pure Rust** - No C dependencies
- ✅ **No unwrap()** - All errors handled
- ✅ **RFC compliant** - Follows GeoJSON spec
- ✅ **Well tested** - Comprehensive test suite

## License

Licensed under Apache-2.0.

Copyright © 2025 COOLJAPAN OU (Team Kitasan)

## See Also

- [GeoJSON RFC 7946](https://tools.ietf.org/html/rfc7946)
- [API Documentation](https://docs.rs/oxigdal-geojson)
- [GitHub Repository](https://github.com/cool-japan/oxigdal)
