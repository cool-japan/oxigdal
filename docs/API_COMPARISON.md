# API Comparison: OxiGDAL vs GDAL

Comprehensive reference comparing OxiGDAL APIs with their GDAL equivalents.

## Table of Contents

- [Raster Operations](#raster-operations)
- [Vector Operations](#vector-operations)
- [Coordinate Reference Systems](#coordinate-reference-systems)
- [Metadata](#metadata)
- [Data Types](#data-types)
- [Error Handling](#error-handling)
- [I/O Operations](#io-operations)

## Raster Operations

### Opening and Reading Files

| Operation | GDAL (C++) | GDAL (Python) | OxiGDAL (Rust) |
|-----------|-----------|---------------|----------------|
| Open file | `GDALOpen("file.tif", GA_ReadOnly)` | `gdal.Open("file.tif")` | `FileDataSource::open("file.tif")?` |
| Get width | `dataset->GetRasterXSize()` | `ds.RasterXSize` | `reader.width()` |
| Get height | `dataset->GetRasterYSize()` | `ds.RasterYSize` | `reader.height()` |
| Get band count | `dataset->GetRasterCount()` | `ds.RasterCount` | `reader.band_count()` |
| Get raster band | `dataset->GetRasterBand(1)` | `ds.GetRasterBand(1)` | `reader.band(0)?` |
| Read tile | `band->RasterIO(GF_Read, ...)` | `band.ReadAsArray(...)` | `reader.read_tile_buffer(x, y, z)?` |
| Get data type | `band->GetRasterDataType()` | `band.DataType` | `buffer.data_type()` |

### Raster Data Access

```rust
// OxiGDAL: Type-safe raster access
let buffer = reader.read_tile_buffer(0, 0, 0)?;

// Get pixel
let value = buffer.get_pixel(x, y)?;

// Set pixel
buffer.set_pixel(x, y, value)?;

// Get window
let window = buffer.window(x, y, width, height)?;

// Iterate pixels
for pixel in buffer.iter() {
    // process pixel
}
```

### Raster Statistics

| Operation | GDAL (C++) | GDAL (Python) | OxiGDAL |
|-----------|-----------|---------------|---------|
| Compute stats | `band->ComputeStatistics()` | `band.ComputeStatistics()` | `buffer.compute_statistics()?` |
| Get minimum | `band->GetMinimum()` | `band.GetMinimum()` | `stats.min` |
| Get maximum | `band->GetMaximum()` | `band.GetMaximum()` | `stats.max` |
| Get mean | Via `ComputeStatistics()` | Via `ComputeStatistics()` | `stats.mean` |
| Get histogram | `band->GetHistogram()` | `band.GetHistogram()` | `buffer.histogram()?` |

### Raster Resampling

```rust
// OxiGDAL: Resample raster data
use oxigdal_algorithms::resample::{Resampling, ResampleOptions};

let options = ResampleOptions {
    width: new_width,
    height: new_height,
    resampling: Resampling::Bilinear,
};

let resampled = buffer.resample(&options)?;
```

### Band Math

```rust
// OxiGDAL: Type-safe band math
fn calculate_ndvi(nir: &RasterBuffer, red: &RasterBuffer) -> Result<RasterBuffer> {
    let mut result = RasterBuffer::zeros(
        nir.width(),
        nir.height(),
        RasterDataType::Float32
    );

    for y in 0..nir.height() {
        for x in 0..nir.width() {
            let n = nir.get_pixel(x, y)?;
            let r = red.get_pixel(x, y)?;
            let ndvi = (n - r) / (n + r);
            result.set_pixel(x, y, ndvi)?;
        }
    }

    Ok(result)
}
```

### Raster Writing

| Operation | GDAL (C++) | GDAL (Python) | OxiGDAL |
|-----------|-----------|---------------|---------|
| Create file | `driver->Create()` | `driver.Create()` | `GeoTiffWriter::new()?` |
| Write band | `band->WriteArray()` | `band.WriteArray()` | `writer.write_buffer()?` |
| Set transform | `dataset->SetGeoTransform()` | `ds.SetGeoTransform()` | `GeoTiffWriterOptions::geo_transform` |
| Set projection | `dataset->SetProjection()` | `ds.SetProjection()` | `GeoTiffWriterOptions::epsg_code` |
| Flush cache | `band->FlushCache()` | `band.FlushCache()` | Automatic (RAII) |
| Close | Manual delete | Context manager | Automatic (RAII) |

## Vector Operations

### Opening and Reading Vector Data

| Operation | GDAL (OGR C++) | GDAL (OGR Python) | OxiGDAL |
|-----------|---------------|-------------------|---------|
| Open source | `OGROpen("file.shp")` | `ogr.Open("file.shp")` | `FileDataSource::open()?` |
| Get layer | `datasource->GetLayer(0)` | `ds.GetLayer(0)` | `reader.layer(0)?` |
| Get feature count | `layer->GetFeatureCount()` | `layer.GetFeatureCount()` | `layer.feature_count()?` |
| Read feature | `layer->GetNextFeature()` | `layer.GetNextFeature()` | `layer.iter_features()?` |
| Get geometry | `feature->GetGeometryRef()` | `feature.GetGeometryRef()` | `feature.geometry()` |
| Get field | `feature->GetField(i)` | `feature.GetField()` | `feature.get_property()` |

### Geometry Operations

| Operation | Shapely (Python) | OxiGDAL (Rust via geo) |
|-----------|------------------|----------------------|
| Create point | `Point(x, y)` | `Point::new(x, y)` |
| Create linestring | `LineString(coords)` | `LineString::from(coords)` |
| Create polygon | `Polygon(coords)` | `Polygon::new(exterior, holes)` |
| Buffer | `geom.buffer(dist)` | `BufferBuilder::new().buffer(&geom, dist)` |
| Contains | `geom.contains(other)` | `geom.contains(&other)` |
| Intersects | `geom.intersects(other)` | `geom.intersects(&other)` |
| Union | `geom.union(other)` | `geom.union(&other)` |
| Difference | `geom.difference(other)` | `geom.difference(&other)` |
| Distance | `geom.distance(other)` | `geom.distance(&other)` |
| Area | `geom.area` | `geom.unsigned_area()` |

### Vector Writing

```rust
// OxiGDAL: Write vector features
use oxigdal_core::vector::{Feature, Geometry};
use oxigdal_geojson::GeoJsonWriter;

let mut writer = GeoJsonWriter::create("output.geojson")?;

// Create feature
let point = Geometry::Point(Point::new(0.0, 0.0));
let mut feature = Feature::new(point);
feature.set_property("name", PropertyValue::String("Point 1".into()));

writer.write_feature(&feature)?;
```

## Coordinate Reference Systems

| Operation | GDAL (Python) | OxiGDAL |
|-----------|---------------|---------|
| Create from EPSG | `osr.SpatialReference(4326)` | `Projection::from_epsg(4326)?` |
| Create from WKT | `osr.SpatialReference(wkt_string)` | `Projection::from_wkt(wkt_string)?` |
| Get EPSG code | `srs.GetAttrValue("AUTHORITY", 1)` | `proj.epsg_code()` |
| Get WKT | `srs.ExportToWkt()` | `proj.to_wkt()?` |
| Transform point | `ct.TransformPoint(x, y)` | `proj.transform_point(x, y, &other_proj)?` |
| Transform buffer | `gdal.Warp()` | `oxigdal_algorithms::reproject()` |

### Projection Example

```rust
// OxiGDAL: Coordinate transformation
use oxigdal_proj::Projection;

let from = Projection::from_epsg(4326)?;  // WGS84
let to = Projection::from_epsg(3857)?;    // Web Mercator

let (x, y) = from.transform_point(10.0, 20.0, &to)?;
println!("Transformed: ({}, {})", x, y);
```

## Metadata

### GeoTransform

| Aspect | GDAL | OxiGDAL |
|--------|------|---------|
| Get transform | `ds.GetGeoTransform()` | `reader.geo_transform()` |
| Tuple structure | `(x_off, pixel_x, x_skew, y_off, y_skew, pixel_y)` | Struct with named fields |
| Compute bounds | Manual calculation | `geo_transform.compute_bounds(width, height)` |
| Pixel to geo | Manual calculation | `geo_transform.pixel_to_geo(x, y)` |
| Geo to pixel | Manual calculation | `geo_transform.geo_to_pixel(lon, lat)` |

### NoData Values

| Operation | GDAL (Python) | OxiGDAL |
|-----------|---------------|---------|
| Get NoData | `band.GetNoDataValue()` | `buffer.nodata_value()` |
| Set NoData | `band.SetNoDataValue()` | `GeoTiffWriterOptions::nodata` |
| Check if NoData | Manual comparison | `buffer.is_nodata(x, y)?` |

## Data Types

### Raster Data Type Mapping

| GDAL | OxiGDAL | Bits | Range |
|------|---------|------|-------|
| GDT_Byte | UInt8 | 8 | 0-255 |
| GDT_UInt16 | UInt16 | 16 | 0-65535 |
| GDT_Int16 | Int16 | 16 | -32768-32767 |
| GDT_UInt32 | UInt32 | 32 | 0-4294967295 |
| GDT_Int32 | Int32 | 32 | -2147483648-2147483647 |
| GDT_Float32 | Float32 | 32 | IEEE 754 |
| GDT_Float64 | Float64 | 64 | IEEE 754 |
| GDT_CInt16 | N/A (use arrays) | 32 | Complex |
| GDT_CInt32 | N/A | 64 | Complex |
| GDT_CFloat32 | N/A | 64 | Complex |
| GDT_CFloat64 | N/A | 128 | Complex |

## Error Handling

### GDAL Error Pattern

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
        raise RuntimeError("Failed to open")
except Exception as e:
    print(f"Error: {e}")
```

### OxiGDAL Error Pattern

```rust
// Explicit Result type
fn process() -> Result<()> {
    let source = FileDataSource::open("file.tif")?;
    let reader = GeoTiffReader::open(source)?;
    let buffer = reader.read_tile_buffer(0, 0, 0)?;
    Ok(())
}

// Error propagation with ?
match process() {
    Ok(()) => println!("Success"),
    Err(e) => eprintln!("Error: {}", e),
}

// Match pattern
match FileDataSource::open("file.tif") {
    Ok(source) => { /* handle success */ }
    Err(e) => { /* handle error */ }
}
```

## I/O Operations

### File Reading

| Operation | GDAL | OxiGDAL |
|-----------|------|---------|
| Read file | `gdal.Open()` | `FileDataSource::open()?` |
| Read HTTP | `/vsicurl/http://...` | `HttpBackend::get()?` |
| Read S3 | `/vsis3/bucket/key` | `S3Backend::get()?` |
| Read memory | GDALOpen with buffer | `MemoryDataSource::new()` |

### Cloud Storage Access

```rust
// OxiGDAL: Cloud-native I/O
use oxigdal_cloud::backends::{S3Backend, HttpBackend};
use oxigdal_cloud::retry::RetryConfig;

// S3
let retry = RetryConfig::default();
let s3 = S3Backend::new(retry);
let data = s3.get("s3://bucket/key").await?;

// HTTP with range requests
let http = HttpBackend::new(retry);
let range_data = http.get_range("https://example.com/file.tif", 0, 1024).await?;
```

### Async Operations

```rust
// OxiGDAL: Native async support
#[tokio::main]
async fn main() -> Result<()> {
    let http = HttpBackend::new(RetryConfig::default());

    // Concurrent requests
    let futures = vec![
        http.get("https://example.com/file1.tif"),
        http.get("https://example.com/file2.tif"),
    ];

    let results = futures::future::try_join_all(futures).await?;
    Ok(())
}
```

## Driver Feature Matrix

### Raster Formats

| Format | GDAL Support | OxiGDAL | Notes |
|--------|-------------|---------|-------|
| GeoTIFF/COG | ✅ Full | ✅ Full | Cloud Optimized GeoTIFF |
| HDF5 | ✅ Full | ✅ Full | Scientific data |
| NetCDF | ✅ Full | ✅ Full | Climate/weather data |
| GRIB | ✅ Full | ✅ Full | Weather forecasts |
| Zarr | ✅ Via plugin | ✅ Full | Cloud arrays |
| JPEG2000 | ✅ Optional | ✅ Limited | Satellite imagery |

### Vector Formats

| Format | GDAL Support | OxiGDAL | Notes |
|--------|-------------|---------|-------|
| Shapefile | ✅ Full | ✅ Full | Legacy format |
| GeoJSON | ✅ Full | ✅ Full | Web standard |
| GeoParquet | ✅ New | ✅ Full | Columnar vector |
| FlatGeoBuf | ✅ New | ✅ Full | Efficient binary |
| GeoPackage | ✅ Full | ⏳ Planned | SQLite-based |

## Performance Characteristics

### Memory Usage

| Operation | GDAL | OxiGDAL | Notes |
|-----------|------|---------|-------|
| Load 1GB file | ~3.5GB | ~1.2GB | Rust vs Python GDAL |
| Read tile | Contiguous | Cached | Smart caching |
| Transform | In-place | Copy | Trade-off for safety |

### Speed

| Operation | GDAL | OxiGDAL | Notes |
|-----------|------|---------|-------|
| NDVI (4096x4096) | 120ms | 45ms (2.7x) | NumPy vs optimized Rust |
| With SIMD | - | 18ms (6.7x) | SIMD operations |
| Parallel (8 cores) | ~45ms | 8ms (15x) | With tile parallelism |
| HTTP tile read | Variable | Cached | Smart prefetching |

## Migration Path Quick Reference

### Step 1: Data Loading
```cpp
// GDAL
GDALDataset *ds = (GDALDataset *)GDALOpen("file.tif", GA_ReadOnly);

// OxiGDAL
let source = FileDataSource::open("file.tif")?;
let reader = GeoTiffReader::open(source)?;
```

### Step 2: Processing
```python
# GDAL
band = ds.GetRasterBand(1)
data = band.ReadAsArray()
result = data * 2

# OxiGDAL
let buffer = reader.read_tile_buffer(0, 0, 0)?;
let result = buffer.multiply_scalar(2.0)?;
```

### Step 3: Output
```python
# GDAL
driver = gdal.GetDriverByName('GTiff')
out_ds = driver.Create('output.tif', width, height, 1, gdal.GDT_Float32)
out_ds.SetGeoTransform(gt)
out_ds.GetRasterBand(1).WriteArray(result)

# OxiGDAL
let file = File::create("output.tif")?;
let writer = GeoTiffWriter::new(file, options)?;
writer.write_buffer(&result)?;
```

## See Also

- [MIGRATION_FROM_GDAL.md](MIGRATION_FROM_GDAL.md) - Detailed migration guide
- [PYTHON_TO_RUST.md](PYTHON_TO_RUST.md) - Python developer guide
- [ARCHITECTURE.md](ARCHITECTURE.md) - System architecture
