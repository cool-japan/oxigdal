# TODO: oxigdal-index

## High Priority
- [x] Implement entry deletion from RTree (completed 2026-04-19, part of I1 R-tree enhancements)
- [x] Add bulk loading (Sort-Tile-Recursive / STR) for RTree (completed 2026-04-19, part of I1 R-tree enhancements)
- [ ] Implement R*-tree insertion with forced reinsert strategy (current implementation uses basic linear split only)
- [ ] Add 3D R-tree variant (Bbox3D + 3D spatial queries) for point cloud and volumetric data
- [x] Implement proper priority-queue k-NN search on RTree (completed 2026-04-19, part of I1 R-tree enhancements)
- [x] Add serialization/deserialization for RTree (completed 2026-04-19, part of I1 R-tree enhancements)

## Medium Priority
- [ ] Implement search_dedup for GridIndex (currently documents duplicates but provides no built-in dedup)
- [ ] Add Hilbert R-tree variant for better spatial locality on disk-backed indices
- [ ] Implement line-segment intersection query (find all entries whose bbox intersects a polyline corridor)
- [ ] Add polygon-polygon intersection test (beyond bbox overlap, actual geometry intersection)
- [ ] Implement Visvalingam-Whyatt simplification as alternative to Douglas-Peucker
- [ ] Add multi-polygon support to validation module (validate_multipolygon with shared-edge checks)
- [ ] Implement spatial hash grid as a third index option (good for uniformly-sized objects)
- [ ] Add window query with result count limit (top-k within bbox, sorted by distance to center)
- [ ] Implement polygon union, intersection, and difference (boolean operations via Weiler-Atherton or similar)

## Low Priority / Future
- [ ] Add adaptive grid index that automatically subdivides hot cells (quadtree-like refinement)
- [ ] Implement geographic distance queries (haversine/vincenty) alongside Euclidean distance
- [ ] Add Voronoi diagram construction from point set
- [ ] Implement Delaunay triangulation for TIN generation
- [ ] Add spatial clustering (DBSCAN, k-means) operating on the RTree index
- [ ] Implement feature-gated parallel spatial join using rayon
- [ ] Add streaming/online insertion that maintains a balanced tree without full rebuild
- [ ] Implement minimum bounding circle (smallest enclosing circle) for point sets
- [ ] Add no_std support for GridIndex (currently only RTree and operations support no_std)
- [ ] Implement line intersection sweep-line algorithm for efficient batch intersection detection
