# TODO: oxigdal-rs3gw

## High Priority
- [ ] Implement actual S3 HTTP transport (currently config/struct only)
- [ ] Add COG byte-range request optimization (read only needed tiles)
- [ ] Implement multipart upload for large file writes
- [ ] Add GCS (Google Cloud Storage) backend integration
- [ ] Implement Azure Blob Storage backend
- [ ] Add credential chain resolution (env vars, instance metadata, config files)

## Medium Priority
- [ ] Implement ML-based cache prefetching (predict next tile access)
- [ ] Add content-based deduplication for Zarr chunk storage
- [ ] Implement AES-256-GCM client-side encryption
- [ ] Add retry logic with exponential backoff for transient failures
- [ ] Implement presigned URL generation for temporary access
- [ ] Add bandwidth throttling for metered connections
- [ ] Implement object versioning support (S3 versioning, GCS generations)
- [ ] Add MinIO health check and cluster status monitoring
- [ ] Implement range coalescing for adjacent byte ranges

## Low Priority / Future
- [ ] Add R2 (Cloudflare) storage backend
- [ ] Implement Backblaze B2 storage backend
- [ ] Add object lifecycle management (expiration, transition rules)
- [ ] Implement server-side encryption configuration (SSE-S3, SSE-KMS)
- [ ] Add multi-region replication support
- [ ] Implement storage cost estimation based on access patterns
