# OxiGDAL Architecture Overview

Comprehensive guide to OxiGDAL's design, structure, and component interactions.

## Table of Contents

- [System Architecture](#system-architecture)
- [Core Components](#core-components)
- [Data Flow](#data-flow)
- [Module Organization](#module-organization)
- [Design Patterns](#design-patterns)
- [Memory Model](#memory-model)
- [Concurrency](#concurrency)
- [Extension Points](#extension-points)

## System Architecture

### High-Level Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                   User Application                          │
└─────────────────────────────────────────────────────────────┘
                          ▲
                          │ Uses
                          │
┌─────────────────────────────────────────────────────────────┐
│                   Public API Layer                          │
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐    │
│  │ Raster API   │  │ Vector API   │  │ Spatial Ops  │    │
│  └──────────────┘  └──────────────┘  └──────────────┘    │
└─────────────────────────────────────────────────────────────┘
                          ▲
                          │ Depends on
                          │
┌─────────────────────────────────────────────────────────────┐
│              Driver & Format Support Layer                  │
│                                                             │
│  ┌─────────┐ ┌─────────┐ ┌────────┐ ┌──────┐ ┌─────┐    │
│  │ GeoTIFF │ │ GeoJSON │ │ Zarr   │ │ HDF5 │ │ ... │    │
│  └─────────┘ └─────────┘ └────────┘ └──────┘ └─────┘    │
└─────────────────────────────────────────────────────────────┘
                          ▲
                          │ Uses
                          │
┌─────────────────────────────────────────────────────────────┐
│                  Core Abstractions                          │
│                                                             │
│  ┌────────────────────────────────────────────────────┐   │
│  │ Types: BoundingBox, GeoTransform, RasterDataType   │   │
│  │ Buffers: RasterBuffer, SIMD-aware storage           │   │
│  │ Traits: DataSource, Read, Write                     │   │
│  │ Vectors: Geometry, Feature, FeatureCollection       │   │
│  └────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                          ▲
                          │ Uses
                          │
┌─────────────────────────────────────────────────────────────┐
│              I/O & Storage Backends                         │
│                                                             │
│  ┌────────┐ ┌────────┐ ┌─────┐ ┌────────┐ ┌──────────┐  │
│  │  File  │ │ HTTP   │ │ S3  │ │ Azure  │ │ GCS      │  │
│  │  I/O   │ │ Fetch  │ │ Ops │ │ Blob   │ │ Storage  │  │
│  └────────┘ └────────┘ └─────┘ └────────┘ └──────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## Core Components

### 1. oxigdal-core

**Purpose**: Foundational types and traits (no_std compatible)

**Key Modules**:
- `types/` - RasterDataType, BoundingBox, GeoTransform, RasterMetadata
- `buffer/` - RasterBuffer, typed pixel access
- `simd_buffer/` - SIMD-optimized operations
- `vector/` - Geometry types (Point, Polygon, LineString, etc.)
- `io/` - DataSource trait and implementations
- `error/` - OxiGdalError type

**Responsibility**:
- Define the public API surface
- No FFI or external C dependencies
- Works in no_std environments with alloc feature

### 2. Driver Crates

**Structure**:
```
oxigdal-drivers/
├── geotiff/        # GeoTIFF and Cloud Optimized GeoTIFF
├── geojson/        # GeoJSON and ndjson
├── zarr/           # Zarr arrays
├── shapefile/      # Shapefile format
├── netcdf/         # NetCDF scientific data
├── geoparquet/     # GeoParquet columnar format
├── hdf5/           # HDF5 hierarchical data
├── grib/           # GRIB weather data
├── vrt/            # Virtual rasters
└── jpeg2000/       # JPEG2000 imagery
```

**Design Pattern**: Each driver implements:
```rust
pub trait DataFormat {
    fn open<S: DataSource>(source: S) -> Result<Self>;
    fn read(&mut self) -> Result<Buffer>;
    fn write(&mut self, buffer: &Buffer) -> Result<()>;
}
```

### 3. Feature-Specific Crates

**Specialized capabilities**:

| Crate | Purpose | Features |
|-------|---------|----------|
| `oxigdal-algorithms` | Raster algorithms | NDVI, resampling, statistics, morphology |
| `oxigdal-cloud` | Cloud storage backends | S3, Azure, GCS, HTTP |
| `oxigdal-proj` | Coordinate projections | EPSG, WKT, coordinate transforms |
| `oxigdal-postgis` | PostGIS integration | Spatial queries, database I/O |
| `oxigdal-3d` | 3D/point cloud support | LAS/LAZ, OBJ, glTF, point clouds |
| `oxigdal-analytics` | Spatial analytics | Clustering, hotspots, change detection |
| `oxigdal-temporal` | Time series analysis | Trend analysis, gap filling, anomalies |
| `oxigdal-ml` | Machine learning | Classification, segmentation, ONNX |
| `oxigdal-wasm` | WebAssembly support | Browser-based processing |
| `oxigdal-server` | Web services | WMS, WMTS, XYZ tiles, REST API |

## Data Flow

### Typical Raster Processing Pipeline

```
File on Disk/Cloud
     │
     ▼
DataSource (File/HTTP/S3)
     │
     ├─── Trait: Read bytes
     │
     ▼
Format Driver (GeoTIFF)
     │
     ├─── Parse headers
     ├─── Decompress tiles
     ├─── Validate checksums
     │
     ▼
RasterBuffer (in-memory)
     │
     ├─── Typed pixel access
     ├─── SIMD operations
     ├─── Statistics computation
     │
     ▼
Output Operations
     │
     ├─── Algorithm processing
     ├─── Format conversion
     ├─── Writing to new location
     │
     ▼
Output File/Stream
```

### Vector Processing Pipeline

```
GeoJSON/Shapefile
     │
     ▼
Feature Parser
     │
     ├─── Extract Geometry
     ├─── Extract Attributes
     │
     ▼
Vector::Feature objects
     │
     ├─── Geometry operations (buffer, intersect)
     ├─── Spatial queries
     ├─── Attribute filtering
     │
     ▼
FeatureCollection
     │
     ▼
Output Writer
     │
     ▼
New GeoJSON/Shapefile
```

## Module Organization

### oxigdal-core Module Tree

```
oxigdal_core/
├── types/
│   ├── mod.rs              # RasterDataType, BoundingBox
│   ├── geotransform.rs     # Affine transformation
│   └── metadata.rs         # Raster metadata
├── buffer/
│   ├── mod.rs              # RasterBuffer trait and impl
│   ├── typed_buffer.rs     # Type-safe pixel access
│   └── operations.rs       # Buffer math operations
├── simd_buffer/
│   ├── mod.rs              # SIMD wrapper
│   ├── operations.rs       # Vectorized math
│   └── intrinsics.rs       # CPU-specific optimizations
├── vector/
│   ├── mod.rs
│   ├── geometry/           # Geometry types
│   ├── feature.rs          # Feature and properties
│   └── operations.rs       # Geometric operations
├── io/
│   ├── mod.rs              # DataSource trait
│   ├── file.rs             # FileDataSource
│   ├── memory.rs           # In-memory buffers
│   └── http.rs             # HTTP fetching
└── error/
    └── mod.rs              # OxiGdalError type
```

### Driver Module Organization (Example: GeoTIFF)

```
oxigdal_geotiff/
├── lib.rs                  # Exports
├── error.rs                # GeoTIFF-specific errors
├── tiff/
│   ├── mod.rs              # TIFF format structs
│   ├── header.rs           # TIFF/BigTIFF headers
│   ├── ifd.rs              # IFD (Image File Directory)
│   ├── tags.rs             # TIFF tags
│   └── compression/        # Compression codecs
├── geokeys/
│   ├── mod.rs              # GeoTIFF keys
│   └── epsg.rs             # EPSG code handling
├── cog/
│   ├── mod.rs              # Cloud Optimized GeoTIFF
│   ├── tile_reader.rs      # Tile-based reading
│   ├── overview.rs         # Pyramidal overview levels
│   └── validator.rs        # COG compliance checking
├── reader.rs               # GeoTiffReader implementation
└── writer.rs               # GeoTiffWriter implementation
```

## Design Patterns

### 1. Result-Based Error Handling

All fallible operations return `Result<T, OxiGdalError>`:

```rust
pub type Result<T> = std::result::Result<T, OxiGdalError>;

// Usage
fn open_file(path: &str) -> Result<RasterBuffer> {
    let source = FileDataSource::open(path)?;
    let reader = GeoTiffReader::open(source)?;
    reader.read_tile_buffer(0, 0, 0)
}
```

**Benefits**:
- Explicit error handling
- Compile-time verification
- Zero-cost abstractions
- No panic safety issues

### 2. Builder Pattern

Complex objects use builders:

```rust
let options = GeoTiffWriterOptions {
    geo_transform: Some(gt),
    epsg_code: Some(4326),
    compression: Some(Compression::Deflate),
    creation_date: Some(now),
    ..Default::default()
};

let writer = GeoTiffWriter::new(file, options)?;
```

### 3. Trait-Based Abstraction

Extensible through traits:

```rust
pub trait DataSource: Read + Send + Sync {
    fn size(&self) -> u64;
    fn metadata(&self) -> Option<&Metadata>;
}

pub trait RasterReader {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn read_tile(&mut self, x: u32, y: u32) -> Result<RasterBuffer>;
}
```

### 4. RAII (Resource Acquisition Is Initialization)

Automatic cleanup via destructors:

```rust
{
    let file = File::create("output.tif")?;
    let writer = GeoTiffWriter::new(file, options)?;
    writer.write_buffer(&buffer)?;
} // File automatically closed, writer drops
```

### 5. Type-Safe Coordinates

Wrapper types prevent coordinate system confusion:

```rust
struct GeoCoordinate { lon: f64, lat: f64 }
struct PixelCoordinate { x: u32, y: u32 }

impl GeoTransform {
    fn pixel_to_geo(&self, px: PixelCoordinate) -> GeoCoordinate { ... }
    fn geo_to_pixel(&self, gc: GeoCoordinate) -> PixelCoordinate { ... }
}
```

## Memory Model

### Stack vs Heap Allocation

**Stack** (Preferred when possible):
- Metadata: BoundingBox, GeoTransform
- Small fixed-size arrays: tile headers
- Iterators and closures

**Heap** (For large/variable-size data):
- RasterBuffer (pixel data)
- FeatureCollection (vector features)
- Compressed tile data

### Zero-Copy Operations

Where possible, OxiGDAL uses zero-copy techniques:

```rust
// No copy: memory-mapped file
let source = FileDataSource::open("cog.tif")?;

// No copy: HTTP range request returns direct bytes
let http = HttpBackend::new(config);
let tile = http.get_range(url, start, end).await?;

// Copy: decompression unavoidable
let compressed = read_tile_data();
let decompressed = decompress(&compressed)?;
```

### Memory Safety Guarantees

Rust ensures:
- **No buffer overflows**: Bounds checking at compile/runtime
- **No use-after-free**: Borrow checker prevents dangling references
- **No data races**: Mutable references are exclusive
- **No double-free**: Ownership system prevents deallocation bugs

## Concurrency

### Threading Model

OxiGDAL uses three concurrency patterns:

#### 1. Rayon Parallel Iterators

Data parallelism for tile processing:

```rust
let results: Vec<_> = tiles
    .par_iter()
    .map(|tile| process(tile))
    .collect();
```

#### 2. Tokio Async/Await

I/O parallelism for cloud operations:

```rust
#[tokio::main]
async fn main() {
    let futures = vec![
        fetch_file("https://example.com/file1.tif"),
        fetch_file("https://example.com/file2.tif"),
    ];
    futures::future::join_all(futures).await
}
```

#### 3. Send + Sync Traits

Thread safety at compile time:

```rust
pub struct RasterBuffer {
    data: Vec<f32>,  // Send + Sync
}

impl Send for RasterBuffer {}
impl Sync for RasterBuffer {}
```

### Avoid Sharing State

Use channels or owned data:

```rust
// Good: owned data per thread
let chunks: Vec<_> = buffer
    .par_chunks(256)
    .map(|chunk| process_chunk(chunk.to_vec()))
    .collect();

// Avoid: shared mutable state
static mut GLOBAL_BUFFER: RasterBuffer;  // Unsafe!
```

## Extension Points

### Adding a New Format Driver

1. **Create a new crate**:
```bash
cargo new --lib crates/oxigdal-drivers/myformat
```

2. **Implement core traits**:
```rust
pub struct MyFormatReader<S: DataSource> {
    source: S,
    header: Header,
}

impl<S: DataSource> RasterReader for MyFormatReader<S> {
    fn width(&self) -> u32 { self.header.width }
    fn height(&self) -> u32 { self.header.height }
    fn read_tile(&mut self, x: u32, y: u32) -> Result<RasterBuffer> { ... }
}
```

3. **Add to workspace**:
```toml
# workspace Cargo.toml
members = ["crates/oxigdal-drivers/myformat"]

# oxigdal-myformat Cargo.toml
[dependencies]
oxigdal-core = { path = "../../../oxigdal-core" }
```

### Adding Custom Algorithms

```rust
// In oxigdal-algorithms or new feature crate
pub fn custom_algorithm(buffer: &RasterBuffer, params: &Params) -> Result<RasterBuffer> {
    let mut result = RasterBuffer::zeros(buffer.width(), buffer.height(), buffer.data_type());

    for y in 0..buffer.height() {
        for x in 0..buffer.width() {
            let pixel = buffer.get_pixel(x, y)?;
            let processed = your_algorithm(pixel, params);
            result.set_pixel(x, y, processed)?;
        }
    }

    Ok(result)
}
```

### Custom I/O Backend

```rust
pub struct CustomBackend { ... }

impl DataSource for CustomBackend {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> { ... }
    fn size(&self) -> u64 { ... }
}
```

## Performance Considerations

### Optimization Hierarchy

1. **Algorithm selection** (most impact)
   - SIMD vs scalar
   - Streaming vs all-in-memory
   - Parallel vs sequential

2. **Data layout**
   - Memory alignment for SIMD
   - Cache-friendly access patterns
   - Minimize allocations

3. **Compilation**
   - `--release` mode mandatory
   - `target-cpu=native` for native optimizations
   - LTO (Link Time Optimization)

4. **Runtime tuning**
   - Rayon thread pool sizing
   - I/O prefetching
   - Tile cache configuration

### Profiling Tools

```bash
# CPU flamegraph
cargo flamegraph --release

# Memory profiling
valgrind --tool=massif ./target/release/app

# Benchmark with criterion
cargo bench --release

# Native CPU info
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

## See Also

- [PERFORMANCE_GUIDE.md](PERFORMANCE_GUIDE.md) - Optimization techniques
- [BEST_PRACTICES.md](BEST_PRACTICES.md) - Development patterns
- [API_COMPARISON.md](API_COMPARISON.md) - GDAL vs OxiGDAL
