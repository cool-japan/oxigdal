# oxigdal-noalloc

`no_std`, zero-allocation geometry primitives for the
[OxiGDAL](https://github.com/cool-japan/oxigdal) ecosystem. Designed for
embedded and RISC-V environments where heap allocation is unavailable.

## Features

- `Point2D` / `Point3D` -- distance, midpoint, projection
- `BBox2D` -- axis-aligned bounding box with containment and intersection tests
- `LineSegment2D` -- parametric intersection detection
- `Triangle2D` -- area (shoelace), containment (barycentric), centroid, perimeter
- `FixedPolygon<N>` -- inline fixed-capacity polygon (no heap)
- `CoordTransform` -- 2D affine transforms (scale, translate, rotate, compose)
- `GeoHashFixed` -- geohash encoding stored as `[u8; 12]`
- Software `sqrt`/`sin`/`cos` via Newton-Raphson and Taylor series (no libm dependency)
- `#![deny(unsafe_code)]`

## Usage

```rust
use oxigdal_noalloc::{Point2D, FixedPolygon, CoordTransform};

let a = Point2D::new(0.0, 0.0);
let b = Point2D::new(3.0, 4.0);
assert!((a.distance_to(&b) - 5.0).abs() < 1e-10);

let mut poly = FixedPolygon::<16>::new();
poly.try_push(Point2D::new(0.0, 0.0));
poly.try_push(Point2D::new(4.0, 0.0));
poly.try_push(Point2D::new(4.0, 3.0));
assert!((poly.area() - 6.0).abs() < 1e-10);

let transform = CoordTransform::translate(10.0, 20.0);
let moved = transform.apply(a);
assert!((moved.x - 10.0).abs() < 1e-10);
```

## Status

- 59 tests passing, 0 failures

## License

See the top-level [OxiGDAL](https://github.com/cool-japan/oxigdal) repository for license details.
