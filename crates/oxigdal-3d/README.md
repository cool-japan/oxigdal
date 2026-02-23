# OxiGDAL 3D

[![Crates.io](https://img.shields.io/crates/v/oxigdal-3d.svg)](https://crates.io/crates/oxigdal-3d)
[![Documentation](https://docs.rs/oxigdal-3d/badge.svg)](https://docs.rs/oxigdal-3d)
[![License](https://img.shields.io/crates/l/oxigdal-3d.svg)](LICENSE)

Comprehensive 3D geospatial data handling for OxiGDAL. Process point clouds, create terrain meshes, and build web-ready 3D visualizations entirely in Pure Rust.

## Overview

OxiGDAL 3D provides production-ready support for:

- **Point Cloud Formats**: LAS/LAZ, Cloud Optimized Point Cloud (COPC), Entwine Point Tiles (EPT)
- **3D Mesh Operations**: OBJ and glTF 2.0/GLB export with materials and textures
- **Terrain Processing**: Triangulated Irregular Networks (TIN), DEM to mesh conversion
- **Web Visualization**: 3D Tiles (Cesium format) for browser-based 3D mapping
- **Automatic Classification**: Ground, vegetation, and building point extraction
- **Spatial Indexing**: R*-tree based spatial queries for efficient large-scale processing

## Pure Rust Implementation

This library is **100% Pure Rust** with zero C/Fortran dependencies. All functionality works out of the box without external build tools or system libraries.

## Features

- **LAS/LAZ Point Cloud Support**: Read and write LAS/LAZ format with full compression support via LAZ
- **Cloud Optimized Point Clouds**: COPC hierarchical access with HTTP range requests
- **Entwine Point Tiles**: EPT octree structure support for massive datasets
- **Mesh Export**: Generate OBJ and glTF 2.0/GLB with materials, normals, and texture coordinates
- **TIN Generation**: Delaunay triangulation-based Triangulated Irregular Networks
- **DEM Processing**: Convert Digital Elevation Models to 3D meshes with customizable resolution
- **3D Tiles**: Cesium-compatible 3D Tiles generation for web-based visualization
- **Point Classification**: Progressive morphological filtering for ground, vegetation, and building classification
- **Spatial Indexing**: R*-tree spatial indexes for efficient neighborhood queries
- **Async Support**: Optional async/await API for cloud storage backends (COPC, EPT)
- **Streaming**: Memory-efficient processing for datasets larger than available RAM
- **Error Handling**: No unwrap() policy with descriptive error types

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-3d = "0.1"
```

### Feature Flags

Enable specific capabilities as needed:

```toml
[dependencies]
oxigdal-3d = { version = "0.1", features = ["async", "copc", "ept"] }
```

| Feature | Description |
|---------|-------------|
| `las-laz` | LAS/LAZ point cloud support (default) |
| `mesh` | OBJ and glTF mesh export (default) |
| `terrain` | TIN and DEM processing (default) |
| `tiles3d` | 3D Tiles visualization (default) |
| `async` | Async/await support for cloud backends |
| `copc` | Cloud Optimized Point Cloud access (requires `async`) |
| `ept` | Entwine Point Tiles support (requires `async`) |

## Quick Start

### Read and Process LAS Point Cloud

```rust
use oxigdal_3d::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open LAS file
    let mut reader = pointcloud::LasReader::open("input.las")?;
    let point_cloud = reader.read_all()?;

    println!("Loaded {} points", point_cloud.len());
    println!("Bounds: {:?}", point_cloud.bounds());

    Ok(())
}
```

### Classify Ground Points

```rust
use oxigdal_3d::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = pointcloud::LasReader::open("input.las")?;
    let point_cloud = reader.read_all()?;

    // Classify ground points automatically
    let ground_points = classification::classify_ground(&point_cloud.points)?;
    println!("Found {} ground points", ground_points.len());

    Ok(())
}
```

### Create Terrain from Ground Points

```rust
use oxigdal_3d::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = pointcloud::LasReader::open("input.las")?;
    let point_cloud = reader.read_all()?;

    // Extract ground points
    let ground = classification::classify_ground(&point_cloud.points)?;

    // Create TIN (Triangulated Irregular Network)
    let tin = terrain::create_tin(&ground)?;

    // Convert to mesh
    let mesh = terrain::tin_to_mesh(&tin)?;

    // Export as glTF
    mesh::export_gltf(&mesh, "terrain.glb")?;

    Ok(())
}
```

### Export to Web-Ready 3D Tiles

```rust
use oxigdal_3d::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = pointcloud::LasReader::open("points.las")?;
    let point_cloud = reader.read_all()?;

    // Create 3D tileset for Cesium
    let tileset = visualization::create_3d_tileset(
        &point_cloud.points,
        Default::default(),
    )?;

    // Write tileset.json and tile files
    visualization::write_3d_tiles(&tileset, "output_tiles")?;

    Ok(())
}
```

## Usage Guide

### Point Cloud Operations

The point cloud module provides comprehensive support for reading, writing, and analyzing point clouds:

```rust
use oxigdal_3d::pointcloud::*;

// Read LAS file
let mut reader = LasReader::open("data.las")?;
let header = reader.header();
println!("Point count: {}", header.point_count);

// Access point cloud data
let points = reader.read_all()?;
let ground = points.filter_by_classification(Classification::Ground);

// Spatial queries
let index = SpatialIndex::new(points.points);
let nearby = index.within_radius(x, y, z, 10.0); // 10m radius
let nearest = index.nearest_k(x, y, z, 5);      // 5 nearest points

// Write results
let mut writer = LasWriter::create("output.las", &header)?;
for point in ground {
    writer.write_point(&point)?;
}
```

### Mesh Creation and Export

```rust
use oxigdal_3d::mesh::*;

// Create vertices
let v1 = Vertex::new([0.0, 0.0, 0.0]);
let v2 = Vertex::new([1.0, 0.0, 0.0]);
let v3 = Vertex::new([0.5, 1.0, 0.0]);

let vertices = vec![v1, v2, v3];
let indices = vec![Triangle::new(0, 1, 2)];

let mut material = Material::new("ground");
material.base_color = [0.5, 0.5, 0.5, 1.0];

let mesh = Mesh::new(vertices, indices, vec![material]);

// Export to different formats
export_obj(&mesh, "output.obj")?;
export_gltf(&mesh, "output.glb")?;
```

### Terrain Processing

```rust
use oxigdal_3d::terrain::*;

// Create TIN from point cloud
let tin = create_tin(&ground_points)?;

// Convert to mesh for visualization
let mesh = tin_to_mesh(&tin)?;

// DEM to mesh conversion
let options = DemMeshOptions {
    max_z_error: 0.5,
    simplification: true,
    ..Default::default()
};

let terrain_mesh = dem_to_mesh(&dem_data, &options)?;
```

### Classification Algorithms

```rust
use oxigdal_3d::classification::*;

let params = ClassificationParams {
    search_radius: 2.0,
    min_points: 5,
    ground_threshold: 0.5,
    vegetation_range: (0.5, 30.0),
    building_height: 3.0,
    noise_threshold: 0.1,
};

// Classify with custom parameters
let ground = classify_ground_with_params(&points, &params)?;
let vegetation = classify_vegetation_with_params(&points, &params)?;
let buildings = classify_buildings(&points, &params)?;
```

### Async Cloud Data Access

With the `async` feature enabled, access cloud-optimized point clouds:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // COPC (Cloud Optimized Point Cloud) from HTTP
    #[cfg(feature = "copc")]
    {
        let reader = pointcloud::copc::CopcReader::open_async(
            "https://example.com/points.copc.laz"
        ).await?;
        let info = reader.info()?;
        println!("COPC points: {}", info.info.point_count);
    }

    // EPT (Entwine Point Tiles) from cloud storage
    #[cfg(feature = "ept")]
    {
        let reader = pointcloud::ept::EptReader::open_async(
            "https://example.com/ept.json"
        ).await?;
        let metadata = reader.metadata()?;
        println!("EPT bounds: {:?}", metadata.bounds);
    }

    Ok(())
}
```

## API Overview

### Core Modules

| Module | Purpose |
|--------|---------|
| `pointcloud` | LAS/LAZ reading/writing, COPC, EPT, spatial indexing |
| `mesh` | 3D mesh structures, OBJ/glTF export, materials |
| `terrain` | TIN generation, DEM conversion, surface analysis |
| `visualization` | 3D Tiles, Cesium format, LOD structures |
| `classification` | Point classification, filtering, segmentation |
| `error` | Error types and Result definitions |

### Main Types

- **Point**: 3D point with elevation, classification, intensity, RGB
- **PointCloud**: Collection of points with LAS header metadata
- **SpatialIndex**: R*-tree based spatial indexing for efficient queries
- **Mesh**: Vertices, triangles, materials, and texture coordinates
- **Tin**: Triangulated Irregular Network for terrain representation
- **Tileset**: 3D Tiles structure for web visualization
- **Classification**: Point class enumerations (Ground, Vegetation, Buildings, etc.)

## Performance Characteristics

- **Spatial Indexing**: O(log n) point queries using R*-tree
- **Point Classification**: Multi-threaded processing using Rayon
- **Mesh Generation**: O(n log n) Delaunay triangulation
- **Memory Efficiency**: Streaming I/O for files larger than available RAM

### Benchmark Results (on modern hardware)

| Operation | Time | Memory |
|-----------|------|--------|
| Load 1M points (LAS) | ~500ms | ~200MB |
| Classify ground | ~2s | ~150MB |
| Create TIN (100k points) | ~1s | ~80MB |
| Export glTF mesh | ~300ms | ~50MB |
| Generate 3D Tiles | ~5s | ~300MB |

## Examples

Comprehensive examples are provided in the test suite:

```bash
# Run tests
cargo test --lib

# Run with specific features
cargo test --features "copc,ept" --lib

# Run benchmarks
cargo bench
```

Key examples demonstrate:
- Loading and analyzing point clouds
- Automatic ground classification
- TIN creation and mesh generation
- 3D Tiles creation for web visualization
- Async COPC and EPT access

## Documentation

Full API documentation is available at [docs.rs/oxigdal-3d](https://docs.rs/oxigdal-3d).

Key documentation:
- Module documentation in source code
- Comprehensive example code in doc comments
- Error type documentation with recovery strategies
- Performance optimization guidelines

## Error Handling

This library follows the "no unwrap" policy. All fallible operations return descriptive `Result<T>` types:

```rust
use oxigdal_3d::{Result, Error};

fn process() -> Result<()> {
    let cloud = pointcloud::LasReader::open("file.las")?
        .read_all()?;

    // Handle specific errors
    match classification::classify_ground(&cloud.points) {
        Ok(ground) => println!("Classified {} ground points", ground.len()),
        Err(Error::EmptyDataset(msg)) => eprintln!("No data: {}", msg),
        Err(e) => eprintln!("Classification failed: {}", e),
    }

    Ok(())
}
```

Error types include:
- I/O errors (file access, network)
- Format errors (LAS, glTF, JSON)
- Geometry errors (invalid bounds, topology)
- Processing errors (classification, triangulation)
- Configuration errors (invalid parameters)

## Workspace Integration

OxiGDAL 3D is part of the larger OxiGDAL ecosystem:

- **oxigdal-core**: Core geospatial types and utilities
- **oxigdal-algorithms**: Spatial algorithms and analysis
- **oxigdal-proj**: Coordinate system transformations
- **oxigdal-drivers**: Format-specific drivers

See [OxiGDAL](https://github.com/cool-japan/oxigdal) for the full ecosystem.

## COOLJAPAN Standards

This project adheres to COOLJAPAN ecosystem requirements:

- **Pure Rust**: 100% Pure Rust implementation (no C/Fortran dependencies)
- **No Unwrap Policy**: All error paths use `Result<T>` types
- **Workspace Policy**: Uses workspace dependencies with no version duplication
- **Latest Crates**: Dependencies kept current with latest versions
- **Code Quality**: No warnings, comprehensive error handling

## License

This project is licensed under Apache-2.0. See [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines.

## Related Projects

- [OxiGDAL](https://github.com/cool-japan/oxigdal) - Complete geospatial data toolkit
- [OxiBLAS](https://github.com/cool-japan/oxiblas) - Pure Rust linear algebra
- [SciRS2](https://github.com/cool-japan/scirs) - Scientific computing ecosystem

---

Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem of Pure Rust geospatial and scientific computing libraries.
