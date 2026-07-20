# oxigeo-geojson-stream

Pure Rust streaming GeoJSON reader and writer for the
[OxiGeo](https://github.com/cool-japan/oxigeo) ecosystem.

## Features

- Full GeoJSON geometry support (Point, LineString, Polygon, Multi-variants, GeometryCollection, with Z coordinates)
- Streaming feature reader for memory-efficient processing of large files
- Compact and pretty-print writer modes
- Built-in validator with configurable severity levels
- Feature filtering by property values (8 filter operators)
- CRS support via `GeoJsonCrs`

## Usage

```rust
use oxigeo_geojson_stream::{GeoJsonParser, GeoJsonWriter};

let json = br#"{"type":"FeatureCollection","features":[]}"#;
let parser = GeoJsonParser::new();
let doc = parser.parse(json).expect("valid GeoJSON");

let writer = GeoJsonWriter::compact();
println!("{}", writer.write_document(&doc));
```

### Filtering Features

```rust
use oxigeo_geojson_stream::{FeatureFilter, PropertyFilter, FilterOp};

let filter = FeatureFilter::new()
    .add_property(PropertyFilter::new("population", FilterOp::GreaterThan, 1_000_000.into()));
```

## Status

- 148 tests passing, 0 failures

## License

See the top-level [OxiGeo](https://github.com/cool-japan/oxigeo) repository for license details.
