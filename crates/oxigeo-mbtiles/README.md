# oxigeo-mbtiles

Pure Rust MBTiles tile archive reader and writer for the
[OxiGeo](https://github.com/cool-japan/oxigeo) ecosystem. No C/Fortran
dependencies.

## Features

- In-memory MBTiles store (`MBTiles`, `MBTilesMetadata`)
- Real, on-disk `.mbtiles` file I/O (feature `sqlite`) via the Pure-Rust
  [`oxisql-sqlite-compat`](https://crates.io/crates/oxisql-sqlite-compat) engine (no C/FFI,
  no `libsqlite3`) -- read archives produced by `tippecanoe`/`mb-util`/QGIS, and write a
  spec-conformant MBTiles 1.3 archive (`metadata` + `tiles` tables, unique tile index)
- MBTiles 1.3 metadata compliance validator (`MBTilesMetadata::validate`)
- Tile archive builder with TMS and XYZ scheme support
- Lazy `TileRangeIter` for bbox-to-tile enumeration
- Per-zoom statistics aggregation (`TileStatsAggregator`)
- Geographic coordinate utilities: lon/lat to tile, tile to bbox, resolution at zoom level
- TMS/XYZ coordinate conversion

## Usage

```rust
use oxigeo_mbtiles::{TileCoord, TileFormat, lonlat_to_tile, tile_to_bbox};

// Convert geographic coordinates to tile coordinates
let (tx, ty) = lonlat_to_tile(-73.9857, 40.7484, 14);
println!("Tile: z=14, x={}, y={}", tx, ty);

// Get the bounding box of a tile
let bbox = tile_to_bbox(tx, ty, 14);
println!("Bbox: {:?}", bbox);
```

## Status

- 157 tests passing, 0 failures

## License

See the top-level [OxiGeo](https://github.com/cool-japan/oxigeo) repository for license details.
