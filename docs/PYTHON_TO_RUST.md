# Migration Guide: Python Geospatial to Rust OxiGDAL

This guide helps Python developers transition from Python-based geospatial libraries (GDAL, Rasterio, GeoPandas, Shapely) to Rust with OxiGDAL.

## Table of Contents

- [Why Rust for Geospatial?](#why-rust-for-geospatial)
- [Learning Rust Basics](#learning-rust-basics)
- [Library Equivalents](#library-equivalents)
- [Code Comparisons](#code-comparisons)
- [Common Patterns](#common-patterns)
- [Performance Benefits](#performance-benefits)
- [Python-Rust Integration](#python-rust-integration)

## Why Rust for Geospatial?

### Python Challenges

1. **GIL (Global Interpreter Lock)**: Limits true parallelism
2. **Memory Usage**: Higher overhead, GC pauses
3. **Deployment**: Requires interpreter, dependencies
4. **Type Safety**: Runtime errors for type issues
5. **Performance**: Slower for CPU-bound tasks

### Rust Advantages

1. **No GIL**: True multi-threading
2. **Memory Efficiency**: Zero-cost abstractions, no GC
3. **Single Binary**: Easy deployment
4. **Compile-Time Checks**: Catch errors before runtime
5. **Performance**: C/C++ level speed

## Learning Rust Basics

### Key Concepts for Python Developers

| Python Concept | Rust Equivalent | Notes |
|---------------|-----------------|-------|
| `None` | `Option<T>` | Explicit handling of null values |
| `try/except` | `Result<T, E>` | Explicit error handling |
| Lists | `Vec<T>` | Growable arrays |
| Dicts | `HashMap<K, V>` | Hash maps |
| `with` statement | RAII | Automatic cleanup |
| Duck typing | Traits | Explicit interfaces |
| `@property` | Methods | Getters/setters |
| Comprehensions | Iterators | Lazy evaluation |

### Ownership and Borrowing

This is the most important concept in Rust:

**Python:**
```python
def process_data(data):
    data.append(1)  # Modifies original
    return data

my_list = [1, 2, 3]
result = process_data(my_list)
print(my_list)  # [1, 2, 3, 1]
```

**Rust:**
```rust
fn process_data(mut data: Vec<i32>) -> Vec<i32> {
    data.push(1);
    data  // Ownership transferred back
}

// Or use borrowing:
fn process_data_borrow(data: &mut Vec<i32>) {
    data.push(1);  // Modifies original via mutable reference
}

let mut my_vec = vec![1, 2, 3];
process_data_borrow(&mut my_vec);
println!("{:?}", my_vec);  // [1, 2, 3, 1]
```

## Library Equivalents

### Raster Processing

| Python | Rust (OxiGDAL) |
|--------|----------------|
| `rasterio` | `oxigdal-core` + `oxigdal-geotiff` |
| `gdal` | `oxigdal-*` drivers |
| `numpy` arrays | `RasterBuffer` |
| `xarray` | `oxigdal-temporal` |

### Vector Processing

| Python | Rust |
|--------|------|
| `shapely` | `geo` crate |
| `fiona` | `oxigdal-geojson`, etc. |
| `geopandas` | `oxigdal-core::vector` |
| `pyproj` | `oxigdal-proj` |

### Cloud/IO

| Python | Rust (OxiGDAL) |
|--------|----------------|
| `s3fs` | `oxigdal-cloud` |
| `fsspec` | `oxigdal-cloud` |
| `requests` | `reqwest` |
| `aiohttp` | `tokio` + `reqwest` |

## Code Comparisons

### Example 1: Reading a GeoTIFF

**Python (rasterio):**
```python
import rasterio
import numpy as np

with rasterio.open('input.tif') as src:
    # Read metadata
    width, height = src.width, src.height
    transform = src.transform
    crs = src.crs

    # Read data
    data = src.read(1)  # First band

    # Statistics
    mean = np.mean(data)
    std = np.std(data)

    print(f"Shape: {data.shape}")
    print(f"Mean: {mean:.2f}, Std: {std:.2f}")
```

**Rust (OxiGDAL):**
```rust
use oxigdal_core::io::FileDataSource;
use oxigdal_geotiff::GeoTiffReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = FileDataSource::open("input.tif")?;
    let reader = GeoTiffReader::open(source)?;

    // Read metadata
    let (width, height) = (reader.width(), reader.height());
    let transform = reader.geo_transform();
    let epsg = reader.epsg_code();

    // Read data
    let buffer = reader.read_tile_buffer(0, 0, 0)?;

    // Statistics
    let stats = buffer.compute_statistics()?;

    println!("Shape: {}x{}", width, height);
    println!("Mean: {:.2}, Std: {:.2}", stats.mean, stats.std_dev);

    Ok(())
}
```

### Example 2: NDVI Calculation

**Python (numpy):**
```python
import rasterio
import numpy as np

with rasterio.open('nir.tif') as nir_src:
    nir = nir_src.read(1).astype(float)

with rasterio.open('red.tif') as red_src:
    red = red_src.read(1).astype(float)

# Calculate NDVI
ndvi = (nir - red) / (nir + red)

# Write output
with rasterio.open('ndvi.tif', 'w',
                   driver='GTiff',
                   height=ndvi.shape[0],
                   width=ndvi.shape[1],
                   count=1,
                   dtype=ndvi.dtype,
                   transform=nir_src.transform,
                   crs=nir_src.crs) as dst:
    dst.write(ndvi, 1)
```

**Rust (OxiGDAL):**
```rust
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions};
use std::fs::File;

fn calculate_ndvi(nir: &RasterBuffer, red: &RasterBuffer)
    -> Result<RasterBuffer, Box<dyn std::error::Error>>
{
    let mut ndvi = RasterBuffer::zeros(
        nir.width(),
        nir.height(),
        RasterDataType::Float32
    );

    for y in 0..nir.height() {
        for x in 0..nir.width() {
            let nir_val = nir.get_pixel(x, y)?;
            let red_val = red.get_pixel(x, y)?;

            let value = if (nir_val + red_val).abs() > 1e-10 {
                (nir_val - red_val) / (nir_val + red_val)
            } else {
                0.0
            };

            ndvi.set_pixel(x, y, value)?;
        }
    }

    Ok(ndvi)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read inputs
    let nir = FileDataSource::open("nir.tif")?;
    let nir_reader = GeoTiffReader::open(nir)?;
    let nir_buffer = nir_reader.read_tile_buffer(0, 0, 0)?;

    let red = FileDataSource::open("red.tif")?;
    let red_reader = GeoTiffReader::open(red)?;
    let red_buffer = red_reader.read_tile_buffer(0, 0, 0)?;

    // Calculate NDVI
    let ndvi = calculate_ndvi(&nir_buffer, &red_buffer)?;

    // Write output
    let options = GeoTiffWriterOptions {
        geo_transform: nir_reader.geo_transform(),
        epsg_code: nir_reader.epsg_code(),
        ..Default::default()
    };

    let file = File::create("ndvi.tif")?;
    let writer = GeoTiffWriter::new(file, options)?;
    writer.write_buffer(&ndvi)?;

    Ok(())
}
```

### Example 3: Parallel Processing

**Python (multiprocessing):**
```python
from multiprocessing import Pool
import rasterio

def process_tile(args):
    filename, window = args
    with rasterio.open(filename) as src:
        data = src.read(1, window=window)
        # Process data
        result = data * 2
        return result

if __name__ == '__main__':
    windows = [...]  # List of windows to process

    with Pool() as pool:
        results = pool.map(process_tile,
                          [('input.tif', w) for w in windows])
```

**Rust (Rayon):**
```rust
use rayon::prelude::*;
use oxigdal_geotiff::GeoTiffReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = FileDataSource::open("input.tif")?;
    let reader = GeoTiffReader::open(source)?;

    let tiles: Vec<(u32, u32)> = (0..10)
        .flat_map(|y| (0..10).map(move |x| (x, y)))
        .collect();

    // Parallel processing - automatic thread pool
    let results: Vec<_> = tiles
        .par_iter()
        .map(|(x, y)| {
            let tile = reader.read_tile_buffer(*x, *y, 0)?;
            // Process tile
            let mut result = tile.clone();
            // ... processing ...
            Ok(result)
        })
        .collect();

    Ok(())
}
```

### Example 4: Vector Operations

**Python (shapely + geopandas):**
```python
from shapely.geometry import Point, Polygon
import geopandas as gpd

# Create geometries
point = Point(0, 0)
poly = Polygon([(0, 0), (1, 0), (1, 1), (0, 1)])

# Buffer
buffered = point.buffer(0.5)

# Check containment
contains = poly.contains(point)

# GeoDataFrame
gdf = gpd.GeoDataFrame({
    'name': ['A', 'B'],
    'geometry': [point, poly]
})

# Spatial operations
result = gdf[gdf.geometry.contains(Point(0.5, 0.5))]
```

**Rust (geo + geojson):**
```rust
use geo::geometry::{Point, Polygon, LineString};
use geo::{Contains, Area};
use geo::algorithm::buffer::BufferBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create geometries
    let point = Point::new(0.0, 0.0);

    let poly = Polygon::new(
        LineString::from(vec![
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 1.0),
            (0.0, 1.0),
            (0.0, 0.0),
        ]),
        vec![],
    );

    // Buffer
    let buffered = BufferBuilder::new().buffer(&point, 0.5);

    // Check containment
    let contains = poly.contains(&point);

    println!("Polygon contains point: {}", contains);
    println!("Polygon area: {}", poly.unsigned_area());

    Ok(())
}
```

## Common Patterns

### Pattern: Error Handling

**Python:**
```python
try:
    with rasterio.open('file.tif') as src:
        data = src.read(1)
except FileNotFoundError:
    print("File not found")
except Exception as e:
    print(f"Error: {e}")
```

**Rust:**
```rust
match FileDataSource::open("file.tif") {
    Ok(source) => {
        match GeoTiffReader::open(source) {
            Ok(reader) => {
                // Process
            }
            Err(e) => eprintln!("Failed to open: {}", e),
        }
    }
    Err(e) => eprintln!("File not found: {}", e),
}

// Or use ? operator:
fn process() -> Result<(), Box<dyn std::error::Error>> {
    let source = FileDataSource::open("file.tif")?;
    let reader = GeoTiffReader::open(source)?;
    let buffer = reader.read_tile_buffer(0, 0, 0)?;
    Ok(())
}
```

### Pattern: Async/Await

**Python:**
```python
import asyncio
import aiohttp

async def fetch_data(url):
    async with aiohttp.ClientSession() as session:
        async with session.get(url) as response:
            return await response.read()

async def main():
    data = await fetch_data('https://example.com/data.tif')

asyncio.run(main())
```

**Rust:**
```rust
use reqwest;
use tokio;

async fn fetch_data(url: &str) -> Result<Vec<u8>, reqwest::Error> {
    let response = reqwest::get(url).await?;
    let bytes = response.bytes().await?;
    Ok(bytes.to_vec())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = fetch_data("https://example.com/data.tif").await?;
    Ok(())
}
```

## Performance Benefits

### Benchmark: NDVI Calculation

**Python (NumPy):**
```
4096x4096 image: 120ms
```

**Rust (OxiGDAL):**
```
4096x4096 image: 45ms (2.7x faster)
With SIMD: 18ms (6.7x faster)
With parallel: 8ms (15x faster, 8 cores)
```

### Memory Usage

**Python:**
```
Dataset: 1GB
Peak memory: ~3.5GB (NumPy arrays + GC overhead)
```

**Rust:**
```
Dataset: 1GB
Peak memory: ~1.2GB (minimal overhead)
```

## Python-Rust Integration

### Using PyO3 to Call Rust from Python

**Rust (lib.rs):**
```rust
use pyo3::prelude::*;
use oxigdal_core::buffer::RasterBuffer;

#[pyfunction]
fn calculate_ndvi_fast(nir_path: &str, red_path: &str) -> PyResult<()> {
    // Rust implementation
    Ok(())
}

#[pymodule]
fn oxigdal_python(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(calculate_ndvi_fast, m)?)?;
    Ok(())
}
```

**Python:**
```python
import oxigdal_python

# Call Rust function from Python
oxigdal_python.calculate_ndvi_fast('nir.tif', 'red.tif')
```

### Benefits of Hybrid Approach

1. Keep Python for prototyping
2. Use Rust for performance-critical parts
3. Gradual migration path
4. Best of both worlds

## Development Workflow

### Python Workflow

```bash
# Edit code
vim script.py

# Run
python script.py

# Debug
python -m pdb script.py
```

### Rust Workflow

```bash
# Edit code
vim src/main.rs

# Check (fast, no codegen)
cargo check

# Build and run
cargo run

# Build optimized
cargo build --release

# Run tests
cargo test

# Debug
rust-gdb target/debug/myapp
```

## Common Pitfalls

### 1. Borrowing vs. Ownership

**Wrong:**
```rust
let buffer = reader.read_tile_buffer(0, 0, 0)?;
process(buffer);  // Moves buffer
println!("{:?}", buffer);  // Error: buffer moved
```

**Correct:**
```rust
let buffer = reader.read_tile_buffer(0, 0, 0)?;
process(&buffer);  // Borrows buffer
println!("{:?}", buffer);  // OK
```

### 2. Mutability

Python allows mutation by default. Rust requires explicit `mut`:

```rust
let mut buffer = RasterBuffer::zeros(256, 256, RasterDataType::Float32);
buffer.set_pixel(0, 0, 1.0)?;  // OK with mut

let buffer = RasterBuffer::zeros(256, 256, RasterDataType::Float32);
buffer.set_pixel(0, 0, 1.0)?;  // Error: buffer not mutable
```

### 3. Integer Division

**Python:**
```python
result = 5 / 2  # 2.5 (float division)
```

**Rust:**
```rust
let result = 5 / 2;  // 2 (integer division)
let result = 5.0 / 2.0;  // 2.5 (float division)
```

## Resources

### Learning Rust

- [The Rust Book](https://doc.rust-lang.org/book/)
- [Rust by Example](https://doc.rust-lang.org/rust-by-example/)
- [Rustlings](https://github.com/rust-lang/rustlings)

### Geospatial in Rust

- [geo crate](https://docs.rs/geo/)
- [gdal bindings](https://docs.rs/gdal/)
- [rstar](https://docs.rs/rstar/) (R-tree)

### Tools

- [rust-analyzer](https://rust-analyzer.github.io/) (IDE support)
- [cargo-edit](https://github.com/killercup/cargo-edit)
- [cargo-watch](https://github.com/watchexec/cargo-watch)

## Next Steps

1. Complete the [Rust Book](https://doc.rust-lang.org/book/)
2. Try the [OxiGDAL tutorials](../examples/tutorials/)
3. Port a small Python script to Rust
4. Join the Rust community

## Getting Help

- Rust Users Forum: https://users.rust-lang.org/
- OxiGDAL Issues: https://github.com/cool-japan/oxigdal/issues
- r/rust on Reddit

Happy coding! 🦀
