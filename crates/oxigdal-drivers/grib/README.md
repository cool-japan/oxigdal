# OxiGDAL GRIB Driver

[![Crates.io](https://img.shields.io/crates/v/oxigdal-grib.svg)](https://crates.io/crates/oxigdal-grib)
[![Documentation](https://docs.rs/oxigdal-grib/badge.svg)](https://docs.rs/oxigdal-grib)
[![License](https://img.shields.io/crates/l/oxigdal-grib.svg)](LICENSE)
[![GitHub](https://img.shields.io/badge/github-oxigdal-blue?logo=github)](https://github.com/cool-japan/oxigdal)

A comprehensive, pure Rust implementation of the GRIB (GRIdded Binary) meteorological data format driver for OxiGDAL. Supports both GRIB Edition 1 and GRIB Edition 2 formats with full WMO parameter table lookups and multiple grid projection types.

## Features

- **Pure Rust Implementation**: 100% Pure Rust, no C/Fortran dependencies. Compliant with COOLJAPAN policies.
- **GRIB1 and GRIB2 Support**: Read both legacy (GRIB Edition 1) and modern (GRIB Edition 2) GRIB formats
- **WMO Parameter Tables**: Complete WMO standard parameter lookups for meteorological variables (temperature, wind, precipitation, humidity, etc.)
- **Multiple Grid Projections**: Support for regular lat/lon, rotated lat/lon, Lambert conformal, Mercator, polar stereographic, Gaussian, and space view grids
- **Data Decoding**: Efficient unpacking of packed binary data with proper scaling and bit manipulation
- **Metadata Extraction**: Access to time information (reference time, valid time, forecast offset), level types (isobaric, surface, height above ground, etc.)
- **Error Handling**: Comprehensive error types with no `unwrap()` calls - follows COOLJAPAN no-unwrap policy
- **Zero-Copy Design**: Minimal allocations with efficient binary parsing using `bytemuck` for bit manipulation
- **Integration Ready**: Designed to integrate seamlessly with `oxigdal-core` for dataset management
- **Well-Tested**: Comprehensive test suite including unit tests, integration tests, and property-based tests

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-grib = "0.1.0"
```

### Feature Flags

```toml
[dependencies]
oxigdal-grib = { version = "0.1.0", features = ["grib1", "grib2"] }
```

Available features:
- `std` (default): Enable standard library support
- `alloc`: Support for no_std with alloc (experimental)
- `grib1` (default): Enable GRIB Edition 1 support
- `grib2` (default): Enable GRIB Edition 2 support
- `complex_packing` (optional): Complex packing data compression support
- `async` (optional): Async I/O support with tokio

## Quick Start

```rust
use oxigdal_grib::reader::GribReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open a GRIB file
    let mut reader = GribReader::open("data/forecast.grib2")?;

    // Iterate through messages
    for record in reader {
        let record = record?;

        // Get parameter information
        let param = record.parameter()?;
        println!("Parameter: {} ({})", param.long_name, param.units);

        // Get level information
        println!("Level: {:?} = {}", record.level_type(), record.level_value());

        // Get timing information
        if let Some(ref_time) = record.reference_time() {
            println!("Reference time: {}", ref_time);
            println!("Forecast offset: {} hours", record.forecast_offset_hours());
        }

        // Decode the actual data values
        let data = record.decode_data()?;
        println!("Data points: {}", data.len());
        if let Some(first) = data.first() {
            println!("First value: {}", first);
        }
    }

    Ok(())
}
```

## Usage

### Reading GRIB Files

```rust
use oxigdal_grib::reader::GribReader;

fn read_grib_file(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = GribReader::open(path)?;

    // Read all messages at once
    let records = reader.read_all()?;
    println!("Total messages: {}", records.len());

    // Or iterate through messages one by one
    let mut reader = GribReader::open(path)?;
    while let Some(record) = reader.next_message()? {
        process_record(&record)?;
    }

    Ok(())
}

fn process_record(record: &oxigdal_grib::reader::GribRecord) -> Result<(), Box<dyn std::error::Error>> {
    let param = record.parameter()?;
    let level_type = record.level_type();
    let data = record.decode_data()?;

    println!("Parameter: {}", param.short_name);
    println!("Level Type: {:?}", level_type);
    println!("Data length: {}", data.len());

    Ok(())
}
```

### Working with Parameters

```rust
use oxigdal_grib::parameter::{lookup_grib2_parameter, lookup_grib1_parameter};

fn parameter_lookup_example() -> Result<(), Box<dyn std::error::Error>> {
    // Look up GRIB2 parameters using discipline-category-number
    // Discipline 0 = Meteorological products

    // Temperature (0, 0, 0)
    let temp = lookup_grib2_parameter(0, 0, 0)?;
    assert_eq!(temp.short_name, "TMP");
    assert_eq!(temp.units, "K");

    // U-component of wind (0, 2, 2)
    let u_wind = lookup_grib2_parameter(0, 2, 2)?;
    assert_eq!(u_wind.short_name, "UGRD");
    assert_eq!(u_wind.units, "m/s");

    // Relative humidity (0, 1, 1)
    let rh = lookup_grib2_parameter(0, 1, 1)?;
    assert_eq!(rh.short_name, "RH");
    assert_eq!(rh.units, "%");

    // For GRIB1, use table version and parameter number
    let temp_grib1 = lookup_grib1_parameter(3, 11)?;
    assert_eq!(temp_grib1.short_name, "TMP");

    Ok(())
}
```

### Working with Grid Definitions

```rust
use oxigdal_grib::grid::{GridDefinition, LatLonGrid, ScanMode};

fn grid_example() -> Result<(), Box<dyn std::error::Error>> {
    // Create a regular latitude/longitude grid
    let grid = LatLonGrid {
        ni: 360,           // 360 points in longitude direction
        nj: 181,           // 181 points in latitude direction
        la1: 90.0,         // First point latitude (North Pole)
        lo1: 0.0,          // First point longitude
        la2: -90.0,        // Last point latitude (South Pole)
        lo2: 359.0,        // Last point longitude
        di: 1.0,           // Longitude increment
        dj: 1.0,           // Latitude increment
        scan_mode: ScanMode::default(),
    };

    // Get total number of points
    let total_points = grid.num_points();
    println!("Total grid points: {}", total_points);

    // Get coordinates for specific grid indices
    let (lat, lon) = grid.coordinates(0, 0)?;
    println!("First point: lat={}, lon={}", lat, lon);

    // Get grid dimensions
    let (ni, nj) = grid.dimensions();
    println!("Grid dimensions: {}x{}", ni, nj);

    Ok(())
}
```

### Working with Level Types

```rust
use oxigdal_grib::parameter::LevelType;

fn level_type_example() {
    // GRIB2 level type codes
    assert_eq!(LevelType::from_grib2_code(1), LevelType::Surface);
    assert_eq!(LevelType::from_grib2_code(100), LevelType::Isobaric);
    assert_eq!(LevelType::from_grib2_code(101), LevelType::MeanSeaLevel);
    assert_eq!(LevelType::from_grib2_code(103), LevelType::HeightAboveGround);

    // Get human-readable descriptions
    println!("{}", LevelType::Surface.description());              // "Surface"
    println!("{}", LevelType::Isobaric.description());             // "Isobaric (pressure level)"
    println!("{}", LevelType::HeightAboveGround.description());    // "Height above ground"
}
```

### Working with Custom Readers

```rust
use oxigdal_grib::reader::GribReader;
use std::io::Cursor;

fn read_from_memory() -> Result<(), Box<dyn std::error::Error>> {
    // Read GRIB data from memory buffer
    let grib_data: &[u8] = include_bytes!("../test_data.grib2");
    let mut reader = GribReader::new(Cursor::new(grib_data));

    while let Some(record) = reader.next_message()? {
        let data = record.decode_data()?;
        println!("Decoded {} data points", data.len());
    }

    Ok(())
}
```

## API Overview

| Module | Description |
|--------|-------------|
| [`error`](src/error.rs) | Comprehensive error types with proper error context and no panics |
| [`reader`](src/reader.rs) | High-level file reading API with iterator support |
| [`message`](src/message.rs) | Low-level GRIB message parsing and routing |
| [`parameter`](src/parameter.rs) | WMO parameter tables for GRIB1 and GRIB2 meteorological variables |
| [`grid`](src/grid.rs) | Grid definition types and coordinate transformations |
| [`grib1`](src/grib1/mod.rs) | GRIB Edition 1 format support (PDS, GDS, BDS sections) |
| [`grib2`](src/grib2/mod.rs) | GRIB Edition 2 format support (Sections 0-8) |

## Grid Support

The GRIB driver supports the following grid projection types:

| Grid Type | Description | Use Cases |
|-----------|-------------|-----------|
| **Regular Lat/Lon** | Equidistant cylindrical projection (Plate Carrée) | Global weather models, satellite data |
| **Rotated Lat/Lon** | Lat/lon grid rotated for better regional coverage | Regional weather models |
| **Lambert Conformal** | Lambert conformal conic projection | Regional weather, storm tracking |
| **Mercator** | Mercator projection | Marine/ocean data, Mercator-based systems |
| **Polar Stereographic** | Stereographic projection centered at pole | Polar weather, ice extent |
| **Gaussian** | Gaussian latitude grid with reduced grid in longitude | Spectral model output |
| **Space View** | Orthographic or perspective projection | Satellite/geostationary data |

## Parameter Support

The driver includes comprehensive WMO standard parameter tables:

### GRIB2 Parameters (Discipline 0 - Meteorological)

| Category | Parameters |
|----------|-----------|
| **Temperature** | Temperature, Virtual temperature, Potential temperature, Dew point, Dry bulb temperature, etc. |
| **Moisture** | Specific humidity, Relative humidity, Mixing ratio, Precipitation rate, Total precipitation, etc. |
| **Wind** | U/V components, Wind speed, Wind direction, Vertical velocity, etc. |
| **Pressure** | Pressure, Pressure reduced to MSL, Surface pressure, Geopotential height, etc. |
| **Momentum** | Vorticity, Stream function, Divergence, Absolute vorticity, etc. |
| **Clouds** | Cloud water content, Cloud cover, Cloud top height, etc. |
| **Radiation** | Solar radiation, Thermal radiation, Albedo, etc. |
| **Stability** | Lifted index, Showalter index, Stability indices, etc. |

### GRIB1 Parameters

Support for WMO GRIB1 parameter tables with lookups by table version and parameter number.

## GRIB Format Structure

### GRIB Edition 1

```
Indicator Section (IS)
├─ Magic bytes "GRIB"
├─ Edition (1)
└─ Total message length

Product Definition Section (PDS)
├─ Parameter (temperature, pressure, wind, etc.)
├─ Level type and value
├─ Time information
└─ Data representation flags

Grid Definition Section (GDS) [optional]
├─ Grid type (lat/lon, Lambert, etc.)
├─ Grid parameters (dimensions, projection details)
└─ Scan mode

Bit Map Section (BMS) [optional]
└─ Missing data bitmap

Binary Data Section (BDS)
├─ Packing information
├─ Scale factors
└─ Packed data values

End Section
└─ Magic bytes "7777"
```

### GRIB Edition 2

```
Section 0: Indicator Section
├─ Magic bytes "GRIB"
├─ Edition (2)
└─ Total message length

Section 1: Identification Section
├─ Center, subcenter
├─ Master table version
└─ Discipline

Section 2: Local Use Section [optional]
└─ Local/vendor-specific data

Section 3: Grid Definition Section
├─ Grid template
├─ Grid parameters
└─ Scan mode

Section 4: Product Definition Section
├─ Product template
├─ Parameter (discipline, category, number)
├─ Level type and value
└─ Time information

Section 5: Data Representation Section
├─ Data template
├─ Reference value
├─ Binary scale factor
└─ Decimal scale factor

Section 6: Bit-Map Section [optional]
└─ Missing data bitmap

Section 7: Data Section
└─ Packed data values

Section 8: End Section
└─ Magic bytes "7777"
```

## Performance

This implementation prioritizes correctness and ease of maintenance while maintaining reasonable performance:

- **Zero-copy parsing** for message headers and metadata
- **Lazy data decoding** - only decode data when explicitly requested
- **Efficient binary operations** using `bytemuck` for bit-level operations
- **Buffered I/O** automatically used with `GribReader::open()`

Typical performance on modern hardware:
- Message parsing: ~1-5 microseconds per message
- Data decoding: ~10-50 microseconds per 1000 data points
- Memory usage: ~10-50 MB for typical meteorological fields

For production systems with large GRIB files:
- Use `GribReader::open()` for automatic buffering
- Process messages sequentially to minimize memory usage
- Cache decoded data if accessing the same message multiple times

## Pure Rust

This library is **100% Pure Rust** with no C/Fortran dependencies. All functionality works out of the box without external libraries. This means:

- No build-time compilation of C/Fortran code
- Full compatibility with WebAssembly (`wasm32-unknown-unknown` target)
- No platform-specific binary dependencies
- Deterministic builds across all platforms
- No additional system libraries required

## Examples

The crate includes comprehensive integration tests demonstrating:
- Parameter lookups (GRIB1 and GRIB2)
- Level type conversions
- Grid coordinate calculations
- File I/O and message iteration
- Error handling patterns

See the [`tests/`](tests/) directory for complete examples.

## Documentation

Full API documentation is available at [docs.rs/oxigdal-grib](https://docs.rs/oxigdal-grib).

### Key Concepts

- **Message**: A single GRIB-encoded grid field (temperature at specific time/level, etc.)
- **Record**: A high-level wrapper around a message with convenient accessor methods
- **Grid**: The spatial definition of data points (projection, dimensions, coordinates)
- **Parameter**: The meteorological variable being encoded (temperature, wind, etc.)
- **Level**: The vertical level of the data (surface, pressure level, height above ground)
- **Time**: Reference time and forecast offset for the data

## Error Handling

This library follows the COOLJAPAN "no unwrap" policy. All fallible operations return `Result<T, GribError>` with descriptive error context:

```rust
use oxigdal_grib::error::GribError;

// Pattern: Use match or ? operator
match record.decode_data() {
    Ok(data) => process(data),
    Err(GribError::DecodingError(msg)) => eprintln!("Decoding failed: {}", msg),
    Err(e) => eprintln!("Error: {}", e),
}

// Or use the ? operator in functions returning Result
let data = record.decode_data()?;
```

## Contributing

Contributions are welcome! Please ensure:
- All tests pass: `cargo test --all-features`
- No clippy warnings: `cargo clippy`
- Code follows COOLJAPAN policies (pure Rust, no unwrap, proper error handling)
- Documentation is included for public items

## License

This project is licensed under Apache-2.0.

Copyright (c) COOLJAPAN OU (Team Kitasan)

## Related Projects

- [**OxiGDAL**](https://github.com/cool-japan/oxigdal) - OxiGDAL ecosystem for geospatial data
- [**SciRS2**](https://github.com/cool-japan/scirs) - Scientific computing ecosystem
- [**NumRS2**](https://github.com/cool-japan/numrs) - Numerical computing (NumPy-like)
- [**OxiBLAS**](https://github.com/cool-japan/oxiblas) - Pure Rust BLAS operations
- [**OxiCode**](https://github.com/cool-japan/oxicode) - Pure Rust serialization (bincode replacement)
- [**OxiFFT**](https://github.com/cool-japan/oxifft) - Pure Rust FFT library

---

Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem - Pure Rust libraries for science, data, and technology.
