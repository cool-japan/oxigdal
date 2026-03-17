# TODO: oxigdal-mbtiles

## High Priority
- [ ] Implement SQLite-based MBTiles file reader (parse actual .mbtiles SQLite databases to read tile data)
- [ ] Implement SQLite-based MBTiles file writer (write tiles table, metadata table, and optional grids to .mbtiles)
- [ ] Add gzip/zlib tile decompression for compressed PBF vector tiles
- [ ] Implement tile deduplication via content-addressable storage (shared tile blobs with tile_map indirection)
- [ ] Add MBTiles specification v1.3 compliance validation (check required metadata keys, tile table schema)

## Medium Priority
- [ ] Implement TMS-to-XYZ batch tile coordinate conversion for bulk re-addressing
- [ ] Add tile image format detection (sniff PNG/JPEG/WebP/PBF magic bytes from tile data)
- [ ] Implement multi-resolution tile pyramid builder from a single high-resolution source
- [ ] Add tile set diffing: compare two MBTiles archives and produce a delta archive
- [ ] Implement tile set merging: combine tiles from multiple MBTiles files with conflict resolution
- [ ] Add bounding box auto-computation from tile extents for metadata population
- [ ] Implement MBTiles-to-PMTiles conversion (oxigdal-pmtiles integration)
- [ ] Add vector tile (MVT/PBF) decoding for feature-level inspection of vector tiles
- [ ] Implement tile set pruning: remove tiles outside a geographic region or zoom range
- [ ] Add progress reporting callback for long-running tile operations

## Low Priority / Future
- [ ] Implement UTFGrid support (grid table, grid_data table) for interactive tile overlays
- [ ] Add tile compression (gzip, brotli, zstd) on write with configurable level
- [ ] Implement tile set statistics visualization (tile count histogram by zoom, coverage heatmap)
- [ ] Add HTTP range-request tile serving compatible with MBTiles (integration with oxigdal-services)
- [ ] Implement MBTiles export to directory-of-tiles layout (z/x/y.ext file tree)
- [ ] Add tile cache layer wrapping MBTiles reader for repeated access patterns
- [ ] Implement parallel tile generation using rayon (feature-gated)
- [ ] Add MBTiles archive size estimation before writing (predict output size from tile count and mean size)
- [ ] Implement tile re-encoding (convert JPEG tiles to WebP, PNG to AVIF)
