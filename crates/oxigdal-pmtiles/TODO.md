# TODO: oxigdal-pmtiles

## High Priority
- [x] Implement tile data retrieval by (z, x, y) (completed 2026-04-19, part of P1)
- [x] Add gzip/brotli/zstd decompression via OxiARC (completed 2026-04-19, part of P1; `compression` feature, no C deps)
- [x] Implement leaf directory support (root + leaf, per PMTiles v3 spec; reader follows `run_length == 0` entries)
- [ ] Add HTTP range-request reader: fetch only the header + directory + requested tiles over the network without downloading the entire file
- [x] Implement tile content deduplication in PmTilesBuilder (completed 2026-04-19, part of P1; FNV-1a content hash, shared offsets)
- [ ] Add PMTiles v3 archive validation (check header consistency, directory integrity, offset/length bounds)

## Medium Priority
- [ ] Implement streaming/async tile reader (read tiles from `AsyncRead` / `Read` source without loading entire file)
- [ ] Add metadata JSON parsing and structured access (parse the metadata section into typed fields)
- [ ] Implement PmTilesReader tile enumeration (iterate over all tiles by decoding directory)
- [ ] Add tile type auto-detection from tile data content (sniff PNG/JPEG/WebP/MVT magic bytes)
- [x] Implement run-length encoding optimization in PmTilesBuilder for consecutive tiles with identical data (completed 2026-04-18)
- [ ] Add PMTiles-to-MBTiles conversion (oxigdal-mbtiles integration)
- [~] PMTiles v3 archive writer with root + leaf directory hierarchy (planned 2026-04-18)
  - **Goal:** Writer produces valid PMTiles v3 archives from (tile_id, tile_data) pairs. Root-only for < ~16k tiles, root+leaves for larger archives.
  - **Design:** Two-pass: collect (tile_id, content_hash, data) → sort by tile_id → run-length compress directory → decide root/leaf split → write header with seek-and-patch for offsets.
  - **Files:** Enhanced writer.rs (run-length + leaf support), writer/leaves.rs (leaf split logic)
  - **Tests:** 6 tests: magic, small archive, large archive with leaves, dedup, run-length, roundtrip
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
