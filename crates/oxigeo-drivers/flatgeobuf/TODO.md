# TODO: oxigeo-drivers/flatgeobuf

> **Purpose:** FlatGeobuf driver for OxiGeo - Pure Rust GDAL reimplementation
> **Status (2026-07-28):** 3,323 Rust LoC (src) - 132 tests all-features / 127 default-features - 0 source-code stubs
> **Roadmap:** v0.1.7 - v0.2.0 (current slice) - v1.0.0

## High Priority (next slice - verified gaps)

- [x] Add async HTTP range-request reader for cloud-hosted FlatGeobuf
  - **Verified done:** `src/http.rs:213` now defines `pub struct AsyncHttpReader { url, client: reqwest::Client, header, geometry_codec, index: Option<PackedRTree>, features_offset }` (real non-blocking `reqwest::Client`, not `::blocking::Client`), gated `#[cfg(all(feature = "http", feature = "async"))]`. `AsyncHttpReader::new` (`:225`) builds the client and calls `initialize()` (`:242`), which range-fetches the first 1 MiB, verifies the magic bytes, parses the size-prefixed header FlatBuffer, and — when `header.has_index` — parses the packed R-tree, matching the sync reader's approach but on the async client. `query_bbox` (`:325`) is async.
  - **Not independently re-verified:** whether R-tree-leaf-driven feature range requests are issued concurrently via `FuturesUnordered` as originally sketched, vs. sequentially — re-check `query_bbox`'s body if concurrency matters for a performance claim.

- [ ] Add CRS reprojection during read/write via `oxigeo-proj`
  - **Verified gap:** `src/header.rs` defines `CrsInfo` (re-exported at `src/lib.rs:26`). No reprojection helper. `rg -n "reproject|transform.*crs|to_crs|from_crs" -g '*.rs' src/` returns no matches.
  - **Goal:** `FlatGeobufReader::reproject_to(target_crs)` and `FlatGeobufWriter::from_crs(source_crs).to_crs(target_crs)` so users can convert geometries on the fly.
  - **Design:** Use `oxigeo-proj::Transformer` (workspace dep). Per-feature coordinate iterator -> transform -> rebuild geometry. Spec: WKT2 (ISO 19162) CRS strings; `oxigeo-proj` already handles EPSG codes.
  - **Files:** (new) `src/reproject.rs`, `Cargo.toml` (add `oxigeo-proj.workspace = true` under a `reproject` feature gate to keep base crate light)
  - **Tests:** (proposed) `test_reproject_wgs84_to_web_mercator`, `test_reproject_polygon_with_z_preserved`, `test_reproject_preserves_attribute_columns`
  - **Risk:** Reprojection invalidates the existing Hilbert spatial index; document that reprojected output needs `rebuild_index()`.
  - **Prerequisites:** None.

- [ ] Streaming write for large feature collections (avoid in-memory accumulation)
  - **Verified gap:** `src/writer.rs:36-39` - `FlatGeobufWriter` holds `features: Vec<Vec<u8>>` and `bboxes: Vec<BoundingBox>` accumulators. `add_feature` (`src/writer.rs:43-66`) appends to these vectors; no temporary-file streaming mode. For >1M features this consumes O(features) memory.
  - **Goal:** Write 100M-row datasets in bounded memory.
  - **Design:** Two-pass with disk spill: (1) Pass 1: write each feature blob to a tmpfile, keep only `(bbox, tmpfile_offset, blob_len)` in RAM (~32 bytes/feature vs current ~hundreds). (2) Sort by Hilbert key (already in `src/index.rs:385 hilbert_index_for_bbox`). (3) Pass 2: emit header + R-tree + features in sorted order by seeking the tmpfile. Use `std::env::temp_dir()` per workspace policy.
  - **Files:** `src/writer.rs`
  - **Tests:** (proposed) `test_streaming_writer_1m_features_bounded_memory`, `test_streaming_writer_sorted_output_equiv_to_in_memory`, `test_streaming_writer_cleans_up_tmpfile`
  - **Risk:** Tmpfile cleanup on panic; use `tempfile::NamedTempFile`.
  - **Prerequisites:** None.

## Medium Priority (planned - design sketched)

- [ ] Support for additional FlatGeobuf geometry types: CircularString, CompoundCurve, CurvePolygon, MultiCurve, MultiSurface
  - **Goal:** Decode all geometry types per FlatGeobuf schema, not just Point/Line/Polygon families.
  - **Files:** `src/geometry.rs`
  - **Why deferred:** Rare in practice; needs core `Geometry` enum extension first.

- [ ] Attribute (column) projection during read
  - **Goal:** Skip non-selected columns without decoding to FieldValue.
  - **Files:** `src/reader.rs`
  - **Why deferred:** Performance optimization; not correctness.

- [ ] Feature-count and bbox extraction without full scan
  - **Goal:** Both are already in the header; expose typed accessors.
  - **Files:** `src/header.rs`, `src/reader.rs`
  - **Why deferred:** Likely already partially exposed; needs API review.

- [ ] On-the-fly geometry simplification during read (Douglas-Peucker)
  - **Goal:** Reduce vertex count by tolerance during streaming read.
  - **Files:** (new) `src/simplify.rs`
  - **Why deferred:** Useful but secondary; can be done by caller after read.

- [ ] FlatGeobuf-to-GeoJSON / GeoJSON-to-FlatGeobuf conversion helpers
  - **Goal:** One-shot conversion functions.
  - **Files:** (new) `src/convert.rs`
  - **Why deferred:** Cross-crate; better as `oxigeo` umbrella CLI subcommand.

- [ ] File validation and integrity checking
  - **Goal:** Verify magic, header consistency, R-tree integrity, feature count match.
  - **Files:** (new) `src/validate.rs`
  - **Why deferred:** Polish item; not core.

- [ ] Round-trip foreign-member preservation (currently structs exist; needs test suite)
  - **Goal:** Ensure unknown properties survive read-modify-write.
  - **Files:** `src/header.rs`
  - **Why deferred:** Test suite addition.

## Low Priority / Future (speculative - concise)

- [ ] FlatGeobuf index rebuilding tool (for files where index was stripped)
- [ ] FlatGeobuf merge (combine multiple files, preserving spatial index)
- [ ] Parallel feature encoding using rayon
- [ ] FlatGeobuf diff (detect changes between two files)
- [ ] Memory-mapped reading for local file performance
- [ ] FlatGeobuf to PMTiles / MBTiles conversion pipeline
- [ ] Configurable geometry encoding precision (vertex coordinate quantization)
- [ ] FlatGeobuf statistics without loading features (count by type, bbox, attribute histograms)

## Cross-crate dependencies
- **Blocks:** None directly.
- **Blocked by:** `oxigeo-proj` (for reprojection feature only).

## Recently completed (kept verbatim from previous TODO.md)
- [x] FlatGeobuf writer with Packed Hilbert R-tree index (planned 2026-04-18)
  - **Goal:** Writer produces valid `.fgb` files: 8-byte magic, header, optional Packed Hilbert R-tree index, feature FlatBuffers in bbox-sorted order.
  - **Design:** Two-pass: collect bboxes - sort on Hilbert curve over global bbox - write header+index+features.
  - **Files:** writer.rs (enhanced), index.rs (Hilbert sort fix)
  - **Tests:** 6 tests covering magic, empty file, Hilbert sorted features, R-tree node size, roundtrip with index seek, Z flag

_(Note from 2026-05-16 audit: R-tree spatial index querying is also implemented - `PackedRTree::search` at `src/index.rs:256` and `FlatGeobufReader::features_in_bbox` at `src/reader.rs:209` exist. The previous TODO line "Implement R-tree spatial index querying for bbox-filtered reads" was a stale entry and has been moved to Recently completed below.)_

- [x] R-tree spatial index querying for bbox-filtered reads — `PackedRTree::search` (`src/index.rs:256`) plus `FlatGeobufReader::features_in_bbox` (`src/reader.rs:209`); verified by audit on 2026-05-16.

- [x] Feature-level random access via spatial index offsets — `FlatGeobufReader::seek_feature` (`src/reader.rs:232`); verified by audit on 2026-05-16.

---
*Last audited: 2026-07-28 (status line refreshed: 107→132/127 tests, LoC 3,417→3,323 (src-only via tokei, previous figure included tests), date bumped; async HTTP range-request reader confirmed real and flipped to done; CRS reprojection and streaming/spill writer re-checked and confirmed still absent — left open)*
