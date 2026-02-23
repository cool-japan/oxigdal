# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2025-01-25

### Added

#### Core Types
- `RasterBuffer` - Type-safe raster data buffer with automatic memory management
- `RasterMetadata` - Complete metadata for raster datasets
- `RasterDataType` - Support for all standard GDAL data types (UInt8-64, Int8-64, Float32/64, CFloat32/64)
- `RasterStatistics` - Pixel statistics computation (min, max, mean, std_dev)
- `BoundingBox` - 2D spatial extent with geometric operations (intersection, union, containment)
- `BoundingBox3D` - 3D spatial extent with elevation support
- `GeoTransform` - Affine transformation for georeferencing with pixel-to-geo conversion
- `NoDataValue` - Type-safe representation of missing data values
- `PixelLayout` - Memory organization (interleaved vs separate bands)
- `ColorInterpretation` - Band color meanings (RGB, grayscale, alpha, etc.)
- `SampleInterpretation` - Pixel value interpretation (unsigned, signed, complex, etc.)

#### Vector Types
- `Geometry` - Comprehensive geometry support (Point, LineString, Polygon, Multi-*)
- `Feature` - Vector features with properties and geometry
- `GeometryType` - Geometry type enumeration with multi-geometry support
- Coordinate types: `Coordinate` (2D), `Coordinate3D` (3D)

#### I/O Traits
- `DataSource` trait for async/sync data source abstraction
- `RasterReader` trait for raster data reading
- `RasterWriter` trait for raster data writing
- `VectorReader` trait for vector data reading
- `VectorWriter` trait for vector data writing

#### Error Handling
- `OxiGdalError` - Main error type with comprehensive variants
- `IoError` - I/O-specific errors (file not found, network, HTTP, etc.)
- `FormatError` - Format-specific errors (invalid magic, corrupt data, etc.)
- `CrsError` - CRS transformation errors
- `CompressionError` - Compression/decompression errors
- Proper error conversion from `std::io::Error`

#### Features
- `std` (default) - Standard library support
- `alloc` - Allocation support without full std (embedded/WASM)
- `arrow` - Apache Arrow integration for zero-copy buffers
- `async` - Async I/O traits with tokio integration

#### Performance
- SIMD-optimized buffer operations (when target supports it)
- Zero-copy buffer conversions with Apache Arrow
- Efficient memory layout for raster data

#### Documentation
- Comprehensive rustdoc for all public APIs
- Examples for common operations
- Cross-references between related types
- Module-level documentation

### Implementation Notes

This is the foundational crate for the OxiGDAL ecosystem. All drivers and algorithms depend on the types and traits defined here.

#### Design Decisions
- **Pure Rust**: No C/C++/Fortran dependencies for maximum portability
- **No unwrap()**: All errors handled explicitly for production safety
- **no_std support**: Can be used in embedded environments
- **Arrow integration**: Optional zero-copy interop with Apache Arrow
- **Type safety**: Compile-time guarantees for data type handling

#### Known Limitations
- Complex number support is basic (CFloat32/64 as paired reals)
- No built-in CRS database (delegated to oxigdal-proj)
- Vector operations are minimal (geometry construction only)

### Future Roadmap

#### 0.2.0 (Planned)
- Enhanced SIMD operations for more architectures
- Streaming buffer support for large datasets
- Lazy evaluation for buffer transformations
- Color table support
- Metadata domain support (XML, JSON)

#### 0.3.0 (Planned)
- Virtual datasets (VRT) core support
- Band masks and validity buffers
- Pyramid/overview support in core types
- Histogram computation

### Dependencies

All dependencies use latest stable versions:
- `thiserror` 2.x - Error handling
- `serde` 1.x - Serialization support
- `byteorder` 1.x - Endian-aware I/O
- `bytes` 1.x (optional) - Zero-copy buffer management
- `arrow-*` 57.x (optional) - Apache Arrow integration
- `async-trait` 0.1.x (optional) - Async trait support
- `futures` 0.3.x (optional) - Async utilities

### Testing

- 100+ unit tests
- Property-based tests with proptest
- No unwrap() policy enforced via clippy
- All public APIs tested

### License

Apache-2.0

Copyright © 2025 COOLJAPAN OU (Team Kitasan)
