# OxiGDAL Development Tools

[![Crates.io](https://img.shields.io/crates/v/oxigdal-dev-tools.svg)](https://crates.io/crates/oxigdal-dev-tools)
[![Documentation](https://docs.rs/oxigdal-dev-tools/badge.svg)](https://docs.rs/oxigdal-dev-tools)
[![License](https://img.shields.io/crates/l/oxigdal-dev-tools.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org/)

Comprehensive development tools for OxiGDAL: profiling, debugging, validation, and testing utilities for geospatial data processing in Rust.

## Features

- **Performance Profiler**: Track execution time, memory usage, CPU consumption, and custom metrics
- **File Inspector**: Analyze and inspect geospatial file formats (GeoTIFF, GeoJSON, Shapefile, Zarr, NetCDF, HDF5, GeoParquet, FlatGeobuf)
- **Data Validator**: Comprehensive validation with error categorization (format, integrity, metadata, projection, bounds)
- **Debug Utilities**: Enhanced debugging helpers for geospatial operations
- **Test Data Generator**: Generate synthetic geospatial data for testing and benchmarking
- **Benchmarking Tools**: Quick and easy performance benchmarking
- **100% Pure Rust**: No C/Fortran dependencies, works out of the box
- **No Unwrap Policy**: All fallible operations return proper Result types with descriptive errors

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-dev-tools = "0.1.3"
oxigdal-core = "0.1.3"
```

## Quick Start

### Performance Profiling

```rust
use oxigdal_dev_tools::profiler::Profiler;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create and start a profiler
    let mut profiler = Profiler::new("my_operation");
    profiler.start();

    // Your work here
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Stop and display results
    profiler.stop();
    println!("{}", profiler.report());

    Ok(())
}
```

### File Inspection

```rust
use oxigdal_dev_tools::inspector::FileInspector;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Inspect a geospatial file
    let inspector = FileInspector::new("/path/to/file.tif")?;

    // Get file information
    println!("Format: {:?}", inspector.format());
    println!("Size: {} bytes", inspector.size());
    println!("{}", inspector.summary());

    Ok(())
}
```

### Data Validation

```rust
use oxigdal_dev_tools::validator::GeoDataValidator;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Validate geospatial data
    let validator = GeoDataValidator::new();
    let result = validator.validate_file("/path/to/data.geojson")?;

    if result.passed {
        println!("Validation passed!");
    } else {
        for error in &result.errors {
            println!("Error: {} - {}", error.category as i32, error.message);
        }
    }

    Ok(())
}
```

### Test Data Generation

```rust
use oxigdal_dev_tools::generator::DataGenerator;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Generate test data
    let generator = DataGenerator::new();

    // Create synthetic geometries
    let points = generator.generate_points(100, (-180.0, 180.0), (-90.0, 90.0))?;
    let polygons = generator.generate_polygons(10, 5)?;

    println!("Generated {} points", points.len());
    println!("Generated {} polygons", polygons.len());

    Ok(())
}
```

### Benchmarking

```rust
use oxigdal_dev_tools::benchmarker::BenchmarkRunner;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bench = BenchmarkRunner::new("my_benchmark");

    // Run benchmark
    for i in 0..10 {
        let result = bench.measure(|| {
            // Your code here
            std::thread::sleep(std::time::Duration::from_millis(10));
        })?;
        println!("Run {}: {:?}", i, result);
    }

    // Display summary
    println!("{}", bench.summary());

    Ok(())
}
```

## Usage

### Profiler Module

The profiler tracks:
- **Execution Time**: High-precision duration measurement
- **Memory Usage**: Memory allocation and delta
- **CPU Usage**: CPU utilization percentage
- **Custom Metrics**: User-defined metrics tracking

```rust
use oxigdal_dev_tools::profiler::Profiler;

let mut profiler = Profiler::new("task");
profiler.start();

// Do work...

profiler.stop();
profiler.add_metric("custom_metric", 42.5)?;

// Display formatted report
println!("{}", profiler.report());

// Export as JSON
let json = profiler.to_json()?;
```

### Inspector Module

Supported file formats:
- **GeoTIFF**: Raster imagery with geospatial metadata
- **GeoJSON**: Vector features in JSON format
- **Shapefile**: ESRI vector format
- **Zarr**: Array storage format
- **NetCDF**: Scientific multidimensional data
- **HDF5**: Hierarchical data format
- **GeoParquet**: Columnar vector format
- **FlatGeobuf**: Efficient binary vector format

```rust
use oxigdal_dev_tools::inspector::FileInspector;

let inspector = FileInspector::new("data.geojson")?;

// Get file information
let info = inspector.info();
println!("Path: {}", info.path);
println!("Size: {} bytes", info.size);
println!("Format: {:?}", info.format);
println!("Readable: {}", info.readable);

// Display detailed analysis
println!("{}", inspector.detailed_analysis());
```

### Validator Module

Validation categories:
- **Format**: File format compliance
- **Integrity**: Data consistency checks
- **Metadata**: Metadata validity
- **Projection**: CRS and projection validation
- **Bounds**: Spatial boundary checks

```rust
use oxigdal_dev_tools::validator::GeoDataValidator;

let validator = GeoDataValidator::new();
let result = validator.validate_file("data.tif")?;

// Check validation results
if result.passed {
    for info in &result.info {
        println!("Info: {}", info);
    }
} else {
    for error in &result.errors {
        println!("Error: {} - {}", error.category as i32, error.message);
    }
    for warning in &result.warnings {
        println!("Warning: {} - {}", warning.category as i32, warning.message);
    }
}

// Export results
let json = serde_json::to_string(&result)?;
```

### Debugger Module

```rust
use oxigdal_dev_tools::debugger::Debugger;

let mut debugger = Debugger::new();

// Log debug information
debugger.log_point("before_processing")?;

// Your code...

debugger.log_point("after_processing")?;

// Display trace
println!("{}", debugger.trace_report());
```

### Generator Module

```rust
use oxigdal_dev_tools::generator::DataGenerator;

let generator = DataGenerator::new();

// Generate test geometries
let points = generator.generate_points(1000, bounds)?;
let lines = generator.generate_lines(100)?;
let polygons = generator.generate_polygons(50, 8)?;

// Generate raster data
let raster = generator.generate_raster(256, 256, 1)?;
```

### Benchmarker Module

```rust
use oxigdal_dev_tools::benchmarker::BenchmarkRunner;

let mut bench = BenchmarkRunner::new("operation");

// Warmup
for _ in 0..5 {
    bench.measure(|| {
        // warmup code
    })?;
}

// Actual benchmarks
for _ in 0..100 {
    bench.measure(|| {
        // your code
    })?;
}

// Statistics
println!("Min: {:?}", bench.min());
println!("Max: {:?}", bench.max());
println!("Mean: {:?}", bench.mean());
println!("Std Dev: {:?}", bench.std_dev());
```

## API Overview

| Module | Description |
|--------|-------------|
| `profiler` | Performance profiling with timing, memory, and CPU metrics |
| `inspector` | File format analysis and inspection utilities |
| `validator` | Data validation with detailed error reporting |
| `debugger` | Debugging utilities and trace logging |
| `generator` | Synthetic geospatial data generation |
| `benchmarker` | Benchmarking and performance measurement tools |

## Error Handling

This library follows the "no unwrap" policy. All fallible operations return `Result<T, DevToolsError>` with descriptive error types:

```rust
use oxigdal_dev_tools::{Result, DevToolsError};

fn process_file(path: &str) -> Result<()> {
    let inspector = FileInspector::new(path)?; // Returns DevToolsError on failure

    // Detailed error handling
    match inspector.analyze() {
        Ok(analysis) => println!("{:?}", analysis),
        Err(e) => eprintln!("Analysis failed: {}", e),
    }

    Ok(())
}
```

## Performance Considerations

- **Profiler**: Minimal overhead with optimized memory tracking
- **Inspector**: Fast file format detection without full parsing
- **Validator**: Configurable validation depth for performance tuning
- **Generator**: Efficient synthetic data creation with parallel support

## Pure Rust

OxiGDAL Development Tools is 100% Pure Rust with no C/Fortran dependencies. All functionality works out of the box without external libraries.

## Examples

See the [examples](../oxigdal-examples) directory for complete working examples including:
- Profile GIS operations
- Inspect various geospatial formats
- Validate geospatial data
- Generate test datasets
- Benchmark algorithms

## Documentation

Full API documentation is available at [docs.rs/oxigdal-dev-tools](https://docs.rs/oxigdal-dev-tools).

## Testing

Run tests with:

```bash
cargo test --lib
```

Run all tests including integration tests:

```bash
cargo test --all-features
```

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

## License

Licensed under Apache License 2.0 ([LICENSE](../../LICENSE))

## Related Projects

- [OxiGDAL Core](../oxigdal-core) - Core geospatial functionality
- [OxiGDAL Algorithms](../oxigdal-algorithms) - Spatial algorithms
- [OxiGDAL Drivers](../oxigdal-drivers) - Format-specific drivers
- [OxiGDAL QC](../oxigdal-qc) - Quality control utilities

---

Part of the [OxiGDAL](https://github.com/cool-japan/oxigdal) ecosystem and [COOLJAPAN](https://github.com/cool-japan) project.
