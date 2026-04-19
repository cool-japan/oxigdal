# TODO: oxigdal-noalloc

## High Priority
- [x] Add FixedLineString<N> type (completed 2026-04-19, part of N1)
- [x] Implement FixedRing<N> type (completed 2026-04-19, part of N1)
- [x] Add BBox3D type (completed 2026-04-19, part of N1)
- [ ] Implement fixed-capacity spatial index (FixedGridIndex<N, M>) for no_alloc spatial queries
- [x] Add Mercator projection (completed 2026-04-19, part of N1)
- [x] Implement geohash neighbour computation (completed 2026-04-19, part of N1)

## Medium Priority
- [ ] Add great-circle distance (Haversine formula) for Point2D interpreted as (lon, lat)
- [ ] Implement Vincenty distance formula for geodesic accuracy on the ellipsoid
- [ ] Add fixed-capacity point cloud type (FixedPointCloud<N>) with bbox, centroid, and k-nearest for Point3D
- [ ] Implement UTM zone determination and forward/inverse UTM projection (no_std, pure arithmetic)
- [ ] Add matrix inverse for CoordTransform (compute the reverse affine mapping)
- [ ] Implement FixedPolygon point-in-polygon test (ray casting on the inline vertex array)
- [ ] Add convex hull computation for FixedPolygon (Graham scan variant operating in-place on fixed arrays)
- [ ] Implement f64 atan2 using CORDIC or polynomial approximation for no_std (needed for bearing/azimuth)
- [ ] Add FixedMultiPoint<N> type for storing multiple points without allocation

## Low Priority / Future
- [ ] Add RISC-V soft-float verification (ensure libm_sqrt/libm_sin/libm_cos produce correct results on rv32imac)
- [ ] Implement S2 cell ID encoding/decoding as an alternative to geohash
- [ ] Add no_std WKB (Well-Known Binary) encoder/decoder operating on fixed-size buffers
- [ ] Implement fixed-size GeoJSON coordinate serialization (write [x,y] to &mut [u8] without alloc)
- [ ] Add compile-time unit tests via const_assert for geometry invariants
- [ ] Implement no_std tile coordinate computation (lonlat_to_tile, tile_to_bbox using only core math)
- [ ] Add Redox OS compatibility testing in CI
- [ ] Implement fixed-capacity R-tree (FixedRTree<N, M>) for embedded spatial indexing
- [ ] Add benchmark suite comparing no_alloc implementations against std counterparts for performance parity
- [ ] Implement coordinate quantization (f64 to i32 with scale/offset) for compact storage in constrained environments
