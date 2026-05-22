# TODO: oxigdal-drivers/zarr

> **Purpose:** Zarr v2/v3 driver for OxiGDAL - Pure Rust multidimensional array storage
> **Status (2026-05-16):** 17,111 Rust LoC (incl. tests) - 286 tests - 3 real stubs (transformers + sharding codec resolver)
> **Roadmap:** v0.1.5 (current slice) - v0.2.0 - v1.0.0

## High Priority (next slice - verified gaps)

- [ ] Replace placeholder SHA256 with real implementation in checksum transformer
  - **Verified gap:** `src/transformers.rs:210` - `// This is a placeholder - in production, use a proper SHA256 implementation` and `src/transformers.rs:213-215` - `for (i, &byte) in data.iter().enumerate() { hash[i % 32] ^= byte; }`. This is an XOR fold, not a hash.
  - **Goal:** Zarr v3 `c2c` checksum codec produces real FIPS 180-4 SHA-256 digests so integrity verification works against files written by zarr-python / numcodecs.
  - **Design:** Use the `sha2` workspace crate (Pure Rust, RustCrypto). Replace the body of `Sha256Transformer::compute_hash` with `sha2::Sha256::digest(data).into()`. Spec: FIPS PUB 180-4 §6.2. Zarr v3 ZEP-0007 names this codec `sha256`.
  - **Files:** `src/transformers.rs`, `Cargo.toml` (add `sha2 = { workspace = true }` if not already)
  - **Tests:** (proposed) `test_sha256_empty_string_matches_fips_kat`, `test_sha256_known_input_abc`, `test_sha256_append_round_trip`, `test_sha256_prepend_round_trip`, `test_sha256_corruption_detected`
  - **Risk:** Existing zarr files written with the XOR placeholder will fail to verify; this is a breaking change for data written by previous oxigdal-zarr versions, but since the placeholder never matched any other implementation, no real interop is lost. Document in CHANGELOG.
  - **Prerequisites:** None.

- [ ] Replace placeholder XOR cipher with real AES-256-GCM in encryption transformer
  - **Verified gap:** `src/transformers.rs:300` - `// This is a placeholder - in production, use a proper AES-GCM implementation` and `src/transformers.rs:303-307` - XOR loop with comment `// Simple XOR cipher for demonstration (NOT SECURE!)`.
  - **Goal:** `AesGcmTransformer::encrypt`/`decrypt` perform authenticated encryption per NIST SP 800-38D so data-at-rest encryption is actually secure.
  - **Design:** Use `aes-gcm` workspace crate (Pure Rust, RustCrypto, AEAD-compliant). Inputs: 32-byte key (already validated), generate 12-byte nonce via `rand_core::OsRng` for each encrypt, prepend nonce to ciphertext (`nonce || ct_with_tag`). Decrypt: split nonce off, verify 128-bit GCM tag. Spec: NIST SP 800-38D §7-8.
  - **Files:** `src/transformers.rs`, `Cargo.toml` (add `aes-gcm = { workspace = true }` and `rand_core` if needed)
  - **Tests:** (proposed) `test_aesgcm_round_trip_short_payload`, `test_aesgcm_round_trip_large_payload_1mb`, `test_aesgcm_wrong_key_rejected`, `test_aesgcm_tampered_ciphertext_rejected`, `test_aesgcm_nonce_uniqueness_across_encrypts`
  - **Risk:** Same backward-compat concern as SHA256: any encrypted data written with the XOR placeholder is unrecoverable, but it was never secure in the first place.
  - **Prerequisites:** None.

- [ ] Replace `build_codec_from_metadata` stub that always returns `NullCodec`
  - **Verified gap:** `src/sharding.rs:522-527` - `// This is a placeholder - actual implementation would use the codec registry` followed by `Ok(Box::new(NullCodec))`. This means sharded chunks always go through a no-op codec even if metadata names gzip/zstd/blosc.
  - **Goal:** Sharded reads/writes correctly dispatch to gzip/zstd/lz4/blosc/transpose/bytes codecs based on `CodecMetadata`.
  - **Design:** Build a registry keyed on `codec_meta.name` (`"gzip"`, `"zstd"`, `"lz4"`, `"blosc"`, `"transpose"`, `"bytes"`, `"crc32c"`, `"sha256"`, `"null"`). Each arm consults configuration JSON in `codec_meta.configuration` for level / shuffle / endian / etc. Return `Err(ZarrError::UnsupportedCodec { name })` for unknown names. Spec: Zarr v3 ZEP-0001 codec system.
  - **Files:** `src/sharding.rs` (rewrite `build_codec_from_metadata`), reuse existing `src/codecs/` types
  - **Tests:** (proposed) `test_build_codec_gzip_default_level`, `test_build_codec_zstd_with_level`, `test_build_codec_unknown_name_errors`, `test_sharded_read_with_gzip_chunks_roundtrip`
  - **Risk:** Codec configuration JSON shape varies subtly per codec; cross-check against numcodecs spec.
  - **Prerequisites:** None - all needed codec types already in `src/codecs/`.

- [ ] Implement Zarr v3 sharding codec read path (`storage_transformers::sharding_indexed`)
  - **Verified gap:** `src/sharding.rs` defines `ShardIndexEntry`, codec chain assembly, but the codec dispatch is the stub above. End-to-end sharded array read is not exercised.
  - **Goal:** Open and read a Zarr v3 sharded array produced by zarr-python 3.x; chunks-of-chunks layout, shard footer index decoded, individual chunks materialized.
  - **Design:** Per ZEP-0002 sharding: each shard file contains N inner chunks concatenated with a footer index `[offset_0, size_0, ..., offset_{N-1}, size_{N-1}]` of `N * 16` bytes at file end (or beginning, configurable). Read shard footer first; for each requested inner chunk, look up `(offset, size)`; decode inner chunk using chunk-codec chain. `ShardIndexEntry::missing()` sentinel = `(u64::MAX, u64::MAX)` -> return fill value.
  - **Files:** `src/sharding.rs`, `src/reader/v3.rs` (sharding dispatch)
  - **Tests:** (proposed) `test_sharded_read_2x2_inner_chunks`, `test_sharded_read_missing_chunk_returns_fill`, `test_sharded_read_inner_gzip_codec`, `test_sharded_read_footer_at_start_position`
  - **Risk:** ZEP-0002 footer position is configurable (`"end"` vs `"start"`); honour both.
  - **Prerequisites:** `build_codec_from_metadata` item above.

- [ ] Implement S3 store async I/O for cloud Zarr
  - **Verified gap:** `Cargo.toml` declares optional `aws-sdk-s3` / `aws-config` dependencies and `s3` feature flag, but `src/storage/` has no `s3.rs` (verified by `ls`). The `pub use storage::s3::S3Storage` in `src/lib.rs:174` is gated behind `#[cfg(feature = "s3")]`.
  - **Goal:** Open a Zarr group/array stored on S3 (`s3://bucket/prefix/zarr/`) and read chunks asynchronously.
  - **Design:** Implement `Store` trait for `S3Storage { client: aws_sdk_s3::Client, bucket: String, prefix: String }`. `get(key)` -> `GetObjectRequest`; `set(key, val)` -> `PutObject`; `list_prefix(prefix)` -> `ListObjectsV2` paginated. Use `tokio` runtime. Concurrent multi-chunk reads via `futures::stream::FuturesUnordered`.
  - **Files:** (new) `src/storage/s3.rs`, `src/storage/mod.rs` (add `#[cfg(feature = "s3")] pub mod s3;`)
  - **Tests:** (proposed) `test_s3_get_object_round_trip`, `test_s3_list_prefix_pagination`, `test_s3_concurrent_chunk_reads`, `test_s3_404_returns_keynotfound` (use `aws-sdk-s3-mock` or `localstack`)
  - **Risk:** Real S3 integration tests need a mock; gate behind `#[cfg(feature = "s3-mock")]` to avoid network in CI.
  - **Prerequisites:** None.

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
- **Blocks:** `oxigdal-drivers/netcdf` (Zarr-as-NetCDF), `oxigdal-stac` (some STAC assets are Zarr).
- **Blocked by:** None for core gaps. S3/HTTP backends are independent.

## Recently completed (kept verbatim from previous TODO.md)
_(Previous TODO.md had no `[x]` entries.)_

---
*Last audited: 2026-05-16*
