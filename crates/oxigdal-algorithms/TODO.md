# TODO: oxigdal-algorithms

## High Priority
- [ ] Implement Weiler-Atherton polygon clipping for robust polygon intersection
- [ ] Add geodetic area calculation using Karney's method (currently Haversine-based)
- [ ] Implement topology-preserving simplification (shared edges between adjacent polygons)
- [ ] Add `Overlaps` and `Crosses` spatial predicates (DE-9IM matrix)
- [ ] Implement raster contour generation (marching squares algorithm)
- [ ] Add SIMD-optimized bilinear/bicubic resampling kernels (AVX2 + NEON)
- [ ] Implement raster polygonization (connected component labeling)
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
