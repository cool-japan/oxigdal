# oxigeo-proj

Pure Rust coordinate transformation and projection support.

[![Crates.io](https://img.shields.io/crates/v/oxigeo-proj)](https://crates.io/crates/oxigeo-proj)
[![Documentation](https://docs.rs/oxigeo-proj/badge.svg)](https://docs.rs/oxigeo-proj)
[![License](https://img.shields.io/crates/l/oxigeo-proj)](LICENSE)

## Overview

`oxigeo-proj` provides coordinate reference system (CRS) operations and transformations for OxiGeo, implemented in pure Rust with an embedded EPSG database.

### Features

- ✅ EPSG code database (500+ built-in definitions; optional PROJ.db reader adds ~7,500 more)
- ✅ WKT parsing and generation
- ✅ Coordinate transformations
- ✅ Datum conversions
- ✅ Pure Rust implementation (no PROJ.4 required)
- ✅ Optional C bindings for compatibility

## Installation

```toml
[dependencies]
oxigeo-proj = "0.2"
```

## Quick Start

### CRS from EPSG Code

```rust
use oxigeo_proj::Crs;

// WGS84
let wgs84 = Crs::from_epsg(4326)?;
println!("WKT: {}", wgs84.to_wkt()?);

// Web Mercator
let web_mercator = Crs::from_epsg(3857)?;
```

### Coordinate Transformation

```rust
use oxigeo_proj::{Coordinate, Crs, Transformer};

let src = Crs::from_epsg(4326)?; // WGS84
let dst = Crs::from_epsg(3857)?; // Web Mercator

let transformer = Transformer::new(src, dst)?;

// Transform San Francisco coordinates
let sf = Coordinate::from_lon_lat(-122.4, 37.8);
let transformed = transformer.transform(&sf)?;
println!("Web Mercator: ({}, {})", transformed.x, transformed.y);
```

### Batch Transformation

```rust
let coords = vec![
    Coordinate::from_lon_lat(-122.4, 37.8), // San Francisco
    Coordinate::from_lon_lat(-74.0, 40.7),  // New York
    Coordinate::from_lon_lat(0.0, 51.5),    // London
];

let transformed = transformer.transform_batch(&coords)?;
for coord in transformed {
    println!("({}, {})", coord.x, coord.y);
}
```

## CRS Operations

### WKT Parsing

```rust
use oxigeo_proj::Crs;

let wkt = r#"
    GEOGCS["WGS 84",
        DATUM["WGS_1984",
            SPHEROID["WGS 84",6378137,298.257223563]],
        PRIMEM["Greenwich",0],
        UNIT["degree",0.0174532925199433]]
"#;

let crs = Crs::from_wkt(wkt)?;
println!("EPSG: {:?}", crs.epsg_code());
```

### CRS Information

```rust
let crs = Crs::from_epsg(4326)?;

println!("Name: {:?}", crs.name());
println!("Type: {:?}", crs.crs_type());
println!("Authority: {:?}", crs.authority());
println!("EPSG code: {:?}", crs.epsg_code());
println!("Datum: {:?}", crs.datum());
```

## Supported Transformations

- **Geographic ↔ Projected**
  - WGS84 ↔ UTM
  - WGS84 ↔ Web Mercator
  - NAD83 ↔ State Plane

- **Datum Shifts**
  - WGS84 ↔ NAD83
  - WGS84 ↔ ETRS89
  - 7-parameter transformations
  - NTv2 / NADCON grid-shift transforms

- **Height Transformations**
  - Ellipsoidal ↔ Orthometric
  - Geoid models

## EPSG Database

Built-in support for common coordinate systems:

```rust
use oxigeo_proj::epsg;

// List all embedded EPSG codes
let codes = epsg::available_epsg_codes();
println!("{} EPSG codes available", codes.len());

// Get a specific definition
let definition = epsg::lookup_epsg(4326)?;
println!("{}: {}", definition.code, definition.name);
```

## Performance

- Transformation: ~100ns per coordinate pair
- Batch transformation: ~50ns per coordinate (SIMD)
- CRS lookup: <1μs (cached)
- WKT parsing: ~10μs

## Features

- **`std`** (default): Standard library support
- **`proj-db`**: Pure-Rust SQLite reader for a system PROJ.db (~7,500 additional EPSG codes, optional)
- **`proj4rs-compat`**: Retains `proj4rs` as an optional fallback engine for backward-compatible error conversion (optional)

## Pure Rust Engine

The transformation engine is the pure Rust `OxiProj` (COOLJAPAN cartographic) engine — there are no C bindings and no C/C++ toolchain is required. `proj4rs` is retained as an optional fallback via the `proj4rs-compat` feature for backward-compatible error conversion.

## COOLJAPAN Policies

- ✅ **Pure Rust** - Default implementation (OxiProj)
- ✅ **No unwrap()** - All errors handled
- ✅ **Embedded database** - No external files
- ✅ **Well tested** - Comprehensive accuracy tests

## License

Licensed under Apache-2.0.

Copyright © 2025 COOLJAPAN OU (Team Kitasan)

## See Also

- [EPSG Registry](https://epsg.org/)
- [proj4rs](https://docs.rs/proj4rs)
- [API Documentation](https://docs.rs/oxigeo-proj)
