# oxigdal-proj

Pure Rust coordinate transformation and projection support.

[![Crates.io](https://img.shields.io/crates/v/oxigdal-proj)](https://crates.io/crates/oxigdal-proj)
[![Documentation](https://docs.rs/oxigdal-proj/badge.svg)](https://docs.rs/oxigdal-proj)
[![License](https://img.shields.io/crates/l/oxigdal-proj)](LICENSE)

## Overview

`oxigdal-proj` provides coordinate reference system (CRS) operations and transformations for OxiGDAL, implemented in pure Rust with an embedded EPSG database.

### Features

- ✅ EPSG code database (10,000+ definitions)
- ✅ WKT parsing and generation
- ✅ Coordinate transformations
- ✅ Datum conversions
- ✅ Pure Rust implementation (no PROJ.4 required)
- ✅ Optional C bindings for compatibility

## Installation

```toml
[dependencies]
oxigdal-proj = "0.1"
```

## Quick Start

### CRS from EPSG Code

```rust
use oxigdal_proj::Crs;

// WGS84
let wgs84 = Crs::from_epsg(4326)?;
println!("WKT: {}", wgs84.to_wkt()?);

// Web Mercator
let web_mercator = Crs::from_epsg(3857)?;
```

### Coordinate Transformation

```rust
use oxigdal_proj::{Crs, Transformer};

let src = Crs::from_epsg(4326)?; // WGS84
let dst = Crs::from_epsg(3857)?; // Web Mercator

let transformer = Transformer::new(&src, &dst)?;

// Transform San Francisco coordinates
let (x, y) = transformer.transform(-122.4, 37.8)?;
println!("Web Mercator: ({}, {})", x, y);
```

### Batch Transformation

```rust
let coords = vec![
    (-122.4, 37.8),   // San Francisco
    (-74.0, 40.7),    // New York
    (0.0, 51.5),      // London
];

let transformed = transformer.transform_batch(&coords)?;
for (x, y) in transformed {
    println!("({}, {})", x, y);
}
```

## CRS Operations

### WKT Parsing

```rust
use oxigdal_proj::Crs;

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

println!("Name: {}", crs.name());
println!("Type: {:?}", crs.crs_type());
println!("Authority: {}", crs.authority());
println!("Code: {}", crs.code());
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

- **Height Transformations**
  - Ellipsoidal ↔ Orthometric
  - Geoid models

## EPSG Database

Built-in support for common coordinate systems:

```rust
use oxigdal_proj::epsg;

// Search by name
let results = epsg::search("UTM zone 10")?;
for crs in results {
    println!("{}: {}", crs.code, crs.name);
}

// Get definition
let definition = epsg::get(4326)?;
println!("{}", definition.wkt);
```

## Performance

- Transformation: ~100ns per coordinate pair
- Batch transformation: ~50ns per coordinate (SIMD)
- CRS lookup: <1μs (cached)
- WKT parsing: ~10μs

## Features

- **`std`** (default): Standard library support
- **`proj-sys`**: C bindings to PROJ library (optional)

## Pure Rust vs C Bindings

By default, uses pure Rust implementation (proj4rs). For maximum compatibility with PROJ ecosystem, enable `proj-sys` feature:

```toml
[dependencies]
oxigdal-proj = { version = "0.1", features = ["proj-sys"] }
```

⚠️ **Note**: `proj-sys` violates COOLJAPAN Pure Rust policy. Use only when compatibility is required.

## COOLJAPAN Policies

- ✅ **Pure Rust** - Default implementation (proj4rs)
- ✅ **No unwrap()** - All errors handled
- ✅ **Embedded database** - No external files
- ✅ **Well tested** - Comprehensive accuracy tests

## License

Licensed under Apache-2.0.

Copyright © 2025 COOLJAPAN OU (Team Kitasan)

## See Also

- [EPSG Registry](https://epsg.org/)
- [proj4rs](https://docs.rs/proj4rs)
- [API Documentation](https://docs.rs/oxigdal-proj)
