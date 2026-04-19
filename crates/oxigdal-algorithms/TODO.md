# TODO: oxigdal-algorithms

## High Priority
- [x] Implement Weiler-Atherton polygon clipping for robust polygon intersection (completed 2026-04-19)
  - **Goal:** Production-grade polygon clipping engine supporting Intersection, Union, Difference, XOR. Handles holes, degeneracies, self-intersecting polygons. difference.rs refactored from 1859 to 1650 lines (production code 996 lines).
  - **Design:** ClipOperation enum, ClipVertex with parametric alpha + cross-links, Weiler-Atherton 5-phase algorithm (intersection detection, entry/exit labeling, polygon tracing, hole handling, degeneracy handling). Area validation fallback for edge cases.
  - **Files:** CREATED clipping.rs (1621 lines), MODIFIED difference.rs/intersection.rs/union_ops.rs/mod.rs/lib.rs
  - **Tests:** 19 new tests: overlapping squares, holes, containment, L-shaped concave, self-intersecting bowtie, degenerate shared edge, XOR identical/disjoint, area invariants, clip_multi, validation errors
- [x] Add geodetic area calculation using Karney's method (completed 2026-04-19)
  - **Goal:** Karney's ellipsoidal geodesic area algorithm, sub-millimeter accuracy, antimeridian/polar support. New AreaMethod::KarneyGeodesic variant.
  - **Design:** Geodesic struct wrapping geographiclib-rs (Pure Rust), PolygonArea with signed/unsigned modes, InverseResult with S12 area element.
  - **Files:** CREATE geodesic.rs (981 lines), MODIFY area.rs/mod.rs/Cargo.toml
  - **Tests:** 25 tests: equator unit square (rel err <1e-6 vs GeographicLib), ellipsoid area, high-lat, antimeridian, polar, CW/CCW, diamond, perimeter, signed winding, degenerate collinear, open ring, hole subtraction, AreaMethod integration
- [x] Implement topology-preserving simplification (shared edges between adjacent polygons) (completed 2026-04-19)
  - **Goal:** Shared edges simplified once, both polygons get identical result. Junction vertices preserved.
  - **Design:** Edge decomposition + canonical EdgeKey (quantized 1e6), chain building per ring, Douglas-Peucker on shared chains with shared_cache (canonical poly-pair ordering), non-shared chains simplified independently, ring reassembly with junction dedup, self-intersection validation with retry at reduced tolerance.
  - **Files:** CREATE src/vector/topology_simplify.rs (1419 lines), MODIFY src/vector/mod.rs (module declared + re-exports)
  - **Tests:** 24 tests passing: adjacent squares basic, 2x2 grid, non-adjacent polygons, jagged shared edge, polygon with hole, junction preservation, self-intersection prevention, bowtie rejection, tolerance=0 no-op, negative tolerance error, shared-edge consistency between reverse-winding polygons, large (100-pt) circle, options/quantize/edge-key/DP helpers
- [x] Add Overlaps and Crosses spatial predicates with full DE-9IM matrix (completed 2026-04-19)
  - **Goal:** Complete DE-9IM matrix computation. Fix CrossesPredicate for Polygon (OGC compliance). All named predicates: equals, disjoint, intersects, touches, crosses, within, contains, overlaps, covers, coveredBy.
  - **Design:** De9im struct ([Dimension; 9]), relate() function, pattern matching, named predicate methods. Fix: Polygon/Polygon crosses() returns false.
  - **Files:** CREATE de9im.rs, MODIFY contains.rs/mod.rs
  - **Tests:** 37 tests (35 unit + 1 doctest + 1 crosses-fix): disjoint/overlapping/contained/touching squares, line crossing polygon, point-polygon, equals, pattern round-trip, trait impls
- [x] Implement raster contour generation using marching squares algorithm (completed 2026-04-19)
  - **Goal:** Iso-contour extraction from raster grids as LineString geometries. Multi-level, linear interpolation, saddle disambiguation, NoData, GeoTransform.
  - **Design:** 4-bit case index, EDGE_TABLE lookup, saddle disambiguation via center value, greedy line assembly with quantized endpoint hash map, ContourOptions/ContourLevel/ContourResult structs.
  - **Files:** CREATE raster/contour.rs, MODIFY raster/mod.rs
  - **Tests:** 48 tests (47 unit + 1 doctest): flat raster, 2x2 gradient, 3x3 peak, saddle point, multi-level, NoData, GeoTransform, edge cases, line assembly, simplification, 100x100 radial stress test
- [x] Add SIMD-optimized bilinear/bicubic resampling kernels (AVX2 + NEON) (completed 2026-04-19)
  - **Goal:** AVX2 (8 pixels/iter) + NEON (4 pixels/iter) intrinsics for bilinear and bicubic. 3-6x throughput target.
  - **Design:** Gather-lerp-lerp bilinear with FMA, separable 4x4 bicubic with Catmull-Rom and vaddvq_f32 horizontal sum. Three sub-modules: avx2_impl, neon_impl, scalar_impl with runtime dispatch.
  - **Files:** MODIFIED src/simd/resampling.rs (1402 lines)
  - **Tests:** 29 tests: identity, up/downsample, odd sizes, asymmetric, gradient, monotonicity, accuracy vs scalar within 1e-4 (FMA rounding tolerance)
- [x] Implement raster polygonization (connected component labeling) (completed 2026-04-19)
  - **Goal:** Full raster-to-polygon via CCL + boundary tracing + polygon assembly. Equivalent to GDAL GDALPolygonize().
  - **Design:** Two-pass union-find CCL (path compression + union by rank), 4/8-connectivity, dual boundary strategies (pixel-edge rectilinear matching GDAL; Moore-Neighbor pixel-center), shoelace ring classification, hole assignment via smallest-containing-exterior PIP, optional Douglas-Peucker simplification, min-area filter, GeoTransform pixel-to-world.
  - **Files:** CREATED src/raster/polygonize/{mod.rs (1310L), union_find.rs (226L), boundary.rs (1093L)}; MODIFIED src/raster/mod.rs (+2 lines for re-exports)
  - **Tests:** 80 tests (12 union_find + 25 boundary + 43 polygonize): single pixel, uniform, checkerboard (4-conn=9 components, 8-conn=2), donut with hole (verifies interior ring), multi-component, NoData (NaN + custom), GeoTransform, 500x500 stress test, min_area filter, Douglas-Peucker simplify, 1xN/Nx1 grids, RasterBuffer integration.
- [ ] Add parallel raster processing via `rayon` feature (hillshade, slope, focal stats)

## Medium Priority
- [ ] Implement Snap Rounding for robust geometric operations with floating point
- [ ] Add weighted Voronoi diagrams (power diagrams)
- [ ] Implement constrained Delaunay triangulation (CDT)
- [ ] Add raster flow direction and flow accumulation (D8, D-infinity)
- [ ] Implement watershed delineation from flow accumulation grids
- [ ] Add morphological operations: opening, closing, top-hat, black-hat
- [ ] Implement line offset (parallel curves) for cartographic styling
- [ ] Add Frechet distance and Hausdorff distance between geometries
- [ ] Implement minimum bounding geometry (rotated rectangle, circle, convex hull)
- [ ] Add viewshed analysis with Earth curvature and refraction correction

## Low Priority / Future
- [ ] Implement DSL expression compiler to native code (dsl feature, currently interpreter)
- [ ] Add TIN interpolation (natural neighbor, IDW from triangulation)
- [ ] Implement point cloud thinning algorithms (grid, random, Poisson disk)
- [ ] Add map generalization operators (collapse, exaggerate, displace)
- [ ] Implement network routing (Dijkstra, A* on vector networks)
- [ ] Add cost-distance analysis with anisotropic friction surfaces
- [ ] Implement geometric median and other robust location estimators
- [ ] Add streaming/chunked raster processing for datasets exceeding RAM
