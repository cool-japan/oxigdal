# oxigdal-pmtiles

Pure Rust PMTiles v3 reader and writer for the
[OxiGDAL](https://github.com/cool-japan/oxigdal) ecosystem. No C/Fortran
dependencies.

## Features

- 127-byte fixed header parser (`PmTilesHeader`)
- Varint-encoded directory entry decoder (`DirectoryEntry`, `decode_directory`)
- Hilbert curve tile ID computation (`zxy_to_tile_id`, `tile_id_to_zxy`)
- High-level reader (`PmTilesReader`) and builder (`PmTilesBuilder`)
- Compression type and tile format detection

## Usage

```rust
use oxigdal_pmtiles::{PmTilesReader, zxy_to_tile_id, tile_id_to_zxy};

// Convert z/x/y to a Hilbert curve tile ID
let tile_id = zxy_to_tile_id(5, 10, 15);
let (z, x, y) = tile_id_to_zxy(tile_id);
assert_eq!((z, x, y), (5, 10, 15));

// Read a PMTiles archive
let data: &[u8] = &[/* pmtiles file bytes */];
let reader = PmTilesReader::new(data).expect("valid PMTiles");
let header = reader.header();
println!("Tile type: {:?}, {} entries", header.tile_type, reader.num_entries());
```

## Status

- 147 tests passing, 0 failures

## License

See the top-level [OxiGDAL](https://github.com/cool-japan/oxigdal) repository for license details.
