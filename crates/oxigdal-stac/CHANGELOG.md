# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2025-01-25

### Added

#### Core Models
- **STAC Item** - Geospatial asset metadata
  - ID, geometry, bbox, properties
  - Asset management
  - Extension support
- **STAC Collection** - Dataset collections
  - Spatial and temporal extents
  - Provider information
  - License metadata
  - Summaries
- **STAC Catalog** - Hierarchical organization
  - Links and relationships
  - Child catalogs and items

#### STAC API Client
- Async HTTP client for STAC API
- Search endpoint support
  - Bbox filtering
  - Datetime filtering
  - Collection filtering
  - Property queries
  - Pagination
- Collections endpoint
- Items endpoint
- Asset download with streaming

#### Extensions
- **EO Extension** - Earth observation metadata
  - Cloud cover
  - Band information
  - Center wavelength
  - Common names
- **SAR Extension** - Synthetic aperture radar
  - Frequency band
  - Polarization
  - Product type
- **Projection Extension** - CRS metadata
  - EPSG codes
  - WKT2 definitions
  - Transform matrices
  - Shape information
- **View Extension** - Viewing geometry
  - Sun azimuth/elevation
  - Off-nadir angle
- **Scientific Extension** - Citations
  - DOI
  - Publications

#### Builder API
- **ItemBuilder** - Fluent API for creating items
- **CollectionBuilder** - Fluent API for collections
- **CatalogBuilder** - Fluent API for catalogs

#### Validation
- STAC specification compliance checking
- JSON Schema validation
- Extension validation
- Link validation

### Implementation Details

#### Design Decisions
- **Pure Rust**: No dependencies on external tools
- **Async-first**: Built for cloud-native workflows
- **Type safety**: Strong typing for all STAC models
- **Extensible**: Easy to add custom extensions

#### Known Limitations
- Limited extension support (core extensions only)
- No STAC API write operations yet
- No OGC API Features support
- Label extension not implemented

### Performance

- Item parsing: ~100μs
- Collection parsing: ~200μs
- API search: ~500ms (network dependent)
- Asset download: Streamed (minimal memory usage)

### Future Roadmap

#### 0.2.0 (Planned)
- STAC API write operations (POST, PUT, DELETE)
- Transaction extension
- OGC API Features support
- Label extension
- More extension support

#### 0.3.0 (Planned)
- STAC browser functionality
- Local catalog management
- Static catalog generation
- Thumbnail generation

### Dependencies

- `oxigdal-core` 0.1.x - Core types
- `serde` 1.x - Serialization
- `serde_json` 1.x - JSON parsing
- `reqwest` 0.12.x (optional) - HTTP client
- `chrono` 0.4.x - Date/time handling
- `url` 2.x - URL parsing
- `geojson` 0.24.x - GeoJSON support

### Testing

- 80+ unit tests
- STAC specification compliance tests
- API integration tests (mock server)
- Extension validation tests
- Real-world catalog compatibility tests

### License

Apache-2.0

Copyright © 2025 COOLJAPAN OU (Team Kitasan)
