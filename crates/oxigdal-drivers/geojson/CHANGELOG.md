# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2025-01-25

### Added

#### Reading Support
- RFC 7946 compliant GeoJSON parser
- All geometry types (Point, LineString, Polygon, Multi*, GeometryCollection)
- Feature and FeatureCollection parsing
- Streaming reader for large files
- CRS support (including EPSG codes)
- Bounding box parsing
- Property deserialization (arbitrary JSON)
- Foreign members support

#### Writing Support
- GeoJSON writer for all geometry types
- Pretty-printing and compact output
- FeatureCollection generation
- Bounding box computation
- CRS metadata
- Property serialization

#### Validation
- Geometry validation
- CRS validation
- Bounding box validation
- RFC 7946 compliance checking
- Self-intersection detection
- Ring orientation validation

#### Performance
- Zero-copy parsing via serde_json
- Streaming API for memory efficiency
- Lazy geometry validation
- Efficient bounding box computation

### Implementation Details

#### Design Decisions
- **Pure Rust**: No dependencies on C libraries
- **RFC compliant**: Strict adherence to RFC 7946
- **Streaming**: Support for files larger than RAM
- **Type safety**: Strong typing for geometry types

#### Known Limitations
- No support for legacy CRS (RFC 7946 removed CRS)
- Coordinate precision follows JSON number limits
- No topology support (TopoJSON separate format)

### Future Roadmap

#### 0.2.0 (Planned)
- TopoJSON support
- Geometry simplification during write
- Spatial indexing integration
- Parallel feature processing

### Dependencies

- `oxigdal-core` 0.1.x - Core types
- `serde` 1.x - Serialization
- `serde_json` 1.x - JSON parsing

### Testing

- 100+ unit tests
- RFC 7946 compliance tests
- Round-trip tests
- Real-world file compatibility tests

### License

Apache-2.0

Copyright © 2025 COOLJAPAN OU (Team Kitasan)
