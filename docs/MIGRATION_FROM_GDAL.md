# Migration Guide: From GDAL/OGR to OxiGDAL

This guide helps you migrate from GDAL/OGR (C/C++ or Python bindings) to OxiGDAL (Pure Rust).

## Table of Contents

- [Why Migrate to OxiGDAL?](#why-migrate-to-oxigdal)
- [Key Differences](#key-differences)
- [API Mapping](#api-mapping)
- [Common Patterns](#common-patterns)
- [Performance Considerations](#performance-considerations)
- [Troubleshooting](#troubleshooting)

## Why Migrate to OxiGDAL?

### Advantages of OxiGDAL

1. **Memory Safety**: Rust's ownership system prevents common errors
   - No buffer overflows
   - No null pointer dereferences
   - Thread safety guaranteed at compile time

2. **Performance**:
   - Zero-cost abstractions
   - SIMD optimization
   - Parallel processing built-in
   - No GIL (unlike Python GDAL)

3. **Deployment**:
   - Single binary (no dependencies)
   - Cross-compilation support
   - Smaller footprint
   - Easier containerization

4. **Type Safety**:
   - Compile-time error checking
   - Better IDE support
   - Reduced runtime errors

5. **Modern Ecosystem**:
   - Async/await support
   - Cloud-native features
   - WebAssembly support

## Key Differences

### Conceptual Differences

| Aspect | GDAL/OGR | OxiGDAL |
|--------|----------|---------|
| Memory | Manual/GC | Automatic (ownership) |
| Errors | Return codes/exceptions | Result<T, E> |
| Null values | NULL pointers | Option<T> |
| Concurrency | Manual locking | Compile-time safe |
| Drivers | Plugin-based | Feature-based |

### Installation

**GDAL:**
```bash
# System dependencies required
apt-get install gdal-bin libgdal-dev
pip install gdal
```

**OxiGDAL:**
```bash
# Pure Rust, no system dependencies
cargo add oxigdal-core oxigdal-geotiff
```

## API Mapping

### Opening Files

**GDAL (C++):**
```cpp
GDALDataset *dataset = (GDALDataset*)GDALOpen("file.tif", GA_ReadOnly);
if (dataset == NULL) {
    // Handle error
}
```

**GDAL (Python):**
```python
from osgeo import gdal
dataset = gdal.Open("file.tif")
if not dataset:
    raise Exception("Failed to open")
```

**OxiGDAL:**
```rust
use oxigdal_core::io::FileDataSource;
use oxigdal_geotiff::GeoTiffReader;

let source = FileDataSource::open("file.tif")?;
let reader = GeoTiffReader::open(source)?;
// Errors handled via Result type
```

### Reading Metadata

**GDAL (Python):**
```python
width = dataset.RasterXSize
height = dataset.RasterYSize
bands = dataset.RasterCount
transform = dataset.GetGeoTransform()
projection = dataset.GetProjection()
```

**OxiGDAL:**
```rust
let width = reader.width();
let height = reader.height();
let bands = reader.band_count();
let transform = reader.geo_transform();
let epsg = reader.epsg_code();
```

### Reading Raster Data

**GDAL (Python):**
```python
band = dataset.GetRasterBand(1)
array = band.ReadAsArray()

# Read window
window = band.ReadAsArray(xoff=100, yoff=100, xsize=256, ysize=256)
```

**OxiGDAL:**
```rust
// Read full raster
let buffer = reader.read_tile_buffer(0, 0, 0)?;

// Read window
let window = buffer.window(100, 100, 256, 256)?;
```

### Writing Raster Data

**GDAL (Python):**
```python
driver = gdal.GetDriverByName('GTiff')
dataset = driver.Create('output.tif', width, height, 1, gdal.GDT_Float32)
dataset.SetGeoTransform(transform)
dataset.SetProjection(projection)

band = dataset.GetRasterBand(1)
band.WriteArray(array)
band.FlushCache()

dataset = None  # Close
```

**OxiGDAL:**
```rust
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions};
use std::fs::File;

let options = GeoTiffWriterOptions {
    geo_transform: Some(transform),
    epsg_code: Some(4326),
    ..Default::default()
};

let file = File::create("output.tif")?;
let writer = GeoTiffWriter::new(file, options)?;
writer.write_buffer(&buffer)?;
// Automatically closed via RAII
```

### Reprojection

**GDAL (Python):**
```python
from osgeo import osr, gdal

src_ds = gdal.Open('input.tif')
dst_ds = gdal.Warp('output.tif', src_ds,
                   srcSRS='EPSG:4326',
                   dstSRS='EPSG:3857')
```

**OxiGDAL:**
```rust
use oxigdal_algorithms::reproject::{reproject, ReprojectOptions, Resampling};
use oxigdal_proj::Projection;

let src_proj = Projection::from_epsg(4326)?;
let dst_proj = Projection::from_epsg(3857)?;

let options = ReprojectOptions {
    src_projection: &src_proj,
    dst_projection: &dst_proj,
    src_geo_transform: geo_transform,
    dst_width: width,
    dst_height: height,
    resampling: Resampling::Bilinear,
    nodata: None,
};

let reprojected = reproject(&buffer, &options)?;
```

### Vector Operations

**GDAL (Python):**
```python
from osgeo import ogr

driver = ogr.GetDriverByName('GeoJSON')
datasource = driver.Open('input.geojson')
layer = datasource.GetLayer()

for feature in layer:
    geom = feature.GetGeometryRef()
    area = geom.GetArea()
```

**OxiGDAL:**
```rust
use geo::Area;
use geojson::GeoJson;
use std::fs;

let geojson_str = fs::read_to_string("input.geojson")?;
let geojson = geojson_str.parse::<GeoJson>()?;

if let GeoJson::FeatureCollection(fc) = geojson {
    for feature in fc.features {
        if let Some(geom) = feature.geometry {
            // Convert to geo types and process
        }
    }
}
```

## Common Patterns

### Pattern 1: Tile Processing

**GDAL (Python):**
```python
def process_tiles(dataset, tile_size=256):
    width = dataset.RasterXSize
    height = dataset.RasterYSize
    band = dataset.GetRasterBand(1)

    for y in range(0, height, tile_size):
        for x in range(0, width, tile_size):
            w = min(tile_size, width - x)
            h = min(tile_size, height - y)

            tile = band.ReadAsArray(x, y, w, h)
            # Process tile
            result = process(tile)
            # Write back if needed
```

**OxiGDAL:**
```rust
use rayon::prelude::*;

fn process_tiles(reader: &GeoTiffReader, tile_size: u32) -> Result<(), Error> {
    let width = reader.width();
    let height = reader.height();

    let tiles: Vec<(u32, u32)> = (0..height/tile_size)
        .flat_map(|ty| (0..width/tile_size).map(move |tx| (tx, ty)))
        .collect();

    // Parallel processing
    tiles.par_iter().try_for_each(|(tx, ty)| {
        let tile = reader.read_tile_buffer(*tx, *ty, 0)?;
        // Process tile
        let result = process(&tile)?;
        Ok(())
    })
}
```

### Pattern 2: Band Math

**GDAL (Python):**
```python
import numpy as np

red = dataset.GetRasterBand(4).ReadAsArray()
nir = dataset.GetRasterBand(5).ReadAsArray()

ndvi = (nir - red) / (nir + red)
```

**OxiGDAL:**
```rust
fn calculate_ndvi(nir: &RasterBuffer, red: &RasterBuffer)
    -> Result<RasterBuffer, Error>
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
```

### Pattern 3: Cloud/HTTP Data

**GDAL (Python):**
```python
# GDAL uses virtual file systems
dataset = gdal.Open('/vsicurl/https://example.com/data.tif')
```

**OxiGDAL:**
```rust
use oxigdal_cloud::backends::HttpBackend;
use oxigdal_cloud::retry::RetryConfig;

let retry_config = RetryConfig::default();
let http_backend = HttpBackend::new(retry_config);

// Async operation
let data = http_backend.get("https://example.com/data.tif").await?;
```

## Performance Considerations

### Memory Usage

**GDAL:**
- Manual memory management
- Potential leaks with improper cleanup
- Python GC overhead

**OxiGDAL:**
- Automatic cleanup (RAII)
- No GC overhead
- Stack allocation when possible

### Parallelization

**GDAL:**
```python
# Need to use multiprocessing
from multiprocessing import Pool

def worker(tile_coords):
    # Each process opens file independently
    ds = gdal.Open('input.tif')
    return process(ds, tile_coords)

with Pool() as pool:
    results = pool.map(worker, tile_list)
```

**OxiGDAL:**
```rust
// Built-in parallel iterators
use rayon::prelude::*;

let results: Vec<_> = tile_list
    .par_iter()
    .map(|tile| process(tile))
    .collect();
// Compiler ensures thread safety
```

### SIMD Operations

**GDAL:**
- Limited SIMD support
- Numpy can use BLAS/MKL

**OxiGDAL:**
```rust
use oxigdal_core::simd_buffer::SimdBuffer;

let simd_buffer = SimdBuffer::from_buffer(&buffer)?;
let result = simd_buffer.add_scalar(10.0)?; // Vectorized automatically
```

## Error Handling

### GDAL Approach

**C++:**
```cpp
CPLErr err = GDALRasterIO(...);
if (err != CE_None) {
    fprintf(stderr, "Error: %s\n", CPLGetLastErrorMsg());
}
```

**Python:**
```python
try:
    dataset = gdal.Open('file.tif')
    if not dataset:
        raise Exception("Failed to open file")
except RuntimeError as e:
    print(f"Error: {e}")
```

### OxiGDAL Approach

```rust
use oxigdal_core::error::Result;

fn process_file(path: &str) -> Result<()> {
    let source = FileDataSource::open(path)?;
    let reader = GeoTiffReader::open(source)?;

    // Errors propagate via ?
    let buffer = reader.read_tile_buffer(0, 0, 0)?;

    Ok(())
}

// Using the function
match process_file("input.tif") {
    Ok(()) => println!("Success"),
    Err(e) => eprintln!("Error: {}", e),
}
```

## Migration Checklist

- [ ] Identify GDAL features used in your code
- [ ] Check OxiGDAL feature support
- [ ] Replace file I/O with OxiGDAL equivalents
- [ ] Update error handling to use Result<T, E>
- [ ] Replace NULL checks with Option<T>
- [ ] Convert array operations to RasterBuffer
- [ ] Update build system (remove GDAL dependency)
- [ ] Add appropriate Cargo features
- [ ] Update tests
- [ ] Profile performance

## Troubleshooting

### Issue: Missing Driver Support

**Problem:** GDAL driver not available in OxiGDAL

**Solution:**
- Check if driver is feature-gated: `cargo add oxigdal-hdf5 --features hdf5`
- For unsupported formats, consider preprocessing with GDAL

### Issue: Different Results

**Problem:** Numerical results differ slightly

**Solution:**
- Floating point precision differences are normal
- Check algorithm implementations
- Use `approx` crate for fuzzy comparisons

### Issue: Performance Regression

**Problem:** OxiGDAL slower than GDAL for specific operation

**Solution:**
- Enable release mode: `cargo build --release`
- Use parallel processing with Rayon
- Enable SIMD: `RUSTFLAGS="-C target-cpu=native"`
- Profile with `cargo flamegraph`

## Resources

- [OxiGDAL Documentation](https://docs.rs/oxigdal)
- [Example Gallery](../examples/)
- [Rust Book](https://doc.rust-lang.org/book/)
- [GDAL API Documentation](https://gdal.org/api/)

## Getting Help

- GitHub Issues: https://github.com/cool-japan/oxigdal/issues
- Discussions: https://github.com/cool-japan/oxigdal/discussions
- COOLJAPAN Community: Contact team

## Next Steps

After migration:
1. Review the [Performance Guide](PERFORMANCE_GUIDE.md)
2. Check the [Deployment Guide](DEPLOYMENT_GUIDE.md)
3. Explore advanced features in tutorials
4. Join the community and share feedback
