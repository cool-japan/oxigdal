# oxigeo-index

Pure Rust spatial indexing library for the
[OxiGeo](https://github.com/cool-japan/oxigeo) ecosystem. Provides R-tree
and grid-based spatial indices, geometry validation, and computational geometry
operations.

## Features

- **R-tree** (R*-tree split heuristic with forced reinsertion) -- point/window/top-k queries, priority-queue k-nearest neighbours, spatial joins (optionally parallel via `parallel`/rayon), bulk loading (STR), line-corridor search
- **3D R-tree** (`RTree3D`) -- volumetric/point-cloud indexing with 3D k-NN and bulk load
- **Hilbert R-tree** -- Hilbert-curve-ordered bulk-loaded index for disk-friendly locality
- **Grid index** and **spatial hash grid** -- fast uniform-distribution and unbounded-extent spatial lookups
- **Adaptive grid** -- loose-quadtree that subdivides hot cells automatically
- **Streaming R-tree** -- online insertion with buffered rebalancing (no full rebuild per insert)
- **Geometry operations** -- area, perimeter, centroid, convex hull, point-in-polygon, buffer, simplify (Douglas-Peucker and Visvalingam-Whyatt), distance
- **Polygon boolean ops** -- union, intersection, difference, symmetric difference (Sutherland-Hodgman / Weiler-Atherton)
- **Polygon / multipolygon validation** -- ring closure, orientation, self-intersection, shared-edge and interior-overlap checks
- **Computational geometry** -- Voronoi diagrams, Delaunay triangulation, minimum bounding circle (Welzl), Bentley-Ottmann line-segment intersection sweep
- **Spatial clustering** -- DBSCAN over an internal R-tree
- **Geographic distance** -- haversine and Vincenty-inverse (WGS84) distance/nearest-k/within-radius queries
- `no_std` support: the original R-tree/bbox/operations core is `no_std` + `alloc` compatible, but several modules added since (grid index, spatial hash, adaptive grid, 3D R-tree, Hilbert R-tree, Voronoi, DBSCAN, sweep-line, streaming R-tree, bounding circle) currently import `std::collections`/`std::cmp` directly and are **not** yet no_std-clean -- `cargo build -p oxigeo-index --no-default-features` currently fails. See TODO.md.

## Usage

```rust
use oxigeo_index::{RTree, Bbox2D, SpatialQuery};

let mut tree: RTree<&str> = RTree::new();
tree.insert(Bbox2D::new(0.0, 0.0, 2.0, 2.0).unwrap(), "polygon A");
tree.insert(Bbox2D::new(3.0, 3.0, 5.0, 5.0).unwrap(), "polygon B");

let query = Bbox2D::new(1.0, 1.0, 4.0, 4.0).unwrap();
let hits = tree.search(&query);
assert_eq!(hits.len(), 2);

let count = SpatialQuery::count_in(&tree, &query);
assert_eq!(count, 2);
```

## Status

- 460 tests passing, 0 failures (all-features; 446 with default features)

## License

See the top-level [OxiGeo](https://github.com/cool-japan/oxigeo) repository for license details.
