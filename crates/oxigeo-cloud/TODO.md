# TODO: oxigeo-cloud

> **Purpose:** Advanced cloud storage backends for OxiGeo - Pure Rust cloud integration (S3/Azure Blob/GCS/HTTP, retry, multi-level cache, prefetch).
> **Status (2026-07-28):** 9,110 Rust LoC · 151 tests · 0 remaining stub sites in Azure/GCS/multicloud dispatch (see "Recently completed"); STS AssumeRole/IMDSv2 credential refresh is the one open High-Priority gap.
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (next slice — verified gaps)
- [x] Wire `AzureBlobBackend::get/put/delete/exists/list` to `azure_storage_blobs` SDK (replace 5 placeholders)
  - **Done (verified 2026-07-28):** `src/backends/azure.rs` (607 LoC) implements all 6 `CloudStorageBackend` methods (`get`, `get_range`, `put`, `delete`, `exists`, `list_prefix`) against the real `azure_storage_blobs` 0.21 SDK (`BlobClient::get_content()`, `put_block_blob()`, etc.), including a documented bridge for the `azure_core` 0.21-vs-1.x `TokenCredential` version mismatch. All "Placeholder for Azure SDK integration" sites are gone.

- [x] Wire `GcsBackend::get/put/delete/exists/list` to `google-cloud-storage` SDK
  - **Done (verified 2026-07-28):** `src/backends/gcs.rs` (629 LoC) implements all 6 `CloudStorageBackend` methods against the real `google_cloud_storage::client::{Storage, StorageControl}` SDK, including ranged reads via `model_ext::ReadRange::segment`. The "placeholder for the actual GCS SDK integration" comment is gone.

- [x] Implement `MultiCloudManager::get_from_provider` real dispatch
  - **Done (verified 2026-07-28):** `src/multicloud.rs` — `build_backend()` matches on `provider.provider` (`AwsS3`/`Gcs`/`Azure`/`Http`) and constructs the real typed backend (feature-gated, `NotSupported` only when the corresponding Cargo feature is off); `resolve_backend()` caches it in `backend_cache: DashMap<...>` (or builds fresh without the `cache` feature); `get_from_provider`/`put_to_provider` call the resolved backend directly — no more placeholder `NotSupported` stub.

- [ ] STS `AssumeRole` + EC2 IMDSv2 credential refresh for `S3Backend`
  - **Verified gap:** `src/auth.rs` exposes `Credentials` but no `Credentials::from_assume_role` / `from_imds` constructors; `S3Backend::create_client` (s3.rs) uses `aws_config::load_defaults` only.
  - **Goal:** First-class STS support — long-running daemons rotate credentials before 15-minute expiry without recreating the client.
  - **Design:** `aws_credential_types::provider::CredentialsCache` wrapping `aws_config::sts::AssumeRoleProvider`. Surface `S3Backend::with_assume_role(arn, session_name, external_id)`. IMDSv2 already provided by SDK; add explicit `S3Backend::with_imds()` and configurable hop limit.
  - **Files:** `src/auth.rs` (new variants), `src/backends/s3.rs` (~80 LoC).
  - **Tests:** (proposed) `test_assume_role_provider_caches_credentials`, `test_imds_v2_with_hop_limit`, `test_credentials_refresh_before_expiry`.
  - **Risk:** SDK type-name churn between `aws-sdk-s3` releases; pin types we use.
  - **Prerequisites:** None.

- [x] Byte-range GET (`Range:` header) on all backends — required by COG/Zarr partial reads
  - **Done (verified 2026-07-28):** `CloudStorageBackend` in `src/backends/mod.rs` declares `async fn get_range(&self, key: &str, range: ByteRange) -> Result<Bytes>`, implemented for all four backends (`s3.rs:433`, `azure.rs:268`, `gcs.rs:277`, `http.rs:365`).

## Medium Priority
- [ ] Disk-tier of `MultiLevelCache` with content-addressed storage + TTL eviction
  - **Goal:** Today the cache (`src/cache/`) is mem-LRU only via `lru`; add an on-disk tier keyed by blake3-hash, with TTL.
  - **Files:** `src/cache/disk.rs` (new), `src/cache/mod.rs`.
  - **Why deferred:** SDK wiring (above) is the blocking prerequisite for real cloud round-trips that justify cache benchmarks.

- [ ] Conditional GETs (`If-None-Match`, `If-Modified-Since`) for cache validation
  - **Goal:** Cache hits validate via 304 instead of re-downloading whole object.
  - **Files:** `src/cache/` + per-backend `get_conditional`.
  - **Why deferred:** Needs metadata storage in cache entries (ETag, Last-Modified).

- [ ] Server-side `copy` within provider (S3 `CopyObject`, Azure `Copy Blob`, GCS `rewriteTo`)
  - **Goal:** Avoid round-trip download for same-provider moves.
  - **Files:** `src/backends/mod.rs` (trait method) + 4 impls.
  - **Why deferred:** Each provider has different async-copy semantics; needs design pass.

- [ ] Cross-cloud streaming transfer (S3 → GCS, Azure → S3) via tokio::io pipe
  - **Goal:** Reuse `MultiCloudManager` to streamed-copy objects across providers without local buffering.
  - **Files:** `src/multicloud.rs` (`transfer()` method); ties to `CrossCloudTransferConfig` already defined.
  - **Why deferred:** Backend wiring must complete first.

- [ ] Connection pooling + keep-alive across requests
  - **Goal:** Reuse one `aws_sdk_s3::Client` / `reqwest::Client` per backend instance (today some paths recreate).
  - **Files:** `src/backends/s3.rs`, `src/backends/http.rs`.
  - **Why deferred:** Minor optimization; correctness items first.

- [ ] Bandwidth throttling enforcement in `prefetch` module
  - **Goal:** `prefetch.rs` tracks bandwidth but does not throttle; wire a `tokio::time::interval`-based limiter.
  - **Files:** `src/prefetch.rs`.
  - **Why deferred:** Useful only after real prefetch traffic exists.

- [ ] MinIO / Cloudflare R2 / Backblaze B2 presets for `S3Backend`
  - **Goal:** Fluent constructors with correct endpoint/path-style defaults.
  - **Files:** `src/backends/s3.rs::S3Backend::for_minio()` / `for_r2()` / `for_b2()`.
  - **Why deferred:** Cosmetic — users can already supply endpoints.

- [ ] Global retry budget tracker (rate limiter shared across concurrent requests)
  - **Goal:** Bound total retries per second across all backends to avoid retry storms.
  - **Files:** `src/retry.rs`.
  - **Why deferred:** Edge case; per-call retry already in place.

## Low Priority / Future (one-liners)
- [ ] S3 Object Lambda pass-through.
- [ ] OCI Object Storage + DigitalOcean Spaces presets.
- [ ] S3 Glacier restore workflow (initiate + poll + download).
- [ ] Cost-estimation helper using AWS/GCP/Azure published rates.
- [ ] FUSE-style virtual FS over `CloudBackend`.
- [ ] OpenTelemetry tracing spans for all cloud operations.
- [ ] S3 Access Points + Multi-Region Access Points.
- [ ] Azure DataLake Gen2 hierarchical-namespace `rename` operation.
- [ ] GCS Pub/Sub notifications integration.

## Cross-crate dependencies
- **Blocks:** oxigeo-streaming (HTTP range reads), oxigeo-geotiff (COG range reads), oxigeo-zarr (chunk range reads), oxigeo-rs3gw (S3 path overlap).
- **Blocked by:** None.

## Recently completed (verbatim)
- [x] Azure Blob (`src/backends/azure.rs`), GCS (`src/backends/gcs.rs`), `MultiCloudManager::get_from_provider` real dispatch, and byte-range GET on all 4 backends — see High Priority section above (verified 2026-07-28; test suite grew from 84 to 151 all-features tests since the 2026-05-16 audit).

---
*Last audited: 2026-07-28*
