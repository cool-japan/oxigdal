# oxigdal-index

Pure Rust spatial indexing library for the
[OxiGDAL](https://github.com/cool-japan/oxigdal) ecosystem. Provides R-tree
and grid-based spatial indices, geometry validation, and computational geometry
operations.

## Features

- **R-tree** (linear-split variant) -- point/window queries, approximate k-nearest neighbours, spatial joins
- **Grid index** -- fast uniform-distribution spatial lookups
- **Geometry operations** -- area, perimeter, centroid, convex hull, point-in-polygon, buffer, simplify, distance
- **Polygon validation** -- ring closure, orientation, self-intersection checks
- `no_std` compatible (with `alloc`)

## Usage

```rust
use oxigdal_index::{RTree, Bbox2D, SpatialQuery};

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

- 153 tests passing, 0 failures

## License

See the top-level [OxiGDAL](https://github.com/cool-japan/oxigdal) repository for license details.
