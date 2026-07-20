# TODO: oxigeo-cloud

> **Purpose:** Advanced cloud storage backends for OxiGeo - Pure Rust cloud integration (S3/Azure Blob/GCS/HTTP, retry, multi-level cache, prefetch).
> **Status (2026-05-16):** 9,110 Rust LoC · 84 tests · 7 real-stub sites (Azure 5, GCS 1, multicloud 1)
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (next slice — verified gaps)
- [ ] Wire `AzureBlobBackend::get/put/delete/exists/list` to `azure_storage_blobs` SDK (replace 5 placeholders)
  - **Verified gap:** `src/backends/azure.rs:163` — `// Placeholder for Azure SDK integration` (also at lines 187, 210, 229, 246, each followed by `message: "Azure SDK integration pending - requires azure_storage_blobs setup"`)
  - **Goal:** Real Azure Blob I/O via Azure Blob Storage REST API 2023-11-03 (Pure Rust SDK already in `Cargo.toml:58-61`); SAS tokens + account-key + managed-identity auth paths.
  - **Design:** `BlobClient::new(account, container, blob)` from `azure_storage_blobs`. `get_content().into_body()` → `bytes::Bytes`. `put_block_blob(data)` for size ≤ 256 MiB; switch to staged-block-upload (`put_block` + `commit_block_list`) above threshold. Map azure errors via `From<AzureError>`.
  - **Files:** `src/backends/azure.rs` (replace 5 placeholder sites, ~250 LoC); reuse existing `Credentials`/`RetryExecutor` plumbing.
  - **Tests:** (proposed) `test_azure_get_roundtrip` (mock server), `test_azure_put_block_blob`, `test_azure_staged_block_upload_above_threshold`, `test_azure_sas_token_auth`, `test_azure_404_to_not_found`.
  - **Risk:** Azure SDK is async-only; `#[cfg(feature = "async")]` already in place, but error variants may need expansion.
  - **Prerequisites:** None.

- [ ] Wire `GcsBackend::get/put/delete/exists/list` to `google-cloud-storage` SDK
  - **Verified gap:** `src/backends/gcs.rs:117` — `// This is a placeholder for the actual GCS SDK integration` (followed by runtime errors `"GCS SDK integration pending - requires google-cloud-storage setup"`).
  - **Goal:** Real GCS JSON API v1 I/O. Pure-Rust SDK `google-cloud-storage = "1.11"` already pulled in `Cargo.toml:64`.
  - **Design:** `ClientConfig::default().with_auth().await` → `Client::new(cfg)`. `download_object(GetObjectRequest { bucket, object, ... })`. `upload_object(UploadObjectRequest, data, UploadType::Multipart)`. Resumable uploads (`UploadType::Resumable`) above 5 MiB threshold.
  - **Files:** `src/backends/gcs.rs` (~280 LoC).
  - **Tests:** (proposed) `test_gcs_get_roundtrip`, `test_gcs_put_multipart`, `test_gcs_resumable_upload_above_threshold`, `test_gcs_signed_url_v4`, `test_gcs_404_to_not_found`.
  - **Risk:** auth chain (ADC / service account / metadata server) differs by env; cover each in CI matrix.
  - **Prerequisites:** None.

- [ ] Implement `MultiCloudManager::get_from_provider` real dispatch
  - **Verified gap:** `src/multicloud.rs:921` — `// For now, return a placeholder implementation` returning `CloudError::NotSupported`. Method signature already wired through `get_with_failover` at line 928.
  - **Goal:** Construct the right `S3Backend`/`AzureBlobBackend`/`GcsBackend`/`HttpBackend` from `CloudProviderConfig` and call its `get()`, so failover actually transfers bytes.
  - **Design:** Match on `provider.kind`. Build backend from cached configs in `MultiCloudManager::backends: DashMap<String, CloudBackendBox>`; instantiate lazily on first call; reuse across retries.
  - **Files:** `src/multicloud.rs` (~120 new LoC for `backends` field + factory + `put_to_provider` mirror).
  - **Tests:** (proposed) `test_multicloud_get_routes_to_s3`, `test_multicloud_failover_to_secondary`, `test_multicloud_put_routes_to_gcs`, `test_multicloud_backend_cache_reuse`.
  - **Risk:** Backend cache must respect `CloudProvider`'s mutable credential rotation; invalidate on auth refresh.
  - **Prerequisites:** Azure + GCS SDK wiring above (so failover paths actually work).

- [ ] STS `AssumeRole` + EC2 IMDSv2 credential refresh for `S3Backend`
  - **Verified gap:** `src/auth.rs` exposes `Credentials` but no `Credentials::from_assume_role` / `from_imds` constructors; `S3Backend::create_client` (s3.rs) uses `aws_config::load_defaults` only.
  - **Goal:** First-class STS support — long-running daemons rotate credentials before 15-minute expiry without recreating the client.
  - **Design:** `aws_credential_types::provider::CredentialsCache` wrapping `aws_config::sts::AssumeRoleProvider`. Surface `S3Backend::with_assume_role(arn, session_name, external_id)`. IMDSv2 already provided by SDK; add explicit `S3Backend::with_imds()` and configurable hop limit.
  - **Files:** `src/auth.rs` (new variants), `src/backends/s3.rs` (~80 LoC).
  - **Tests:** (proposed) `test_assume_role_provider_caches_credentials`, `test_imds_v2_with_hop_limit`, `test_credentials_refresh_before_expiry`.
  - **Risk:** SDK type-name churn between `aws-sdk-s3` releases; pin types we use.
  - **Prerequisites:** None.

- [ ] Byte-range GET (`Range:` header) on all backends — required by COG/Zarr partial reads
  - **Verified gap:** `CloudStorageBackend` trait in `src/backends/mod.rs` has no `get_range`; S3/Azure/GCS impls all read full object.
  - **Goal:** Add `async fn get_range(&self, key: &str, range: Range<u64>) -> Result<Bytes>` to trait; implement for all four backends.
  - **Design:** S3 → `GetObjectRequest::range("bytes=start-end")`. Azure → `BlobClient::get().range(...)`. GCS → `GetObjectRequest::range(...)` (HTTP `Range` header). HTTP backend → `reqwest::Client::get().header(RANGE, ...)`.
  - **Files:** `src/backends/mod.rs` (trait), `src/backends/{s3,azure,gcs,http}.rs` (~40 LoC each).
  - **Tests:** (proposed) `test_s3_get_range_first_1kb`, `test_azure_get_range_aligned`, `test_gcs_get_range_open_ended`, `test_http_get_range_206`.
  - **Risk:** Range-not-satisfiable (416) must surface as a distinct error variant; oxigeo-geotiff COG reader needs it.
  - **Prerequisites:** Azure + GCS SDK wiring above.

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
- (None — existing TODO.md had no `[x]` items.)

---
*Last audited: 2026-05-16*
