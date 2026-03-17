# TODO: oxigdal-cloud

## High Priority
- [ ] Implement actual HTTP transport layer (currently backends are data-model only, no real network I/O)
- [ ] Add multipart upload support for S3/GCS/Azure large objects (>5GB)
- [ ] Implement STS AssumeRole and IMDS credential refresh for S3Backend
- [ ] Add byte-range GET support (Range header) for COG/Zarr partial reads
- [ ] Wire CloudBackend::get/put to real async HTTP client (reqwest or hyper)
- [ ] Add server-side encryption (SSE-S3, SSE-KMS, SSE-C) configuration for S3
- [ ] Implement Azure SAS token generation and Managed Identity auth flow
- [ ] Add GCS signed URL generation with configurable expiry

## Medium Priority
- [ ] Add `delete` and `list` operations to CloudStorageBackend trait
- [ ] Implement `copy` (server-side) across same provider (S3 CopyObject, GCS rewrite)
- [ ] Add cross-cloud transfer (S3 -> GCS, Azure -> S3) via streaming pipe
- [ ] Implement disk-level cache tier with content-addressed storage and TTL eviction
- [ ] Add bandwidth throttling to prefetch module (currently tracks but does not enforce)
- [ ] Implement conditional requests (If-None-Match, If-Modified-Since) for cache validation
- [ ] Add S3 Select / GCS JSON query pass-through for server-side filtering
- [ ] Implement connection pooling and keep-alive for HTTP backend
- [ ] Add retry budget tracking across concurrent requests (global rate limiter)
- [ ] Add MinIO and Cloudflare R2 as S3-compatible endpoint presets

## Low Priority / Future
- [ ] Support S3 Object Lambda for on-the-fly transformations
- [ ] Add OCI (Oracle Cloud) and DigitalOcean Spaces backends
- [ ] Implement S3 Glacier restore workflow (initiate + poll + download)
- [ ] Add cloud storage cost estimation based on operation counts and data volume
- [ ] Implement FUSE-like virtual filesystem interface over cloud backends
- [ ] Add OpenTelemetry tracing spans for all cloud operations
- [ ] Support S3 Access Points and multi-region access points
