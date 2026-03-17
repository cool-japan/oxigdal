# TODO: oxigdal-3d

## High Priority
- [ ] Implement LAS 1.4 reader with all point record formats (0-10)
- [ ] Add LAZ decompression support (Pure Rust, no laszip C dependency)
- [ ] Implement Delaunay triangulation for TIN generation from point clouds
- [ ] Add glTF 2.0 / GLB binary export with embedded textures
- [ ] Implement 3D Tiles (Cesium) tileset.json generation with content hierarchy

## Medium Priority
- [ ] Add COPC (Cloud Optimized Point Cloud) reader with octree traversal
- [ ] Implement DEM-to-mesh conversion with configurable LOD levels
- [ ] Add OBJ export with MTL material file generation
- [ ] Implement ground classification using cloth simulation filter (CSF)
- [ ] Add building footprint extraction from classified point clouds
- [ ] Implement vegetation height model (CHM) from normalized point clouds
- [ ] Add progressive mesh simplification (edge collapse with quadric error)
- [ ] Implement texture mapping from orthophoto onto terrain mesh

## Low Priority / Future
- [ ] Add EPT (Entwine Point Tiles) reader for streaming point clouds
- [ ] Implement implicit surface reconstruction (Poisson) for dense meshes
- [ ] Add CityGML / CityJSON export for urban 3D models
- [ ] Implement viewshed analysis on 3D mesh surfaces
- [ ] Add IFC (Industry Foundation Classes) basic geometry import
- [ ] Implement 3D Tiles Next with glTF structural metadata
- [ ] Add point cloud colorization from aerial imagery
- [ ] Implement draco mesh compression for bandwidth-efficient delivery
