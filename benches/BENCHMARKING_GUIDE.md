# OxiGDAL Benchmarking Guide

## Quick Start

Run all benchmarks:
```bash
cargo bench --package oxigdal-benchmarks
```

Run a specific benchmark suite:
```bash
cargo bench --package oxigdal-benchmarks --bench geo_transform
cargo bench --package oxigdal-benchmarks --bench bounding_box
cargo bench --package oxigdal-benchmarks --bench raster_buffer
cargo bench --package oxigdal-benchmarks --bench compression
cargo bench --package oxigdal-benchmarks --bench tiff_parsing
```

Run a specific test within a suite:
```bash
cargo bench --package oxigdal-benchmarks --bench compression -- deflate
cargo bench --package oxigdal-benchmarks --bench geo_transform -- pixel_to_world
```

## Understanding Results

### Criterion Output

Criterion provides detailed statistics:

```
geo_transform/pixel_to_world/north_up
                        time:   [450.23 ns 452.18 ns 454.32 ns]
                        thrpt:  [221.44 Melem/s 222.54 Melem/s 223.59 Melem/s]
```

- **time**: Mean execution time with 95% confidence interval
- **thrpt**: Throughput (operations or elements per second)

### HTML Reports

View detailed reports at `target/criterion/report/index.html`:
- Performance over time (if run multiple times)
- Violin plots showing distribution
- Statistical comparisons between runs

## Optimization Tips

### 1. CPU Frequency Scaling

For consistent results, disable CPU frequency scaling:

```bash
# Linux
sudo cpupower frequency-set -g performance

# macOS
sudo pmset -a powermode performance
```

### 2. Background Processes

Close unnecessary applications to reduce measurement noise.

### 3. Multiple Runs

Run benchmarks 3-5 times and use median results:

```bash
for i in {1..5}; do
    cargo bench --package oxigdal-benchmarks
done
```

## Comparing with GDAL/Rasterio

### Setup

1. **Install GDAL** (with Python bindings):
```bash
# macOS
brew install gdal

# Ubuntu
sudo apt install gdal-bin python3-gdal

# Verify
gdalinfo --version
python3 -c "import osgeo.gdal; print(osgeo.gdal.__version__)"
```

2. **Install rasterio**:
```bash
pip install rasterio pytest-benchmark
```

3. **Install hyperfine** (for command-line benchmarks):
```bash
# macOS
brew install hyperfine

# Linux
cargo install hyperfine
```

### Benchmark Scripts

#### Python/Rasterio Benchmark

Create `bench_rasterio.py`:

```python
import rasterio
import numpy as np
from rasterio.transform import from_bounds
import time

def bench_read_geotiff(filename, iterations=100):
    times = []
    for _ in range(iterations):
        start = time.perf_counter()
        with rasterio.open(filename) as src:
            data = src.read(1)
        end = time.perf_counter()
        times.append(end - start)

    mean_time = np.mean(times)
    median_time = np.median(times)
    print(f"Read {filename}:")
    print(f"  Mean:   {mean_time*1000:.2f} ms")
    print(f"  Median: {median_time*1000:.2f} ms")

def bench_geotransform(iterations=10000):
    transform = from_bounds(-180, -90, 180, 90, 3600, 1800)

    times = []
    for _ in range(iterations):
        start = time.perf_counter()
        for x in range(100):
            for y in range(100):
                world_x, world_y = transform * (x, y)
        end = time.perf_counter()
        times.append(end - start)

    mean_time = np.mean(times) / 10000  # Per operation
    print(f"GeoTransform pixel_to_world:")
    print(f"  Mean: {mean_time*1e6:.2f} ns per operation")

if __name__ == "__main__":
    # Create test file
    width, height = 1024, 1024
    data = np.random.randint(0, 256, (height, width), dtype=np.uint8)
    transform = from_bounds(-180, -90, 180, 90, width, height)

    with rasterio.open(
        'test_1024x1024_uint8.tif',
        'w',
        driver='GTiff',
        height=height,
        width=width,
        count=1,
        dtype=data.dtype,
        crs='EPSG:4326',
        transform=transform,
        compress='deflate',
    ) as dst:
        dst.write(data, 1)

    # Run benchmarks
    bench_read_geotiff('test_1024x1024_uint8.tif')
    bench_geotransform()
```

Run:
```bash
python bench_rasterio.py
```

#### GDAL Command-Line Benchmark

Using `hyperfine`:

```bash
# Create test file
gdal_create -of GTiff -burn 0 -outsize 1024 1024 test.tif

# Benchmark gdalinfo
hyperfine --warmup 3 'gdalinfo test.tif'

# Benchmark gdal_translate
hyperfine --warmup 3 'gdal_translate -of GTiff -co COMPRESS=DEFLATE test.tif output.tif'
```

#### C++/GDAL Benchmark

Using Google Benchmark, create `bench_gdal.cpp`:

```cpp
#include <benchmark/benchmark.h>
#include <gdal_priv.h>
#include <cmath>

static void BM_GeoTransform_PixelToWorld(benchmark::State& state) {
    double gt[6] = {-180.0, 0.1, 0.0, 90.0, 0.0, -0.1};

    for (auto _ : state) {
        for (int x = 0; x < 100; ++x) {
            for (int y = 0; y < 100; ++y) {
                double world_x = gt[0] + x * gt[1] + y * gt[2];
                double world_y = gt[3] + x * gt[4] + y * gt[5];
                benchmark::DoNotOptimize(world_x);
                benchmark::DoNotOptimize(world_y);
            }
        }
    }
}
BENCHMARK(BM_GeoTransform_PixelToWorld);

static void BM_ReadGeoTIFF(benchmark::State& state) {
    GDALAllRegister();
    const char* filename = "test_1024x1024_uint8.tif";

    for (auto _ : state) {
        GDALDataset* dataset = (GDALDataset*)GDALOpen(filename, GA_ReadOnly);
        GDALRasterBand* band = dataset->GetRasterBand(1);

        int width = band->GetXSize();
        int height = band->GetYSize();
        std::vector<uint8_t> buffer(width * height);

        band->RasterIO(GF_Read, 0, 0, width, height,
                      buffer.data(), width, height, GDT_Byte, 0, 0);

        GDALClose(dataset);
    }
}
BENCHMARK(BM_ReadGeoTIFF);

BENCHMARK_MAIN();
```

Compile and run:
```bash
g++ -std=c++17 -O3 -o bench_gdal bench_gdal.cpp \
    $(gdal-config --cflags) $(gdal-config --libs) \
    -lbenchmark -lpthread

./bench_gdal
```

### Result Comparison

Create a comparison table:

| Operation | OxiGDAL (μs) | GDAL (μs) | Rasterio (μs) | Speedup |
|-----------|-------------|-----------|---------------|---------|
| GeoTransform pixel→world | 0.45 | 0.62 | 0.78 | 1.38x / 1.73x |
| Read 1024×1024 UInt8 | 125 | 156 | 189 | 1.25x / 1.51x |
| DEFLATE decompress 1MB | 4.2 | 5.1 | 5.8 | 1.21x / 1.38x |

## Advanced Benchmarking

### Flamegraphs

Generate CPU flamegraphs to identify hotspots:

```bash
# Install cargo-flamegraph
cargo install flamegraph

# Run with flamegraph
cargo flamegraph --bench geo_transform -- --bench
```

### Memory Profiling

Use valgrind/massif to analyze memory usage:

```bash
# Build benchmark
cargo bench --no-run --package oxigdal-benchmarks

# Find benchmark binary
find target/release -name "geo_transform*" -type f

# Run with valgrind
valgrind --tool=massif ./target/release/deps/geo_transform-<hash> --bench

# Visualize
massif-visualizer massif.out.*
```

### Cache Analysis

Use `perf` on Linux:

```bash
# Record cache misses
perf stat -e cache-references,cache-misses \
    cargo bench --package oxigdal-benchmarks --bench raster_buffer

# Detailed analysis
perf record cargo bench --package oxigdal-benchmarks --bench raster_buffer
perf report
```

## CI/CD Integration

### GitHub Actions

Add to `.github/workflows/bench.yml`:

```yaml
name: Benchmark

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

      - name: Run benchmarks
        run: cargo bench --package oxigdal-benchmarks -- --save-baseline main

      - name: Upload results
        uses: actions/upload-artifact@v4
        with:
          name: benchmark-results
          path: target/criterion/

      - name: Comment on PR
        if: github.event_name == 'pull_request'
        uses: benchmark-action/github-action-benchmark@v1
        with:
          tool: 'cargo'
          output-file-path: target/criterion/*/base/estimates.json
          github-token: ${{ secrets.GITHUB_TOKEN }}
          comment-on-alert: true
          alert-threshold: '120%'
```

## Best Practices

1. **Consistent Environment**: Always run on the same hardware
2. **Multiple Iterations**: Run 100-1000 iterations for stability
3. **Warmup**: Include warmup runs to eliminate cold-start effects
4. **Statistical Significance**: Use t-tests to verify differences
5. **Document Changes**: Note any configuration or hardware changes
6. **Version Control**: Track benchmark results over time
7. **Regression Tests**: Set up alerts for performance regressions

## Troubleshooting

### Noisy Results

If results vary widely:
- Close background applications
- Disable CPU frequency scaling
- Increase sample size
- Run during off-peak hours
- Use `nice -n -20` for higher priority

### Out of Memory

For large benchmarks:
- Reduce buffer sizes
- Use streaming operations
- Increase swap space
- Use a machine with more RAM

### Slow Benchmarks

If benchmarks take too long:
- Reduce iteration count
- Focus on specific operations
- Use sampling instead of exhaustive testing
- Run in parallel with `--jobs N`

## Resources

- [Criterion.rs Book](https://bheisler.github.io/criterion.rs/book/)
- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [GDAL Benchmarking](https://gdal.org/development/dev_practices.html#performance)
- [Google Benchmark](https://github.com/google/benchmark)

## License

Copyright (c) 2026 COOLJAPAN OU (Team Kitasan)

Licensed under Apache-2.0
