# oxigeo-pmtiles

Pure Rust PMTiles v3 reader and writer for the
[OxiGeo](https://github.com/cool-japan/oxigeo) ecosystem. No C/Fortran
dependencies.

## Features

- 127-byte fixed header parser (`PmTilesHeader`)
- Varint-encoded directory entry decoder (`DirectoryEntry`, `decode_directory`)
- Hilbert curve tile ID computation (`zxy_to_tile_id`, `tile_id_to_zxy`)
- High-level reader (`PmTilesReader`) and builder (`PmTilesBuilder`)
- Compression type and tile format detection (`detect_tile_format`)
- PMTiles v2 backward-compatibility reader (`PmTilesV2Reader`)
- Archive validation (`validate_archive`), tile-set diffing (`diff_archives`), and gap-removing compaction (`compact_archive`)
- Bounding-box sub-region extraction (`extract_subregion`) and gzip/brotli/zstd re-compression (`transcode_archive`, `compression` feature)
- Auto bounds/center/zoom calculation and clustered vs. leaf-optimized directory layout selection
- Structured metadata JSON access (`PmTilesMetadata`)
- HTTP range-request reader with ETag LRU caching (`HttpPmTilesReader`, `http-range` feature)
- Async reader over `AsyncRead + AsyncSeek` (`AsyncPmTilesReader`, `async` feature)
- Cloud storage (S3/GCS/Azure Blob) range-request reader (`CloudPmTilesReader`, `cloud-storage` feature)
- PMTiles → MBTiles export (`MbTilesExporter`, `mbtiles` feature)
- Parallel tile encoding via rayon (`parallel` feature)

## Usage

```rust
use oxigeo_pmtiles::{PmTilesReader, zxy_to_tile_id, tile_id_to_zxy};

// Convert z/x/y to a Hilbert curve tile ID
let tile_id = zxy_to_tile_id(5, 10, 15).expect("valid z/x/y");
let (z, x, y) = tile_id_to_zxy(tile_id).expect("valid tile ID");
assert_eq!((z, x, y), (5, 10, 15));

// Read a PMTiles archive
let data: Vec<u8> = vec![/* pmtiles file bytes */];
let reader = PmTilesReader::from_bytes(data).expect("valid PMTiles");
let header = &reader.header;
println!("Tile type: {:?}, {} tile entries", header.tile_type, header.tile_entries);
```

## Status

- 508 tests passing, 0 failures with `--all-features`; 397 with default features

## License

See the top-level [OxiGeo](https://github.com/cool-japan/oxigeo) repository for license details.
