# TODO: oxigdal-copc

## High Priority
- [ ] Implement actual COPC file reader: parse LAS header + VLR chain + hierarchy pages from byte stream to populate the Octree
- [ ] Add LAZ (compressed LAS) decompression support (pure Rust, no laszip C dependency)
- [ ] Support LAS point data record formats 6-10 (extended fields: NIR, waveform, scan angle as i16)
- [ ] Implement COPC hierarchy page traversal for level-of-detail streaming (read only the octree nodes needed for a spatial query)
- [ ] Add point record binary deserialization from raw LAS bytes using scale/offset from the header
- [ ] Implement COPC writer: serialize Octree back to LAS 1.4 + COPC VLR format

## Medium Priority
- [ ] Add Extended VLR (EVLR) parsing for LAS 1.4 files (offset > 4 GB)
- [ ] Implement Coordinate Reference System VLR parsing (GeoTIFF keys VLR, WKT VLR)
- [ ] Add classification reclassification (batch reclassify points by spatial region or rules)
- [ ] Implement cloth simulation filter (CSF) for ground point classification as alternative to slope-based filter
- [ ] Add return-number filtering (first return, last return, single return) to Octree queries
- [ ] Implement intensity normalization (correct for range and scan angle)
- [ ] Add canopy height model (CHM) generation from ground-classified points
- [ ] Support streaming/chunked octree construction for datasets too large for memory
- [ ] Implement tree canopy cover percentage calculation per grid cell

## Low Priority / Future
- [ ] Add integration with oxigdal-proj for on-the-fly CRS reprojection of point clouds
- [ ] Implement point cloud thinning strategies beyond voxel grid (random, nth-point, Poisson disk)
- [ ] Add LAS Extra Bytes VLR support for user-defined per-point attributes
- [ ] Implement cross-section extraction along arbitrary 3D polylines (not just 2D transects)
- [ ] Add point cloud colorization from raster imagery (oxigdal-geotiff integration)
- [ ] Implement digital terrain model (DTM) interpolation from ground points (TIN/IDW)
- [ ] Add support for reading EPT (Entwine Point Tile) format as an alternative to COPC
- [ ] Implement point density heatmap rasterization
- [ ] Add COPC hierarchy page caching for repeated spatial queries on the same file
- [ ] Benchmark and optimize k-NN with a proper priority-queue pruning strategy instead of full collection + sort
