# oxigeo-gpkg

Pure Rust GeoPackage (GPKG) reader and writer for the
[OxiGeo](https://github.com/cool-japan/oxigeo) ecosystem. Includes a
minimal SQLite binary format parser -- no C/FFI dependencies required.

## Features

- SQLite binary format parser (`SqliteReader`, `SqliteHeader`), including WAL-mode overlay reads (`WalReader`, `overlay_wal`)
- GeoPackage schema layer (`GeoPackage`, `GpkgContents`, `GpkgSrs`)
- Vector feature tables with WKB geometry parsing (Point/LineString/Polygon/Multi* plus Z, M, and ZM variants, big/little endian)
- GeoPackage Binary (GPB) header parsing
- GeoPackage **writer** (`GeoPackageBuilder`) — OGC-compliant `.gpkg` creation, no `libsqlite3-sys`/C dependency
- Tile matrix support for raster tiles, incl. a tile-pyramid reader (`TilePyramidReader`)
- Bbox filtering, SQL-like attribute filtering (`FilterExpr`), and GeoJSON round-trip conversion
- R*Tree spatial-index reader (`GpkgRTreeReader`), `gpkg_extensions` / `gpkg_metadata` / `gpkg_data_columns` / Related Tables Extension parsers
- Schema-constraint validation, file-integrity checks (`check_integrity`), and incremental insert/update/delete (`GeoPackageEditor`)
- Feature-gated: CRS reprojection (`reproject`), FlatGeobuf export (`flatgeobuf-export`), MBTiles export (`mbtiles-export`), trigger-based change tracking (`change-tracking`), POSIX file locking (`file-locking`, Unix)

## Usage

### Reading

```rust
use oxigeo_gpkg::{SqliteReader, GeoPackage};

let data: &[u8] = &[/* gpkg file bytes */];
let reader = SqliteReader::new(data).expect("valid SQLite");
let gpkg = GeoPackage::open(&reader).expect("valid GeoPackage");

for table in gpkg.contents() {
    println!("Table: {} (type: {:?})", table.table_name, table.data_type);
}
```

### Writing

```rust
use oxigeo_gpkg::GeoPackageBuilder;

let bytes = GeoPackageBuilder::new(4326) // SRS id (EPSG:4326)
    .add_feature_table("cities", "POINT", vec![(1, -122.4, 37.8), (2, -74.0, 40.7)])
    .build()
    .expect("valid GeoPackage bytes");

std::fs::write("cities.gpkg", bytes).expect("write file");
```

## Status

- 553 tests passing, 0 failures (all-features)

## License

See the top-level [OxiGeo](https://github.com/cool-japan/oxigeo) repository for license details.
