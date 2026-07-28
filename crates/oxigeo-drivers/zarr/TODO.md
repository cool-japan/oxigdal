# TODO: oxigeo-drivers/zarr

> **Purpose:** Zarr v2/v3 driver for OxiGeo - Pure Rust multidimensional array storage
> **Status (2026-07-28):** 19,558 Rust LoC (incl. tests) - 349 tests - 0 real-code stubs remain among the items tracked below; the S3 store and the ZEP-0002 sharded read path from the prior audit are now implemented (see Recently completed).
> **Roadmap:** v0.1.7 - v0.2.0 (current slice) - v1.0.0

## High Priority (next slice - verified gaps)

- [x] Replace placeholder SHA256 with real implementation in checksum transformer
  - Done: 2026-05-31 (Slice 29). Tests: 12 new (crypto_transformers_test) + 243 existing = 255 total.
  - Real FIPS 180-4 SHA-256 via `sha2::Sha256` (RustCrypto). `compute_hash` is now a one-liner; on-disk format unchanged (32-byte append/prepend).

- [x] Replace placeholder XOR cipher with real AES-256-GCM in encryption transformer
  - Done: 2026-05-31 (Slice 29). Tests: included in crypto_transformers_test above.
  - Real NIST SP 800-38D AES-256-GCM via `aes-gcm` (RustCrypto). On-disk frame: `nonce(12)||ct_with_tag`. Nonce from `getrandom::fill` (OS CSPRNG).

- [x] Replace `build_codec_from_metadata` stub that always returns `NullCodec`
  - Done: 2026-05-31 (Slice 28). Tests: 19 new (codec_registry_test) + 224 existing = 243 total.
  - **Verified gap:** `src/sharding.rs:522-527` - `// This is a placeholder - actual implementation would use the codec registry` followed by `Ok(Box::new(NullCodec))`. This means sharded chunks always go through a no-op codec even if metadata names gzip/zstd/blosc.
  - **Goal:** Sharded reads/writes correctly dispatch to gzip/zstd/lz4/blosc/transpose/bytes codecs based on `CodecMetadata`.
  - **Design:** Build a registry keyed on `codec_meta.name` (`"gzip"`, `"zstd"`, `"lz4"`, `"blosc"`, `"transpose"`, `"bytes"`, `"crc32c"`, `"sha256"`, `"null"`). Each arm consults configuration JSON in `codec_meta.configuration` for level / shuffle / endian / etc. Return `Err(ZarrError::UnsupportedCodec { name })` for unknown names. Spec: Zarr v3 ZEP-0001 codec system.
  - **Files:** `src/sharding.rs` (rewrite `build_codec_from_metadata`), reuse existing `src/codecs/` types
  - **Tests:** (proposed) `test_build_codec_gzip_default_level`, `test_build_codec_zstd_with_level`, `test_build_codec_unknown_name_errors`, `test_sharded_read_with_gzip_chunks_roundtrip`
  - **Risk:** Codec configuration JSON shape varies subtly per codec; cross-check against numcodecs spec.
  - **Prerequisites:** None - all needed codec types already in `src/codecs/`.

- [x] Implement Zarr v3 sharding codec read path (`storage_transformers::sharding_indexed`)
  - **Done:** 2026-07-21 (0.2.1 production campaign). `ZarrV3Reader::read_chunk` (`src/reader/v3.rs`) detects a `sharding_indexed` codec via `find_sharding_config()` and delegates to `read_sharded_chunk`, which computes `shard_coords = coords / chunks_per_shard` and `inner_coords = coords % chunks_per_shard`, fetches the shard file, and dispatches through `ShardReader` / `ShardIndexEntry` (`src/sharding.rs`) for real per-codec chunk + index decode via `codecs::dispatch::build_codec_from_metadata` (the codec registry moved from the originally-stubbed spot in `sharding.rs` into its own `src/codecs/dispatch.rs`, shared by the v3 reader, v3 writer, and sharding). `shard_index_encoded_len` derives the footer length from the actual index codec instead of assuming a fixed `n*16` layout, so both length-deterministic index codecs are handled correctly; a compressive index codec is rejected with a typed error rather than silently mis-reading the footer.
  - **Original gap (resolved):** end-to-end sharded array read was not exercised — codec dispatch used to always return `NullCodec` (see codec-dispatch item above).

- [x] Implement S3 store async I/O for cloud Zarr
  - **Done:** by 2026-07-20 (0.2.0 release cycle). `src/storage/s3.rs` (378 lines) implements a real `S3Storage { bucket, prefix, region, endpoint }` backed by `aws-sdk-s3`: `AsyncStore` impl with `get`/`set`/`delete`/`exists` (via `head_object`, treating 404/NotFound as `Ok(false)` rather than an error) and a paginated `list_prefix` (loops on `continuation_token` until `is_truncated != Some(true)`); errors mapped to `StorageError::S3`. Wired at `lib.rs:174-175` as `pub use storage::s3::S3Storage` behind `#[cfg(feature = "s3")]`. 5 unit tests cover construction/builder/key-prefixing; 2 `#[ignore]`d live-S3/MinIO round-trip and list integration tests are gated on `TEST_S3_BUCKET`/`TEST_S3_ENDPOINT` env vars (run via `cargo test --features s3,async -- --ignored`). Concurrent multi-chunk reads (`FuturesUnordered`) from the original design were not added — sequential per-call today — flagged as a possible future optimization, not a correctness gap.
  - **Original gap (resolved):** `src/storage/` used to have no `s3.rs` at all.

## Medium Priority (planned - design sketched)

- [ ] Chunk-level parallel read/write using rayon
  - **Goal:** Read N independent chunks across N rayon threads.
  - **Files:** `src/reader/mod.rs`, `src/writer/mod.rs`
  - **Why deferred:** Sharding correctness first.

- [ ] Blosc codec (numcodecs-compatible: shuffle + zstd / lz4)
  - **Goal:** Read Zarr files that use the very common Blosc compressor.
  - **Files:** `src/codecs/` (new `blosc.rs`)
  - **Why deferred:** Pure-Rust Blosc decoder needed; `blosc-src` is C and feature-gated only.

- [ ] Consolidated metadata reading and writing (`.zmetadata`)
  - **Goal:** Open a Zarr group with a single read from `.zmetadata` instead of N HEAD requests.
  - **Files:** `src/consolidation.rs` (exists but limited)
  - **Why deferred:** Bigger gains realized after S3 backend lands.

- [ ] Dimension coordinate variable convention (xarray-compatible)
  - **Goal:** Auto-detect coordinate arrays linked to named axes.
  - **Files:** `src/dimension.rs`
  - **Why deferred:** Convention; layer above core driver.

- [ ] Slice reading with stride/step (`array[::2, 0:100:5]` semantics)
  - **Goal:** Read sub-sampled views without loading full chunks then discarding.
  - **Files:** `src/reader/mod.rs`
  - **Why deferred:** API design refinement.

- [ ] HTTP range-request store
  - **Goal:** Read remote Zarr over HTTPS (e.g., Cloud Optimized GeoTIFF analogue).
  - **Files:** `src/storage/http.rs` (gated, scaffolded similarly to s3.rs)
  - **Why deferred:** Together with S3; share testing infrastructure.

- [ ] Chunk cache with configurable size and LRU eviction
  - **Goal:** Decoded-chunk cache so repeated reads of overlapping slices hit memory.
  - **Files:** `src/storage/cache.rs` (`CachingStorage` exists; needs decode-side cache layer above)
  - **Why deferred:** Optimization; not correctness.

- [ ] LZ4 frame codec (distinct from existing LZ4 block)
  - **Goal:** Decode `frame`-format LZ4 streams (with checksums) in addition to raw blocks.
  - **Files:** `src/codecs/lz4.rs`
  - **Why deferred:** Frame format rarely used vs block.

- [ ] Group hierarchy traversal and metadata aggregation
  - **Goal:** Walk nested groups and list all arrays.
  - **Files:** `src/reader/mod.rs`
  - **Why deferred:** Use case driven; needed for xarray-like opens.

## Low Priority / Future (speculative - concise)

- [ ] Zarr v3 datetime / string / structured data type extensions
- [ ] Fill value handling for sparse arrays (skip-encode runs of fill)
- [ ] fsspec-compatible storage abstraction
- [ ] Zarr-to-NetCDF/HDF5 conversion tool
- [ ] Kerchunk-compatible reference file generation
- [ ] Zarr virtual store (reference multiple remote chunks)
- [ ] Zarr-based append operations for time-series data
- [ ] Zarr checksum verification (crc32c codec) - parallel to SHA256 work
- [ ] Zarr directory consolidation (combine many small files)
- [ ] Zarr diff tool (compare two arrays chunk-by-chunk)

## Cross-crate dependencies
- **Blocks:** `oxigeo-drivers/netcdf` (Zarr-as-NetCDF), `oxigeo-stac` (some STAC assets are Zarr).
- **Blocked by:** None for core gaps. S3/HTTP backends are independent.

## Recently completed (kept verbatim from previous TODO.md)
_(Previous TODO.md had no `[x]` entries.)_

- [x] Zarr v3 sharding codec read path (ZEP-0002) — `src/reader/v3.rs::read_sharded_chunk`, `src/sharding.rs`, `src/codecs/dispatch.rs`
- [x] S3 storage backend (`aws-sdk-s3`-backed `AsyncStore`) — `src/storage/s3.rs`

---
*Last audited: 2026-07-28*
