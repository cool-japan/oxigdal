# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.6] - Unreleased

### Added

- **WebP compression codec** (TIFF tag 50001) under the `webp` feature flag, backed by the pure Rust `image-webp` crate. The decoder reads both VP8 (lossy) and VP8L (lossless) WebP payloads; the encoder produces lossless VP8L, which preserves raster values bit-exact and is the right default for COG bands. Lossy VP8 encoding requires the C `libwebp` library and is therefore excluded by COOLJAPAN's Pure Rust Policy.
  - New public API: `compression::compress_webp_with_params(data, width, height, color_type)` and re-exported `compression::WebpColorType`.
  - Dispatch in `compression::decompress` now recognises `Compression::WebP`; `compression::compress` directs callers to the `compress_webp_with_params` helper because WebP requires explicit dimensions.
  - 3 new regression tests (`test_issue_6_webp_compression`, `test_issue_6_webp_dispatch_recognises_tag_50001`, `test_issue_6_webp_encoder_validates_input_length`).
  - Closes [#6](https://github.com/cool-japan/oxigdal/issues/6).

## [0.1.0] - 2025-01-25

### Added

#### Reading Support
- **GeoTIFF Reader** - Complete TIFF/BigTIFF reading
  - Baseline TIFF 6.0 support
  - BigTIFF support for files >4GB
  - Tiled and stripped layouts
  - Little-endian and big-endian formats
  - All sample formats (unsigned, signed, float, complex)
  - All data types (UInt8-UInt64, Float32/64, CFloat32/64)
- **COG Reader** - Cloud Optimized GeoTIFF optimized reading
  - HTTP range request optimization
  - Lazy IFD loading
  - Overview-aware tile access
  - Efficient spatial queries
  - Multi-level pyramid support
- **Compression Support**
  - DEFLATE/zlib (feature: `deflate`)
  - LZW (feature: `lzw`)
  - ZSTD (feature: `zstd`)
  - PackBits (built-in)
  - Uncompressed (built-in)
- **GeoKey Support**
  - Complete GeoKey directory parsing
  - EPSG code extraction
  - WKT CRS parsing
  - Coordinate transformation parameters
  - Datum and ellipsoid information

#### Writing Support
- **GeoTIFF Writer** - Create standard GeoTIFF files
  - Tiled and stripped writing
  - All compression methods
  - Metadata tags (DateTime, Software, etc.)
  - GeoKey directory generation
  - Multi-band support
  - Planar and interleaved configurations
- **COG Writer** - Generate Cloud Optimized GeoTIFFs
  - Automatic overview generation
  - Tiled layout optimization
  - IFD offset optimization
  - Compression per overview level
  - Validation against COG spec
  - Internal mask support

#### Metadata
- TIFF tag reading and writing
- GeoTIFF GeoKey support
- Color interpretation (RGB, grayscale, palette)
- NoData value handling
- Statistics caching
- Metadata domains

#### Performance
- Zero-copy tile reading where possible
- Efficient IFD caching
- Chunked decompression
- Memory-mapped I/O (optional)
- Async I/O support (feature: `async`)

#### Documentation
- Comprehensive rustdoc for all public APIs
- Examples for reading and writing
- COG creation guidelines
- Performance tuning guide

### Implementation Details

#### Design Decisions
- **Pure Rust**: No libtiff/libgeotiff dependencies
- **Zero-copy**: Minimize memory allocations
- **Streaming**: Support for large files >RAM
- **Cloud-native**: HTTP range request optimization
- **Spec compliant**: Follows TIFF 6.0 and GeoTIFF 1.0/1.1 specs

#### COG Compliance
Follows OGC COG specification:
- Tiled layout (recommended: 256x256 or 512x512)
- Internal overviews
- IFDs ordered from smallest to largest
- Header optimization for cloud access
- Optional internal masks

#### Known Limitations
- JPEG compression not yet implemented (feature planned)
- WebP compression not yet implemented (feature planned)
- YCbCr color space conversion is basic
- Palette/indexed color support is limited
- Some exotic TIFF features not supported (e.g., SubIFDs)

### Performance Benchmarks

Reading 4096x4096 GeoTIFF (Float32, DEFLATE):
- Full read: 340ms
- Single tile (256x256): 8ms
- Header-only: 2ms

Writing 4096x4096 COG with 3 overviews:
- Uncompressed: 180ms
- DEFLATE: 520ms
- ZSTD: 450ms

HTTP range request efficiency:
- First tile: 3 requests (header + IFD + tile)
- Subsequent tiles: 1 request per tile
- Bandwidth: ~1MB for header, <100KB per tile

### Future Roadmap

#### 0.2.0 (Planned)
- JPEG compression support
- WebP compression support
- Advanced color space conversions
- Thumbnail generation
- EXIF metadata support
- PDF-style georeferencing

#### 0.3.0 (Planned)
- Streaming tile writer
- Incremental COG updates
- Multi-threaded compression
- Virtual overviews
- Sparse tile support

### Dependencies

- `oxigdal-core` 0.1.x - Core types and traits
- `byteorder` 1.x - Endian-aware I/O
- `bytes` 1.x - Zero-copy buffer management
- `flate2` 1.x (optional) - DEFLATE compression
- `weezl` 0.1.x (optional) - LZW compression
- `zstd` 0.13.x (optional) - ZSTD compression
- `tokio` 1.x (optional) - Async I/O

### Testing

- 150+ unit tests
- Round-trip tests (write then read)
- COG validation tests
- Compression round-trip tests
- Property-based tests for edge cases
- Real-world GeoTIFF compatibility tests
- No unwrap() policy enforced

### License

Apache-2.0

Copyright © 2025 COOLJAPAN OU (Team Kitasan)
