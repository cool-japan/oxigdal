# oxigdal-bench

Comprehensive performance profiling and benchmarking suite for the OxiGDAL geospatial library ecosystem.

## Features

- **CPU Profiling**: Profile CPU usage with automatic flamegraph generation using `pprof`
- **Memory Profiling**: Track memory usage over time and detect memory leaks
- **System Monitoring**: Monitor system resources (CPU, memory, processes) during benchmarks
- **Benchmark Scenarios**: Predefined scenarios for common geospatial operations
- **Performance Comparison**: Compare performance across different implementations
- **Regression Detection**: Automatically detect performance regressions against baseline
- **Multi-format Reports**: Generate HTML, JSON, CSV, and Markdown reports
- **CI/CD Integration**: Easy integration with continuous integration pipelines

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-bench = "0.1.3"
```

## Quick Start

### Basic Benchmarking

```rust
use oxigdal_bench::prelude::*;

// Create a custom benchmark scenario
let scenario = ScenarioBuilder::new("my_benchmark")
    .description("Test raster processing performance")
    .execute(|| {
        // Your benchmark code here
        Ok(())
    })
    .build();

// Run the scenario
let mut runner = ScenarioRunner::new();
runner.add_scenario(scenario);
runner.run_all()?;

// Generate a report
let mut report = BenchmarkReport::new("My Benchmark Report");
report.add_scenario_results(runner.results().to_vec());
report.generate("report.html", ReportFormat::Html)?;
```

### CPU Profiling

```rust
use oxigdal_bench::profiler::{profile_cpu, CpuProfilerConfig};

let config = CpuProfilerConfig {
    frequency: 100,
    generate_flamegraph: true,
    ..Default::default()
};

let (result, report) = profile_cpu(|| {
    // Code to profile
    expensive_computation()
}, config)?;

println!("Profiling duration: {:?}", report.duration);
if let Some(path) = report.flamegraph_path {
    println!("Flamegraph saved to: {}", path.display());
}
```

### Memory Profiling

```rust
use oxigdal_bench::profiler::{MemoryProfiler, MemoryProfilerConfig};

let mut profiler = MemoryProfiler::new(MemoryProfilerConfig::default());

profiler.start()?;
// Run your code
profiler.stop()?;

let report = profiler.stop()?;
println!("Peak memory: {} MB", report.peak_memory / 1024 / 1024);
println!("Memory growth: {} MB", report.memory_growth / 1024 / 1024);

// Save detailed report
report.save_json("memory_profile.json")?;
```

### Regression Detection

```rust
use oxigdal_bench::regression::{RegressionDetector, RegressionConfig};

let config = RegressionConfig {
    max_slowdown: 1.1, // Allow 10% slowdown
    std_dev_threshold: 2.0,
    min_samples: 3,
};

let mut detector = RegressionDetector::new("baselines.json", config)?;

// Run benchmarks
let mut runner = ScenarioRunner::new();
// ... add scenarios ...
runner.run_all()?;

// Detect regressions
let regressions = detector.detect(runner.results());

// Generate regression report
let report = RegressionReport::new(regressions);
if report.has_regressions() {
    println!("{}", report.generate_summary());
}

// Update baselines (if no regressions)
if !report.has_regressions() {
    detector.update_baselines(runner.results())?;
}
```

## Feature Flags

- `raster`: Enable raster operation benchmarks (requires `oxigdal-geotiff`)
- `vector`: Enable vector operation benchmarks (requires `oxigdal-geojson`)
- `cloud`: Enable cloud storage benchmarks (requires `oxigdal-cloud`)
- `ml`: Enable ML inference benchmarks (requires `oxigdal-ml`)
- `analytics`: Enable analytics benchmarks (requires `oxigdal-analytics`)
- `full`: Enable all features

```toml
[dependencies]
oxigdal-bench = { version = "0.1.3", features = ["raster", "vector"] }
```

## Benchmark Scenarios

### Raster Operations

```rust
use oxigdal_bench::scenarios::raster::*;

// GeoTIFF reading
let scenario = GeoTiffReadScenario::new("input.tif")
    .with_tile_size(256, 256);

// Compression comparison
let scenario = CompressionBenchmarkScenario::new("input.tif", "output_dir")
    .with_methods(vec!["none", "lzw", "deflate", "zstd"]);
```

### Vector Operations

```rust
use oxigdal_bench::scenarios::vector::*;

// Geometry simplification
let scenario = SimplificationScenario::new("input.geojson", 0.001)
    .with_algorithm(SimplificationAlgorithm::DouglasPeucker);

// Spatial indexing
let scenario = SpatialIndexScenario::new("input.geojson")
    .with_index_type(SpatialIndexType::RTree)
    .with_query_count(1000);
```

### I/O Performance

```rust
use oxigdal_bench::scenarios::io::*;

// Sequential read/write
let scenario = SequentialReadScenario::new("input.bin")
    .with_buffer_size(8192);

// Random access patterns
let scenario = RandomAccessScenario::new("input.bin", 1000)
    .with_chunk_size(4096);
```

## Criterion Integration

Run benchmarks with Criterion:

```bash
cargo bench --features full
```

Available benchmark suites:
- `raster_ops`: Raster operation benchmarks
- `vector_ops`: Vector operation benchmarks
- `io_ops`: I/O performance benchmarks
- `cloud_ops`: Cloud storage benchmarks
- `ml_inference`: ML inference benchmarks

## Report Generation

### HTML Report

```rust
report.generate("report.html", ReportFormat::Html)?;
```

### JSON Report

```rust
report.generate("report.json", ReportFormat::Json)?;
```

### CSV Report

```rust
report.generate("report.csv", ReportFormat::Csv)?;
```

### Markdown Report

```rust
report.generate("report.md", ReportFormat::Markdown)?;
```

## Performance Comparison

```rust
use oxigdal_bench::comparison::*;

let mut comparison = Comparison::new("geotiff_read");
comparison.set_baseline("oxigdal");

// Add results from different implementations
comparison.add_result(oxigdal_result);
comparison.add_result(gdal_result);

// Calculate speedup
if let Some(speedup) = comparison.speedup("gdal") {
    println!("GDAL is {:.2}x faster than oxigdal", speedup);
}

// Generate comparison report
let suite = ComparisonSuite::new("Performance Comparison");
suite.add_comparison(comparison);
let report = ComparisonReport::new(suite);
println!("{}", report.generate_markdown_table());
```

## CI/CD Integration

### GitHub Actions

```yaml
name: Benchmark
on: [push, pull_request]

jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - name: Run benchmarks
        run: cargo bench --features full
      - name: Upload results
        uses: actions/upload-artifact@v3
        with:
          name: benchmark-results
          path: target/criterion
```

## License

Licensed under the Apache License, Version 2.0.

## Contributing

Contributions are welcome! Please see CONTRIBUTING.md for details.

## Copyright

Copyright (c) 2025 COOLJAPAN OU (Team Kitasan)
