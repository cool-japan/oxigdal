# TODO: oxigeo-copc

> **Purpose:** Pure Rust COPC (Cloud Optimized Point Cloud) reader for OxiGeo — LAS/LAZ format with spatial index.
> **Status (2026-07-28):** ~5,270 Rust LoC · 303 tests · 0 in-source `TODO:` markers (gaps are absences in the module tree).
> **Roadmap:** v0.1.7 → v0.2.0 (current slice) → v1.0.0

## High Priority (next slice — verified gaps)

- [x] Pure-Rust LAZ chunk decompression for COPC point data (PARTIAL — Slice 24 = PF0/PF1; PF6/7/8 remain open)
  - **Verified gap:** `src/copc_reader.rs:140-149` reads `chunk_data` directly into `point_format::deserialize_points`; no LZ77/arithmetic decoder is invoked. COPC requires every hierarchy chunk to be LAZ-compressed, so today's reader fails on every real-world `.copc.laz` file.
  - **Goal:** Decode LASzip-compressed chunks (point formats 6-8 only — the COPC 1.0 mandate) and feed the resulting raw records into `deserialize_points` unchanged.
  - **Design:** New `laz` module with (a) chunk-table parser (per-chunk byte size + uncompressed point count, in the EVLR area), (b) integer-coding arithmetic decoder (Said & Pearlman / Martin), (c) per-field LASzip predictors for X/Y/Z deltas, GPS time, classification, RGB, NIR. Decode one chunk into a `Vec<u8>` of fixed-size records, then call the existing path. Memory-bounded: stream record-by-record where possible. Reference: LASzip Specification 3.4 (`pdal/laszip` repo).
  - **Files:** `(new) src/laz/mod.rs`, `(new) src/laz/arithmetic.rs`, `(new) src/laz/predictors.rs`, `(new) src/laz/chunk_table.rs`, modify `src/copc_reader.rs` to detect compressed chunks via the LASzip VLR.
  - **Tests:** `(proposed)` test_laz_decode_format6_chunk, test_laz_decode_format7_with_rgb, test_laz_chunk_table_roundtrip, test_laz_decode_against_pdal_reference_buffer
  - **Risk:** Floating-point predictor drift relative to the LASzip C++ reference — pin against `pdal --writer las.laz` round-trip vectors. Performance: arithmetic coder is the hot path; budget ≥ 50 Mpts/s on a single core.
  - **Prerequisites:** None.
  - **Done (partial):** 2026-05-20 (Slice 24). Closed the foundational layer: Martin Range Coder + Item Compressor v1 + LAS Point Format 0 + Point Format 1 (PF0 + GPS time) decoders, plus the LASzip VLR detection + chunk-table parser + `copc_reader.rs` routing branch at line 140. Encoder gated behind `laz-encoder` feature for round-trip tests; production surface is decode-only. `decompress_chunk` returns `CopcError::UnsupportedLazFormat { format_id }` for PF ≥ 2 (no panic).
  - **Slice 25 follow-up:** PF6/PF7/PF8 (LASzip Item Compressor v3 — layered context architecture, ~1500 LoC). The current `UnsupportedLazFormat` path means real-world `.copc.laz` files using the COPC 1.0 PF6/7/8 mandate are NOT yet decodable; only research/legacy PF0/PF1 archives work end-to-end today.
  - **Tests:** 18 integration tests in `crates/oxigeo-copc/tests/laz_test.rs` + 16 inline unit tests in `crates/oxigeo-copc/src/laz/*.rs` modules. Coverage: arithmetic round-trip (bits + 2-symbol + integer compressor); chunk table parse + truncation; VLR detection + items field parse; XYZ/intensity/classification/user_data predictors; PF0/PF1 decode round-trip; PF6 typed-error path; end-to-end CopcReader routing.

- [x] LAS point data record formats 9 and 10 (LAS 1.4 waveform variants)
  - Done: 2026-05-31 (Slice 29). Tests: 14 new (waveform_test) + 264 existing = 278 total.
  - New `WaveformPacket` struct (ASPRS LAS 1.4 R15 Tables 17-18, 29 bytes), `Point3D.waveform: Option<WaveformPacket>`, `min_record_size` 9→59/10→67, WKB parse at byte 30 (PF9) / 38 (PF10), bounds-checked. `WaveformPacket` re-exported from `lib.rs`.

- [x] Extended VLR (EVLR) chain parsing
  - **Verified gap:** `crates/oxigeo-copc/src/lib.rs` exposes no EVLR module; `rg "EVLR|extended_vlr|extended_byte_count"` returns 0 matches across `src/`.
  - **Goal:** Parse the EVLR chain that LAS 1.4 places after the point data (header offsets `start_of_first_evlr` + `number_of_evlrs`). Required for files where the LASzip VLR or COPC hierarchy exceeds 64 KiB.
  - **Design:** New struct `ExtendedVlr { reserved: u16, user_id: [u8;16], record_id: u16, record_length: u64, description: [u8;32], data: Vec<u8> }` (60-byte header vs. classic VLR's 54). Walk N entries from header bytes 235-242. Extend `vlr_chain::find_copc_hierarchy_vlr` to fall through to EVLRs when not present in classic chain.
  - **Files:** `(new) src/extended_vlr.rs`, modify `src/vlr_chain.rs` (chain-walking fallback), `src/las_header.rs` (expose `start_of_first_evlr`, `number_of_evlrs`).
  - **Tests:** `(proposed)` test_evlr_header_size_60, test_evlr_chain_walk_two_records, test_evlr_used_for_oversize_hierarchy
  - **Risk:** Old LAS 1.0-1.3 headers lack the EVLR fields — guard on `version_minor >= 4`.
  - **Prerequisites:** None.
  - **Done:** 2026-05-22 (Slice 25). New `src/extended_vlr.rs`: `ExtendedVlr { reserved, user_id[16], record_id, record_length: u64, description[32], data }` + 60-byte header + `user_id_str`/`description_str` (NUL-trim, lossy UTF-8); bounds-checked `parse_evlr`/`parse_evlr_chain(bytes, start_of_first_evlr, number_of_evlrs)` (truncation → `CopcError::InvalidFormat`, never panics); `version_supports_evlr(&LasVersion)` LAS-1.4 guard. `las_header.rs` +2 public fields `start_of_first_evlr: u64` (bytes 235-242) + `number_of_evlrs: u32` (243-246) with V14 guard + accessors. `vlr_chain.rs` +`find_copc_hierarchy_in_evlrs` fallback. `copc_reader.rs` +1 private field `extended_vlrs` + parse in `from_bytes` (`.unwrap_or_default()` so malformed EVLR degrades to empty) + `extended_vlrs()`/`crs()` accessors. `lib.rs` adds 2 mod + re-exports.
  - **Tests:** 14 in `crates/oxigeo-copc/tests/evlr_crs_test.rs` + 6 inline unit tests in `extended_vlr.rs`. Coverage: 60-byte header, chain walk N=2, zero-count empty, truncated-no-panic error, user_id/description NUL trim, version_minor<4 guard.

- [ ] COPC writer: serialize Octree → LAS 1.4 + COPC info VLR + hierarchy
  - **Verified gap:** `src/lib.rs` lists no `writer` or `builder` module; `octree::Octree` has spatial-index logic but no encoder. Round-trip use cases (clip-and-export, decimate-and-save) cannot be served.
  - **Goal:** Given an `Octree<Point3D>` and target format-id, emit a valid COPC 1.0 `.copc.laz` file: LAS header → COPC info VLR (record_id 1) → LASzip VLR → hierarchy pages → point data chunks.
  - **Design:** Two-pass: (a) walk octree depth-first, encode each node's points to a LAZ chunk via the `laz` module, record `(offset, size, count)`; (b) build hierarchy pages bottom-up so pointer offsets resolve, seek-back to patch the header's `start_of_first_evlr`. Use `HashMap<VoxelKey, ChunkRef>` as the intermediate index.
  - **Files:** `(new) src/writer/mod.rs`, `(new) src/writer/hierarchy_writer.rs`, `(new) src/writer/copc_info_writer.rs`.
  - **Tests:** `(proposed)` test_writer_roundtrip_format6, test_writer_hierarchy_pages_match_reader, test_writer_octree_with_leaf_chunks, test_writer_copc_info_vlr_size_160_bytes
  - **Risk:** Hierarchy-page byte-offsets are forward references that require seek-and-patch; doing this in one pass over a `Vec<u8>` works, but streaming to disk requires a temp file.
  - **Prerequisites:** LAZ chunk encoder (sibling of decoder item 1).

## Medium Priority (planned — design sketched)

- [x] CRS VLR parsing (GeoTIFF keys VLR and WKT VLR record IDs 34735/34737/2112)
  - **Goal:** Surface the file's CRS as a structured object so callers can reproject without sniffing raw VLR bytes.
  - **Files:** `(new) src/crs_vlr.rs`, modify `src/copc_reader.rs` to expose `fn crs(&self) -> Option<CrsInfo>`.
  - **Why deferred:** Needs WKT parsing — would pull in `oxigeo-proj` work that has its own roadmap.
  - **Done:** 2026-05-22 (Slice 25). Scoped to **structural** VLR parsing (no semantic WKT→CRS conversion — that remains future work; intentionally does NOT pull in `oxigeo-proj`). New `src/crs_vlr.rs`: `GeoKeyEntry { key_id, tiff_tag_location, count, value_offset }`, `GeoKeyDirectory { key_directory_version, key_revision, minor_revision, keys }`, `CrsInfo { geotiff_keys, geo_doubles, geo_ascii, wkt }` + `is_empty()`; record-id constants 34735/34736/34737/2112; bounds-checked `parse_geo_key_directory` (4×u16 header + N×4×u16 entries); `extract_crs_info(&[Vlr], &[ExtendedVlr]) -> CrsInfo` scanning both classic + EVLR lists (classic VLR wins on duplicate). `copc_reader.rs` exposes `pub fn crs(&self) -> CrsInfo`. **Slice 26 follow-up:** semantic WKT→CRS conversion via `oxigeo-proj`.
  - **Tests:** 14 integration tests in `crates/oxigeo-copc/tests/evlr_crs_test.rs` (covering both EVLR + CRS) + 9 inline unit tests in `crs_vlr.rs`. CRS coverage: WKT record 2112, GeoAscii 34737, GeoKeyDirectory 34735 + truncation error, all-none when absent, classic-VLR precedence over EVLR.

- [ ] Classification-based filter pushdown to hierarchy queries
  - **Goal:** Skip whole octree nodes when a classification filter excludes every point in their bbox — saves IO for "ground only" or "buildings only" extracts.
  - **Files:** `src/copc_reader.rs`, `src/hierarchy.rs`.
  - **Why deferred:** Requires per-chunk min/max classification cached during read; small but new ABI surface.

- [ ] Return-number filter on `query_points_in_bbox` (first, last, single)
  - **Goal:** Forestry workflows want only first returns; current API forces full decode + post-filter.
  - **Files:** `src/copc_reader.rs`.
  - **Why deferred:** Awaits the filter-pushdown machinery above.

- [ ] Cloth Simulation Filter (CSF) for ground classification
  - **Goal:** Alternative to slope-based filter in `profile::GroundFilter`; better on dense forest.
  - **Files:** `(new) src/profile/csf.rs`.
  - **Why deferred:** Larger algorithm — Zhang et al. 2016, ~600 LoC.

- [ ] Canopy height model raster generation from ground-classified points
  - **Goal:** Output `Array2<f32>` aligned to a target grid.
  - **Files:** `(new) src/raster/chm.rs`.
  - **Why deferred:** Couples to `oxigeo-terrain` for IDW/TIN interpolation.

- [ ] Intensity normalisation for range and scan angle
  - **Goal:** Per-point `intensity_norm = intensity * (range/r_ref)^2 / cos(scan_angle)`.
  - **Files:** `src/point.rs`, `src/copc_reader.rs`.
  - **Why deferred:** Needs scanner range — not always present.

- [ ] LAS Extra Bytes VLR for user-defined per-point attributes
  - **Goal:** Decode VLR record_id 4 and expose extras alongside `Point3D`.
  - **Files:** `(new) src/extra_bytes.rs`, modify `src/point_format.rs`.
  - **Why deferred:** Wide design surface (untyped bytes → typed fields).

- [ ] Streaming/chunked octree construction for datasets too large for memory
  - **Goal:** Build the octree from a `Read` source without materialising every point.
  - **Files:** `src/octree.rs`.
  - **Why deferred:** Requires the writer first for a meaningful round-trip test.

## Low Priority / Future (speculative — one-liners only)

- [ ] Integration with `oxigeo-proj` for on-the-fly point-cloud reprojection
- [ ] Thinning strategies beyond voxel-grid (random, nth-point, Poisson disk)
- [ ] EPT (Entwine Point Tile) format reader as an alternative input
- [ ] Tree canopy cover percentage per grid cell
- [ ] 3D cross-section extraction along arbitrary polylines
- [ ] Point cloud colorization from `oxigeo-geotiff` rasters
- [ ] DTM interpolation from ground points (TIN/IDW)
- [ ] k-NN with priority-queue pruning (current: full collect + sort)
- [ ] Hierarchy page LRU cache for repeated spatial queries
- [ ] Point density heatmap rasterisation
- [ ] Classification reclassification by spatial region or attribute rule

## Cross-crate dependencies
- **Blocks:** `oxigeo-terrain` (LiDAR-derived DEMs), `oxigeo-services` (point-cloud tile endpoints)
- **Blocked by:** None — internal LAZ decoder unblocks everything

## Recently completed (kept verbatim)
- [x] Implement actual COPC file reader (completed 2026-04-19, part of C1)
- [x] Implement COPC hierarchy page traversal (completed 2026-04-19, part of C1)
- [x] Add point record binary deserialization (completed 2026-04-19, part of C1)

---
*Last audited: 2026-07-28 (status line refreshed: test count 139→303, LoC ~4,132→~5,270; checkbox contents re-verified against source, no changes — writer/CSF/canopy-height/intensity-norm/extra-bytes/streaming-octree/PF6-8 LAZ all confirmed still open)*
