# oxigeo-stac

Pure Rust STAC (SpatioTemporal Asset Catalog) support for cloud-native geospatial data.

[![Crates.io](https://img.shields.io/crates/v/oxigeo-stac)](https://crates.io/crates/oxigeo-stac)
[![Documentation](https://docs.rs/oxigeo-stac/badge.svg)](https://docs.rs/oxigeo-stac)
[![License](https://img.shields.io/crates/l/oxigeo-stac)](LICENSE)

## Overview

`oxigeo-stac` provides comprehensive support for STAC (SpatioTemporal Asset Catalog), enabling efficient discovery and access to cloud-optimized geospatial datasets.

### Features

- ✅ STAC v1.0.0 specification compliance
- ✅ Item, Collection, and Catalog models
- ✅ STAC API client (search, browse, query)
- ✅ Asset management and download
- ✅ Extension support (EO, SAR, Projection, etc.)
- ✅ Async API client
- ✅ Builder pattern for creating STAC objects

## Installation

```toml
[dependencies]
oxigeo-stac = "0.2.2"

# With async HTTP client:
oxigeo-stac = { version = "0.2.2", features = ["async"] }
```

## Quick Start

### Reading STAC Items

```rust
use oxigeo_stac::Item;

let json = std::fs::read_to_string("item.json")?;
let item: Item = serde_json::from_str(&json)?;

println!("ID: {}", item.id);
println!("Geometry: {:?}", item.geometry);
println!("Assets: {}", item.assets.len());
```

### Creating STAC Items

```rust
use oxigeo_stac::builder::ItemBuilder;
use oxigeo_core::types::BoundingBox;

let bbox = BoundingBox::new(-122.5, 37.5, -122.0, 38.0)?;

let item = ItemBuilder::new("my-item")
    .bbox(bbox)
    .datetime_utc("2025-01-25T00:00:00Z")
    .collection("my-collection")
    .add_asset("cog", "https://example.com/data.tif", "image/tiff")
    .build()?;

let json = serde_json::to_string_pretty(&item)?;
println!("{}", json);
```

### Searching STAC API

```rust
use oxigeo_stac::client::StacClient;
use oxigeo_core::types::BoundingBox;

let client = StacClient::new("https://earth-search.aws.element84.com/v1")?;

// Search for Sentinel-2 data
let bbox = BoundingBox::new(-122.5, 37.5, -122.0, 38.0)?;
let items = client
    .search()
    .collections(&["sentinel-2-l2a"])
    .bbox(&bbox)
    .datetime("2025-01-01/2025-01-31")
    .max_items(10)
    .execute()
    .await?;

for item in items {
    println!("Found: {} at {}", item.id, item.properties.datetime);
}
```

### Downloading Assets

```rust
use oxigeo_stac::Asset;

let asset = item.assets.get("visual")
    .ok_or(StacError::AssetNotFound("visual".into()))?;
if let Some(url) = &asset.href {
    let data = client.download_asset(url).await?;
    std::fs::write("output.tif", data)?;
}
```

## STAC Extensions

### Earth Observation (EO)

```rust
use oxigeo_stac::extensions::eo::{EoExtension, Band};

let eo = EoExtension {
    cloud_cover: Some(15.5),
    bands: vec![
        Band {
            name: Some("B04".to_string()),
            common_name: Some("red".to_string()),
            center_wavelength: Some(0.665),
            ..Default::default()
        }
    ],
};

item.add_extension(eo);
```

### Projection

```rust
use oxigeo_stac::extensions::proj::ProjectionExtension;

let proj = ProjectionExtension {
    epsg: Some(32610),
    wkt2: None,
    projjson: None,
    geometry: None,
    bbox: None,
    centroid: None,
    shape: Some(vec![10980, 10980]),
    transform: None,
};

item.add_extension(proj);
```

## STAC Collections

```rust
use oxigeo_stac::builder::CollectionBuilder;

let collection = CollectionBuilder::new("sentinel-2")
    .title("Sentinel-2 L2A")
    .description("Sentinel-2 Level-2A processed data")
    .license("proprietary")
    .extent_spatial(vec![-180.0, -90.0, 180.0, 90.0])
    .extent_temporal("2015-06-27T00:00:00Z", None)
    .add_provider("ESA", vec!["producer", "licensor"])
    .build()?;
```

## STAC Catalogs

```rust
use oxigeo_stac::Catalog;

let catalog = Catalog {
    id: "my-catalog".to_string(),
    description: "My STAC Catalog".to_string(),
    links: vec![],
    ..Default::default()
};

// Add items to catalog
catalog.add_child_link("./items/item1.json", "item");
```

## Validation

```rust
use oxigeo_stac::validation::StacValidator;

let validator = StacValidator::new();
let result = validator.validate_item(&item)?;

if !result.is_valid {
    for error in result.errors {
        eprintln!("Validation error: {}", error);
    }
}
```

## Performance

- Item parsing: ~100μs
- Collection parsing: ~200μs
- API search: ~500ms (network dependent)
- Asset download: Streamed (minimal memory)

## Features

- **`std`** (default): Standard library support
- **`async`**: Async HTTP client for STAC API (`StacClient`, `SearchBuilder`, `Paginator`); activates the `reqwest` dependency
- **`reqwest`**: Backwards-compat alias for `async` (kept for downstream crates that request `features = ["reqwest"]` directly)

## Supported Extensions

- ✅ Earth Observation (EO)
- ✅ Synthetic Aperture Radar (SAR)
- ✅ Projection
- ✅ View Geometry
- ✅ Scientific Citation
- ✅ Electro-Optical (EO)
- ⏳ Label (v0.2.0)

## COOLJAPAN Policies

- ✅ **Pure Rust** - No C dependencies
- ✅ **No unwrap()** - All errors handled
- ✅ **STAC compliant** - Follows v1.0.0 spec
- ✅ **Well tested** - Comprehensive test suite

## License

Licensed under Apache-2.0.

Copyright © 2025 COOLJAPAN OU (Team Kitasan)

## See Also

- [STAC Specification](https://stacspec.org/)
- [STAC API](https://github.com/radiantearth/stac-api-spec)
- [API Documentation](https://docs.rs/oxigeo-stac)
