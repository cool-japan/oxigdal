# OxiGDAL Troubleshooting Guide

Solutions to common issues and problems you might encounter with OxiGDAL.

## Table of Contents

- [Installation Issues](#installation-issues)
- [Compilation Errors](#compilation-errors)
- [Runtime Errors](#runtime-errors)
- [Performance Issues](#performance-issues)
- [Data Issues](#data-issues)
- [Integration Problems](#integration-problems)
- [Debugging Tips](#debugging-tips)

## Installation Issues

### Issue: Cargo Cannot Find Package

**Error:**
```
error: no default toolchain configured
```

**Solution:**
```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Update toolchain
rustup update

# Verify installation
cargo --version
```

### Issue: Out of Disk Space During Build

**Error:**
```
error: linker link.exe not found
error: could not compile 'oxigdal-core'
```

**Solution:**
```bash
# Clean previous builds
cargo clean

# Build with reduced artifacts
cargo build --release

# Check disk space
df -h
# If low, remove large files:
rm -rf target/  # Rebuild from scratch
```

### Issue: Platform-Specific Build Failures

**For macOS:**
```bash
# Ensure Xcode tools installed
xcode-select --install

# Use Apple silicon native compilation
rustup target add aarch64-apple-darwin
cargo build --target aarch64-apple-darwin
```

**For Windows:**
```bash
# Use Visual Studio Build Tools
# Download from: https://visualstudio.microsoft.com/downloads/

# Or use MinGW
rustup target add x86_64-pc-windows-gnu
cargo build --target x86_64-pc-windows-gnu
```

**For Linux:**
```bash
# Install build essentials (Ubuntu/Debian)
sudo apt-get install build-essential pkg-config

# Install development libraries
sudo apt-get install libssl-dev
```

## Compilation Errors

### Issue: Cannot Find Type 'RasterDataType'

**Error:**
```
error[E0433]: cannot find type `RasterDataType` in this scope
  |
  | let buffer = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
  |                                                ^^^^^^^^^^^^^^
```

**Solution:**
```rust
// Add missing import
use oxigdal_core::types::RasterDataType;

// Or use full path
let buffer = oxigdal_core::types::RasterBuffer::zeros(512, 512, oxigdal_core::types::RasterDataType::Float32);
```

### Issue: Lifetime Mismatch

**Error:**
```
error[E0623]: lifetime mismatch
  |
  | fn process(buffer: &RasterBuffer) { ... }
```

**Solution:**
```rust
// Specify lifetime explicitly if needed
fn process<'a>(buffer: &'a RasterBuffer) -> &'a RasterBuffer {
    buffer
}

// Or use lifetime elision (preferred)
fn process(buffer: &RasterBuffer) -> &RasterBuffer {
    buffer
}
```

### Issue: Move vs Copy

**Error:**
```
error[E0382]: use of moved value: `buffer`
  |
  | let buffer = reader.read_tile_buffer(0, 0, 0)?;
  | let processed = process(buffer);  // Moves buffer
  | println!("{:?}", buffer);  // Error: buffer already moved
```

**Solution:**
```rust
// Borrow instead of moving
let buffer = reader.read_tile_buffer(0, 0, 0)?;
let processed = process(&buffer);  // Borrows immutably
println!("{:?}", buffer);  // OK

// Or make a copy
let buffer = reader.read_tile_buffer(0, 0, 0)?;
let processed = process(buffer.clone());  // Clone if implemented
let result = buffer.clone();  // OK
```

### Issue: Mutable Borrow Conflicts

**Error:**
```
error[E0502]: cannot borrow `buffer` as mutable more than once at a time
  |
  | buffer.normalize();
  | ^^^^^^ first mutable borrow
  | buffer.scale(2.0);
  | ^^^^^^ second mutable borrow
```

**Solution:**
```rust
// Method chaining (if available)
let buffer = reader.read_tile_buffer(0, 0, 0)?;
buffer.normalize()?.scale(2.0)?;

// Or separate operations
let mut buffer = reader.read_tile_buffer(0, 0, 0)?;
buffer.normalize()?;  // First use
// First borrow ends here
buffer.scale(2.0)?;   // OK: new borrow
```

## Runtime Errors

### Issue: "File Not Found"

**Error:**
```
Error: OxiGdalError: IO error: file not found
```

**Solution:**
```rust
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = "data/image.tif";

    // Check if file exists
    if !Path::new(path).exists() {
        eprintln!("File not found: {}", path);
        eprintln!("Current directory: {}", std::env::current_dir()?.display());
        return Err("File not found".into());
    }

    let source = FileDataSource::open(path)?;
    // ...
    Ok(())
}

// When running, ensure file exists:
// cargo run --release -- data/image.tif
```

### Issue: Out of Memory

**Error:**
```
Segmentation fault (core dumped)
// or
thread panicked at 'allocation error'
```

**Solution:**
```rust
// Process in chunks instead of loading entire file
fn process_large_file_streaming(path: &str) -> Result<()> {
    let source = FileDataSource::open(path)?;
    let reader = GeoTiffReader::open(source)?;

    const CHUNK_HEIGHT: u32 = 512;
    let height = reader.height();

    for y in (0..height).step_by(CHUNK_HEIGHT as usize) {
        let h = std::cmp::min(CHUNK_HEIGHT, height - y);
        let chunk = read_band_window(&reader, 0, y, reader.width(), h)?;

        // Process chunk
        let _ = process_chunk(&chunk)?;

        // Chunk is dropped here, memory freed
    }

    Ok(())
}

// Monitor memory usage
// valgrind --tool=massif ./target/release/app
```

### Issue: Thread Panic in Parallel Operations

**Error:**
```
panicked at 'index out of bounds'
```

**Solution:**
```rust
use rayon::prelude::*;

// Good: catch errors in parallel iterator
let results: Vec<Result<_, _>> = tiles
    .par_iter()
    .map(|tile| {
        match process_tile(tile) {
            Ok(result) => Ok(result),
            Err(e) => {
                eprintln!("Error processing tile {:?}: {}", tile, e);
                Err(e)
            }
        }
    })
    .collect();

// Check for errors
for result in results {
    match result {
        Ok(value) => { /* use value */ }
        Err(e) => { eprintln!("Processing failed: {}", e); }
    }
}
```

### Issue: Invalid Bounding Box

**Error:**
```
BoundingBox::new(10.0, 20.0, 0.0, 30.0)?
// Error: min_x > max_x
```

**Solution:**
```rust
fn create_safe_bbox(x1: f64, y1: f64, x2: f64, y2: f64) -> Result<BoundingBox> {
    let min_x = x1.min(x2);
    let max_x = x1.max(x2);
    let min_y = y1.min(y2);
    let max_y = y1.max(y2);

    BoundingBox::new(min_x, min_y, max_x, max_y)
}

// Usage
let bbox = create_safe_bbox(10.0, 20.0, 0.0, 30.0)?;
// Safe: min_x=0, max_x=10, min_y=20, max_y=30
```

## Performance Issues

### Issue: Slow Tile Reading

**Diagnosis:**
```bash
# Profile the application
cargo flamegraph --release -- --input large_file.tif

# Check for obvious bottlenecks in output flamegraph
```

**Solutions:**

1. **Use Release Mode:**
```bash
cargo build --release
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

2. **Enable SIMD:**
```rust
use oxigdal_core::simd_buffer::SimdBuffer;

let buffer = reader.read_tile_buffer(0, 0, 0)?;
let simd_buffer = SimdBuffer::from_buffer(&buffer)?;
let result = simd_buffer.add_scalar(10.0)?;
```

3. **Use Parallel Processing:**
```rust
use rayon::prelude::*;

let results: Vec<_> = tiles
    .par_iter()
    .map(|tile| process_tile(tile))
    .collect();
```

### Issue: High Memory Usage

**Diagnosis:**
```bash
# Monitor memory during execution
/usr/bin/time -v ./target/release/app

# Or use valgrind
valgrind --tool=massif ./target/release/app
ms_print massif.out.<pid>
```

**Solutions:**

1. **Stream Instead of Loading:**
```rust
// Bad: loads entire file
let buffer = reader.read_all_bands()?;

// Good: read in chunks
let chunk = reader.read_band_window(0, 0, 512, 512)?;
```

2. **Reduce Buffer Copies:**
```rust
// Bad: multiple copies
let buffer1 = reader.read_tile_buffer(0, 0, 0)?;
let buffer2 = buffer1.clone();  // Copy 1
let buffer3 = buffer2.clone();  // Copy 2

// Good: borrow instead
let buffer1 = reader.read_tile_buffer(0, 0, 0)?;
process_one(&buffer1);
process_two(&buffer1);
```

### Issue: Slow HTTP/Cloud Access

**Diagnosis:**
```bash
# Check network with curl
curl -w '%{time_total}\n' -o /dev/null -s https://example.com/file.tif

# Profile cloud operations
RUST_LOG=debug cargo run --release
```

**Solutions:**

1. **Enable Caching:**
```rust
use oxigdal_cloud::cache::CacheConfig;

let cache = CacheConfig {
    enabled: true,
    max_size_mb: 500,
    ttl_secs: 3600,
};

let http = HttpBackend::new(retry_config, cache);
```

2. **Use Prefetching:**
```rust
use oxigdal_cloud::prefetch::PrefetchConfig;

let prefetch = PrefetchConfig {
    enabled: true,
    num_threads: 4,
    window_size: 10,  // prefetch 10 ahead
};

let tile = http.get_with_prefetch(url, prefetch).await?;
```

3. **Use Appropriate Retry Config:**
```rust
use oxigdal_cloud::retry::RetryConfig;

let retry = RetryConfig {
    max_retries: 3,
    initial_delay_ms: 100,
    max_delay_ms: 5000,
    backoff_multiplier: 2.0,
};
```

## Data Issues

### Issue: Different Values After Conversion

**Problem:**
```
Original: 123.456
After conversion: 123.456001
```

**Solution:**
```rust
// Floating point precision is normal
let original = 123.456f64;
let after = 123.456001f64;

// Use approximate comparison
assert!((original - after).abs() < 1e-6);

// Or use approx crate
use approx::assert_abs_diff_eq;
assert_abs_diff_eq!(original, after, epsilon = 1e-6);
```

### Issue: NoData Values Not Handled

**Error:**
```
Mean: NaN (should be valid number)
```

**Solution:**
```rust
fn compute_stats_ignore_nodata(buffer: &RasterBuffer, nodata: Option<f64>) -> Result<Stats> {
    let mut sum = 0.0;
    let mut count = 0;

    for pixel in buffer.iter() {
        if let Some(nd) = nodata {
            if (pixel - nd).abs() < 1e-10 {
                continue;  // Skip nodata value
            }
        }

        sum += pixel;
        count += 1;
    }

    let mean = if count > 0 { sum / count as f64 } else { 0.0 };

    Ok(Stats { mean, ..Default::default() })
}
```

### Issue: CRS/Projection Mismatch

**Problem:**
```
File says EPSG:4326 but coordinates don't match
```

**Solution:**
```rust
use oxigdal_proj::Projection;

fn verify_crs(reader: &GeoTiffReader) -> Result<()> {
    let epsg = reader.epsg_code();
    println!("EPSG code: {:?}", epsg);

    let wkt = reader.projection_wkt();
    println!("WKT: {}", wkt);

    // Transform a test point
    if let Some(epsg_code) = epsg {
        let proj = Projection::from_epsg(epsg_code)?;

        // Check if coordinate transformation makes sense
        let (x, y) = proj.transform_point(10.0, 20.0, &Projection::from_epsg(3857)?)?;
        println!("Test point transformed: ({}, {})", x, y);
    }

    Ok(())
}
```

## Integration Problems

### Issue: Python-Rust Integration via PyO3

**Problem:**
```
ModuleNotFoundError: No module named 'oxigdal_python'
```

**Solution:**
```bash
# Build the extension
maturin develop

# Or use PyO3 directly
maturin build --release
pip install ./target/wheels/oxigdal_python-*.whl
```

### Issue: JavaScript-Rust Integration via WASM

**Problem:**
```
ReferenceError: oxigdal is not defined
```

**Solution:**
```javascript
// Ensure WASM module is loaded
import init, * as oxigdal from './pkg/oxigdal_wasm.js';

(async () => {
    await init();  // Initialize WASM
    const viewer = oxigdal.WasmCogViewer.new();
    // Now oxigdal functions available
})();
```

## Debugging Tips

### Tip 1: Use Debug Logging

```rust
use tracing::{debug, info, warn, error};

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    debug!("Starting application");

    match process_file("input.tif") {
        Ok(buf) => {
            info!("Successfully loaded: {}x{}", buf.width(), buf.height());
        }
        Err(e) => {
            error!("Failed to load: {}", e);
        }
    }
}

// Run with debug output
RUST_LOG=debug cargo run --release
```

### Tip 2: Print Detailed Error Context

```rust
use std::backtrace::Backtrace;

fn process() -> Result<(), Box<dyn std::error::Error>> {
    let buffer = match FileDataSource::open("file.tif") {
        Ok(source) => GeoTiffReader::open(source)?,
        Err(e) => {
            eprintln!("Failed to open file:");
            eprintln!("  Error: {}", e);
            eprintln!("  Current dir: {}", std::env::current_dir()?.display());
            eprintln!("  Backtrace:\n{}", Backtrace::capture());
            return Err(e);
        }
    };

    Ok(())
}
```

### Tip 3: Add Assertions During Development

```rust
#[cfg(debug_assertions)]
{
    assert_eq!(buffer.width(), expected_width, "Width mismatch");
    assert_eq!(buffer.height(), expected_height, "Height mismatch");
    assert!(buffer.width() > 0, "Width must be positive");
}
```

### Tip 4: Use Debugger

```bash
# Build with debug symbols
cargo build

# Use rust-gdb or lldb
rust-gdb ./target/debug/app
# or
lldb ./target/debug/app

# In debugger:
# (gdb) break main
# (gdb) run
# (gdb) step
# (gdb) print variable_name
```

### Tip 5: Write Minimal Reproduction

```rust
// minimal.rs - Reproduce the issue in isolation
use oxigdal_core::io::FileDataSource;
use oxigdal_geotiff::GeoTiffReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Minimal steps to reproduce
    let source = FileDataSource::open("test.tif")?;
    let reader = GeoTiffReader::open(source)?;
    let buffer = reader.read_tile_buffer(0, 0, 0)?;

    // Issue appears here
    println!("{:?}", buffer);

    Ok(())
}

// Run: cargo run --bin minimal
```

## Getting Help

### Resources

- **Official Documentation**: https://docs.rs/oxigdal
- **GitHub Issues**: https://github.com/cool-japan/oxigdal/issues
- **Discussions**: https://github.com/cool-japan/oxigdal/discussions
- **COOLJAPAN Community**: Contact team

### When Reporting Issues

Include:
1. Rust version: `rustc --version`
2. OxiGDAL version: `cargo tree | grep oxigdal`
3. Operating system: `uname -a`
4. Minimal reproduction code
5. Full error message with backtrace:
```bash
RUST_BACKTRACE=full cargo run 2>&1 | head -100
```

## See Also

- [BEST_PRACTICES.md](BEST_PRACTICES.md) - Avoid common mistakes
- [PERFORMANCE_GUIDE.md](PERFORMANCE_GUIDE.md) - Optimization tips
- [GETTING_STARTED.md](GETTING_STARTED.md) - Quick start guide
