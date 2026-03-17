# TODO: oxigdal-node

## High Priority
- [ ] Implement N-API async worker threads for non-blocking raster I/O
- [ ] Add Node.js Buffer zero-copy integration for large raster data transfer
- [ ] Implement streaming read API using Node.js Readable streams
- [ ] Add proper error propagation with JavaScript Error subclasses
- [ ] Implement COG reader with HTTP range requests via Node.js fetch/http

## Medium Priority
- [ ] Add GeoPackage and MBTiles read/write bindings
- [ ] Implement vector tile (MVT) generation bindings
- [ ] Add coordinate transformation bindings (EPSG code support)
- [ ] Implement raster reprojection (warp) with progress callback
- [ ] Add TypeScript declaration generation from napi-rs annotations
- [ ] Implement prebuilt binary distribution via npm (prebuild-install pattern)
- [ ] Add GDAL-compatible CLI tool wrapping Node.js bindings
- [ ] Implement batch processing API with worker_threads parallelism

## Low Priority / Future
- [ ] Add Express/Fastify middleware for tile server endpoint
- [ ] Implement MapLibre GL JS data source plugin
- [ ] Add gRPC server bindings for microservice deployment
- [ ] Implement Deno/Bun compatibility layer
- [ ] Add PM2 cluster mode support with shared memory tile cache
- [ ] Implement STAC API client bindings
- [ ] Add sharp-compatible image processing API surface
- [ ] Implement GeoArrow/GeoParquet bindings for columnar data exchange
