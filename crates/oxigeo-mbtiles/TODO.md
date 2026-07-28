# TODO: oxigeo-mbtiles

> **Purpose:** Pure Rust MBTiles tile archive reader/writer for OxiGeo — SQLite-based tile pyramid support.
> **Status (2026-07-28):** 1,430 Rust LoC (tokei, `src/`) · 157 tests (all-features and default-features), 0 failed · both the reader and writer are now real, Pure-Rust `oxisql-sqlite-compat`-backed SQLite I/O (see below)
> **Roadmap:** v0.1.7 → v0.2.0 → v0.2.1 (current) → v1.0.0

## High Priority (next slice — verified gaps)

- [x] Real SQLite-backed MBTiles file reader
  - **Verified gap:** `src/mbtiles.rs:82-91` — `"In-memory MBTiles tile store. In production use this would delegate to a SQLite backend; here it provides a pure-Rust, dependency-free store suitable for testing."` and field `tiles: HashMap<TileCoord, Vec<u8>>`. The crate cannot read any actual `.mbtiles` file produced by `tippecanoe`, `mb-util`, `mbview`, or QGIS — every consumer must construct the in-memory store by hand.
  - **Goal:** Open a `.mbtiles` file path and read its `metadata` and `tiles` tables (MBTiles 1.3 spec, <https://github.com/mapbox/mbtiles-spec/blob/master/1.3/spec.md>) using a Pure-Rust SQLite parser.
  - **Design:** Reuse `oxigeo-gpkg::sqlite_reader::SqliteReader` (already a Pure-Rust B-tree walker) via a workspace dependency. New `MBTiles::from_bytes(data: Vec<u8>) -> Result<Self>` and `from_path(path: &Path)`: parse SQLite header → `scan_table_by_name("metadata")` → build `MBTilesMetadata` from key/value rows → `scan_table_by_name("tiles")` lazily into the `HashMap`. Decode `(zoom_level, tile_column, tile_row, tile_data)` columns by serial-type. Preserve existing in-memory API for tests by keeping `MBTiles::new(metadata)` as a constructor.
  - **Files:** `(new) src/reader.rs`, modify `src/mbtiles.rs` (add real constructors), `Cargo.toml` (add `oxigeo-gpkg = { workspace = true }` for the SQLite parser), `src/lib.rs` (re-export).
  - **Tests:** `(proposed)` test_reader_opens_tippecanoe_sample_mbtiles, test_reader_parses_metadata_bounds, test_reader_tile_count_matches_select_count, test_reader_handles_jpeg_png_webp_pbf_formats, test_reader_skips_grids_table_if_absent
  - **Risk:** Cross-crate dependency on `oxigeo-gpkg` for SQLite parsing — acceptable because both crates already depend on the SQLite format. Alternative: move `SqliteReader` to a shared `oxigeo-sqlite` crate (track separately).
  - **Prerequisites:** None — the gpkg crate already exposes the SQLite reader.
  - **Done:** 2026-05-20 (Slice 24), implementation superseded 2026-07-28. Originally shipped on `rusqlite = { workspace = true, features = ["bundled"] }` (a COOLJAPAN Pure-Rust-policy violation, since `rusqlite`'s `bundled` feature vendors and compiles C SQLite). **Re-verified 2026-07-28: `src/reader.rs` and `Cargo.toml` no longer reference `rusqlite` at all** — the `sqlite` feature now depends on `oxisql-core` + `oxisql-sqlite-compat` (Pure Rust, no C/FFI), and `src/reader.rs` uses `oxisql_core::{Connection, Value}` + `oxisql_sqlite_compat::SqliteConnection` directly. The policy violation noted here is resolved; this note is kept for history. Feature gate: `sqlite`. `MBTilesMetadata.extra` field renamed to `extras` to align with new `from_map_strict` + accessor.
  - **Tests:** 16 in `crates/oxigeo-mbtiles/tests/reader_test.rs` (canonical metadata keys; bounds/center CSV parsing; tile round-trip + missing; zoom_levels distinct/sorted; list_tiles ordered; into_mbtiles preservation; malformed-bounds typed error; in-memory open round-trip via tempfile rendezvous; missing-tiles/metadata-table guards).

- [x] Real SQLite-backed MBTiles file writer
  - **Verified gap:** `src/writer.rs:441-445` `MBTilesWriter { metadata, tiles: HashMap<TileCoord, Vec<u8>>, format }` — produces `MBTilesData` (an immutable in-memory snapshot, line 391), not a serialised `.mbtiles` byte stream. Line 1 doc-comment: `"MBTiles writer and in-memory tile archive builder."` — accurate but the "writer" half is unimplemented.
  - **Goal:** Add `MBTilesWriter::build_sqlite() -> Result<Vec<u8>>` and `write_to_path(path: &Path) -> Result<()>` producing a valid `.mbtiles` SQLite archive that QGIS / `mbview` / `tile-stitch` can open.
  - **Design:** Reuse the `sqlite_writer` planned for `oxigeo-gpkg` (see that crate's high-priority writer item) via re-export. Required tables per MBTiles 1.3 spec: `metadata(name TEXT, value TEXT)` and `tiles(zoom_level INT, tile_column INT, tile_row INT, tile_data BLOB)`. Required indexes: `tile_index ON tiles (zoom_level, tile_column, tile_row)`. Required metadata keys: `name, format, bounds, center, minzoom, maxzoom, attribution, description, type, version, json`.
  - **Files:** `(new) src/writer_sqlite.rs`, modify `src/writer.rs` (add `build_sqlite` method), `Cargo.toml` (depend on the gpkg writer).
  - **Tests:** `(proposed)` test_writer_emits_metadata_required_keys, test_writer_round_trip_via_reader, test_writer_unique_index_on_tile_coords, test_writer_handles_empty_archive_valid_sqlite, test_writer_tms_y_coordinate_preserved
  - **Risk:** Blocked on the gpkg writer landing first; can scaffold the API and gate behind a `sqlite` feature in the interim.
  - **Prerequisites:** `oxigeo-gpkg` writer (sibling crate, sibling slice).
  - **Done:** verified fixed as of 2026-07-28 (root `CHANGELOG.md` [0.2.1] Added: "`oxigeo-mbtiles` / `oxigeo-gpkg` / `oxigeo-pmtiles`: a real SQLite-backed MBTiles writer (now genuinely persists to `.mbtiles`)"). Implemented independently as `(new) src/sqlite_writer.rs` (290 LoC, not `writer_sqlite.rs` as sketched here) rather than a re-export from `oxigeo-gpkg`: `impl MBTilesData { pub fn write_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), MbTilesError> }` (and a consuming variant), using `oxisql_core`/`oxisql_sqlite_compat` (same Pure-Rust engine as the reader, gated behind the same `sqlite` feature) to emit exactly the `metadata`/`tiles` tables and `tile_index` unique index the spec requires, all inside one transaction so a failed write never leaves a half-written archive on disk. API name differs from the `build_sqlite`/`write_to_path` sketched above but the goal is met.

- [ ] Tile decompression for compressed vector tiles (PBF + gzip)
  - **Verified gap:** `src/writer.rs:1-2` and `lib.rs:1-5` say nothing about decompression; `rg "decompress|gunzip"` across `src/` returns no matches. MBTiles 1.3 (§Metadata) mandates that PBF tiles MUST be gzip-compressed in the BLOB column — current code returns the raw gzipped bytes, forcing every caller to dig out an `oxiarc-flate`-equivalent decoder themselves.
  - **Goal:** Add `MBTiles::get_tile_decoded(coord) -> Option<Vec<u8>>` that auto-detects gzip magic (`0x1f 0x8b`) and decompresses via `oxiarc-flate`. Per COOLJAPAN Compression Policy — no `flate2`, no `miniz_oxide` direct use.
  - **Design:** Sniff first 2 bytes. If `0x1f 0x8b`, run gzip decode via `oxiarc-flate::gunzip`. Otherwise pass through. Keep the raw API (`get_tile`) untouched. Mark vector layers via `MBTilesMetadata::format == TileFormat::Pbf`.
  - **Files:** modify `src/mbtiles.rs` (new method), `Cargo.toml` (add `oxiarc-flate = { workspace = true, optional = true }` behind `compression` feature).
  - **Tests:** `(proposed)` test_decode_pbf_strips_gzip, test_decode_png_passthrough, test_decode_raw_pbf_no_magic_returns_as_is, test_decode_truncated_gzip_returns_error
  - **Risk:** None significant — `oxiarc-flate` already supports gzip per ecosystem memory.
  - **Prerequisites:** `oxiarc-flate` workspace dep must be available.

- [x] MBTiles 1.3 metadata compliance validator
  - **Verified gap:** `src/mbtiles.rs::MBTilesMetadata::from_map` (lines 41-74) silently accepts any subset of keys; no enforcement of the spec's required keys (`name, format, minzoom, maxzoom, bounds`) and no warning on unknown values.
  - **Goal:** `MBTilesMetadata::validate(&self, scheme: TileScheme) -> Vec<ValidationIssue>` reporting missing-required, invalid-bounds, zoom-out-of-range, format-unknown, type-not-in-{overlay, baselayer}.
  - **Design:** Mirror the `oxigeo-geojson::GeoJsonValidator` pattern: `enum ValidationIssue { MissingKey(String), InvalidBounds(String), …}` with `IssueSeverity { Error, Warning, Info }`. Validate against MBTiles 1.3 §Specification.
  - **Files:** `(new) src/validation.rs`, modify `src/mbtiles.rs` (add `validate` method), `src/lib.rs` (re-export).
  - **Tests:** `(proposed)` test_validator_flags_missing_format, test_validator_accepts_valid_min_set, test_validator_flags_bounds_outside_minus180_180, test_validator_flags_minzoom_greater_than_maxzoom
  - **Risk:** Definition of "required" varies between MBTiles 1.0/1.1/1.2/1.3 — version against 1.3 explicitly.
  - **Prerequisites:** None.
  - **Done:** 2026-05-22 (Slice 25). New `src/validation.rs`: `IssueSeverity { Error, Warning, Info }`, 7-variant `ValidationIssue` (MissingRequiredKey, InvalidBounds, InvalidCenter, ZoomOutOfRange, MinZoomGreaterThanMaxZoom, UnknownFormat, InvalidType) with `severity()` + `Display`, free fn `validate_metadata(&MBTilesMetadata, TileScheme)`. `MBTilesMetadata::validate(&self, TileScheme)` impl block appended additively to `mbtiles.rs` (no struct-literal use — passes the `extras`-field constraint). Reused the pre-existing `TileScheme` enum from `writer.rs`. `from_map` and existing fields untouched. Validator and tests are feature-independent (do NOT require `sqlite`).
  - **Tests:** 14 in `crates/oxigeo-mbtiles/tests/validation_test.rs` (accepts valid min set; missing format / missing name flagged; bounds outside ±180 / lat outside ±90; minzoom > maxzoom; zoom > 30; unknown format warning; invalid type; center outside bounds; valid full metadata no errors; missing-recommended not error; Display non-empty; scheme-invariance). All built via `from_map`, no struct literals.

## Medium Priority (planned — design sketched)

- [ ] Tile deduplication via content-addressable storage
  - **Goal:** Replace duplicate BLOBs with a shared `tile_map` (id → tile_data) and `tiles` view, shrinking transparent / uniform tilesets 10×.
  - **Files:** `src/writer.rs` (build-time dedup), `(new) src/cas.rs`.
  - **Why deferred:** Writer must land first.

- [ ] TMS ↔ XYZ coordinate batch converter
  - **Goal:** Bulk-flip every tile's y-coordinate across an entire archive when migrating between conventions.
  - **Files:** `src/tile_coords.rs` (already has `flip_y` — add batch API).
  - **Why deferred:** Trivial once writer exists.

- [ ] Tile format auto-detection from BLOB magic bytes
  - **Goal:** Inspect first 4 bytes to decide PNG (`89 50 4e 47`), JPEG (`ff d8 ff`), WebP (`52 49 46 46`), or PBF (gzip `1f 8b`).
  - **Files:** `(new) src/format_sniff.rs`.
  - **Why deferred:** Needed mainly when metadata's `format` key disagrees with actual bytes.

- [ ] Multi-resolution pyramid builder from a single source raster
  - **Goal:** Given a high-res raster, generate all zoom levels by downsampling.
  - **Files:** `(new) src/pyramid_builder.rs`.
  - **Why deferred:** Couples to `oxigeo-geotiff` for raster IO.

- [ ] Tile-set diff between two archives → delta archive
  - **Goal:** Bandwidth-efficient updates for vector-tile workflows.
  - **Files:** `(new) src/diff.rs`.
  - **Why deferred:** Awaits dedup + writer.

- [ ] Tile-set merge with conflict resolution policies
  - **Goal:** Combine two archives; pick newer / preferred / first.
  - **Files:** `(new) src/merge.rs`.
  - **Why deferred:** Awaits writer.

- [ ] Auto-compute `bounds` and `center` from tile extents
  - **Goal:** When metadata is missing geographic bounds, derive them from non-empty tile coverage.
  - **Files:** `src/bbox_util.rs` (already has the math — wire it up).
  - **Why deferred:** Low complexity; bundle with validator slice.

- [ ] MBTiles → PMTiles conversion via `oxigeo-pmtiles`
  - **Goal:** Cross-format migration with deterministic tile ordering.
  - **Files:** `(new) src/pmtiles_export.rs`.
  - **Why deferred:** Pairs naturally with the PMTiles ↔ MBTiles bidirectional work in `oxigeo-pmtiles`.

- [ ] MVT/PBF decoder for feature-level inspection of vector tiles
  - **Goal:** Decode a single vector tile's layers/features for QC.
  - **Files:** `(new) src/mvt.rs`.
  - **Why deferred:** Standalone codec — sized for its own crate later.

- [ ] Spatial pruning: remove tiles outside bbox / zoom range
  - **Goal:** Trim an archive in place to a region of interest.
  - **Files:** `(new) src/prune.rs`.
  - **Why deferred:** Builds on the writer + diff slices.

## Low Priority / Future (speculative — one-liners only)

- [ ] UTFGrid support (`grid` and `grid_data` tables)
- [ ] Tile recompression on write (gzip / brotli / zstd via `oxiarc-*`, with level)
- [ ] Tile-count histogram + coverage heatmap statistics
- [ ] HTTP range-request tile serving via `oxigeo-services`
- [ ] Export to directory-of-tiles (`z/x/y.ext`) layout
- [ ] LRU cache wrapper for repeated tile access
- [ ] Parallel tile generation via rayon (feature-gated)
- [ ] Pre-write archive-size estimation from tile count + mean size
- [ ] Tile re-encoding (JPEG → WebP, PNG → AVIF)

## Cross-crate dependencies
- **Blocks:** `oxigeo-services` (tile serving), `oxigeo-pmtiles` (cross-format conversion)
- **Blocked by:** `oxigeo-gpkg` (SQLite reader/writer reuse for High Priority items 1-2)

## Recently completed (kept verbatim)
(No prior `[x]` items in previous TODO.md)

---
*Last audited: 2026-07-28*
