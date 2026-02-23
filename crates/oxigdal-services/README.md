# OxiGDAL Services

OGC-compliant web service implementations for geospatial data access and processing.

## Features

- **WFS (Web Feature Service) 2.0/3.0**: Vector data access with filtering and transactions
- **WCS (Web Coverage Service) 2.0**: Raster data access with subsetting and format conversion
- **WPS (Web Processing Service) 2.0**: Geospatial processing with built-in algorithms
- **CSW (Catalog Service for the Web) 2.0.2**: Metadata catalog search and retrieval

## Standards Compliance

This crate follows official OGC standards:

- OGC WFS 2.0.0 (ISO 19142:2010)
- OGC WFS 3.0 (OGC API - Features Part 1: Core)
- OGC WCS 2.0.1 Core
- OGC WPS 2.0.0
- OGC CSW 2.0.2 (ISO 19115/19119)

## Quick Start

```rust
use oxigdal_services::{wfs, wcs, wps, csw};
use axum::{Router, routing::get};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create WFS service
    let wfs_info = wfs::ServiceInfo {
        title: "My WFS Service".to_string(),
        abstract_text: Some("Vector data service".to_string()),
        provider: "COOLJAPAN OU".to_string(),
        service_url: "http://localhost:8080/wfs".to_string(),
        versions: vec!["2.0.0".to_string()],
    };
    let wfs_state = wfs::WfsState::new(wfs_info);

    // Build router
    let app = Router::new()
        .route("/wfs", get(wfs::handle_wfs_request).with_state(wfs_state));

    // Serve
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

## Supported Operations

### WFS Operations

- **GetCapabilities**: Service metadata
- **DescribeFeatureType**: Feature schema information
- **GetFeature**: Feature retrieval with filtering, pagination, and CRS transformation
- **Transaction**: Feature insert, update, delete (when enabled)

### WCS Operations

- **GetCapabilities**: Service metadata
- **DescribeCoverage**: Coverage schema and structure
- **GetCoverage**: Raster data retrieval with subsetting and format conversion

### WPS Operations

- **GetCapabilities**: Service and process metadata
- **DescribeProcess**: Process input/output descriptions
- **Execute**: Process execution (synchronous and asynchronous)

Built-in processes:
- Buffer: Create buffer around geometry
- Clip: Clip geometry by boundary
- Union: Union multiple geometries

### CSW Operations

- **GetCapabilities**: Service metadata
- **GetRecords**: Search metadata records
- **GetRecordById**: Retrieve specific metadata record

## Architecture

The crate is organized into four main modules:

- `wfs/`: Web Feature Service implementation
- `wcs/`: Web Coverage Service implementation
- `wps/`: Web Processing Service implementation
- `csw/`: Catalog Service for the Web implementation

Each module provides:
- Service state management
- Request parameter parsing
- OGC-compliant XML/JSON response generation
- Error handling with proper exception reports

## COOLJAPAN Policies

- **Pure Rust**: No C/C++ dependencies
- **No unwrap()**: All error paths handled properly
- **Workspace**: Uses workspace dependencies
- **Files < 2000 lines**: Modular organization (largest file: 504 LOC)

## Performance

- Async request handling with Tokio
- Efficient XML generation with quick-xml
- DashMap for concurrent service registries
- LRU caching support

## License

Apache-2.0

## Author

COOLJAPAN OU (Team Kitasan)
