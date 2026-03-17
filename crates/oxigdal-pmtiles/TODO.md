# TODO: oxigdal-pmtiles

## High Priority
- [ ] Implement tile data retrieval by (z, x, y) in PmTilesReader (currently only exposes raw directory; need binary search on directory entries + data section extraction)
- [ ] Add gzip/brotli/zstd decompression for compressed root directories and tile data
- [ ] Implement leaf directory support (multi-level directory tree for archives with > ~21k tiles)
- [ ] Add HTTP range-request reader: fetch only the header + directory + requested tiles over the network without downloading the entire file
- [ ] Implement tile content deduplication in PmTilesBuilder (currently tracks hashes for stats but writes every tile's data)
- [ ] Add PMTiles v3 archive validation (check header consistency, directory integrity, offset/length bounds)

## Medium Priority
- [ ] Implement streaming/async tile reader (read tiles from `AsyncRead` / `Read` source without loading entire file)
- [ ] Add metadata JSON parsing and structured access (parse the metadata section into typed fields)
- [ ] Implement PmTilesReader tile enumeration (iterate over all tiles by decoding directory)
- [ ] Add tile type auto-detection from tile data content (sniff PNG/JPEG/WebP/MVT magic bytes)
- [ ] Implement run-length encoding optimization in PmTilesBuilder for consecutive tiles with identical data
- [ ] Add PMTiles-to-MBTiles conversion (oxigdal-mbtiles integration)
- [ ] Implement clustered vs. non-clustered directory layout selection based on tile ordering
- [ ] Add bounding box and center point auto-calculation from tile extents
- [ ] Implement archive compaction (rewrite archive to remove gaps from deleted/replaced tiles)

## Low Priority / Future
- [ ] Add S3/GCS/Azure Blob Storage range-request adapter for cloud-hosted PMTiles
- [ ] Implement ETag-based caching for HTTP range-request reader
- [ ] Add tile set diffing between two PMTiles archives
- [ ] Implement Hilbert curve visualization/debugging utilities
- [ ] Add integration with oxigdal-services for HTTP tile serving directly from PMTiles
- [ ] Implement parallel tile encoding in PmTilesBuilder using rayon (feature-gated)
- [ ] Add PMTiles v2 backward compatibility reader
- [ ] Implement directory compression (gzip/brotli) in writer for smaller archive headers
- [ ] Add tile re-compression (transcode between gzip/brotli/zstd) during archive construction
- [ ] Implement extract sub-region: create a new PMTiles archive containing only tiles within a bbox
