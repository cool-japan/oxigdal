# TODO: oxigdal-core

## High Priority
- [ ] Add `RasterBuffer` typed accessors (get_f32, get_f64, get_u16, etc.) with bounds checking
- [ ] Implement `Dataset` trait as a unified interface for raster and vector drivers
- [ ] Add `BandIterator` for lazy per-band iteration over raster data
- [ ] Implement `GeoTransform::inverse()` for pixel-to-coordinate mapping
- [ ] Add `PixelLayout::BandInterleaved` (BIP) and `PixelLayout::LineInterleaved` (BIL) support
- [ ] Implement `From<RasterBuffer>` for Arrow `RecordBatch` (arrow feature)
- [ ] Add `no_std` + `alloc` support for `RasterBuffer` (currently std-only internals)
- [ ] Implement `SpatialReference` type wrapping CRS info for core-level reprojection awareness

## Medium Priority
- [ ] Add `RasterWindow` type for sub-region reads without full-band allocation
- [ ] Implement `VectorDataset` trait (open/iterate features/spatial filter)
- [ ] Add `Feature` and `FieldValue` types to core for driver-agnostic vector access
- [ ] Implement SIMD-accelerated buffer operations in `simd_buffer.rs` (currently scaffolding)
- [ ] Add `TileIndex` and `TileIterator` types for standardized tiled access
- [ ] Implement memory-mapped I/O path in `io::DataSource` for large files
- [ ] Add `Statistics` struct (min/max/mean/stddev/histogram) to `RasterMetadata`
- [ ] Implement `AsyncDataSource` trait for cloud-native async reading

## Low Priority / Future
- [ ] Add `Mask` type for nodata/validity bitmask operations
- [ ] Implement arena allocator integration for zero-alloc tile processing pipelines
- [ ] Add hugepage and NUMA-aware allocation policies for HPC workloads
- [ ] Implement `Display` and `Debug` formatting for all public types
- [ ] Add `serde` feature for serialization of metadata types
- [ ] Implement `ColorTable` type for palette-indexed raster support
- [ ] Add benchmark suite comparing buffer operations against raw slice ops
