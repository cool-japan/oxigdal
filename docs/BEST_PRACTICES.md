# Best Practices for OxiGDAL Development

Guidelines and patterns for writing robust, performant, and maintainable OxiGDAL applications.

## Table of Contents

- [Error Handling](#error-handling)
- [Memory Management](#memory-management)
- [Performance](#performance)
- [Code Organization](#code-organization)
- [Testing](#testing)
- [Documentation](#documentation)
- [Security](#security)
- [Deployment](#deployment)

## Error Handling

### Best Practice 1: Use Result<T, E> Consistently

```rust
// Good: Explicit error handling
fn process_file(path: &str) -> Result<Vec<f64>, Box<dyn std::error::Error>> {
    let source = FileDataSource::open(path)?;
    let reader = GeoTiffReader::open(source)?;
    let buffer = reader.read_tile_buffer(0, 0, 0)?;

    let values: Vec<f64> = buffer.iter().collect();
    Ok(values)
}

// Avoid: Panicking on errors
fn bad_process_file(path: &str) -> Vec<f64> {
    let source = FileDataSource::open(path).unwrap();  // Panics if fails!
    let buffer = source.read_tile_buffer(0, 0, 0).unwrap();
    buffer.iter().collect()
}
```

### Best Practice 2: Provide Context in Errors

```rust
use std::io;

fn validate_bounds(bbox: &BoundingBox) -> Result<(), Box<dyn std::error::Error>> {
    if bbox.min_x >= bbox.max_x {
        return Err(format!(
            "Invalid bounding box: min_x ({}) >= max_x ({})",
            bbox.min_x, bbox.max_x
        ).into());
    }
    Ok(())
}
```

### Best Practice 3: Use Custom Error Types

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProcessingError {
    #[error("Invalid raster dimensions: {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },

    #[error("Unsupported data type: {0:?}")]
    UnsupportedDataType(RasterDataType),

    #[error("File I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("OxiGDAL error: {0}")]
    OxiGdalError(#[from] OxiGdalError),
}

// Usage
fn load_and_validate(path: &str) -> Result<RasterBuffer, ProcessingError> {
    let source = FileDataSource::open(path)?;  // IoError converted automatically
    let reader = GeoTiffReader::open(source)?;  // OxiGdalError converted

    if reader.width() == 0 || reader.height() == 0 {
        return Err(ProcessingError::InvalidDimensions {
            width: reader.width(),
            height: reader.height(),
        });
    }

    reader.read_tile_buffer(0, 0, 0).map_err(Into::into)
}
```

### Best Practice 4: Recoverable vs Fatal Errors

```rust
// Recoverable: can continue
fn try_read_metadata(path: &str) -> Option<Metadata> {
    let source = FileDataSource::open(path).ok()?;
    let reader = GeoTiffReader::open(source).ok()?;
    Some(reader.metadata())
}

// Fatal: operation cannot continue
fn must_read_data(path: &str) -> Result<RasterBuffer> {
    let source = FileDataSource::open(path)?;
    let reader = GeoTiffReader::open(source)?;
    reader.read_tile_buffer(0, 0, 0)
}
```

## Memory Management

### Best Practice 1: Avoid Large Stack Allocations

```rust
// Good: use Vec for large data
let mut buffer = Vec::with_capacity(1024 * 1024);  // 1MB on heap

// Avoid: stack allocation of large arrays
fn bad_function() {
    let large_array: [f64; 1024*1024] = [0.0; 1024*1024];  // 8MB on stack!
}
```

### Best Practice 2: Use RasterBuffer for Pixel Data

```rust
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

// Good: appropriate data structure
let buffer = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
let stats = buffer.compute_statistics()?;

// Avoid: manual Vec<Vec<>> or nested vectors
let bad_buffer = vec![vec![0.0; 512]; 512];  // Poor cache locality
```

### Best Practice 3: Explicit Lifetime Management

```rust
// Good: clear ownership
fn process_buffers(nir: RasterBuffer, red: RasterBuffer) -> Result<RasterBuffer> {
    // nir and red are owned, will be dropped at end of function
    let mut result = RasterBuffer::zeros(nir.width(), nir.height(), RasterDataType::Float32);

    for (n, r) in nir.iter().zip(red.iter()) {
        let ndvi = (n - r) / (n + r + 1e-10);
        result.iter_mut().next().map(|p| *p = ndvi);
    }

    Ok(result)
}

// Borrowing instead of owning
fn analyze_buffer(buffer: &RasterBuffer) -> Result<Statistics> {
    buffer.compute_statistics()  // Borrows immutably, doesn't take ownership
}
```

### Best Practice 4: Pool Resources

```rust
use std::collections::VecDeque;

pub struct BufferPool {
    buffers: VecDeque<RasterBuffer>,
}

impl BufferPool {
    pub fn new(capacity: usize, width: u32, height: u32) -> Self {
        let buffers = (0..capacity)
            .map(|_| RasterBuffer::zeros(width, height, RasterDataType::Float32))
            .collect();

        Self { buffers }
    }

    pub fn acquire(&mut self) -> Option<RasterBuffer> {
        self.buffers.pop_front()
    }

    pub fn release(&mut self, buffer: RasterBuffer) {
        if self.buffers.len() < 10 {
            self.buffers.push_back(buffer);
        }
    }
}
```

## Performance

### Best Practice 1: Use Release Mode

```bash
# Always use release mode for performance-sensitive code
cargo build --release
cargo run --release

# Enable native CPU optimizations
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

### Best Practice 2: Profile Before Optimizing

```bash
# Generate flamegraph
cargo flamegraph --release -- --workload-size 1000

# Run benchmarks
cargo bench --release

# Check for hot paths
perf record -g ./target/release/app
perf report
```

### Best Practice 3: Use SIMD Operations

```rust
use oxigdal_core::simd_buffer::SimdBuffer;

// Good: SIMD-accelerated operations
let buffer = RasterBuffer::zeros(1024, 1024, RasterDataType::Float32);
let simd_buffer = SimdBuffer::from_buffer(&buffer)?;
let result = simd_buffer.add_scalar(10.0)?;

// Avoid: naive pixel-by-pixel loops for large operations
let mut result = buffer.clone();
for pixel in result.iter_mut() {
    *pixel += 10.0;  // Not auto-vectorized
}
```

### Best Practice 4: Parallelize Data Processing

```rust
use rayon::prelude::*;

// Good: parallel tile processing
fn process_tiles_parallel(reader: &GeoTiffReader) -> Result<Vec<RasterBuffer>> {
    let tiles: Vec<_> = (0..10)
        .flat_map(|ty| (0..10).map(move |tx| (tx, ty)))
        .collect();

    tiles
        .par_iter()
        .map(|(tx, ty)| reader.read_tile_buffer(*tx, *ty, 0))
        .collect()
}

// Avoid: sequential processing
fn process_tiles_sequential(reader: &GeoTiffReader) -> Result<Vec<RasterBuffer>> {
    let mut results = Vec::new();
    for ty in 0..10 {
        for tx in 0..10 {
            results.push(reader.read_tile_buffer(tx, ty, 0)?);
        }
    }
    Ok(results)
}
```

### Best Practice 5: Stream Large Datasets

```rust
// Good: process in chunks to minimize memory
fn process_large_file_streaming(path: &str, chunk_size: u32) -> Result<()> {
    let source = FileDataSource::open(path)?;
    let reader = GeoTiffReader::open(source)?;
    let height = reader.height();

    for y in (0..height).step_by(chunk_size as usize) {
        let h = std::cmp::min(chunk_size, height - y);
        let chunk = read_band_window(&reader, 0, y, reader.width(), h)?;
        process_chunk(&chunk)?;
    }

    Ok(())
}

// Avoid: loading entire file into memory
fn process_large_file_naive(path: &str) -> Result<()> {
    let source = FileDataSource::open(path)?;
    let reader = GeoTiffReader::open(source)?;
    let buffer = reader.read_tile_buffer(0, 0, 0)?;  // May be gigabytes!
    process_chunk(&buffer)?;
    Ok(())
}
```

## Code Organization

### Best Practice 1: Modular Project Structure

```rust
// src/main.rs
mod raster;
mod vector;
mod io;
mod processing;

use raster::RasterOps;
use vector::VectorOps;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    match args[1].as_str() {
        "ndvi" => processing::calculate_ndvi(&args[2], &args[3])?,
        "reproject" => processing::reproject(&args[2], &args[3])?,
        _ => eprintln!("Unknown command"),
    }
    Ok(())
}

// src/raster.rs
pub trait RasterOps {
    fn apply_filter(&self, kernel: &[f64]) -> Result<RasterBuffer>;
    fn histogram(&self) -> Result<Histogram>;
}

// src/vector.rs
pub trait VectorOps {
    fn buffer(&self, distance: f64) -> Result<Geometry>;
    fn intersect(&self, other: &Geometry) -> Result<Geometry>;
}
```

### Best Practice 2: Semantic Naming

```rust
// Good: clear, domain-specific names
fn calculate_ndvi(nir_path: &str, red_path: &str) -> Result<RasterBuffer> {
    // ...
}

// Avoid: vague names
fn process_files(f1: &str, f2: &str) -> Result<RasterBuffer> {
    // ...
}
```

### Best Practice 3: Separate Concerns

```rust
// Good: separate I/O, processing, and output
fn read_file(path: &str) -> Result<RasterBuffer> { ... }
fn process_data(buffer: &RasterBuffer) -> Result<RasterBuffer> { ... }
fn write_file(buffer: &RasterBuffer, path: &str) -> Result<()> { ... }

fn main() -> Result<()> {
    let input = read_file("input.tif")?;
    let output = process_data(&input)?;
    write_file(&output, "output.tif")?;
    Ok(())
}

// Avoid: mixing concerns
fn process_file(input_path: &str, output_path: &str) -> Result<()> {
    let buffer = read_file(input_path)?;
    // ... processing interleaved with I/O ...
    // ... mixed with validation and logging ...
}
```

## Testing

### Best Practice 1: Unit Test Core Functions

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ndvi_calculation() -> Result<()> {
        let nir = RasterBuffer::filled(10, 10, 100.0)?;
        let red = RasterBuffer::filled(10, 10, 50.0)?;

        let ndvi = calculate_ndvi(&nir, &red)?;

        // NDVI = (100-50)/(100+50) = 0.333...
        assert!((ndvi.get_pixel(0, 0)? - 0.333).abs() < 0.01);
        Ok(())
    }

    #[test]
    fn test_bounds_validation() {
        let valid = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        assert!(valid.is_ok());

        let invalid = BoundingBox::new(10.0, 0.0, 0.0, 10.0);  // min_x > max_x
        assert!(invalid.is_err());
    }
}
```

### Best Practice 2: Integration Tests

```rust
// tests/integration_test.rs
use oxigdal_core::io::FileDataSource;
use oxigdal_geotiff::GeoTiffReader;

#[test]
fn test_read_write_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let temp_file = std::env::temp_dir().join("test_roundtrip.tif");

    // Write
    let original = create_test_buffer();
    write_geotiff(&temp_file, &original)?;

    // Read back
    let source = FileDataSource::open(&temp_file)?;
    let reader = GeoTiffReader::open(source)?;
    let restored = reader.read_tile_buffer(0, 0, 0)?;

    // Verify
    for (o, r) in original.iter().zip(restored.iter()) {
        assert!((o - r).abs() < 1e-6);
    }

    Ok(())
}
```

### Best Practice 3: Use Temporary Files in Tests

```rust
use std::fs::File;
use std::io::Write;

#[test]
fn test_file_processing() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("test_processing.bin");

    // Create test data
    let mut file = File::create(&test_file)?;
    file.write_all(b"test data")?;

    // Process
    let result = process_file(&test_file)?;

    // Verify
    assert_eq!(result.len(), 9);

    // Cleanup (automatic when dropped or explicitly)
    std::fs::remove_file(&test_file).ok();

    Ok(())
}
```

## Documentation

### Best Practice 1: Document Public APIs

```rust
/// Calculates Normalized Difference Vegetation Index (NDVI).
///
/// NDVI = (NIR - RED) / (NIR + RED)
///
/// # Arguments
///
/// * `nir` - Near-infrared band raster buffer
/// * `red` - Red band raster buffer
///
/// # Returns
///
/// A new `RasterBuffer` containing NDVI values in range [-1, 1]
///
/// # Errors
///
/// Returns an error if buffers have different dimensions
///
/// # Examples
///
/// ```
/// # use oxigdal_core::buffer::RasterBuffer;
/// # use oxigdal_core::types::RasterDataType;
/// let nir = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
/// let red = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
/// let ndvi = calculate_ndvi(&nir, &red)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn calculate_ndvi(nir: &RasterBuffer, red: &RasterBuffer)
    -> Result<RasterBuffer>
{
    // implementation
}
```

### Best Practice 2: Write README Examples

```markdown
## Examples

### Reading a GeoTIFF

\`\`\`rust
let source = FileDataSource::open("image.tif")?;
let reader = GeoTiffReader::open(source)?;
let buffer = reader.read_tile_buffer(0, 0, 0)?;
\`\`\`

### Computing NDVI

\`\`\`rust
let ndvi = calculate_ndvi(&nir_buffer, &red_buffer)?;
\`\`\`
```

## Security

### Best Practice 1: Validate Input

```rust
pub fn process_bounds(bbox: &BoundingBox) -> Result<()> {
    // Validate coordinate order
    if bbox.min_x >= bbox.max_x || bbox.min_y >= bbox.max_y {
        return Err("Invalid bounding box coordinates".into());
    }

    // Validate reasonable ranges
    if bbox.min_x < -180.0 || bbox.max_x > 180.0 {
        return Err("Longitude out of valid range [-180, 180]".into());
    }

    if bbox.min_y < -90.0 || bbox.max_y > 90.0 {
        return Err("Latitude out of valid range [-90, 90]".into());
    }

    Ok(())
}
```

### Best Practice 2: Limit Resource Usage

```rust
const MAX_BUFFER_SIZE: u32 = 10_000;
const MAX_TILE_SIZE: u32 = 4096;

pub fn validate_raster_request(width: u32, height: u32) -> Result<()> {
    if width > MAX_TILE_SIZE || height > MAX_TILE_SIZE {
        return Err("Requested tile too large".into());
    }

    let total = width.saturating_mul(height);
    if total > MAX_BUFFER_SIZE * MAX_BUFFER_SIZE {
        return Err("Requested area too large".into());
    }

    Ok(())
}
```

### Best Practice 3: Use Safe File Operations

```rust
// Good: use std::fs safely
fn read_file_safe(path: &Path) -> Result<Vec<u8>> {
    use std::path::Component;

    // Prevent directory traversal
    for component in path.components() {
        if component == Component::ParentDir {
            return Err("Directory traversal not allowed".into());
        }
    }

    std::fs::read(path).map_err(|e| e.into())
}

// Avoid: accepting arbitrary paths without validation
fn read_file_unsafe(path: &str) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| e.into())
}
```

## Deployment

### Best Practice 1: Build Optimization

```bash
# For binary size
cargo build --release -Z build-std=std,panic_abort --target x86_64-unknown-linux-gnu
strip target/x86_64-unknown-linux-gnu/release/oxigdal

# For runtime speed
RUSTFLAGS="-C target-cpu=native -C llvm-args=-mcpu=native" cargo build --release

# For both
cargo build --release -C lto=fat -C codegen-units=1
```

### Best Practice 2: Dependency Management

```toml
[dependencies]
# Pin critical dependencies
oxigdal-core = "=0.1.0"

# Allow patch versions for stable APIs
tokio = "~1.40"

# Allow minor versions for new features
serde = "^1.0"

[dev-dependencies]
# Test dependencies don't affect release binary
proptest = "1"
criterion = { version = "0.7", features = ["html_reports"] }
```

### Best Practice 3: Logging and Monitoring

```rust
use tracing::{info, warn, error, debug};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!("Starting OxiGDAL application");

    match process_file("input.tif") {
        Ok(buffer) => {
            info!("Successfully processed file: {}x{}", buffer.width(), buffer.height());
        }
        Err(e) => {
            error!("Failed to process file: {}", e);
            return Err(e);
        }
    }

    Ok(())
}
```

## See Also

- [ARCHITECTURE.md](ARCHITECTURE.md) - System design
- [PERFORMANCE_GUIDE.md](PERFORMANCE_GUIDE.md) - Optimization techniques
- [TROUBLESHOOTING.md](TROUBLESHOOTING.md) - Common issues
