# oxigdal-gpkg

Pure Rust GeoPackage (GPKG) reader and writer for the
[OxiGDAL](https://github.com/cool-japan/oxigdal) ecosystem. Includes a
minimal SQLite binary format parser -- no C/FFI dependencies required.

## Features

- SQLite binary format parser (`SqliteReader`, `SqliteHeader`)
- GeoPackage schema layer (`GeoPackage`, `GpkgContents`, `GpkgSrs`)
- Vector feature tables with WKB geometry parsing (8 geometry types, big/little endian)
- GeoPackage Binary (GPB) header parsing
- Tile matrix support for raster tiles
- Bbox filtering and GeoJSON output

## Usage

```rust
use oxigdal_gpkg::{SqliteReader, GeoPackage};

let data: &[u8] = &[/* gpkg file bytes */];
let reader = SqliteReader::new(data).expect("valid SQLite");
let gpkg = GeoPackage::open(&reader).expect("valid GeoPackage");

for table in gpkg.contents() {
    println!("Table: {} (type: {:?})", table.table_name, table.data_type);
}
```

## Status

- 156 tests passing, 0 failures

## License

See the top-level [OxiGDAL](https://github.com/cool-japan/oxigdal) repository for license details.
