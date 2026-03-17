# oxigdal-copc

Pure Rust COPC (Cloud Optimized Point Cloud) and LAS 1.4 reader for the
[OxiGDAL](https://github.com/cool-japan/oxigdal) ecosystem. No C/Fortran
dependencies.

## Features

- ASPRS LAS 1.4 public header parser (`LasHeader`, `LasVersion`)
- COPC-specific VLR parsing (`CopcInfo`, `Vlr`, `VlrKey`)
- Full 3D point representation with LAS 1.4 fields and ASPRS classification codes 0-18
- Octree spatial index with bbox/sphere queries, k-nearest neighbours, and voxel downsampling
- Height profile extraction and ground filtering

## Usage

```rust
use oxigdal_copc::{Point3D, BoundingBox3D, Octree};

// Build an octree from point cloud data
let bbox = BoundingBox3D::new(0.0, 0.0, 0.0, 100.0, 100.0, 50.0);
let mut octree = Octree::new(bbox, 8); // max depth = 8

let point = Point3D::new(10.0, 20.0, 5.0);
octree.insert(point);

// Spatial query
let query_box = BoundingBox3D::new(0.0, 0.0, 0.0, 50.0, 50.0, 25.0);
let results = octree.query_bbox(&query_box);
```

## Status

- 139 tests passing, 0 failures

## License

See the top-level [OxiGDAL](https://github.com/cool-japan/oxigdal) repository for license details.
