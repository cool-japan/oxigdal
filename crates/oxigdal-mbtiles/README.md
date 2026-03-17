# oxigdal-mbtiles

Pure Rust MBTiles tile archive reader and writer for the
[OxiGDAL](https://github.com/cool-japan/oxigdal) ecosystem. No C/Fortran
dependencies.

## Features

- In-memory MBTiles store (`MBTiles`, `MBTilesMetadata`)
- Tile archive builder with TMS and XYZ scheme support
- Lazy `TileRangeIter` for bbox-to-tile enumeration
- Per-zoom statistics aggregation (`TileStatsAggregator`)
- Geographic coordinate utilities: lon/lat to tile, tile to bbox, resolution at zoom level
- TMS/XYZ coordinate conversion

## Usage

```rust
use oxigdal_mbtiles::{TileCoord, TileFormat, lonlat_to_tile, tile_to_bbox};

// Convert geographic coordinates to tile coordinates
let (tx, ty) = lonlat_to_tile(-73.9857, 40.7484, 14);
println!("Tile: z=14, x={}, y={}", tx, ty);

// Get the bounding box of a tile
let bbox = tile_to_bbox(tx, ty, 14);
println!("Bbox: {:?}", bbox);
```

## Status

- 123 tests passing, 0 failures

## License

See the top-level [OxiGDAL](https://github.com/cool-japan/oxigdal) repository for license details.
