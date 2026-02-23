# OxiGDAL Driver Guide

This guide provides detailed documentation for each geospatial data format driver supported by OxiGDAL.

## Table of Contents

1. [GeoTIFF / COG Driver](#geotiff--cog-driver)
2. [GeoJSON Driver](#geojson-driver)
3. [GeoParquet Driver](#geoparquet-driver)
4. [Zarr Driver](#zarr-driver)
5. [FlatGeobuf Driver](#flatgeobuf-driver)

---

## GeoTIFF / COG Driver

**Crate**: `oxigdal-geotiff`
**Format**: GeoTIFF, Cloud Optimized GeoTIFF (COG)
**Type**: Raster
**Read**: ✅ Full support
**Write**: ✅ Full support

### Overview

The GeoTIFF driver provides comprehensive support for reading and writing GeoTIFF files, with special optimizations for Cloud Optimized GeoTIFFs (COGs).

### Key Features

- ✅ Classic TIFF and BigTIFF support
- ✅ Cloud Optimized GeoTIFF (COG) reading and writing
- ✅ Tiled and stripped layouts
- ✅ Multiple compression schemes (DEFLATE, LZW, ZSTD, JPEG)
- ✅ All standard data types (UInt8, Int16, Float32, etc.)
- ✅ Overview/pyramid levels
- ✅ GeoKeys for coordinate reference systems
- ✅ HTTP range request optimization for cloud data

### Reading GeoTIFF Files

#### Basic Reading

```rust
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_core::io::FileDataSource;

fn read_geotiff() -> Result<(), Box<dyn std::error::Error>> {
    let source = FileDataSource::open("elevation.tif")?;
    let reader = GeoTiffReader::open(source)?;

    // Get metadata
    println!("Size: {}x{}", reader.width(), reader.height());
    println!("Bands: {}", reader.band_count());
    println!("Data type: {:?}", reader.data_type());
    println!("Compression: {:?}", reader.compression());

    // Get geospatial metadata
    if let Some(gt) = reader.geo_transform() {
        println!("Resolution: {:?}", gt.resolution());
        println!("Origin: ({}, {})", gt.origin_x(), gt.origin_y());
    }

    // Read a tile
    let (tiles_x, tiles_y) = reader.tile_count();
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let tile_data = reader.read_tile(0, tx, ty)?;
            // Process tile_data...
        }
    }

    Ok(())
}
```

#### Reading Cloud Optimized GeoTIFF (COG)

```rust
use oxigdal_geotiff::CogReader;
use oxigdal_core::io::FileDataSource;

fn read_cog() -> Result<(), Box<dyn std::error::Error>> {
    let source = FileDataSource::open("satellite.tif")?;
    let reader = CogReader::open(source)?;

    // Validate it's a proper COG
    println!("Is valid COG: {}", reader.is_valid_cog());

    // Get primary image info
    let info = reader.primary_info();
    println!("Image size: {}x{}", info.width, info.height);
    println!("Tile size: {:?}x{:?}", info.tile_width, info.tile_height);

    // Access overviews
    for level in 0..reader.overview_count() {
        let overview = reader.overview_info(level)?;
        println!("Overview {}: {}x{}", level, overview.width, overview.height);
    }

    // Read a specific tile from a specific level
    let tile_data = reader.read_tile(0, 0, 0)?; // level 0, tile (0,0)

    Ok(())
}
```

#### Validating COG Structure

```rust
use oxigdal_geotiff::{TiffFile, cog};
use oxigdal_core::io::FileDataSource;

fn validate_cog() -> Result<(), Box<dyn std::error::Error>> {
    let source = FileDataSource::open("maybe_cog.tif")?;
    let tiff = TiffFile::parse(&source)?;
    let validation = cog::validate_cog(&tiff, &source);

    if validation.is_valid {
        println!("✓ Valid COG");
    } else {
        println!("✗ Not a valid COG");
        for warning in &validation.warnings {
            println!("  Warning: {}", warning);
        }
        for error in &validation.errors {
            println!("  Error: {}", error);
        }
    }

    Ok(())
}
```

### Writing GeoTIFF Files

#### Basic GeoTIFF Writing

```rust
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{RasterDataType, GeoTransform, BoundingBox};
use std::fs::File;

fn write_geotiff() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let output_path = temp_dir.join("output.tif");

    // Create raster data
    let buffer = RasterBuffer::zeros(1024, 1024, RasterDataType::Float32);

    // Configure writer options
    let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)?;
    let geo_transform = GeoTransform::from_bounds(&bbox, 1024, 1024)?;

    let options = GeoTiffWriterOptions {
        geo_transform: Some(geo_transform),
        epsg_code: Some(4326), // WGS84
        tile_width: Some(256),
        tile_height: Some(256),
        ..Default::default()
    };

    // Write the file
    let file = File::create(&output_path)?;
    let writer = GeoTiffWriter::new(file, options)?;
    writer.write_buffer(&buffer)?;

    println!("Wrote GeoTIFF to {:?}", output_path);
    Ok(())
}
```

#### Writing Cloud Optimized GeoTIFF (COG)

```rust
use oxigdal_geotiff::writer::{CogWriter, CogWriterOptions, OverviewResampling};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{RasterDataType, GeoTransform, BoundingBox};
use oxigdal_geotiff::tiff::Compression;
use std::fs::File;

fn write_cog() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let output_path = temp_dir.join("output_cog.tif");

    // Create raster data (larger for meaningful overviews)
    let buffer = RasterBuffer::zeros(4096, 4096, RasterDataType::UInt16);

    // Configure COG options
    let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)?;
    let geo_transform = GeoTransform::from_bounds(&bbox, 4096, 4096)?;

    let options = CogWriterOptions {
        geo_transform: Some(geo_transform),
        epsg_code: Some(4326),
        tile_width: 512,
        tile_height: 512,
        compression: Compression::Deflate,
        overview_resampling: OverviewResampling::Average,
        overview_levels: vec![2, 4, 8, 16], // Create 4 overview levels
        ..Default::default()
    };

    // Write COG
    let file = File::create(&output_path)?;
    let writer = CogWriter::new(file, options)?;
    writer.write_buffer(&buffer)?;

    println!("Wrote COG to {:?}", output_path);
    Ok(())
}
```

### Compression Options

The GeoTIFF driver supports multiple compression schemes:

```rust
use oxigdal_geotiff::tiff::Compression;

// Available compression methods:
let compressions = vec![
    Compression::None,           // No compression (largest file)
    Compression::Deflate,        // DEFLATE/zlib (good compression, fast)
    Compression::Lzw,           // LZW (good for categorical data)
    Compression::Zstd,          // ZSTD (best compression, requires feature)
    Compression::Jpeg,          // JPEG (lossy, for photos)
];
```

---

## GeoJSON Driver

**Crate**: `oxigdal-geojson`
**Format**: GeoJSON (RFC 7946)
**Type**: Vector
**Read**: ✅ Full support
**Write**: ✅ Full support

### Overview

The GeoJSON driver provides full RFC 7946 compliance with support for all geometry types, features, and coordinate reference systems.

### Key Features

- ✅ All geometry types (Point, LineString, Polygon, Multi*, GeometryCollection)
- ✅ Feature and FeatureCollection support
- ✅ Property preservation (arbitrary JSON)
- ✅ Bounding box support
- ✅ CRS handling (with legacy support)
- ✅ Streaming reader for large files
- ✅ Pretty-printing and compact output
- ✅ Validation against RFC 7946

### Reading GeoJSON

```rust
use oxigdal_geojson::{GeoJsonReader, FeatureCollection};
use std::fs::File;
use std::io::BufReader;

fn read_geojson() -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open("boundaries.geojson")?;
    let reader = BufReader::new(file);

    let mut geojson_reader = GeoJsonReader::new(reader);
    let collection = geojson_reader.read_feature_collection()?;

    println!("Features: {}", collection.features.len());

    // Access features
    for feature in &collection.features {
        if let Some(geometry) = &feature.geometry {
            println!("Type: {:?}", geometry.geometry_type);
            println!("Coords: {} points", geometry.coordinates.len());
        }

        // Access properties
        if let Some(name) = feature.properties.get("name") {
            println!("Name: {:?}", name);
        }
    }

    Ok(())
}
```

### Writing GeoJSON

```rust
use oxigdal_geojson::{
    GeoJsonWriter, FeatureCollection, Feature, Geometry, GeometryType,
    Properties, Coordinate,
};
use std::fs::File;
use std::io::BufWriter;

fn write_geojson() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let output_path = temp_dir.join("output.geojson");

    // Create a feature collection
    let mut collection = FeatureCollection {
        features: vec![],
        bbox: None,
        foreign_members: Default::default(),
    };

    // Create a point feature
    let mut properties = Properties::new();
    properties.insert("name".to_string(), "San Francisco".into());
    properties.insert("population".to_string(), 874961.into());

    let point = Feature {
        geometry: Some(Geometry {
            geometry_type: GeometryType::Point,
            coordinates: vec![vec![-122.4194, 37.7749]],
            bbox: None,
        }),
        properties,
        id: Some("sf".to_string()),
        bbox: None,
        foreign_members: Default::default(),
    };

    collection.features.push(point);

    // Write with pretty formatting
    let file = File::create(&output_path)?;
    let writer = BufWriter::new(file);
    let mut geojson_writer = GeoJsonWriter::new(writer);
    geojson_writer.set_pretty(true);
    geojson_writer.write_feature_collection(&collection)?;

    Ok(())
}
```

### Geometry Types

All GeoJSON geometry types are supported:

```rust
use oxigdal_geojson::{Geometry, GeometryType};

// Point
let point = Geometry {
    geometry_type: GeometryType::Point,
    coordinates: vec![vec![-122.0, 37.0]],
    bbox: None,
};

// LineString
let linestring = Geometry {
    geometry_type: GeometryType::LineString,
    coordinates: vec![
        vec![-122.0, 37.0],
        vec![-122.1, 37.1],
        vec![-122.2, 37.2],
    ],
    bbox: None,
};

// Polygon (with hole)
let polygon = Geometry {
    geometry_type: GeometryType::Polygon,
    coordinates: vec![
        // Outer ring
        vec![
            vec![0.0, 0.0],
            vec![10.0, 0.0],
            vec![10.0, 10.0],
            vec![0.0, 10.0],
            vec![0.0, 0.0],
        ],
        // Inner ring (hole)
        vec![
            vec![2.0, 2.0],
            vec![8.0, 2.0],
            vec![8.0, 8.0],
            vec![2.0, 8.0],
            vec![2.0, 2.0],
        ],
    ],
    bbox: None,
};
```

### Validation

```rust
use oxigdal_geojson::Validator;

fn validate_geojson(collection: &FeatureCollection) {
    let validator = Validator::new();
    let results = validator.validate_collection(collection);

    if results.is_valid() {
        println!("✓ Valid GeoJSON");
    } else {
        for error in results.errors() {
            println!("✗ Error: {}", error);
        }
        for warning in results.warnings() {
            println!("⚠ Warning: {}", warning);
        }
    }
}
```

---

## GeoParquet Driver

**Crate**: `oxigdal-geoparquet`
**Format**: GeoParquet
**Type**: Vector (columnar)
**Read**: ✅ Full support
**Write**: ✅ Full support

### Overview

GeoParquet is a columnar vector format built on Apache Parquet, optimized for cloud storage and analytics.

### Key Features

- ✅ Apache Arrow integration
- ✅ Spatial indexing (R-tree)
- ✅ Multiple geometry encoding (WKB, WKT)
- ✅ Compression (Snappy, GZIP, ZSTD, LZ4)
- ✅ Partitioning support
- ✅ Cloud-optimized reads
- ✅ Zero-copy where possible

### Reading GeoParquet

```rust
use oxigdal_geoparquet::GeoParquetReader;
use std::fs::File;

fn read_geoparquet() -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open("cities.parquet")?;
    let reader = GeoParquetReader::new(file)?;

    // Get metadata
    let metadata = reader.metadata()?;
    println!("Primary geometry column: {:?}", metadata.primary_column);
    println!("CRS: {:?}", metadata.crs);
    println!("Bbox: {:?}", metadata.bbox);

    // Read all data as Arrow RecordBatches
    let batches = reader.read_all()?;
    for batch in &batches {
        println!("Batch with {} rows", batch.num_rows());
    }

    Ok(())
}
```

### Writing GeoParquet

```rust
use oxigdal_geoparquet::{GeoParquetWriter, GeoParquetMetadata, CompressionCodec};
use arrow_array::RecordBatch;
use arrow_schema::{Schema, Field, DataType};
use std::fs::File;
use std::sync::Arc;

fn write_geoparquet() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let output_path = temp_dir.join("output.parquet");

    // Create Arrow schema
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("geometry", DataType::Binary, false),
    ]));

    // Create metadata
    let metadata = GeoParquetMetadata {
        version: "1.0.0".to_string(),
        primary_column: "geometry".to_string(),
        columns: Default::default(),
        crs: Some("EPSG:4326".to_string()),
        bbox: None,
    };

    // Create writer
    let file = File::create(&output_path)?;
    let mut writer = GeoParquetWriter::new(file, schema.clone(), metadata)?;
    writer.set_compression(CompressionCodec::Zstd);

    // Write batches
    // (create RecordBatch with actual data)
    // writer.write_batch(&batch)?;

    writer.finish()?;

    Ok(())
}
```

---

## Zarr Driver

**Crate**: `oxigdal-zarr`
**Format**: Zarr (v2 and v3)
**Type**: Raster (N-dimensional)
**Read**: ✅ Full support
**Write**: ✅ Full support

### Overview

Zarr is a chunked, compressed, N-dimensional array format optimized for cloud storage.

### Key Features

- ✅ Zarr v2 and v3 support
- ✅ Multiple storage backends (filesystem, S3, HTTP)
- ✅ Multiple codecs (GZIP, ZSTD, LZ4, Blosc)
- ✅ Chunk caching
- ✅ Lazy loading
- ✅ Metadata consolidation

### Reading Zarr

```rust
use oxigdal_zarr::{ZarrReader, storage::FilesystemStore};

fn read_zarr() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let zarr_path = temp_dir.join("data.zarr");

    // Create storage backend
    let store = FilesystemStore::new(zarr_path)?;
    let reader = ZarrReader::open(store, "array_name")?;

    // Get array metadata
    let metadata = reader.metadata();
    println!("Shape: {:?}", metadata.shape);
    println!("Chunks: {:?}", metadata.chunks);
    println!("Data type: {:?}", metadata.dtype);

    // Read a chunk
    let chunk_data = reader.read_chunk(&[0, 0, 0])?;
    println!("Read {} bytes", chunk_data.len());

    Ok(())
}
```

### Writing Zarr

```rust
use oxigdal_zarr::{
    ZarrWriter, storage::FilesystemStore,
    metadata::{ArrayMetadata, DataType},
    codecs::CompressionCodec,
};

fn write_zarr() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let zarr_path = temp_dir.join("output.zarr");

    // Create storage backend
    let store = FilesystemStore::new(zarr_path)?;

    // Create array metadata
    let metadata = ArrayMetadata {
        shape: vec![100, 100, 100],
        chunks: vec![10, 10, 10],
        dtype: DataType::Float32,
        compressor: Some(CompressionCodec::Zstd),
        fill_value: Some(0.0),
        ..Default::default()
    };

    // Create writer
    let mut writer = ZarrWriter::create(store, "array_name", metadata)?;

    // Write chunks
    let chunk_data = vec![0.0f32; 1000]; // 10x10x10 chunk
    writer.write_chunk(&[0, 0, 0], &chunk_data)?;

    writer.finish()?;

    Ok(())
}
```

### Cloud Storage (S3)

```rust
use oxigdal_zarr::{ZarrReader, storage::S3Store};

async fn read_zarr_s3() -> Result<(), Box<dyn std::error::Error>> {
    // Create S3 storage backend
    let store = S3Store::new("my-bucket", "path/to/data.zarr").await?;
    let reader = ZarrReader::open(store, "array")?;

    // Read data
    let chunk = reader.read_chunk(&[0, 0])?;

    Ok(())
}
```

---

## FlatGeobuf Driver

**Crate**: `oxigdal-flatgeobuf`
**Format**: FlatGeobuf
**Type**: Vector (streaming)
**Read**: ✅ Full support
**Write**: ✅ Full support

### Overview

FlatGeobuf is a binary vector format optimized for streaming and HTTP range requests, with built-in spatial indexing.

### Key Features

- ✅ Streaming reads (constant memory)
- ✅ HTTP range request support
- ✅ Packed Hilbert R-tree index
- ✅ All geometry types
- ✅ Property attributes
- ✅ No external dependencies

### Reading FlatGeobuf

```rust
use oxigdal_flatgeobuf::FgbReader;
use std::fs::File;

fn read_flatgeobuf() -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open("data.fgb")?;
    let mut reader = FgbReader::new(file)?;

    // Get header metadata
    let header = reader.header();
    println!("Features: {}", header.features_count);
    println!("Geometry type: {:?}", header.geometry_type);

    // Read features (streaming - low memory)
    while let Some(feature) = reader.read_feature()? {
        println!("Feature ID: {:?}", feature.id);
        if let Some(geom) = feature.geometry {
            println!("Geometry: {:?}", geom);
        }
    }

    Ok(())
}
```

### Spatial Queries with HTTP

```rust
use oxigdal_flatgeobuf::HttpReader;
use oxigdal_core::types::BoundingBox;

async fn query_flatgeobuf_http() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://example.com/data.fgb";
    let reader = HttpReader::new(url).await?;

    // Spatial query using bounding box
    let bbox = BoundingBox::new(-122.5, 37.7, -122.4, 37.8)?;
    let features = reader.query_bbox(&bbox).await?;

    println!("Found {} features in bbox", features.len());

    Ok(())
}
```

### Writing FlatGeobuf

```rust
use oxigdal_flatgeobuf::{FgbWriter, Header, Feature};
use oxigdal_core::vector::GeometryType;
use std::fs::File;

fn write_flatgeobuf() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = std::env::temp_dir();
    let output_path = temp_dir.join("output.fgb");

    // Create header
    let header = Header {
        geometry_type: GeometryType::Point,
        features_count: 0,
        has_z: false,
        has_m: false,
        ..Default::default()
    };

    // Create writer
    let file = File::create(&output_path)?;
    let mut writer = FgbWriter::new(file, header)?;

    // Write features
    // (create Feature objects and write)
    // writer.write_feature(&feature)?;

    writer.finish()?;

    Ok(())
}
```

---

## Performance Comparison

| Format | Read Speed | Write Speed | File Size | Cloud Optimized | Use Case |
|--------|-----------|-------------|-----------|-----------------|----------|
| GeoTIFF/COG | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | Large | ✅ | Raster imagery, DEMs |
| GeoJSON | ⭐⭐⭐ | ⭐⭐⭐⭐ | Large | ❌ | Simple vectors, web APIs |
| GeoParquet | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | Small | ✅ | Analytics, large datasets |
| Zarr | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | Small | ✅ | N-D arrays, climate data |
| FlatGeobuf | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | Medium | ✅ | Streaming vectors, web maps |

## See Also

- [Quickstart Guide](oxigdal_quickstart_guide.md)
- [Algorithm Guide](oxigdal_algorithm_guide.md)
- [WASM Guide](oxigdal_wasm_guide.md)
